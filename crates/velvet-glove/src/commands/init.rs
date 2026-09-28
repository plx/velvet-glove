//! `velvet-glove init`: detect which builtins fit a project and write a
//! commented starter policy.

use super::project::{
    FileMatcher, Resolution, as_path_refs, build_globset, list_project_files, resolve_search_dirs,
    resolve_tool,
};
use hookkit_pkl_config::schema::{Detect, Settings, ToolSpec};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

/// Project-relative location of the generated policy.
pub const POLICY_PATH: &str = ".velvet-glove/post-tool-use.pkl";

/// Largest file `contains` indicators will read.
const MAX_INDICATOR_BYTES: u64 = 1024 * 1024;

/// Bound on how many same-named files (e.g. every `package.json`) a
/// `contains` check reads for one tool, so a monorepo with many workspaces
/// stays fast.
const MAX_CONTAINS_CANDIDATES: usize = 25;

/// Bound on how many "installed, opt-in" tools `init` suggests, and how
/// broadly one of them may match before it looks like generic hygiene tooling
/// rather than something specific to this project.
const MAX_OPT_IN_SUGGESTIONS: usize = 5;
const OPT_IN_MAX_FILES: usize = 50;

/// Evidence that a project uses a tool: an indicator glob matched a project
/// file, or a `contains` needle turned up inside one.
#[derive(Debug, Clone)]
pub enum Indicator {
    /// An indicator glob matched this project-relative file.
    File(String),
    /// This needle was found inside this project-relative file.
    Contains { needle: String, file: String },
}

impl Indicator {
    /// How this reads in the generated policy's comments: the bare file for
    /// a glob match, or the needle (quoted, since it may be a truncated
    /// bracket like `[tool.ruff`) and the file it was found in.
    fn describe(&self) -> String {
        match self {
            Self::File(file) => file.clone(),
            Self::Contains { needle, file } => format!("{needle:?} in {file}"),
        }
    }
}

/// One builtin whose file globs match something in the project.
#[derive(Debug)]
pub struct Candidate {
    /// Builtins key, e.g. `cargoFmt`.
    pub key: String,
    /// Human-readable tool name.
    pub display_name: String,
    /// Detection metadata (empty when the spec declares none).
    pub detect: Detect,
    /// Number of project files the tool's globs select.
    pub file_count: usize,
    /// One selected file, for the generated comment.
    pub example_file: String,
    /// Evidence that the project uses this tool, if any.
    pub indicator: Option<Indicator>,
    /// The program that decided `resolution`.
    pub program: String,
    /// Where that program resolved.
    pub resolution: Resolution,
    /// Install guidance from the spec.
    pub install_hint: Option<String>,
    /// Whether the project shows it wants this tool (indicator or role default).
    pub wanted: bool,
    /// Whether `init` enables this tool.
    pub selected: bool,
    /// Why it was or was not enabled.
    pub reason: String,
}

/// Write (or print) a starter policy for `dir`.
pub fn run(dir: &Path, print: bool, force: bool) -> ExitCode {
    let target = dir.join(POLICY_PATH);
    if !print && target.exists() && !force {
        eprintln!(
            "velvet-glove init: {} already exists; pass --force to overwrite it or --print to preview.",
            target.display()
        );
        return ExitCode::FAILURE;
    }
    let catalog = match hookkit_pkl_config::builtin_specs() {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("velvet-glove init: cannot load the builtin catalog: {error}");
            return ExitCode::FAILURE;
        }
    };

    let files = list_project_files(dir);
    let candidates = detect(dir, &files, &catalog);
    let source = render(&candidates);

    // Never hand the user a policy the runner cannot load.
    match hookkit_pkl_config::evaluate_pkl_source(&source) {
        Ok(config) if config.run == selected_keys(&candidates) => {}
        Ok(_) => {
            eprintln!("velvet-glove init: internal error: generated run list did not round-trip");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("velvet-glove init: generated policy failed to evaluate: {error}");
            return ExitCode::FAILURE;
        }
    }

    if print {
        print!("{source}");
        return ExitCode::SUCCESS;
    }
    if let Err(error) = target
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&target, &source))
    {
        eprintln!(
            "velvet-glove init: cannot write {}: {error}",
            target.display()
        );
        return ExitCode::FAILURE;
    }

    let selected = selected_keys(&candidates);
    println!("Wrote {}.", target.display());
    if selected.is_empty() {
        println!(
            "No builtin tool was selected, so the hooks will do nothing yet. Add tools from `velvet-glove tools` to the file."
        );
    } else {
        println!("Enabled: {}.", selected.join(", "));
    }
    println!("Next: run `velvet-glove doctor` to check the setup.");
    ExitCode::SUCCESS
}

fn selected_keys(candidates: &[Candidate]) -> Vec<String> {
    candidates
        .iter()
        .filter(|candidate| candidate.selected)
        .map(|candidate| candidate.key.clone())
        .collect()
}

/// Decide which enabled builtins fit the project whose files are `files`.
///
/// A tool is selected when its globs match a project file, its programs
/// resolve the way the hooks resolve them (the default `localBinDirs`,
/// searched from the directories of its own matching files up to the project
/// root, then `PATH`), and either one of its indicators is present anywhere
/// in the project or it is its role's default and no tool in that role has an
/// indicator. A bare indicator glob (no `/`) matches in any directory, and a
/// `contains` check reads every project file with that name, not just the
/// one at the project root, so a nested workspace (`frontend/package.json`)
/// counts the same as a root one.
pub fn detect(
    root: &Path,
    files: &[String],
    catalog: &BTreeMap<String, ToolSpec>,
) -> Vec<Candidate> {
    let Settings {
        exclude: global_exclude,
        local_bin_dirs,
        ..
    } = Settings::default();
    let mut contents = BTreeMap::<String, Option<String>>::new();
    let by_basename = index_by_basename(files);
    let mut candidates: Vec<Candidate> = catalog
        .iter()
        .filter(|(_, spec)| spec.enabled)
        .filter_map(|(key, spec)| {
            let matcher = FileMatcher::new(&spec.files, &global_exclude).ok()?;
            let matching: Vec<&str> = files
                .iter()
                .filter(|file| matcher.matches(file))
                .map(String::as_str)
                .collect();
            let example_file = matching.first()?.to_string();
            let file_count = matching.len();
            let detect = spec.detect.clone().unwrap_or_default();
            let indicator = find_indicator(root, &by_basename, &detect, &mut contents);
            let search_dirs = resolve_search_dirs(root, &matching);
            let (program, resolution) =
                resolve_tool(spec, root, &local_bin_dirs, &as_path_refs(&search_dirs));
            Some(Candidate {
                key: key.clone(),
                display_name: spec.display_name.clone(),
                detect,
                file_count,
                example_file,
                indicator,
                program,
                resolution,
                install_hint: spec.install_hint.clone(),
                wanted: false,
                selected: false,
                reason: String::new(),
            })
        })
        .collect();

    let configured_roles: BTreeSet<String> = candidates
        .iter()
        .filter(|candidate| candidate.indicator.is_some())
        .filter_map(|candidate| candidate.detect.role.clone())
        .collect();
    for candidate in &mut candidates {
        let role_open = candidate
            .detect
            .role
            .as_ref()
            .is_none_or(|role| !configured_roles.contains(role));
        candidate.wanted = candidate.indicator.is_some() || (candidate.detect.default && role_open);
        candidate.selected = candidate.wanted && candidate.resolution.runnable();
        if candidate.wanted {
            candidate.reason = match &candidate.indicator {
                Some(indicator) => format!("found {}", indicator.describe()),
                None => format!(
                    "default {} tool",
                    candidate.detect.role.as_deref().unwrap_or("project")
                ),
            };
        }
    }

    let selected_roles: BTreeMap<String, String> = candidates
        .iter()
        .filter(|candidate| candidate.selected)
        .filter_map(|candidate| Some((candidate.detect.role.clone()?, candidate.key.clone())))
        .collect();
    for candidate in candidates
        .iter_mut()
        .filter(|candidate| !candidate.selected)
    {
        candidate.reason = unselected_reason(candidate, &selected_roles);
    }
    candidates
}

fn unselected_reason(candidate: &Candidate, selected_roles: &BTreeMap<String, String>) -> String {
    let role = candidate.detect.role.as_deref();
    match &candidate.resolution {
        Resolution::Unconfigured(path) => {
            return format!(
                "{} is only at {}; add that directory to settings.localBinDirs",
                candidate.program,
                path.display()
            );
        }
        Resolution::Missing => {
            let hint = candidate
                .install_hint
                .clone()
                .unwrap_or_else(|| format!("install {}", candidate.program));
            return format!(
                "{} not found on PATH or in project-local bin directories; {hint}",
                candidate.program
            );
        }
        Resolution::Path(_) | Resolution::ProjectLocal(_) => {}
    }
    if let Some(chosen) = role.and_then(|role| selected_roles.get(role)) {
        return format!("alternative to {chosen} for {}", role.unwrap_or_default());
    }
    // Installed, matches project files, but opt-in: explain what would turn
    // it on, so "not installed" is never claimed for a tool that plainly is.
    if let Some(note) = &candidate.detect.note {
        return note.clone();
    }
    if !candidate.detect.indicators.is_empty() {
        return format!("add {} to enable", candidate.detect.indicators.join(" or "));
    }
    if let Some((file, needle)) = candidate.detect.contains.iter().next() {
        return format!("add {needle:?} to {file} to enable");
    }
    "installed, opt-in; add its key to `run` to enable".to_string()
}

/// Index project files by basename, nearest-to-root first and bounded, so a
/// `contains` check can look at every same-named file (every `package.json`,
/// not just the root one) without walking the project again.
fn index_by_basename(files: &[String]) -> BTreeMap<&str, Vec<&str>> {
    let mut by_basename: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for file in files {
        if let Some(name) = Path::new(file).file_name().and_then(|name| name.to_str()) {
            by_basename.entry(name).or_default().push(file.as_str());
        }
    }
    for matches in by_basename.values_mut() {
        matches.sort_by_key(|file| file.matches('/').count());
        matches.truncate(MAX_CONTAINS_CANDIDATES);
    }
    by_basename
}

/// A bare indicator glob (no `/`) also matches nested files, e.g. a
/// `frontend/eslint.config.mjs` counts as evidence for `eslint.config.*`.
fn indicator_globset(patterns: &[String]) -> globset::GlobSet {
    let mut expanded: Vec<String> = Vec::with_capacity(patterns.len() * 2);
    for pattern in patterns {
        expanded.push(pattern.clone());
        if !pattern.contains('/') {
            expanded.push(format!("**/{pattern}"));
        }
    }
    build_globset(&expanded)
}

fn find_indicator(
    root: &Path,
    by_basename: &BTreeMap<&str, Vec<&str>>,
    detect: &Detect,
    contents: &mut BTreeMap<String, Option<String>>,
) -> Option<Indicator> {
    let indicators = indicator_globset(&detect.indicators);
    let mut matching_files: Vec<&str> = by_basename
        .values()
        .flatten()
        .copied()
        .filter(|file| indicators.is_match(*file))
        .collect();
    matching_files.sort_by_key(|file| file.matches('/').count());
    if let Some(file) = matching_files.first() {
        return Some(Indicator::File((*file).to_string()));
    }
    detect.contains.iter().find_map(|(name, needle)| {
        by_basename
            .get(name.as_str())
            .into_iter()
            .flatten()
            .find_map(|file| {
                let text = contents
                    .entry((*file).to_string())
                    .or_insert_with(|| read_small(&root.join(file)));
                text.as_deref()
                    .is_some_and(|text| text.contains(needle.as_str()))
                    .then(|| Indicator::Contains {
                        needle: needle.clone(),
                        file: (*file).to_string(),
                    })
            })
    })
}

fn read_small(path: &Path) -> Option<String> {
    let metadata = path.metadata().ok()?;
    (metadata.is_file() && metadata.len() <= MAX_INDICATOR_BYTES)
        .then(|| std::fs::read_to_string(path).ok())
        .flatten()
}

/// One labeled, commented-out group of unselected tools in the generated
/// policy, e.g. "wanted but not installed".
struct OptionGroup<'a> {
    heading: &'a str,
    candidates: Vec<&'a Candidate>,
}

/// Render the generated policy. Unselected tools appear as commented-out
/// options, grouped by why they are not enabled: the project wants them but
/// they (or a program they need) are not installed; they are an installed
/// alternative already covered by a selected tool in the same role; or they
/// are installed and match project files but are opt-in and undetected.
pub fn render(candidates: &[Candidate]) -> String {
    let selected: Vec<&Candidate> = candidates.iter().filter(|c| c.selected).collect();
    let selected_roles: BTreeSet<&str> = selected
        .iter()
        .filter_map(|c| c.detect.role.as_deref())
        .collect();
    let is_alternative = |c: &&Candidate| {
        c.resolution.runnable()
            && c.detect
                .role
                .as_deref()
                .is_some_and(|role| selected_roles.contains(role))
    };

    let wanted_missing: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| !c.selected && c.wanted)
        .collect();
    let alternatives: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| !c.selected && !c.wanted && is_alternative(c))
        .collect();
    let mut opt_in: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| !c.selected && !c.wanted && !is_alternative(c) && c.resolution.runnable())
        .filter(|c| c.file_count <= OPT_IN_MAX_FILES)
        .collect();
    opt_in.sort_by_key(|c| (c.file_count, c.key.as_str()));
    opt_in.truncate(MAX_OPT_IN_SUGGESTIONS);

    let mut out = String::new();
    out.push_str(HEADER);
    out.push_str("tools {\n");
    for candidate in &selected {
        let _ = writeln!(
            out,
            "  // {}: {}; {} matching file{} (e.g. {}).",
            candidate.display_name,
            pkl_comment(&candidate.reason),
            candidate.file_count,
            if candidate.file_count == 1 { "" } else { "s" },
            pkl_comment(&candidate.example_file),
        );
        let _ = writeln!(out, "  [\"{0}\"] = Builtins.{0}", candidate.key);
    }
    if selected.is_empty() {
        out.push_str(
            "  // No builtin tool fits this project automatically; see `velvet-glove tools`.\n",
        );
    }
    for group in [
        OptionGroup {
            heading: "Wanted but not installed. Install the tool (or point settings.localBinDirs\n  // at it), then uncomment and add its key to `run`.",
            candidates: wanted_missing,
        },
        OptionGroup {
            heading: "Installed alternatives not enabled: this project already has another tool\n  // for the same role. Swap by uncommenting one of these and removing the\n  // selected tool above (and in `run`).",
            candidates: alternatives,
        },
        OptionGroup {
            heading: "Installed, opt-in: on PATH and match project files, but need an explicit\n  // choice (see the reason) rather than being auto-enabled.",
            candidates: opt_in,
        },
    ] {
        if group.candidates.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n  // {}", group.heading);
        for candidate in &group.candidates {
            let _ = writeln!(
                out,
                "  // [\"{0}\"] = Builtins.{0}  // {1}",
                candidate.key,
                pkl_comment(&candidate.reason)
            );
        }
    }
    out.push_str("}\n\n// Tools run in this order.\nrun {\n");
    for candidate in &selected {
        let _ = writeln!(out, "  \"{}\"", candidate.key);
    }
    out.push_str("}\n");
    out.push_str(RECIPES);
    out
}

fn pkl_comment(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

const HEADER: &str = r#"// Velvet Glove policy for this project, generated by `velvet-glove init`.
//
// The hooks run the tools in `run` on files the coding agent edits. Clean
// files stay silent, automatic fixes get a one-line notice, and only issues
// that need a manual fix are reported back to the agent.
//
//   velvet-glove doctor         check this setup
//   velvet-glove check [FILES]  run the tools on changed (or named) files now
//   velvet-glove tools          list every builtin tool and its KEY
//   velvet-glove init --force   regenerate this file (discards your edits)
//
// Personal overrides belong in .velvet-glove/post-tool-use.local.pkl, which is
// merged after this file. Reference:
// https://github.com/plx/velvet-glove/blob/main/docs/configuration.md
amends "Config.pkl"

import "Builtins.pkl"

"#;

const RECIPES: &str = r#"
// Recipes: replace a tool's entry inside `tools` above with an amended copy
// (more in docs/configuration.md).
//
// Ignore a rule only when the hook runs a tool, e.g. keep ruff from deleting
// unused imports while the agent is mid-edit. Give every ruff command the same
// arguments so the check and the fix agree:
//
//   local hookOnly = new Listing<String> { "--ignore"; "F401" }
//   ["ruff"] = (Builtins.ruff) {
//     phases { ["fix"] { extraArgs = hookOnly }; ["verify"] { extraArgs = hookOnly } }
//     workflows {
//       ["lint"] { check { extraArgs = hookOnly }; remedy { extraArgs = hookOnly } }
//     }
//   }
//
// Keep a tool away from some paths, e.g. generated code:
//
//   ["ruff"] = (Builtins.ruff) { files { exclude { "generated/**" } } }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use hookkit_pkl_config::schema::FileSelection;

    fn spec(executable: &str, include: &str, detect: Detect) -> ToolSpec {
        ToolSpec {
            id: executable.to_string(),
            display_name: executable.to_string(),
            executable: executable.to_string(),
            files: FileSelection {
                include: vec![include.to_string()],
                exclude: Vec::new(),
            },
            detect: Some(detect),
            ..ToolSpec::default()
        }
    }

    fn role(role: &str, default: bool, indicators: &[&str]) -> Detect {
        Detect {
            indicators: indicators.iter().map(ToString::to_string).collect(),
            role: Some(role.to_string()),
            default,
            ..Detect::default()
        }
    }

    fn selected(candidates: &[Candidate]) -> Vec<&str> {
        candidates
            .iter()
            .filter(|c| c.selected)
            .map(|c| c.key.as_str())
            .collect()
    }

    const PRESENT: &str = "/bin/sh";
    const ABSENT: &str = "/nonexistent/velvet-glove-test-tool";

    #[test]
    fn role_default_applies_only_when_no_role_member_is_configured() {
        let catalog = BTreeMap::from([
            (
                "fmtA".to_string(),
                spec(PRESENT, "*.py", role("py-format", true, &[])),
            ),
            (
                "fmtB".to_string(),
                spec(PRESENT, "*.py", role("py-format", false, &["b.toml"])),
            ),
            (
                "lint".to_string(),
                spec(PRESENT, "*.py", role("py-lint", true, &[])),
            ),
            (
                "other".to_string(),
                spec(PRESENT, "*.rs", role("rs", true, &[])),
            ),
        ]);
        let root = Path::new("/nonexistent");

        let plain = detect(root, &["app.py".to_string()], &catalog);
        assert_eq!(selected(&plain), ["fmtA", "lint"]);

        let files = ["app.py".to_string(), "b.toml".to_string()];
        let configured = detect(root, &files, &catalog);
        assert_eq!(selected(&configured), ["fmtB", "lint"]);
        let fmt_a = configured.iter().find(|c| c.key == "fmtA").unwrap();
        assert_eq!(fmt_a.reason, "alternative to fmtB for py-format");
    }

    #[test]
    fn wanted_tools_that_are_not_installed_are_listed_but_not_selected() {
        let catalog = BTreeMap::from([
            (
                "missing".to_string(),
                spec(ABSENT, "*.js", role("js", false, &[".cfg"])),
            ),
            (
                "optIn".to_string(),
                spec(PRESENT, "*.js", Detect::default()),
            ),
        ]);
        let files = ["a.js".to_string(), ".cfg".to_string()];
        let candidates = detect(Path::new("/nonexistent"), &files, &catalog);
        assert!(selected(&candidates).is_empty());
        let source = render(&candidates);
        assert!(source.contains("Wanted but not installed"), "{source}");
        assert!(source.contains("// [\"missing\"] = Builtins.missing  // "));
        // Installed and matches a project file, but declares no detect signal
        // at all: still surfaced as an opt-in suggestion, not silently
        // dropped (the old "not installed" header would have been wrong for
        // it anyway, since it *is* installed).
        assert!(source.contains("Installed, opt-in"), "{source}");
        assert!(
            source.contains("// [\"optIn\"] = Builtins.optIn  // installed, opt-in"),
            "{source}"
        );
        assert!(source.contains("run {\n}\n"));
    }

    #[test]
    fn contains_indicators_read_the_named_file() {
        let root = std::env::temp_dir().join(format!("vg-init-contains-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("pyproject.toml"),
            "[tool.black]\nline-length = 100\n",
        )
        .unwrap();
        let detect_black = Detect {
            contains: BTreeMap::from([("pyproject.toml".into(), "[tool.black]".into())]),
            ..Detect::default()
        };
        let catalog = BTreeMap::from([("black".to_string(), spec(PRESENT, "*.py", detect_black))]);
        let files = ["a.py".to_string(), "pyproject.toml".to_string()];
        let candidates = detect(&root, &files, &catalog);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(selected(&candidates), ["black"]);
        // The needle is quoted so a truncated-looking bracket like
        // `[tool.ruff` reads clearly as a substring, not a typo.
        assert_eq!(
            candidates[0].reason,
            "found \"[tool.black]\" in pyproject.toml"
        );
    }

    #[test]
    fn contains_indicators_check_every_same_named_file() {
        let root =
            std::env::temp_dir().join(format!("vg-init-nested-contains-{}", std::process::id()));
        std::fs::create_dir_all(root.join("backend")).unwrap();
        std::fs::write(root.join("pyproject.toml"), "[project]\nname = \"demo\"\n").unwrap();
        std::fs::write(
            root.join("backend/pyproject.toml"),
            "[tool.ruff.lint]\nselect = [\"E\"]\n",
        )
        .unwrap();
        let detect_ruff = Detect {
            contains: BTreeMap::from([("pyproject.toml".into(), "[tool.ruff".into())]),
            ..Detect::default()
        };
        let catalog = BTreeMap::from([("ruff".to_string(), spec(PRESENT, "*.py", detect_ruff))]);
        let files = [
            "app.py".to_string(),
            "pyproject.toml".to_string(),
            "backend/pyproject.toml".to_string(),
        ];
        let candidates = detect(&root, &files, &catalog);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(selected(&candidates), ["ruff"]);
        assert_eq!(
            candidates[0].reason,
            "found \"[tool.ruff\" in backend/pyproject.toml"
        );
    }

    #[test]
    fn documented_recipes_evaluate_when_uncommented() {
        if std::process::Command::new("pkl")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: pkl is not on PATH");
            return;
        }
        let body: String = RECIPES
            .lines()
            .filter_map(|line| line.strip_prefix("//   "))
            .map(|line| format!("{line}\n"))
            .collect();
        // Both recipes replace ruff; exercise each on its own.
        let (first, second) = body
            .split_once("[\"ruff\"] = (Builtins.ruff) { files")
            .expect("two recipes");
        for recipe in [
            first.to_string(),
            format!("[\"ruff\"] = (Builtins.ruff) {{ files{second}"),
        ] {
            let source = format!("{HEADER}tools {{\n{recipe}}}\nrun {{ \"ruff\" }}\n");
            let config = hookkit_pkl_config::evaluate_pkl_source(&source)
                .unwrap_or_else(|error| panic!("recipe must evaluate: {error}\n{source}"));
            assert_eq!(config.run, ["ruff"]);
        }
        let source = format!("{HEADER}tools {{\n{first}}}\nrun {{ \"ruff\" }}\n");
        let config = hookkit_pkl_config::evaluate_pkl_source(&source).unwrap();
        let ruff = &config.tools["ruff"];
        assert_eq!(ruff.phases["verify"].extra_args, ["--ignore", "F401"]);
        let lint_check = ruff.workflows["lint"].check.as_ref().unwrap();
        assert_eq!(lint_check.extra_args, ["--ignore", "F401"]);
    }
}
