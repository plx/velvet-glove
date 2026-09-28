mod attribution;
mod execution;
#[cfg(all(test, unix))]
mod execution_tests;
mod guard;
mod lowering;
mod model;
mod reporting;

pub(crate) use execution::{
    DeferredLog, ScheduledWorkflow, combined_output, execute_deferred_workflows,
};
pub(crate) use guard::{LoopGuardState, decide as decide_loop_guard, issue_fingerprint};
pub(crate) use lowering::{DEFAULT_BLOCK_REASON, StopLoweringMetadata, plan_stop_lowering};
pub(crate) use reporting::{
    BlockReasons, DeferredReporter, RenderedBuckets, RenderedMessages, TemplateRun, problem_entries,
};

pub use model::{
    ArtifactClassification, CheckOutcome, CommandPhase, CoverageGap, DeferredRunResult,
    FileAssessment, FileResult, FileStatus, OperationalProblem, RunArtifact, ToolReport,
    ToolReportRef,
};
pub use reporting::{IssueExcerpt, ProblemSummary};
