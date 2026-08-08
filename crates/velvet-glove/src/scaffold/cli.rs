use super::harness::Harness;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
/// Velvet Glove command-line interface.
#[derive(Debug, Parser)]
#[rustfmt::skip]
#[command(
    name = "velvet-glove",
    about = "Deferred linting and formatting for coding agents"
)]
pub struct Cli {
    /// Coding-agent harness that emitted this native hook event.
    #[arg(long, value_enum)]
    pub harness: Harness,

    /// Explicit Pkl policy. When omitted, Velvet Glove uses layered
    /// user/project/local discovery rooted at the hook event's workspace.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,


    /// Shared Velvet Glove state root override.
    #[arg(long, value_name = "PATH")]
    pub state_dir: Option<PathBuf>,

    /// Explicit hook event to execute.
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Parse arguments and reject cross-field combinations Clap cannot express
    /// through derive attributes alone.
    pub fn parse_validated() -> Self {
        Self::parse()
            .validate()
            .unwrap_or_else(|error| error.exit())
    }

    /// Validate command/harness compatibility for programmatic callers.
    pub fn validate(self) -> Result<Self, clap::Error> {
        if self.config.is_some()
            && matches!(
                &self.command,
                Command::PostTool | Command::SessionStartState
            )
        {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::ArgumentConflict,
                "--config is only valid with post-tool-immediate or turn-completion",
            ));
        }
        if self.state_dir.is_some() && matches!(&self.command, Command::PostToolImmediate) {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::ArgumentConflict,
                "--state-dir is not used by post-tool-immediate",
            ));
        }
        if matches!(self.harness, Harness::Antigravity)
            && matches!(&self.command, Command::SessionStartState)
        {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::InvalidValue,
                "session-start-state supports Claude Code and Codex only; Antigravity uses inferred first-observation bootstrap",
            ));
        }
        Ok(self)
    }
}

/// Selected hook commands. Names remain stable when more hooks are added.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Record post-tool file activity for deferred turn-completion checks.
    #[command(name = "post-tool")]
    PostTool,
    /// Run configured checks immediately for files changed by this tool call.
    #[command(name = "post-tool-immediate")]
    PostToolImmediate,
    /// Reconcile recorded activity and run deferred checks at turn completion.
    #[command(name = "turn-completion")]
    TurnCompletion,
    /// Record an exact Claude Code or Codex session-start lower bound.
    #[command(name = "session-start-state")]
    SessionStartState,
}
