//! Attribute a failing check's output to the files it names.

use crate::excerpt::strip_ansi;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// Upper bound on distinct path-like tokens inspected in one output.
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

/// Classify check output by the existing files it mentions.
///
/// Paths are recognized as whitespace/punctuation-delimited tokens, with any
/// `:line:column` suffix removed, that resolve to an existing file either as
/// absolute paths or relative to `bases` (the command's working directory and
/// the project root). Named candidates are returned as given.
pub(crate) fn attribute(
    output: &str,
    candidates: &BTreeSet<PathBuf>,
    bases: &[&Path],
) -> Attribution {
    let candidates = candidates
        .iter()
        .map(|path| {
            let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            (canonical, path.clone())
        })
        .collect::<BTreeMap<_, _>>();
    let text = strip_ansi(output);
    let mut seen = HashSet::new();
    let mut named = BTreeSet::new();
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
        let Some(resolved) = resolve(path, bases) else {
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

/// Strip a `:line[:column]` suffix and trailing sentence punctuation.
fn path_part(token: &str) -> &str {
    let token = token.split(':').next().unwrap_or_default();
    token.trim_end_matches(['.', '!', '?'])
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
