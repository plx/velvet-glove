use super::attribution::{Attribution, attribute, resolution_bases};
use super::{CheckOutcome, DeferredRunResult, OperationalProblem, ToolReport};
use crate::{
    CheckScope, CommandPhase, PhaseLog, PhaseStatus, RenderedCommand, Snapshot, ToolContext,
    ToolJob, ToolPhase, ToolSpec, render_command, resolve_worker_count, run_phase_command,
    write_scope,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone)]
pub(crate) struct ScheduledWorkflow {
    pub tool_index: usize,
    pub workflow_index: usize,
    pub job_index: usize,
    pub spec: Arc<ToolSpec>,
    pub workflow_id: String,
    pub check: Option<ToolPhase>,
    pub remedy: Option<ToolPhase>,
    pub check_scope: CheckScope,
    pub compatibility_translation: bool,
    pub job: ToolJob,
    pub project_root: PathBuf,
}

impl ScheduledWorkflow {
    pub(crate) fn report_id(&self) -> String {
        format!(
            "{:03}-{}-{:03}-{:03}",
            self.tool_index, self.spec.id, self.workflow_index, self.job_index
        )
    }

    fn context(&self) -> ToolContext<'_> {
        ToolContext {
            spec: &self.spec,
            project_root: &self.project_root,
            global_diagnostics_dir: None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct DeferredLog {
    pub tool_index: usize,
    pub workflow_index: usize,
    pub job_index: usize,
    pub phase: CommandPhase,
    pub log: PhaseLog,
}

#[derive(Debug, Default)]
pub(crate) struct DeferredExecution {
    pub result: DeferredRunResult,
    pub logs: Vec<DeferredLog>,
}

#[derive(Debug, Default)]
struct WorkflowState {
    initial_check: Option<CheckOutcome>,
    /// Outcome and combined output of the most recent check.
    last_check: Option<CheckOutcome>,
    last_output: String,
    /// Number of write impacts that the most recent check already observed.
    checked_at: usize,
    fix_attempted: bool,
    changed_files: BTreeSet<PathBuf>,
    /// No authoritative result exists: a check failed, the remedy was
    /// skipped under failFast, or a check-less remedy failed.
    operational: bool,
}

#[derive(Debug)]
struct WriteImpact {
    workspace: PathBuf,
    changed_files: BTreeSet<PathBuf>,
}

/// Why a command could not produce a usable result.
struct CommandFailure {
    message: String,
    missing_tool: bool,
}

/// Execute one global Stop-time plan: all checks first, then one ordered
/// remedy pass in which a clean check invalidated by an earlier remedy is
/// rerun before its remedy decision, then authoritative reruns for every
/// check a remedy may have invalidated.
///
/// With `fail_fast`, an operational failure stops later remedies of the same
/// tool only; other tools still run their remedies.
pub(crate) fn execute_deferred_workflows(
    plan: &[ScheduledWorkflow],
    jobs_setting: u32,
    fail_fast: bool,
) -> DeferredExecution {
    let mut execution = DeferredExecution::default();
    let mut states = (0..plan.len())
        .map(|_| WorkflowState::default())
        .collect::<Vec<_>>();
    let mut impacts = Vec::<WriteImpact>::new();
    let mut stopped_tools = BTreeSet::new();

    let initial_indices = plan
        .iter()
        .enumerate()
        .filter_map(|(index, scheduled)| scheduled.check.as_ref().map(|_| index))
        .collect::<Vec<_>>();
    for (index, log) in run_checks(plan, &initial_indices, jobs_setting) {
        let outcome = record_check(
            &mut execution,
            &mut states[index],
            &plan[index],
            CommandPhase::InitialCheck,
            log,
            0,
        );
        states[index].initial_check = outcome;
        if outcome.is_none() && fail_fast {
            stopped_tools.insert(plan[index].tool_index);
        }
    }

    for (index, scheduled) in plan.iter().enumerate() {
        if states[index].operational {
            continue;
        }
        if states[index].last_check == Some(CheckOutcome::Clean)
            && invalidated_since(scheduled, &impacts, states[index].checked_at)
        {
            let log = run_check(scheduled, &check_command(scheduled));
            let outcome = record_check(
                &mut execution,
                &mut states[index],
                scheduled,
                CommandPhase::Recheck,
                log,
                impacts.len(),
            );
            if outcome.is_none() {
                if fail_fast {
                    stopped_tools.insert(scheduled.tool_index);
                }
                continue;
            }
        }
        let needs_remedy = states[index].last_check == Some(CheckOutcome::Issues)
            || (scheduled.check.is_none()
                && scheduled.compatibility_translation
                && scheduled.remedy.is_some());
        if !needs_remedy {
            continue;
        }
        let Some(remedy) = scheduled.remedy.as_ref() else {
            continue;
        };
        if stopped_tools.contains(&scheduled.tool_index) {
            states[index].operational = true;
            record_problem(
                &mut execution.result,
                scheduled,
                "remedy",
                CommandFailure {
                    message: "remedy skipped after an earlier failure of this tool under failFast"
                        .into(),
                    missing_tool: false,
                },
            );
            continue;
        }

        states[index].fix_attempted = true;
        let context = scheduled.context();
        let scope = write_scope(remedy.writes, &scheduled.job, &context);
        let before = Snapshot::read(&scope);
        let command = render_command(remedy, &scheduled.job, &context);
        let log = run_phase_command(remedy, &command, &scheduled.job.workspace_dir);
        let after_scope = write_scope(remedy.writes, &scheduled.job, &context);
        let after = Snapshot::read(&after_scope);
        let changed_files = before
            .changed_files(&after)
            .into_iter()
            .collect::<BTreeSet<_>>();
        states[index]
            .changed_files
            .extend(changed_files.iter().cloned());
        if !changed_files.is_empty() {
            impacts.push(WriteImpact {
                workspace: scheduled.job.workspace_dir.clone(),
                changed_files,
            });
        }
        let failed = command_failed(&log);
        execution
            .logs
            .push(deferred_log(scheduled, CommandPhase::Remedy, log));
        if let Some(failure) = failed {
            // A failed remedy is an operational problem, but the final check
            // that follows still decides the files: a compiler error that
            // makes `clippy --fix` fail must still block as a manual issue.
            // Only a remedy without a check has no other verdict.
            if scheduled.check.is_none() {
                states[index].operational = true;
            }
            record_problem(&mut execution.result, scheduled, "remedy", failure);
            if fail_fast {
                stopped_tools.insert(scheduled.tool_index);
            }
        }
    }

    let final_indices = plan
        .iter()
        .enumerate()
        .filter_map(|(index, scheduled)| {
            scheduled.check.as_ref()?;
            let state = &states[index];
            (state.fix_attempted || invalidated_since(scheduled, &impacts, state.checked_at))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    for (index, log) in run_checks(plan, &final_indices, jobs_setting) {
        record_check(
            &mut execution,
            &mut states[index],
            &plan[index],
            CommandPhase::FinalCheck,
            log,
            impacts.len(),
        );
    }

    for (index, scheduled) in plan.iter().enumerate() {
        let state = &states[index];
        // A user tool with only mutating phases (a formatter without a
        // verify phase) has no check to confirm its remedy, so the remedy's
        // own success is the verdict: files it changed are auto-fixed
        // (unverified) and the rest clean. It can never block.
        let remedy_only = scheduled.check.is_none();

        let mut report = ToolReport {
            id: scheduled.report_id(),
            tool_id: scheduled.spec.id.clone(),
            tool_name: scheduled.spec.display_name.clone(),
            workflow_id: scheduled.workflow_id.clone(),
            job_id: format!("{:03}", scheduled.job_index),
            candidate_files: scheduled.job.files.clone(),
            changed_files: state.changed_files.iter().cloned().collect(),
            initial_check: state.initial_check,
            fix_attempted: state.fix_attempted,
            final_check: state.last_check,
            conservative_attribution: false,
            issue_files: Vec::new(),
            out_of_scope_files: Vec::new(),
            unverified: remedy_only,
            artifact_ids: Vec::new(),
        };
        report.normalize();

        if state.operational {
            execution.result.reports.insert(report.id.clone(), report);
            continue;
        }
        if remedy_only {
            execution.result.record_report(report);
            continue;
        }
        if report.final_check.is_none() {
            record_problem(
                &mut execution.result,
                scheduled,
                "final-check",
                CommandFailure {
                    message: "workflow completed without an authoritative final check".into(),
                    missing_tool: false,
                },
            );
            execution.result.reports.insert(report.id.clone(), report);
            continue;
        }
        if report.final_check == Some(CheckOutcome::Issues) {
            attribute_issues(&mut report, &state.last_output, scheduled);
        }
        execution.result.record_report(report);
    }

    execution
        .logs
        .sort_by_key(|log| (log.tool_index, log.workflow_index, log.job_index, log.phase));
    execution
}

/// Attribute remaining issues to the candidate (or remedy-changed) files the
/// check output names; blame nothing in this run when it names only other
/// files; otherwise conservatively blame every candidate.
fn attribute_issues(report: &mut ToolReport, output: &str, scheduled: &ScheduledWorkflow) {
    let scope = report
        .candidate_files
        .iter()
        .chain(report.changed_files.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let bases = resolution_bases(&scheduled.job.workspace_dir, &scheduled.project_root);
    match attribute(output, &scope, &bases) {
        Attribution::Named(files) => report.issue_files = files,
        Attribution::OutOfScope(files) => report.out_of_scope_files = files,
        Attribution::Unnamed => {
            report.conservative_attribution = report.candidate_files.len() > 1;
            report.issue_files = report.candidate_files.clone();
        }
    }
}

fn record_check(
    execution: &mut DeferredExecution,
    state: &mut WorkflowState,
    scheduled: &ScheduledWorkflow,
    phase: CommandPhase,
    log: PhaseLog,
    checked_at: usize,
) -> Option<CheckOutcome> {
    let outcome = check_outcome(&log);
    state.checked_at = checked_at;
    match &outcome {
        Ok(outcome) => {
            state.last_check = Some(*outcome);
            state.last_output = combined_output(&log);
        }
        Err(_) => {
            state.last_check = None;
            state.operational = true;
        }
    }
    execution.logs.push(deferred_log(scheduled, phase, log));
    match outcome {
        Ok(outcome) => Some(outcome),
        Err(failure) => {
            record_problem(
                &mut execution.result,
                scheduled,
                command_phase_label(phase),
                failure,
            );
            None
        }
    }
}

fn command_phase_label(phase: CommandPhase) -> &'static str {
    match phase {
        CommandPhase::InitialCheck => "initial-check",
        CommandPhase::Recheck => "recheck",
        CommandPhase::Remedy => "remedy",
        CommandPhase::FinalCheck => "final-check",
        CommandPhase::Combined => "combined",
        CommandPhase::Configuration => "configuration",
    }
}

/// Combined standard output and error, in that order.
pub(crate) fn combined_output(log: &PhaseLog) -> String {
    match (log.stdout.trim().is_empty(), log.stderr.trim().is_empty()) {
        (false, false) => format!("{}\n{}", log.stdout.trim_end(), log.stderr),
        (false, true) => log.stdout.clone(),
        (true, _) => log.stderr.clone(),
    }
}

/// Identity of a check invocation: identical keys within one check stage
/// run once and share the result.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CheckKey {
    tool_index: usize,
    phase_id: String,
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
}

fn check_command(scheduled: &ScheduledWorkflow) -> RenderedCommand {
    let check = scheduled.check.as_ref().expect("scheduled check");
    render_command(check, &scheduled.job, &scheduled.context())
}

fn run_checks(
    plan: &[ScheduledWorkflow],
    indices: &[usize],
    jobs_setting: u32,
) -> Vec<(usize, PhaseLog)> {
    // Compatibility translation pairs every mutator with the same verifier,
    // so one stage can hold several identical commands for the same job.
    let mut leaders = Vec::<(usize, RenderedCommand)>::new();
    let mut positions = BTreeMap::<CheckKey, usize>::new();
    let mut members = Vec::<(usize, usize)>::new();
    for &index in indices {
        let scheduled = &plan[index];
        let command = check_command(scheduled);
        let key = CheckKey {
            tool_index: scheduled.tool_index,
            phase_id: scheduled
                .check
                .as_ref()
                .expect("scheduled check")
                .id
                .clone(),
            program: command.program.clone(),
            args: command.args.clone(),
            cwd: scheduled.job.workspace_dir.clone(),
        };
        let position = *positions.entry(key).or_insert_with(|| {
            leaders.push((index, command));
            leaders.len() - 1
        });
        members.push((index, position));
    }
    let logs = run_unique_checks(plan, &leaders, jobs_setting);
    let mut results = members
        .into_iter()
        .map(|(index, position)| (index, logs[position].clone()))
        .collect::<Vec<_>>();
    results.sort_by_key(|(index, _)| *index);
    results
}

fn run_unique_checks(
    plan: &[ScheduledWorkflow],
    leaders: &[(usize, RenderedCommand)],
    jobs_setting: u32,
) -> Vec<PhaseLog> {
    let worker_count = resolve_worker_count(jobs_setting, leaders.len());
    if worker_count <= 1 {
        return leaders
            .iter()
            .map(|(index, command)| run_check(&plan[*index], command))
            .collect();
    }
    let cursor = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(leaders.len()));
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let offset = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some((index, command)) = leaders.get(offset) else {
                        break;
                    };
                    let log = run_check(&plan[*index], command);
                    results
                        .lock()
                        .expect("deferred check mutex poisoned")
                        .push((offset, log));
                }
            });
        }
    });
    let mut results = results.into_inner().expect("deferred check mutex poisoned");
    results.sort_by_key(|(offset, _)| *offset);
    results.into_iter().map(|(_, log)| log).collect()
}

fn run_check(scheduled: &ScheduledWorkflow, command: &RenderedCommand) -> PhaseLog {
    let check = scheduled.check.as_ref().expect("scheduled check");
    run_phase_command(check, command, &scheduled.job.workspace_dir)
}

fn check_outcome(log: &PhaseLog) -> Result<CheckOutcome, CommandFailure> {
    if let Some(failure) = command_failed(log) {
        return Err(failure);
    }
    match log.classification {
        Some(PhaseStatus::Clean) => Ok(CheckOutcome::Clean),
        Some(PhaseStatus::Issues) => Ok(CheckOutcome::Issues),
        Some(PhaseStatus::Failure) | None => Err(CommandFailure {
            message: format!("{} produced no usable result", log.phase),
            missing_tool: false,
        }),
    }
}

fn command_failed(log: &PhaseLog) -> Option<CommandFailure> {
    if let Some(error) = &log.error {
        let missing_tool = error == "not found";
        let message = if missing_tool {
            format!("{} not found", log.program)
        } else {
            format!("{}: could not run {}: {error}", log.phase, log.program)
        };
        return Some(CommandFailure {
            message,
            missing_tool,
        });
    }
    let message = match (log.classification, log.status) {
        (Some(PhaseStatus::Clean | PhaseStatus::Issues), _) => return None,
        (_, Some(code)) => format!("{} failed with exit code {code}", log.phase),
        (_, None) => format!("{} was terminated by a signal", log.phase),
    };
    Some(CommandFailure {
        message,
        missing_tool: false,
    })
}

/// Whether any write recorded at or after `since` invalidates this check.
fn invalidated_since(scheduled: &ScheduledWorkflow, impacts: &[WriteImpact], since: usize) -> bool {
    impacts
        .get(since..)
        .unwrap_or_default()
        .iter()
        .any(|impact| check_invalidated(scheduled, impact))
}

fn check_invalidated(scheduled: &ScheduledWorkflow, impact: &WriteImpact) -> bool {
    match scheduled.check_scope {
        CheckScope::TargetFiles => impact
            .changed_files
            .iter()
            .any(|path| scheduled.job.files.contains(path)),
        CheckScope::Workspace => {
            impact.workspace == scheduled.job.workspace_dir
                || impact
                    .changed_files
                    .iter()
                    .any(|path| path.starts_with(&scheduled.job.workspace_dir))
        }
    }
}

fn record_problem(
    result: &mut DeferredRunResult,
    scheduled: &ScheduledWorkflow,
    phase: &str,
    failure: CommandFailure,
) {
    result.record_operational_problem(OperationalProblem {
        id: format!("{}-{phase}", scheduled.report_id()),
        tool_id: Some(scheduled.spec.id.clone()),
        tool_name: Some(scheduled.spec.display_name.clone()),
        missing_tool: failure.missing_tool,
        install_hint: failure
            .missing_tool
            .then(|| scheduled.spec.install_hint.clone())
            .flatten(),
        phase: Some(phase.into()),
        affected_files: scheduled.job.files.clone(),
        message: failure.message,
        artifact_ids: Vec::new(),
    });
}

fn deferred_log(scheduled: &ScheduledWorkflow, phase: CommandPhase, log: PhaseLog) -> DeferredLog {
    DeferredLog {
        tool_index: scheduled.tool_index,
        workflow_index: scheduled.workflow_index,
        job_index: scheduled.job_index,
        phase,
        log,
    }
}
