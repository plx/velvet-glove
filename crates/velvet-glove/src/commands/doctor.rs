//! `velvet-glove doctor`: explain what the hooks would do in a directory.

use super::project::{
    MIN_PKL_VERSION, Resolution, as_path_refs, list_project_files, pkl_version, resolve_tool,
    tool_search_dirs,
};
use hookkit_pkl_config::discovery::{self, DiscoveredKind, LEGACY_CONFIG_DIR};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Print the configuration chain, run list, tool resolution, Pkl version, and
/// state directory. Exits nonzero when the hooks cannot work as configured.
pub fn run(dir: &Path, config: Option<&Path>, state_dir: &Path) -> ExitCode {
    let mut problems: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let (min_major, min_minor, min_patch) = MIN_PKL_VERSION;
    let minimum = format!("{min_major}.{min_minor}.{min_patch}");

    println!("Directory: {}", dir.display());

    match pkl_version() {
        Ok(pkl) if pkl.supported => println!("Pkl: {} (ok; needs {minimum} or newer)", pkl.version),
        Ok(pkl) => {
            println!("Pkl: {} (too old)", pkl.version);
            problems.push(format!(
                "Pkl {} is older than {minimum}; upgrade it (https://pkl-lang.org)",
                pkl.version
            ));
        }
        Err(error) => {
            println!("Pkl: not available");
            problems.push(format!(
                "{error}; install Pkl {minimum} or newer (https://pkl-lang.org)"
            ));
        }
    }

    println!("\nConfiguration (merged in this order; later files win):");
    let chain: Vec<(String, PathBuf)> = match config {
        Some(path) => vec![("explicit".to_string(), path.to_path_buf())],
        None => discovery::discover(dir)
            .into_iter()
            .map(|found| (layer_name(found.kind).to_string(), found.path))
            .collect(),
    };
    if chain.is_empty() {
        println!("  (none found)");
    }
    let mut seen: Vec<&Path> = Vec::new();
    for (layer, path) in &chain {
        let mut note = String::new();
        if seen.contains(&path.as_path()) {
            note.push_str("  [duplicate: evaluated twice]");
        }
        if path
            .components()
            .any(|part| part.as_os_str() == LEGACY_CONFIG_DIR)
        {
            note.push_str("  [legacy location]");
            warnings.push(format!(
                "{} uses the legacy {LEGACY_CONFIG_DIR} directory; move it to .velvet-glove/",
                path.display()
            ));
        }
        println!("  {layer:<8} {}{note}", path.display());
        seen.push(path);
    }

    let loaded = if chain.is_empty() {
        None
    } else {
        match hookkit_pkl_config::discover_and_load(dir, config) {
            Ok(loaded) => Some(loaded),
            Err(error) => {
                problems.push(format!("the configuration does not load: {error}"));
                None
            }
        }
    };

    if let Some(loaded) = &loaded {
        println!("\nProject root: {}", loaded.project_root.display());
        let run = &loaded.config.run;
        println!(
            "Run list ({} tool{}):",
            run.len(),
            if run.len() == 1 { "" } else { "s" }
        );
        // Listed once and reused for every tool below: resolution searches
        // from the directories of each tool's own matching files, the same
        // way `init` and the runner do, so a tool that lives in a nested
        // workspace (e.g. `frontend/node_modules/.bin/eslint`) is found.
        let files = list_project_files(&loaded.project_root);
        // Loading validated that every `run` entry names a `tools` entry; an
        // unknown one is reported above as a load error.
        for (key, spec) in run
            .iter()
            .filter_map(|key| Some((key, loaded.config.tools.get(key)?)))
        {
            if !spec.enabled {
                println!("  {key:<20} disabled (enabled = false)");
                warnings.push(format!("{key} is in `run` but disabled, so it never runs"));
                continue;
            }
            let local_bin_dirs = &loaded.config.settings.local_bin_dirs;
            let search_dirs = tool_search_dirs(
                &loaded.project_root,
                &spec.files,
                &loaded.config.settings.exclude,
                &files,
            );
            let (program, resolution) = resolve_tool(
                spec,
                &loaded.project_root,
                local_bin_dirs,
                &as_path_refs(&search_dirs),
            );
            match &resolution {
                Resolution::Path(path) => println!("  {key:<20} {program} -> {}", path.display()),
                Resolution::ProjectLocal(path) => {
                    println!(
                        "  {key:<20} {program} -> {} (project-local)",
                        path.display()
                    );
                }
                Resolution::Unconfigured(path) => {
                    println!(
                        "  {key:<20} {program} only at {} (not searched)",
                        path.display()
                    );
                    warnings.push(format!(
                        "{key}: {program} is only at {}, which settings.localBinDirs ({}) does not include; add its directory there or put it on PATH",
                        path.display(),
                        local_bin_dirs.join(", ")
                    ));
                }
                Resolution::Missing => {
                    let hint = spec
                        .install_hint
                        .clone()
                        .unwrap_or_else(|| format!("install {program}"));
                    println!("  {key:<20} {program} missing; {hint}");
                    warnings.push(format!(
                        "{key}: {program} is neither on PATH nor in settings.localBinDirs, so it will be skipped; {hint}"
                    ));
                }
            }
        }
        if run.is_empty() {
            warnings.push("the run list is empty, so the hooks will not run any tool".to_string());
        }
    } else if chain.is_empty() {
        warnings.push(
            "no configuration found, so the hooks will not run any tool; run `velvet-glove init`"
                .to_string(),
        );
    }

    let state_status = if state_dir.is_dir() {
        "exists"
    } else {
        "created on first use"
    };
    println!(
        "\nState directory: {} ({state_status})",
        state_dir.display()
    );

    if !warnings.is_empty() {
        println!("\nWarnings:");
        for warning in &warnings {
            println!("  - {warning}");
        }
    }
    if !problems.is_empty() {
        println!("\nProblems:");
        for problem in &problems {
            println!("  - {problem}");
        }
        println!(
            "\n{} problem(s) will stop the hooks from working.",
            problems.len()
        );
        return ExitCode::FAILURE;
    }
    println!("\nOK: {} warning(s).", warnings.len());
    ExitCode::SUCCESS
}

fn layer_name(kind: DiscoveredKind) -> &'static str {
    match kind {
        DiscoveredKind::Home => "home",
        DiscoveredKind::Project => "project",
        DiscoveredKind::Local => "local",
    }
}
