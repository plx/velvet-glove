mod attribution;
mod disposition;
mod execution;
#[cfg(all(test, unix))]
mod execution_tests;
mod guard;
mod lowering;
mod model;
mod reporting;

pub(crate) use attribution::{Attribution, attribute, resolution_bases, source_failure_files};
pub(crate) use execution::{
    DeferredLog, ScheduledWorkflow, combined_output, execute_deferred_workflows,
};
pub(crate) use guard::{LoopGuardState, decide as decide_loop_guard, issue_fingerprint};
pub(crate) use lowering::{DEFAULT_BLOCK_REASON, StopLoweringMetadata, plan_stop_lowering};
pub(crate) use reporting::{
    BlockReasons, DeferredReporter, RenderedBuckets, RenderedMessages, TemplateRun, problem_entries,
};

pub(crate) use disposition::ActivityResolution;
pub(crate) use disposition::DeferredStateDisposition;
pub(crate) use disposition::apply_deferred_state_disposition;
pub(crate) use disposition::plan_deferred_state_disposition;
pub(crate) use disposition::record_activity_resolution;
pub(crate) use disposition::source_gap_messages;
pub use model::{
    ArtifactClassification, CheckOutcome, CommandPhase, CoverageGap, DeferredRunResult,
    FileAssessment, FileResult, FileStatus, OperationalProblem, RunArtifact, ToolReport,
    ToolReportRef,
};
pub use reporting::{IssueExcerpt, ProblemSummary};
