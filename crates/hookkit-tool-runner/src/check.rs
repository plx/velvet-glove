//! Direct deferred checks for `velvet-glove check`.
//!
//! The Stop hook's planning, execution, and reporting applied to explicit
//! files, outside any hook: no session state, file-activity window, loop
//! guard, or native lowering is involved. Command logs and a `summary.json`
//! go to a fresh directory so excerpts can point at full output.

use crate::convert::resolve_run_order;
use crate::deferred::{
    BatchToolSummary, DeferredReporter, DeferredRunResult, IssueExcerpt, ProblemSummary,
    build_deferred_plan, execute_deferred_workflows, problem_entries, write_deferred_artifacts,
};
use crate::paths::{display_roots, normalize_path};
use crate::record_uncovered_candidates;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Check directories kept under [`CheckRequest::log_root`]; older ones are
/// removed.
const RETAINED_CHECK_RUNS: usize = 20;

/// Inputs for [`run_check`].
#[derive(Debug, Clone)]
pub struct CheckRequest<'a> {
    /// Directory whose policy applies: the discovery start, and the project
    /// root when `config_path` is given.
    pub directory: &'a Path,
    /// Explicit Pkl policy, or `None` for layered discovery.
    pub config_path: Option<&'a Path>,
    /// Files to check. Each is canonicalized; tools select them by their
    /// globs exactly as at Stop.
    pub files: &'a [PathBuf],
    /// Parent of the per-run log directory.
    pub log_root: &'a Path,
}

/// Why a check could not run at all.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    /// The policy failed to load, validate, or plan.
    #[error("{0}")]
    Configuration(String),
    /// Logs or the summary could not be written.
    #[error("{0}")]
    Io(String),
}

/// Result of [`run_check`].
#[derive(Debug, Clone)]
pub struct CheckReport {
    /// Canonical project root that relative globs resolved against.
    pub project_root: PathBuf,
    /// Directory holding this run's command logs and `summary.json`.
    pub log_directory: PathBuf,
    /// Canonical candidate files, sorted.
    pub candidates: Vec<PathBuf>,
    /// Complete deferred result; file display paths are project-relative.
    pub result: DeferredRunResult,
    /// Bounded excerpts of the checks that still report issues.
    pub issues: Vec<IssueExcerpt>,
    /// Operational problems, one entry per tool.
    pub problems: Vec<ProblemSummary>,
}

/// Aggregate outcome of a check, in increasing severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckStatus {
    /// Every checked file is clean (or no tool applied).
    Clean,
    /// Remedies fixed everything they touched.
    AutoFixed,
    /// Some file still needs a manual fix.
    Manual,
    /// A tool could not run, so the result is incomplete.
    Operational,
}

impl CheckReport {
    /// Worst outcome: operational problems outrank manual issues, which
    /// outrank auto-fixes.
    pub fn status(&self) -> CheckStatus {
        if self.result.has_operational_problems() {
            CheckStatus::Operational
        } else if self.result.has_manual_fixes() {
            CheckStatus::Manual
        } else if self
            .result
            .files
            .values()
            .any(|file| file.status == crate::FileStatus::AutoFixed)
        {
            CheckStatus::AutoFixed
        } else {
            CheckStatus::Clean
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckSummary<'a> {
    schema_version: u32,
    kind: &'static str,
    status: CheckStatus,
    project_root: &'a Path,
    candidate_files: &'a [PathBuf],
    tools: Vec<BatchToolSummary>,
    result: &'a DeferredRunResult,
}

/// Run the configured deferred workflows (check, remedy, final check) on
/// `request.files`, like the Stop hook but outside any hook session.
pub fn run_check(request: CheckRequest<'_>) -> Result<CheckReport, CheckError> {
    let configuration =
        |error: &dyn std::fmt::Display| CheckError::Configuration(error.to_string());
    let loaded = hookkit_pkl_config::discover_and_load(request.directory, request.config_path)
        .map_err(|error| configuration(&error))?;
    let settings = &loaded.config.settings;
    let project_root = normalize_path(&loaded.project_root);
    let roots = display_roots(&project_root, &loaded.project_root);
    let reporter = DeferredReporter::new(&settings.deferred_reporting)
        .map_err(|error| configuration(&error))?;
    let mut candidates = request
        .files
        .iter()
        .map(|path| normalize_path(path))
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    let tools = resolve_run_order(&loaded.config).map_err(|error| configuration(&error))?;
    let (plan, planned_tools) = build_deferred_plan(&tools, &candidates, &project_root, settings)
        .map_err(|error| configuration(&error))?;

    let log_directory = new_log_directory(request.log_root)?;
    let mut execution = {
        let _project = crate::project_lock::lock_project(&project_root);
        execute_deferred_workflows(&plan, settings.jobs, settings.fail_fast)
    };
    let tool_summaries = write_deferred_artifacts(
        &mut |relative, contents| {
            let path = log_directory.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, contents)?;
            Ok(path)
        },
        &plan,
        &planned_tools,
        &execution.logs,
        &mut execution.result,
    )
    .map_err(|error| CheckError::Io(error.to_string()))?;
    let mut result = execution.result;
    record_uncovered_candidates(&mut result, &candidates);
    reporter.apply_groups(&mut result, &project_root);
    let root_refs = roots.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let issues = reporter.issue_entries(&result, &project_root, &root_refs);
    let problems = problem_entries(&result);

    let report = CheckReport {
        project_root,
        log_directory,
        candidates,
        result,
        issues,
        problems,
    };
    let summary = CheckSummary {
        schema_version: 1,
        kind: "check",
        status: report.status(),
        project_root: &report.project_root,
        candidate_files: &report.candidates,
        tools: tool_summaries,
        result: &report.result,
    };
    let summary = serde_json::to_vec_pretty(&summary)
        .map_err(|error| CheckError::Io(format!("could not serialize summary: {error}")))?;
    std::fs::write(report.log_directory.join("summary.json"), summary).map_err(|error| {
        CheckError::Io(format!(
            "could not write {}: {error}",
            report.log_directory.display()
        ))
    })?;
    Ok(report)
}

/// A fresh `<millis>-<pid>` directory under `root`, pruning older ones.
fn new_log_directory(root: &Path) -> Result<PathBuf, CheckError> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let directory = root.join(format!("{millis}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).map_err(|error| {
        CheckError::Io(format!(
            "could not create log directory {}: {error}",
            directory.display()
        ))
    })?;
    crate::deferred::prune_run_bundles(root, RETAINED_CHECK_RUNS, &directory);
    Ok(directory)
}
