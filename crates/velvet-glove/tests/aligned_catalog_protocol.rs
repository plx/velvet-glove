//! Every selected aligned native arm is exercised from its exact fixture.

#[test]
#[rustfmt::skip]
fn claude_code_post_tool_uses_native_arm() {
let variables = hookkit_core::EnvironmentVariables::from_pairs([
        ("CLAUDECODE", "1"),
        ("CLAUDE_CODE_CHILD_SESSION", "1"),
        ("CLAUDE_CODE_SESSION_ID", "s1"),
        ("CLAUDE_PROJECT_DIR", "/repo"),
]);
    let emission = hookkit_runtime::aligned::execute_aligned_event::<
        hookkit_runtime::aligned::PostToolUse,
        _,
    >(
hookkit_core::HarnessId::CLAUDE_CODE,
        include_bytes!(concat!("../fixtures/", "claude-code", "/", "post_tool_use", ".json")),
        &variables,
|input, environment, context| {
            velvet_glove::hooks::aligned::post_tool(
                input,
                environment,
                context,
                None,
            )
        },
    ).expect("aligned representative fixture must execute");

    assert_eq!(emission.exit_code(), 0);
}
#[test]
#[rustfmt::skip]
fn codex_post_tool_uses_native_arm() {
let variables = hookkit_core::EnvironmentVariables::new();
    let emission = hookkit_runtime::aligned::execute_aligned_event::<
        hookkit_runtime::aligned::PostToolUse,
        _,
    >(
hookkit_core::HarnessId::CODEX,
        include_bytes!(concat!("../fixtures/", "codex", "/", "post_tool_use", ".json")),
        &variables,
|input, environment, context| {
            velvet_glove::hooks::aligned::post_tool(
                input,
                environment,
                context,
                None,
            )
        },
    ).expect("aligned representative fixture must execute");

    assert_eq!(emission.exit_code(), 0);
}
#[test]
#[rustfmt::skip]
fn antigravity_post_tool_uses_native_arm() {
let variables = hookkit_core::EnvironmentVariables::new();
    let emission = hookkit_runtime::aligned::execute_aligned_event::<
        hookkit_runtime::aligned::PostToolUse,
        _,
    >(
hookkit_core::HarnessId::ANTIGRAVITY,
        include_bytes!(concat!("../fixtures/", "antigravity", "/", "post_tool_use", ".json")),
        &variables,
|input, environment, context| {
            velvet_glove::hooks::aligned::post_tool(
                input,
                environment,
                context,
                None,
            )
        },
    ).expect("aligned representative fixture must execute");

    assert_eq!(emission.exit_code(), 0);
}
#[test]
#[rustfmt::skip]
fn claude_code_turn_completion_uses_native_arm() {
let variables = hookkit_core::EnvironmentVariables::from_pairs([
        ("CLAUDECODE", "1"),
        ("CLAUDE_CODE_CHILD_SESSION", "1"),
        ("CLAUDE_CODE_SESSION_ID", "s1"),
        ("CLAUDE_PROJECT_DIR", "/repo"),
]);
    let emission = hookkit_runtime::aligned::execute_aligned_event::<
        hookkit_runtime::aligned::TurnCompletion,
        _,
    >(
hookkit_core::HarnessId::CLAUDE_CODE,
        include_bytes!(concat!("../fixtures/", "claude-code", "/", "stop", ".json")),
        &variables,
|input, environment, context| {
            velvet_glove::hooks::aligned::turn_completion(
                input,
                environment,
                context,
                None,
            )
        },
    ).expect("aligned representative fixture must execute");

    assert_eq!(emission.exit_code(), 0);
}
#[test]
#[rustfmt::skip]
fn codex_turn_completion_uses_native_arm() {
let variables = hookkit_core::EnvironmentVariables::new();
    let emission = hookkit_runtime::aligned::execute_aligned_event::<
        hookkit_runtime::aligned::TurnCompletion,
        _,
    >(
hookkit_core::HarnessId::CODEX,
        include_bytes!(concat!("../fixtures/", "codex", "/", "stop", ".json")),
        &variables,
|input, environment, context| {
            velvet_glove::hooks::aligned::turn_completion(
                input,
                environment,
                context,
                None,
            )
        },
    ).expect("aligned representative fixture must execute");

    assert_eq!(emission.exit_code(), 0);
}
#[test]
#[rustfmt::skip]
fn antigravity_turn_completion_uses_native_arm() {
let variables = hookkit_core::EnvironmentVariables::new();
    let emission = hookkit_runtime::aligned::execute_aligned_event::<
        hookkit_runtime::aligned::TurnCompletion,
        _,
    >(
hookkit_core::HarnessId::ANTIGRAVITY,
        include_bytes!(concat!("../fixtures/", "antigravity", "/", "stop", ".json")),
        &variables,
|input, environment, context| {
            velvet_glove::hooks::aligned::turn_completion(
                input,
                environment,
                context,
                None,
            )
        },
    ).expect("aligned representative fixture must execute");

    assert_eq!(emission.exit_code(), 0);
}
