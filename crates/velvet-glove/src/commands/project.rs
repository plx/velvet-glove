//! Project inspection shared by `tools`, `doctor`, and `init`: executable
//! resolution, project file listing, glob matching, and the Pkl version check.

use globset::{Glob, GlobSet, GlobSetBuilder};
use hookkit_pkl_config::schema::{FileSelection, TOOL_CACHE_DIRECTORIES, ToolSpec};
use hookkit_tool_runner::is_executable_file;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Oldest Pkl release Velvet Glove supports.
pub const MIN_PKL_VERSION: (u64, u64, u64) = (0, 31, 1);

/// Upper bound on files listed when inspecting a project.
const MAX_PROJECT_FILES: usize = 100_000;

/// Other conventional project-local install directories. Hooks search them
/// only when `settings.localBinDirs` names them; a tool found only there is
/// reported so users know why the hooks cannot run it.
const OTHER_PROJECT_BIN_DIRS: &[&str] =
    &["node_modules/.bin", ".venv/bin", "venv/bin", "vendor/bin"];

/// Directories never worth scanning when `git ls-files` is unavailable, in
/// addition to the tool caches in [`TOOL_CACHE_DIRECTORIES`].
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
    /// Found in one of `settings.localBinDirs` at the project root; hooks
    /// prefer it over `PATH`.
    ProjectLocal(PathBuf),
    /// Found only in a conventional project-local directory that
    /// `settings.localBinDirs` does not name, so hooks cannot run it.
    Unconfigured(PathBuf),
    /// Not found anywhere.
    Missing,
}

impl Resolution {
    /// Whether the hook runner can execute this program as configured.
    pub fn runnable(&self) -> bool {
        matches!(self, Self::Path(_) | Self::ProjectLocal(_))
    }

    /// Short machine-readable status.
    pub fn status(&self) -> &'static str {
        match self {
            Self::Path(_) => "found",
            Self::ProjectLocal(_) => "project-local",
            Self::Unconfigured(_) => "not-in-local-bin-dirs",
            Self::Missing => "missing",
        }
    }

    /// Resolved path, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) | Self::ProjectLocal(path) | Self::Unconfigured(path) => Some(path),
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
pub fn resolve_tool(
    spec: &ToolSpec,
    project_dir: &Path,
    local_bin_dirs: &[String],
) -> (String, Resolution) {
    let mut first = None;
    for program in required_programs(spec) {
        let resolution = resolve_program(&program, project_dir, local_bin_dirs);
        if !resolution.runnable() {
            return (program, resolution);
        }
        first.get_or_insert((program, resolution));
    }
    first.unwrap_or_else(|| (spec.executable.clone(), Resolution::Missing))
}

/// Resolve one program the way the hook runner does for a file at the project
/// root, with the runner's own resolver: a path with a separator relative to
/// the project root (where the hooks run it for a tool without a workspace
/// indicator); a bare name in each `local_bin_dirs` entry, then on `PATH`.
/// (At run time the runner also searches from each file's directory and
/// nested workspaces, nearest first.) Other conventional project-local
/// directories are checked last, for reporting only.
pub fn resolve_program(program: &str, project_dir: &Path, local_bin_dirs: &[String]) -> Resolution {
    if program.is_empty() {
        return Resolution::Missing;
    }
    if Path::new(program).components().count() != 1 {
        let path = project_dir.join(program);
        return if is_executable_file(&path) {
            Resolution::Path(path)
        } else {
            Resolution::Missing
        };
    }
    if let Some(path) =
        hookkit_tool_runner::local_program(program, &[project_dir], project_dir, local_bin_dirs)
    {
        return Resolution::ProjectLocal(path);
    }
    if let Some(path) = std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable_file(candidate))
    {
        return Resolution::Path(path);
    }
    OTHER_PROJECT_BIN_DIRS
        .iter()
        .filter(|dir| !local_bin_dirs.iter().any(|configured| configured == *dir))
        .map(|dir| project_dir.join(dir).join(program))
        .find(|candidate| is_executable_file(candidate))
        .map_or(Resolution::Missing, Resolution::Unconfigured)
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

/// Whether a slash-separated relative path lies in a `.velvet-glove/`
/// policy directory, which is never a lint candidate.
pub fn in_policy_directory(relative: &str) -> bool {
    relative.split('/').any(|part| part == ".velvet-glove")
}

/// Project files as slash-separated paths relative to `root`: `git ls-files`
/// (tracked plus untracked-but-not-ignored) when available, otherwise a
/// bounded walk that skips common build and dependency directories and the
/// simple patterns of the root `.gitignore`. `.velvet-glove/` is excluded.
pub fn list_project_files(root: &Path) -> Vec<String> {
    let mut files = git_files(root).unwrap_or_else(|| walk_files(root));
    files.retain(|file| !in_policy_directory(file));
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
            let skipped = WALK_SKIP_DIRS.contains(&name.as_ref())
                || TOOL_CACHE_DIRECTORIES.contains(&name.as_ref());
            entry.depth() == 0
                || !(entry.file_type().is_dir() && skipped || ignored.is_match(&relative))
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

/// File selection through the runner's own matcher: an empty include list
/// selects every file; excludes (global plus tool) always win.
pub struct FileMatcher(hookkit_tool_runner::FileMatcher);

impl FileMatcher {
    /// Build a matcher from a tool's selection plus global excludes; `Err`
    /// describes an invalid glob.
    pub fn new(selection: &FileSelection, global_exclude: &[String]) -> Result<Self, String> {
        let selection = hookkit_tool_runner::FileSelection::include(selection.include.iter())
            .with_exclude(global_exclude.iter().chain(&selection.exclude));
        hookkit_tool_runner::FileMatcher::new(&selection)
            .map(Self)
            .map_err(|error| error.to_string())
    }

    /// Whether a project-relative path is selected.
    pub fn matches(&self, relative: &str) -> bool {
        self.0.matches_relative(relative)
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

    #[cfg(unix)]
    #[test]
    fn configured_local_bins_are_runnable_and_others_are_only_reported() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("vg-resolve-{}", std::process::id()));
        for dir in ["node_modules/.bin", "venv/bin"] {
            let bin = root.join(dir);
            std::fs::create_dir_all(&bin).unwrap();
            let tool = bin.join("vg-test-tool");
            std::fs::write(&tool, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let defaults = hookkit_pkl_config::schema::default_local_bin_dirs();

        let local = resolve_program("vg-test-tool", &root, &defaults);
        assert_eq!(
            local,
            Resolution::ProjectLocal(root.join("node_modules/.bin/vg-test-tool"))
        );
        assert!(local.runnable());

        let unconfigured = resolve_program("vg-test-tool", &root, &[".venv/bin".to_string()]);
        assert_eq!(
            unconfigured,
            Resolution::Unconfigured(root.join("node_modules/.bin/vg-test-tool"))
        );
        assert!(!unconfigured.runnable());
        let venv = resolve_program("vg-test-tool", &root, &["venv/bin".to_string()]);
        assert_eq!(
            venv,
            Resolution::ProjectLocal(root.join("venv/bin/vg-test-tool"))
        );
        assert_eq!(
            resolve_program("vg-absent-tool", &root, &defaults),
            Resolution::Missing
        );
        std::fs::remove_dir_all(&root).unwrap();
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
        let matcher = FileMatcher::new(&selection, &["node_modules/**".to_string()]).unwrap();
        assert!(matcher.matches("src/app.py"));
        assert!(!matcher.matches("gen/app.py"));
        assert!(!matcher.matches("node_modules/x.py"));
        assert!(!matcher.matches("src/app.rs"));
        let invalid = FileSelection {
            include: vec!["src/{a".into()],
            exclude: Vec::new(),
        };
        assert!(FileMatcher::new(&invalid, &[]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn program_paths_resolve_from_the_project_root_not_the_process_cwd() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("vg-resolve-path-{}", std::process::id()));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let tool = root.join("bin/mylint");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(
            resolve_program("bin/mylint", &root, &[]),
            Resolution::Path(tool.clone())
        );
        assert_eq!(
            resolve_program(&tool.to_string_lossy(), Path::new("/"), &[]),
            Resolution::Path(tool)
        );
        assert_eq!(
            resolve_program("bin/absent", &root, &[]),
            Resolution::Missing
        );
        assert!(in_policy_directory(".velvet-glove/post-tool-use.pkl"));
        assert!(in_policy_directory(
            "sub/.velvet-glove/post-tool-use.local.pkl"
        ));
        assert!(!in_policy_directory("src/velvet-glove.rs"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
