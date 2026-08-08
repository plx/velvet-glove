use clap::{CommandFactory, Parser as _};
#[test]
fn command_tree_is_internally_consistent() {
    velvet_glove::scaffold::cli::Cli::command().debug_assert();
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
