//! Running a tool's jobs for one PostToolUse call, in parallel, and their outcomes.

use crate::command::{PhaseStatus, format_logs, render_command, run_phase_command};
use crate::deferred::{
    Attribution, attribute, combined_output, resolution_bases, source_failure_files,
};
use crate::jobs::{ToolContext, ToolJob, resolve_worker_count};
use crate::snapshot::{Snapshot, snapshot_scope};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub(super) enum ToolRunOutcome {
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
pub(super) struct CompletedToolOutcome {
    pub(super) issues: IssueState,
    pub(super) changes: ChangeState,
    pub(super) diagnostics: String,
    /// Raw output of the phases that decided `issues`: the verifiers that
    /// reported issues or, for a tool without a verifier, every phase that
    /// did. Empty when the outcome is clean.
    pub(super) issue_output: String,
    /// Files the issues are attributed to: those the deciding output names,
    /// or every job file when it names none. Empty when the output names
    /// only other files.
    pub(super) files: Vec<PathBuf>,
    /// Existing files outside the job that the output names instead.
    pub(super) out_of_scope: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IssueState {
    Clean,
    Issues,
}

#[derive(Debug)]
pub(super) enum ChangeState {
    Unchanged,
    Changed { files: Vec<PathBuf> },
}

/// Run a tool's independent per-workspace jobs, honoring `settings.jobs` for
/// bounded parallelism. Outcomes are returned in job order regardless of which
/// job finishes first, so downstream aggregation stays deterministic.
pub(super) fn run_jobs(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{CommandArgTemplate, PhaseMode, ToolPhase, ToolSpec};
    use crate::test_support::job_with_file;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

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
