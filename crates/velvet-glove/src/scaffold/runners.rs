//! Small adapters from this package's CLI to HookKit's library-owned runners.

use hookkit_core::HarnessId;
use std::path::PathBuf;
use std::process::ExitCode;

/// Package-specific state root shared by the lifecycle producer and consumer.
pub fn default_state_dir() -> PathBuf {
    std::env::temp_dir().join("velvet-glove").join("state")
}

/// Record precise session metadata at Claude/Codex SessionStart.
///
/// Antigravity intentionally does not call this entrypoint: its lower bound is
/// established best-effort by the first PostToolUse observation instead.
pub fn run_session_start(harness: HarnessId, state_dir: PathBuf) -> ExitCode {
    hookkit_tool_runner::run_session_start_observer(hookkit_tool_runner::SessionStartCli {
        harness,
        state_dir: Some(state_dir),
    })
}

/// Append PostToolUse file-activity evidence without emitting hook messages.
pub fn run_file_activity(harness: HarnessId, state_dir: PathBuf) -> ExitCode {
    hookkit_tool_runner::run_file_activity_observer(hookkit_tool_runner::FileActivityCli {
        harness,
        state_dir: Some(state_dir),
    })
}

/// Run configured quality workflows immediately for one PostToolUse event.
///
/// A missing `config_path` deliberately selects Velvet Glove's layered
/// discovery rooted at the workspace reported by the native hook event.
pub fn run_immediate(harness: HarnessId, config_path: Option<PathBuf>) -> ExitCode {
    hookkit_tool_runner::run_runner(hookkit_tool_runner::Cli {
        harness,
        config_path,
    })
}

/// Reconcile accumulated file activity and run the deferred quality workflows.
pub fn run_turn_completion(
    harness: HarnessId,
    config_path: Option<PathBuf>,
    state_dir: PathBuf,
) -> ExitCode {
    hookkit_tool_runner::run_turn_completion_runner(hookkit_tool_runner::TurnCompletionCli {
        harness,
        config_path,
        state_dir: Some(state_dir),
    })
}
