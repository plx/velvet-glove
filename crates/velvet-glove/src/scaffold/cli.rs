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
    /// Harness whose native contract should parse and emit this invocation.
    #[arg(long, value_enum)]
    pub harness: Harness,

    /// Package-owned Pkl policy passed explicitly to the HookKit runner.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,


    /// Shared package-specific HookKit state root override.
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
    /// Execute the post tool aligned family.
    #[command(name = "post-tool")]
    PostTool,
    /// Execute the turn completion aligned family.
    #[command(name = "turn-completion")]
    TurnCompletion,
    /// Record an exact Claude Code or Codex session-start lower bound.
    #[command(name = "session-start-state")]
    SessionStartState,
}
