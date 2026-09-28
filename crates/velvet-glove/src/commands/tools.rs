//! `velvet-glove tools`: list the built-in catalog.

use super::project::{resolve_tool, summarize_globs};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

/// Print every builtin tool with its Pkl key, id, executable status, and globs.
/// Executables resolve as the hooks resolve them with the default
/// `settings.localBinDirs` (`doctor` applies the project's own settings).
pub fn run(dir: &Path, json: bool) -> ExitCode {
    let catalog = match hookkit_pkl_config::builtin_specs() {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("velvet-glove: cannot load the builtin catalog: {error}");
            return ExitCode::FAILURE;
        }
    };
    let local_bin_dirs = hookkit_pkl_config::schema::default_local_bin_dirs();

    if json {
        let entries: Vec<serde_json::Value> = catalog
            .iter()
            .map(|(key, spec)| {
                let (program, resolution) = resolve_tool(spec, dir, &local_bin_dirs);
                let detect = spec.detect.clone().unwrap_or_default();
                serde_json::json!({
                    "key": key,
                    "id": spec.id,
                    "displayName": spec.display_name,
                    "executable": spec.executable,
                    "enabled": spec.enabled,
                    "include": spec.files.include,
                    "exclude": spec.files.exclude,
                    "resolution": {
                        "program": program,
                        "status": resolution.status(),
                        "path": resolution.path().map(|path| path.display().to_string()),
                    },
                    "installHint": spec.install_hint,
                    "detect": {
                        "indicators": detect.indicators,
                        "contains": detect.contains,
                        "role": detect.role,
                        "default": detect.default,
                        "note": detect.note,
                    },
                })
            })
            .collect();
        let text = serde_json::to_string_pretty(&entries).unwrap_or_default();
        return emit(&format!("{text}\n"));
    }

    let rows: Vec<[String; 5]> = catalog
        .iter()
        .map(|(key, spec)| {
            let (program, resolution) = resolve_tool(spec, dir, &local_bin_dirs);
            let status = match resolution.path() {
                Some(path) => format!("{} ({})", resolution.status(), path.display()),
                None => format!("{program}: missing"),
            };
            [
                key.clone(),
                spec.id.clone(),
                if spec.enabled { "yes" } else { "no" }.to_string(),
                status,
                summarize_globs(&spec.files),
            ]
        })
        .collect();
    let headers = ["KEY", "ID", "ENABLED", "EXECUTABLE", "FILES"];
    let widths: Vec<usize> = (0..4)
        .map(|column| {
            rows.iter()
                .map(|row| row[column].len())
                .chain([headers[column].len()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    let mut print_row = |cells: [&str; 5]| {
        let _ = writeln!(
            out,
            "{:w0$}  {:w1$}  {:w2$}  {:w3$}  {}",
            cells[0],
            cells[1],
            cells[2],
            cells[3],
            cells[4],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
        );
    };
    print_row(headers);
    for row in &rows {
        print_row([&row[0], &row[1], &row[2], &row[3], &row[4]]);
    }
    let _ = writeln!(
        out,
        "\n{} tools. Use a KEY in your policy: tools {{ [\"KEY\"] = Builtins.KEY }} and run {{ \"KEY\" }}.",
        rows.len()
    );
    emit(&out)
}

/// Write everything at once; a closed pipe (e.g. `| head`) is not an error.
fn emit(text: &str) -> ExitCode {
    let _ = std::io::stdout().lock().write_all(text.as_bytes());
    ExitCode::SUCCESS
}
