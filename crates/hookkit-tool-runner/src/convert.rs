//! Conversion from the Pkl configuration schema to the runtime tool model.

use crate::errors::invalid_data;
use crate::spec::{
    CheckScope, CommandArgTemplate, ExitCodePolicy, FileSelection, InvocationGranularity,
    PhaseMode, ToolMessages, ToolPhase, ToolSpec, ToolWorkflow, UnexpectedExitPolicy,
    WorkspaceFallback, WriteBehavior,
};
use hookkit_pkl_config::schema as pkl;
use std::collections::BTreeSet;
use std::time::Duration;

/// Resolve the `run` list to ordered tool specs. Loading already rejects a
/// `run` entry naming no tool, so a missing one is an internal invariant
/// violation rather than a user error.
pub(crate) fn resolve_run_order(
    config: &pkl::RunnerConfig,
) -> hookkit_core::Result<Vec<&pkl::ToolSpec>> {
    let mut tools = Vec::with_capacity(config.run.len());
    for id in &config.run {
        let Some(spec) = config.tools.get(id) else {
            return Err(invalid_data(format!(
                "internal error: validated run list names unknown tool `{id}`"
            )));
        };
        tools.push(spec);
    }
    Ok(tools)
}

/// Convert a Pkl-shaped tool spec to the runtime execution type.
///
/// `ExtraArgs` expands to the tool's `extraArgs`, then the workflow's (for
/// explicit workflows), then the phase's or command's own.
pub(crate) fn convert_tool_spec(spec: &pkl::ToolSpec, settings: &pkl::Settings) -> ToolSpec {
    let phases: Vec<ToolPhase> = ordered_phases(spec)
        .into_iter()
        .map(|(id, phase)| {
            let mut converted = convert_phase((id, phase));
            converted.extra_args = concat_args(&[&spec.extra_args, &phase.extra_args]);
            converted
        })
        .collect();

    let mut exclude = settings.exclude.clone();
    exclude.extend(spec.files.exclude.clone());
    let workflows = convert_workflows(spec, &phases);
    let timeout_seconds = spec
        .timeout_seconds
        .unwrap_or(settings.command_timeout_seconds);

    ToolSpec {
        id: spec.id.clone(),
        display_name: spec.display_name.clone(),
        executable: spec.executable.clone(),
        install_hint: spec.install_hint.clone(),
        file_selection: FileSelection {
            include: spec.files.include.clone(),
            exclude,
        },
        workspace_indicator: spec.workspace_indicator.clone(),
        workspace_fallback: match spec.workspace_fallback {
            pkl::WorkspaceFallback::Skip => WorkspaceFallback::Skip,
            pkl::WorkspaceFallback::ProjectRoot => WorkspaceFallback::ProjectRoot,
        },
        phase_invocation: convert_invocation(spec.phase_invocation),
        workflows,
        phases,
        messages: convert_messages(&spec.messages),
        diagnostics_directory: spec.diagnostics.directory.clone(),
        enabled: spec.enabled,
        env: spec
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        timeout: (timeout_seconds > 0).then(|| Duration::from_secs(timeout_seconds)),
        local_bin_dirs: settings.local_bin_dirs.clone(),
    }
}

fn concat_args(parts: &[&[String]]) -> Vec<String> {
    parts.iter().flat_map(|part| part.iter().cloned()).collect()
}

fn convert_workflows(spec: &pkl::ToolSpec, phases: &[ToolPhase]) -> Vec<ToolWorkflow> {
    if !spec.workflows.is_empty() {
        return ordered_workflows(spec)
            .into_iter()
            .map(|(id, workflow)| ToolWorkflow {
                id: id.clone(),
                check: workflow.check.as_ref().map(|command| {
                    let mut check =
                        convert_workflow_command(format!("{id}.check"), command, PhaseMode::Verify);
                    check.extra_args =
                        concat_args(&[&spec.extra_args, &workflow.extra_args, &command.extra_args]);
                    check
                }),
                remedy: workflow.remedy.as_ref().map(|command| {
                    let mut remedy =
                        convert_workflow_command(format!("{id}.remedy"), command, PhaseMode::Fix);
                    remedy.extra_args =
                        concat_args(&[&spec.extra_args, &workflow.extra_args, &command.extra_args]);
                    remedy
                }),
                check_scope: match workflow.check_scope {
                    pkl::CheckScope::TargetFiles => CheckScope::TargetFiles,
                    pkl::CheckScope::Workspace => CheckScope::Workspace,
                },
                invocation: convert_invocation(workflow.invocation),
                compatibility_translation: false,
                enabled: workflow.enabled,
            })
            .collect();
    }

    // Compatibility translation for the existing immediate-runner phase
    // shape. Every mutator becomes a separate deferred workflow paired with
    // the last enabled verifier. A tool with no verifier (a user-defined
    // formatter) gets check-less workflows whose remedy result is reported
    // as an unverified auto-fix; builtin validation forbids that shape.
    let verifier = phases
        .iter()
        .rev()
        .find(|phase| phase.enabled && phase.is_verifier())
        .cloned();
    let mut workflows = phases
        .iter()
        .filter(|phase| phase.enabled && !phase.is_verifier())
        .map(|remedy| ToolWorkflow {
            id: remedy.id.clone(),
            check: verifier.clone(),
            remedy: Some(remedy.clone()),
            check_scope: if spec.workspace_indicator.is_some()
                && !remedy.args.iter().any(|arg| {
                    matches!(
                        arg,
                        CommandArgTemplate::Files | CommandArgTemplate::WorkspaceFiles
                    )
                }) {
                CheckScope::Workspace
            } else {
                CheckScope::TargetFiles
            },
            invocation: convert_invocation(spec.phase_invocation),
            compatibility_translation: true,
            enabled: true,
        })
        .collect::<Vec<_>>();
    if workflows.is_empty() {
        workflows.extend(
            phases
                .iter()
                .filter(|phase| phase.enabled && phase.is_verifier())
                .cloned()
                .map(|check| ToolWorkflow {
                    id: check.id.clone(),
                    check: Some(check),
                    remedy: None,
                    check_scope: if spec.workspace_indicator.is_some() {
                        CheckScope::Workspace
                    } else {
                        CheckScope::TargetFiles
                    },
                    invocation: convert_invocation(spec.phase_invocation),
                    compatibility_translation: true,
                    enabled: true,
                }),
        );
    }
    workflows
}

fn convert_invocation(invocation: pkl::InvocationGranularity) -> InvocationGranularity {
    match invocation {
        pkl::InvocationGranularity::PerFile => InvocationGranularity::PerFile,
        pkl::InvocationGranularity::Batch => InvocationGranularity::Batch,
        pkl::InvocationGranularity::Workspace => InvocationGranularity::Workspace,
    }
}

fn ordered_workflows(spec: &pkl::ToolSpec) -> Vec<(&String, &pkl::Workflow)> {
    let mut seen = BTreeSet::new();
    let mut workflows = Vec::new();
    for id in &spec.workflow_order {
        if let Some(workflow) = spec.workflows.get(id) {
            if seen.insert(id.clone()) {
                workflows.push((id, workflow));
            }
        }
    }
    workflows.extend(
        spec.workflows
            .iter()
            .filter(|(id, _)| !seen.contains(id.as_str())),
    );
    workflows
}

fn convert_workflow_command(
    id: String,
    command: &pkl::WorkflowCommand,
    mode: PhaseMode,
) -> ToolPhase {
    ToolPhase {
        id,
        mode,
        program: command.program.clone(),
        args: command.argv.iter().map(convert_argv_element).collect(),
        exit_codes: convert_exit_codes(&command.exit_codes),
        issues_on_stdout: command.issues_on_stdout,
        writes: convert_writes(command.writes),
        extra_args: command.extra_args.clone(),
        enabled: true,
    }
}

fn ordered_phases(spec: &pkl::ToolSpec) -> Vec<(String, &pkl::Phase)> {
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::<(String, &pkl::Phase)>::new();

    // Honor explicit phase order first.
    for id in &spec.phase_order {
        if let Some(phase) = spec.phases.get(id) {
            if seen.insert(id.clone()) {
                out.push((id.clone(), phase));
            }
        }
    }

    // Append any remaining phases sorted by canonical mode order, then by id.
    let mut remaining: Vec<(&String, &pkl::Phase)> = spec
        .phases
        .iter()
        .filter(|(id, _)| !seen.contains(id.as_str()))
        .collect();
    remaining.sort_by(|a, b| {
        canonical_mode_order(a.1.mode)
            .cmp(&canonical_mode_order(b.1.mode))
            .then_with(|| a.0.cmp(b.0))
    });
    for (id, phase) in remaining {
        out.push((id.clone(), phase));
    }
    out
}

fn canonical_mode_order(mode: pkl::PhaseMode) -> u8 {
    match mode {
        pkl::PhaseMode::Format => 0,
        pkl::PhaseMode::Fix => 1,
        pkl::PhaseMode::Verify => 2,
        pkl::PhaseMode::CheckOnly => 3,
    }
}

fn convert_phase((id, phase): (String, &pkl::Phase)) -> ToolPhase {
    ToolPhase {
        id,
        mode: convert_phase_mode(phase.mode),
        program: phase.program.clone(),
        args: phase.argv.iter().map(convert_argv_element).collect(),
        exit_codes: convert_exit_codes(&phase.exit_codes),
        issues_on_stdout: false,
        writes: convert_writes(phase.writes),
        extra_args: phase.extra_args.clone(),
        enabled: phase.enabled,
    }
}

fn convert_phase_mode(mode: pkl::PhaseMode) -> PhaseMode {
    match mode {
        pkl::PhaseMode::Format => PhaseMode::Format,
        pkl::PhaseMode::Fix => PhaseMode::Fix,
        pkl::PhaseMode::Verify => PhaseMode::Verify,
        pkl::PhaseMode::CheckOnly => PhaseMode::CheckOnly,
    }
}

fn convert_argv_element(element: &pkl::ArgvElement) -> CommandArgTemplate {
    match element {
        pkl::ArgvElement::Literal(s) => CommandArgTemplate::Literal(s.clone()),
        pkl::ArgvElement::Token(t) => match t {
            pkl::ArgToken::Files => CommandArgTemplate::Files,
            pkl::ArgToken::WorkspaceFiles => CommandArgTemplate::WorkspaceFiles,
            pkl::ArgToken::Workspace => CommandArgTemplate::Workspace,
            pkl::ArgToken::WorkspaceIndicator => CommandArgTemplate::WorkspaceIndicator,
            pkl::ArgToken::ProjectRoot => CommandArgTemplate::ProjectRoot,
            pkl::ArgToken::ToolExecutable => CommandArgTemplate::ToolExecutable,
            pkl::ArgToken::ExtraArgs => CommandArgTemplate::ExtraArgs,
        },
    }
}

fn convert_exit_codes(codes: &pkl::ExitCodes) -> ExitCodePolicy {
    ExitCodePolicy {
        clean: codes.clean.clone(),
        issues: codes.issues.clone(),
        failure: codes.failure.clone(),
        unexpected: match codes.unexpected {
            pkl::UnexpectedExitPolicy::Failure => UnexpectedExitPolicy::Failure,
            pkl::UnexpectedExitPolicy::Issues => UnexpectedExitPolicy::Issues,
        },
    }
}

fn convert_writes(writes: pkl::WriteBehavior) -> WriteBehavior {
    match writes {
        pkl::WriteBehavior::None => WriteBehavior::None,
        pkl::WriteBehavior::TargetFiles => WriteBehavior::TargetFiles,
        pkl::WriteBehavior::MatchingGlobs => WriteBehavior::MatchingGlobs,
        pkl::WriteBehavior::Workspace => WriteBehavior::Workspace,
    }
}

fn convert_messages(messages: &pkl::Messages) -> ToolMessages {
    ToolMessages {
        clean_changed_agent: messages.clean_changed_agent.clone(),
        issues_agent: messages.issues_agent.clone(),
        issues_changed_agent: messages.issues_changed_agent.clone(),
        unavailable_user: messages.unavailable_user.clone(),
        failed_user: messages.failed_user.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn extra_args_expand_tool_then_workflow_then_command() {
        let args = |values: &[&str]| values.iter().map(|v| v.to_string()).collect::<Vec<_>>();
        let command = |extra: &[&str]| pkl::WorkflowCommand {
            argv: vec![pkl::ArgvElement::Token(pkl::ArgToken::ExtraArgs)],
            extra_args: args(extra),
            ..pkl::WorkflowCommand::default()
        };
        let explicit = pkl::ToolSpec {
            id: "ruff".into(),
            executable: "ruff".into(),
            extra_args: args(&["--tool"]),
            env: BTreeMap::from([("RUFF_CACHE_DIR".into(), "/tmp/cache".into())]),
            timeout_seconds: Some(7),
            workflows: BTreeMap::from([(
                "lint".into(),
                pkl::Workflow {
                    check: Some(command(&["--check"])),
                    remedy: Some(pkl::WorkflowCommand {
                        writes: pkl::WriteBehavior::TargetFiles,
                        ..command(&["--remedy"])
                    }),
                    extra_args: args(&["--ignore", "F401"]),
                    ..pkl::Workflow::default()
                },
            )]),
            ..pkl::ToolSpec::default()
        };
        let settings = pkl::Settings {
            local_bin_dirs: args(&["bin"]),
            ..pkl::Settings::default()
        };
        let spec = convert_tool_spec(&explicit, &settings);
        let workflow = &spec.workflows[0];
        assert_eq!(
            workflow.check.as_ref().unwrap().extra_args,
            args(&["--tool", "--ignore", "F401", "--check"])
        );
        assert_eq!(
            workflow.remedy.as_ref().unwrap().extra_args,
            args(&["--tool", "--ignore", "F401", "--remedy"])
        );
        assert_eq!(
            spec.env,
            vec![("RUFF_CACHE_DIR".to_owned(), "/tmp/cache".to_owned())]
        );
        assert_eq!(spec.timeout, Some(Duration::from_secs(7)));
        assert_eq!(spec.local_bin_dirs, args(&["bin"]));

        let phased = pkl::ToolSpec {
            id: "fmt".into(),
            executable: "fmt".into(),
            extra_args: args(&["--tool"]),
            timeout_seconds: Some(0),
            phases: BTreeMap::from([
                (
                    "format".into(),
                    pkl::Phase {
                        mode: pkl::PhaseMode::Format,
                        writes: pkl::WriteBehavior::TargetFiles,
                        extra_args: args(&["--format"]),
                        ..pkl::Phase::default()
                    },
                ),
                (
                    "verify".into(),
                    pkl::Phase {
                        extra_args: args(&["--verify"]),
                        ..pkl::Phase::default()
                    },
                ),
            ]),
            ..pkl::ToolSpec::default()
        };
        let spec = convert_tool_spec(&phased, &pkl::Settings::default());
        assert_eq!(spec.phases[0].extra_args, args(&["--tool", "--format"]));
        assert_eq!(spec.phases[1].extra_args, args(&["--tool", "--verify"]));
        // Phase-translated workflows reuse the converted phases.
        let translated = &spec.workflows[0];
        assert_eq!(
            translated.remedy.as_ref().unwrap().extra_args,
            args(&["--tool", "--format"])
        );
        assert_eq!(
            translated.check.as_ref().unwrap().extra_args,
            args(&["--tool", "--verify"])
        );
        assert_eq!(spec.timeout, None, "timeoutSeconds = 0 disables the limit");
    }

    #[test]
    fn compatibility_workflows_inherit_phase_invocation() {
        let schema = pkl::ToolSpec {
            id: "jq".into(),
            executable: "jq".into(),
            phase_invocation: pkl::InvocationGranularity::PerFile,
            phases: BTreeMap::from([("verify".into(), pkl::Phase::default())]),
            phase_order: vec!["verify".into()],
            ..pkl::ToolSpec::default()
        };

        let spec = convert_tool_spec(&schema, &pkl::Settings::default());

        assert_eq!(spec.phase_invocation, InvocationGranularity::PerFile);
        assert_eq!(spec.workflows.len(), 1);
        assert_eq!(spec.workflows[0].id, "verify");
        assert_eq!(spec.workflows[0].invocation, InvocationGranularity::PerFile);
        assert!(spec.workflows[0].compatibility_translation);
        assert!(spec.workflows[0].check.is_some());
        assert!(spec.workflows[0].remedy.is_none());
    }
}
