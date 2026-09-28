//! Reusable runners for immediate post-tool and session-batched completion hooks.
#![deny(missing_docs)]
//!
//! The runner reads a harness-native post-tool-use event from stdin, loads the
//! Pkl-driven tool catalog through [`hookkit_pkl_config`], runs each tool in
//! the configured order, and lowers a unified result to the selected harness.
//! The completion runner consumes exact snapshots from
//! [`hookkit_session_state`] and commits detailed run bundles before deciding
//! whether a turn may stop.

mod check;
mod deferred;
mod excerpt;
mod vcs;

pub use check::{CheckError, CheckReport, CheckRequest, CheckStatus, run_check};
pub use deferred::{
    ArtifactClassification, CheckOutcome, CommandPhase, CoverageGap, DeferredRunResult,
    FileAssessment, FileResult, FileStatus, IssueExcerpt, OperationalProblem, ProblemSummary,
    RunArtifact, ToolReport, ToolReportRef,
};
use deferred::{
    Attribution, BlockReasons, DEFAULT_BLOCK_REASON, DeferredLog, DeferredReporter, LoopGuardState,
    RenderedBuckets, RenderedMessages, ScheduledWorkflow, StopLoweringMetadata, TemplateRun,
    attribute, combined_output, decide_loop_guard, execute_deferred_workflows, issue_fingerprint,
    plan_stop_lowering, resolution_bases, source_failure_files,
};

use globset::{Glob, GlobSet, GlobSetBuilder};
use hookkit_common::message::{DiagnosticArtifact, DiagnosticReport};
use hookkit_common::{
    PostToolUseCommandEnvironment, PostToolUseInput, PostToolUseOutput,
    TurnCompletionCommandEnvironment, TurnCompletionInput, TurnCompletionOutput, UserNotice,
};
use hookkit_core::{HarnessId, HookkitError, RuntimeContext, Utf8PathBuf};
use hookkit_file_activity::{
    FileActivityEvent, FileActivityStore, FileActivityTarget, PendingFileActivity,
    ReconciliationOptions, ResolveOptions, VcsFallback, observe_post_tool as observe_file_activity,
    reconcile, resolve_files,
};
use hookkit_pkl_config::schema as pkl;
use hookkit_runtime::artifacts::{ArtifactKey, ArtifactManager};
use hookkit_session_state::{
    EntityOperationError, EntityOutcome, EntityView, FamilyId, RunBundle, SessionState,
    StateFamily, StateRoot, UtcTimestamp,
};
use minijinja::Environment;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const BATCHED_TOOLS_FAMILY: &str = "velvet-glove.batched-tools";

// ----------------------------------------------------------------------------
// Public runtime types
// ----------------------------------------------------------------------------

/// A complete reusable hook CLI specification for one external tool.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// Stable identifier referenced by configuration and diagnostics.
    pub id: String,
    /// Human-readable name used in output templates.
    pub display_name: String,
    /// Default executable name or path.
    pub executable: String,
    /// Optional installation guidance shown when the executable is missing.
    pub install_hint: Option<String>,
    /// Include and exclusion globs used to select files.
    pub file_selection: FileSelection,
    /// Optional marker used to partition files into nearest workspaces.
    pub workspace_indicator: Option<String>,
    /// Where files with no workspace indicator above them run.
    pub workspace_fallback: WorkspaceFallback,
    /// Granularity used by the immediate pipeline and phase-derived workflows.
    pub phase_invocation: InvocationGranularity,
    /// Deferred workflows executed at turn completion.
    pub workflows: Vec<ToolWorkflow>,
    /// External commands executed in vector order.
    pub phases: Vec<ToolPhase>,
    /// User- and agent-facing output templates.
    pub messages: ToolMessages,
    /// Per-tool diagnostic directory override.
    pub diagnostics_directory: Option<String>,
    /// Whether this specification participates in execution.
    pub enabled: bool,
    /// Environment variables set for every command of this tool.
    pub env: Vec<(String, String)>,
    /// Wall-clock limit per command, or `None` for no limit.
    pub timeout: Option<Duration>,
    /// Project-local executable directories searched before `PATH`.
    pub local_bin_dirs: Vec<String>,
}

impl ToolSpec {
    /// Creates an enabled tool with no phases and default file/message settings.
    pub fn new(
        id: impl Into<String>,
        display_name: impl Into<String>,
        executable: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            display_name: display_name.into(),
            executable: executable.into(),
            install_hint: None,
            file_selection: FileSelection::default(),
            workspace_indicator: None,
            workspace_fallback: WorkspaceFallback::default(),
            phase_invocation: InvocationGranularity::default(),
            workflows: Vec::new(),
            phases: Vec::new(),
            messages: ToolMessages::default(),
            diagnostics_directory: None,
            enabled: true,
            env: Vec::new(),
            timeout: Some(Duration::from_secs(pkl::DEFAULT_COMMAND_TIMEOUT_SECONDS)),
            local_bin_dirs: Vec::new(),
        }
    }

    /// Sets installation guidance shown when the executable is unavailable.
    pub fn with_install_hint(mut self, hint: impl Into<String>) -> Self {
        self.install_hint = Some(hint.into());
        self
    }

    /// Replaces the tool's file selection.
    pub fn with_file_selection(mut self, file_selection: FileSelection) -> Self {
        self.file_selection = file_selection;
        self
    }

    /// Sets the marker used to partition files into nearest workspaces.
    pub fn with_workspace_indicator(mut self, indicator: impl Into<String>) -> Self {
        self.workspace_indicator = Some(indicator.into());
        self
    }

    /// Sets where files with no workspace indicator above them run.
    pub fn with_workspace_fallback(mut self, fallback: WorkspaceFallback) -> Self {
        self.workspace_fallback = fallback;
        self
    }

    /// Appends a phase to the execution order.
    pub fn with_phase(mut self, phase: ToolPhase) -> Self {
        self.phases.push(phase);
        self
    }

    /// Appends a deferred workflow to the execution order.
    pub fn with_workflow(mut self, workflow: ToolWorkflow) -> Self {
        self.workflows.push(workflow);
        self
    }

    /// Replaces the tool's output templates.
    pub fn with_messages(mut self, messages: ToolMessages) -> Self {
        self.messages = messages;
        self
    }
}

/// Where a tool runs a file with no workspace indicator between it and the
/// project root.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceFallback {
    /// Leave the file out: the tool needs its workspace.
    #[default]
    Skip,
    /// Run the file from the project root, as without an indicator.
    ProjectRoot,
}

/// One Stop-time non-mutating check and optional automatic remedy.
#[derive(Debug, Clone)]
pub struct ToolWorkflow {
    /// Stable workflow identifier.
    pub id: String,
    /// Read-only command used to detect issues.
    pub check: Option<ToolPhase>,
    /// Optional command used to repair detected issues.
    pub remedy: Option<ToolPhase>,
    /// Inputs whose changes invalidate a prior check.
    pub check_scope: CheckScope,
    /// Granularity used to divide selected files into invocations.
    pub invocation: InvocationGranularity,
    /// Whether this workflow was translated from legacy immediate phases.
    pub compatibility_translation: bool,
    /// Whether this workflow participates in deferred execution.
    pub enabled: bool,
}

/// Inputs whose writes invalidate a workflow's prior check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CheckScope {
    /// Only writes to workflow target files invalidate a prior check.
    #[default]
    TargetFiles,
    /// Any write in the workspace invalidates a prior check.
    Workspace,
}

/// How a workflow divides the selected files into invocations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InvocationGranularity {
    /// Invoke once for each selected file.
    PerFile,
    /// Invoke once for the selected file batch.
    #[default]
    Batch,
    /// Invoke once for each workspace partition.
    Workspace,
}

/// Include/exclude globs used to select modified files.
#[derive(Debug, Clone, Default)]
pub struct FileSelection {
    /// Inclusion globs evaluated relative to the project root.
    pub include: Vec<String>,
    /// Exclusion globs applied after inclusion.
    pub exclude: Vec<String>,
}

impl FileSelection {
    /// Creates a selection from inclusion patterns with no exclusions.
    pub fn include(patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            include: patterns.into_iter().map(Into::into).collect(),
            exclude: Vec::new(),
        }
    }

    /// Replaces the exclusion patterns and returns the selection.
    pub fn with_exclude(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.exclude = patterns.into_iter().map(Into::into).collect();
        self
    }
}

/// One external command phase.
#[derive(Debug, Clone)]
pub struct ToolPhase {
    /// Stable phase identifier.
    pub id: String,
    /// Semantic role of the phase.
    pub mode: PhaseMode,
    /// Per-phase executable override, or `None` to use [`ToolSpec::executable`].
    pub program: Option<String>,
    /// Argument template expanded for each job.
    pub args: Vec<CommandArgTemplate>,
    /// Exit-code classification.
    pub exit_codes: ExitCodePolicy,
    /// Whether non-empty standard output represents actionable issues.
    pub issues_on_stdout: bool,
    /// Paths the command may modify.
    pub writes: WriteBehavior,
    /// Literal values expanded by [`CommandArgTemplate::ExtraArgs`].
    pub extra_args: Vec<String>,
    /// Whether the phase participates in execution.
    pub enabled: bool,
}

impl ToolPhase {
    /// Creates an enabled phase with no arguments and failure-on-unexpected exit codes.
    pub fn new(id: impl Into<String>, mode: PhaseMode) -> Self {
        Self {
            id: id.into(),
            mode,
            program: None,
            args: Vec::new(),
            exit_codes: ExitCodePolicy::default(),
            issues_on_stdout: false,
            writes: WriteBehavior::None,
            extra_args: Vec::new(),
            enabled: true,
        }
    }

    /// Sets a phase-specific executable.
    pub fn with_program(mut self, program: impl Into<String>) -> Self {
        self.program = Some(program.into());
        self
    }

    /// Replaces the phase's argument template.
    pub fn with_args(mut self, args: impl IntoIterator<Item = CommandArgTemplate>) -> Self {
        self.args = args.into_iter().collect();
        self
    }

    /// Replaces the exit-code classification.
    pub fn with_exit_codes(mut self, exit_codes: ExitCodePolicy) -> Self {
        self.exit_codes = exit_codes;
        self
    }

    /// Declares the paths this phase may modify.
    pub fn with_writes(mut self, writes: WriteBehavior) -> Self {
        self.writes = writes;
        self
    }

    /// Replaces the literal values expanded by the extra-arguments token.
    pub fn with_extra_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.extra_args = args.into_iter().map(Into::into).collect();
        self
    }

    fn is_verifier(&self) -> bool {
        matches!(self.mode, PhaseMode::Verify | PhaseMode::CheckOnly)
    }
}

/// High-level phase purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseMode {
    /// Rewrite inputs into canonical formatting.
    Format,
    /// Apply automatic fixes.
    Fix,
    /// Verify inputs without expected modification.
    Verify,
    /// Run a read-only check whose issues are diagnostic.
    CheckOnly,
}

/// What a phase may write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteBehavior {
    /// The phase is not expected to modify files.
    None,
    /// The phase may modify only its target files.
    TargetFiles,
    /// The phase may modify any file selected by the tool globs.
    MatchingGlobs,
    /// The phase may modify any file in its workspace partition.
    Workspace,
}

/// Command argument template.
#[derive(Debug, Clone)]
pub enum CommandArgTemplate {
    /// A literal argument.
    Literal(String),
    /// Files selected for the current job.
    Files,
    /// The same files as [`CommandArgTemplate::Files`], but rewritten relative to
    /// the current workspace partition root (falling back to the absolute path
    /// for any file that lies outside that root).
    WorkspaceFiles,
    /// Root of the current workspace partition.
    Workspace,
    /// Full marker path that established the workspace partition.
    WorkspaceIndicator,
    /// Root associated with the discovered project configuration.
    ProjectRoot,
    /// Executable selected for the current tool command.
    ToolExecutable,
    /// Literal extra arguments configured on the phase.
    ExtraArgs,
}

impl CommandArgTemplate {
    /// Creates a literal argument template.
    pub fn literal(value: impl Into<String>) -> Self {
        Self::Literal(value.into())
    }
}

/// Exit-code classification for a phase.
#[derive(Debug, Clone)]
pub struct ExitCodePolicy {
    /// Exit codes indicating a clean result.
    pub clean: Vec<i32>,
    /// Exit codes indicating actionable issues rather than execution failure.
    pub issues: Vec<i32>,
    /// Exit codes indicating tool failure.
    pub failure: Vec<i32>,
    /// Classification for codes absent from all explicit lists.
    pub unexpected: UnexpectedExitPolicy,
}

impl Default for ExitCodePolicy {
    fn default() -> Self {
        Self {
            clean: vec![0],
            issues: Vec::new(),
            failure: Vec::new(),
            unexpected: UnexpectedExitPolicy::Failure,
        }
    }
}

impl ExitCodePolicy {
    /// Creates the default policy, where only zero is clean.
    pub fn clean() -> Self {
        Self::default()
    }

    /// Replaces the exit codes classified as issues.
    pub fn issues(mut self, codes: impl IntoIterator<Item = i32>) -> Self {
        self.issues = codes.into_iter().collect();
        self
    }

    /// Replaces the exit codes classified as failures.
    pub fn failure(mut self, codes: impl IntoIterator<Item = i32>) -> Self {
        self.failure = codes.into_iter().collect();
        self
    }

    /// Sets the classification for unlisted exit codes.
    pub fn unexpected(mut self, policy: UnexpectedExitPolicy) -> Self {
        self.unexpected = policy;
        self
    }
}

/// How to classify an exit code not listed in the policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnexpectedExitPolicy {
    /// Treat the result as an execution failure.
    Failure,
    /// Treat the result as actionable issues.
    Issues,
}

/// Tool-specific output templates.
#[derive(Debug, Clone)]
pub struct ToolMessages {
    /// Agent message when the tool changed files and left no issues.
    pub clean_changed_agent: String,
    /// Agent message when issues remain but files did not change.
    pub issues_agent: String,
    /// Agent message when files changed and issues remain.
    pub issues_changed_agent: String,
    /// Optional user message when the executable is unavailable.
    pub unavailable_user: Option<String>,
    /// Optional user message when tool execution fails.
    pub failed_user: Option<String>,
}

impl Default for ToolMessages {
    fn default() -> Self {
        Self {
            clean_changed_agent: pkl::default_clean_changed_agent(),
            issues_agent: pkl::default_issues_agent(),
            issues_changed_agent: pkl::default_issues_changed_agent(),
            unavailable_user: None,
            failed_user: None,
        }
    }
}

// ----------------------------------------------------------------------------
// Runner entry points
// ----------------------------------------------------------------------------

/// Execution options supplied by the Velvet Glove CLI.
#[derive(Debug, Clone)]
pub struct Cli {
    /// Harness whose native post-tool event is read from standard input.
    pub harness: HarnessId,
    /// Explicit Pkl file, or `None` to use layered discovery.
    pub config_path: Option<PathBuf>,
}

/// Execution options for the stop-time batch runner.
#[derive(Debug, Clone)]
pub struct TurnCompletionCli {
    /// Harness whose native turn-completion event is read from standard input.
    pub harness: HarnessId,
    /// Explicit Pkl file, or `None` to use layered discovery.
    pub config_path: Option<PathBuf>,
    /// Session-state directory override.
    pub state_dir: Option<PathBuf>,
}

/// Execution options for the library-owned precise session-start observer.
#[derive(Debug, Clone)]
pub struct SessionStartCli {
    /// Harness whose native session-start event is read from standard input.
    pub harness: HarnessId,
    /// Session-state directory override.
    pub state_dir: Option<PathBuf>,
}

/// Execution options for the quiet post-tool file-activity observer.
#[derive(Debug, Clone)]
pub struct FileActivityCli {
    /// Harness whose native post-tool event is read from standard input.
    pub harness: HarnessId,
    /// Session-state directory override.
    pub state_dir: Option<PathBuf>,
}

// ----------------------------------------------------------------------------
// Shared immediate post-tool path discovery
// ----------------------------------------------------------------------------

fn discover_modified_files(input: &PostToolUseInput, context: &RuntimeContext<'_>) -> Vec<PathBuf> {
    observe_file_activity(input, context)
        .evidence()
        .filter_map(|evidence| match &evidence.target {
            FileActivityTarget::Path { path, .. } => Some(normalize_path(path.as_std_path())),
            FileActivityTarget::Workspace { .. } => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
/// Run the full post-tool-use hook from parsed CLI args.
pub fn run_runner(cli: Cli) -> std::process::ExitCode {
    hookkit_runtime::aligned::run_aligned_event::<hookkit_runtime::aligned::PostToolUse, _>(
        cli.harness,
        move |input, environment, ctx| {
            run_post_tool_input(input, environment, ctx, cli.config_path.as_deref())
        },
    )
}

/// Run the bundled quiet PostToolUse file-activity observer.
pub fn run_file_activity_observer(cli: FileActivityCli) -> std::process::ExitCode {
    hookkit_runtime::aligned::run_aligned_event::<hookkit_runtime::aligned::PostToolUse, _>(
        cli.harness,
        move |input, _environment, ctx| {
            let report = observe_file_activity(&input, ctx);
            let state_root = state_root(cli.state_dir.as_deref());
            let store = FileActivityStore::ensure(ctx, state_root).map_err(activity_error)?;
            let invocation = String::from_utf8_lossy(ctx.raw().bytes());
            store
                .append_report(&invocation, &report)
                .map_err(activity_error)?;
            post_tool_no_op(ctx.harness())
        },
    )
}

fn post_tool_no_op(harness: &HarnessId) -> hookkit_core::Result<PostToolUseOutput> {
    match harness.as_str() {
        "claude-code" => Ok(PostToolUseOutput::Claude(
            hookkit_claude::protocol::PostToolUseOutput::no_op(),
        )),
        "codex" => Ok(PostToolUseOutput::Codex(
            hookkit_codex::protocol::PostToolUseOutput::no_op(),
        )),
        "antigravity" => Ok(PostToolUseOutput::Antigravity(
            hookkit_antigravity::PostToolUseOutput::default(),
        )),
        _ => Err(invalid_data(format!(
            "file-activity observer does not support {harness}"
        ))),
    }
}

/// Run the stop-time batch hook from parsed CLI args.
pub fn run_turn_completion_runner(cli: TurnCompletionCli) -> std::process::ExitCode {
    hookkit_runtime::aligned::run_aligned_event::<hookkit_runtime::aligned::TurnCompletion, _>(
        cli.harness,
        move |input, environment, ctx| {
            run_turn_completion_input(
                input,
                environment,
                ctx,
                cli.config_path.as_deref(),
                cli.state_dir.as_deref(),
            )
        },
    )
}

/// Run the small, library-owned lifecycle observer used when precise native
/// session-start timing is desired even before another stateful hook runs.
pub fn run_session_start_observer(cli: SessionStartCli) -> std::process::ExitCode {
    let state_dir = cli.state_dir;
    match cli.harness.as_str() {
        "claude-code" => hookkit_runtime::typed::run_typed::<
            hookkit_claude::protocol::SessionStart,
            _,
        >(move |_, _, ctx| {
            ensure_session_metadata(ctx, state_dir.as_deref())?;
            Ok(hookkit_claude::protocol::SessionStartOutput::no_op())
        }),
        "codex" => hookkit_runtime::typed::run_typed::<hookkit_codex::catalog::SessionStart, _>(
            move |_, _, ctx| {
                ensure_session_metadata(ctx, state_dir.as_deref())?;
                Ok(hookkit_codex::catalog::SessionStartOutput::no_op())
            },
        ),
        _ => std::process::ExitCode::from(1),
    }
}

fn ensure_session_metadata(
    ctx: &RuntimeContext<'_>,
    state_dir: Option<&Path>,
) -> hookkit_core::Result<()> {
    let root = state_root(state_dir);
    SessionState::ensure(ctx, root).map_err(state_error)?;
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchToolSummary {
    tool_id: String,
    file_count: usize,
    issues: bool,
    operational_failure: bool,
    artifacts: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchRunSummary {
    schema_version: u32,
    run: BatchRunIdentity,
    status: &'static str,
    source_entry_count: usize,
    source_entry_ids: Vec<String>,
    candidate_files: Vec<PathBuf>,
    counts: BatchCounts,
    clean_files: Vec<PathBuf>,
    auto_fixed_files: Vec<PathBuf>,
    manual_fix_files: Vec<PathBuf>,
    groups: Vec<BatchGroupSummary>,
    artifact_paths: Vec<PathBuf>,
    state_disposition: PlannedStateDisposition,
    block: BlockMetadata,
    rendered_messages: RenderedMessageMetadata,
    tools: Vec<BatchToolSummary>,
    result: DeferredRunResult,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchRunIdentity {
    id: String,
    project_root: PathBuf,
    summary_path: PathBuf,
    state_directory: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchCounts {
    clean: usize,
    auto_fixed: usize,
    manual_fixes_needed: usize,
    operational_errors: usize,
    uncovered: usize,
    not_applicable: usize,
    coverage_gaps: usize,
    out_of_scope: usize,
    groups: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchGroupSummary {
    id: String,
    display_name: String,
    files: Vec<PathBuf>,
    count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlannedStateDisposition {
    source: &'static str,
    retry_files: Vec<Utf8PathBuf>,
    retry_targets: Vec<FileActivityTarget>,
    retry_gaps: Vec<String>,
    handled_baseline_files: Vec<Utf8PathBuf>,
}

/// Why the run did or did not block, including the loop guard's view.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BlockMetadata {
    reasons: BlockReasons,
    stop_hook_active: bool,
    fingerprint: String,
    blocked: bool,
    guard_note: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenderedMessageMetadata {
    harness: String,
    lowering: StopLoweringMetadata,
    buckets: RenderedBuckets,
    user: Option<String>,
    agent: Option<String>,
    references_summary: bool,
}

struct BatchSummaryParts<'a> {
    run: &'a RunBundle,
    project_root: &'a Path,
    state_directory: &'a Path,
    harness: &'a HarnessId,
    status: &'static str,
    rendered_messages: RenderedMessages,
    lowering: StopLoweringMetadata,
    block: BlockMetadata,
    source: (usize, Vec<String>),
    candidates: &'a [PathBuf],
    tools: Vec<BatchToolSummary>,
    disposition: &'a DeferredStateDisposition,
    result: DeferredRunResult,
}

#[derive(Debug)]
struct ActivityResolution {
    not_applicable_files: BTreeSet<PathBuf>,
    unresolved_targets: Vec<FileActivityTarget>,
    gap_messages: BTreeSet<String>,
    truncated: bool,
}

#[derive(Debug, Clone)]
struct DeferredStateDisposition {
    retry_files: BTreeSet<Utf8PathBuf>,
    retry_targets: Vec<FileActivityTarget>,
    retry_gaps: BTreeSet<String>,
    handled_files: BTreeSet<Utf8PathBuf>,
}

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

fn run_turn_completion_input(
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

/// Absolute prefixes that excerpts rewrite to project-relative paths: the
/// canonical project root and the spelling the harness or config used.
fn display_roots(canonical: &Path, spelled: &Path) -> Vec<PathBuf> {
    let mut roots = vec![canonical.to_path_buf()];
    if spelled != canonical {
        roots.push(spelled.to_path_buf());
    }
    roots
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
fn record_uncovered_candidates(result: &mut DeferredRunResult, candidates: &[PathBuf]) {
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

/// Keep the newest `keep` run bundles (named `<millis>-...`) in `runs`,
/// always including `current`: the run just committed is never removed, even
/// if a backwards clock step gave it the oldest name.
fn prune_run_bundles(runs: &Path, keep: usize, current: &Path) {
    let Ok(entries) = std::fs::read_dir(runs) else {
        return;
    };
    let mut bundles = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| entry.path() != current)
        .filter_map(|entry| {
            let millis = entry
                .file_name()
                .to_str()?
                .split('-')
                .next()?
                .parse::<u128>()
                .ok()?;
            Some((millis, entry.path()))
        })
        .collect::<Vec<_>>();
    bundles.sort_by(|left, right| right.cmp(left));
    for (_, stale) in bundles.into_iter().skip(keep.saturating_sub(1)) {
        let _ = std::fs::remove_dir_all(stale);
    }
}

/// Advisory lock serializing the tool runs of every hook and `check`
/// invocation on one project, so two sessions' fixers never rewrite the same
/// files at once and neither session's before/after snapshots record the
/// other's writes as its own. Released on drop; best effort (no lock when the
/// lock file cannot be opened, and none off Unix).
struct ProjectLock {
    _file: Option<std::fs::File>,
}

fn lock_project(project_root: &Path) -> ProjectLock {
    let directory = std::env::temp_dir()
        .join("velvet-glove")
        .join("project-locks");
    let name = excerpt::fingerprint([project_root.to_string_lossy().as_bytes()]);
    let file = std::fs::create_dir_all(&directory).ok().and_then(|()| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(format!("{name}.lock")))
            .ok()
    });
    #[cfg(unix)]
    if let Some(file) = &file {
        use std::os::unix::io::AsRawFd;
        loop {
            // SAFETY: `flock` has no memory-safety preconditions; the
            // descriptor stays open for the lifetime of the guard.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0
                || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                break;
            }
        }
    }
    ProjectLock { _file: file }
}

#[derive(Debug)]
struct PlannedDeferredTool {
    index: usize,
    spec: Arc<ToolSpec>,
    files: Vec<PathBuf>,
}

fn build_deferred_plan(
    schemas: &[&pkl::ToolSpec],
    candidates: &[PathBuf],
    project_root: &Path,
    settings: &pkl::Settings,
) -> hookkit_core::Result<(Vec<ScheduledWorkflow>, Vec<PlannedDeferredTool>)> {
    let mut plan = Vec::new();
    let mut planned_tools = Vec::new();
    for (tool_index, schema) in schemas.iter().enumerate() {
        if !schema.enabled {
            continue;
        }
        for id in &schema.workflow_order {
            if !schema.workflows.contains_key(id) {
                return Err(invalid_data(format!(
                    "tool `{}` workflowOrder references unknown workflow `{id}`",
                    schema.id
                )));
            }
        }
        let spec = Arc::new(convert_tool_spec(schema, settings));
        let matcher = FileMatcher::new(&spec.file_selection)?;
        let files = candidates
            .iter()
            .filter(|path| matcher.matches(path, project_root))
            .cloned()
            .collect::<Vec<_>>();
        if files.is_empty() {
            continue;
        }
        let base_jobs = build_jobs(&files, project_root, &spec);
        if base_jobs.is_empty() {
            continue;
        }
        for (workflow_index, workflow) in spec.workflows.iter().enumerate() {
            if !workflow.enabled {
                continue;
            }
            if workflow.check.is_none() && !workflow.compatibility_translation {
                return Err(invalid_data(format!(
                    "tool `{}` workflow `{}` requires a non-mutating check",
                    spec.id, workflow.id
                )));
            }
            if workflow
                .check
                .as_ref()
                .is_some_and(|check| check.writes != WriteBehavior::None)
            {
                return Err(invalid_data(format!(
                    "tool `{}` workflow `{}` check must declare writes = none",
                    spec.id, workflow.id
                )));
            }
            if workflow
                .remedy
                .as_ref()
                .is_some_and(|remedy| remedy.writes == WriteBehavior::None)
            {
                return Err(invalid_data(format!(
                    "tool `{}` workflow `{}` remedy must declare a write scope",
                    spec.id, workflow.id
                )));
            }
            let jobs = invocation_jobs(&base_jobs, workflow.invocation);
            for (job_index, job) in jobs.into_iter().enumerate() {
                plan.push(ScheduledWorkflow {
                    tool_index,
                    workflow_index,
                    job_index,
                    spec: Arc::clone(&spec),
                    workflow_id: workflow.id.clone(),
                    check: workflow.check.clone(),
                    remedy: workflow.remedy.clone(),
                    check_scope: workflow.check_scope,
                    compatibility_translation: workflow.compatibility_translation,
                    job,
                    project_root: project_root.to_path_buf(),
                });
            }
        }
        planned_tools.push(PlannedDeferredTool {
            index: tool_index,
            spec,
            files,
        });
    }
    Ok((plan, planned_tools))
}

fn invocation_jobs(base_jobs: &[ToolJob], invocation: InvocationGranularity) -> Vec<ToolJob> {
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

/// Writes one text artifact at a run-relative path and returns its absolute
/// path: a session run bundle for hooks, a plain directory for `check`.
type ArtifactWriter<'a> = dyn FnMut(&str, &str) -> hookkit_core::Result<PathBuf> + 'a;

fn write_deferred_artifacts(
    write: &mut ArtifactWriter<'_>,
    plan: &[ScheduledWorkflow],
    tools: &[PlannedDeferredTool],
    logs: &[DeferredLog],
    result: &mut DeferredRunResult,
) -> hookkit_core::Result<Vec<BatchToolSummary>> {
    let mut tool_artifacts = BTreeMap::<usize, Vec<String>>::new();
    for log in logs {
        let scheduled = plan
            .iter()
            .find(|scheduled| {
                scheduled.tool_index == log.tool_index
                    && scheduled.workflow_index == log.workflow_index
                    && scheduled.job_index == log.job_index
            })
            .ok_or_else(|| invalid_data("deferred log has no scheduled workflow".into()))?;
        let report_id = scheduled.report_id();
        let changed_files = result
            .reports
            .get(&report_id)
            .map(|report| report.changed_files.clone())
            .unwrap_or_default();
        let candidate_files = scheduled.job.files.clone();
        let files = candidate_files
            .iter()
            .chain(changed_files.iter())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let phase = command_phase_name(log.phase);
        let tool_component = safe_artifact_component(&scheduled.spec.id);
        let workflow_component = safe_artifact_component(&scheduled.workflow_id);
        let relative = format!(
            "tools/{:03}-{tool_component}/workflows/{:03}-{workflow_component}/jobs/{:03}/{phase}.log",
            scheduled.tool_index, scheduled.workflow_index, scheduled.job_index,
        );
        let contents = format_deferred_artifact(log)?;
        let absolute = write(&relative, &contents)?;
        let artifact_id = format!("{report_id}-{phase}");
        attach_report_artifact(result, &report_id, &artifact_id);
        result.record_artifact(RunArtifact {
            id: artifact_id.clone(),
            absolute_path: absolute,
            run_relative_path: relative.clone().into(),
            media_type: "text/plain; charset=utf-8".into(),
            tool_id: Some(scheduled.spec.id.clone()),
            workflow_id: Some(scheduled.workflow_id.clone()),
            job_id: Some(format!("{:03}", scheduled.job_index)),
            report_id: Some(report_id),
            phase: log.phase,
            classification: artifact_classification(&log.log),
            exit_code: log.log.status,
            program: Some(log.log.program.clone()),
            arguments: log.log.arguments.clone(),
            working_directory: Some(scheduled.job.workspace_dir.clone()),
            files,
            candidate_files,
            changed_files,
            contents,
            output: combined_output(&log.log),
        });
        tool_artifacts
            .entry(scheduled.tool_index)
            .or_default()
            .push(relative);
    }

    let mut summaries = Vec::new();
    for tool in tools {
        let issues = result.reports.values().any(|report| {
            report.tool_id == tool.spec.id && report.final_check == Some(CheckOutcome::Issues)
        });
        let operational_failure = result
            .operational_problems
            .values()
            .any(|problem| problem.tool_id.as_deref() == Some(tool.spec.id.as_str()));
        summaries.push(BatchToolSummary {
            tool_id: tool.spec.id.clone(),
            file_count: tool.files.len(),
            issues,
            operational_failure,
            artifacts: tool_artifacts.remove(&tool.index).unwrap_or_default(),
        });
    }
    Ok(summaries)
}

fn attach_report_artifact(result: &mut DeferredRunResult, report_id: &str, artifact_id: &str) {
    if let Some(report) = result.reports.get_mut(report_id) {
        report.artifact_ids.push(artifact_id.into());
        report.artifact_ids.sort();
        report.artifact_ids.dedup();
    }
    for file in result.files.values_mut() {
        for report in &mut file.reports {
            if report.report_id == report_id {
                report.artifact_ids.push(artifact_id.into());
                report.artifact_ids.sort();
                report.artifact_ids.dedup();
            }
        }
    }
    for problem in result.operational_problems.values_mut() {
        if problem.id.starts_with(&format!("{report_id}-")) {
            problem.artifact_ids.push(artifact_id.into());
            problem.artifact_ids.sort();
            problem.artifact_ids.dedup();
        }
    }
}

fn format_deferred_artifact(log: &DeferredLog) -> hookkit_core::Result<String> {
    let argv = std::iter::once(log.log.program.as_str())
        .chain(log.log.arguments.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let argv = serde_json::to_string(&argv)
        .map_err(|error| invalid_data(format!("could not serialize command argv: {error}")))?;
    Ok(format!(
        "workflow_index: {}\njob_index: {}\ncommand_phase: {}\nargv: {argv}\n{}",
        log.workflow_index,
        log.job_index,
        command_phase_name(log.phase),
        format_logs(std::slice::from_ref(&log.log)),
    ))
}

fn artifact_classification(log: &PhaseLog) -> ArtifactClassification {
    if log.error.is_some() {
        return ArtifactClassification::SpawnError;
    }
    match log.classification {
        Some(PhaseStatus::Clean) => ArtifactClassification::Clean,
        Some(PhaseStatus::Issues) => ArtifactClassification::Issues,
        Some(PhaseStatus::Failure) => ArtifactClassification::Failure,
        None if log.status.is_none() => ArtifactClassification::Failure,
        None => ArtifactClassification::Unclassified,
    }
}

fn command_phase_name(phase: CommandPhase) -> &'static str {
    match phase {
        CommandPhase::InitialCheck => "initial-check",
        CommandPhase::Recheck => "recheck",
        CommandPhase::Remedy => "remedy",
        CommandPhase::FinalCheck => "final-check",
        CommandPhase::Combined => "combined",
        CommandPhase::Configuration => "configuration",
    }
}

fn safe_artifact_component(value: &str) -> String {
    let component = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if component.is_empty() {
        "unnamed".into()
    } else {
        component
    }
}

fn build_batch_summary(parts: BatchSummaryParts<'_>) -> hookkit_core::Result<BatchRunSummary> {
    let run_id = run_id(parts.run.directory())?;
    let summary_path = parts.run.directory().join("summary.json");
    let RenderedMessages {
        buckets,
        user,
        agent,
    } = parts.rendered_messages;
    let summary_text = summary_path.to_string_lossy();
    let references_summary = user
        .iter()
        .chain(agent.iter())
        .any(|message| message.contains(summary_text.as_ref()));
    let clean_files = files_with_status(&parts.result, FileStatus::Clean);
    let auto_fixed_files = files_with_status(&parts.result, FileStatus::AutoFixed);
    let manual_fix_files = files_with_status(&parts.result, FileStatus::ManualFixesNeeded);
    let mut grouped = BTreeMap::<String, Vec<PathBuf>>::new();
    for file in parts.result.files.values() {
        grouped
            .entry(file.group_id.clone())
            .or_default()
            .push(file.path.clone());
    }
    let groups = grouped
        .into_iter()
        .map(|(id, mut files)| {
            files.sort();
            files.dedup();
            BatchGroupSummary {
                display_name: if id == "other" {
                    "Other".into()
                } else {
                    id.clone()
                },
                id,
                count: files.len(),
                files,
            }
        })
        .collect::<Vec<_>>();
    let artifact_paths = parts
        .result
        .artifacts
        .values()
        .map(|artifact| artifact.absolute_path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let counts = BatchCounts {
        clean: clean_files.len(),
        auto_fixed: auto_fixed_files.len(),
        manual_fixes_needed: manual_fix_files.len(),
        operational_errors: parts.result.operational_problems.len(),
        uncovered: parts.result.uncovered_files.len(),
        not_applicable: parts.result.not_applicable_files.len(),
        coverage_gaps: parts.result.coverage_gaps.len(),
        out_of_scope: parts.result.out_of_scope_reports().count(),
        groups: groups.len(),
    };
    let state_disposition = PlannedStateDisposition {
        source: "acknowledge-sealed-window",
        retry_files: parts.disposition.retry_files.iter().cloned().collect(),
        retry_targets: parts.disposition.retry_targets.clone(),
        retry_gaps: parts.disposition.retry_gaps.iter().cloned().collect(),
        handled_baseline_files: parts.disposition.handled_files.iter().cloned().collect(),
    };
    let (source_entry_count, source_entry_ids) = parts.source;
    Ok(BatchRunSummary {
        schema_version: 2,
        run: BatchRunIdentity {
            id: run_id,
            project_root: parts.project_root.to_path_buf(),
            summary_path,
            state_directory: parts.state_directory.to_path_buf(),
        },
        status: parts.status,
        source_entry_count,
        source_entry_ids,
        candidate_files: parts.candidates.to_vec(),
        counts,
        clean_files,
        auto_fixed_files,
        manual_fix_files,
        groups,
        artifact_paths,
        state_disposition,
        block: parts.block,
        rendered_messages: RenderedMessageMetadata {
            harness: parts.harness.to_string(),
            lowering: parts.lowering,
            buckets,
            user,
            agent,
            references_summary,
        },
        tools: parts.tools,
        result: parts.result,
    })
}

fn files_with_status(result: &DeferredRunResult, status: FileStatus) -> Vec<PathBuf> {
    result
        .files
        .values()
        .filter(|file| file.status == status)
        .map(|file| file.path.clone())
        .collect()
}

fn source_gap_messages(view: &EntityView<'_, PendingFileActivity>) -> BTreeSet<String> {
    view.events()
        .iter()
        .filter_map(|record| match record.event() {
            FileActivityEvent::Gap(gap) => Some(gap.detail.clone()),
            FileActivityEvent::Retry(retry) if retry.target.is_none() => Some(retry.reason.clone()),
            FileActivityEvent::Evidence(_) | FileActivityEvent::Retry(_) => None,
        })
        .collect()
}

fn record_activity_resolution(result: &mut DeferredRunResult, resolution: &ActivityResolution) {
    for path in &resolution.not_applicable_files {
        result.record_not_applicable(path.clone());
    }
    for (index, target) in resolution.unresolved_targets.iter().enumerate() {
        let target = serde_json::to_string(target)
            .unwrap_or_else(|_| "unserializable file activity target".into());
        result.record_coverage_gap(CoverageGap {
            id: format!("unresolved-target-{index:03}"),
            target: Some(target.clone()),
            message: format!("file activity target could not be fully materialized: {target}"),
            retained: true,
        });
    }
    for (index, message) in resolution.gap_messages.iter().enumerate() {
        result.record_coverage_gap(CoverageGap {
            id: format!("source-gap-{index:03}"),
            target: None,
            message: message.clone(),
            retained: true,
        });
    }
    if resolution.truncated {
        result.record_coverage_gap(CoverageGap {
            id: "resolution-budget-exhausted".into(),
            target: None,
            message: "file activity target resolution exhausted its traversal budget".into(),
            retained: true,
        });
    }
}

fn plan_deferred_state_disposition(
    result: &DeferredRunResult,
    resolution: &ActivityResolution,
    missing_tool_policy: pkl::MissingToolPolicy,
) -> hookkit_core::Result<DeferredStateDisposition> {
    let mut retry_files = BTreeSet::new();
    for file in result.files.values() {
        if file.status == FileStatus::ManualFixesNeeded {
            retry_files.insert(utf8_activity_path(&file.path)?);
        }
    }
    for problem in result.operational_problems.values() {
        // Retrying cannot conjure a missing executable; under the default
        // notice-only policy the next edit of these files checks them again.
        if problem.missing_tool && missing_tool_policy == pkl::MissingToolPolicy::UserNotice {
            continue;
        }
        for path in &problem.affected_files {
            retry_files.insert(utf8_activity_path(path)?);
        }
    }

    let mut handled_files = BTreeSet::new();
    for file in result.files.values() {
        if matches!(file.status, FileStatus::Clean | FileStatus::AutoFixed) {
            handled_files.insert(utf8_activity_path(&file.path)?);
        }
    }
    // A missing exact path is itself a stable handled state. Recording it
    // prevents an opt-in Git-dirty fallback from resurrecting the same
    // deletion immediately after the source observation is discharged.
    for path in &resolution.not_applicable_files {
        handled_files.insert(utf8_activity_path(path)?);
    }
    handled_files.retain(|path| !retry_files.contains(path));

    let mut retry_targets = resolution.unresolved_targets.clone();
    retry_targets.sort();
    retry_targets.dedup();
    let mut retry_gaps = resolution.gap_messages.clone();
    if resolution.truncated {
        retry_gaps.insert("file activity target resolution exhausted its traversal budget".into());
    }
    Ok(DeferredStateDisposition {
        retry_files,
        retry_targets,
        retry_gaps,
        handled_files,
    })
}

fn apply_deferred_state_disposition(
    activity_store: &FileActivityStore,
    disposition: DeferredStateDisposition,
    run_id: String,
) -> hookkit_core::Result<()> {
    activity_store
        .requeue_exact("deferred-unresolved-file", disposition.retry_files)
        .map_err(activity_error)?;
    activity_store
        .requeue_targets("deferred-unresolved-target", disposition.retry_targets)
        .map_err(activity_error)?;
    activity_store
        .requeue_gaps("deferred-coverage-gap", disposition.retry_gaps)
        .map_err(activity_error)?;
    if disposition.handled_files.is_empty() {
        return Ok(());
    }
    let baseline_report = activity_store
        .record_handled_baselines(disposition.handled_files, run_id)
        .map_err(activity_error)?;
    if baseline_report.failures.is_empty() {
        return Ok(());
    }
    let failures = baseline_report
        .failures
        .iter()
        .map(|failure| format!("{}: {}", failure.path, failure.message))
        .collect::<Vec<_>>()
        .join("; ");
    Err(invalid_data(format!(
        "could not record all handled file baselines; source window retained: {failures}"
    )))
}

fn utf8_activity_path(path: &Path) -> hookkit_core::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(normalize_path(path)).map_err(|path| {
        invalid_data(format!(
            "deferred file activity path is not valid UTF-8: {}",
            path.display()
        ))
    })
}

fn run_id(directory: &Path) -> hookkit_core::Result<String> {
    directory
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            invalid_data(format!(
                "run directory has no UTF-8 id: {}",
                directory.display()
            ))
        })
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

fn state_error(error: hookkit_session_state::StateError) -> HookkitError {
    std::io::Error::other(error).into()
}

fn state_root(override_dir: Option<&Path>) -> StateRoot {
    StateRoot::new(override_dir.map_or_else(
        || std::env::temp_dir().join("velvet-glove").join("state"),
        Path::to_path_buf,
    ))
}

fn activity_error(error: hookkit_file_activity::FileActivityError) -> HookkitError {
    std::io::Error::other(error).into()
}

/// Run an exact aligned input through the Pkl-driven runner.
fn run_post_tool_input(
    post_tool: PostToolUseInput,
    _environment: &PostToolUseCommandEnvironment,
    ctx: &RuntimeContext<'_>,
    config_path: Option<&Path>,
) -> hookkit_core::Result<PostToolUseOutput> {
    let harness = ctx.harness();
    let lowering_warning_artifact = lowering_warning_artifact(&post_tool, ctx);
    let clean = || {
        lower_domain_outcome(
            harness,
            RunnerDomainOutcome::Clean,
            lowering_warning_artifact.as_ref(),
        )
    };

    // Most tool calls (Read, Grep, ...) touch no files: skip every other cost,
    // including Pkl evaluation, for them.
    let mut candidates = discover_modified_files(&post_tool, ctx)
        .into_iter()
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return clean();
    }

    let cwd = ctx
        .workspace_roots()
        .first()
        .map(|root| PathBuf::from(root.as_str()))
        .ok_or_else(|| invalid_data("post-tool-use input has no workspace root".into()))?;
    // As at Stop, build outputs and other Git-ignored paths are never lint
    // candidates; a call that touched only those costs no Pkl evaluation.
    let ignored = vcs::git_ignored_paths(&normalize_path(&cwd), &candidates);
    candidates.retain(|path| !ignored.contains(path));
    if candidates.is_empty() {
        return clean();
    }
    let loaded = match hookkit_pkl_config::discover_and_load(&cwd, config_path) {
        Ok(loaded) => loaded,
        // A broken policy is an operational problem: tell the user, never the
        // agent, and never fail or block the tool call.
        Err(error) => {
            let detail = error.to_string();
            let mut message = format!(
                "velvet-glove: configuration error; no tools ran ({})",
                error_summary(&detail)
            );
            if let Ok(log) =
                write_immediate_artifact(&immediate_log_directory(), "config-error", &detail, ctx)
            {
                message.push_str(&format!(". Details: {}", log.display()));
            }
            let output = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::default())
                .with_user_notice(UserNotice::error(message));
            return lower_domain_outcome(
                harness,
                RunnerDomainOutcome::Report(output),
                lowering_warning_artifact.as_ref(),
            );
        }
    };

    let project_root = normalize_path(&loaded.project_root);
    // A project's policy applies only to files inside the project: plans,
    // memory files, scratch files, and sibling repositories are left alone.
    candidates.retain(|path| path.starts_with(&project_root));
    if candidates.is_empty() {
        return clean();
    }
    let settings = &loaded.config.settings;
    let mut output = RunnerPostToolUseOutput::new(settings.lowering_policy).with_excerpt_limits(
        ExcerptLimits::new(
            &settings.deferred_reporting,
            display_roots(&project_root, &loaded.project_root),
        ),
    );
    let mut had_hard_failure: Option<String> = None;
    let mut had_harness_block_message: Option<String> = None;

    let tools = resolve_run_order(&loaded.config)?;
    if tools.is_empty() {
        return clean();
    }

    let global_diagnostics_dir = settings.diagnostics_directory.clone();

    let _project = lock_project(&project_root);
    for schema_spec in tools {
        if !schema_spec.enabled {
            continue;
        }
        let spec = convert_tool_spec(schema_spec, settings);
        let context = ToolContext {
            spec: &spec,
            project_root: &project_root,
            global_diagnostics_dir: global_diagnostics_dir.as_deref(),
        };
        // An error in one tool's configuration (a glob, a message template,
        // an unwritable diagnostics directory) is that tool's operational
        // problem: tell the user and keep everything already collected.
        let batch_status = run_immediate_tool(
            &context,
            &candidates,
            ctx,
            settings,
            &mut output,
            (&mut had_hard_failure, &mut had_harness_block_message),
        )
        .unwrap_or_else(|error| {
            output.notices.push(UserNotice::error(format!(
                "velvet-glove could not run {} ({error})",
                spec.display_name
            )));
            ToolBatchStatus {
                operational_failure: true,
                issues: false,
            }
        });

        if had_harness_block_message.is_some()
            || had_hard_failure.is_some()
            || (settings.fail_fast && batch_status.operational_failure)
            || (!settings.continue_after_issues && batch_status.issues)
        {
            break;
        }
    }

    let outcome = if let Some(message) = had_harness_block_message {
        RunnerDomainOutcome::HarnessBlock { message, output }
    } else if let Some(message) = had_hard_failure {
        RunnerDomainOutcome::OperationalFailure {
            message: format!("{message} (missingToolPolicy is hard-failure)"),
        }
    } else if is_empty_output(&output) {
        RunnerDomainOutcome::Clean
    } else {
        RunnerDomainOutcome::Report(output)
    };
    lower_domain_outcome(harness, outcome, lowering_warning_artifact.as_ref())
}

/// Run one tool on the candidates its globs select and fold its outcomes
/// into `output`. `flags` receive the run's hard-failure and harness-block
/// messages.
fn run_immediate_tool(
    context: &ToolContext<'_>,
    candidates: &[PathBuf],
    ctx: &RuntimeContext<'_>,
    settings: &pkl::Settings,
    output: &mut RunnerPostToolUseOutput,
    flags: (&mut Option<String>, &mut Option<String>),
) -> hookkit_core::Result<ToolBatchStatus> {
    let matcher = FileMatcher::new(&context.spec.file_selection)?;
    let runnable_paths = candidates
        .iter()
        .filter(|path| matcher.matches(path, context.project_root))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if runnable_paths.is_empty() {
        return Ok(ToolBatchStatus::default());
    }
    let base_jobs = build_jobs(&runnable_paths, context.project_root, context.spec);
    let jobs = invocation_jobs(&base_jobs, context.spec.phase_invocation);
    if jobs.is_empty() {
        return Ok(ToolBatchStatus::default());
    }
    let outcomes = run_jobs(&jobs, context, settings.jobs);
    let (had_hard_failure, had_harness_block_message) = flags;
    accumulate_outcomes(
        outcomes,
        context,
        ctx,
        settings.missing_tool_policy,
        output,
        had_hard_failure,
        had_harness_block_message,
    )
}

/// Accumulated common output produced by the post-tool runner.
///
/// Fields are runner-owned so callers receive this value through
/// [`RunnerDomainOutcome`] and lower it with the selected harness workflow.
#[derive(Debug, Default)]
pub struct RunnerPostToolUseOutput {
    notices: Vec<UserNotice>,
    agent_feedback: Vec<AgentFeedback>,
    diagnostics: Vec<DiagnosticReport>,
    auto_fixed: Vec<AutoFixed>,
    harness_block: Option<String>,
    lowering: pkl::LoweringPolicy,
    excerpt_limits: ExcerptLimits,
}

/// One agent-facing line: rendered, or a tool's remaining issues whose
/// excerpt is cut only once every tool has run, so all of them share the
/// excerpt budget fairly.
#[derive(Debug)]
enum AgentFeedback {
    Rendered(String),
    Issues(Box<PendingIssues>),
}

/// A tool's remaining issues, rendered through its `issuesAgent` or
/// `issuesChangedAgent` template once the excerpt is known.
#[derive(Debug)]
struct PendingIssues {
    template: String,
    /// Built-in template used when `template` fails to render.
    fallback: String,
    /// Which `messages` field `template` came from, for error notices.
    field: &'static str,
    tool: String,
    tool_id: String,
    project_root: PathBuf,
    changed_files: Vec<String>,
    issue_files: Vec<String>,
    diagnostics: PathBuf,
    output: String,
}

impl PendingIssues {
    fn render(&self, excerpt: &str, notices: &mut Vec<UserNotice>) -> String {
        let tool = TemplateTool {
            name: &self.tool,
            id: &self.tool_id,
            project_root: &self.project_root,
        };
        let args = MessageArgs {
            changed_files: &self.changed_files,
            issue_files: &self.issue_files,
            diagnostics_path: Some(&self.diagnostics),
            excerpt,
            ..MessageArgs::default()
        };
        render_with_fallback(
            &self.template,
            &self.fallback,
            self.field,
            &tool,
            &args,
            notices,
        )
    }
}

/// Agent excerpt limits shared by every tool in one immediate run, plus the
/// absolute prefixes that excerpts rewrite to project-relative paths. The
/// limits are the deferred reporter's (`deferredReporting.excerptMax*`) and
/// are divided among the tools that report issues exactly as at Stop.
#[derive(Debug, Default)]
struct ExcerptLimits {
    lines: usize,
    chars: usize,
    roots: Vec<PathBuf>,
}

impl ExcerptLimits {
    fn new(reporting: &pkl::DeferredReporting, roots: Vec<PathBuf>) -> Self {
        Self {
            lines: reporting.excerpt_max_lines as usize,
            chars: reporting.excerpt_max_chars as usize,
            roots,
        }
    }

    /// Bounded, ANSI-free, project-relative excerpts of `outputs`, one per
    /// output, sharing the limits; a cut excerpt points at its log.
    fn excerpts(&self, outputs: &[(&str, &Path)]) -> Vec<String> {
        let roots = self.roots.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        let normalized = outputs
            .iter()
            .map(|(output, _)| excerpt::normalize(output, &roots))
            .collect::<Vec<_>>();
        excerpt::clip_shared(&normalized, self.lines, self.chars)
            .iter()
            .zip(outputs)
            .map(|(clipped, (_, log))| {
                excerpt::with_log_note(clipped, Some(&log.to_string_lossy()))
            })
            .collect()
    }
}

/// Files one tool changed and left clean.
#[derive(Debug)]
struct AutoFixed {
    tool: String,
    files: Vec<String>,
    /// Whether the agent learns about it through the shared auto-fix line
    /// (the tool keeps the default `cleanChangedAgent` template).
    in_agent_line: bool,
}

/// Longest configuration or tool error summary echoed in a user notice.
const ERROR_SUMMARY_CHARS: usize = 300;

/// One-line summary of an error: its first line, plus the first informative
/// line after it when the first only introduces the detail (`pkl eval failed
/// for <file>:` followed by Pkl's `–– Pkl Error ––` banner and message).
fn error_summary(detail: &str) -> String {
    let mut lines = detail
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let first = lines.next().unwrap_or_default();
    let summary = match first.strip_suffix(':') {
        Some(head) => match lines.find(|line| !line.starts_with("––") && !line.starts_with("--"))
        {
            Some(next) => format!("{head}: {}", next.trim_start_matches("- ")),
            None => head.to_owned(),
        },
        None => first.to_owned(),
    };
    excerpt::clip(&summary, 1, ERROR_SUMMARY_CHARS).text
}

/// One terse line naming the auto-fixed files (at most
/// [`pkl::AUTO_FIXED_LISTED_FILES`], as at Stop) and the tools that changed
/// each: `velvet-glove auto-fixed a.py (Ruff), b.ts (Prettier); re-read
/// before editing.`
fn auto_fixed_line<'a>(entries: impl IntoIterator<Item = &'a AutoFixed>) -> Option<String> {
    let mut files = Vec::<(&str, Vec<&str>)>::new();
    for entry in entries {
        for file in &entry.files {
            match files.iter_mut().find(|(known, _)| known == file) {
                Some((_, tools)) if !tools.contains(&entry.tool.as_str()) => {
                    tools.push(&entry.tool);
                }
                Some(_) => {}
                None => files.push((file, vec![&entry.tool])),
            }
        }
    }
    (!files.is_empty()).then(|| {
        let listed = files
            .iter()
            .take(pkl::AUTO_FIXED_LISTED_FILES)
            .map(|(file, tools)| format!("{file} ({})", tools.join(", ")))
            .collect::<Vec<_>>()
            .join(", ");
        let more = match files.len().saturating_sub(pkl::AUTO_FIXED_LISTED_FILES) {
            0 => String::new(),
            more => format!(" and {more} more"),
        };
        format!("velvet-glove auto-fixed {listed}{more}; re-read before editing.")
    })
}

impl RunnerPostToolUseOutput {
    fn new(lowering: pkl::LoweringPolicy) -> Self {
        Self {
            lowering,
            ..Self::default()
        }
    }

    fn with_user_notice(mut self, notice: UserNotice) -> Self {
        self.notices.push(notice);
        self
    }

    #[cfg(test)]
    fn with_agent_feedback(mut self, feedback: impl Into<String>) -> Self {
        self.agent_feedback
            .push(AgentFeedback::Rendered(feedback.into()));
        self
    }

    #[cfg(test)]
    fn with_diagnostic_report(mut self, report: DiagnosticReport) -> Self {
        self.diagnostics.push(report);
        self
    }

    fn with_harness_block(mut self, message: impl Into<String>) -> Self {
        self.harness_block = Some(message.into());
        self
    }

    #[cfg(test)]
    fn with_auto_fixed(mut self, auto_fixed: AutoFixed) -> Self {
        self.auto_fixed.push(auto_fixed);
        self
    }

    fn with_excerpt_limits(mut self, limits: ExcerptLimits) -> Self {
        self.excerpt_limits = limits;
        self
    }

    /// Render every pending issue message, dividing the excerpt budget
    /// among them. Returns the agent-facing lines in order.
    fn rendered_agent_feedback(&mut self) -> Vec<String> {
        let feedback = std::mem::take(&mut self.agent_feedback);
        let pending = feedback
            .iter()
            .filter_map(|entry| match entry {
                AgentFeedback::Issues(issues) => {
                    Some((issues.output.as_str(), issues.diagnostics.as_path()))
                }
                AgentFeedback::Rendered(_) => None,
            })
            .collect::<Vec<_>>();
        let mut excerpts = self.excerpt_limits.excerpts(&pending).into_iter();
        feedback
            .iter()
            .map(|entry| match entry {
                AgentFeedback::Rendered(text) => text.clone(),
                AgentFeedback::Issues(issues) => {
                    let excerpt = excerpts.next().unwrap_or_default();
                    issues.render(&excerpt, &mut self.notices)
                }
            })
            .collect()
    }
}

/// Runner-owned semantic result. Tool policy and classification deliberately do
/// not leak into core/common crates.
#[derive(Debug)]
pub enum RunnerDomainOutcome {
    /// No messages, diagnostics, or block decision were produced.
    Clean,
    /// Common output should be lowered to the selected harness.
    Report(RunnerPostToolUseOutput),
    /// The configured policy requests a harness-native block decision.
    HarnessBlock {
        /// Reason presented through the harness decision mechanism.
        message: String,
        /// Additional notices, feedback, and diagnostics to lower.
        output: RunnerPostToolUseOutput,
    },
    /// Runner execution failed independently of tool-reported issues.
    OperationalFailure {
        /// Human-readable failure diagnostic.
        message: String,
    },
    /// The selected harness cannot represent or execute this workflow.
    UnsupportedHarness {
        /// Selected harness identifier.
        harness: String,
        /// Explanation of the unsupported behavior.
        reason: String,
    },
}

#[derive(Debug)]
struct LoweringWarningArtifact {
    directory: PathBuf,
    key: ArtifactKey,
}

fn lowering_warning_artifact(
    input: &PostToolUseInput,
    ctx: &RuntimeContext<'_>,
) -> Option<LoweringWarningArtifact> {
    let PostToolUseInput::Antigravity(input) = input else {
        return None;
    };
    let directory = PathBuf::from(ctx.artifact_directory()?.as_str());
    Some(LoweringWarningArtifact {
        directory,
        key: runner_artifact_key(
            ctx,
            format!("post-tool-use-step-{}-lowering-warning", input.step_idx),
        ),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoweringWarningRecord<'a> {
    format_version: u8,
    kind: &'static str,
    harness: &'static str,
    event: &'static str,
    lowering_policy: &'static str,
    unavailable: LoweringWarningMessages<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoweringWarningMessages<'a> {
    user_notices: &'a [UserNotice],
    diagnostics: Vec<LoweringWarningDiagnostic>,
    agent_feedback: &'a [String],
    rendered_user: &'a [String],
    rendered_agent: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoweringWarningDiagnostic {
    title: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact: Option<LoweringWarningDiagnosticArtifact>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoweringWarningDiagnosticArtifact {
    absolute_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_relative_path: Option<String>,
    media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
}

fn record_antigravity_lowering_warning(
    target: Option<&LoweringWarningArtifact>,
    output: &RunnerPostToolUseOutput,
    agent_feedback: &[String],
    rendered_user: &[String],
    rendered_agent: &str,
) -> hookkit_core::Result<PathBuf> {
    let target = target.ok_or_else(|| {
        invalid_data(
            "cannot record Antigravity PostToolUse lowering loss: exact input has no artifact directory"
                .into(),
        )
    })?;
    let diagnostics = output
        .diagnostics
        .iter()
        .map(|diagnostic| LoweringWarningDiagnostic {
            title: diagnostic.title.clone(),
            text: diagnostic.text.clone(),
            artifact: diagnostic.artifact.as_ref().map(|artifact| {
                LoweringWarningDiagnosticArtifact {
                    absolute_path: artifact.absolute_path.to_string_lossy().into_owned(),
                    project_relative_path: artifact
                        .project_relative_path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    media_type: artifact.media_type.clone(),
                    summary: artifact.summary.clone(),
                }
            }),
        })
        .collect();
    let record = LoweringWarningRecord {
        format_version: 1,
        kind: "post-tool-use-lowering-loss",
        harness: "antigravity",
        event: "PostToolUse",
        lowering_policy: "best-effort-with-warnings",
        unavailable: LoweringWarningMessages {
            user_notices: &output.notices,
            diagnostics,
            agent_feedback,
            rendered_user,
            rendered_agent,
        },
    };
    let value = serde_json::to_value(record)?;
    let manager = ArtifactManager::new(&target.directory).map_err(|error| {
        invalid_data(format!(
            "cannot create Antigravity lowering-warning artifact directory {}: {error}",
            target.directory.display()
        ))
    })?;
    manager
        .write_json_unique(&target.key, &value)
        .map_err(|error| {
            invalid_data(format!(
                "cannot write Antigravity lowering-warning artifact in {}: {error}",
                target.directory.display()
            ))
        })
}

fn lower_domain_outcome(
    harness: &HarnessId,
    outcome: RunnerDomainOutcome,
    lowering_warning_artifact: Option<&LoweringWarningArtifact>,
) -> hookkit_core::Result<PostToolUseOutput> {
    match outcome {
        RunnerDomainOutcome::Clean => lower_report(
            harness,
            RunnerPostToolUseOutput::default(),
            lowering_warning_artifact,
        ),
        RunnerDomainOutcome::Report(output) => {
            lower_report(harness, output, lowering_warning_artifact)
        }
        RunnerDomainOutcome::HarnessBlock { message, output } => lower_report(
            harness,
            output.with_harness_block(message),
            lowering_warning_artifact,
        ),
        RunnerDomainOutcome::OperationalFailure { message } => Err(invalid_data(message)),
        RunnerDomainOutcome::UnsupportedHarness { harness, reason } => Err(invalid_data(format!(
            "post-tool-use runner does not support {harness}: {reason}"
        ))),
    }
}

fn lower_report(
    harness: &HarnessId,
    mut output: RunnerPostToolUseOutput,
    lowering_warning_artifact: Option<&LoweringWarningArtifact>,
) -> hookkit_core::Result<PostToolUseOutput> {
    // Agent: the shared auto-fix line (tools on the default template) plus
    // each tool's own feedback. Rendering may add notices, so it comes first.
    let agent_feedback = output.rendered_agent_feedback();
    let context = auto_fixed_line(output.auto_fixed.iter().filter(|entry| entry.in_agent_line))
        .into_iter()
        .chain(agent_feedback.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n");

    if let Some(message) = output.harness_block.take() {
        // A block replaces the normal channels, so it carries what the agent
        // would otherwise have been told: files earlier tools rewrote must be
        // re-read before the next edit.
        let message = [context.as_str(), message.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return match harness.as_str() {
            "claude-code" => Ok(PostToolUseOutput::Claude(
                hookkit_claude::protocol::PostToolUseOutput::feedback_error(message),
            )),
            "codex" => Ok(PostToolUseOutput::Codex(
                hookkit_codex::protocol::PostToolUseOutput::blocking_error(message),
            )),
            _ => Err(invalid_data(format!(
                "post-tool-use runner does not support {harness}"
            ))),
        };
    }

    // User: one terse auto-fix line plus one line per notice, through the
    // native user-only `systemMessage`. Full diagnostics stay in their files.
    let auto_fixed = auto_fixed_line(&output.auto_fixed);
    let user_lines = auto_fixed
        .iter()
        .cloned()
        .chain(output.notices.iter().map(format_notice))
        .collect::<Vec<_>>();
    let user_message = user_lines.join("\n");

    match harness.as_str() {
        "claude-code" => {
            let native = if context.is_empty() {
                hookkit_claude::protocol::PostToolUseOutput::no_op()
            } else {
                hookkit_claude::protocol::PostToolUseOutput::with_context(context)
            };
            Ok(PostToolUseOutput::Claude(if user_message.is_empty() {
                native
            } else {
                native.with_system_message(user_message)?
            }))
        }
        "codex" => {
            let native = if context.is_empty() {
                hookkit_codex::protocol::PostToolUseOutput::no_op()
            } else {
                hookkit_codex::protocol::PostToolUseOutput::with_context(context)
            };
            Ok(PostToolUseOutput::Codex(if user_message.is_empty() {
                native
            } else {
                native.with_system_message(user_message)?
            }))
        }
        "antigravity" => {
            let rendered_user = user_lines
                .into_iter()
                .chain(output.diagnostics.iter().map(format_diagnostic))
                .collect::<Vec<_>>();
            if !context.is_empty() && output.lowering == pkl::LoweringPolicy::Strict {
                return Err(invalid_data(
                    "antigravity PostToolUse has no structured agent-only message channel".into(),
                ));
            }
            let mut native = hookkit_antigravity::PostToolUseOutput::default();
            if output.lowering == pkl::LoweringPolicy::BestEffortWithWarnings
                && (!rendered_user.is_empty() || !context.is_empty())
            {
                let path = record_antigravity_lowering_warning(
                    lowering_warning_artifact,
                    &output,
                    &agent_feedback,
                    &rendered_user,
                    &context,
                )?;
                native = native.with_protocol_stderr(format!(
                    "hookkit: Antigravity PostToolUse could not represent user/agent messages; full lowering record: {}",
                    path.display()
                ))?;
            }
            Ok(PostToolUseOutput::Antigravity(native))
        }
        _ => Err(invalid_data(format!(
            "post-tool-use runner does not support {harness}"
        ))),
    }
}

/// A user notice as one line attributed to velvet-glove, worded like the
/// Stop notices rather than tagged with a severity.
fn format_notice(notice: &UserNotice) -> String {
    if notice.text.starts_with("velvet-glove") {
        notice.text.clone()
    } else {
        format!("velvet-glove: {}", notice.text)
    }
}

fn format_diagnostic(diagnostic: &DiagnosticReport) -> String {
    let mut rendered = format!("{}:\n{}", diagnostic.title, diagnostic.text.trim());
    if let Some(artifact) = &diagnostic.artifact {
        rendered.push_str(&format!("\nartifact: {}", artifact.absolute_path.display()));
    }
    rendered
}

fn is_empty_output(output: &RunnerPostToolUseOutput) -> bool {
    output.notices.is_empty()
        && output.agent_feedback.is_empty()
        && output.diagnostics.is_empty()
        && output.auto_fixed.is_empty()
        && output.harness_block.is_none()
}

/// Resolve the `run` list to ordered tool specs. Loading already rejects a
/// `run` entry naming no tool, so a missing one is an internal invariant
/// violation rather than a user error.
fn resolve_run_order(config: &pkl::RunnerConfig) -> hookkit_core::Result<Vec<&pkl::ToolSpec>> {
    let mut tools = Vec::with_capacity(config.run.len());
    for id in &config.run {
        let Some(spec) = config.tools.get(id) else {
            return Err(invalid_data(format!(
                "internal error: validated run list names unknown tool `{id}`"
            )));
        };
        tools.push(spec);
    }
    Ok(tools)
}

#[derive(Debug, Clone, Copy, Default)]
struct ToolBatchStatus {
    operational_failure: bool,
    issues: bool,
}

/// Convert a Pkl-shaped tool spec to the runtime execution type.
///
/// `ExtraArgs` expands to the tool's `extraArgs`, then the workflow's (for
/// explicit workflows), then the phase's or command's own.
fn convert_tool_spec(spec: &pkl::ToolSpec, settings: &pkl::Settings) -> ToolSpec {
    let phases: Vec<ToolPhase> = ordered_phases(spec)
        .into_iter()
        .map(|(id, phase)| {
            let mut converted = convert_phase((id, phase));
            converted.extra_args = concat_args(&[&spec.extra_args, &phase.extra_args]);
            converted
        })
        .collect();

    let mut exclude = settings.exclude.clone();
    exclude.extend(spec.files.exclude.clone());
    let workflows = convert_workflows(spec, &phases);
    let timeout_seconds = spec
        .timeout_seconds
        .unwrap_or(settings.command_timeout_seconds);

    ToolSpec {
        id: spec.id.clone(),
        display_name: spec.display_name.clone(),
        executable: spec.executable.clone(),
        install_hint: spec.install_hint.clone(),
        file_selection: FileSelection {
            include: spec.files.include.clone(),
            exclude,
        },
        workspace_indicator: spec.workspace_indicator.clone(),
        workspace_fallback: match spec.workspace_fallback {
            pkl::WorkspaceFallback::Skip => WorkspaceFallback::Skip,
            pkl::WorkspaceFallback::ProjectRoot => WorkspaceFallback::ProjectRoot,
        },
        phase_invocation: convert_invocation(spec.phase_invocation),
        workflows,
        phases,
        messages: convert_messages(&spec.messages),
        diagnostics_directory: spec.diagnostics.directory.clone(),
        enabled: spec.enabled,
        env: spec
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        timeout: (timeout_seconds > 0).then(|| Duration::from_secs(timeout_seconds)),
        local_bin_dirs: settings.local_bin_dirs.clone(),
    }
}

fn concat_args(parts: &[&[String]]) -> Vec<String> {
    parts.iter().flat_map(|part| part.iter().cloned()).collect()
}

fn convert_workflows(spec: &pkl::ToolSpec, phases: &[ToolPhase]) -> Vec<ToolWorkflow> {
    if !spec.workflows.is_empty() {
        return ordered_workflows(spec)
            .into_iter()
            .map(|(id, workflow)| ToolWorkflow {
                id: id.clone(),
                check: workflow.check.as_ref().map(|command| {
                    let mut check =
                        convert_workflow_command(format!("{id}.check"), command, PhaseMode::Verify);
                    check.extra_args =
                        concat_args(&[&spec.extra_args, &workflow.extra_args, &command.extra_args]);
                    check
                }),
                remedy: workflow.remedy.as_ref().map(|command| {
                    let mut remedy =
                        convert_workflow_command(format!("{id}.remedy"), command, PhaseMode::Fix);
                    remedy.extra_args =
                        concat_args(&[&spec.extra_args, &workflow.extra_args, &command.extra_args]);
                    remedy
                }),
                check_scope: match workflow.check_scope {
                    pkl::CheckScope::TargetFiles => CheckScope::TargetFiles,
                    pkl::CheckScope::Workspace => CheckScope::Workspace,
                },
                invocation: convert_invocation(workflow.invocation),
                compatibility_translation: false,
                enabled: workflow.enabled,
            })
            .collect();
    }

    // Compatibility translation for the existing immediate-runner phase
    // shape. Every mutator becomes a separate deferred workflow paired with
    // the last enabled verifier. A tool with no verifier (a user-defined
    // formatter) gets check-less workflows whose remedy result is reported
    // as an unverified auto-fix; builtin validation forbids that shape.
    let verifier = phases
        .iter()
        .rev()
        .find(|phase| phase.enabled && phase.is_verifier())
        .cloned();
    let mut workflows = phases
        .iter()
        .filter(|phase| phase.enabled && !phase.is_verifier())
        .map(|remedy| ToolWorkflow {
            id: remedy.id.clone(),
            check: verifier.clone(),
            remedy: Some(remedy.clone()),
            check_scope: if spec.workspace_indicator.is_some()
                && !remedy.args.iter().any(|arg| {
                    matches!(
                        arg,
                        CommandArgTemplate::Files | CommandArgTemplate::WorkspaceFiles
                    )
                }) {
                CheckScope::Workspace
            } else {
                CheckScope::TargetFiles
            },
            invocation: convert_invocation(spec.phase_invocation),
            compatibility_translation: true,
            enabled: true,
        })
        .collect::<Vec<_>>();
    if workflows.is_empty() {
        workflows.extend(
            phases
                .iter()
                .filter(|phase| phase.enabled && phase.is_verifier())
                .cloned()
                .map(|check| ToolWorkflow {
                    id: check.id.clone(),
                    check: Some(check),
                    remedy: None,
                    check_scope: if spec.workspace_indicator.is_some() {
                        CheckScope::Workspace
                    } else {
                        CheckScope::TargetFiles
                    },
                    invocation: convert_invocation(spec.phase_invocation),
                    compatibility_translation: true,
                    enabled: true,
                }),
        );
    }
    workflows
}

fn convert_invocation(invocation: pkl::InvocationGranularity) -> InvocationGranularity {
    match invocation {
        pkl::InvocationGranularity::PerFile => InvocationGranularity::PerFile,
        pkl::InvocationGranularity::Batch => InvocationGranularity::Batch,
        pkl::InvocationGranularity::Workspace => InvocationGranularity::Workspace,
    }
}

fn ordered_workflows(spec: &pkl::ToolSpec) -> Vec<(&String, &pkl::Workflow)> {
    let mut seen = BTreeSet::new();
    let mut workflows = Vec::new();
    for id in &spec.workflow_order {
        if let Some(workflow) = spec.workflows.get(id) {
            if seen.insert(id.clone()) {
                workflows.push((id, workflow));
            }
        }
    }
    workflows.extend(
        spec.workflows
            .iter()
            .filter(|(id, _)| !seen.contains(id.as_str())),
    );
    workflows
}

fn convert_workflow_command(
    id: String,
    command: &pkl::WorkflowCommand,
    mode: PhaseMode,
) -> ToolPhase {
    ToolPhase {
        id,
        mode,
        program: command.program.clone(),
        args: command.argv.iter().map(convert_argv_element).collect(),
        exit_codes: convert_exit_codes(&command.exit_codes),
        issues_on_stdout: command.issues_on_stdout,
        writes: convert_writes(command.writes),
        extra_args: command.extra_args.clone(),
        enabled: true,
    }
}

fn ordered_phases(spec: &pkl::ToolSpec) -> Vec<(String, &pkl::Phase)> {
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::<(String, &pkl::Phase)>::new();

    // Honor explicit phase order first.
    for id in &spec.phase_order {
        if let Some(phase) = spec.phases.get(id) {
            if seen.insert(id.clone()) {
                out.push((id.clone(), phase));
            }
        }
    }

    // Append any remaining phases sorted by canonical mode order, then by id.
    let mut remaining: Vec<(&String, &pkl::Phase)> = spec
        .phases
        .iter()
        .filter(|(id, _)| !seen.contains(id.as_str()))
        .collect();
    remaining.sort_by(|a, b| {
        canonical_mode_order(a.1.mode)
            .cmp(&canonical_mode_order(b.1.mode))
            .then_with(|| a.0.cmp(b.0))
    });
    for (id, phase) in remaining {
        out.push((id.clone(), phase));
    }
    out
}

fn canonical_mode_order(mode: pkl::PhaseMode) -> u8 {
    match mode {
        pkl::PhaseMode::Format => 0,
        pkl::PhaseMode::Fix => 1,
        pkl::PhaseMode::Verify => 2,
        pkl::PhaseMode::CheckOnly => 3,
    }
}

fn convert_phase((id, phase): (String, &pkl::Phase)) -> ToolPhase {
    ToolPhase {
        id,
        mode: convert_phase_mode(phase.mode),
        program: phase.program.clone(),
        args: phase.argv.iter().map(convert_argv_element).collect(),
        exit_codes: convert_exit_codes(&phase.exit_codes),
        issues_on_stdout: false,
        writes: convert_writes(phase.writes),
        extra_args: phase.extra_args.clone(),
        enabled: phase.enabled,
    }
}

fn convert_phase_mode(mode: pkl::PhaseMode) -> PhaseMode {
    match mode {
        pkl::PhaseMode::Format => PhaseMode::Format,
        pkl::PhaseMode::Fix => PhaseMode::Fix,
        pkl::PhaseMode::Verify => PhaseMode::Verify,
        pkl::PhaseMode::CheckOnly => PhaseMode::CheckOnly,
    }
}

fn convert_argv_element(element: &pkl::ArgvElement) -> CommandArgTemplate {
    match element {
        pkl::ArgvElement::Literal(s) => CommandArgTemplate::Literal(s.clone()),
        pkl::ArgvElement::Token(t) => match t {
            pkl::ArgToken::Files => CommandArgTemplate::Files,
            pkl::ArgToken::WorkspaceFiles => CommandArgTemplate::WorkspaceFiles,
            pkl::ArgToken::Workspace => CommandArgTemplate::Workspace,
            pkl::ArgToken::WorkspaceIndicator => CommandArgTemplate::WorkspaceIndicator,
            pkl::ArgToken::ProjectRoot => CommandArgTemplate::ProjectRoot,
            pkl::ArgToken::ToolExecutable => CommandArgTemplate::ToolExecutable,
            pkl::ArgToken::ExtraArgs => CommandArgTemplate::ExtraArgs,
        },
    }
}

fn convert_exit_codes(codes: &pkl::ExitCodes) -> ExitCodePolicy {
    ExitCodePolicy {
        clean: codes.clean.clone(),
        issues: codes.issues.clone(),
        failure: codes.failure.clone(),
        unexpected: match codes.unexpected {
            pkl::UnexpectedExitPolicy::Failure => UnexpectedExitPolicy::Failure,
            pkl::UnexpectedExitPolicy::Issues => UnexpectedExitPolicy::Issues,
        },
    }
}

fn convert_writes(writes: pkl::WriteBehavior) -> WriteBehavior {
    match writes {
        pkl::WriteBehavior::None => WriteBehavior::None,
        pkl::WriteBehavior::TargetFiles => WriteBehavior::TargetFiles,
        pkl::WriteBehavior::MatchingGlobs => WriteBehavior::MatchingGlobs,
        pkl::WriteBehavior::Workspace => WriteBehavior::Workspace,
    }
}

fn convert_messages(messages: &pkl::Messages) -> ToolMessages {
    ToolMessages {
        clean_changed_agent: messages.clean_changed_agent.clone(),
        issues_agent: messages.issues_agent.clone(),
        issues_changed_agent: messages.issues_changed_agent.clone(),
        unavailable_user: messages.unavailable_user.clone(),
        failed_user: messages.failed_user.clone(),
    }
}

// ----------------------------------------------------------------------------
// File matching
// ----------------------------------------------------------------------------

/// File selection shared by the hooks and the CLI: globs match
/// project-relative, `/`-separated paths; an empty include list selects every
/// file; excludes always win.
pub struct FileMatcher {
    include: GlobSet,
    exclude: GlobSet,
    include_all: bool,
}

impl FileMatcher {
    /// Compile a selection; an invalid glob is an error.
    pub fn new(config: &FileSelection) -> hookkit_core::Result<Self> {
        Ok(Self {
            include: build_globset(&config.include)?,
            exclude: build_globset(&config.exclude)?,
            include_all: config.include.is_empty(),
        })
    }

    /// Whether `absolute_path` is selected. Globs match its path relative to
    /// `project_root`, so unanchored excludes such as `**/target/**` never
    /// fire on the directories *containing* the project. A path outside the
    /// project root is never selected: a project's policy applies only to
    /// its own files.
    pub fn matches(&self, absolute_path: &Path, project_root: &Path) -> bool {
        absolute_path
            .strip_prefix(project_root)
            .is_ok_and(|relative| self.matches_relative(&slash_path(relative)))
    }

    /// Whether a project-relative, `/`-separated path is selected.
    pub fn matches_relative(&self, relative: &str) -> bool {
        (self.include_all || self.include.is_match(relative)) && !self.exclude.is_match(relative)
    }
}

fn build_globset(patterns: &[String]) -> hookkit_core::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern)
            .map_err(|e| invalid_data(format!("invalid file glob `{pattern}`: {e}")))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|e| invalid_data(format!("invalid file glob set: {e}")))
}

// ----------------------------------------------------------------------------
// Per-tool execution
// ----------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ToolContext<'a> {
    spec: &'a ToolSpec,
    project_root: &'a Path,
    global_diagnostics_dir: Option<&'a str>,
}

#[derive(Debug, Clone)]
struct ToolJob {
    workspace_dir: PathBuf,
    workspace_indicator: Option<PathBuf>,
    files: Vec<PathBuf>,
}

/// Group `paths` into jobs: by nearest workspace indicator when the tool has
/// one, where a file with no indicator above it is skipped or, with the
/// project-root fallback, grouped at the project root without a marker.
fn build_jobs(paths: &[PathBuf], project_root: &Path, spec: &ToolSpec) -> Vec<ToolJob> {
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
enum ToolRunOutcome {
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
struct CompletedToolOutcome {
    issues: IssueState,
    changes: ChangeState,
    diagnostics: String,
    /// Raw output of the phases that decided `issues`: the verifiers that
    /// reported issues or, for a tool without a verifier, every phase that
    /// did. Empty when the outcome is clean.
    issue_output: String,
    /// Files the issues are attributed to: those the deciding output names,
    /// or every job file when it names none. Empty when the output names
    /// only other files.
    files: Vec<PathBuf>,
    /// Existing files outside the job that the output names instead.
    out_of_scope: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssueState {
    Clean,
    Issues,
}

#[derive(Debug)]
enum ChangeState {
    Unchanged,
    Changed { files: Vec<PathBuf> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhaseStatus {
    Clean,
    Issues,
    Failure,
}

#[derive(Debug, Clone)]
struct PhaseLog {
    phase: String,
    command: String,
    program: String,
    arguments: Vec<String>,
    status: Option<i32>,
    classification: Option<PhaseStatus>,
    stdout: String,
    stderr: String,
    error: Option<String>,
}

/// Run a tool's independent per-workspace jobs, honoring `settings.jobs` for
/// bounded parallelism. Outcomes are returned in job order regardless of which
/// job finishes first, so downstream aggregation stays deterministic.
fn run_jobs(jobs: &[ToolJob], context: &ToolContext<'_>, jobs_setting: u32) -> Vec<ToolRunOutcome> {
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
fn resolve_worker_count(jobs_setting: u32, job_count: usize) -> usize {
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

#[derive(Debug)]
struct RenderedCommand {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    timeout: Option<Duration>,
}

fn render_command(phase: &ToolPhase, job: &ToolJob, context: &ToolContext<'_>) -> RenderedCommand {
    let program = resolve_program(
        phase.program.as_deref().unwrap_or(&context.spec.executable),
        job,
        context,
    );
    let mut args = Vec::new();
    for arg in &phase.args {
        match arg {
            CommandArgTemplate::Literal(value) => args.push(value.clone()),
            CommandArgTemplate::Files => args.extend(job.files.iter().map(|path| path_arg(path))),
            CommandArgTemplate::WorkspaceFiles => {
                args.extend(job.files.iter().map(|path| {
                    path.strip_prefix(&job.workspace_dir)
                        .map(path_arg)
                        .unwrap_or_else(|_| path_arg(path))
                }));
            }
            CommandArgTemplate::Workspace => args.push(path_arg(&job.workspace_dir)),
            CommandArgTemplate::WorkspaceIndicator => {
                if let Some(path) = &job.workspace_indicator {
                    args.push(path_arg(path));
                }
            }
            CommandArgTemplate::ProjectRoot => args.push(path_arg(context.project_root)),
            CommandArgTemplate::ToolExecutable => {
                args.push(resolve_program(&context.spec.executable, job, context))
            }
            CommandArgTemplate::ExtraArgs => args.extend(phase.extra_args.iter().cloned()),
        }
    }
    RenderedCommand {
        program,
        args,
        env: context.spec.env.clone(),
        timeout: context.spec.timeout,
    }
}

/// Resolve a bare program name against the configured project-local bin
/// directories, searched from each job file's directory and the job's
/// workspace up to the project root (see [`local_program`]). Anything else is
/// left to `PATH`, or run as a path relative to the command's directory.
fn resolve_program(program: &str, job: &ToolJob, context: &ToolContext<'_>) -> String {
    let mut starts = job
        .files
        .iter()
        .filter_map(|file| file.parent())
        .collect::<Vec<_>>();
    starts.push(&job.workspace_dir);
    let mut seen = BTreeSet::new();
    starts.retain(|start| seen.insert(*start));
    local_program(
        program,
        &starts,
        context.project_root,
        &context.spec.local_bin_dirs,
    )
    .map_or_else(|| program.to_owned(), |path| path_arg(&path))
}

/// Find a bare program name (one with no path separator) in project-local
/// bin directories the way the hooks do: for each `local_bin_dirs` entry in
/// order, every directory from each of `search_from` up to `project_root`,
/// nearest first; the first executable file wins. So a package's own
/// `node_modules/.bin/eslint` beats the repository root's. `None` means the
/// program runs as given: through `PATH`, or as a path.
pub fn local_program(
    program: &str,
    search_from: &[&Path],
    project_root: &Path,
    local_bin_dirs: &[String],
) -> Option<PathBuf> {
    if Path::new(program).components().count() != 1 {
        return None;
    }
    local_bin_dirs.iter().find_map(|bin_dir| {
        search_from.iter().find_map(|start| {
            start
                .ancestors()
                .take_while(|dir| dir.starts_with(project_root))
                .map(|dir| dir.join(bin_dir).join(program))
                .find(|candidate| is_executable_file(candidate))
        })
    })
}

/// Whether `path` is a file the hooks can execute (on Unix, one with an
/// execute bit).
pub fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

struct CommandOutput {
    status: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: Option<Duration>,
}

/// Longest wait for output after the command itself exits. Only a
/// descendant still holding the pipes (a backgrounded helper or a daemon
/// that did not detach) keeps them open that long; its later output is not
/// the command's.
const LINGERING_OUTPUT_GRACE: Duration = Duration::from_secs(2);

/// Shortest wait for output already written when the command exits.
const MIN_OUTPUT_GRACE: Duration = Duration::from_millis(100);

/// Run a command with captured output, killing it (and, on Unix, its process
/// group) if it outlives `command.timeout`. Output collection is bounded
/// too: once the command exits, output is gathered until its pipes close,
/// the timeout's deadline, or [`LINGERING_OUTPUT_GRACE`], whichever is first.
fn execute_command(command: &RenderedCommand, cwd: &Path) -> std::io::Result<CommandOutput> {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Instant;

    /// Output read so far from one pipe, and a signal once the pipe closes.
    struct Drain {
        buffer: Arc<Mutex<Vec<u8>>>,
        closed: mpsc::Receiver<()>,
    }

    fn drain(pipe: Option<impl Read + Send + 'static>) -> Drain {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let (sender, closed) = mpsc::channel();
        if let Some(mut pipe) = pipe {
            let shared = Arc::clone(&buffer);
            std::thread::spawn(move || {
                let mut chunk = [0u8; 8192];
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(read) => shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .extend_from_slice(&chunk[..read]),
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
                let _ = sender.send(());
            });
        }
        Drain { buffer, closed }
    }

    /// Everything read by `until`, even if the pipe is still open.
    fn collect(drain: Drain, until: Instant) -> Vec<u8> {
        let _ = drain
            .closed
            .recv_timeout(until.saturating_duration_since(Instant::now()));
        std::mem::take(
            &mut *drain
                .buffer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    let mut process = Command::new(&command.program);
    process
        .args(&command.args)
        .envs(command.env.iter().map(|(key, value)| (key, value)))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    let mut child = process.spawn()?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let deadline = command.timeout.map(|timeout| Instant::now() + timeout);
    let mut timed_out = None;
    let status = match (command.timeout, deadline) {
        (Some(timeout), Some(deadline)) => {
            let mut delay = Duration::from_millis(1);
            loop {
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                let now = Instant::now();
                if now >= deadline {
                    kill_process_tree(&mut child);
                    timed_out = Some(timeout);
                    break child.wait()?;
                }
                std::thread::sleep(delay.min(deadline - now));
                delay = (delay * 2).min(Duration::from_millis(20));
            }
        }
        _ => child.wait()?,
    };
    // A descendant may still hold the pipes after the command exits (or is
    // killed); take whatever output arrives promptly rather than waiting.
    let now = Instant::now();
    let grace = match (timed_out, deadline) {
        (Some(_), _) => Duration::from_secs(1),
        (None, Some(deadline)) => deadline
            .saturating_duration_since(now)
            .min(LINGERING_OUTPUT_GRACE),
        (None, None) => LINGERING_OUTPUT_GRACE,
    }
    .max(MIN_OUTPUT_GRACE);
    let until = now + grace;
    Ok(CommandOutput {
        status: status.code(),
        stdout: collect(stdout, until),
        stderr: collect(stderr, until),
        timed_out,
    })
}

fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = libc::pid_t::try_from(child.id()) {
            // SAFETY: `kill` has no memory-safety preconditions; the child was
            // spawned as the leader of its own process group.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
}

fn run_phase_command(phase: &ToolPhase, command: &RenderedCommand, cwd: &Path) -> PhaseLog {
    match execute_command(command, cwd) {
        Ok(output) if output.timed_out.is_some() => PhaseLog {
            phase: phase.id.clone(),
            command: display_command(&command.program, &command.args),
            program: command.program.clone(),
            arguments: command.args.clone(),
            status: output.status,
            classification: None,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            error: Some(format!(
                "timed out after {}s and was killed (settings.commandTimeoutSeconds / tool timeoutSeconds)",
                output.timed_out.unwrap_or_default().as_secs()
            )),
        },
        Ok(output) => {
            let status = output.status;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let mut classification = status.map(|code| classify_exit_code(&phase.exit_codes, code));
            if phase.issues_on_stdout
                && classification == Some(PhaseStatus::Clean)
                && !stdout.trim().is_empty()
            {
                classification = Some(PhaseStatus::Issues);
            }
            PhaseLog {
                phase: phase.id.clone(),
                command: display_command(&command.program, &command.args),
                program: command.program.clone(),
                arguments: command.args.clone(),
                status,
                classification,
                stdout,
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                error: None,
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => PhaseLog {
            phase: phase.id.clone(),
            command: display_command(&command.program, &command.args),
            program: command.program.clone(),
            arguments: command.args.clone(),
            status: None,
            classification: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some("not found".to_string()),
        },
        Err(e) => PhaseLog {
            phase: phase.id.clone(),
            command: display_command(&command.program, &command.args),
            program: command.program.clone(),
            arguments: command.args.clone(),
            status: None,
            classification: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(e.to_string()),
        },
    }
}

fn classify_exit_code(policy: &ExitCodePolicy, code: i32) -> PhaseStatus {
    if policy.clean.contains(&code) {
        PhaseStatus::Clean
    } else if policy.issues.contains(&code) {
        PhaseStatus::Issues
    } else if policy.failure.contains(&code) {
        PhaseStatus::Failure
    } else {
        match policy.unexpected {
            UnexpectedExitPolicy::Failure => PhaseStatus::Failure,
            UnexpectedExitPolicy::Issues => PhaseStatus::Issues,
        }
    }
}

// ----------------------------------------------------------------------------
// Snapshots
// ----------------------------------------------------------------------------

#[derive(Debug)]
struct Snapshot {
    files: BTreeMap<PathBuf, Option<Vec<u8>>>,
}

impl Snapshot {
    fn read(paths: &BTreeSet<PathBuf>) -> Self {
        let files = paths
            .iter()
            .map(|path| {
                let bytes = if path.is_file() {
                    std::fs::read(path).ok()
                } else {
                    None
                };
                (path.clone(), bytes)
            })
            .collect();
        Self { files }
    }

    fn changed_files(&self, after: &Self) -> Vec<PathBuf> {
        self.files
            .keys()
            .chain(after.files.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|path| self.files.get(path) != after.files.get(path))
            .collect()
    }
}

/// Every file an enabled phase of the tool may write.
fn snapshot_scope(job: &ToolJob, context: &ToolContext<'_>) -> BTreeSet<PathBuf> {
    let mut writes = context
        .spec
        .phases
        .iter()
        .filter(|phase| phase.enabled)
        .map(|phase| phase.writes)
        .collect::<Vec<_>>();
    writes.sort_by_key(|writes| *writes as u8);
    writes.dedup();
    writes
        .into_iter()
        .flat_map(|writes| write_scope(writes, job, context))
        .collect()
}

/// Files a command declaring `writes` may change, snapshotted around it to
/// learn what it changed. Glob- and workspace-wide writers are snapshotted
/// from [`write_root`], so a workspace-wide fixer's writes in sibling
/// packages are seen and reported too.
fn write_scope(
    writes: WriteBehavior,
    job: &ToolJob,
    context: &ToolContext<'_>,
) -> BTreeSet<PathBuf> {
    match writes {
        WriteBehavior::None => BTreeSet::new(),
        WriteBehavior::TargetFiles => job.files.iter().cloned().collect(),
        WriteBehavior::MatchingGlobs => collect_matching_files(
            &write_root(job, context),
            context.project_root,
            &context.spec.file_selection,
        ),
        WriteBehavior::Workspace => collect_workspace_files(&write_root(job, context)),
    }
}

/// Where a workspace-wide command can reach: the outermost directory from
/// the job's workspace up to the project root that holds the tool's
/// workspace indicator. `cargo --workspace`, Go workspaces, and npm
/// workspaces act on every member from there even when the job's workspace
/// is one member. Without an indicator, the job's workspace.
fn write_root(job: &ToolJob, context: &ToolContext<'_>) -> PathBuf {
    let Some(indicator) = &context.spec.workspace_indicator else {
        return job.workspace_dir.clone();
    };
    job.workspace_dir
        .ancestors()
        .take_while(|dir| dir.starts_with(context.project_root))
        .filter(|dir| dir.join(indicator).is_file())
        .last()
        .unwrap_or(&job.workspace_dir)
        .to_path_buf()
}

/// Files under `base` that the selection matches, with globs applied to
/// project-relative paths exactly as for candidates.
fn collect_matching_files(
    base: &Path,
    project_root: &Path,
    selection: &FileSelection,
) -> BTreeSet<PathBuf> {
    let matcher = match FileMatcher::new(selection) {
        Ok(matcher) => matcher,
        Err(_) => return BTreeSet::new(),
    };
    walk_files(base)
        .into_iter()
        .filter(|path| matcher.matches(path, project_root))
        .collect()
}

fn collect_workspace_files(base: &Path) -> BTreeSet<PathBuf> {
    walk_files(base).into_iter().collect()
}

fn walk_files(base: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(base)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".git" | "target" | "node_modules")
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .collect()
}

// ----------------------------------------------------------------------------
// Outcome aggregation and reporting
// ----------------------------------------------------------------------------

fn accumulate_outcomes(
    outcomes: Vec<ToolRunOutcome>,
    context: &ToolContext<'_>,
    ctx: &RuntimeContext<'_>,
    missing_tool_policy: pkl::MissingToolPolicy,
    output: &mut RunnerPostToolUseOutput,
    had_hard_failure: &mut Option<String>,
    had_harness_block_message: &mut Option<String>,
) -> hookkit_core::Result<ToolBatchStatus> {
    let mut changed_files = BTreeSet::new();
    let mut issue_files = BTreeSet::new();
    let mut out_of_scope_files = BTreeSet::new();
    let mut issue_diagnostics = Vec::new();
    let mut issue_outputs = Vec::new();
    let mut failure_diagnostics = Vec::new();
    let mut unavailable = Vec::new();

    for outcome in outcomes {
        match outcome {
            ToolRunOutcome::Completed(completed) => {
                if let ChangeState::Changed { files } = completed.changes {
                    changed_files.extend(files);
                }
                if completed.issues == IssueState::Issues {
                    if completed.files.is_empty() {
                        // The output names only files this call did not
                        // change: not the agent's problem right now.
                        out_of_scope_files.extend(completed.out_of_scope);
                    } else {
                        issue_files.extend(completed.files);
                        issue_diagnostics.push(completed.diagnostics);
                        if !issue_outputs.contains(&completed.issue_output) {
                            issue_outputs.push(completed.issue_output);
                        }
                    }
                }
            }
            ToolRunOutcome::ToolUnavailable {
                phase,
                executable,
                install_hint,
                changed_files: files,
            } => {
                changed_files.extend(files);
                unavailable.push((phase, executable, install_hint));
            }
            ToolRunOutcome::ToolFailed {
                phase,
                exit_code,
                error,
                diagnostics,
                changed_files: files,
            } => {
                changed_files.extend(files);
                failure_diagnostics.push((phase, exit_code, error, diagnostics));
            }
        }
    }

    let mut status = ToolBatchStatus {
        operational_failure: !unavailable.is_empty() || !failure_diagnostics.is_empty(),
        issues: !issue_diagnostics.is_empty(),
    };
    let tool = context.template_tool();

    if !unavailable.is_empty() {
        match missing_tool_policy {
            pkl::MissingToolPolicy::UserNotice => {
                for (phase, executable, install_hint) in &unavailable {
                    let message = render_unavailable_message(
                        context,
                        phase,
                        executable,
                        install_hint.as_deref(),
                        &mut output.notices,
                    );
                    output.notices.push(UserNotice::warning(message));
                }
            }
            pkl::MissingToolPolicy::HardFailure => {
                if let Some((phase, executable, install_hint)) = unavailable.first() {
                    *had_hard_failure = Some(render_unavailable_message(
                        context,
                        phase,
                        executable,
                        install_hint.as_deref(),
                        &mut output.notices,
                    ));
                }
                return Ok(status);
            }
            pkl::MissingToolPolicy::HarnessBlock => {
                if let Some((phase, executable, install_hint)) = unavailable.first() {
                    let message = render_unavailable_message(
                        context,
                        phase,
                        executable,
                        install_hint.as_deref(),
                        &mut output.notices,
                    );
                    *had_harness_block_message = Some(message);
                }
                return Ok(status);
            }
        }
    }

    if !failure_diagnostics.is_empty() {
        let diagnostics = failure_diagnostics
            .iter()
            .map(|(phase, exit_code, _, diagnostics)| {
                format!(
                    "== phase {phase} failed (exit {exit_code:?}) ==\n{}",
                    diagnostics.trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let artifact = write_diagnostics(
            "tool-failure",
            &diagnostics,
            context,
            ctx,
            &mut output.notices,
        )?;
        let (phase, exit_code, error, _) = &failure_diagnostics[0];
        let message = render_failed_message(
            context,
            &artifact,
            (phase, *exit_code, error.as_deref()),
            &mut output.notices,
        );
        output.notices.push(UserNotice::error(message));
        output.diagnostics.push(report_with_artifact(
            format!("{} failure diagnostics", context.spec.display_name),
            diagnostics,
            artifact,
            context.project_root,
        ));
    }

    let changed_paths = changed_files
        .iter()
        .map(|path| rel_display(path, context.project_root))
        .collect::<Vec<_>>();
    let issue_paths = issue_files
        .iter()
        .map(|path| rel_display(path, context.project_root))
        .collect::<Vec<_>>();

    if !issue_diagnostics.is_empty() {
        let diagnostics = issue_diagnostics
            .iter()
            .map(|diagnostics| diagnostics.trim())
            .filter(|diagnostics| !diagnostics.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        let artifact = write_diagnostics(
            "tool-issues",
            &diagnostics,
            context,
            ctx,
            &mut output.notices,
        )?;
        output.notices.push(UserNotice::warning(format!(
            "{}: issues remain in {}; diagnostics: {}",
            context.spec.display_name,
            issue_paths.join(", "),
            artifact.display()
        )));
        output.diagnostics.push(report_with_artifact(
            format!("{} diagnostics", context.spec.display_name),
            diagnostics,
            artifact.clone(),
            context.project_root,
        ));

        let (template, fallback, field) = if changed_paths.is_empty() {
            (
                &context.spec.messages.issues_agent,
                pkl::default_issues_agent(),
                "issuesAgent",
            )
        } else {
            (
                &context.spec.messages.issues_changed_agent,
                pkl::default_issues_changed_agent(),
                "issuesChangedAgent",
            )
        };
        output
            .agent_feedback
            .push(AgentFeedback::Issues(Box::new(PendingIssues {
                template: template.clone(),
                fallback,
                field,
                tool: tool.name.to_owned(),
                tool_id: tool.id.to_owned(),
                project_root: context.project_root.to_path_buf(),
                changed_files: changed_paths,
                issue_files: issue_paths,
                diagnostics: artifact,
                output: issue_outputs.join("\n"),
            })));
    } else if !changed_paths.is_empty() {
        // Tools on the default template share one consolidated agent line;
        // a customised `cleanChangedAgent` is rendered as configured (or, if
        // it cannot be, the tool joins the shared line).
        let template = &context.spec.messages.clean_changed_agent;
        let mut in_agent_line = *template == pkl::default_clean_changed_agent();
        if !in_agent_line {
            let args = MessageArgs {
                changed_files: &changed_paths,
                ..MessageArgs::default()
            };
            match render_template(template, &tool, &args) {
                Ok(rendered) => output
                    .agent_feedback
                    .push(AgentFeedback::Rendered(rendered)),
                Err(error) => {
                    output.notices.push(template_failure_notice(
                        &tool,
                        "cleanChangedAgent",
                        &error,
                    ));
                    in_agent_line = true;
                }
            }
        }
        output.auto_fixed.push(AutoFixed {
            tool: context.spec.display_name.clone(),
            files: changed_paths,
            in_agent_line,
        });
    }

    if !out_of_scope_files.is_empty() {
        let files = out_of_scope_files
            .iter()
            .map(|path| rel_display(path, context.project_root))
            .collect::<Vec<_>>();
        let more = if files.len() > 5 { ", …" } else { "" };
        output.notices.push(UserNotice::info(format!(
            "velvet-glove: not reporting issues outside the files this call changed: {} ({}{more}).",
            context.spec.display_name,
            files[..files.len().min(5)].join(", ")
        )));
    }

    status.operational_failure = status.operational_failure || had_hard_failure.is_some();
    Ok(status)
}

/// The notice for a missing executable, from `messages.unavailableUser` when
/// set, else worded as at Stop.
fn render_unavailable_message(
    context: &ToolContext<'_>,
    phase: &str,
    executable: &str,
    install_hint: Option<&str>,
    notices: &mut Vec<UserNotice>,
) -> String {
    if let Some(template) = context.spec.messages.unavailable_user.as_ref() {
        let tool = context.template_tool();
        let args = MessageArgs {
            phase_error: Some((phase, executable, install_hint)),
            ..MessageArgs::default()
        };
        match render_template(template, &tool, &args) {
            Ok(message) => return message,
            Err(error) => notices.push(template_failure_notice(&tool, "unavailableUser", &error)),
        }
    }
    let hint = install_hint
        .map(|hint| format!("; {hint}"))
        .unwrap_or_default();
    format!(
        "velvet-glove could not run {} ({executable} not found{hint}).",
        context.spec.display_name
    )
}

/// The notice for a phase that failed operationally, from
/// `messages.failedUser` when set, else worded as at Stop.
fn render_failed_message(
    context: &ToolContext<'_>,
    diagnostics_path: &Path,
    (phase, exit_code, error): (&str, Option<i32>, Option<&str>),
    notices: &mut Vec<UserNotice>,
) -> String {
    if let Some(template) = context.spec.messages.failed_user.as_ref() {
        let tool = context.template_tool();
        let args = MessageArgs {
            diagnostics_path: Some(diagnostics_path),
            phase_error: Some((phase, "", None)),
            ..MessageArgs::default()
        };
        match render_template(template, &tool, &args) {
            Ok(message) => return message,
            Err(error) => notices.push(template_failure_notice(&tool, "failedUser", &error)),
        }
    }
    let reason = match (error, exit_code) {
        (Some(error), _) => format!("{phase}: {error}"),
        (None, Some(code)) => format!("{phase} failed with exit code {code}"),
        (None, None) => format!("{phase} was terminated by a signal"),
    };
    format!(
        "velvet-glove could not run {} ({reason}; log: {}).",
        context.spec.display_name,
        diagnostics_path.display()
    )
}

/// The tool identity a message template sees.
struct TemplateTool<'a> {
    name: &'a str,
    id: &'a str,
    project_root: &'a Path,
}

impl ToolContext<'_> {
    fn template_tool(&self) -> TemplateTool<'_> {
        TemplateTool {
            name: &self.spec.display_name,
            id: &self.spec.id,
            project_root: self.project_root,
        }
    }
}

/// Values a message template may reference besides the tool.
#[derive(Default)]
struct MessageArgs<'a> {
    changed_files: &'a [String],
    issue_files: &'a [String],
    diagnostics_path: Option<&'a Path>,
    /// Phase, executable, and install hint of a failed or missing command.
    phase_error: Option<(&'a str, &'a str, Option<&'a str>)>,
    excerpt: &'a str,
}

fn render_template(
    template: &str,
    tool: &TemplateTool<'_>,
    args: &MessageArgs<'_>,
) -> hookkit_core::Result<String> {
    let diagnostics_path_text = args
        .diagnostics_path
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let diagnostics_rel_path = args
        .diagnostics_path
        .and_then(|path| path.strip_prefix(tool.project_root).ok())
        .map(slash_path)
        .unwrap_or_default();
    let (phase, executable, install_hint) = args.phase_error.unwrap_or(("", "", None));
    let json_context = serde_json::json!({
        "tool": tool.name,
        "tool_id": tool.id,
        "changed_files": args.changed_files,
        "issue_files": args.issue_files,
        "diagnostics_path": diagnostics_path_text,
        "diagnostics_absolute_path": diagnostics_path_text,
        "diagnostics_rel_path": diagnostics_rel_path,
        "diagnostics_project_path": diagnostics_rel_path,
        "project_root": tool.project_root.to_string_lossy(),
        "phase": phase,
        "executable": executable,
        "install_hint": install_hint.unwrap_or(""),
        "excerpt": args.excerpt,
    });

    Environment::new()
        .render_str(template, &json_context)
        .map_err(|e| invalid_data(format!("failed to render message template: {e}")))
}

/// Render `template`, or the built-in `fallback` with a user notice when the
/// configured template cannot be rendered: the agent must still hear about
/// changed files and remaining issues.
fn render_with_fallback(
    template: &str,
    fallback: &str,
    field: &str,
    tool: &TemplateTool<'_>,
    args: &MessageArgs<'_>,
    notices: &mut Vec<UserNotice>,
) -> String {
    render_template(template, tool, args).unwrap_or_else(|error| {
        notices.push(template_failure_notice(tool, field, &error));
        render_template(fallback, tool, args).unwrap_or_default()
    })
}

fn template_failure_notice(
    tool: &TemplateTool<'_>,
    field: &str,
    error: &HookkitError,
) -> UserNotice {
    UserNotice::warning(format!(
        "velvet-glove: {}: messages.{field} could not be rendered ({}); used the default",
        tool.name,
        error_summary(&error.to_string())
    ))
}

/// Default home of immediate-mode diagnostics, outside the project.
fn immediate_log_directory() -> PathBuf {
    std::env::temp_dir()
        .join("velvet-glove")
        .join("state")
        .join("post-tool-immediate")
}

/// Write full diagnostics to the configured `diagnosticsDirectory`, or to the
/// default location (with a user notice) when that cannot be written.
fn write_diagnostics(
    label: &str,
    diagnostics: &str,
    context: &ToolContext<'_>,
    ctx: &RuntimeContext<'_>,
    notices: &mut Vec<UserNotice>,
) -> hookkit_core::Result<PathBuf> {
    let label = format!("{}-{label}", context.spec.id);
    let configured = context
        .spec
        .diagnostics_directory
        .as_deref()
        .or(context.global_diagnostics_dir)
        .map(|dir| absolute_from(Path::new(dir), context.project_root));
    if let Some(directory) = configured {
        match write_immediate_artifact(&directory, &label, diagnostics, ctx) {
            Ok(path) => return Ok(path),
            Err(error) => notices.push(UserNotice::warning(format!(
                "velvet-glove: cannot write diagnostics to {} ({error}); using the default location",
                directory.display()
            ))),
        }
    }
    write_immediate_artifact(&immediate_log_directory(), &label, diagnostics, ctx)
}

fn write_immediate_artifact(
    directory: &Path,
    label: &str,
    text: &str,
    ctx: &RuntimeContext<'_>,
) -> hookkit_core::Result<PathBuf> {
    let manager = ArtifactManager::new(directory)?;
    manager
        .write_text(&runner_artifact_key(ctx, label.to_owned()), text)
        .map_err(Into::into)
}

fn runner_artifact_key(ctx: &RuntimeContext<'_>, label: String) -> ArtifactKey {
    let session = ctx
        .session_id()
        .map(ToString::to_string)
        .or_else(|| ctx.conversation_id().map(ToString::to_string))
        .unwrap_or_else(|| "unknown-session".to_string());
    let mut key = ArtifactKey::new(session, label);
    if let Some(turn) = ctx.turn_id() {
        key = key.with_turn(turn.to_string());
    }
    if let Some(tool_call) = ctx.tool_call_id() {
        key = key.with_tool_use(tool_call.to_string());
    }
    key
}

fn report_with_artifact(
    title: String,
    diagnostics: String,
    artifact_path: PathBuf,
    project_root: &Path,
) -> DiagnosticReport {
    let mut artifact = DiagnosticArtifact::new(artifact_path.clone(), "text/plain");
    if let Ok(rel_path) = artifact_path.strip_prefix(project_root) {
        artifact = artifact.with_project_relative_path(rel_path);
    }
    artifact = artifact.with_summary(&title);
    DiagnosticReport::new(title, diagnostics).with_artifact(artifact)
}

fn format_logs(logs: &[PhaseLog]) -> String {
    let mut out = String::new();
    for log in logs {
        out.push_str(&format!(
            "[{phase}] command: {command}\nstatus: {status:?}\nclassification: {classification:?}\n",
            phase = log.phase,
            command = log.command,
            status = log.status,
            classification = log.classification
        ));
        if let Some(err) = &log.error {
            out.push_str(&format!("error: {err}\n"));
        }
        if !log.stdout.trim().is_empty() {
            out.push_str("stdout:\n");
            out.push_str(&log.stdout);
            if !log.stdout.ends_with('\n') {
                out.push('\n');
            }
        }
        if !log.stderr.trim().is_empty() {
            out.push_str("stderr:\n");
            out.push_str(&log.stderr);
            if !log.stderr.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push('\n');
    }
    out
}

fn display_command(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn absolute_from(path: &Path, base: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn rel_display(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .map(slash_path)
        .unwrap_or_else(|_| slash_path(path))
}

fn invalid_data(message: String) -> HookkitError {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hookkit_core::EventSpec as _;
    use proptest::prelude::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn default_excludes_are_unanchored_and_ignore_directories_above_the_project() {
        let matcher = FileMatcher::new(&FileSelection {
            include: vec!["**/*.py".into()],
            exclude: pkl::default_excludes(),
        })
        .unwrap();
        let root = Path::new("/home/user/target/project");
        assert!(matcher.matches(&root.join("src/a.py"), root));
        assert!(
            !matcher.matches(Path::new("/home/user/scratch/plan.py"), root),
            "a file outside the project is never selected"
        );
        for excluded in [
            "node_modules/x.py",
            "web/node_modules/pkg/x.py",
            "svc/.venv/lib/x.py",
            "pkg/__pycache__/x.py",
            "crates/a/target/x.py",
            ".git/hooks/x.py",
            ".ruff_cache/0.16.6/x.py",
            "svc/.tox/py312/lib/x.py",
            "app/.next/server/x.py",
        ] {
            assert!(!matcher.matches(&root.join(excluded), root), "{excluded}");
        }
    }

    #[test]
    fn immediate_excerpts_are_plain_project_relative_and_share_one_budget() {
        let reporting = pkl::DeferredReporting {
            excerpt_max_lines: 12,
            ..Default::default()
        };
        let limits = ExcerptLimits::new(
            &reporting,
            display_roots(Path::new("/private/repo"), Path::new("/repo")),
        );
        let log = Path::new("/tmp/vg/issues.txt");
        let long = (1..=20)
            .map(|line| format!("b.py:{line}: E{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let excerpts = limits.excerpts(&[
            (
                "\u{1b}[31m/private/repo/src/a.py:1:1\u{1b}[0m: E1\n/repo/src/a.py:2:1: E2\n",
                log,
            ),
            (&long, log),
            ("c.py:1: E5", log),
        ]);
        assert_eq!(excerpts[0], "src/a.py:1:1: E1\nsrc/a.py:2:1: E2");
        // A verbose tool gets its share, not the whole budget ...
        assert!(
            excerpts[1].ends_with("…truncated; full log: /tmp/vg/issues.txt"),
            "{}",
            excerpts[1]
        );
        assert_eq!(
            excerpts[1].lines().count(),
            5 + 1,
            "its share plus the log note"
        );
        // ... so a later tool is still quoted.
        assert_eq!(excerpts[2], "c.py:1: E5");
    }

    #[test]
    fn pending_issue_messages_fall_back_to_the_default_template() {
        let mut output = RunnerPostToolUseOutput::default()
            .with_excerpt_limits(ExcerptLimits::new(&Default::default(), Vec::new()));
        output
            .agent_feedback
            .push(AgentFeedback::Issues(Box::new(PendingIssues {
                template: "{{ tool | nosuchfilter }}".into(),
                fallback: pkl::default_issues_changed_agent(),
                field: "issuesChangedAgent",
                tool: "Fmt".into(),
                tool_id: "fmt".into(),
                project_root: PathBuf::from("/repo"),
                changed_files: vec!["src/a.txt".into()],
                issue_files: vec!["src/a.txt".into()],
                diagnostics: PathBuf::from("/tmp/fmt.txt"),
                output: "src/a.txt:1: bad".into(),
            })));
        let rendered = output.rendered_agent_feedback();
        assert_eq!(
            rendered,
            vec![
                "velvet-glove: Fmt changed src/a.txt (re-read before editing); issues remain in src/a.txt:\nsrc/a.txt:1: bad"
            ]
        );
        assert!(
            format_notice(&output.notices[0]).starts_with(
                "velvet-glove: Fmt: messages.issuesChangedAgent could not be rendered"
            ),
            "{:?}",
            output.notices
        );
    }

    #[test]
    fn auto_fix_line_names_at_most_ten_files() {
        let files = (0..14)
            .map(|n| format!("src/f{n:02}.rs"))
            .collect::<Vec<_>>();
        let line = auto_fixed_line(&[AutoFixed {
            tool: "cargo fmt".into(),
            files,
            in_agent_line: true,
        }])
        .unwrap();
        assert!(line.contains("src/f09.rs (cargo fmt)"), "{line}");
        assert!(!line.contains("src/f10.rs"), "{line}");
        assert!(
            line.ends_with(" and 4 more; re-read before editing."),
            "{line}"
        );
    }

    #[test]
    fn a_harness_block_still_tells_the_agent_what_changed() {
        let output = RunnerPostToolUseOutput::default()
            .with_auto_fixed(AutoFixed {
                tool: "Ruff".into(),
                files: vec!["src/a.py".into()],
                in_agent_line: true,
            })
            .with_agent_feedback("Custom rewrote web/c.ts")
            .with_harness_block("velvet-glove could not run ESLint (eslint not found).");
        let PostToolUseOutput::Claude(native) =
            lower_report(&HarnessId::CLAUDE_CODE, output, None).unwrap()
        else {
            panic!("expected Claude output");
        };
        let emission = hookkit_claude::protocol::PostToolUse::emit(native).unwrap();
        assert_eq!(
            String::from_utf8_lossy(emission.stderr()).trim_end(),
            "velvet-glove auto-fixed src/a.py (Ruff); re-read before editing.\nCustom rewrote web/c.ts\nvelvet-glove could not run ESLint (eslint not found)."
        );
    }

    #[test]
    fn error_summaries_keep_the_informative_line() {
        assert_eq!(
            error_summary(
                "pkl eval failed for /p/.velvet-glove/post-tool-use.pkl:\n–– Pkl Error ––\nExpected value of type `Int`, but got type `String`.\n\n2 | jobs = \"x\"\n"
            ),
            "pkl eval failed for /p/.velvet-glove/post-tool-use.pkl: Expected value of type `Int`, but got type `String`."
        );
        assert_eq!(
            error_summary(
                "invalid Velvet Glove configuration:\n- ruff (ruff): invalid file glob `src/{a`"
            ),
            "invalid Velvet Glove configuration: ruff (ruff): invalid file glob `src/{a`"
        );
        assert_eq!(error_summary("plain failure"), "plain failure");
    }

    fn emitted_json(output: PostToolUseOutput) -> (serde_json::Value, Vec<u8>) {
        let emission = match output {
            PostToolUseOutput::Claude(native) => {
                hookkit_claude::protocol::PostToolUse::emit(native).unwrap()
            }
            PostToolUseOutput::Codex(native) => {
                hookkit_codex::protocol::PostToolUse::emit(native).unwrap()
            }
            _ => panic!("expected Claude or Codex output"),
        };
        assert_eq!(emission.exit_code(), 0);
        (
            serde_json::from_slice(emission.stdout()).unwrap(),
            emission.stderr().to_vec(),
        )
    }

    #[test]
    fn immediate_user_notices_use_system_message_and_auto_fixes_share_one_line() {
        let fixed = |tool: &str, files: &[&str], in_agent_line| AutoFixed {
            tool: tool.into(),
            files: files.iter().map(|file| file.to_string()).collect(),
            in_agent_line,
        };
        for harness in [HarnessId::CLAUDE_CODE, HarnessId::CODEX] {
            let output = RunnerPostToolUseOutput::default()
                .with_auto_fixed(fixed("Ruff", &["src/a.py", "src/b.py"], true))
                .with_auto_fixed(fixed("Black", &["src/a.py"], true))
                .with_auto_fixed(fixed("Custom", &["web/c.ts"], false))
                .with_agent_feedback("Custom rewrote web/c.ts")
                .with_user_notice(UserNotice::warning("Lint: `lint` is unavailable"))
                .with_diagnostic_report(DiagnosticReport::new("Lint diagnostics", "full log"));
            let (json, stderr) = emitted_json(lower_report(&harness, output, None).unwrap());

            assert!(
                stderr.is_empty(),
                "{harness}: user notices must not use stderr"
            );
            assert_eq!(
                json["systemMessage"],
                "velvet-glove auto-fixed src/a.py (Ruff, Black), src/b.py (Ruff), web/c.ts (Custom); re-read before editing.\nvelvet-glove: Lint: `lint` is unavailable",
                "{harness}"
            );
            assert_eq!(
                json["hookSpecificOutput"]["additionalContext"],
                "velvet-glove auto-fixed src/a.py (Ruff, Black), src/b.py (Ruff); re-read before editing.\nCustom rewrote web/c.ts",
                "{harness}"
            );

            let (json, stderr) = emitted_json(
                lower_report(&harness, RunnerPostToolUseOutput::default(), None).unwrap(),
            );
            assert_eq!(json, serde_json::json!({}), "{harness}: clean is silent");
            assert!(stderr.is_empty());
        }
    }

    #[test]
    fn domain_outcomes_keep_clean_failure_and_unsupported_distinct() {
        assert!(matches!(
            lower_domain_outcome(&HarnessId::CLAUDE_CODE, RunnerDomainOutcome::Clean, None)
                .unwrap(),
            PostToolUseOutput::Claude(_)
        ));
        assert!(
            lower_domain_outcome(
                &HarnessId::CLAUDE_CODE,
                RunnerDomainOutcome::OperationalFailure {
                    message: "checker crashed".into(),
                },
                None,
            )
            .is_err()
        );
        assert!(matches!(
            lower_domain_outcome(&HarnessId::ANTIGRAVITY, RunnerDomainOutcome::Clean, None)
                .unwrap(),
            PostToolUseOutput::Antigravity(_)
        ));
        assert!(
            lower_domain_outcome(
                &HarnessId::ANTIGRAVITY,
                RunnerDomainOutcome::UnsupportedHarness {
                    harness: "antigravity".into(),
                    reason: "no changed-file data".into(),
                },
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn antigravity_warning_lowering_records_full_loss_and_preserves_exact_stdout() {
        let directory = unique_test_directory("antigravity-lowering-warning");
        let target = LoweringWarningArtifact {
            directory: directory.clone(),
            key: ArtifactKey::new("conversation-7", "post-tool-use-step-3-lowering-warning"),
        };
        let diagnostic_artifact =
            DiagnosticArtifact::new(directory.join("complete-diagnostic.txt"), "text/plain")
                .with_project_relative_path(".velvet-glove/complete-diagnostic.txt")
                .with_summary("complete tool output");
        let warning = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::BestEffortWithWarnings)
            .with_user_notice(UserNotice::warning("review every diagnostic line"))
            .with_diagnostic_report(
                DiagnosticReport::new("lint report", "first line\nsecond line")
                    .with_artifact(diagnostic_artifact),
            )
            .with_agent_feedback("re-read generated.rs\nthen repair it");

        let native = match lower_report(&HarnessId::ANTIGRAVITY, warning, Some(&target)).unwrap() {
            PostToolUseOutput::Antigravity(native) => native,
            _ => panic!("expected Antigravity output"),
        };
        let emission = hookkit_antigravity::PostToolUse::emit(native).unwrap();
        let artifact_path = directory.join(format!("{}.json", target.key.filename()));

        assert_eq!(emission.stdout(), b"{}");
        assert_eq!(emission.exit_code(), 0);
        assert!(
            String::from_utf8_lossy(emission.stderr())
                .contains(&artifact_path.to_string_lossy().into_owned())
        );
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&artifact_path).unwrap()).unwrap();
        assert_eq!(record["formatVersion"], 1);
        assert_eq!(record["kind"], "post-tool-use-lowering-loss");
        assert_eq!(record["harness"], "antigravity");
        assert_eq!(record["event"], "PostToolUse");
        assert_eq!(record["loweringPolicy"], "best-effort-with-warnings");
        assert_eq!(
            record["unavailable"]["userNotices"][0]["text"],
            "review every diagnostic line"
        );
        assert_eq!(
            record["unavailable"]["diagnostics"][0]["text"],
            "first line\nsecond line"
        );
        assert_eq!(
            record["unavailable"]["diagnostics"][0]["artifact"]["summary"],
            "complete tool output"
        );
        assert_eq!(
            record["unavailable"]["agentFeedback"][0],
            "re-read generated.rs\nthen repair it"
        );
        assert_eq!(
            record["unavailable"]["renderedAgent"],
            "re-read generated.rs\nthen repair it"
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn antigravity_best_effort_omits_unavailable_messages_without_a_record() {
        let directory = unique_test_directory("antigravity-lowering-best-effort");
        let target = LoweringWarningArtifact {
            directory: directory.clone(),
            key: ArtifactKey::new("conversation-8", "post-tool-use-step-4-lowering-warning"),
        };
        let best_effort = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::BestEffort)
            .with_user_notice(UserNotice::warning("review diagnostics"))
            .with_agent_feedback("re-read generated.rs");
        let native =
            match lower_report(&HarnessId::ANTIGRAVITY, best_effort, Some(&target)).unwrap() {
                PostToolUseOutput::Antigravity(native) => native,
                _ => panic!("expected Antigravity output"),
            };
        let emission = hookkit_antigravity::PostToolUse::emit(native).unwrap();

        assert_eq!(emission.stdout(), b"{}");
        assert!(emission.stderr().is_empty());
        assert!(
            !directory
                .join(format!("{}.json", target.key.filename()))
                .exists()
        );
    }

    #[test]
    fn antigravity_warning_lowering_never_overwrites_a_reused_step_key() {
        let directory = unique_test_directory("antigravity-lowering-collision");
        let target = LoweringWarningArtifact {
            directory: directory.clone(),
            key: ArtifactKey::new("conversation-8", "post-tool-use-step-0-lowering-warning"),
        };
        let lower = |feedback: &str| {
            let output = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::BestEffortWithWarnings)
                .with_agent_feedback(feedback);
            let native = match lower_report(&HarnessId::ANTIGRAVITY, output, Some(&target)).unwrap()
            {
                PostToolUseOutput::Antigravity(native) => native,
                _ => panic!("expected Antigravity output"),
            };
            let emission = hookkit_antigravity::PostToolUse::emit(native).unwrap();
            let stderr = String::from_utf8(emission.stderr().to_vec()).unwrap();
            PathBuf::from(stderr.rsplit_once(": ").unwrap().1)
        };

        let first = lower("first invocation");
        let second = lower("second invocation");

        assert_ne!(first, second);
        let first_record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(first).unwrap()).unwrap();
        let second_record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(second).unwrap()).unwrap();
        assert_eq!(
            first_record["unavailable"]["agentFeedback"][0],
            "first invocation"
        );
        assert_eq!(
            second_record["unavailable"]["agentFeedback"][0],
            "second invocation"
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn antigravity_strict_lowering_errors_instead_of_recording_loss() {
        let strict = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::Strict)
            .with_agent_feedback("re-read generated.rs");
        assert!(lower_report(&HarnessId::ANTIGRAVITY, strict, None).is_err());
    }

    #[test]
    fn antigravity_warning_lowering_errors_when_the_record_cannot_be_written() {
        let directory = unique_test_directory("antigravity-lowering-write-failure");
        std::fs::create_dir_all(&directory).unwrap();
        let not_a_directory = directory.join("regular-file");
        std::fs::write(&not_a_directory, "occupied").unwrap();
        let target = LoweringWarningArtifact {
            directory: not_a_directory,
            key: ArtifactKey::new("conversation-9", "post-tool-use-step-5-lowering-warning"),
        };
        let warning = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::BestEffortWithWarnings)
            .with_agent_feedback("this must be retained");

        assert!(lower_report(&HarnessId::ANTIGRAVITY, warning, Some(&target)).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn unique_test_directory(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "velvet-glove-runner-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

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

    #[test]
    fn state_disposition_discharges_successes_and_retries_only_unfinished_files() {
        let root = PathBuf::from("/tmp/hookkit-selective-disposition");
        let clean = root.join("clean.rs");
        let auto_fixed = root.join("auto.rs");
        let manual = root.join("manual.rs");
        let operational = root.join("operational.rs");
        let deleted = root.join("deleted.rs");
        let unchecked = root.join("unchecked.rs");
        let mut result = DeferredRunResult::default();
        result.record_file(FileAssessment::new(&clean, FileStatus::Clean));
        result.record_file(FileAssessment::new(&auto_fixed, FileStatus::AutoFixed));
        result.record_file(FileAssessment::new(&manual, FileStatus::ManualFixesNeeded));
        for (id, path, missing_tool) in [
            ("tool-failure", &operational, false),
            ("tool-missing", &unchecked, true),
        ] {
            result.record_operational_problem(OperationalProblem {
                id: id.into(),
                tool_id: Some(id.into()),
                tool_name: None,
                missing_tool,
                install_hint: None,
                phase: Some("initial-check".into()),
                affected_files: vec![path.clone()],
                message: "tool crashed".into(),
                artifact_ids: Vec::new(),
            });
        }
        let unresolved = FileActivityTarget::Workspace {
            root: Some(Utf8PathBuf::from("/tmp/hookkit-selective-disposition")),
        };
        let resolution = ActivityResolution {
            not_applicable_files: BTreeSet::from([deleted.clone()]),
            unresolved_targets: vec![unresolved.clone()],
            gap_messages: BTreeSet::from(["dynamic shell target".into()]),
            truncated: false,
        };

        let blocking = plan_deferred_state_disposition(
            &result,
            &resolution,
            pkl::MissingToolPolicy::HarnessBlock,
        )
        .unwrap();
        assert!(
            blocking
                .retry_files
                .contains(&Utf8PathBuf::from_path_buf(unchecked).unwrap()),
            "a blocking missing tool keeps its files pending"
        );
        let disposition = plan_deferred_state_disposition(
            &result,
            &resolution,
            pkl::MissingToolPolicy::UserNotice,
        )
        .unwrap();
        assert_eq!(
            disposition.retry_files,
            BTreeSet::from([
                Utf8PathBuf::from_path_buf(manual).unwrap(),
                Utf8PathBuf::from_path_buf(operational).unwrap(),
            ])
        );
        assert_eq!(disposition.retry_targets, vec![unresolved]);
        assert_eq!(
            disposition.retry_gaps,
            BTreeSet::from(["dynamic shell target".into()])
        );
        assert_eq!(
            disposition.handled_files,
            BTreeSet::from([
                Utf8PathBuf::from_path_buf(auto_fixed).unwrap(),
                Utf8PathBuf::from_path_buf(clean).unwrap(),
                Utf8PathBuf::from_path_buf(deleted).unwrap(),
            ])
        );
    }

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

    #[cfg(unix)]
    #[test]
    fn project_lock_serializes_runs_on_the_same_project_only() {
        let project = unique_test_directory("project-lock");
        let first = lock_project(&project);
        let (sender, receiver) = std::sync::mpsc::channel();
        let contender = project.clone();
        let handle = std::thread::spawn(move || {
            let _second = lock_project(&contender);
            sender.send(()).unwrap();
        });
        assert!(
            receiver.recv_timeout(Duration::from_millis(300)).is_err(),
            "a second run on the same project must wait"
        );
        drop(first);
        receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("the second run proceeds once the first finishes");
        handle.join().unwrap();

        let _held = lock_project(&project);
        let _other = lock_project(&project.join("other"));
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn run_bundle_pruning_keeps_the_newest_bundles() {
        let runs = unique_test_directory("prune-runs");
        for name in [
            "1000-1-0-turn-completion",
            "3000-1-0-turn-completion",
            "2000-1-0-turn-completion",
            "not-a-run",
        ] {
            std::fs::create_dir_all(runs.join(name)).unwrap();
        }
        prune_run_bundles(&runs, 2, &runs.join("3000-1-0-turn-completion"));
        let mut remaining = std::fs::read_dir(&runs)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        remaining.sort();
        assert_eq!(
            remaining,
            vec![
                "2000-1-0-turn-completion",
                "3000-1-0-turn-completion",
                "not-a-run"
            ]
        );

        // A run committed after a backwards clock step has the oldest name
        // but is never the one removed.
        std::fs::create_dir_all(runs.join("500-1-0-turn-completion")).unwrap();
        prune_run_bundles(&runs, 2, &runs.join("500-1-0-turn-completion"));
        let mut remaining = std::fs::read_dir(&runs)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        remaining.sort();
        assert_eq!(
            remaining,
            vec![
                "3000-1-0-turn-completion",
                "500-1-0-turn-completion",
                "not-a-run"
            ]
        );
        let _ = std::fs::remove_dir_all(runs);
    }

    #[test]
    fn deferred_artifact_argv_is_unambiguous_and_path_components_are_safe() {
        let log = DeferredLog {
            tool_index: 0,
            workflow_index: 1,
            job_index: 2,
            phase: CommandPhase::InitialCheck,
            log: PhaseLog {
                phase: "check".into(),
                command: "checker argument with spaces line break".into(),
                program: "checker tool".into(),
                arguments: vec!["argument with spaces".into(), "line\nbreak".into()],
                status: Some(0),
                classification: Some(PhaseStatus::Clean),
                stdout: "ok".into(),
                stderr: String::new(),
                error: None,
            },
        };
        let contents = format_deferred_artifact(&log).unwrap();
        let argv = contents
            .lines()
            .find_map(|line| line.strip_prefix("argv: "))
            .expect("argv line");
        assert_eq!(
            serde_json::from_str::<Vec<String>>(argv).unwrap(),
            vec!["checker tool", "argument with spaces", "line\nbreak"]
        );
        assert_eq!(
            safe_artifact_component("../../tool/name"),
            "______tool_name"
        );
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

        /// Property: overlapping exit-code policy lists have a documented
        /// precedence (clean, then issues, then failure), and unlisted values
        /// use exactly the configured fallback.
        #[test]
        fn exit_code_classification_has_stable_precedence(
            clean in prop::collection::vec(any::<i32>(), 0..30),
            issues in prop::collection::vec(any::<i32>(), 0..30),
            failure in prop::collection::vec(any::<i32>(), 0..30),
            code in any::<i32>(),
            unexpected_issues in any::<bool>(),
        ) {
            let unexpected = if unexpected_issues {
                UnexpectedExitPolicy::Issues
            } else {
                UnexpectedExitPolicy::Failure
            };
            let policy = ExitCodePolicy {
                clean: clean.clone(),
                issues: issues.clone(),
                failure: failure.clone(),
                unexpected,
            };
            let expected = if clean.contains(&code) {
                PhaseStatus::Clean
            } else if issues.contains(&code) {
                PhaseStatus::Issues
            } else if failure.contains(&code) {
                PhaseStatus::Failure
            } else if unexpected_issues {
                PhaseStatus::Issues
            } else {
                PhaseStatus::Failure
            };

            prop_assert_eq!(classify_exit_code(&policy, code), expected);
        }

        /// Property: lexical normalization for not-yet-created output paths is
        /// idempotent, absolute, and cannot retain traversal above root.
        #[test]
        fn non_existing_output_path_normalization_is_stable(
            segments in prop::collection::vec(prop_oneof![Just(".".to_owned()), Just("..".to_owned()), "[a-z]{1,8}"], 0..30),
        ) {
            let path = PathBuf::from(format!(
                "/hookkit-property-path-that-does-not-exist/{}/{}",
                std::process::id(),
                segments.join("/")
            ));
            let once = normalize_path(&path);
            let twice = normalize_path(&once);

            prop_assert_eq!(&once, &twice);
            prop_assert!(once.is_absolute());
            let contains_traversal = once.components().any(|component| {
                matches!(component, Component::CurDir | Component::ParentDir)
            });
            prop_assert!(!contains_traversal);
        }
    }

    fn job_with_file(root: &Path, name: &str) -> ToolJob {
        ToolJob {
            workspace_dir: root.to_path_buf(),
            workspace_indicator: None,
            files: vec![root.join(name)],
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

    #[test]
    fn extra_args_expand_tool_then_workflow_then_command() {
        let args = |values: &[&str]| values.iter().map(|v| v.to_string()).collect::<Vec<_>>();
        let command = |extra: &[&str]| pkl::WorkflowCommand {
            argv: vec![pkl::ArgvElement::Token(pkl::ArgToken::ExtraArgs)],
            extra_args: args(extra),
            ..pkl::WorkflowCommand::default()
        };
        let explicit = pkl::ToolSpec {
            id: "ruff".into(),
            executable: "ruff".into(),
            extra_args: args(&["--tool"]),
            env: BTreeMap::from([("RUFF_CACHE_DIR".into(), "/tmp/cache".into())]),
            timeout_seconds: Some(7),
            workflows: BTreeMap::from([(
                "lint".into(),
                pkl::Workflow {
                    check: Some(command(&["--check"])),
                    remedy: Some(pkl::WorkflowCommand {
                        writes: pkl::WriteBehavior::TargetFiles,
                        ..command(&["--remedy"])
                    }),
                    extra_args: args(&["--ignore", "F401"]),
                    ..pkl::Workflow::default()
                },
            )]),
            ..pkl::ToolSpec::default()
        };
        let settings = pkl::Settings {
            local_bin_dirs: args(&["bin"]),
            ..pkl::Settings::default()
        };
        let spec = convert_tool_spec(&explicit, &settings);
        let workflow = &spec.workflows[0];
        assert_eq!(
            workflow.check.as_ref().unwrap().extra_args,
            args(&["--tool", "--ignore", "F401", "--check"])
        );
        assert_eq!(
            workflow.remedy.as_ref().unwrap().extra_args,
            args(&["--tool", "--ignore", "F401", "--remedy"])
        );
        assert_eq!(
            spec.env,
            vec![("RUFF_CACHE_DIR".to_owned(), "/tmp/cache".to_owned())]
        );
        assert_eq!(spec.timeout, Some(Duration::from_secs(7)));
        assert_eq!(spec.local_bin_dirs, args(&["bin"]));

        let phased = pkl::ToolSpec {
            id: "fmt".into(),
            executable: "fmt".into(),
            extra_args: args(&["--tool"]),
            timeout_seconds: Some(0),
            phases: BTreeMap::from([
                (
                    "format".into(),
                    pkl::Phase {
                        mode: pkl::PhaseMode::Format,
                        writes: pkl::WriteBehavior::TargetFiles,
                        extra_args: args(&["--format"]),
                        ..pkl::Phase::default()
                    },
                ),
                (
                    "verify".into(),
                    pkl::Phase {
                        extra_args: args(&["--verify"]),
                        ..pkl::Phase::default()
                    },
                ),
            ]),
            ..pkl::ToolSpec::default()
        };
        let spec = convert_tool_spec(&phased, &pkl::Settings::default());
        assert_eq!(spec.phases[0].extra_args, args(&["--tool", "--format"]));
        assert_eq!(spec.phases[1].extra_args, args(&["--tool", "--verify"]));
        // Phase-translated workflows reuse the converted phases.
        let translated = &spec.workflows[0];
        assert_eq!(
            translated.remedy.as_ref().unwrap().extra_args,
            args(&["--tool", "--format"])
        );
        assert_eq!(
            translated.check.as_ref().unwrap().extra_args,
            args(&["--tool", "--verify"])
        );
        assert_eq!(spec.timeout, None, "timeoutSeconds = 0 disables the limit");
    }

    #[cfg(unix)]
    #[test]
    fn local_bin_dirs_resolve_nearest_first_before_path() {
        let root = unique_test_directory("local-bin");
        let workspace = root.join("packages/web");
        for dir in [
            root.join("node_modules/.bin"),
            workspace.join("node_modules/.bin"),
            workspace.join(".venv/bin"),
        ] {
            std::fs::create_dir_all(&dir).unwrap();
        }
        let write_tool = |path: PathBuf, mode: u32| {
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        };
        let root_eslint = write_tool(root.join("node_modules/.bin/eslint"), 0o755);
        let nested_eslint = write_tool(workspace.join("node_modules/.bin/eslint"), 0o755);
        let venv_eslint = write_tool(workspace.join(".venv/bin/eslint"), 0o755);
        write_tool(root.join("node_modules/.bin/not-executable"), 0o644);
        let ruff = write_tool(workspace.join(".venv/bin/ruff"), 0o755);

        let mut spec = ToolSpec::new("eslint", "ESLint", "eslint");
        spec.local_bin_dirs = pkl::default_local_bin_dirs();
        let context = ToolContext {
            spec: &spec,
            project_root: &root,
            global_diagnostics_dir: None,
        };
        let job = job_with_file(&workspace, "a.ts");
        let root_job = job_with_file(&root, "a.ts");
        let resolve = |program: &str, job: &ToolJob| resolve_program(program, job, &context);

        assert_eq!(resolve("eslint", &job), path_arg(&nested_eslint));
        assert_eq!(resolve("eslint", &root_job), path_arg(&root_eslint));
        assert_eq!(resolve("ruff", &job), path_arg(&ruff));
        assert_eq!(resolve("not-executable", &job), "not-executable");
        assert_eq!(resolve("absent", &job), "absent");
        assert_eq!(resolve("/usr/bin/env", &job), "/usr/bin/env");
        assert_ne!(resolve("eslint", &job), path_arg(&venv_eslint));

        // Without a workspace indicator the job runs from the project root,
        // but a package's own install still wins for its files.
        let package_job = ToolJob {
            workspace_dir: root.clone(),
            workspace_indicator: None,
            files: vec![workspace.join("src/a.ts")],
        };
        assert_eq!(resolve("eslint", &package_job), path_arg(&nested_eslint));
        // A policy above the repository still finds the repository's tools.
        let outer = local_program(
            "eslint",
            &[workspace.join("src").as_path()],
            root.parent().unwrap(),
            &pkl::default_local_bin_dirs(),
        );
        assert_eq!(outer, Some(nested_eslint.clone()));

        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn commands_receive_tool_env_and_are_killed_at_the_timeout() {
        let root = std::env::temp_dir();
        let phase = ToolPhase::new("verify", PhaseMode::Verify);
        let command = |script: &str, timeout_ms: u64| RenderedCommand {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: vec![("VELVET_GLOVE_PROBE".into(), "probe-value".into())],
            timeout: Some(Duration::from_millis(timeout_ms)),
        };

        let log = run_phase_command(
            &phase,
            &command("printf %s \"$VELVET_GLOVE_PROBE\"", 5_000),
            &root,
        );
        assert_eq!(log.stdout, "probe-value");
        assert_eq!(log.classification, Some(PhaseStatus::Clean));

        let started = std::time::Instant::now();
        let log = run_phase_command(
            &phase,
            &command("printf started; sleep 30 & wait", 200),
            &root,
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timeout must kill the command"
        );
        assert_eq!(log.classification, None);
        assert!(
            log.error.as_deref().unwrap().contains("timed out after"),
            "{log:?}"
        );
        assert_eq!(log.stdout, "started");

        // A descendant that outlives the command and keeps its output pipes
        // open must not hold the hook past the timeout (or, without one,
        // past a short grace period).
        for timeout in [Some(Duration::from_millis(1_500)), None] {
            let started = std::time::Instant::now();
            let log = run_phase_command(
                &phase,
                &RenderedCommand {
                    timeout,
                    ..command("(sleep 8) & echo checked; exit 0", 0)
                },
                &root,
            );
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "lingering descendant held the pipes for {:?} ({timeout:?})",
                started.elapsed()
            );
            assert_eq!(log.stdout, "checked\n");
            assert_eq!(log.classification, Some(PhaseStatus::Clean));
        }
    }

    #[test]
    fn compatibility_workflows_inherit_phase_invocation() {
        let schema = pkl::ToolSpec {
            id: "jq".into(),
            executable: "jq".into(),
            phase_invocation: pkl::InvocationGranularity::PerFile,
            phases: BTreeMap::from([("verify".into(), pkl::Phase::default())]),
            phase_order: vec!["verify".into()],
            ..pkl::ToolSpec::default()
        };

        let spec = convert_tool_spec(&schema, &pkl::Settings::default());

        assert_eq!(spec.phase_invocation, InvocationGranularity::PerFile);
        assert_eq!(spec.workflows.len(), 1);
        assert_eq!(spec.workflows[0].id, "verify");
        assert_eq!(spec.workflows[0].invocation, InvocationGranularity::PerFile);
        assert!(spec.workflows[0].compatibility_translation);
        assert!(spec.workflows[0].check.is_some());
        assert!(spec.workflows[0].remedy.is_none());
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
