//! `velvet-glove check`: run the Stop-time workflows (check, remedy, final
//! check) on files now, outside any hook, and report per file.
//!
//! This is a thin layer over [`hookkit_tool_runner::run_check`]: it picks the
//! files, then renders the report for a person or as JSON. It never touches
//! hook session state.

use super::project::{in_policy_directory, list_project_files};
use hookkit_tool_runner::{CheckReport, CheckRequest, CheckStatus, FileStatus};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Exit status when manual fixes remain.
pub const EXIT_MANUAL: u8 = 1;
/// Exit status when a tool could not run or the policy is broken.
pub const EXIT_OPERATIONAL: u8 = 2;

/// Uncovered files named individually in the text summary.
const MAX_LISTED_UNCOVERED: usize = 10;

/// Check `files` (relative to `dir`; directories are expanded), or the Git
/// work tree's modified and untracked files under `dir` when none are given.
pub fn run(dir: &Path, config: Option<&Path>, files: &[PathBuf], json: bool) -> ExitCode {
    let candidates = match candidate_files(dir, files) {
        Ok(candidates) => candidates,
        Err(message) => return fail(json, &message),
    };
    let log_root = std::env::temp_dir().join("velvet-glove").join("check");
    let report = match hookkit_tool_runner::run_check(CheckRequest {
        directory: dir,
        config_path: config,
        files: &candidates,
        log_root: &log_root,
    }) {
        Ok(report) => report,
        Err(error) => return fail(json, &error.to_string()),
    };
    let code = exit_code(report.status());
    let text = if json {
        let mut text =
            serde_json::to_string_pretty(&render_json(&report, code)).unwrap_or_default();
        text.push('\n');
        text
    } else {
        render_text(&report)
    };
    let _ = std::io::stdout().lock().write_all(text.as_bytes());
    ExitCode::from(code)
}

/// Process exit status for an aggregate outcome.
pub fn exit_code(status: CheckStatus) -> u8 {
    match status {
        CheckStatus::Clean | CheckStatus::AutoFixed => 0,
        CheckStatus::Manual => EXIT_MANUAL,
        CheckStatus::Operational => EXIT_OPERATIONAL,
    }
}

fn fail(json: bool, message: &str) -> ExitCode {
    if json {
        let value = json!({"status": "error", "exitCode": EXIT_OPERATIONAL, "error": message});
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        );
    } else {
        eprintln!("velvet-glove check: {message}");
    }
    ExitCode::from(EXIT_OPERATIONAL)
}

/// Absolute candidate files: the named files and every project file under
/// named directories, or Git's modified and untracked files under `dir`.
fn candidate_files(dir: &Path, files: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    if files.is_empty() {
        return git_changed_files(dir);
    }
    let mut candidates = Vec::new();
    for file in files {
        let path = dir.join(file);
        if path.is_dir() {
            candidates.extend(
                list_project_files(&path)
                    .into_iter()
                    .map(|relative| path.join(relative)),
            );
        } else if path.is_file() {
            candidates.push(path);
        } else {
            return Err(format!("{} does not exist", path.display()));
        }
    }
    Ok(candidates)
}

/// Modified, staged, and untracked (not ignored) files under `dir`, except
/// the policy files in `.velvet-glove/` (as for named directories).
fn git_changed_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let git = |args: &[&str]| Command::new("git").arg("-C").arg(dir).args(args).output();
    let toplevel = git(&["rev-parse", "--show-toplevel"])
        .ok()
        .filter(|output| output.status.success())
        .map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
        .ok_or_else(|| {
            format!(
                "{} is not in a Git work tree; name the files to check",
                dir.display()
            )
        })?;
    let output = git(&[
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--",
        ".",
    ])
    .map_err(|error| format!("could not run git status: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(porcelain_paths(&output.stdout)
        .into_iter()
        .filter(|relative| !in_policy_directory(relative))
        .map(|relative| toplevel.join(relative))
        .filter(|path| path.is_file())
        .collect())
}

/// Paths from `git status --porcelain=v1 -z`, skipping rename sources.
fn porcelain_paths(stdout: &[u8]) -> Vec<String> {
    let mut paths = Vec::new();
    let mut fields = stdout
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    while let Some(record) = fields.next() {
        let [x, y, b' ', path @ ..] = record else {
            continue;
        };
        paths.push(String::from_utf8_lossy(path).into_owned());
        if [x, y].iter().any(|status| matches!(status, b'R' | b'C')) {
            fields.next();
        }
    }
    paths
}

/// Per-file verdict shared by the text and JSON renderings.
struct FileLine {
    display: String,
    absolute: PathBuf,
    status: &'static str,
    fixed_by: Vec<String>,
    unverified: bool,
    failed_tools: Vec<String>,
}

fn file_lines(report: &CheckReport) -> Vec<FileLine> {
    let result = &report.result;
    let mut failed = BTreeMap::<&Path, BTreeSet<String>>::new();
    for problem in result.operational_problems.values() {
        let tool = problem
            .tool_name
            .clone()
            .unwrap_or_else(|| "configuration".into());
        for file in &problem.affected_files {
            failed.entry(file).or_default().insert(tool.clone());
        }
    }
    // Candidates, then any other file a remedy changed (a workspace-wide fix
    // may rewrite files nobody named), so every write is reported.
    let rewritten = result
        .files
        .keys()
        .filter(|path| report.candidates.binary_search(path).is_err());
    report
        .candidates
        .iter()
        .chain(rewritten)
        .map(|path| {
            let failed_tools = failed
                .get(path.as_path())
                .map(|tools| tools.iter().cloned().collect())
                .unwrap_or_default();
            let Some(file) = result.files.get(path) else {
                let status = if result.not_applicable_files.contains(path) {
                    "not-applicable"
                } else if failed.contains_key(path.as_path()) {
                    "operational"
                } else {
                    "uncovered"
                };
                return FileLine {
                    display: display(path, &report.project_root),
                    absolute: path.clone(),
                    status,
                    fixed_by: Vec::new(),
                    unverified: false,
                    failed_tools,
                };
            };
            let unverified = file.status == FileStatus::AutoFixed
                && file.reports.iter().any(|reference| {
                    result
                        .reports
                        .get(&reference.report_id)
                        .is_some_and(|report| report.unverified)
                });
            FileLine {
                display: file.display_path.clone(),
                absolute: path.clone(),
                status: match file.status {
                    FileStatus::Clean => "clean",
                    FileStatus::AutoFixed => "auto-fixed",
                    FileStatus::ManualFixesNeeded => "manual-fixes-needed",
                },
                fixed_by: file.fixed_by.clone(),
                unverified,
                failed_tools,
            }
        })
        .collect()
}

fn render_text(report: &CheckReport) -> String {
    let mut out = String::new();
    let lines = file_lines(report);
    let mut uncovered = Vec::new();
    for line in &lines {
        let verdict = match line.status {
            "clean" => "clean".to_owned(),
            "auto-fixed" => format!(
                "auto-fixed by {}{}",
                line.fixed_by.join(", "),
                if line.unverified { " (unverified)" } else { "" }
            ),
            "manual-fixes-needed" if line.fixed_by.is_empty() => "needs manual fixes".to_owned(),
            "manual-fixes-needed" => format!(
                "needs manual fixes (partly auto-fixed by {})",
                line.fixed_by.join(", ")
            ),
            "operational" => format!(
                "not checked: {} could not run",
                line.failed_tools.join(", ")
            ),
            "not-applicable" => "not applicable".to_owned(),
            _ => {
                uncovered.push(line.display.as_str());
                continue;
            }
        };
        let failed = if line.status != "operational" && !line.failed_tools.is_empty() {
            format!("; {} could not run", line.failed_tools.join(", "))
        } else {
            String::new()
        };
        let _ = writeln!(out, "{}: {verdict}{failed}", line.display);
    }
    if !uncovered.is_empty() {
        let shown = uncovered
            .iter()
            .take(MAX_LISTED_UNCOVERED)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        let more = uncovered.len().saturating_sub(MAX_LISTED_UNCOVERED);
        let _ = writeln!(
            out,
            "No configured tool applies to {} file{}: {shown}{}",
            uncovered.len(),
            if uncovered.len() == 1 { "" } else { "s" },
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        );
    }
    if lines.is_empty() {
        out.push_str("No files to check.\n");
    }
    for issue in &report.issues {
        let _ = writeln!(
            out,
            "\n{} ({}): {}",
            issue.tool,
            issue.workflow,
            issue.files.join(", ")
        );
        for excerpt_line in issue.excerpt.lines() {
            if excerpt_line.trim().is_empty() {
                out.push('\n');
            } else {
                let _ = writeln!(out, "    {excerpt_line}");
            }
        }
    }
    if !report.problems.is_empty() {
        out.push_str("\nCould not run:\n");
        for problem in &report.problems {
            let detail = match (&problem.install_hint, &problem.log_path) {
                (Some(hint), _) if problem.missing_tool => format!("; {hint}"),
                (_, Some(log)) => format!("; log: {log}"),
                _ => String::new(),
            };
            let _ = writeln!(out, "  {}: {}{detail}", problem.tool, problem.reason);
        }
    }
    for entry in report.result.out_of_scope_reports() {
        let files = entry
            .out_of_scope_files
            .iter()
            .map(|path| display(path, &report.project_root))
            .collect::<Vec<_>>();
        let _ = writeln!(
            out,
            "\n{} also reports issues in files that were not checked: {}",
            entry.tool_name,
            files.join(", ")
        );
    }
    let count = |status: &str| lines.iter().filter(|line| line.status == status).count();
    let checked = lines.len() - uncovered.len();
    let _ = writeln!(
        out,
        "\nChecked {checked} file{}: {} clean, {} auto-fixed, {} needing manual fixes, {} not checked. Logs: {}",
        if checked == 1 { "" } else { "s" },
        count("clean"),
        count("auto-fixed"),
        count("manual-fixes-needed"),
        count("operational"),
        report.log_directory.display()
    );
    out
}

fn render_json(report: &CheckReport, code: u8) -> Value {
    let files = file_lines(report)
        .into_iter()
        .map(|line| {
            json!({
                "path": line.display,
                "absolutePath": line.absolute,
                "status": line.status,
                "fixedBy": line.fixed_by,
                "unverified": line.unverified,
                "failedTools": line.failed_tools,
            })
        })
        .collect::<Vec<_>>();
    let issues = report
        .issues
        .iter()
        .map(|issue| {
            json!({
                "tool": issue.tool,
                "toolId": issue.tool_id,
                "workflow": issue.workflow,
                "files": issue.files,
                "excerpt": issue.excerpt,
                "truncated": issue.truncated,
                "logPath": issue.log_path,
            })
        })
        .collect::<Vec<_>>();
    let problems = report
        .problems
        .iter()
        .map(|problem| {
            json!({
                "tool": problem.tool,
                "toolId": problem.tool_id,
                "reason": problem.reason,
                "missingTool": problem.missing_tool,
                "installHint": problem.install_hint,
                "logPath": problem.log_path,
                "count": problem.count,
            })
        })
        .collect::<Vec<_>>();
    let out_of_scope = report
        .result
        .out_of_scope_reports()
        .map(|entry| {
            json!({
                "tool": entry.tool_name,
                "files": entry
                    .out_of_scope_files
                    .iter()
                    .map(|path| display(path, &report.project_root))
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "status": report.status(),
        "exitCode": code,
        "projectRoot": report.project_root,
        "logDirectory": report.log_directory,
        "summaryPath": report.log_directory.join("summary.json"),
        "files": files,
        "issues": issues,
        "problems": problems,
        "outOfScope": out_of_scope,
    })
}

fn display(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_paths_skip_rename_sources_and_keep_spaces() {
        let stdout = b" M src/a.py\0R  new name.py\0old name.py\0?? notes/b.md\0A  c.rs\0";
        assert_eq!(
            porcelain_paths(stdout),
            ["src/a.py", "new name.py", "notes/b.md", "c.rs"]
        );
    }

    #[test]
    fn exit_codes_rank_operational_over_manual() {
        assert_eq!(exit_code(CheckStatus::Clean), 0);
        assert_eq!(exit_code(CheckStatus::AutoFixed), 0);
        assert_eq!(exit_code(CheckStatus::Manual), EXIT_MANUAL);
        assert_eq!(exit_code(CheckStatus::Operational), EXIT_OPERATIONAL);
    }
}
