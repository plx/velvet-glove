//! Per-tool jobs: grouping files by workspace, running jobs in parallel, and their outcomes.

use crate::command::{PhaseStatus, format_logs, render_command, run_phase_command};
use crate::deferred::{
    Attribution, attribute, combined_output, resolution_bases, source_failure_files,
};
use crate::snapshot::{Snapshot, snapshot_scope};
use crate::spec::{InvocationGranularity, ToolSpec, WorkspaceFallback};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone)]
pub(crate) struct ToolContext<'a> {
    pub(crate) spec: &'a ToolSpec,
    pub(crate) project_root: &'a Path,
    pub(crate) global_diagnostics_dir: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolJob {
    pub(crate) workspace_dir: PathBuf,
    pub(crate) workspace_indicator: Option<PathBuf>,
    pub(crate) files: Vec<PathBuf>,
}

/// Group `paths` into jobs: by nearest workspace indicator when the tool has
/// one, where a file with no indicator above it is skipped or, with the
/// project-root fallback, grouped at the project root without a marker.
pub(crate) fn build_jobs(paths: &[PathBuf], project_root: &Path, spec: &ToolSpec) -> Vec<ToolJob> {
    if let Some(indicator) = &spec.workspace_indicator {
        let mut grouped = BTreeMap::<PathBuf, ToolJob>::new();
        for path in paths {
            let (workspace_dir, indicator_path) =
                match nearest_workspace_indicator(path, project_root, indicator) {
                    Some((workspace_dir, indicator_path)) => (workspace_dir, Some(indicator_path)),
                    None if spec.workspace_fallback == WorkspaceFallback::ProjectRoot => {
                        (project_root.to_path_buf(), None)
                    }
                    None => continue,
                };
            grouped
                .entry(workspace_dir.clone())
                .or_insert_with(|| ToolJob {
                    workspace_dir,
                    workspace_indicator: indicator_path,
                    files: Vec::new(),
                })
                .files
                .push(path.clone());
        }
        grouped.into_values().collect()
    } else {
        vec![ToolJob {
            workspace_dir: project_root.to_path_buf(),
            workspace_indicator: None,
            files: paths.to_vec(),
        }]
    }
}

/// Finds the nearest ancestor of `path` (up to `project_root`) whose
/// `indicator`-relative file exists, returning both that ancestor (the
/// workspace root) and the indicator file itself. `indicator` may be a
/// multi-component relative path (e.g. `sorbet/config`), so the workspace
/// root is the directory the search matched *from*, not simply the
/// indicator file's immediate parent — that would land inside `sorbet/`
/// instead of the app root for a nested indicator like that one.
fn nearest_workspace_indicator(
    path: &Path,
    project_root: &Path,
    indicator: &str,
) -> Option<(PathBuf, PathBuf)> {
    // Never look above the project root, or outside it for an outside file.
    if !path.starts_with(project_root) {
        return None;
    }
    let mut current = path.parent();
    while let Some(dir) = current {
        let candidate = dir.join(indicator);
        if candidate.is_file() {
            return Some((dir.to_path_buf(), candidate));
        }
        if dir == project_root {
            break;
        }
        current = dir.parent();
    }
    None
}

#[derive(Debug)]
pub(crate) enum ToolRunOutcome {
    Completed(CompletedToolOutcome),
    ToolUnavailable {
        phase: String,
        executable: String,
        install_hint: Option<String>,
        changed_files: Vec<PathBuf>,
    },
    ToolFailed {
        phase: String,
        exit_code: Option<i32>,
        /// Spawn or timeout error, when the command did not simply exit.
        error: Option<String>,
        diagnostics: String,
        changed_files: Vec<PathBuf>,
    },
}

#[derive(Debug)]
pub(crate) struct CompletedToolOutcome {
    pub(crate) issues: IssueState,
    pub(crate) changes: ChangeState,
    pub(crate) diagnostics: String,
    /// Raw output of the phases that decided `issues`: the verifiers that
    /// reported issues or, for a tool without a verifier, every phase that
    /// did. Empty when the outcome is clean.
    pub(crate) issue_output: String,
    /// Files the issues are attributed to: those the deciding output names,
    /// or every job file when it names none. Empty when the output names
    /// only other files.
    pub(crate) files: Vec<PathBuf>,
    /// Existing files outside the job that the output names instead.
    pub(crate) out_of_scope: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IssueState {
    Clean,
    Issues,
}

#[derive(Debug)]
pub(crate) enum ChangeState {
    Unchanged,
    Changed { files: Vec<PathBuf> },
}

/// Run a tool's independent per-workspace jobs, honoring `settings.jobs` for
/// bounded parallelism. Outcomes are returned in job order regardless of which
/// job finishes first, so downstream aggregation stays deterministic.
pub(crate) fn run_jobs(
    jobs: &[ToolJob],
    context: &ToolContext<'_>,
    jobs_setting: u32,
) -> Vec<ToolRunOutcome> {
    let worker_count = resolve_worker_count(jobs_setting, jobs.len());
    if worker_count <= 1 {
        return jobs.iter().map(|job| run_job(job, context)).collect();
    }

    // Work-stealing over a shared cursor: each worker claims the next index via
    // an atomic fetch-add, so uneven job costs balance across threads. The
    // mutex is held only to stash a finished outcome, never across `run_job`.
    let cursor = AtomicUsize::new(0);
    let outcomes = Mutex::new(Vec::<(usize, ToolRunOutcome)>::with_capacity(jobs.len()));

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let idx = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(idx) else { break };
                    let outcome = run_job(job, context);
                    outcomes
                        .lock()
                        .expect("run_jobs outcome mutex poisoned")
                        .push((idx, outcome));
                }
            });
        }
    });

    let mut outcomes = outcomes
        .into_inner()
        .expect("run_jobs outcome mutex poisoned");
    outcomes.sort_by_key(|(idx, _)| *idx);
    outcomes.into_iter().map(|(_, outcome)| outcome).collect()
}

/// Upper bound for `jobs = 0` (auto).
const AUTO_JOBS_CAP: usize = 8;

/// Resolve `settings.jobs` to a worker-thread count for a batch of `job_count`
/// independent jobs.
///
/// `jobs = 0` selects "auto": the available parallelism, capped at
/// [`AUTO_JOBS_CAP`]. `jobs = n >= 1` runs up to `n` jobs concurrently. Both
/// are capped at `job_count` since extra workers would have nothing to claim.
pub(crate) fn resolve_worker_count(jobs_setting: u32, job_count: usize) -> usize {
    if job_count == 0 {
        return 0;
    }
    let requested = match jobs_setting {
        0 => std::thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .min(AUTO_JOBS_CAP),
        n => n as usize,
    };
    requested.clamp(1, job_count)
}

fn run_job(job: &ToolJob, context: &ToolContext<'_>) -> ToolRunOutcome {
    let before_scope = snapshot_scope(job, context);
    let before = Snapshot::read(&before_scope);
    let bases = resolution_bases(&job.workspace_dir, context.project_root);
    let job_files = job.files.iter().cloned().collect::<BTreeSet<_>>();
    let mut logs = Vec::new();
    let mut saw_issues = false;
    let mut verify_state = None;
    let mut verifier_issue_output = Vec::new();
    let mut phase_issue_output = Vec::new();
    // Verifier output that blames files by mentioning them, and the job files
    // that verifiers failing at a source location named: those are blamed
    // exactly, as at Stop, not by everything else their output mentions.
    let mut attributable_output = Vec::new();
    let mut located = BTreeSet::new();
    // A mutating phase that exited with a failure code, typically a
    // formatter that cannot parse a syntax error (some, like `ruff format
    // --quiet` before Ruff 0.16, say nothing). As a failed remedy at Stop,
    // it does not end the tool: later phases still run, and the failure
    // stands only if they blame none of the job's files.
    let mut failed_mutation = None;

    for phase in &context.spec.phases {
        if !phase.enabled {
            continue;
        }

        let command = render_command(phase, job, context);
        let mut log = run_phase_command(phase, &command, &job.workspace_dir);
        if !phase.is_verifier()
            && log.error.is_none()
            && log.status.is_some()
            && log.classification == Some(PhaseStatus::Failure)
        {
            failed_mutation.get_or_insert((phase.id.clone(), log.status));
            logs.push(log);
            continue;
        }
        // A verifier failing while naming a job file at a source location
        // (mypy's exit 2 on a syntax error) reports issues in that file.
        let source_files = if phase.is_verifier() {
            source_failure_files(&log, &job_files, &bases)
        } else {
            Vec::new()
        };
        if !source_files.is_empty() {
            log.classification = Some(PhaseStatus::Issues);
        }

        if let Some(error) = &log.error {
            if error == "not found" {
                let executable = command.program.clone();
                return ToolRunOutcome::ToolUnavailable {
                    phase: phase.id.clone(),
                    executable,
                    install_hint: context.spec.install_hint.clone(),
                    changed_files: changed_files_since(&before, job, context),
                };
            }
            let error = error.clone();
            logs.push(log);
            return ToolRunOutcome::ToolFailed {
                phase: phase.id.clone(),
                exit_code: None,
                error: Some(error),
                diagnostics: format_logs(&logs),
                changed_files: changed_files_since(&before, job, context),
            };
        }

        match log.classification {
            Some(PhaseStatus::Clean) => {
                if phase.is_verifier() && verify_state != Some(IssueState::Issues) {
                    // Don't downgrade a prior verifier's Issues verdict.
                    verify_state = Some(IssueState::Clean);
                }
            }
            Some(PhaseStatus::Issues) => {
                saw_issues = true;
                phase_issue_output.push(combined_output(&log));
                if phase.is_verifier() {
                    verify_state = Some(IssueState::Issues);
                    verifier_issue_output.push(combined_output(&log));
                    if source_files.is_empty() {
                        attributable_output.push(combined_output(&log));
                    } else {
                        located.extend(source_files);
                    }
                }
            }
            Some(PhaseStatus::Failure) | None => {
                logs.push(log);
                return ToolRunOutcome::ToolFailed {
                    phase: phase.id.clone(),
                    exit_code: logs.last().and_then(|log| log.status),
                    error: None,
                    diagnostics: format_logs(&logs),
                    changed_files: changed_files_since(&before, job, context),
                };
            }
        }

        logs.push(log);
    }

    let after_scope = snapshot_scope(job, context);
    let after = Snapshot::read(&after_scope);
    let changed_files = before.changed_files(&after);
    let issues = verify_state.unwrap_or(if saw_issues {
        IssueState::Issues
    } else {
        IssueState::Clean
    });
    let (issue_output, attributable_output) = match (issues, verify_state) {
        (IssueState::Clean, _) => (Vec::new(), Vec::new()),
        (IssueState::Issues, Some(_)) => (verifier_issue_output, attributable_output),
        (IssueState::Issues, None) => (phase_issue_output.clone(), phase_issue_output),
    };
    let issue_output = issue_output.join("\n");
    // As at Stop, blame the files the deciding output names: a workspace-wide
    // check reporting a pre-existing issue elsewhere must not be pinned on the
    // file this call changed.
    let (files, out_of_scope) = match issues {
        IssueState::Clean => (job.files.clone(), Vec::new()),
        IssueState::Issues if attributable_output.is_empty() => {
            (located.into_iter().collect(), Vec::new())
        }
        IssueState::Issues => {
            let scope = job
                .files
                .iter()
                .chain(&changed_files)
                .cloned()
                .collect::<BTreeSet<_>>();
            match attribute(&attributable_output.join("\n"), &scope, &bases) {
                Attribution::Named(files) => {
                    located.extend(files);
                    (located.into_iter().collect(), Vec::new())
                }
                Attribution::OutOfScope(others) if located.is_empty() => (Vec::new(), others),
                Attribution::OutOfScope(_) => (located.into_iter().collect(), Vec::new()),
                Attribution::Unnamed => (job.files.clone(), Vec::new()),
            }
        }
    };
    if let Some((phase, exit_code)) = failed_mutation {
        let explained =
            issues == IssueState::Issues && files.iter().any(|file| job_files.contains(file));
        if !explained {
            return ToolRunOutcome::ToolFailed {
                phase,
                exit_code,
                error: None,
                diagnostics: format_logs(&logs),
                changed_files,
            };
        }
    }
    let changes = if changed_files.is_empty() {
        ChangeState::Unchanged
    } else {
        ChangeState::Changed {
            files: changed_files,
        }
    };

    ToolRunOutcome::Completed(CompletedToolOutcome {
        issues,
        changes,
        diagnostics: format_logs(&logs),
        issue_output,
        files,
        out_of_scope,
    })
}

fn changed_files_since(
    before: &Snapshot,
    job: &ToolJob,
    context: &ToolContext<'_>,
) -> Vec<PathBuf> {
    let after_scope = snapshot_scope(job, context);
    let after = Snapshot::read(&after_scope);
    before.changed_files(&after)
}

pub(crate) fn invocation_jobs(
    base_jobs: &[ToolJob],
    invocation: InvocationGranularity,
) -> Vec<ToolJob> {
    if invocation != InvocationGranularity::PerFile {
        return base_jobs.to_vec();
    }
    base_jobs
        .iter()
        .flat_map(|job| {
            job.files.iter().cloned().map(|file| ToolJob {
                workspace_dir: job.workspace_dir.clone(),
                workspace_indicator: job.workspace_indicator.clone(),
                files: vec![file],
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{CommandArgTemplate, PhaseMode, ToolPhase};
    use crate::test_support::{job_with_file, unique_test_directory};
    use proptest::prelude::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn nearest_workspace_indicator_resolves_nested_indicators_to_their_own_directory() {
        // A single-component indicator (e.g. "Cargo.toml") sitting directly in
        // the workspace root, and a nested one (e.g. "sorbet/config") one
        // level deeper, must both resolve `workspace_dir` to the same app
        // root — not to the indicator's immediate parent, which for the
        // nested case would be the "sorbet" directory itself.
        let root = unique_test_directory("nearest-workspace-indicator");
        std::fs::create_dir_all(root.join("sorbet")).unwrap();
        std::fs::write(root.join("Gemfile"), "source 'https://rubygems.org'\n").unwrap();
        std::fs::write(root.join("sorbet/config"), "--dir\n.\n").unwrap();
        let file = root.join("example.rb");
        std::fs::write(&file, "# typed: true\n").unwrap();

        let (single_dir, single_indicator) =
            nearest_workspace_indicator(&file, &root, "Gemfile").expect("Gemfile found");
        assert_eq!(single_dir, root);
        assert_eq!(single_indicator, root.join("Gemfile"));

        let (nested_dir, nested_indicator) =
            nearest_workspace_indicator(&file, &root, "sorbet/config").expect("config found");
        assert_eq!(nested_dir, root);
        assert_eq!(nested_indicator, root.join("sorbet/config"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn files_without_an_indicator_run_from_the_project_root_only_with_the_fallback() {
        let root = unique_test_directory("workspace-fallback");
        std::fs::create_dir_all(root.join("frontend/src")).unwrap();
        std::fs::write(root.join("frontend/package.json"), "{}\n").unwrap();
        let nested = root.join("frontend/src/x.js");
        let loose = root.join("README.md");
        let paths = [nested.clone(), loose.clone()];
        let spec = ToolSpec::new("tool", "Tool", "tool").with_workspace_indicator("package.json");

        let skipped = build_jobs(&paths, &root, &spec);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].workspace_dir, root.join("frontend"));
        assert_eq!(skipped[0].files, std::slice::from_ref(&nested));

        let spec = spec.with_workspace_fallback(WorkspaceFallback::ProjectRoot);
        let jobs = build_jobs(&paths, &root, &spec);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].workspace_dir, root);
        assert_eq!(jobs[0].workspace_indicator, None);
        assert_eq!(jobs[0].files, [loose]);
        assert_eq!(jobs[1].workspace_dir, root.join("frontend"));
        assert_eq!(
            jobs[1].workspace_indicator,
            Some(root.join("frontend/package.json"))
        );
        assert_eq!(jobs[1].files, [nested]);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_worker_count_honors_jobs_setting() {
        // auto (0) uses the available parallelism, capped at 8.
        let auto = std::thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .min(AUTO_JOBS_CAP);
        assert_eq!(resolve_worker_count(0, 64), auto);
        assert!(resolve_worker_count(0, 5) <= 5);
        // explicit serial.
        assert_eq!(resolve_worker_count(1, 5), 1);
        // bounded parallelism up to the requested count.
        assert_eq!(resolve_worker_count(4, 5), 4);
        // never spin up more workers than there are jobs.
        assert_eq!(resolve_worker_count(8, 5), 5);
        assert_eq!(resolve_worker_count(2, 1), 1);
        // no jobs means no workers.
        assert_eq!(resolve_worker_count(4, 0), 0);
        assert_eq!(resolve_worker_count(0, 0), 0);
    }

    proptest! {
        /// Property: worker selection is total and bounded. A non-empty batch
        /// always gets at least one worker, never more workers than jobs, and
        /// explicit settings are honored up to that cap (`0` means serial auto).
        #[test]
        fn worker_count_is_bounded(jobs_setting in any::<u32>(), job_count in any::<usize>()) {
            let actual = resolve_worker_count(jobs_setting, job_count);
            let expected = if job_count == 0 {
                0
            } else if jobs_setting == 0 {
                resolve_worker_count(0, usize::MAX).min(job_count)
            } else {
                usize::try_from(jobs_setting).unwrap_or(usize::MAX).min(job_count)
            };

            prop_assert_eq!(actual, expected);
            prop_assert!(jobs_setting != 0 || actual <= AUTO_JOBS_CAP);
            prop_assert!(actual <= job_count);
            prop_assert_eq!(actual == 0, job_count == 0);
        }
    }

    #[test]
    fn per_file_invocation_splits_jobs_without_losing_workspace_context() {
        let root = PathBuf::from("/tmp/hookkit-per-file-jobs");
        let marker = root.join("package.json");
        let base = ToolJob {
            workspace_dir: root.clone(),
            workspace_indicator: Some(marker.clone()),
            files: vec![root.join("first.json"), root.join("second.json")],
        };

        let jobs = invocation_jobs(std::slice::from_ref(&base), InvocationGranularity::PerFile);

        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].workspace_dir, root);
        assert_eq!(jobs[0].workspace_indicator.as_ref(), Some(&marker));
        assert_eq!(jobs[0].files, [base.files[0].clone()]);
        assert_eq!(jobs[1].files, [base.files[1].clone()]);
        assert_eq!(
            invocation_jobs(std::slice::from_ref(&base), InvocationGranularity::Batch)[0].files,
            base.files
        );
    }

    fn completed_files(outcome: &ToolRunOutcome) -> &[PathBuf] {
        match outcome {
            ToolRunOutcome::Completed(completed) => completed.files.as_slice(),
            other => panic!("expected Completed outcome, got {other:?}"),
        }
    }

    #[test]
    fn run_jobs_preserves_order_across_concurrency_levels() {
        // A spec with no phases makes `run_job` complete without spawning any
        // process or touching the filesystem, so this stays fast and
        // deterministic while still exercising the parallel execution path.
        let root = PathBuf::from("/tmp/hookkit-run-jobs-test");
        let spec = ToolSpec::new("test-tool", "Test Tool", "test-exec");
        let context = ToolContext {
            spec: &spec,
            project_root: &root,
            global_diagnostics_dir: None,
        };
        let jobs: Vec<ToolJob> = (0..16)
            .map(|i| job_with_file(&root, &format!("file-{i:02}.rs")))
            .collect();

        // Serial (1), auto (0), and bounded-parallel (>1, including more than
        // CPUs) must all return one outcome per job, in job order.
        for jobs_setting in [0u32, 1, 4, 32] {
            let outcomes = run_jobs(&jobs, &context, jobs_setting);
            assert_eq!(outcomes.len(), jobs.len(), "jobs_setting={jobs_setting}");
            for (job, outcome) in jobs.iter().zip(&outcomes) {
                assert_eq!(
                    completed_files(outcome),
                    job.files.as_slice(),
                    "jobs_setting={jobs_setting}: outcome order must match job order",
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn hermetic_fake_executable_smoke() {
        let root = std::env::temp_dir().join(format!(
            "velvet-glove-hermetic-smoke-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create smoke directory");

        let fake = root.join("fake-checker");
        std::fs::write(&fake, "#!/bin/sh\nprintf 'fake checker clean\\n'\n")
            .expect("write fake executable");
        let mut permissions = std::fs::metadata(&fake)
            .expect("read fake executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake, permissions).expect("make fake executable runnable");

        let target = root.join("input.rs");
        std::fs::write(&target, "fn main() {}\n").expect("write smoke input");
        let spec = ToolSpec::new("fake", "Fake checker", fake.to_string_lossy().into_owned())
            .with_phase(
                ToolPhase::new("verify", PhaseMode::Verify).with_args([CommandArgTemplate::Files]),
            );
        let context = ToolContext {
            spec: &spec,
            project_root: &root,
            global_diagnostics_dir: None,
        };
        let job = job_with_file(&root, "input.rs");

        let outcome = run_job(&job, &context);
        let ToolRunOutcome::Completed(completed) = outcome else {
            panic!("expected completed fake-tool run");
        };
        assert_eq!(completed.issues, IssueState::Clean);
        assert!(matches!(completed.changes, ChangeState::Unchanged));
        assert!(completed.diagnostics.contains("fake checker clean"));

        std::fs::remove_dir_all(&root).expect("remove smoke directory");
    }
}
