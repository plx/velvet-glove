//! Immediate PostToolUse runner: runs each configured tool on the files one
//! tool call changed and lowers the collected result to the harness.

mod diagnostics;
mod lowering;
mod messages;
mod outcomes;
mod output;
pub(crate) use diagnostics::immediate_log_directory;
pub(crate) use diagnostics::write_immediate_artifact;

pub(crate) use lowering::lower_domain_outcome;
pub(crate) use lowering::lowering_warning_artifact;
pub(crate) use outcomes::accumulate_outcomes;
pub(crate) use output::ExcerptLimits;
pub(crate) use output::is_empty_output;
pub use output::{RunnerDomainOutcome, RunnerPostToolUseOutput};
