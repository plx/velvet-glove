//! Lowering the immediate runner's outcome to each harness's native output.

use super::diagnostics::runner_artifact_key;
use super::output::{RunnerDomainOutcome, RunnerPostToolUseOutput, auto_fixed_line};
use crate::errors::invalid_data;
use hookkit_common::message::DiagnosticReport;
use hookkit_common::{PostToolUseInput, PostToolUseOutput, UserNotice};
use hookkit_core::{HarnessId, RuntimeContext};
use hookkit_pkl_config::schema as pkl;
use hookkit_runtime::artifacts::{ArtifactKey, ArtifactManager};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug)]
pub(super) struct LoweringWarningArtifact {
    directory: PathBuf,
    key: ArtifactKey,
}

pub(super) fn lowering_warning_artifact(
    input: &PostToolUseInput,
    ctx: &RuntimeContext<'_>,
) -> Option<LoweringWarningArtifact> {
    let PostToolUseInput::Antigravity(input) = input else {
        return None;
    };
    let directory = PathBuf::from(ctx.artifact_directory()?.as_str());
    Some(LoweringWarningArtifact {
        directory,
        key: runner_artifact_key(
            ctx,
            format!("post-tool-use-step-{}-lowering-warning", input.step_idx),
        ),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoweringWarningRecord<'a> {
    format_version: u8,
    kind: &'static str,
    harness: &'static str,
    event: &'static str,
    lowering_policy: &'static str,
    unavailable: LoweringWarningMessages<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoweringWarningMessages<'a> {
    user_notices: &'a [UserNotice],
    diagnostics: Vec<LoweringWarningDiagnostic>,
    agent_feedback: &'a [String],
    rendered_user: &'a [String],
    rendered_agent: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoweringWarningDiagnostic {
    title: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact: Option<LoweringWarningDiagnosticArtifact>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoweringWarningDiagnosticArtifact {
    absolute_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_relative_path: Option<String>,
    media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
}

fn record_antigravity_lowering_warning(
    target: Option<&LoweringWarningArtifact>,
    output: &RunnerPostToolUseOutput,
    agent_feedback: &[String],
    rendered_user: &[String],
    rendered_agent: &str,
) -> hookkit_core::Result<PathBuf> {
    let target = target.ok_or_else(|| {
        invalid_data(
            "cannot record Antigravity PostToolUse lowering loss: exact input has no artifact directory"
                .into(),
        )
    })?;
    let diagnostics = output
        .diagnostics
        .iter()
        .map(|diagnostic| LoweringWarningDiagnostic {
            title: diagnostic.title.clone(),
            text: diagnostic.text.clone(),
            artifact: diagnostic.artifact.as_ref().map(|artifact| {
                LoweringWarningDiagnosticArtifact {
                    absolute_path: artifact.absolute_path.to_string_lossy().into_owned(),
                    project_relative_path: artifact
                        .project_relative_path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    media_type: artifact.media_type.clone(),
                    summary: artifact.summary.clone(),
                }
            }),
        })
        .collect();
    let record = LoweringWarningRecord {
        format_version: 1,
        kind: "post-tool-use-lowering-loss",
        harness: "antigravity",
        event: "PostToolUse",
        lowering_policy: "best-effort-with-warnings",
        unavailable: LoweringWarningMessages {
            user_notices: &output.notices,
            diagnostics,
            agent_feedback,
            rendered_user,
            rendered_agent,
        },
    };
    let value = serde_json::to_value(record)?;
    let manager = ArtifactManager::new(&target.directory).map_err(|error| {
        invalid_data(format!(
            "cannot create Antigravity lowering-warning artifact directory {}: {error}",
            target.directory.display()
        ))
    })?;
    manager
        .write_json_unique(&target.key, &value)
        .map_err(|error| {
            invalid_data(format!(
                "cannot write Antigravity lowering-warning artifact in {}: {error}",
                target.directory.display()
            ))
        })
}

pub(super) fn lower_domain_outcome(
    harness: &HarnessId,
    outcome: RunnerDomainOutcome,
    lowering_warning_artifact: Option<&LoweringWarningArtifact>,
) -> hookkit_core::Result<PostToolUseOutput> {
    match outcome {
        RunnerDomainOutcome::Clean => lower_report(
            harness,
            RunnerPostToolUseOutput::default(),
            lowering_warning_artifact,
        ),
        RunnerDomainOutcome::Report(output) => {
            lower_report(harness, output, lowering_warning_artifact)
        }
        RunnerDomainOutcome::HarnessBlock { message, output } => lower_report(
            harness,
            output.with_harness_block(message),
            lowering_warning_artifact,
        ),
        RunnerDomainOutcome::OperationalFailure { message } => Err(invalid_data(message)),
        RunnerDomainOutcome::UnsupportedHarness { harness, reason } => Err(invalid_data(format!(
            "post-tool-use runner does not support {harness}: {reason}"
        ))),
    }
}

fn lower_report(
    harness: &HarnessId,
    mut output: RunnerPostToolUseOutput,
    lowering_warning_artifact: Option<&LoweringWarningArtifact>,
) -> hookkit_core::Result<PostToolUseOutput> {
    // Agent: the shared auto-fix line (tools on the default template) plus
    // each tool's own feedback. Rendering may add notices, so it comes first.
    let agent_feedback = output.rendered_agent_feedback();
    let context = auto_fixed_line(output.auto_fixed.iter().filter(|entry| entry.in_agent_line))
        .into_iter()
        .chain(agent_feedback.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n");

    if let Some(message) = output.harness_block.take() {
        // A block replaces the normal channels, so it carries what the agent
        // would otherwise have been told: files earlier tools rewrote must be
        // re-read before the next edit.
        let message = [context.as_str(), message.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return match harness.as_str() {
            "claude-code" => Ok(PostToolUseOutput::Claude(
                hookkit_claude::protocol::PostToolUseOutput::feedback_error(message),
            )),
            "codex" => Ok(PostToolUseOutput::Codex(
                hookkit_codex::protocol::PostToolUseOutput::blocking_error(message),
            )),
            _ => Err(invalid_data(format!(
                "post-tool-use runner does not support {harness}"
            ))),
        };
    }

    // User: one terse auto-fix line plus one line per notice, through the
    // native user-only `systemMessage`. Full diagnostics stay in their files.
    let auto_fixed = auto_fixed_line(&output.auto_fixed);
    let user_lines = auto_fixed
        .iter()
        .cloned()
        .chain(output.notices.iter().map(format_notice))
        .collect::<Vec<_>>();
    let user_message = user_lines.join("\n");

    match harness.as_str() {
        "claude-code" => {
            let native = if context.is_empty() {
                hookkit_claude::protocol::PostToolUseOutput::no_op()
            } else {
                hookkit_claude::protocol::PostToolUseOutput::with_context(context)
            };
            Ok(PostToolUseOutput::Claude(if user_message.is_empty() {
                native
            } else {
                native.with_system_message(user_message)?
            }))
        }
        "codex" => {
            let native = if context.is_empty() {
                hookkit_codex::protocol::PostToolUseOutput::no_op()
            } else {
                hookkit_codex::protocol::PostToolUseOutput::with_context(context)
            };
            Ok(PostToolUseOutput::Codex(if user_message.is_empty() {
                native
            } else {
                native.with_system_message(user_message)?
            }))
        }
        "antigravity" => {
            let rendered_user = user_lines
                .into_iter()
                .chain(output.diagnostics.iter().map(format_diagnostic))
                .collect::<Vec<_>>();
            if !context.is_empty() && output.lowering == pkl::LoweringPolicy::Strict {
                return Err(invalid_data(
                    "antigravity PostToolUse has no structured agent-only message channel".into(),
                ));
            }
            let mut native = hookkit_antigravity::PostToolUseOutput::default();
            if output.lowering == pkl::LoweringPolicy::BestEffortWithWarnings
                && (!rendered_user.is_empty() || !context.is_empty())
            {
                let path = record_antigravity_lowering_warning(
                    lowering_warning_artifact,
                    &output,
                    &agent_feedback,
                    &rendered_user,
                    &context,
                )?;
                native = native.with_protocol_stderr(format!(
                    "hookkit: Antigravity PostToolUse could not represent user/agent messages; full lowering record: {}",
                    path.display()
                ))?;
            }
            Ok(PostToolUseOutput::Antigravity(native))
        }
        _ => Err(invalid_data(format!(
            "post-tool-use runner does not support {harness}"
        ))),
    }
}

/// A user notice as one line attributed to velvet-glove, worded like the
/// Stop notices rather than tagged with a severity.
pub(super) fn format_notice(notice: &UserNotice) -> String {
    if notice.text.starts_with("velvet-glove") {
        notice.text.clone()
    } else {
        format!("velvet-glove: {}", notice.text)
    }
}

fn format_diagnostic(diagnostic: &DiagnosticReport) -> String {
    let mut rendered = format!("{}:\n{}", diagnostic.title, diagnostic.text.trim());
    if let Some(artifact) = &diagnostic.artifact {
        rendered.push_str(&format!("\nartifact: {}", artifact.absolute_path.display()));
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immediate::output::AutoFixed;
    use crate::test_support::unique_test_directory;
    use hookkit_common::message::DiagnosticArtifact;
    use hookkit_core::EventSpec as _;

    #[test]
    fn a_harness_block_still_tells_the_agent_what_changed() {
        let output = RunnerPostToolUseOutput::default()
            .with_auto_fixed(AutoFixed {
                tool: "Ruff".into(),
                files: vec!["src/a.py".into()],
                in_agent_line: true,
            })
            .with_agent_feedback("Custom rewrote web/c.ts")
            .with_harness_block("velvet-glove could not run ESLint (eslint not found).");
        let PostToolUseOutput::Claude(native) =
            lower_report(&HarnessId::CLAUDE_CODE, output, None).unwrap()
        else {
            panic!("expected Claude output");
        };
        let emission = hookkit_claude::protocol::PostToolUse::emit(native).unwrap();
        assert_eq!(
            String::from_utf8_lossy(emission.stderr()).trim_end(),
            "velvet-glove auto-fixed src/a.py (Ruff); re-read before editing.\nCustom rewrote web/c.ts\nvelvet-glove could not run ESLint (eslint not found)."
        );
    }

    fn emitted_json(output: PostToolUseOutput) -> (serde_json::Value, Vec<u8>) {
        let emission = match output {
            PostToolUseOutput::Claude(native) => {
                hookkit_claude::protocol::PostToolUse::emit(native).unwrap()
            }
            PostToolUseOutput::Codex(native) => {
                hookkit_codex::protocol::PostToolUse::emit(native).unwrap()
            }
            _ => panic!("expected Claude or Codex output"),
        };
        assert_eq!(emission.exit_code(), 0);
        (
            serde_json::from_slice(emission.stdout()).unwrap(),
            emission.stderr().to_vec(),
        )
    }

    #[test]
    fn immediate_user_notices_use_system_message_and_auto_fixes_share_one_line() {
        let fixed = |tool: &str, files: &[&str], in_agent_line| AutoFixed {
            tool: tool.into(),
            files: files.iter().map(|file| file.to_string()).collect(),
            in_agent_line,
        };
        for harness in [HarnessId::CLAUDE_CODE, HarnessId::CODEX] {
            let output = RunnerPostToolUseOutput::default()
                .with_auto_fixed(fixed("Ruff", &["src/a.py", "src/b.py"], true))
                .with_auto_fixed(fixed("Black", &["src/a.py"], true))
                .with_auto_fixed(fixed("Custom", &["web/c.ts"], false))
                .with_agent_feedback("Custom rewrote web/c.ts")
                .with_user_notice(UserNotice::warning("Lint: `lint` is unavailable"))
                .with_diagnostic_report(DiagnosticReport::new("Lint diagnostics", "full log"));
            let (json, stderr) = emitted_json(lower_report(&harness, output, None).unwrap());

            assert!(
                stderr.is_empty(),
                "{harness}: user notices must not use stderr"
            );
            assert_eq!(
                json["systemMessage"],
                "velvet-glove auto-fixed src/a.py (Ruff, Black), src/b.py (Ruff), web/c.ts (Custom); re-read before editing.\nvelvet-glove: Lint: `lint` is unavailable",
                "{harness}"
            );
            assert_eq!(
                json["hookSpecificOutput"]["additionalContext"],
                "velvet-glove auto-fixed src/a.py (Ruff, Black), src/b.py (Ruff); re-read before editing.\nCustom rewrote web/c.ts",
                "{harness}"
            );

            let (json, stderr) = emitted_json(
                lower_report(&harness, RunnerPostToolUseOutput::default(), None).unwrap(),
            );
            assert_eq!(json, serde_json::json!({}), "{harness}: clean is silent");
            assert!(stderr.is_empty());
        }
    }

    #[test]
    fn domain_outcomes_keep_clean_failure_and_unsupported_distinct() {
        assert!(matches!(
            lower_domain_outcome(&HarnessId::CLAUDE_CODE, RunnerDomainOutcome::Clean, None)
                .unwrap(),
            PostToolUseOutput::Claude(_)
        ));
        assert!(
            lower_domain_outcome(
                &HarnessId::CLAUDE_CODE,
                RunnerDomainOutcome::OperationalFailure {
                    message: "checker crashed".into(),
                },
                None,
            )
            .is_err()
        );
        assert!(matches!(
            lower_domain_outcome(&HarnessId::ANTIGRAVITY, RunnerDomainOutcome::Clean, None)
                .unwrap(),
            PostToolUseOutput::Antigravity(_)
        ));
        assert!(
            lower_domain_outcome(
                &HarnessId::ANTIGRAVITY,
                RunnerDomainOutcome::UnsupportedHarness {
                    harness: "antigravity".into(),
                    reason: "no changed-file data".into(),
                },
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn antigravity_warning_lowering_records_full_loss_and_preserves_exact_stdout() {
        let directory = unique_test_directory("antigravity-lowering-warning");
        let target = LoweringWarningArtifact {
            directory: directory.clone(),
            key: ArtifactKey::new("conversation-7", "post-tool-use-step-3-lowering-warning"),
        };
        let diagnostic_artifact =
            DiagnosticArtifact::new(directory.join("complete-diagnostic.txt"), "text/plain")
                .with_project_relative_path(".velvet-glove/complete-diagnostic.txt")
                .with_summary("complete tool output");
        let warning = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::BestEffortWithWarnings)
            .with_user_notice(UserNotice::warning("review every diagnostic line"))
            .with_diagnostic_report(
                DiagnosticReport::new("lint report", "first line\nsecond line")
                    .with_artifact(diagnostic_artifact),
            )
            .with_agent_feedback("re-read generated.rs\nthen repair it");

        let native = match lower_report(&HarnessId::ANTIGRAVITY, warning, Some(&target)).unwrap() {
            PostToolUseOutput::Antigravity(native) => native,
            _ => panic!("expected Antigravity output"),
        };
        let emission = hookkit_antigravity::PostToolUse::emit(native).unwrap();
        let artifact_path = directory.join(format!("{}.json", target.key.filename()));

        assert_eq!(emission.stdout(), b"{}");
        assert_eq!(emission.exit_code(), 0);
        assert!(
            String::from_utf8_lossy(emission.stderr())
                .contains(&artifact_path.to_string_lossy().into_owned())
        );
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&artifact_path).unwrap()).unwrap();
        assert_eq!(record["formatVersion"], 1);
        assert_eq!(record["kind"], "post-tool-use-lowering-loss");
        assert_eq!(record["harness"], "antigravity");
        assert_eq!(record["event"], "PostToolUse");
        assert_eq!(record["loweringPolicy"], "best-effort-with-warnings");
        assert_eq!(
            record["unavailable"]["userNotices"][0]["text"],
            "review every diagnostic line"
        );
        assert_eq!(
            record["unavailable"]["diagnostics"][0]["text"],
            "first line\nsecond line"
        );
        assert_eq!(
            record["unavailable"]["diagnostics"][0]["artifact"]["summary"],
            "complete tool output"
        );
        assert_eq!(
            record["unavailable"]["agentFeedback"][0],
            "re-read generated.rs\nthen repair it"
        );
        assert_eq!(
            record["unavailable"]["renderedAgent"],
            "re-read generated.rs\nthen repair it"
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn antigravity_best_effort_omits_unavailable_messages_without_a_record() {
        let directory = unique_test_directory("antigravity-lowering-best-effort");
        let target = LoweringWarningArtifact {
            directory: directory.clone(),
            key: ArtifactKey::new("conversation-8", "post-tool-use-step-4-lowering-warning"),
        };
        let best_effort = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::BestEffort)
            .with_user_notice(UserNotice::warning("review diagnostics"))
            .with_agent_feedback("re-read generated.rs");
        let native =
            match lower_report(&HarnessId::ANTIGRAVITY, best_effort, Some(&target)).unwrap() {
                PostToolUseOutput::Antigravity(native) => native,
                _ => panic!("expected Antigravity output"),
            };
        let emission = hookkit_antigravity::PostToolUse::emit(native).unwrap();

        assert_eq!(emission.stdout(), b"{}");
        assert!(emission.stderr().is_empty());
        assert!(
            !directory
                .join(format!("{}.json", target.key.filename()))
                .exists()
        );
    }

    #[test]
    fn antigravity_warning_lowering_never_overwrites_a_reused_step_key() {
        let directory = unique_test_directory("antigravity-lowering-collision");
        let target = LoweringWarningArtifact {
            directory: directory.clone(),
            key: ArtifactKey::new("conversation-8", "post-tool-use-step-0-lowering-warning"),
        };
        let lower = |feedback: &str| {
            let output = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::BestEffortWithWarnings)
                .with_agent_feedback(feedback);
            let native = match lower_report(&HarnessId::ANTIGRAVITY, output, Some(&target)).unwrap()
            {
                PostToolUseOutput::Antigravity(native) => native,
                _ => panic!("expected Antigravity output"),
            };
            let emission = hookkit_antigravity::PostToolUse::emit(native).unwrap();
            let stderr = String::from_utf8(emission.stderr().to_vec()).unwrap();
            PathBuf::from(stderr.rsplit_once(": ").unwrap().1)
        };

        let first = lower("first invocation");
        let second = lower("second invocation");

        assert_ne!(first, second);
        let first_record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(first).unwrap()).unwrap();
        let second_record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(second).unwrap()).unwrap();
        assert_eq!(
            first_record["unavailable"]["agentFeedback"][0],
            "first invocation"
        );
        assert_eq!(
            second_record["unavailable"]["agentFeedback"][0],
            "second invocation"
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn antigravity_strict_lowering_errors_instead_of_recording_loss() {
        let strict = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::Strict)
            .with_agent_feedback("re-read generated.rs");
        assert!(lower_report(&HarnessId::ANTIGRAVITY, strict, None).is_err());
    }

    #[test]
    fn antigravity_warning_lowering_errors_when_the_record_cannot_be_written() {
        let directory = unique_test_directory("antigravity-lowering-write-failure");
        std::fs::create_dir_all(&directory).unwrap();
        let not_a_directory = directory.join("regular-file");
        std::fs::write(&not_a_directory, "occupied").unwrap();
        let target = LoweringWarningArtifact {
            directory: not_a_directory,
            key: ArtifactKey::new("conversation-9", "post-tool-use-step-5-lowering-warning"),
        };
        let warning = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::BestEffortWithWarnings)
            .with_agent_feedback("this must be retained");

        assert!(lower_report(&HarnessId::ANTIGRAVITY, warning, Some(&target)).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
