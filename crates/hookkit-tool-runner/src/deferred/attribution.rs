//! Attribute a failing check's output to the files it names.

use super::execution::combined_output;
use crate::command::{PhaseLog, PhaseStatus};
use crate::excerpt::strip_ansi;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// Upper bound on distinct path-like tokens inspected in one output while
/// discovering files other than the candidates. Candidates are found by
/// direct search, so a long output never hides them.
const MAX_PATH_TOKENS: usize = 4096;

/// Which files a check's remaining issues belong to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Attribution {
    /// The output names these candidate files.
    Named(Vec<PathBuf>),
    /// The output names only existing files outside the candidates.
    OutOfScope(Vec<PathBuf>),
    /// The output names no existing file, so no attribution is possible.
    Unnamed,
}

/// Directories a tool's relative output paths may be relative to: the
/// command's working directory and each of its ancestors up to and including
/// the project root, nearest first. A workspace-aware tool (cargo, buf,
/// tflint, ...) prints paths relative to its own workspace or config root,
/// which may sit anywhere in that chain.
pub(crate) fn resolution_bases(working_directory: &Path, project_root: &Path) -> Vec<PathBuf> {
    let mut bases = working_directory
        .ancestors()
        .take_while(|dir| dir.starts_with(project_root))
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    if bases.is_empty() {
        bases.push(working_directory.to_path_buf());
    }
    if !bases.iter().any(|base| base == project_root) {
        bases.push(project_root.to_path_buf());
    }
    bases
}

/// Classify check output by the existing files it mentions.
///
/// Candidates are found by searching the output for each one's absolute path
/// and its path relative to every base, so paths containing spaces and
/// candidates named late in a long output are still attributed. Other files
/// are recognized as whitespace/punctuation-delimited tokens, with any
/// `:line:column` suffix removed, that resolve to an existing file either as
/// absolute paths or relative to `bases` (tried in order). Named candidates
/// are returned as given.
pub(crate) fn attribute<B: AsRef<Path>>(
    output: &str,
    candidates: &BTreeSet<PathBuf>,
    bases: &[B],
) -> Attribution {
    let bases = bases.iter().map(AsRef::as_ref).collect::<Vec<_>>();
    let candidates = candidates
        .iter()
        .map(|path| {
            let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            (canonical, path.clone())
        })
        .collect::<BTreeMap<_, _>>();
    let text = strip_ansi(output);
    let mut named = candidates
        .iter()
        .filter(|(canonical, original)| {
            spellings(canonical, original, &bases)
                .iter()
                .any(|spelling| mentions(&text, spelling))
        })
        .map(|(_, original)| original.clone())
        .collect::<BTreeSet<_>>();
    let mut seen = HashSet::new();
    let mut others = BTreeSet::new();
    for token in text.split(is_delimiter) {
        let path = path_part(token);
        if path.is_empty() || !(path.contains('/') || path.contains('.')) || path.starts_with('-') {
            continue;
        }
        if !seen.insert(path) {
            continue;
        }
        if seen.len() > MAX_PATH_TOKENS {
            break;
        }
        let Some(resolved) = resolve(path, &bases) else {
            continue;
        };
        if let Some(candidate) = candidates.get(&resolved) {
            named.insert(candidate.clone());
        } else {
            others.insert(resolved);
        }
    }
    if !named.is_empty() {
        Attribution::Named(named.into_iter().collect())
    } else if !others.is_empty() {
        Attribution::OutOfScope(others.into_iter().collect())
    } else {
        Attribution::Unnamed
    }
}

/// Candidates that `output` names at a source location: any spelling
/// [`attribute`] recognizes, immediately followed by `:<line>` (and
/// optionally `:<column>`), as in `x.py:3: error: invalid syntax` or
/// ` --> src/lib.rs:3:5`. Returned as given, in order.
pub(crate) fn located_candidates<B: AsRef<Path>>(
    output: &str,
    candidates: &BTreeSet<PathBuf>,
    bases: &[B],
) -> Vec<PathBuf> {
    let bases = bases.iter().map(AsRef::as_ref).collect::<Vec<_>>();
    let text = strip_ansi(output);
    candidates
        .iter()
        .filter(|original| {
            let canonical =
                std::fs::canonicalize(original).unwrap_or_else(|_| original.to_path_buf());
            spellings(&canonical, original, &bases)
                .iter()
                .any(|spelling| mentions_at_location(&text, spelling))
        })
        .cloned()
        .collect()
}

/// The candidates a command that exited with a failure code names at a
/// source location: such a command found a problem in those files (a parser
/// error such as mypy's or `ruff format`'s exit 2), not a problem running, so
/// a check doing this reports issues in them. Empty for a failure naming no
/// candidate at a location (a usage error, a crash, a broken config file),
/// for a spawn error, timeout, or signal, and for any other result.
pub(crate) fn source_failure_files<B: AsRef<Path>>(
    log: &PhaseLog,
    candidates: &BTreeSet<PathBuf>,
    bases: &[B],
) -> Vec<PathBuf> {
    if log.error.is_some()
        || log.status.is_none()
        || log.classification != Some(PhaseStatus::Failure)
    {
        return Vec::new();
    }
    located_candidates(&combined_output(log), candidates, bases)
}

/// Every way a tool is likely to print `candidate`: absolute (as given and
/// canonical) and relative to each base, with `/` separators.
fn spellings(canonical: &Path, original: &Path, bases: &[&Path]) -> BTreeSet<String> {
    let mut spellings = BTreeSet::new();
    for path in [canonical, original] {
        spellings.insert(path.to_string_lossy().into_owned());
        for base in bases {
            let canonical_base = std::fs::canonicalize(base).ok();
            for base in std::iter::once(*base).chain(canonical_base.as_deref()) {
                if let Ok(relative) = path.strip_prefix(base) {
                    if !relative.as_os_str().is_empty() {
                        spellings.insert(relative.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }
    }
    spellings
}

/// Whether `text` mentions `spelling` as a whole path: not as the tail of a
/// longer path or the head of a longer name.
fn mentions(text: &str, spelling: &str) -> bool {
    if spelling.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(offset) = text[from..].find(spelling) {
        let start = from + offset;
        let end = start + spelling.len();
        let before = text[..start].chars().next_back();
        let mut after = text[end..].chars();
        let starts_cleanly = before.is_none_or(|c| is_delimiter(c) || c == ':');
        let ends_cleanly = match after.next() {
            None => true,
            Some(c) if is_delimiter(c) || c == ':' => true,
            Some('.' | '!' | '?') => after.next().is_none_or(is_delimiter),
            Some(_) => false,
        };
        if starts_cleanly && ends_cleanly {
            return true;
        }
        from = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Whether `text` mentions `spelling` as a whole path followed by a
/// `:<line>` location.
fn mentions_at_location(text: &str, spelling: &str) -> bool {
    if spelling.is_empty() {
        return false;
    }
    text.match_indices(spelling).any(|(start, _)| {
        let starts_cleanly = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| is_delimiter(c) || c == ':');
        let mut after = text[start + spelling.len()..].chars();
        starts_cleanly
            && after.next() == Some(':')
            && after.next().is_some_and(|c| c.is_ascii_digit())
    })
}

fn is_delimiter(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '"' | '\''
                | '`'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
                | ','
                | ';'
                | '|'
                | '='
        )
}

/// Strip a `:line[:column]` suffix and trailing sentence punctuation. A
/// Windows drive prefix (`C:\` or `C:/`) is part of the path.
fn path_part(token: &str) -> &str {
    let bytes = token.as_bytes();
    let drive = bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let skip = if drive { 2 } else { 0 };
    let end = token[skip..]
        .find(':')
        .map_or(token.len(), |index| skip + index);
    token[..end].trim_end_matches(['.', '!', '?'])
}

fn resolve(path: &str, bases: &[&Path]) -> Option<PathBuf> {
    let path = Path::new(path);
    let attempts = if path.is_absolute() {
        vec![path.to_path_buf()]
    } else {
        bases.iter().map(|base| base.join(path)).collect()
    };
    attempts
        .into_iter()
        .find(|attempt| attempt.is_file())
        .and_then(|attempt| std::fs::canonicalize(attempt).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("hookkit-attribution-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("src")).unwrap();
            for file in ["src/a.py", "src/b.py", "src/other.py"] {
                std::fs::write(root.join(file), "x\n").unwrap();
            }
            Self(std::fs::canonicalize(root).unwrap())
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.0.join(relative)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn named_candidates_win_over_other_mentions() {
        let tree = Tree::new("named");
        let candidates = BTreeSet::from([tree.path("src/a.py"), tree.path("src/b.py")]);
        let output = format!(
            "\u{1b}[1msrc/a.py\u{1b}[0m:3:1: F821 undefined name\n{}:1:1: E999 (other)\n",
            tree.path("src/other.py").display()
        );
        assert_eq!(
            attribute(&output, &candidates, &[&tree.0]),
            Attribution::Named(vec![tree.path("src/a.py")])
        );
    }

    #[test]
    fn absolute_and_punctuated_mentions_resolve() {
        let tree = Tree::new("absolute");
        let candidates = BTreeSet::from([tree.path("src/a.py"), tree.path("src/b.py")]);
        let output = format!(
            "Would reformat: '{}'. Also `src/b.py`.",
            tree.path("src/a.py").display()
        );
        assert_eq!(
            attribute(&output, &candidates, &[&tree.0]),
            Attribution::Named(vec![tree.path("src/a.py"), tree.path("src/b.py")])
        );
    }

    #[test]
    fn only_other_files_are_out_of_scope() {
        let tree = Tree::new("outside");
        let candidates = BTreeSet::from([tree.path("src/a.py")]);
        let output = "  --> src/other.py:10:5\nerror: unused variable\n";
        assert_eq!(
            attribute(output, &candidates, &[&tree.path("missing-dir"), &tree.0]),
            Attribution::OutOfScope(vec![tree.path("src/other.py")])
        );
    }

    #[test]
    fn paths_with_spaces_are_found_despite_other_mentions() {
        let tree = Tree::new("spaces");
        std::fs::write(tree.path("my file.py"), "x\n").unwrap();
        std::fs::write(tree.path("pyproject.toml"), "x\n").unwrap();
        let candidates = BTreeSet::from([tree.path("my file.py")]);
        let output = format!(
            "Using configuration from {}\n{}:1:1: E100 indentation\n",
            tree.path("pyproject.toml").display(),
            tree.path("my file.py").display()
        );
        assert_eq!(
            attribute(&output, &candidates, &[&tree.0]),
            Attribution::Named(vec![tree.path("my file.py")])
        );
        let relative = "my file.py:1:1: E100 indentation\n";
        assert_eq!(
            attribute(relative, &candidates, &[&tree.0]),
            Attribution::Named(vec![tree.path("my file.py")])
        );
    }

    #[test]
    fn a_candidate_named_after_many_other_paths_is_still_attributed() {
        let tree = Tree::new("late");
        let candidates = BTreeSet::from([tree.path("src/a.py")]);
        let mut output = (0..MAX_PATH_TOKENS + 10)
            .map(|index| format!("pkg.mod{index}.Name: note\n"))
            .collect::<String>();
        output.push_str("src/other.py:1:1: E1 issue\nsrc/a.py:2:1: E2 issue\n");
        assert_eq!(
            attribute(&output, &candidates, &[&tree.0]),
            Attribution::Named(vec![tree.path("src/a.py")])
        );
    }

    #[test]
    fn paths_relative_to_an_enclosing_workspace_resolve() {
        let tree = Tree::new("nested");
        std::fs::create_dir_all(tree.path("rust/a/src")).unwrap();
        std::fs::create_dir_all(tree.path("rust/b/src")).unwrap();
        std::fs::write(tree.path("rust/a/src/lib.rs"), "x\n").unwrap();
        std::fs::write(tree.path("rust/b/src/lib.rs"), "x\n").unwrap();
        let candidates = BTreeSet::from([tree.path("rust/a/src/lib.rs")]);
        let bases = resolution_bases(&tree.path("rust/a"), &tree.0);
        assert_eq!(
            bases,
            vec![tree.path("rust/a"), tree.path("rust"), tree.0.clone()]
        );
        let output = "error: function `f` is never used\n --> b/src/lib.rs:2:4\n";
        assert_eq!(
            attribute(output, &candidates, &bases),
            Attribution::OutOfScope(vec![tree.path("rust/b/src/lib.rs")])
        );
        let named = " --> a/src/lib.rs:2:4\n";
        assert_eq!(
            attribute(named, &candidates, &bases),
            Attribution::Named(vec![tree.path("rust/a/src/lib.rs")])
        );
    }

    #[test]
    fn mentions_require_whole_path_boundaries() {
        assert!(mentions("src/a.py:3:1: x", "src/a.py"));
        assert!(mentions("Would reformat 'src/a.py'.", "src/a.py"));
        assert!(mentions("see src/a.py.", "src/a.py"));
        assert!(!mentions("lib/src/a.py:3", "src/a.py"));
        assert!(!mentions("src/a.py.orig", "src/a.py"));
        assert!(!mentions("data.py", "a.py"));
    }

    #[test]
    fn drive_letters_stay_part_of_the_path() {
        assert_eq!(path_part(r"C:\src\a.py:3:1"), r"C:\src\a.py");
        assert_eq!(path_part("C:/src/a.py:3"), "C:/src/a.py");
        assert_eq!(path_part("src/a.py:3:1:"), "src/a.py");
        assert_eq!(path_part("a.py."), "a.py");
    }

    #[test]
    fn located_candidates_need_a_line_after_the_path() {
        let tree = Tree::new("located");
        let candidates = BTreeSet::from([tree.path("src/a.py"), tree.path("src/b.py")]);
        let bases = [&tree.0];
        // mypy, rustc/Ruff, and absolute spellings name a location.
        let mypy = "src/a.py:3: error: invalid syntax  [syntax]\nFound 1 error";
        assert_eq!(
            located_candidates(mypy, &candidates, &bases),
            vec![tree.path("src/a.py")]
        );
        let ruff = format!(
            "error: Failed to parse {}:3:1: unexpected EOF\n --> src/b.py:1:2\n",
            tree.path("src/a.py").display()
        );
        assert_eq!(
            located_candidates(&ruff, &candidates, &bases),
            vec![tree.path("src/a.py"), tree.path("src/b.py")]
        );
        // A bare mention, another file's location, or a longer path is not.
        for output in [
            "error: cannot read src/a.py",
            "src/a.py: permission denied",
            "src/a.py:: odd",
            "src/other.py:3: error: invalid syntax",
            "lib/src/a.py:3: error",
        ] {
            assert!(
                located_candidates(output, &candidates, &bases).is_empty(),
                "{output}"
            );
        }
    }

    fn failed_log(status: Option<i32>, classification: PhaseStatus, stderr: &str) -> PhaseLog {
        PhaseLog {
            phase: "verify".into(),
            command: "tool".into(),
            program: "tool".into(),
            arguments: Vec::new(),
            status,
            classification: Some(classification),
            stdout: String::new(),
            stderr: stderr.into(),
            error: None,
        }
    }

    #[test]
    fn only_a_located_failure_exit_is_a_source_failure() {
        let tree = Tree::new("source-failure");
        std::fs::write(tree.path("pyproject.toml"), "x\n").unwrap();
        let candidates = BTreeSet::from([tree.path("src/a.py")]);
        let bases = [&tree.0];
        let syntax = "src/a.py:3: error: invalid syntax  [syntax]\n";
        assert_eq!(
            source_failure_files(
                &failed_log(Some(2), PhaseStatus::Failure, syntax),
                &candidates,
                &bases
            ),
            vec![tree.path("src/a.py")]
        );
        // A broken config, even at a location, or a usage error is operational.
        for stderr in [
            "error: bad config pyproject.toml",
            "pyproject.toml:2:1: error: unknown key",
            "usage: mypy [-h]\nmypy: error: unrecognized arguments: --bogus-flag",
        ] {
            let log = failed_log(Some(2), PhaseStatus::Failure, stderr);
            assert!(
                source_failure_files(&log, &candidates, &bases).is_empty(),
                "{stderr}"
            );
        }
        // Only a failure exit qualifies: not issues, a signal, or a spawn error.
        let issues = failed_log(Some(1), PhaseStatus::Issues, syntax);
        assert!(source_failure_files(&issues, &candidates, &bases).is_empty());
        let signal = failed_log(None, PhaseStatus::Failure, syntax);
        assert!(source_failure_files(&signal, &candidates, &bases).is_empty());
        let mut timeout = failed_log(Some(2), PhaseStatus::Failure, syntax);
        timeout.error = Some("timed out".into());
        assert!(source_failure_files(&timeout, &candidates, &bases).is_empty());
    }

    #[test]
    fn output_without_existing_files_is_unnamed() {
        let tree = Tree::new("unnamed");
        let candidates = BTreeSet::from([tree.path("src/a.py")]);
        let output = "Found 2 errors in 0.12s; see https://docs.example/rules.html\nmissing.py:1";
        assert_eq!(
            attribute(output, &candidates, &[&tree.0]),
            Attribution::Unnamed
        );
    }
}
