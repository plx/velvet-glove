//! Project inspection shared by `tools`, `doctor`, and `init`: executable
//! resolution, project file listing, glob matching, and the Pkl version check.

use globset::{Glob, GlobSet, GlobSetBuilder};
use hookkit_pkl_config::schema::{FileSelection, ToolSpec};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Oldest Pkl release Velvet Glove supports.
pub const MIN_PKL_VERSION: (u64, u64, u64) = (0, 31, 1);

/// Upper bound on files listed when inspecting a project.
const MAX_PROJECT_FILES: usize = 100_000;

/// Directories that hold project-local tool installs. Hooks do not search
/// them; they are reported so users know why a tool is "missing".
const PROJECT_BIN_DIRS: &[&str] = &["node_modules/.bin", ".venv/bin", "venv/bin", "vendor/bin"];

/// Directories never worth scanning when `git ls-files` is unavailable.
const WALK_SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".velvet-glove",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
];

/// Where one program resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Found on `PATH` (or an explicit path); hooks can run it.
    Path(PathBuf),
    /// Found only in a project-local bin directory, which hooks do not search.
    ProjectLocal(PathBuf),
    /// Not found anywhere.
    Missing,
}

impl Resolution {
    /// Whether the hook runner can execute this program as configured.
    pub fn runnable(&self) -> bool {
        matches!(self, Self::Path(_))
    }

    /// Short machine-readable status.
    pub fn status(&self) -> &'static str {
        match self {
            Self::Path(_) => "found",
            Self::ProjectLocal(_) => "project-local",
            Self::Missing => "missing",
        }
    }

    /// Resolved path, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) | Self::ProjectLocal(path) => Some(path),
            Self::Missing => None,
        }
    }
}

/// Every program a tool spec invokes: its executable plus any enabled
/// per-command `program` overrides (e.g. php-cs also runs `phpcbf`).
pub fn required_programs(spec: &ToolSpec) -> Vec<String> {
    let mut programs = vec![spec.executable.clone()];
    let phases = spec
        .phases
        .values()
        .filter(|phase| phase.enabled)
        .filter_map(|phase| phase.program.as_ref());
    let workflows = spec
        .workflows
        .values()
        .filter(|workflow| workflow.enabled)
        .flat_map(|workflow| [workflow.check.as_ref(), workflow.remedy.as_ref()])
        .flatten()
        .filter_map(|command| command.program.as_ref());
    for program in phases.chain(workflows) {
        if !programs.contains(program) {
            programs.push(program.clone());
        }
    }
    programs
}

/// Resolve every program a tool needs; the first unrunnable one decides.
pub fn resolve_tool(spec: &ToolSpec, project_dir: &Path) -> (String, Resolution) {
    let mut first = None;
    for program in required_programs(spec) {
        let resolution = resolve_program(&program, project_dir);
        if !resolution.runnable() {
            return (program, resolution);
        }
        first.get_or_insert((program, resolution));
    }
    first.unwrap_or_else(|| (spec.executable.clone(), Resolution::Missing))
}

/// Resolve one program the way the hook runner does (`PATH`, or a path with a
/// separator), then fall back to project-local bin directories for reporting.
pub fn resolve_program(program: &str, project_dir: &Path) -> Resolution {
    if program.is_empty() {
        return Resolution::Missing;
    }
    if program.contains('/') {
        let path = PathBuf::from(program);
        return if is_executable(&path) {
            Resolution::Path(path)
        } else {
            Resolution::Missing
        };
    }
    if let Some(path) = std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
    {
        return Resolution::Path(path);
    }
    PROJECT_BIN_DIRS
        .iter()
        .map(|dir| project_dir.join(dir).join(program))
        .find(|candidate| is_executable(candidate))
        .map_or(Resolution::Missing, Resolution::ProjectLocal)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Installed Pkl version as reported by `pkl --version`.
#[derive(Debug, Clone)]
pub struct PklVersion {
    /// Version text such as `0.32.1`.
    pub version: String,
    /// Whether it satisfies [`MIN_PKL_VERSION`].
    pub supported: bool,
}

/// Run `pkl --version`; `Err` explains why Pkl is unusable.
pub fn pkl_version() -> Result<PklVersion, String> {
    let output = Command::new("pkl")
        .arg("--version")
        .output()
        .map_err(|error| format!("pkl is not runnable ({error})"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout
        .split_whitespace()
        .skip_while(|word| *word != "Pkl")
        .nth(1)
        .ok_or_else(|| format!("unrecognized `pkl --version` output: {}", stdout.trim()))?
        .to_string();
    let supported = parse_version(&version).is_some_and(|parsed| parsed >= MIN_PKL_VERSION);
    Ok(PklVersion { version, supported })
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// Project files as slash-separated paths relative to `root`: `git ls-files`
/// (tracked plus untracked-but-not-ignored) when available, otherwise a
/// bounded walk that skips common build and dependency directories and the
/// simple patterns of the root `.gitignore`. `.velvet-glove/` is excluded.
pub fn list_project_files(root: &Path) -> Vec<String> {
    let mut files = git_files(root).unwrap_or_else(|| walk_files(root));
    files.retain(|file| !file.starts_with(".velvet-glove/"));
    files.truncate(MAX_PROJECT_FILES);
    files.sort();
    files
}

fn git_files(root: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut files: Vec<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8_lossy(entry).into_owned())
        .collect();
    files.dedup();
    Some(files)
}

fn walk_files(root: &Path) -> Vec<String> {
    let ignored = root_gitignore(root);
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            let relative = relative_slash_path(entry.path(), root);
            entry.depth() == 0
                || !(entry.file_type().is_dir() && WALK_SKIP_DIRS.contains(&name.as_ref())
                    || ignored.is_match(&relative))
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| relative_slash_path(entry.path(), root))
        .take(MAX_PROJECT_FILES)
        .collect()
}

/// Approximate the root `.gitignore`: plain patterns only, no negation.
fn root_gitignore(root: &Path) -> GlobSet {
    let text = std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
    let mut patterns = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let anchored = line.starts_with('/') || line.trim_end_matches('/').contains('/');
        let pattern = line.trim_start_matches('/').trim_end_matches('/');
        let base = if anchored {
            pattern.to_string()
        } else {
            format!("**/{pattern}")
        };
        patterns.push(format!("{base}/**"));
        patterns.push(base);
    }
    build_globset(&patterns)
}

fn relative_slash_path(path: &Path, root: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Build a glob set, skipping patterns globset rejects.
pub fn build_globset<S: AsRef<str>>(patterns: &[S]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern.as_ref()) {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

/// File selection with the same semantics as the runner: an empty include
/// list selects every file; excludes (tool plus global) always win.
pub struct FileMatcher {
    include: GlobSet,
    include_all: bool,
    exclude: GlobSet,
}

impl FileMatcher {
    /// Build a matcher from a tool's selection plus global excludes.
    pub fn new(selection: &FileSelection, global_exclude: &[String]) -> Self {
        let exclude: Vec<&String> = global_exclude.iter().chain(&selection.exclude).collect();
        Self {
            include: build_globset(&selection.include),
            include_all: selection.include.is_empty(),
            exclude: build_globset(&exclude),
        }
    }

    /// Whether a project-relative path is selected.
    pub fn matches(&self, relative: &str) -> bool {
        (self.include_all || self.include.is_match(relative)) && !self.exclude.is_match(relative)
    }
}

/// Compact, de-duplicated summary of include globs (`*.py`, `**/*.py` → `*.py`).
pub fn summarize_globs(selection: &FileSelection) -> String {
    if selection.include.is_empty() {
        return "all files".to_string();
    }
    let mut seen: Vec<&str> = Vec::new();
    for glob in &selection.include {
        let short = glob.strip_prefix("**/").unwrap_or(glob);
        if !seen.contains(&short) {
            seen.push(short);
        }
    }
    seen.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("0.31.1"), Some((0, 31, 1)));
        assert_eq!(parse_version("0.32.0-dev+abc"), Some((0, 32, 0)));
        assert!(parse_version("0.100.0").unwrap() > MIN_PKL_VERSION);
        assert!(parse_version("0.31.0").unwrap() < MIN_PKL_VERSION);
        assert_eq!(parse_version("garbage"), None);
    }

    #[test]
    fn glob_summary_drops_recursive_duplicates() {
        let selection = FileSelection {
            include: vec!["*.py".into(), "**/*.py".into(), "*.pyi".into()],
            exclude: Vec::new(),
        };
        assert_eq!(summarize_globs(&selection), "*.py *.pyi");
        assert_eq!(summarize_globs(&FileSelection::default()), "all files");
    }

    #[test]
    fn matcher_applies_global_and_tool_excludes() {
        let selection = FileSelection {
            include: vec!["*.py".into()],
            exclude: vec!["gen/**".into()],
        };
        let matcher = FileMatcher::new(&selection, &["node_modules/**".to_string()]);
        assert!(matcher.matches("src/app.py"));
        assert!(!matcher.matches("gen/app.py"));
        assert!(!matcher.matches("node_modules/x.py"));
        assert!(!matcher.matches("src/app.rs"));
    }
}
