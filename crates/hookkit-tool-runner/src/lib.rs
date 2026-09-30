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
mod command;
mod convert;
mod deferred;
mod errors;
mod excerpt;
mod hooks;
mod immediate;
mod jobs;
mod matcher;
mod paths;
mod project_lock;
mod snapshot;
mod spec;
#[cfg(test)]
mod test_support;
mod vcs;

pub use check::{CheckError, CheckReport, CheckRequest, CheckStatus, run_check};
pub use command::{is_executable_file, local_program};
pub use deferred::{
    ArtifactClassification, CheckOutcome, CommandPhase, CoverageGap, DeferredRunResult,
    FileAssessment, FileResult, FileStatus, IssueExcerpt, OperationalProblem, ProblemSummary,
    RunArtifact, ToolReport, ToolReportRef,
};
pub use hooks::{
    Cli, FileActivityCli, SessionStartCli, TurnCompletionCli, run_file_activity_observer,
    run_runner, run_session_start_observer, run_turn_completion_runner,
};
pub use immediate::{RunnerDomainOutcome, RunnerPostToolUseOutput};
pub use matcher::FileMatcher;
pub use spec::{
    CheckScope, CommandArgTemplate, ExitCodePolicy, FileSelection, InvocationGranularity,
    PhaseMode, ToolMessages, ToolPhase, ToolSpec, ToolWorkflow, UnexpectedExitPolicy,
    WorkspaceFallback, WriteBehavior,
};
