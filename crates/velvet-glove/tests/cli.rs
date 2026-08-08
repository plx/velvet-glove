use clap::{CommandFactory, Parser as _};

#[test]
fn command_tree_is_internally_consistent() {
    velvet_glove::scaffold::cli::Cli::command().debug_assert();
}

#[test]
fn every_public_runner_command_parses_through_the_unified_binary() {
    for command in [
        "post-tool",
        "post-tool-immediate",
        "turn-completion",
        "session-start-state",
    ] {
        velvet_glove::scaffold::cli::Cli::try_parse_from([
            "velvet-glove",
            "--harness",
            "claude",
            command,
        ])
        .unwrap_or_else(|error| panic!("{command} must parse: {error}"));
    }
}

#[test]
fn immediate_and_completion_policy_paths_are_optional_for_discovery() {
    for command in ["post-tool-immediate", "turn-completion"] {
        let cli = velvet_glove::scaffold::cli::Cli::try_parse_from([
            "velvet-glove",
            "--harness",
            "codex",
            command,
        ])
        .unwrap_or_else(|error| panic!("{command} must support discovery: {error}"));
        assert!(cli.config.is_none());
    }
}

#[test]
fn command_specific_global_options_cannot_be_silently_ignored() {
    let unused_config = velvet_glove::scaffold::cli::Cli::try_parse_from([
        "velvet-glove",
        "--harness",
        "claude",
        "--config",
        "policy.pkl",
        "post-tool",
    ])
    .expect("arguments are syntactically valid");
    assert!(unused_config.validate().is_err());

    let unused_state = velvet_glove::scaffold::cli::Cli::try_parse_from([
        "velvet-glove",
        "--harness",
        "codex",
        "--state-dir",
        "/tmp/velvet-glove-test",
        "post-tool-immediate",
    ])
    .expect("arguments are syntactically valid");
    assert!(unused_state.validate().is_err());
}

#[test]
fn antigravity_cannot_select_exact_session_start_support() {
    let cli = velvet_glove::scaffold::cli::Cli::try_parse_from([
        "velvet-glove",
        "--harness",
        "antigravity",
        "--config",
        "config/generated.pkl",
        "--state-dir",
        "/tmp/generated-state",
        "session-start-state",
    ])
    .expect("the individual arguments are syntactically valid");

    assert!(cli.validate().is_err());
}
