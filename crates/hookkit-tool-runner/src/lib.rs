//! Reusable runners for immediate post-tool and session-batched completion hooks.
#![deny(missing_docs)]
//!
//! The runner reads a harness-native post-tool-use event from stdin, loads the
//! Pkl-driven tool catalog through [`hookkit_pkl_config`], runs each tool in
//! the configured order, and lowers a unified result to the selected harness.
//! The completion runner consumes exact snapshots from
//! [`hookkit_session_state`] and commits detailed run bundles before deciding
//! whether a turn may stop.

mod deferred;
mod excerpt;
mod vcs;

pub use deferred::{
    ArtifactClassification, CheckOutcome, CommandPhase, CoverageGap, DeferredRunResult,
    FileAssessment, FileResult, FileStatus, OperationalProblem, RunArtifact, ToolReport,
    ToolReportRef,
};
use deferred::{
    BlockReasons, DEFAULT_BLOCK_REASON, DeferredLog, DeferredReporter, LoopGuardState,
    RenderedBuckets, RenderedMessages, ScheduledWorkflow, StopLoweringMetadata, TemplateRun,
    combined_output, decide_loop_guard, execute_deferred_workflows, issue_fingerprint,
    plan_stop_lowering,
};

use globset::{Glob, GlobSet, GlobSetBuilder};
use hookkit_common::message::{DiagnosticArtifact, DiagnosticReport};
use hookkit_common::{
    NoticeLevel, PostToolUseCommandEnvironment, PostToolUseInput, PostToolUseOutput,
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

const DEFAULT_CLEAN_CHANGED_AGENT: &str = "{{ tool }} changed {{ changed_files | join(\", \") }}; re-read changed files before editing further.";
const DEFAULT_ISSUES_AGENT: &str =
    "velvet-glove: {{ tool }} reports issues in {{ issue_files | join(\", \") }}:\n{{ excerpt }}";
const DEFAULT_ISSUES_CHANGED_AGENT: &str = "velvet-glove: {{ tool }} changed {{ changed_files | join(\", \") }} (re-read before editing); issues remain in {{ issue_files | join(\", \") }}:\n{{ excerpt }}";

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
            clean_changed_agent: DEFAULT_CLEAN_CHANGED_AGENT.to_string(),
            issues_agent: DEFAULT_ISSUES_AGENT.to_string(),
            issues_changed_agent: DEFAULT_ISSUES_CHANGED_AGENT.to_string(),
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
    candidates.sort();
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
    let mut execution = execute_deferred_workflows(&plan, settings.jobs, settings.fail_fast);
    let tools = write_deferred_artifacts(
        &run,
        &plan,
        &planned_tools,
        &execution.logs,
        &mut execution.result,
    )?;
    let mut result = execution.result;
    record_activity_resolution(&mut result, &commit.resolution);

    let operational_files = result
        .operational_problems
        .values()
        .flat_map(|problem| problem.affected_files.iter().cloned())
        .collect::<BTreeSet<_>>();
    for candidate in &commit.candidates {
        if !result.files.contains_key(candidate) && !operational_files.contains(candidate) {
            result.record_uncovered(candidate.clone());
        }
    }

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
        let stop_hook_active = self
            .session
            .stop_hook_active
            .unwrap_or(previous.consecutive_blocks > 0);
        let decision = decide_loop_guard(
            &previous,
            stop_hook_active,
            blocks.any(),
            &fingerprint,
            policy.max_consecutive_blocks,
        );
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
        let runs_directory = run.directory().parent().map(Path::to_path_buf);
        run.commit(&summary).map_err(state_error)?;
        if let Some(missing) = hard_failure {
            return Err(invalid_data(format!(
                "missingToolPolicy is hard-failure and a configured tool is missing: {missing}"
            )));
        }
        let output = lowering.finish()?;
        apply_deferred_state_disposition(self.session.activity_store, disposition, run_id)?;
        decision.next.save(&guard_path);
        if let Some(runs) = runs_directory {
            prune_run_bundles(&runs, RETAINED_RUN_BUNDLES);
        }
        let state_root = StateRoot::new(self.session.activity_store.state().state_root());
        let _ = SessionState::gc(&state_root, STALE_SESSION_AGE);
        Ok(EntityOutcome::acknowledge(output))
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

/// Keep the newest `keep` run bundles (named `<millis>-...`) in `runs`.
fn prune_run_bundles(runs: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(runs) else {
        return;
    };
    let mut bundles = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
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
    for (_, stale) in bundles.into_iter().skip(keep) {
        let _ = std::fs::remove_dir_all(stale);
    }
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

fn write_deferred_artifacts(
    run: &RunBundle,
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
        let absolute = run.write_text(&relative, &contents).map_err(state_error)?;
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
    let first_line = detail
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .trim_end_matches(':');
    let first_line = excerpt::clip(first_line, 1, 200).text;
    let message = format!(
        "velvet-glove {headline} ({first_line}). Details: {}",
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

    // Most tool calls (Read, Grep, ...) touch no files: skip every other cost,
    // including Pkl evaluation, for them.
    let mut candidates = discover_modified_files(&post_tool, ctx)
        .into_iter()
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return lower_domain_outcome(
            harness,
            RunnerDomainOutcome::Clean,
            lowering_warning_artifact.as_ref(),
        );
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
        return lower_domain_outcome(
            harness,
            RunnerDomainOutcome::Clean,
            lowering_warning_artifact.as_ref(),
        );
    }
    let loaded = match hookkit_pkl_config::discover_and_load(&cwd, config_path) {
        Ok(loaded) => loaded,
        // A broken policy is an operational problem: tell the user, never the
        // agent, and never fail or block the tool call.
        Err(error) => {
            let output = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::default())
                .with_user_notice(UserNotice::error(format!(
                    "velvet-glove: configuration error; no tools ran: {}",
                    truncate_chars(error.to_string().trim(), CONFIG_ERROR_NOTICE_CHARS)
                )));
            return lower_domain_outcome(
                harness,
                RunnerDomainOutcome::Report(output),
                lowering_warning_artifact.as_ref(),
            );
        }
    };

    let project_root = normalize_path(&loaded.project_root);
    let lowering = loaded.config.settings.lowering_policy;
    let missing_tool_policy = loaded.config.settings.missing_tool_policy;
    let fail_fast = loaded.config.settings.fail_fast;
    let continue_after_issues = loaded.config.settings.continue_after_issues;

    let mut output =
        RunnerPostToolUseOutput::new(lowering).with_excerpt_budget(ExcerptBudget::new(
            &loaded.config.settings.deferred_reporting,
            display_roots(&project_root, &loaded.project_root),
        ));
    let mut had_hard_failure = false;
    let mut had_harness_block_message: Option<String> = None;

    let tools = resolve_run_order(&loaded.config)?;
    if tools.is_empty() {
        return lower_domain_outcome(
            harness,
            RunnerDomainOutcome::Clean,
            lowering_warning_artifact.as_ref(),
        );
    }

    let global_diagnostics_dir = loaded.config.settings.diagnostics_directory.clone();

    for schema_spec in tools {
        if !schema_spec.enabled {
            continue;
        }
        let spec = convert_tool_spec(schema_spec, &loaded.config.settings);
        let context = ToolContext {
            spec: &spec,
            project_root: &project_root,
            global_diagnostics_dir: global_diagnostics_dir.as_deref(),
        };

        let matcher = FileMatcher::new(&spec.file_selection)?;
        let runnable_paths = candidates
            .iter()
            .filter(|p| matcher.matches(p, &project_root))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        if runnable_paths.is_empty() {
            continue;
        }

        let base_jobs = build_jobs(&runnable_paths, &project_root, &spec);
        let jobs = invocation_jobs(&base_jobs, spec.phase_invocation);
        if jobs.is_empty() {
            continue;
        }

        let outcomes = run_jobs(&jobs, &context, loaded.config.settings.jobs);

        let batch_status = accumulate_outcomes(
            outcomes,
            &context,
            ctx,
            missing_tool_policy,
            &mut output,
            &mut had_hard_failure,
            &mut had_harness_block_message,
        )?;

        if had_harness_block_message.is_some()
            || had_hard_failure
            || (fail_fast && batch_status.operational_failure)
            || (!continue_after_issues && batch_status.issues)
        {
            break;
        }
    }

    let outcome = if let Some(message) = had_harness_block_message {
        RunnerDomainOutcome::HarnessBlock { message, output }
    } else if had_hard_failure {
        RunnerDomainOutcome::OperationalFailure {
            message: "tool unavailable with missingToolPolicy=hard-failure".into(),
        }
    } else if is_empty_output(&output) {
        RunnerDomainOutcome::Clean
    } else {
        RunnerDomainOutcome::Report(output)
    };
    lower_domain_outcome(harness, outcome, lowering_warning_artifact.as_ref())
}

/// Accumulated common output produced by the post-tool runner.
///
/// Fields are runner-owned so callers receive this value through
/// [`RunnerDomainOutcome`] and lower it with the selected harness workflow.
#[derive(Debug, Default)]
pub struct RunnerPostToolUseOutput {
    notices: Vec<UserNotice>,
    agent_feedback: Vec<String>,
    diagnostics: Vec<DiagnosticReport>,
    auto_fixed: Vec<AutoFixed>,
    harness_block: Option<String>,
    lowering: pkl::LoweringPolicy,
    excerpt_budget: ExcerptBudget,
}

/// Agent excerpt budget shared by every tool in one immediate run, plus the
/// absolute prefixes that excerpts rewrite to project-relative paths. The
/// limits are the deferred reporter's (`deferredReporting.excerptMax*`).
#[derive(Debug, Default)]
struct ExcerptBudget {
    lines: usize,
    chars: usize,
    roots: Vec<PathBuf>,
}

impl ExcerptBudget {
    fn new(reporting: &pkl::DeferredReporting, roots: Vec<PathBuf>) -> Self {
        Self {
            lines: reporting.excerpt_max_lines as usize,
            chars: reporting.excerpt_max_chars as usize,
            roots,
        }
    }

    /// A bounded, ANSI-free, project-relative excerpt of `output`, charged
    /// against the remaining budget. A cut excerpt points at `log`.
    fn take(&mut self, output: &str, log: &Path) -> String {
        let roots = self.roots.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        let clipped = excerpt::clip(&excerpt::normalize(output, &roots), self.lines, self.chars);
        self.lines = self.lines.saturating_sub(clipped.text.lines().count());
        self.chars = self.chars.saturating_sub(clipped.text.chars().count());
        excerpt::with_log_note(&clipped, Some(&log.to_string_lossy()))
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

/// Longest configuration error echoed in a user notice.
const CONFIG_ERROR_NOTICE_CHARS: usize = 1_500;

fn truncate_chars(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

/// One terse line naming every auto-fixed file and the tools that changed it:
/// `velvet-glove auto-fixed a.py (Ruff), b.ts (Prettier); re-read before editing.`
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
            .map(|(file, tools)| format!("{file} ({})", tools.join(", ")))
            .collect::<Vec<_>>()
            .join(", ");
        format!("velvet-glove auto-fixed {listed}; re-read before editing.")
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

    fn with_agent_feedback(mut self, feedback: impl Into<String>) -> Self {
        self.agent_feedback.push(feedback.into());
        self
    }

    fn with_diagnostic_report(mut self, report: DiagnosticReport) -> Self {
        self.diagnostics.push(report);
        self
    }

    fn with_harness_block(mut self, message: impl Into<String>) -> Self {
        self.harness_block = Some(message.into());
        self
    }

    fn with_auto_fixed(mut self, auto_fixed: AutoFixed) -> Self {
        self.auto_fixed.push(auto_fixed);
        self
    }

    fn with_excerpt_budget(mut self, budget: ExcerptBudget) -> Self {
        self.excerpt_budget = budget;
        self
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
            agent_feedback: &output.agent_feedback,
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
    output: RunnerPostToolUseOutput,
    lowering_warning_artifact: Option<&LoweringWarningArtifact>,
) -> hookkit_core::Result<PostToolUseOutput> {
    if let Some(message) = output.harness_block {
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
    let context = auto_fixed_line(output.auto_fixed.iter().filter(|entry| entry.in_agent_line))
        .into_iter()
        .chain(output.agent_feedback.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n");

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

fn format_notice(notice: &UserNotice) -> String {
    match notice.level {
        NoticeLevel::Info => notice.text.clone(),
        NoticeLevel::Warning => format!("warning: {}", notice.text),
        NoticeLevel::Error => format!("error: {}", notice.text),
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

/// Resolve the `run` list to ordered tool specs.
fn resolve_run_order(config: &pkl::RunnerConfig) -> hookkit_core::Result<Vec<&pkl::ToolSpec>> {
    let mut tools = Vec::with_capacity(config.run.len());
    for id in &config.run {
        let Some(spec) = config.tools.get(id) else {
            return Err(invalid_data(format!(
                "run references unknown tool `{id}`; define it under `tools` or remove it from `run`"
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

struct FileMatcher {
    include: GlobSet,
    exclude: GlobSet,
    include_all: bool,
}

impl FileMatcher {
    fn new(config: &FileSelection) -> hookkit_core::Result<Self> {
        Ok(Self {
            include: build_globset(&config.include)?,
            exclude: build_globset(&config.exclude)?,
            include_all: config.include.is_empty(),
        })
    }

    /// Globs match the project-relative path. Only a path outside the project
    /// root is matched as an absolute path, so unanchored excludes such as
    /// `**/target/**` never fire on the directories *containing* the project.
    fn matches(&self, absolute_path: &Path, project_root: &Path) -> bool {
        let path = slash_path(
            absolute_path
                .strip_prefix(project_root)
                .unwrap_or(absolute_path),
        );
        (self.include_all || self.include.is_match(&path)) && !self.exclude.is_match(&path)
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

fn build_jobs(paths: &[PathBuf], project_root: &Path, spec: &ToolSpec) -> Vec<ToolJob> {
    if let Some(indicator) = &spec.workspace_indicator {
        let mut grouped = BTreeMap::<PathBuf, ToolJob>::new();
        for path in paths {
            if let Some(indicator_path) = nearest_workspace_indicator(path, project_root, indicator)
            {
                let workspace_dir = indicator_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| project_root.to_path_buf());
                grouped
                    .entry(workspace_dir.clone())
                    .or_insert_with(|| ToolJob {
                        workspace_dir,
                        workspace_indicator: Some(indicator_path),
                        files: Vec::new(),
                    })
                    .files
                    .push(path.clone());
            }
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

fn nearest_workspace_indicator(
    path: &Path,
    project_root: &Path,
    indicator: &str,
) -> Option<PathBuf> {
    let mut current = path.parent();
    while let Some(dir) = current {
        let candidate = dir.join(indicator);
        if candidate.is_file() {
            return Some(candidate);
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
    files: Vec<PathBuf>,
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
    let mut logs = Vec::new();
    let mut saw_issues = false;
    let mut verify_state = None;
    let mut verifier_issue_output = Vec::new();
    let mut phase_issue_output = Vec::new();

    for phase in &context.spec.phases {
        if !phase.enabled {
            continue;
        }

        let command = render_command(phase, job, context);
        let log = run_phase_command(phase, &command, &job.workspace_dir);

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
    let issue_output = match (issues, verify_state) {
        (IssueState::Clean, _) => Vec::new(),
        (IssueState::Issues, Some(_)) => verifier_issue_output,
        (IssueState::Issues, None) => phase_issue_output,
    };
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
        issue_output: issue_output.join("\n"),
        files: job.files.clone(),
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
/// directories: each directory (in order) is tried from the job's workspace up
/// to the project root, nearest first. Anything else is left to `PATH`.
fn resolve_program(program: &str, job: &ToolJob, context: &ToolContext<'_>) -> String {
    if Path::new(program).components().count() != 1 {
        return program.to_owned();
    }
    for bin_dir in &context.spec.local_bin_dirs {
        for dir in job
            .workspace_dir
            .ancestors()
            .take_while(|dir| dir.starts_with(context.project_root))
        {
            let candidate = dir.join(bin_dir).join(program);
            if is_executable_file(&candidate) {
                return path_arg(&candidate);
            }
        }
    }
    program.to_owned()
}

fn is_executable_file(path: &Path) -> bool {
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

/// Run a command with captured output, killing it (and, on Unix, its process
/// group) if it outlives `command.timeout`.
fn execute_command(command: &RenderedCommand, cwd: &Path) -> std::io::Result<CommandOutput> {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc;

    fn drain(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<Vec<u8>> {
        let (sender, receiver) = mpsc::channel();
        if let Some(mut pipe) = pipe {
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let _ = pipe.read_to_end(&mut buffer);
                let _ = sender.send(buffer);
            });
        }
        receiver
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

    let mut timed_out = None;
    let status = match command.timeout {
        None => child.wait()?,
        Some(timeout) => {
            let deadline = std::time::Instant::now() + timeout;
            let mut delay = Duration::from_millis(1);
            loop {
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                let now = std::time::Instant::now();
                if now >= deadline {
                    kill_process_tree(&mut child);
                    timed_out = Some(timeout);
                    break child.wait()?;
                }
                std::thread::sleep(delay.min(deadline - now));
                delay = (delay * 2).min(Duration::from_millis(20));
            }
        }
    };
    // A timed-out tool may leave descendants holding the pipes open; take
    // whatever output arrives promptly rather than waiting for them.
    let collect = |receiver: mpsc::Receiver<Vec<u8>>| match timed_out {
        None => receiver.recv().unwrap_or_default(),
        Some(_) => receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_default(),
    };
    Ok(CommandOutput {
        status: status.code(),
        stdout: collect(stdout),
        stderr: collect(stderr),
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

fn snapshot_scope(job: &ToolJob, context: &ToolContext<'_>) -> BTreeSet<PathBuf> {
    let mut scope = BTreeSet::new();
    let mut include_target_files = false;
    let mut include_matching_globs = false;
    let mut include_workspace = false;

    for phase in &context.spec.phases {
        if !phase.enabled {
            continue;
        }
        match phase.writes {
            WriteBehavior::None => {}
            WriteBehavior::TargetFiles => include_target_files = true,
            WriteBehavior::MatchingGlobs => include_matching_globs = true,
            WriteBehavior::Workspace => include_workspace = true,
        }
    }

    if include_target_files {
        scope.extend(job.files.iter().cloned());
    }
    if include_matching_globs {
        scope.extend(collect_matching_files(
            &job.workspace_dir,
            &context.spec.file_selection,
        ));
    }
    if include_workspace {
        scope.extend(collect_workspace_files(&job.workspace_dir));
    }
    scope
}

fn collect_matching_files(base: &Path, selection: &FileSelection) -> BTreeSet<PathBuf> {
    let matcher = match FileMatcher::new(selection) {
        Ok(matcher) => matcher,
        Err(_) => return BTreeSet::new(),
    };
    walk_files(base)
        .into_iter()
        .filter(|path| matcher.matches(path, base))
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
    had_hard_failure: &mut bool,
    had_harness_block_message: &mut Option<String>,
) -> hookkit_core::Result<ToolBatchStatus> {
    let mut changed_files = BTreeSet::new();
    let mut issue_files = BTreeSet::new();
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
                    issue_files.extend(completed.files);
                    issue_diagnostics.push(completed.diagnostics);
                    if !issue_outputs.contains(&completed.issue_output) {
                        issue_outputs.push(completed.issue_output);
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

    if !unavailable.is_empty() {
        match missing_tool_policy {
            pkl::MissingToolPolicy::UserNotice => {
                for (phase, executable, install_hint) in &unavailable {
                    let message = render_unavailable_message(
                        context,
                        phase,
                        executable,
                        install_hint.as_deref(),
                    )?;
                    *output = std::mem::take(output).with_user_notice(UserNotice::warning(message));
                }
            }
            pkl::MissingToolPolicy::HardFailure => {
                *had_hard_failure = true;
                return Ok(status);
            }
            pkl::MissingToolPolicy::HarnessBlock => {
                if let Some((phase, executable, install_hint)) = unavailable.first() {
                    let message = render_unavailable_message(
                        context,
                        phase,
                        executable,
                        install_hint.as_deref(),
                    )?;
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
        let artifact = write_diagnostics("tool-failure", &diagnostics, context, ctx)?;
        let (phase, _, error, _) = &failure_diagnostics[0];
        let message = render_failed_message(context, &artifact, phase, error.as_deref())?;
        *output = std::mem::take(output)
            .with_user_notice(UserNotice::error(message))
            .with_diagnostic_report(report_with_artifact(
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
        let artifact = write_diagnostics("tool-issues", &diagnostics, context, ctx)?;
        let excerpt = output
            .excerpt_budget
            .take(&issue_outputs.join("\n"), &artifact);
        *output = std::mem::take(output)
            .with_user_notice(UserNotice::warning(format!(
                "{}: issues remain in {}; diagnostics: {}",
                context.spec.display_name,
                issue_paths.join(", "),
                artifact.display()
            )))
            .with_diagnostic_report(report_with_artifact(
                format!("{} diagnostics", context.spec.display_name),
                diagnostics,
                artifact.clone(),
                context.project_root,
            ));

        let template = if changed_paths.is_empty() {
            context.spec.messages.issues_agent.clone()
        } else {
            context.spec.messages.issues_changed_agent.clone()
        };
        let rendered = render_template(
            &template,
            context,
            &changed_paths,
            &issue_paths,
            Some(&artifact),
            None,
            &excerpt,
        )?;
        *output = std::mem::take(output).with_agent_feedback(rendered);
    } else if !changed_paths.is_empty() {
        // Tools on the default template share one consolidated agent line;
        // a customised `cleanChangedAgent` is rendered as configured.
        let template = &context.spec.messages.clean_changed_agent;
        let in_agent_line = template == DEFAULT_CLEAN_CHANGED_AGENT;
        if !in_agent_line {
            let rendered = render_template(
                template,
                context,
                &changed_paths,
                &issue_paths,
                None,
                None,
                "",
            )?;
            *output = std::mem::take(output).with_agent_feedback(rendered);
        }
        *output = std::mem::take(output).with_auto_fixed(AutoFixed {
            tool: context.spec.display_name.clone(),
            files: changed_paths,
            in_agent_line,
        });
    }

    status.operational_failure = status.operational_failure || *had_hard_failure;
    Ok(status)
}

fn render_unavailable_message(
    context: &ToolContext<'_>,
    phase: &str,
    executable: &str,
    install_hint: Option<&str>,
) -> hookkit_core::Result<String> {
    if let Some(template) = context.spec.messages.unavailable_user.as_ref() {
        return render_template(
            template,
            context,
            &[],
            &[],
            None,
            Some((phase, executable, install_hint)),
            "",
        );
    }

    let mut message = format!(
        "{}: `{}` is unavailable while running phase `{phase}`",
        context.spec.display_name, executable
    );
    if let Some(hint) = install_hint {
        message.push_str(&format!("; {hint}"));
    }
    Ok(message)
}

fn render_failed_message(
    context: &ToolContext<'_>,
    diagnostics_path: &Path,
    phase: &str,
    error: Option<&str>,
) -> hookkit_core::Result<String> {
    if let Some(template) = context.spec.messages.failed_user.as_ref() {
        return render_template(
            template,
            context,
            &[],
            &[],
            Some(diagnostics_path),
            Some((phase, "", None)),
            "",
        );
    }

    let reason = error.map(|error| format!(" ({error})")).unwrap_or_default();
    Ok(format!(
        "{}: phase `{phase}` failed{reason}; diagnostics: {}",
        context.spec.display_name,
        diagnostics_path.display()
    ))
}

fn render_template(
    template: &str,
    context: &ToolContext<'_>,
    changed_files: &[String],
    issue_files: &[String],
    diagnostics_path: Option<&Path>,
    phase_error: Option<(&str, &str, Option<&str>)>,
    excerpt: &str,
) -> hookkit_core::Result<String> {
    let diagnostics_path_text = diagnostics_path
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let diagnostics_rel_path = diagnostics_path
        .and_then(|path| path.strip_prefix(context.project_root).ok())
        .map(slash_path)
        .unwrap_or_default();
    let (phase, executable, install_hint) = phase_error.unwrap_or(("", "", None));
    let json_context = serde_json::json!({
        "tool": context.spec.display_name,
        "tool_id": context.spec.id,
        "changed_files": changed_files,
        "issue_files": issue_files,
        "diagnostics_path": diagnostics_path_text,
        "diagnostics_absolute_path": diagnostics_path_text,
        "diagnostics_rel_path": diagnostics_rel_path,
        "diagnostics_project_path": diagnostics_rel_path,
        "project_root": context.project_root.to_string_lossy(),
        "phase": phase,
        "executable": executable,
        "install_hint": install_hint.unwrap_or(""),
        "excerpt": excerpt,
    });

    Environment::new()
        .render_str(template, &json_context)
        .map_err(|e| invalid_data(format!("failed to render message template: {e}")))
}

fn write_diagnostics(
    label: &str,
    diagnostics: &str,
    context: &ToolContext<'_>,
    ctx: &RuntimeContext<'_>,
) -> hookkit_core::Result<PathBuf> {
    let base_dir = match (
        context.spec.diagnostics_directory.as_deref(),
        context.global_diagnostics_dir,
    ) {
        (Some(dir), _) => absolute_from(Path::new(dir), context.project_root),
        (None, Some(dir)) => absolute_from(Path::new(dir), context.project_root),
        (None, None) => std::env::temp_dir()
            .join("velvet-glove")
            .join("state")
            .join("post-tool-immediate"),
    };
    let manager = ArtifactManager::new(base_dir)?;
    manager
        .write_text(
            &runner_artifact_key(ctx, format!("{}-{label}", context.spec.id)),
            diagnostics,
        )
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
            excerpt_max_lines: 3,
            ..Default::default()
        };
        let mut budget = ExcerptBudget::new(
            &reporting,
            display_roots(Path::new("/private/repo"), Path::new("/repo")),
        );
        let log = Path::new("/tmp/vg/issues.txt");
        assert_eq!(
            budget.take(
                "\u{1b}[31m/private/repo/src/a.py:1:1\u{1b}[0m: E1\n/repo/src/a.py:2:1: E2\n",
                log
            ),
            "src/a.py:1:1: E1\nsrc/a.py:2:1: E2"
        );
        // One line of the shared budget is left for the next tool.
        assert_eq!(
            budget.take("b.py:1: E3\nb.py:2: E4", log),
            "b.py:1: E3\n…truncated; full log: /tmp/vg/issues.txt"
        );
        assert_eq!(
            budget.take("c.py:1: E5", log),
            "(output omitted; full log: /tmp/vg/issues.txt)"
        );
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
                "velvet-glove auto-fixed src/a.py (Ruff, Black), src/b.py (Ruff), web/c.ts (Custom); re-read before editing.\nwarning: Lint: `lint` is unavailable",
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
        prune_run_bundles(&runs, 2);
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
