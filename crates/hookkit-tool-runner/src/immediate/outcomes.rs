//! Folding one tool's job outcomes into the immediate runner's output.

use super::diagnostics::{report_with_artifact, write_diagnostics};
use super::messages::{MessageArgs, render_template, template_failure_notice};
use super::output::{AgentFeedback, AutoFixed, PendingIssues, RunnerPostToolUseOutput};
use crate::immediate::ToolBatchStatus;
use crate::jobs::{ChangeState, IssueState, ToolContext, ToolRunOutcome};
use crate::paths::rel_display;
use hookkit_common::UserNotice;
use hookkit_core::RuntimeContext;
use hookkit_pkl_config::schema as pkl;
use std::collections::BTreeSet;
use std::path::Path;

pub(super) fn accumulate_outcomes(
    outcomes: Vec<ToolRunOutcome>,
    context: &ToolContext<'_>,
    ctx: &RuntimeContext<'_>,
    missing_tool_policy: pkl::MissingToolPolicy,
    output: &mut RunnerPostToolUseOutput,
    had_hard_failure: &mut Option<String>,
    had_harness_block_message: &mut Option<String>,
) -> hookkit_core::Result<ToolBatchStatus> {
    let mut changed_files = BTreeSet::new();
    let mut issue_files = BTreeSet::new();
    let mut out_of_scope_files = BTreeSet::new();
    let mut issue_diagnostics = Vec::new();
    let mut issue_outputs = Vec::new();
    let mut failure_diagnostics = Vec::new();
    let mut unavailable = Vec::new();

    for outcome in outcomes {
        match outcome {
            ToolRunOutcome::Completed(completed) => {
                if let ChangeState::Changed { files } = completed.changes {
                    changed_files.extend(files);
                }
                if completed.issues == IssueState::Issues {
                    if completed.files.is_empty() {
                        // The output names only files this call did not
                        // change: not the agent's problem right now.
                        out_of_scope_files.extend(completed.out_of_scope);
                    } else {
                        issue_files.extend(completed.files);
                        issue_diagnostics.push(completed.diagnostics);
                        if !issue_outputs.contains(&completed.issue_output) {
                            issue_outputs.push(completed.issue_output);
                        }
                    }
                }
            }
            ToolRunOutcome::ToolUnavailable {
                phase,
                executable,
                install_hint,
                changed_files: files,
            } => {
                changed_files.extend(files);
                unavailable.push((phase, executable, install_hint));
            }
            ToolRunOutcome::ToolFailed {
                phase,
                exit_code,
                error,
                diagnostics,
                changed_files: files,
            } => {
                changed_files.extend(files);
                failure_diagnostics.push((phase, exit_code, error, diagnostics));
            }
        }
    }

    let mut status = ToolBatchStatus {
        operational_failure: !unavailable.is_empty() || !failure_diagnostics.is_empty(),
        issues: !issue_diagnostics.is_empty(),
    };
    let tool = context.template_tool();

    if !unavailable.is_empty() {
        match missing_tool_policy {
            pkl::MissingToolPolicy::UserNotice => {
                for (phase, executable, install_hint) in &unavailable {
                    let message = render_unavailable_message(
                        context,
                        phase,
                        executable,
                        install_hint.as_deref(),
                        &mut output.notices,
                    );
                    output.notices.push(UserNotice::warning(message));
                }
            }
            pkl::MissingToolPolicy::HardFailure => {
                if let Some((phase, executable, install_hint)) = unavailable.first() {
                    *had_hard_failure = Some(render_unavailable_message(
                        context,
                        phase,
                        executable,
                        install_hint.as_deref(),
                        &mut output.notices,
                    ));
                }
                return Ok(status);
            }
            pkl::MissingToolPolicy::HarnessBlock => {
                if let Some((phase, executable, install_hint)) = unavailable.first() {
                    let message = render_unavailable_message(
                        context,
                        phase,
                        executable,
                        install_hint.as_deref(),
                        &mut output.notices,
                    );
                    *had_harness_block_message = Some(message);
                }
                return Ok(status);
            }
        }
    }

    if !failure_diagnostics.is_empty() {
        let diagnostics = failure_diagnostics
            .iter()
            .map(|(phase, exit_code, _, diagnostics)| {
                format!(
                    "== phase {phase} failed (exit {exit_code:?}) ==\n{}",
                    diagnostics.trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let artifact = write_diagnostics(
            "tool-failure",
            &diagnostics,
            context,
            ctx,
            &mut output.notices,
        )?;
        let (phase, exit_code, error, _) = &failure_diagnostics[0];
        let message = render_failed_message(
            context,
            &artifact,
            (phase, *exit_code, error.as_deref()),
            &mut output.notices,
        );
        output.notices.push(UserNotice::error(message));
        output.diagnostics.push(report_with_artifact(
            format!("{} failure diagnostics", context.spec.display_name),
            diagnostics,
            artifact,
            context.project_root,
        ));
    }

    let changed_paths = changed_files
        .iter()
        .map(|path| rel_display(path, context.project_root))
        .collect::<Vec<_>>();
    let issue_paths = issue_files
        .iter()
        .map(|path| rel_display(path, context.project_root))
        .collect::<Vec<_>>();

    if !issue_diagnostics.is_empty() {
        let diagnostics = issue_diagnostics
            .iter()
            .map(|diagnostics| diagnostics.trim())
            .filter(|diagnostics| !diagnostics.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        let artifact = write_diagnostics(
            "tool-issues",
            &diagnostics,
            context,
            ctx,
            &mut output.notices,
        )?;
        output.notices.push(UserNotice::warning(format!(
            "{}: issues remain in {}; diagnostics: {}",
            context.spec.display_name,
            issue_paths.join(", "),
            artifact.display()
        )));
        output.diagnostics.push(report_with_artifact(
            format!("{} diagnostics", context.spec.display_name),
            diagnostics,
            artifact.clone(),
            context.project_root,
        ));

        let (template, fallback, field) = if changed_paths.is_empty() {
            (
                &context.spec.messages.issues_agent,
                pkl::default_issues_agent(),
                "issuesAgent",
            )
        } else {
            (
                &context.spec.messages.issues_changed_agent,
                pkl::default_issues_changed_agent(),
                "issuesChangedAgent",
            )
        };
        output
            .agent_feedback
            .push(AgentFeedback::Issues(Box::new(PendingIssues {
                template: template.clone(),
                fallback,
                field,
                tool: tool.name.to_owned(),
                tool_id: tool.id.to_owned(),
                project_root: context.project_root.to_path_buf(),
                changed_files: changed_paths,
                issue_files: issue_paths,
                diagnostics: artifact,
                output: issue_outputs.join("\n"),
            })));
    } else if !changed_paths.is_empty() {
        // Tools on the default template share one consolidated agent line;
        // a customised `cleanChangedAgent` is rendered as configured (or, if
        // it cannot be, the tool joins the shared line).
        let template = &context.spec.messages.clean_changed_agent;
        let mut in_agent_line = *template == pkl::default_clean_changed_agent();
        if !in_agent_line {
            let args = MessageArgs {
                changed_files: &changed_paths,
                ..MessageArgs::default()
            };
            match render_template(template, &tool, &args) {
                Ok(rendered) => output
                    .agent_feedback
                    .push(AgentFeedback::Rendered(rendered)),
                Err(error) => {
                    output.notices.push(template_failure_notice(
                        &tool,
                        "cleanChangedAgent",
                        &error,
                    ));
                    in_agent_line = true;
                }
            }
        }
        output.auto_fixed.push(AutoFixed {
            tool: context.spec.display_name.clone(),
            files: changed_paths,
            in_agent_line,
        });
    }

    if !out_of_scope_files.is_empty() {
        let files = out_of_scope_files
            .iter()
            .map(|path| rel_display(path, context.project_root))
            .collect::<Vec<_>>();
        let more = if files.len() > 5 { ", …" } else { "" };
        output.notices.push(UserNotice::info(format!(
            "velvet-glove: not reporting issues outside the files this call changed: {} ({}{more}).",
            context.spec.display_name,
            files[..files.len().min(5)].join(", ")
        )));
    }

    status.operational_failure = status.operational_failure || had_hard_failure.is_some();
    Ok(status)
}

/// The notice for a missing executable, from `messages.unavailableUser` when
/// set, else worded as at Stop.
fn render_unavailable_message(
    context: &ToolContext<'_>,
    phase: &str,
    executable: &str,
    install_hint: Option<&str>,
    notices: &mut Vec<UserNotice>,
) -> String {
    if let Some(template) = context.spec.messages.unavailable_user.as_ref() {
        let tool = context.template_tool();
        let args = MessageArgs {
            phase_error: Some((phase, executable, install_hint)),
            ..MessageArgs::default()
        };
        match render_template(template, &tool, &args) {
            Ok(message) => return message,
            Err(error) => notices.push(template_failure_notice(&tool, "unavailableUser", &error)),
        }
    }
    let hint = install_hint
        .map(|hint| format!("; {hint}"))
        .unwrap_or_default();
    format!(
        "velvet-glove could not run {} ({executable} not found{hint}).",
        context.spec.display_name
    )
}

/// The notice for a phase that failed operationally, from
/// `messages.failedUser` when set, else worded as at Stop.
fn render_failed_message(
    context: &ToolContext<'_>,
    diagnostics_path: &Path,
    (phase, exit_code, error): (&str, Option<i32>, Option<&str>),
    notices: &mut Vec<UserNotice>,
) -> String {
    if let Some(template) = context.spec.messages.failed_user.as_ref() {
        let tool = context.template_tool();
        let args = MessageArgs {
            diagnostics_path: Some(diagnostics_path),
            phase_error: Some((phase, "", None)),
            ..MessageArgs::default()
        };
        match render_template(template, &tool, &args) {
            Ok(message) => return message,
            Err(error) => notices.push(template_failure_notice(&tool, "failedUser", &error)),
        }
    }
    let reason = match (error, exit_code) {
        (Some(error), _) => format!("{phase}: {error}"),
        (None, Some(code)) => format!("{phase} failed with exit code {code}"),
        (None, None) => format!("{phase} was terminated by a signal"),
    };
    format!(
        "velvet-glove could not run {} ({reason}; log: {}).",
        context.spec.display_name,
        diagnostics_path.display()
    )
}
