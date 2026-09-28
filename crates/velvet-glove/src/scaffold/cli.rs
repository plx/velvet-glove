use super::harness::Harness;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
/// Velvet Glove command-line interface.
#[derive(Debug, Parser)]
#[rustfmt::skip]
#[command(
    name = "velvet-glove",
    about = "Formatter and linter hooks for coding agents",
    after_help = "Setup: run `velvet-glove init` in a project, then `velvet-glove doctor` and `velvet-glove check`."
)]
pub struct Cli {
    /// Coding-agent harness that emitted this native hook event. Required
    /// for hook commands; not accepted by setup commands.
    #[arg(long, value_enum)]
    pub harness: Option<Harness>,

    /// Explicit Pkl policy. When omitted, Velvet Glove uses layered
    /// user/project/local discovery rooted at the hook event's workspace
    /// (or, for setup commands, at --dir).
    #[arg(long, value_name = "PATH", global = true)]
    pub config: Option<PathBuf>,


    /// Shared Velvet Glove state root override.
    #[arg(long, value_name = "PATH")]
    pub state_dir: Option<PathBuf>,

    /// Explicit hook event or setup command to execute.
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
        let conflict = |message: &str| {
            Err(clap::Error::raw(
                clap::error::ErrorKind::ArgumentConflict,
                message,
            ))
        };
        if !self.command.is_hook() {
            if self.harness.is_some() {
                return conflict("--harness is only valid with hook commands");
            }
            let is_doctor = matches!(&self.command, Command::Doctor(_));
            let accepts_config = is_doctor || matches!(&self.command, Command::Check(_));
            if !accepts_config && self.config.is_some() {
                return conflict("--config is only valid with hook commands, doctor, and check");
            }
            if !is_doctor && self.state_dir.is_some() {
                return conflict("--state-dir is only valid with hook commands and doctor");
            }
            return Ok(self);
        }
        if self.harness.is_none() {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::MissingRequiredArgument,
                "hook commands require --harness <claude|codex|antigravity>",
            ));
        }
        if self.config.is_some()
            && matches!(
                &self.command,
                Command::PostTool | Command::SessionStartState
            )
        {
            return conflict("--config is only valid with post-tool-immediate or turn-completion");
        }
        if self.state_dir.is_some() && matches!(&self.command, Command::PostToolImmediate) {
            return conflict("--state-dir is not used by post-tool-immediate");
        }
        if matches!(self.harness, Some(Harness::Antigravity))
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

/// Hook commands and setup commands. Names remain stable when more are added.
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
    /// List the builtin tool catalog and whether each executable resolves.
    Tools(ToolsArgs),
    /// Explain the configuration, run list, and tool setup for a directory.
    Doctor(DirArgs),
    /// Detect fitting builtin tools and write .velvet-glove/post-tool-use.pkl.
    Init(InitArgs),
    /// Run the Stop-time checks and fixes on files now, outside any hook.
    ///
    /// Exits 0 when everything is clean or was auto-fixed, 1 when manual
    /// fixes remain, and 2 when a tool could not run or the policy is broken.
    Check(CheckArgs),
}

impl Command {
    /// Whether this command handles a native hook event.
    pub fn is_hook(&self) -> bool {
        matches!(
            self,
            Self::PostTool
                | Self::PostToolImmediate
                | Self::TurnCompletion
                | Self::SessionStartState
        )
    }
}

/// Arguments shared by setup commands that inspect a directory.
#[derive(Debug, Args)]
pub struct DirArgs {
    /// Project directory to inspect (defaults to the current directory).
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
}

/// Arguments for `tools`.
#[derive(Debug, Args)]
pub struct ToolsArgs {
    #[command(flatten)]
    pub dir: DirArgs,
    /// Print machine-readable JSON instead of a table.
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    #[command(flatten)]
    pub dir: DirArgs,
    /// Print the generated policy to stdout instead of writing it.
    #[arg(long)]
    pub print: bool,
    /// Overwrite an existing .velvet-glove/post-tool-use.pkl.
    #[arg(long)]
    pub force: bool,
}

/// Arguments for `check`.
#[derive(Debug, Args)]
pub struct CheckArgs {
    #[command(flatten)]
    pub dir: DirArgs,
    /// Print machine-readable JSON instead of a summary.
    #[arg(long)]
    pub json: bool,
    /// Files or directories to check, relative to DIR. Default: the Git
    /// work tree's modified and untracked files under DIR.
    #[arg(value_name = "FILES")]
    pub files: Vec<PathBuf>,
}
