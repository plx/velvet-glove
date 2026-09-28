//! `velvet-glove init`: detect which builtins fit a project and write a
//! commented starter policy.

use super::project::{FileMatcher, Resolution, build_globset, list_project_files, resolve_tool};
use hookkit_pkl_config::schema::{Detect, Settings, ToolSpec};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

/// Project-relative location of the generated policy.
pub const POLICY_PATH: &str = ".velvet-glove/post-tool-use.pkl";

/// Largest file `contains` indicators will read.
const MAX_INDICATOR_BYTES: u64 = 1024 * 1024;

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
    pub indicator: Option<String>,
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
/// resolve the way the hooks resolve them (the default `localBinDirs` at the
/// project root, then `PATH`), and either one of its indicators is present or
/// it is its role's default and no tool in that role has an indicator.
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
    let mut candidates: Vec<Candidate> = catalog
        .iter()
        .filter(|(_, spec)| spec.enabled)
        .filter_map(|(key, spec)| {
            let matcher = FileMatcher::new(&spec.files, &global_exclude);
            let mut matching = files.iter().filter(|file| matcher.matches(file));
            let example_file = matching.next()?.clone();
            let file_count = 1 + matching.count();
            let detect = spec.detect.clone().unwrap_or_default();
            let indicator = find_indicator(root, files, &detect, &mut contents);
            let (program, resolution) = resolve_tool(spec, root, &local_bin_dirs);
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
                Some(indicator) => format!("found {indicator}"),
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
    candidate
        .detect
        .note
        .clone()
        .unwrap_or_else(|| "not selected automatically".to_string())
}

fn find_indicator(
    root: &Path,
    files: &[String],
    detect: &Detect,
    contents: &mut BTreeMap<String, Option<String>>,
) -> Option<String> {
    let indicators = build_globset(&detect.indicators);
    if let Some(file) = files.iter().find(|file| indicators.is_match(file.as_str())) {
        return Some(file.clone());
    }
    detect.contains.iter().find_map(|(file, needle)| {
        let text = contents
            .entry(file.clone())
            .or_insert_with(|| read_small(&root.join(file)));
        text.as_deref()
            .is_some_and(|text| text.contains(needle.as_str()))
            .then(|| format!("{needle} in {file}"))
    })
}

fn read_small(path: &Path) -> Option<String> {
    let metadata = path.metadata().ok()?;
    (metadata.is_file() && metadata.len() <= MAX_INDICATOR_BYTES)
        .then(|| std::fs::read_to_string(path).ok())
        .flatten()
}

/// Render the generated policy. Unselected tools appear as commented-out
/// options when the project wants them but they are not installed, or when
/// they are installed alternatives in the role of a selected tool.
pub fn render(candidates: &[Candidate]) -> String {
    let selected: Vec<&Candidate> = candidates.iter().filter(|c| c.selected).collect();
    let selected_roles: BTreeSet<&str> = selected
        .iter()
        .filter_map(|c| c.detect.role.as_deref())
        .collect();
    let options: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| !c.selected)
        .filter(|c| {
            c.wanted
                || (c.resolution.runnable()
                    && c.detect
                        .role
                        .as_deref()
                        .is_some_and(|role| selected_roles.contains(role)))
        })
        .collect();

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
    if !options.is_empty() {
        out.push_str(
            "\n  // Alternatives and tools this project seems to use but that are not\n  // installed. To enable one, uncomment it and add its key to `run`.\n",
        );
        for candidate in &options {
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
        assert!(source.contains("// [\"missing\"] = Builtins.missing  // "));
        assert!(
            !source.contains("optIn"),
            "unwanted opt-in tools stay out: {source}"
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
        assert_eq!(candidates[0].reason, "found [tool.black] in pyproject.toml");
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
