//! Immediate PostToolUse runner: runs each configured tool on the files one
//! tool call changed and lowers the collected result to the harness.

mod diagnostics;
pub(crate) use diagnostics::immediate_log_directory;
pub(crate) use diagnostics::report_with_artifact;
pub(crate) use diagnostics::runner_artifact_key;
pub(crate) use diagnostics::write_diagnostics;
pub(crate) use diagnostics::write_immediate_artifact;
