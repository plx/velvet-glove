use super::cli::{Cli, Command};

/// Execute one explicit hook command.
#[rustfmt::skip]
pub fn run(cli: Cli) -> std::process::ExitCode {
    let harness = cli.harness.id();
    let config_path = cli
        .config
        .unwrap_or_else(crate::scaffold::runners::default_config_path);
    let state_dir = cli
        .state_dir
        .unwrap_or_else(crate::scaffold::runners::default_state_dir);
    match cli.command {
        Command::PostTool => {
            crate::scaffold::runners::run_file_activity(harness, state_dir)
        }
        Command::TurnCompletion => crate::scaffold::runners::run_turn_completion(
            harness,
            config_path,
            state_dir,
        ),
        Command::SessionStartState => {
            if harness == hookkit_core::HarnessId::ANTIGRAVITY {
                return std::process::ExitCode::from(2);
            }
            crate::scaffold::runners::run_session_start(harness, state_dir)
        }
    }
}
