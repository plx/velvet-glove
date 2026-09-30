//! What a deferred run does with pending file activity: resolution gaps, retries, and handled baselines.

use super::{CoverageGap, DeferredRunResult, FileStatus};
use crate::errors::{activity_error, invalid_data};
use crate::paths::normalize_path;
use hookkit_core::Utf8PathBuf;
use hookkit_file_activity::{
    FileActivityEvent, FileActivityStore, FileActivityTarget, PendingFileActivity,
};
use hookkit_pkl_config::schema as pkl;
use hookkit_session_state::EntityView;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct ActivityResolution {
    pub(crate) not_applicable_files: BTreeSet<PathBuf>,
    pub(crate) unresolved_targets: Vec<FileActivityTarget>,
    pub(crate) gap_messages: BTreeSet<String>,
    pub(crate) truncated: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DeferredStateDisposition {
    pub(crate) retry_files: BTreeSet<Utf8PathBuf>,
    pub(crate) retry_targets: Vec<FileActivityTarget>,
    pub(crate) retry_gaps: BTreeSet<String>,
    pub(crate) handled_files: BTreeSet<Utf8PathBuf>,
}

pub(crate) fn source_gap_messages(view: &EntityView<'_, PendingFileActivity>) -> BTreeSet<String> {
    view.events()
        .iter()
        .filter_map(|record| match record.event() {
            FileActivityEvent::Gap(gap) => Some(gap.detail.clone()),
            FileActivityEvent::Retry(retry) if retry.target.is_none() => Some(retry.reason.clone()),
            FileActivityEvent::Evidence(_) | FileActivityEvent::Retry(_) => None,
        })
        .collect()
}

pub(crate) fn record_activity_resolution(
    result: &mut DeferredRunResult,
    resolution: &ActivityResolution,
) {
    for path in &resolution.not_applicable_files {
        result.record_not_applicable(path.clone());
    }
    for (index, target) in resolution.unresolved_targets.iter().enumerate() {
        let target = serde_json::to_string(target)
            .unwrap_or_else(|_| "unserializable file activity target".into());
        result.record_coverage_gap(CoverageGap {
            id: format!("unresolved-target-{index:03}"),
            target: Some(target.clone()),
            message: format!("file activity target could not be fully materialized: {target}"),
            retained: true,
        });
    }
    for (index, message) in resolution.gap_messages.iter().enumerate() {
        result.record_coverage_gap(CoverageGap {
            id: format!("source-gap-{index:03}"),
            target: None,
            message: message.clone(),
            retained: true,
        });
    }
    if resolution.truncated {
        result.record_coverage_gap(CoverageGap {
            id: "resolution-budget-exhausted".into(),
            target: None,
            message: "file activity target resolution exhausted its traversal budget".into(),
            retained: true,
        });
    }
}

pub(crate) fn plan_deferred_state_disposition(
    result: &DeferredRunResult,
    resolution: &ActivityResolution,
    missing_tool_policy: pkl::MissingToolPolicy,
) -> hookkit_core::Result<DeferredStateDisposition> {
    let mut retry_files = BTreeSet::new();
    for file in result.files.values() {
        if file.status == FileStatus::ManualFixesNeeded {
            retry_files.insert(utf8_activity_path(&file.path)?);
        }
    }
    for problem in result.operational_problems.values() {
        // Retrying cannot conjure a missing executable; under the default
        // notice-only policy the next edit of these files checks them again.
        if problem.missing_tool && missing_tool_policy == pkl::MissingToolPolicy::UserNotice {
            continue;
        }
        for path in &problem.affected_files {
            retry_files.insert(utf8_activity_path(path)?);
        }
    }

    let mut handled_files = BTreeSet::new();
    for file in result.files.values() {
        if matches!(file.status, FileStatus::Clean | FileStatus::AutoFixed) {
            handled_files.insert(utf8_activity_path(&file.path)?);
        }
    }
    // A missing exact path is itself a stable handled state. Recording it
    // prevents an opt-in Git-dirty fallback from resurrecting the same
    // deletion immediately after the source observation is discharged.
    for path in &resolution.not_applicable_files {
        handled_files.insert(utf8_activity_path(path)?);
    }
    handled_files.retain(|path| !retry_files.contains(path));

    let mut retry_targets = resolution.unresolved_targets.clone();
    retry_targets.sort();
    retry_targets.dedup();
    let mut retry_gaps = resolution.gap_messages.clone();
    if resolution.truncated {
        retry_gaps.insert("file activity target resolution exhausted its traversal budget".into());
    }
    Ok(DeferredStateDisposition {
        retry_files,
        retry_targets,
        retry_gaps,
        handled_files,
    })
}

pub(crate) fn apply_deferred_state_disposition(
    activity_store: &FileActivityStore,
    disposition: DeferredStateDisposition,
    run_id: String,
) -> hookkit_core::Result<()> {
    activity_store
        .requeue_exact("deferred-unresolved-file", disposition.retry_files)
        .map_err(activity_error)?;
    activity_store
        .requeue_targets("deferred-unresolved-target", disposition.retry_targets)
        .map_err(activity_error)?;
    activity_store
        .requeue_gaps("deferred-coverage-gap", disposition.retry_gaps)
        .map_err(activity_error)?;
    if disposition.handled_files.is_empty() {
        return Ok(());
    }
    let baseline_report = activity_store
        .record_handled_baselines(disposition.handled_files, run_id)
        .map_err(activity_error)?;
    if baseline_report.failures.is_empty() {
        return Ok(());
    }
    let failures = baseline_report
        .failures
        .iter()
        .map(|failure| format!("{}: {}", failure.path, failure.message))
        .collect::<Vec<_>>()
        .join("; ");
    Err(invalid_data(format!(
        "could not record all handled file baselines; source window retained: {failures}"
    )))
}

fn utf8_activity_path(path: &Path) -> hookkit_core::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(normalize_path(path)).map_err(|path| {
        invalid_data(format!(
            "deferred file activity path is not valid UTF-8: {}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deferred::{FileAssessment, OperationalProblem};

    #[test]
    fn state_disposition_discharges_successes_and_retries_only_unfinished_files() {
        let root = PathBuf::from("/tmp/hookkit-selective-disposition");
        let clean = root.join("clean.rs");
        let auto_fixed = root.join("auto.rs");
        let manual = root.join("manual.rs");
        let operational = root.join("operational.rs");
        let deleted = root.join("deleted.rs");
        let unchecked = root.join("unchecked.rs");
        let mut result = DeferredRunResult::default();
        result.record_file(FileAssessment::new(&clean, FileStatus::Clean));
        result.record_file(FileAssessment::new(&auto_fixed, FileStatus::AutoFixed));
        result.record_file(FileAssessment::new(&manual, FileStatus::ManualFixesNeeded));
        for (id, path, missing_tool) in [
            ("tool-failure", &operational, false),
            ("tool-missing", &unchecked, true),
        ] {
            result.record_operational_problem(OperationalProblem {
                id: id.into(),
                tool_id: Some(id.into()),
                tool_name: None,
                missing_tool,
                install_hint: None,
                phase: Some("initial-check".into()),
                affected_files: vec![path.clone()],
                message: "tool crashed".into(),
                artifact_ids: Vec::new(),
            });
        }
        let unresolved = FileActivityTarget::Workspace {
            root: Some(Utf8PathBuf::from("/tmp/hookkit-selective-disposition")),
        };
        let resolution = ActivityResolution {
            not_applicable_files: BTreeSet::from([deleted.clone()]),
            unresolved_targets: vec![unresolved.clone()],
            gap_messages: BTreeSet::from(["dynamic shell target".into()]),
            truncated: false,
        };

        let blocking = plan_deferred_state_disposition(
            &result,
            &resolution,
            pkl::MissingToolPolicy::HarnessBlock,
        )
        .unwrap();
        assert!(
            blocking
                .retry_files
                .contains(&Utf8PathBuf::from_path_buf(unchecked).unwrap()),
            "a blocking missing tool keeps its files pending"
        );
        let disposition = plan_deferred_state_disposition(
            &result,
            &resolution,
            pkl::MissingToolPolicy::UserNotice,
        )
        .unwrap();
        assert_eq!(
            disposition.retry_files,
            BTreeSet::from([
                Utf8PathBuf::from_path_buf(manual).unwrap(),
                Utf8PathBuf::from_path_buf(operational).unwrap(),
            ])
        );
        assert_eq!(disposition.retry_targets, vec![unresolved]);
        assert_eq!(
            disposition.retry_gaps,
            BTreeSet::from(["dynamic shell target".into()])
        );
        assert_eq!(
            disposition.handled_files,
            BTreeSet::from([
                Utf8PathBuf::from_path_buf(auto_fixed).unwrap(),
                Utf8PathBuf::from_path_buf(clean).unwrap(),
                Utf8PathBuf::from_path_buf(deleted).unwrap(),
            ])
        );
    }
}
