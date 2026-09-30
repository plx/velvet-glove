//! Hook entry points and the CLI options that select them.

use crate::deferred::run_turn_completion_input;
use crate::errors::{activity_error, invalid_data, state_error};
use crate::immediate::run_post_tool_input;
use crate::paths::state_root;
use hookkit_common::PostToolUseOutput;
use hookkit_core::{HarnessId, RuntimeContext};
use hookkit_file_activity::FileActivityStore;
use hookkit_file_activity::observe_post_tool as observe_file_activity;
use hookkit_session_state::SessionState;
use std::path::{Path, PathBuf};

/// Execution options supplied by the Velvet Glove CLI.
#[derive(Debug, Clone)]
pub struct Cli {
    /// Harness whose native post-tool event is read from standard input.
    pub harness: HarnessId,
    /// Explicit Pkl file, or `None` to use layered discovery.
    pub config_path: Option<PathBuf>,
}

/// Execution options for the stop-time batch runner.
#[derive(Debug, Clone)]
pub struct TurnCompletionCli {
    /// Harness whose native turn-completion event is read from standard input.
    pub harness: HarnessId,
    /// Explicit Pkl file, or `None` to use layered discovery.
    pub config_path: Option<PathBuf>,
    /// Session-state directory override.
    pub state_dir: Option<PathBuf>,
}

/// Execution options for the library-owned precise session-start observer.
#[derive(Debug, Clone)]
pub struct SessionStartCli {
    /// Harness whose native session-start event is read from standard input.
    pub harness: HarnessId,
    /// Session-state directory override.
    pub state_dir: Option<PathBuf>,
}

/// Execution options for the quiet post-tool file-activity observer.
#[derive(Debug, Clone)]
pub struct FileActivityCli {
    /// Harness whose native post-tool event is read from standard input.
    pub harness: HarnessId,
    /// Session-state directory override.
    pub state_dir: Option<PathBuf>,
}

/// Run the full post-tool-use hook from parsed CLI args.
pub fn run_runner(cli: Cli) -> std::process::ExitCode {
    hookkit_runtime::aligned::run_aligned_event::<hookkit_runtime::aligned::PostToolUse, _>(
        cli.harness,
        move |input, environment, ctx| {
            run_post_tool_input(input, environment, ctx, cli.config_path.as_deref())
        },
    )
}

/// Run the bundled quiet PostToolUse file-activity observer.
pub fn run_file_activity_observer(cli: FileActivityCli) -> std::process::ExitCode {
    hookkit_runtime::aligned::run_aligned_event::<hookkit_runtime::aligned::PostToolUse, _>(
        cli.harness,
        move |input, _environment, ctx| {
            let report = observe_file_activity(&input, ctx);
            let state_root = state_root(cli.state_dir.as_deref());
            let store = FileActivityStore::ensure(ctx, state_root).map_err(activity_error)?;
            let invocation = String::from_utf8_lossy(ctx.raw().bytes());
            store
                .append_report(&invocation, &report)
                .map_err(activity_error)?;
            post_tool_no_op(ctx.harness())
        },
    )
}

fn post_tool_no_op(harness: &HarnessId) -> hookkit_core::Result<PostToolUseOutput> {
    match harness.as_str() {
        "claude-code" => Ok(PostToolUseOutput::Claude(
            hookkit_claude::protocol::PostToolUseOutput::no_op(),
        )),
        "codex" => Ok(PostToolUseOutput::Codex(
            hookkit_codex::protocol::PostToolUseOutput::no_op(),
        )),
        "antigravity" => Ok(PostToolUseOutput::Antigravity(
            hookkit_antigravity::PostToolUseOutput::default(),
        )),
        _ => Err(invalid_data(format!(
            "file-activity observer does not support {harness}"
        ))),
    }
}

/// Run the stop-time batch hook from parsed CLI args.
pub fn run_turn_completion_runner(cli: TurnCompletionCli) -> std::process::ExitCode {
    hookkit_runtime::aligned::run_aligned_event::<hookkit_runtime::aligned::TurnCompletion, _>(
        cli.harness,
        move |input, environment, ctx| {
            run_turn_completion_input(
                input,
                environment,
                ctx,
                cli.config_path.as_deref(),
                cli.state_dir.as_deref(),
            )
        },
    )
}

/// Run the small, library-owned lifecycle observer used when precise native
/// session-start timing is desired even before another stateful hook runs.
pub fn run_session_start_observer(cli: SessionStartCli) -> std::process::ExitCode {
    let state_dir = cli.state_dir;
    match cli.harness.as_str() {
        "claude-code" => hookkit_runtime::typed::run_typed::<
            hookkit_claude::protocol::SessionStart,
            _,
        >(move |_, _, ctx| {
            ensure_session_metadata(ctx, state_dir.as_deref())?;
            Ok(hookkit_claude::protocol::SessionStartOutput::no_op())
        }),
        "codex" => hookkit_runtime::typed::run_typed::<hookkit_codex::catalog::SessionStart, _>(
            move |_, _, ctx| {
                ensure_session_metadata(ctx, state_dir.as_deref())?;
                Ok(hookkit_codex::catalog::SessionStartOutput::no_op())
            },
        ),
        _ => std::process::ExitCode::from(1),
    }
}

fn ensure_session_metadata(
    ctx: &RuntimeContext<'_>,
    state_dir: Option<&Path>,
) -> hookkit_core::Result<()> {
    let root = state_root(state_dir);
    SessionState::ensure(ctx, root).map_err(state_error)?;
    Ok(())
}
