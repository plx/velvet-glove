//! Every selected aligned native arm is exercised from a canonical native input.

mod support;

use support::native_events::{PostToolUseBuilder, ProtocolSurface};

fn assert_post_tool_uses_native_arm(surface: ProtocolSurface) {
    let input = PostToolUseBuilder::new(surface, "/repo", "src/example.rs")
        .build()
        .expect("canonical PostToolUse fixture");
    let variables = input.environment_variables();
    let emission = hookkit_runtime::aligned::execute_aligned_event::<
        hookkit_runtime::aligned::PostToolUse,
        _,
    >(
        surface.harness_id(),
        input.bytes(),
        &variables,
        |input, environment, context| {
            velvet_glove::hooks::aligned::post_tool(input, environment, context, None)
        },
    )
    .expect("aligned representative fixture must execute");

    assert_eq!(emission.exit_code(), 0);
}

fn assert_turn_completion_uses_native_arm(
    surface: ProtocolSurface,
    fixture: &[u8],
    variables: hookkit_core::EnvironmentVariables,
) {
    let emission = hookkit_runtime::aligned::execute_aligned_event::<
        hookkit_runtime::aligned::TurnCompletion,
        _,
    >(
        surface.harness_id(),
        fixture,
        &variables,
        |input, environment, context| {
            velvet_glove::hooks::aligned::turn_completion(input, environment, context, None)
        },
    )
    .expect("aligned representative fixture must execute");

    assert_eq!(emission.exit_code(), 0);
}

#[test]
fn claude_code_post_tool_uses_native_arm() {
    assert_post_tool_uses_native_arm(ProtocolSurface::Claude);
}

#[test]
fn codex_post_tool_uses_native_arm() {
    assert_post_tool_uses_native_arm(ProtocolSurface::Codex);
}

#[test]
fn antigravity_post_tool_uses_native_arm() {
    assert_post_tool_uses_native_arm(ProtocolSurface::Antigravity);
}

#[test]
fn claude_code_turn_completion_uses_native_arm() {
    assert_turn_completion_uses_native_arm(
        ProtocolSurface::Claude,
        include_bytes!("../fixtures/claude-code/stop.json"),
        hookkit_core::EnvironmentVariables::from_pairs([
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_CHILD_SESSION", "1"),
            ("CLAUDE_CODE_SESSION_ID", "s1"),
            ("CLAUDE_PROJECT_DIR", "/repo"),
        ]),
    );
}

#[test]
fn codex_turn_completion_uses_native_arm() {
    assert_turn_completion_uses_native_arm(
        ProtocolSurface::Codex,
        include_bytes!("../fixtures/codex/stop.json"),
        hookkit_core::EnvironmentVariables::new(),
    );
}

#[test]
fn antigravity_turn_completion_uses_native_arm() {
    assert_turn_completion_uses_native_arm(
        ProtocolSurface::Antigravity,
        include_bytes!("../fixtures/antigravity/stop.json"),
        hookkit_core::EnvironmentVariables::new(),
    );
}
