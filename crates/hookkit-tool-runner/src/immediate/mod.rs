//! Immediate PostToolUse runner: runs each configured tool on the files one
//! tool call changed and lowers the collected result to the harness.

mod diagnostics;
mod jobs;
mod lowering;
mod messages;
mod outcomes;
mod output;

pub use output::{RunnerDomainOutcome, RunnerPostToolUseOutput};

use crate::convert::{convert_tool_spec, resolve_run_order};
use crate::errors::{error_summary, invalid_data};
use crate::jobs::{ToolContext, build_jobs, invocation_jobs};
use crate::matcher::FileMatcher;
use crate::paths::{display_roots, normalize_path};
use crate::project_lock::lock_project;
use crate::vcs;
use diagnostics::{immediate_log_directory, write_immediate_artifact};
use hookkit_common::{
    PostToolUseCommandEnvironment, PostToolUseInput, PostToolUseOutput, UserNotice,
};
use hookkit_core::RuntimeContext;
use hookkit_file_activity::FileActivityTarget;
use hookkit_file_activity::observe_post_tool as observe_file_activity;
use hookkit_pkl_config::schema as pkl;
use jobs::run_jobs;
use lowering::{lower_domain_outcome, lowering_warning_artifact};
use outcomes::accumulate_outcomes;
use output::{ExcerptLimits, is_empty_output};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn discover_modified_files(input: &PostToolUseInput, context: &RuntimeContext<'_>) -> Vec<PathBuf> {
    observe_file_activity(input, context)
        .evidence()
        .filter_map(|evidence| match &evidence.target {
            FileActivityTarget::Path { path, .. } => Some(normalize_path(path.as_std_path())),
            FileActivityTarget::Workspace { .. } => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Run an exact aligned input through the Pkl-driven runner.
pub(crate) fn run_post_tool_input(
    post_tool: PostToolUseInput,
    _environment: &PostToolUseCommandEnvironment,
    ctx: &RuntimeContext<'_>,
    config_path: Option<&Path>,
) -> hookkit_core::Result<PostToolUseOutput> {
    let harness = ctx.harness();
    let lowering_warning_artifact = lowering_warning_artifact(&post_tool, ctx);
    let clean = || {
        lower_domain_outcome(
            harness,
            RunnerDomainOutcome::Clean,
            lowering_warning_artifact.as_ref(),
        )
    };

    // Most tool calls (Read, Grep, ...) touch no files: skip every other cost,
    // including Pkl evaluation, for them.
    let mut candidates = discover_modified_files(&post_tool, ctx)
        .into_iter()
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return clean();
    }

    let cwd = ctx
        .workspace_roots()
        .first()
        .map(|root| PathBuf::from(root.as_str()))
        .ok_or_else(|| invalid_data("post-tool-use input has no workspace root".into()))?;
    // As at Stop, build outputs and other Git-ignored paths are never lint
    // candidates; a call that touched only those costs no Pkl evaluation.
    let ignored = vcs::git_ignored_paths(&normalize_path(&cwd), &candidates);
    candidates.retain(|path| !ignored.contains(path));
    if candidates.is_empty() {
        return clean();
    }
    let loaded = match hookkit_pkl_config::discover_and_load(&cwd, config_path) {
        Ok(loaded) => loaded,
        // A broken policy is an operational problem: tell the user, never the
        // agent, and never fail or block the tool call.
        Err(error) => {
            let detail = error.to_string();
            let mut message = format!(
                "velvet-glove: configuration error; no tools ran ({})",
                error_summary(&detail)
            );
            if let Ok(log) =
                write_immediate_artifact(&immediate_log_directory(), "config-error", &detail, ctx)
            {
                message.push_str(&format!(". Details: {}", log.display()));
            }
            let output = RunnerPostToolUseOutput::new(pkl::LoweringPolicy::default())
                .with_user_notice(UserNotice::error(message));
            return lower_domain_outcome(
                harness,
                RunnerDomainOutcome::Report(output),
                lowering_warning_artifact.as_ref(),
            );
        }
    };

    let project_root = normalize_path(&loaded.project_root);
    // A project's policy applies only to files inside the project: plans,
    // memory files, scratch files, and sibling repositories are left alone.
    candidates.retain(|path| path.starts_with(&project_root));
    if candidates.is_empty() {
        return clean();
    }
    let settings = &loaded.config.settings;
    let mut output = RunnerPostToolUseOutput::new(settings.lowering_policy).with_excerpt_limits(
        ExcerptLimits::new(
            &settings.deferred_reporting,
            display_roots(&project_root, &loaded.project_root),
        ),
    );
    let mut had_hard_failure: Option<String> = None;
    let mut had_harness_block_message: Option<String> = None;

    let tools = resolve_run_order(&loaded.config)?;
    if tools.is_empty() {
        return clean();
    }

    let global_diagnostics_dir = settings.diagnostics_directory.clone();

    let _project = lock_project(&project_root);
    for schema_spec in tools {
        if !schema_spec.enabled {
            continue;
        }
        let spec = convert_tool_spec(schema_spec, settings);
        let context = ToolContext {
            spec: &spec,
            project_root: &project_root,
            global_diagnostics_dir: global_diagnostics_dir.as_deref(),
        };
        // An error in one tool's configuration (a glob, a message template,
        // an unwritable diagnostics directory) is that tool's operational
        // problem: tell the user and keep everything already collected.
        let batch_status = run_immediate_tool(
            &context,
            &candidates,
            ctx,
            settings,
            &mut output,
            (&mut had_hard_failure, &mut had_harness_block_message),
        )
        .unwrap_or_else(|error| {
            output.notices.push(UserNotice::error(format!(
                "velvet-glove could not run {} ({error})",
                spec.display_name
            )));
            ToolBatchStatus {
                operational_failure: true,
                issues: false,
            }
        });

        if had_harness_block_message.is_some()
            || had_hard_failure.is_some()
            || (settings.fail_fast && batch_status.operational_failure)
            || (!settings.continue_after_issues && batch_status.issues)
        {
            break;
        }
    }

    let outcome = if let Some(message) = had_harness_block_message {
        RunnerDomainOutcome::HarnessBlock { message, output }
    } else if let Some(message) = had_hard_failure {
        RunnerDomainOutcome::OperationalFailure {
            message: format!("{message} (missingToolPolicy is hard-failure)"),
        }
    } else if is_empty_output(&output) {
        RunnerDomainOutcome::Clean
    } else {
        RunnerDomainOutcome::Report(output)
    };
    lower_domain_outcome(harness, outcome, lowering_warning_artifact.as_ref())
}

/// Run one tool on the candidates its globs select and fold its outcomes
/// into `output`. `flags` receive the run's hard-failure and harness-block
/// messages.
fn run_immediate_tool(
    context: &ToolContext<'_>,
    candidates: &[PathBuf],
    ctx: &RuntimeContext<'_>,
    settings: &pkl::Settings,
    output: &mut RunnerPostToolUseOutput,
    flags: (&mut Option<String>, &mut Option<String>),
) -> hookkit_core::Result<ToolBatchStatus> {
    let matcher = FileMatcher::new(&context.spec.file_selection)?;
    let runnable_paths = candidates
        .iter()
        .filter(|path| matcher.matches(path, context.project_root))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if runnable_paths.is_empty() {
        return Ok(ToolBatchStatus::default());
    }
    let base_jobs = build_jobs(&runnable_paths, context.project_root, context.spec);
    let jobs = invocation_jobs(&base_jobs, context.spec.phase_invocation);
    if jobs.is_empty() {
        return Ok(ToolBatchStatus::default());
    }
    let outcomes = run_jobs(&jobs, context, settings.jobs);
    let (had_hard_failure, had_harness_block_message) = flags;
    accumulate_outcomes(
        outcomes,
        context,
        ctx,
        settings.missing_tool_policy,
        output,
        had_hard_failure,
        had_harness_block_message,
    )
}

#[derive(Debug, Clone, Copy, Default)]
struct ToolBatchStatus {
    operational_failure: bool,
    issues: bool,
}
