//! Deferred Stop path: plan, execute, attribute, report, and lower batched
//! turn-completion checks, and commit their run bundles and pending state.

mod artifacts;
mod attribution;
mod disposition;
mod execution;
#[cfg(all(test, unix))]
mod execution_tests;
mod guard;
mod lowering;
mod model;
mod plan;
mod reporting;
mod summary;
mod turn_completion;

pub(crate) use artifacts::{prune_run_bundles, write_deferred_artifacts};
pub(crate) use attribution::{Attribution, attribute, resolution_bases, source_failure_files};
pub(crate) use execution::{
    DeferredLog, ScheduledWorkflow, combined_output, execute_deferred_workflows,
};
pub(crate) use guard::{LoopGuardState, decide as decide_loop_guard, issue_fingerprint};
pub(crate) use lowering::{DEFAULT_BLOCK_REASON, StopLoweringMetadata, plan_stop_lowering};
pub(crate) use plan::build_deferred_plan;
pub(crate) use reporting::{
    BlockReasons, DeferredReporter, RenderedBuckets, RenderedMessages, TemplateRun, problem_entries,
};
pub(crate) use summary::BatchToolSummary;
pub(crate) use turn_completion::{record_uncovered_candidates, run_turn_completion_input};

pub use model::{
    ArtifactClassification, CheckOutcome, CommandPhase, CoverageGap, DeferredRunResult,
    FileAssessment, FileResult, FileStatus, OperationalProblem, RunArtifact, ToolReport,
    ToolReportRef,
};
pub use reporting::{IssueExcerpt, ProblemSummary};
