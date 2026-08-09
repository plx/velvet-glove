//! Antigravity's refreshed PostToolUse payload retains typed originating args.

mod support;

use support::native_events::{PostToolUseBuilder, ProtocolSurface};

#[test]
fn antigravity_post_tool_produces_direct_file_activity() {
    let input = PostToolUseBuilder::new(ProtocolSurface::Antigravity, "/repo", "src/generated.rs")
        .identity("conversation-1", "turn-1", "tool-1")
        .build()
        .expect("canonical Antigravity PostToolUse fixture");
    let variables = input.environment_variables();

    let emission = hookkit_runtime::aligned::execute_aligned_event::<
        hookkit_runtime::aligned::PostToolUse,
        _,
    >(
        hookkit_core::HarnessId::ANTIGRAVITY,
        input.bytes(),
        &variables,
        |input, _environment, context| {
            let report = hookkit_file_activity::observe_post_tool(&input, context);
            assert!(report.evidence().any(|evidence| {
                evidence.source == hookkit_file_activity::FileActivitySource::ShellInference
                    && evidence.certainty == hookkit_file_activity::ActivityCertainty::Direct
                    && evidence.target
                        == hookkit_file_activity::FileActivityTarget::exact(
                            hookkit_core::Utf8PathBuf::from("/repo/src/generated.rs"),
                        )
            }));
            hookkit_common::PostToolUseOutput::no_op(context.harness())
        },
    )
    .expect("Antigravity PostToolUse should retain its originating shell call");

    assert_eq!(emission.exit_code(), 0);
    assert_eq!(emission.stdout(), b"{}");
}
