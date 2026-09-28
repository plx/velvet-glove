//! Formatter and linter hooks for coding agents.
//!
//! `scaffold` holds the unified CLI and the adapters to the immediate and
//! deferred runners; `commands` holds the `tools`, `doctor`, and `init` setup
//! commands.

pub mod commands;
pub mod hooks;
pub mod scaffold;

/// Dispatch one parsed CLI invocation.
pub fn run(cli: scaffold::cli::Cli) -> std::process::ExitCode {
    scaffold::dispatch::run(cli)
}
