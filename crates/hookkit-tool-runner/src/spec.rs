//! Public runtime model of a tool: its files, phases, workflows, exit codes, and messages.

use hookkit_pkl_config::schema as pkl;
use std::time::Duration;

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

    pub(crate) fn is_verifier(&self) -> bool {
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
