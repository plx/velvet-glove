//! Immediate PostToolUse runner: runs each configured tool on the files one
//! tool call changed and lowers the collected result to the harness.

mod diagnostics;
mod messages;
mod output;
pub(crate) use diagnostics::immediate_log_directory;
pub(crate) use diagnostics::report_with_artifact;
pub(crate) use diagnostics::runner_artifact_key;
pub(crate) use diagnostics::write_diagnostics;
pub(crate) use diagnostics::write_immediate_artifact;
pub(crate) use messages::MessageArgs;
pub(crate) use messages::render_template;
pub(crate) use messages::template_failure_notice;

pub(crate) use output::AgentFeedback;
pub(crate) use output::AutoFixed;
pub(crate) use output::ExcerptLimits;
pub(crate) use output::PendingIssues;
pub(crate) use output::auto_fixed_line;
pub(crate) use output::is_empty_output;
pub use output::{RunnerDomainOutcome, RunnerPostToolUseOutput};
