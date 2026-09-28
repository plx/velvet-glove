use super::cli::{Cli, Command, DirArgs};
use crate::scaffold::runners;
use std::process::ExitCode;

/// Execute one explicit hook or setup command.
pub fn run(cli: Cli) -> ExitCode {
    let cli = match cli.validate() {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    let state_dir = cli.state_dir.unwrap_or_else(runners::default_state_dir);
    let harness = cli.harness.map(|harness| harness.id());
    match (cli.command, harness) {
        (Command::Tools(args), _) => {
            crate::commands::tools::run(&resolve_dir(&args.dir), args.json)
        }
        (Command::Doctor(args), _) => {
            crate::commands::doctor::run(&resolve_dir(&args), cli.config.as_deref(), &state_dir)
        }
        (Command::Init(args), _) => {
            crate::commands::init::run(&resolve_dir(&args.dir), args.print, args.force)
        }
        (Command::Check(args), _) => crate::commands::check::run(
            &resolve_dir(&args.dir),
            cli.config.as_deref(),
            &args.files,
            args.json,
        ),
        (Command::PostTool, Some(harness)) => runners::run_file_activity(harness, state_dir),
        (Command::PostToolImmediate, Some(harness)) => runners::run_immediate(harness, cli.config),
        (Command::TurnCompletion, Some(harness)) => {
            runners::run_turn_completion(harness, cli.config, state_dir)
        }
        // `Cli::validate` rejects Antigravity here; it has no exact SessionStart.
        (Command::SessionStartState, Some(harness)) => {
            runners::run_session_start(harness, state_dir)
        }
        // `Cli::validate` requires --harness for every hook command.
        (_, None) => ExitCode::from(64),
    }
}

/// The requested directory (default: the current one), made absolute.
fn resolve_dir(args: &DirArgs) -> std::path::PathBuf {
    let dir = args.dir.clone().unwrap_or_else(|| ".".into());
    std::path::absolute(&dir)
        .and_then(|absolute| absolute.canonicalize().or(Ok(absolute)))
        .unwrap_or(dir)
}
