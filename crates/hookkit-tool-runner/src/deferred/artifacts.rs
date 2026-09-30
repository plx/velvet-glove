//! Per-command log artifacts in a run bundle, and pruning of old bundles.

use super::plan::PlannedDeferredTool;
use super::summary::BatchToolSummary;
use super::{
    ArtifactClassification, CheckOutcome, CommandPhase, DeferredLog, DeferredRunResult,
    RunArtifact, ScheduledWorkflow, combined_output,
};
use crate::command::{PhaseLog, PhaseStatus, format_logs};
use crate::errors::invalid_data;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Writes one text artifact at a run-relative path and returns its absolute
/// path: a session run bundle for hooks, a plain directory for `check`.
type ArtifactWriter<'a> = dyn FnMut(&str, &str) -> hookkit_core::Result<PathBuf> + 'a;

pub(crate) fn write_deferred_artifacts(
    write: &mut ArtifactWriter<'_>,
    plan: &[ScheduledWorkflow],
    tools: &[PlannedDeferredTool],
    logs: &[DeferredLog],
    result: &mut DeferredRunResult,
) -> hookkit_core::Result<Vec<BatchToolSummary>> {
    let mut tool_artifacts = BTreeMap::<usize, Vec<String>>::new();
    for log in logs {
        let scheduled = plan
            .iter()
            .find(|scheduled| {
                scheduled.tool_index == log.tool_index
                    && scheduled.workflow_index == log.workflow_index
                    && scheduled.job_index == log.job_index
            })
            .ok_or_else(|| invalid_data("deferred log has no scheduled workflow".into()))?;
        let report_id = scheduled.report_id();
        let changed_files = result
            .reports
            .get(&report_id)
            .map(|report| report.changed_files.clone())
            .unwrap_or_default();
        let candidate_files = scheduled.job.files.clone();
        let files = candidate_files
            .iter()
            .chain(changed_files.iter())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let phase = command_phase_name(log.phase);
        let tool_component = safe_artifact_component(&scheduled.spec.id);
        let workflow_component = safe_artifact_component(&scheduled.workflow_id);
        let relative = format!(
            "tools/{:03}-{tool_component}/workflows/{:03}-{workflow_component}/jobs/{:03}/{phase}.log",
            scheduled.tool_index, scheduled.workflow_index, scheduled.job_index,
        );
        let contents = format_deferred_artifact(log)?;
        let absolute = write(&relative, &contents)?;
        let artifact_id = format!("{report_id}-{phase}");
        attach_report_artifact(result, &report_id, &artifact_id);
        result.record_artifact(RunArtifact {
            id: artifact_id.clone(),
            absolute_path: absolute,
            run_relative_path: relative.clone().into(),
            media_type: "text/plain; charset=utf-8".into(),
            tool_id: Some(scheduled.spec.id.clone()),
            workflow_id: Some(scheduled.workflow_id.clone()),
            job_id: Some(format!("{:03}", scheduled.job_index)),
            report_id: Some(report_id),
            phase: log.phase,
            classification: artifact_classification(&log.log),
            exit_code: log.log.status,
            program: Some(log.log.program.clone()),
            arguments: log.log.arguments.clone(),
            working_directory: Some(scheduled.job.workspace_dir.clone()),
            files,
            candidate_files,
            changed_files,
            contents,
            output: combined_output(&log.log),
        });
        tool_artifacts
            .entry(scheduled.tool_index)
            .or_default()
            .push(relative);
    }

    let mut summaries = Vec::new();
    for tool in tools {
        let issues = result.reports.values().any(|report| {
            report.tool_id == tool.spec.id && report.final_check == Some(CheckOutcome::Issues)
        });
        let operational_failure = result
            .operational_problems
            .values()
            .any(|problem| problem.tool_id.as_deref() == Some(tool.spec.id.as_str()));
        summaries.push(BatchToolSummary {
            tool_id: tool.spec.id.clone(),
            file_count: tool.files.len(),
            issues,
            operational_failure,
            artifacts: tool_artifacts.remove(&tool.index).unwrap_or_default(),
        });
    }
    Ok(summaries)
}

fn attach_report_artifact(result: &mut DeferredRunResult, report_id: &str, artifact_id: &str) {
    if let Some(report) = result.reports.get_mut(report_id) {
        report.artifact_ids.push(artifact_id.into());
        report.artifact_ids.sort();
        report.artifact_ids.dedup();
    }
    for file in result.files.values_mut() {
        for report in &mut file.reports {
            if report.report_id == report_id {
                report.artifact_ids.push(artifact_id.into());
                report.artifact_ids.sort();
                report.artifact_ids.dedup();
            }
        }
    }
    for problem in result.operational_problems.values_mut() {
        if problem.id.starts_with(&format!("{report_id}-")) {
            problem.artifact_ids.push(artifact_id.into());
            problem.artifact_ids.sort();
            problem.artifact_ids.dedup();
        }
    }
}

fn format_deferred_artifact(log: &DeferredLog) -> hookkit_core::Result<String> {
    let argv = std::iter::once(log.log.program.as_str())
        .chain(log.log.arguments.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let argv = serde_json::to_string(&argv)
        .map_err(|error| invalid_data(format!("could not serialize command argv: {error}")))?;
    Ok(format!(
        "workflow_index: {}\njob_index: {}\ncommand_phase: {}\nargv: {argv}\n{}",
        log.workflow_index,
        log.job_index,
        command_phase_name(log.phase),
        format_logs(std::slice::from_ref(&log.log)),
    ))
}

fn artifact_classification(log: &PhaseLog) -> ArtifactClassification {
    if log.error.is_some() {
        return ArtifactClassification::SpawnError;
    }
    match log.classification {
        Some(PhaseStatus::Clean) => ArtifactClassification::Clean,
        Some(PhaseStatus::Issues) => ArtifactClassification::Issues,
        Some(PhaseStatus::Failure) => ArtifactClassification::Failure,
        None if log.status.is_none() => ArtifactClassification::Failure,
        None => ArtifactClassification::Unclassified,
    }
}

fn command_phase_name(phase: CommandPhase) -> &'static str {
    match phase {
        CommandPhase::InitialCheck => "initial-check",
        CommandPhase::Recheck => "recheck",
        CommandPhase::Remedy => "remedy",
        CommandPhase::FinalCheck => "final-check",
        CommandPhase::Combined => "combined",
        CommandPhase::Configuration => "configuration",
    }
}

fn safe_artifact_component(value: &str) -> String {
    let component = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if component.is_empty() {
        "unnamed".into()
    } else {
        component
    }
}

/// Keep the newest `keep` run bundles (named `<millis>-...`) in `runs`,
/// always including `current`: the run just committed is never removed, even
/// if a backwards clock step gave it the oldest name.
pub(crate) fn prune_run_bundles(runs: &Path, keep: usize, current: &Path) {
    let Ok(entries) = std::fs::read_dir(runs) else {
        return;
    };
    let mut bundles = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| entry.path() != current)
        .filter_map(|entry| {
            let millis = entry
                .file_name()
                .to_str()?
                .split('-')
                .next()?
                .parse::<u128>()
                .ok()?;
            Some((millis, entry.path()))
        })
        .collect::<Vec<_>>();
    bundles.sort_by(|left, right| right.cmp(left));
    for (_, stale) in bundles.into_iter().skip(keep.saturating_sub(1)) {
        let _ = std::fs::remove_dir_all(stale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_test_directory;

    #[test]
    fn run_bundle_pruning_keeps_the_newest_bundles() {
        let runs = unique_test_directory("prune-runs");
        for name in [
            "1000-1-0-turn-completion",
            "3000-1-0-turn-completion",
            "2000-1-0-turn-completion",
            "not-a-run",
        ] {
            std::fs::create_dir_all(runs.join(name)).unwrap();
        }
        prune_run_bundles(&runs, 2, &runs.join("3000-1-0-turn-completion"));
        let mut remaining = std::fs::read_dir(&runs)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        remaining.sort();
        assert_eq!(
            remaining,
            vec![
                "2000-1-0-turn-completion",
                "3000-1-0-turn-completion",
                "not-a-run"
            ]
        );

        // A run committed after a backwards clock step has the oldest name
        // but is never the one removed.
        std::fs::create_dir_all(runs.join("500-1-0-turn-completion")).unwrap();
        prune_run_bundles(&runs, 2, &runs.join("500-1-0-turn-completion"));
        let mut remaining = std::fs::read_dir(&runs)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        remaining.sort();
        assert_eq!(
            remaining,
            vec![
                "3000-1-0-turn-completion",
                "500-1-0-turn-completion",
                "not-a-run"
            ]
        );
        let _ = std::fs::remove_dir_all(runs);
    }

    #[test]
    fn deferred_artifact_argv_is_unambiguous_and_path_components_are_safe() {
        let log = DeferredLog {
            tool_index: 0,
            workflow_index: 1,
            job_index: 2,
            phase: CommandPhase::InitialCheck,
            log: PhaseLog {
                phase: "check".into(),
                command: "checker argument with spaces line break".into(),
                program: "checker tool".into(),
                arguments: vec!["argument with spaces".into(), "line\nbreak".into()],
                status: Some(0),
                classification: Some(PhaseStatus::Clean),
                stdout: "ok".into(),
                stderr: String::new(),
                error: None,
            },
        };
        let contents = format_deferred_artifact(&log).unwrap();
        let argv = contents
            .lines()
            .find_map(|line| line.strip_prefix("argv: "))
            .expect("argv line");
        assert_eq!(
            serde_json::from_str::<Vec<String>>(argv).unwrap(),
            vec!["checker tool", "argument with spaces", "line\nbreak"]
        );
        assert_eq!(
            safe_artifact_component("../../tool/name"),
            "______tool_name"
        );
    }
}
