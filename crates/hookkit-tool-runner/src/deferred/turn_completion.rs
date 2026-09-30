//! The Stop-time flow: run the deferred plan on sealed activity, decide blocking, commit.

use super::artifacts::{prune_run_bundles, write_deferred_artifacts};
use super::disposition::{
    ActivityResolution, apply_deferred_state_disposition, plan_deferred_state_disposition,
    record_activity_resolution, source_gap_messages,
};
use super::plan::build_deferred_plan;
use super::summary::{
    BatchSummaryParts, BatchToolSummary, BlockMetadata, build_batch_summary, run_id,
};
use super::{
    ArtifactClassification, BlockReasons, CommandPhase, DEFAULT_BLOCK_REASON, DeferredReporter,
    DeferredRunResult, LoopGuardState, OperationalProblem, RenderedBuckets, RenderedMessages,
    RunArtifact, TemplateRun, decide_loop_guard, execute_deferred_workflows, issue_fingerprint,
    plan_stop_lowering,
};
use crate::convert::resolve_run_order;
use crate::errors::{activity_error, error_summary, invalid_data, state_error};
use crate::paths::{display_roots, normalize_path, state_root};
use crate::project_lock::lock_project;
use crate::vcs;
use hookkit_common::{TurnCompletionCommandEnvironment, TurnCompletionInput, TurnCompletionOutput};
use hookkit_core::{RuntimeContext, Utf8PathBuf};
use hookkit_file_activity::{
    FileActivityStore, PendingFileActivity, ReconciliationOptions, ResolveOptions, VcsFallback,
    reconcile, resolve_files,
};
use hookkit_pkl_config::schema as pkl;
use hookkit_session_state::{
    EntityOperationError, EntityOutcome, EntityView, FamilyId, RunBundle, SessionState,
    StateFamily, StateRoot, UtcTimestamp,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

const BATCHED_TOOLS_FAMILY: &str = "velvet-glove.batched-tools";

/// Loop-guard state file in the runner family's session scope.
const LOOP_GUARD_FILE: &str = "loop-guard.json";

/// Committed run bundles kept per session family; older ones are removed.
const RETAINED_RUN_BUNDLES: usize = 20;

/// Session state directories idle for longer than this are removed.
const STALE_SESSION_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Settings that decide blocking and lowering for one deferred run.
#[derive(Debug, Clone, Copy)]
struct CommitPolicy {
    lowering: pkl::LoweringPolicy,
    missing_tool: pkl::MissingToolPolicy,
    block_on_operational_errors: bool,
    max_consecutive_blocks: u32,
    coverage: pkl::CoverageGapPolicy,
}

impl CommitPolicy {
    fn new(settings: &pkl::Settings, coverage: pkl::CoverageGapPolicy) -> Self {
        Self {
            lowering: settings.lowering_policy,
            missing_tool: settings.missing_tool_policy,
            block_on_operational_errors: settings.deferred_reporting.block_on_operational_errors,
            max_consecutive_blocks: settings.deferred_reporting.max_consecutive_blocks,
            coverage,
        }
    }

    /// Manual issues always block. Missing executables block only under the
    /// `harness-block` missing-tool policy, other operational problems only
    /// when configured, and coverage gaps only under the strict policy.
    fn block_reasons(&self, result: &DeferredRunResult) -> BlockReasons {
        BlockReasons {
            manual: result.has_manual_fixes(),
            operational: result.operational_problems.values().any(|problem| {
                if problem.missing_tool {
                    self.missing_tool == pkl::MissingToolPolicy::HarnessBlock
                } else {
                    self.block_on_operational_errors
                }
            }),
            coverage: self.coverage == pkl::CoverageGapPolicy::Strict
                && !result.coverage_gaps.is_empty(),
        }
    }
}

/// Per-Stop session handles shared by every exit path.
struct DeferredSession<'a, 'c> {
    ctx: &'a RuntimeContext<'c>,
    activity_store: &'a FileActivityStore,
    runner_family: &'a StateFamily,
    /// The harness's `stop_hook_active` flag, when its Stop event has one.
    stop_hook_active: Option<bool>,
}

/// A sealed pending window with its resolved candidates, ready to commit.
struct DeferredCommit<'a, 'c> {
    session: DeferredSession<'a, 'c>,
    source: (usize, Vec<String>),
    candidates: Vec<PathBuf>,
    resolution: ActivityResolution,
}

/// The result of one run, ready for block decision and commit.
struct FinishedRun<'a> {
    project_root: &'a Path,
    display_roots: &'a [PathBuf],
    policy: CommitPolicy,
    result: DeferredRunResult,
    tools: Vec<BatchToolSummary>,
    rendered: RenderedMessages,
}

pub(crate) fn run_turn_completion_input(
    turn_completion: TurnCompletionInput,
    _environment: &TurnCompletionCommandEnvironment,
    ctx: &RuntimeContext<'_>,
    config_path: Option<&Path>,
    state_dir: Option<&Path>,
) -> hookkit_core::Result<TurnCompletionOutput> {
    let cwd = ctx
        .workspace_roots()
        .first()
        .map(|root| PathBuf::from(root.as_str()))
        .ok_or_else(|| invalid_data("turn-completion input has no workspace root".into()))?;
    let state_root = state_root(state_dir);
    let state = SessionState::ensure(ctx, state_root).map_err(state_error)?;
    let activity_store = FileActivityStore::from_state(state.clone()).map_err(activity_error)?;
    let runner_family = state
        .family(FamilyId::new(BATCHED_TOOLS_FAMILY, 1).map_err(state_error)?)
        .map_err(state_error)?;

    // Two concurrent stop hooks must not run formatters against the same
    // sealed window at once. Post-tool producers do not take this lock; exact
    // generation acknowledgement preserves observations appended while we run.
    let _runner_lock = runner_family
        .exclusive_lock("turn-completion")
        .map_err(state_error)?;
    let loaded = hookkit_pkl_config::discover_and_load(&cwd, config_path);
    let activity_settings = loaded
        .as_ref()
        .ok()
        .and_then(|loaded| loaded.config.settings.file_activity.clone())
        .unwrap_or_default();
    let mut reconciliation =
        ReconciliationOptions::new(ctx.workspace_roots().to_vec(), UtcTimestamp::now());
    reconciliation.filesystem_mtime = activity_settings.filesystem_mtime;
    reconciliation.vcs = match activity_settings.vcs {
        pkl::FileActivityVcsFallback::Disabled => VcsFallback::Disabled,
        pkl::FileActivityVcsFallback::GitDirty => VcsFallback::GitDirty,
    };
    reconciliation.timestamp_tolerance =
        Duration::from_millis(activity_settings.timestamp_tolerance_millis);
    reconciliation.max_entries = activity_settings.max_entries;
    reconciliation.ignored_directory_names = activity_settings
        .ignored_directory_names
        .iter()
        .cloned()
        .collect();
    reconcile(&activity_store, reconciliation).map_err(activity_error)?;
    let stop_hook_active = stop_hook_active(&turn_completion);
    activity_store
        .pending()
        .try_with_entity(|view| {
            run_turn_completion_view(
                DeferredSession {
                    ctx,
                    activity_store: &activity_store,
                    runner_family: &runner_family,
                    stop_hook_active,
                },
                loaded,
                &activity_settings,
                view,
            )
        })
        .map_err(|error| match error {
            EntityOperationError::State(error) => state_error(error),
            EntityOperationError::Operation(error) => error,
        })
}

/// Whether the harness reports that this Stop follows a Stop-hook block;
/// `None` when its Stop event carries no such flag (Antigravity).
fn stop_hook_active(input: &TurnCompletionInput) -> Option<bool> {
    let field = match input {
        TurnCompletionInput::Claude(input) => input.field("stop_hook_active"),
        TurnCompletionInput::Codex(input) => input.field("stop_hook_active"),
        _ => return None,
    };
    Some(field.and_then(serde_json::Value::as_bool).unwrap_or(false))
}

fn run_turn_completion_view(
    session: DeferredSession<'_, '_>,
    loaded: Result<hookkit_pkl_config::Loaded, hookkit_pkl_config::PklConfigError>,
    activity_settings: &pkl::FileActivitySettings,
    view: &EntityView<'_, PendingFileActivity>,
) -> hookkit_core::Result<EntityOutcome<TurnCompletionOutput>> {
    let ctx = session.ctx;
    if view.events().is_empty() {
        // Nothing to check, so this Stop is allowed. Without a native
        // `stop_hook_active` flag that ends any chain of continuations, so the
        // next Stop is not mistaken for one.
        if session.stop_hook_active.is_none() {
            end_inferred_block_chain(session.runner_family);
        }
        let lowering = plan_stop_lowering(
            ctx.harness(),
            false,
            None,
            None,
            pkl::LoweringPolicy::BestEffortWithWarnings,
        )?;
        return Ok(EntityOutcome::retain(lowering.finish()?));
    }
    let workspace_root = ctx
        .workspace_roots()
        .first()
        .map(|root| root.as_std_path().to_path_buf())
        .ok_or_else(|| invalid_data("turn-completion input has no workspace root".into()))?;
    let fallback_project_root = normalize_path(&workspace_root);

    let mut resolve_options = ResolveOptions::new(ctx.workspace_roots().to_vec());
    resolve_options.max_entries = activity_settings.max_entries;
    resolve_options.ignored_directory_names = activity_settings
        .ignored_directory_names
        .iter()
        .cloned()
        .collect();
    if let Ok(state_directory) =
        Utf8PathBuf::from_path_buf(session.activity_store.state().directory().into())
    {
        resolve_options.excluded_roots.insert(state_directory);
    }
    let resolved = resolve_files(view.state(), &resolve_options).map_err(activity_error)?;
    let mut resolution = ActivityResolution {
        not_applicable_files: resolved
            .not_applicable_files
            .into_iter()
            .map(|path| normalize_path(path.as_std_path()))
            .collect(),
        unresolved_targets: resolved.unresolved_targets,
        gap_messages: source_gap_messages(view),
        truncated: resolved.truncated,
    };
    let mut candidates = resolved
        .files
        .into_iter()
        .map(|path| normalize_path(path.as_std_path()))
        .collect::<Vec<_>>();
    // Evidence can name one file through different spellings (for example
    // macOS /var vs /private/var); canonical duplicates must not become
    // duplicate jobs on the same file.
    candidates.sort();
    candidates.dedup();
    // Build outputs and other Git-ignored paths are never lint candidates.
    let ignored = vcs::git_ignored_paths(&fallback_project_root, &candidates);
    if !ignored.is_empty() {
        candidates.retain(|path| !ignored.contains(path));
        resolution.not_applicable_files.extend(ignored);
    }
    let source = (
        view.events().len(),
        view.events()
            .iter()
            .map(|entry| entry.id().to_string())
            .collect::<Vec<_>>(),
    );
    let run = session
        .runner_family
        .start_run("turn-completion")
        .map_err(state_error)?;
    let commit = DeferredCommit {
        session,
        source,
        candidates,
        resolution,
    };
    let coverage = activity_settings.coverage_gap_policy;

    let loaded = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            let display_roots = display_roots(&fallback_project_root, &workspace_root);
            return commit.config_failure(
                run,
                FailureContext {
                    project_root: &fallback_project_root,
                    display_roots: &display_roots,
                    policy: CommitPolicy::new(&pkl::Settings::default(), coverage),
                },
                "configuration failed to load; checks were skipped",
                &error.to_string(),
            );
        }
    };
    let settings = &loaded.config.settings;
    let project_root = normalize_path(&loaded.project_root);
    let display_roots = display_roots(&project_root, &loaded.project_root);
    let failure = FailureContext {
        project_root: &project_root,
        display_roots: &display_roots,
        policy: CommitPolicy::new(settings, coverage),
    };
    let reporter = match DeferredReporter::new(&settings.deferred_reporting) {
        Ok(reporter) => reporter,
        Err(error) => {
            return commit.config_failure(
                run,
                failure,
                "reporting configuration is invalid; checks were skipped",
                &error.to_string(),
            );
        }
    };
    let planned = resolve_run_order(&loaded.config)
        .and_then(|tools| build_deferred_plan(&tools, &commit.candidates, &project_root, settings));
    let (plan, planned_tools) = match planned {
        Ok(planned) => planned,
        Err(error) => {
            return commit.config_failure(
                run,
                failure,
                "tool configuration is invalid; checks were skipped",
                &error.to_string(),
            );
        }
    };
    let mut execution = {
        let _project = lock_project(&project_root);
        execute_deferred_workflows(&plan, settings.jobs, settings.fail_fast)
    };
    let tools = write_deferred_artifacts(
        &mut |relative, contents| run.write_text(relative, contents).map_err(state_error),
        &plan,
        &planned_tools,
        &execution.logs,
        &mut execution.result,
    )?;
    let mut result = execution.result;
    record_activity_resolution(&mut result, &commit.resolution);
    record_uncovered_candidates(&mut result, &commit.candidates);

    reporter.apply_groups(&mut result, &project_root);
    let template_run_id = run_id(run.directory())?;
    let summary_path = run.directory().join("summary.json");
    let rendered = match reporter.render(
        &result,
        TemplateRun {
            id: &template_run_id,
            project_root: &project_root,
            summary_path: &summary_path,
            state_directory: commit.session.activity_store.state().directory(),
            directory: run.directory(),
            display_roots: &display_roots,
            blocks: failure.policy.block_reasons(&result),
        },
    ) {
        Ok(messages) => messages,
        Err(error) => record_reporting_failure(
            &run,
            &mut result,
            &commit.candidates,
            &error.to_string(),
            failure.policy,
        )?,
    };
    commit.finish(
        run,
        FinishedRun {
            project_root: &project_root,
            display_roots: &display_roots,
            policy: failure.policy,
            result,
            tools,
            rendered,
        },
    )
}

/// Clear the loop guard's chain count, keeping its fingerprint.
fn end_inferred_block_chain(runner_family: &StateFamily) {
    let Ok(scope) = runner_family.session_scope() else {
        return;
    };
    let path = scope.directory().join(LOOP_GUARD_FILE);
    let mut guard = LoopGuardState::load(&path);
    if guard.consecutive_blocks > 0 {
        guard.consecutive_blocks = 0;
        guard.save(&path);
    }
}

/// Where and how a configuration failure is reported.
#[derive(Clone, Copy)]
struct FailureContext<'a> {
    project_root: &'a Path,
    display_roots: &'a [PathBuf],
    policy: CommitPolicy,
}

impl DeferredCommit<'_, '_> {
    /// Commit an operational configuration failure without running tools.
    fn config_failure(
        self,
        run: RunBundle,
        failure: FailureContext<'_>,
        headline: &str,
        detail: &str,
    ) -> hookkit_core::Result<EntityOutcome<TurnCompletionOutput>> {
        let mut result = DeferredRunResult::default();
        let log_path = record_configuration_problem(
            &run,
            &mut result,
            &self.candidates,
            ("configuration", "config-error.log"),
            detail,
        )?;
        record_activity_resolution(&mut result, &self.resolution);
        let rendered = failure_messages(
            headline,
            detail,
            &log_path,
            failure.policy.block_on_operational_errors,
        );
        self.finish(
            run,
            FinishedRun {
                project_root: failure.project_root,
                display_roots: failure.display_roots,
                policy: failure.policy,
                result,
                tools: Vec::new(),
                rendered,
            },
        )
    }

    /// Decide whether to block (applying the loop guard), commit the run
    /// summary, then update pending state, the guard, and retained runs.
    fn finish(
        self,
        run: RunBundle,
        finished: FinishedRun<'_>,
    ) -> hookkit_core::Result<EntityOutcome<TurnCompletionOutput>> {
        let FinishedRun {
            project_root,
            display_roots,
            policy,
            result,
            tools,
            mut rendered,
        } = finished;
        let ctx = self.session.ctx;
        let blocks = policy.block_reasons(&result);
        let roots = display_roots
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>();
        let fingerprint = issue_fingerprint(&result, blocks, &roots);
        let guard_path = self
            .session
            .runner_family
            .session_scope()
            .map_err(state_error)?
            .directory()
            .join(LOOP_GUARD_FILE);
        let previous = LoopGuardState::load(&guard_path);
        // Without a native flag, a Stop right after a block is presumed to be
        // the agent's continuation, so the guard still bounds the chain.
        let decision = decide_loop_guard(
            &previous,
            self.session.stop_hook_active,
            blocks.any(),
            &fingerprint,
            policy.max_consecutive_blocks,
        );
        let stop_hook_active = decision.stop_hook_active;
        if let Some(note) = &decision.note {
            rendered.agent = None;
            rendered.user = Some(match rendered.user.take() {
                Some(user) => format!("{user}\n{note}"),
                None => note.clone(),
            });
        }
        if decision.block && rendered.agent.is_none() {
            rendered.agent = Some(format!(
                "{DEFAULT_BLOCK_REASON} Details: {}",
                run.directory().display()
            ));
        }
        let lowering = plan_stop_lowering(
            ctx.harness(),
            decision.block,
            rendered.user.as_deref(),
            rendered.agent.as_deref(),
            policy.lowering,
        )?;
        let disposition =
            plan_deferred_state_disposition(&result, &self.resolution, policy.missing_tool)?;
        let hard_failure = (policy.missing_tool == pkl::MissingToolPolicy::HardFailure)
            .then(|| missing_tool_messages(&result))
            .filter(|missing| !missing.is_empty());
        let summary = build_batch_summary(BatchSummaryParts {
            run: &run,
            project_root,
            state_directory: self.session.activity_store.state().directory(),
            harness: ctx.harness(),
            status: run_status(&result),
            rendered_messages: rendered,
            lowering: lowering.metadata.clone(),
            block: BlockMetadata {
                reasons: blocks,
                stop_hook_active,
                fingerprint,
                blocked: decision.block,
                guard_note: decision.note.clone(),
            },
            source: self.source,
            candidates: &self.candidates,
            tools,
            disposition: &disposition,
            result,
        })?;
        let run_id = summary.run.id.clone();
        let current_run = run.directory().to_path_buf();
        run.commit(&summary).map_err(state_error)?;
        if let Some(missing) = hard_failure {
            return Err(invalid_data(format!(
                "missingToolPolicy is hard-failure and a configured tool is missing: {missing}"
            )));
        }
        let output = lowering.finish()?;
        apply_deferred_state_disposition(self.session.activity_store, disposition, run_id)?;
        decision.next.save(&guard_path);
        if let Some(runs) = current_run.parent() {
            prune_run_bundles(runs, RETAINED_RUN_BUNDLES, &current_run);
        }
        let state_root = StateRoot::new(self.session.activity_store.state().state_root());
        let _ = SessionState::gc(&state_root, STALE_SESSION_AGE);
        Ok(EntityOutcome::acknowledge(output))
    }
}

/// Mark candidates that no workflow assessed and no operational problem
/// covers as uncovered (typically: no configured tool selects them).
pub(crate) fn record_uncovered_candidates(result: &mut DeferredRunResult, candidates: &[PathBuf]) {
    let operational_files = result
        .operational_problems
        .values()
        .flat_map(|problem| problem.affected_files.iter().cloned())
        .collect::<BTreeSet<_>>();
    for candidate in candidates {
        if !result.files.contains_key(candidate) && !operational_files.contains(candidate) {
            result.record_uncovered(candidate.clone());
        }
    }
}

fn missing_tool_messages(result: &DeferredRunResult) -> String {
    result
        .operational_problems
        .values()
        .filter(|problem| problem.missing_tool)
        .map(|problem| problem.message.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("; ")
}

fn run_status(result: &DeferredRunResult) -> &'static str {
    if result.has_operational_problems() {
        "operational-failure"
    } else if result.has_manual_fixes() {
        "issues"
    } else if !result.uncovered_files.is_empty() && result.files.is_empty() {
        "not-applicable"
    } else {
        "clean"
    }
}

fn record_reporting_failure(
    run: &RunBundle,
    result: &mut DeferredRunResult,
    candidates: &[PathBuf],
    detail: &str,
    policy: CommitPolicy,
) -> hookkit_core::Result<RenderedMessages> {
    let log_path = record_configuration_problem(
        run,
        result,
        candidates,
        ("reporting-configuration", "reporting-error.log"),
        detail,
    )?;
    Ok(failure_messages(
        "could not render its report",
        detail,
        &log_path,
        policy.block_on_operational_errors,
    ))
}

/// Write a configuration log and record it as an operational problem that
/// affects every candidate. `(id, file)` names the problem and its log.
fn record_configuration_problem(
    run: &RunBundle,
    result: &mut DeferredRunResult,
    candidates: &[PathBuf],
    (id, file): (&str, &str),
    detail: &str,
) -> hookkit_core::Result<PathBuf> {
    let log_path = run.write_text(file, detail).map_err(state_error)?;
    result.record_artifact(RunArtifact {
        id: id.into(),
        absolute_path: log_path.clone(),
        run_relative_path: file.into(),
        media_type: "text/plain; charset=utf-8".into(),
        tool_id: None,
        workflow_id: None,
        job_id: None,
        report_id: None,
        phase: CommandPhase::Configuration,
        classification: ArtifactClassification::ConfigurationError,
        exit_code: None,
        program: None,
        arguments: Vec::new(),
        working_directory: None,
        files: candidates.to_vec(),
        candidate_files: candidates.to_vec(),
        changed_files: Vec::new(),
        contents: detail.into(),
        output: String::new(),
    });
    result.record_operational_problem(OperationalProblem {
        id: id.into(),
        tool_id: None,
        tool_name: None,
        missing_tool: false,
        install_hint: None,
        phase: Some("configuration".into()),
        affected_files: candidates.to_vec(),
        message: detail.into(),
        artifact_ids: vec![id.into()],
    });
    Ok(log_path)
}

/// Terse user notice for a configuration or reporting failure. The agent
/// hears about it only when operational errors are configured to block.
fn failure_messages(headline: &str, detail: &str, log: &Path, blocking: bool) -> RenderedMessages {
    let message = format!(
        "velvet-glove {headline} ({}). Details: {}",
        error_summary(detail),
        log.display()
    );
    RenderedMessages {
        buckets: RenderedBuckets::default(),
        agent: blocking.then(|| message.clone()),
        user: Some(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deferred::{CoverageGap, FileAssessment, FileStatus};

    #[test]
    fn coverage_gap_policy_is_best_effort_by_default_and_strict_on_request() {
        let mut result = DeferredRunResult::default();
        result.record_file(FileAssessment::new("clean.rs", FileStatus::Clean));
        result.record_coverage_gap(CoverageGap {
            id: "gap".into(),
            target: None,
            message: "dynamic target".into(),
            retained: true,
        });

        let settings = pkl::Settings::default();
        let best_effort = CommitPolicy::new(&settings, pkl::CoverageGapPolicy::BestEffort);
        assert!(!best_effort.block_reasons(&result).any());
        let strict = CommitPolicy::new(&settings, pkl::CoverageGapPolicy::Strict);
        assert!(strict.block_reasons(&result).coverage);
    }

    #[test]
    fn operational_problems_block_only_by_policy() {
        let mut result = DeferredRunResult::default();
        for (id, missing_tool) in [("crashed", false), ("missing", true)] {
            result.record_operational_problem(OperationalProblem {
                id: id.into(),
                tool_id: Some(id.into()),
                tool_name: None,
                missing_tool,
                install_hint: None,
                phase: None,
                affected_files: Vec::new(),
                message: id.into(),
                artifact_ids: Vec::new(),
            });
        }
        let mut settings = pkl::Settings::default();
        let coverage = pkl::CoverageGapPolicy::BestEffort;
        assert!(
            !CommitPolicy::new(&settings, coverage)
                .block_reasons(&result)
                .any(),
            "operational problems notify without blocking by default"
        );
        settings.missing_tool_policy = pkl::MissingToolPolicy::HarnessBlock;
        assert!(
            CommitPolicy::new(&settings, coverage)
                .block_reasons(&result)
                .operational
        );
        settings.missing_tool_policy = pkl::MissingToolPolicy::UserNotice;
        settings.deferred_reporting.block_on_operational_errors = true;
        assert!(
            CommitPolicy::new(&settings, coverage)
                .block_reasons(&result)
                .operational
        );
        result.operational_problems.remove("crashed");
        assert!(
            !CommitPolicy::new(&settings, coverage)
                .block_reasons(&result)
                .any(),
            "missing tools follow missingToolPolicy, not blockOnOperationalErrors"
        );
    }
}
