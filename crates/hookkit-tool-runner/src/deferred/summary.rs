//! The `summary.json` a deferred run commits to its run bundle.

use super::disposition::DeferredStateDisposition;
use super::{
    BlockReasons, DeferredRunResult, FileStatus, RenderedBuckets, RenderedMessages,
    StopLoweringMetadata,
};
use crate::errors::invalid_data;
use hookkit_core::{HarnessId, Utf8PathBuf};
use hookkit_file_activity::FileActivityTarget;
use hookkit_session_state::RunBundle;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchToolSummary {
    pub(crate) tool_id: String,
    pub(crate) file_count: usize,
    pub(crate) issues: bool,
    pub(crate) operational_failure: bool,
    pub(crate) artifacts: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchRunSummary {
    schema_version: u32,
    pub(crate) run: BatchRunIdentity,
    status: &'static str,
    source_entry_count: usize,
    source_entry_ids: Vec<String>,
    candidate_files: Vec<PathBuf>,
    counts: BatchCounts,
    clean_files: Vec<PathBuf>,
    auto_fixed_files: Vec<PathBuf>,
    manual_fix_files: Vec<PathBuf>,
    groups: Vec<BatchGroupSummary>,
    artifact_paths: Vec<PathBuf>,
    state_disposition: PlannedStateDisposition,
    block: BlockMetadata,
    rendered_messages: RenderedMessageMetadata,
    tools: Vec<BatchToolSummary>,
    result: DeferredRunResult,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchRunIdentity {
    pub(crate) id: String,
    project_root: PathBuf,
    summary_path: PathBuf,
    state_directory: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchCounts {
    clean: usize,
    auto_fixed: usize,
    manual_fixes_needed: usize,
    operational_errors: usize,
    uncovered: usize,
    not_applicable: usize,
    coverage_gaps: usize,
    out_of_scope: usize,
    groups: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchGroupSummary {
    id: String,
    display_name: String,
    files: Vec<PathBuf>,
    count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlannedStateDisposition {
    source: &'static str,
    retry_files: Vec<Utf8PathBuf>,
    retry_targets: Vec<FileActivityTarget>,
    retry_gaps: Vec<String>,
    handled_baseline_files: Vec<Utf8PathBuf>,
}

/// Why the run did or did not block, including the loop guard's view.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlockMetadata {
    pub(crate) reasons: BlockReasons,
    pub(crate) stop_hook_active: bool,
    pub(crate) fingerprint: String,
    pub(crate) blocked: bool,
    pub(crate) guard_note: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenderedMessageMetadata {
    harness: String,
    lowering: StopLoweringMetadata,
    buckets: RenderedBuckets,
    user: Option<String>,
    agent: Option<String>,
    references_summary: bool,
}

pub(crate) struct BatchSummaryParts<'a> {
    pub(crate) run: &'a RunBundle,
    pub(crate) project_root: &'a Path,
    pub(crate) state_directory: &'a Path,
    pub(crate) harness: &'a HarnessId,
    pub(crate) status: &'static str,
    pub(crate) rendered_messages: RenderedMessages,
    pub(crate) lowering: StopLoweringMetadata,
    pub(crate) block: BlockMetadata,
    pub(crate) source: (usize, Vec<String>),
    pub(crate) candidates: &'a [PathBuf],
    pub(crate) tools: Vec<BatchToolSummary>,
    pub(crate) disposition: &'a DeferredStateDisposition,
    pub(crate) result: DeferredRunResult,
}

pub(crate) fn build_batch_summary(
    parts: BatchSummaryParts<'_>,
) -> hookkit_core::Result<BatchRunSummary> {
    let run_id = run_id(parts.run.directory())?;
    let summary_path = parts.run.directory().join("summary.json");
    let RenderedMessages {
        buckets,
        user,
        agent,
    } = parts.rendered_messages;
    let summary_text = summary_path.to_string_lossy();
    let references_summary = user
        .iter()
        .chain(agent.iter())
        .any(|message| message.contains(summary_text.as_ref()));
    let clean_files = files_with_status(&parts.result, FileStatus::Clean);
    let auto_fixed_files = files_with_status(&parts.result, FileStatus::AutoFixed);
    let manual_fix_files = files_with_status(&parts.result, FileStatus::ManualFixesNeeded);
    let mut grouped = BTreeMap::<String, Vec<PathBuf>>::new();
    for file in parts.result.files.values() {
        grouped
            .entry(file.group_id.clone())
            .or_default()
            .push(file.path.clone());
    }
    let groups = grouped
        .into_iter()
        .map(|(id, mut files)| {
            files.sort();
            files.dedup();
            BatchGroupSummary {
                display_name: if id == "other" {
                    "Other".into()
                } else {
                    id.clone()
                },
                id,
                count: files.len(),
                files,
            }
        })
        .collect::<Vec<_>>();
    let artifact_paths = parts
        .result
        .artifacts
        .values()
        .map(|artifact| artifact.absolute_path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let counts = BatchCounts {
        clean: clean_files.len(),
        auto_fixed: auto_fixed_files.len(),
        manual_fixes_needed: manual_fix_files.len(),
        operational_errors: parts.result.operational_problems.len(),
        uncovered: parts.result.uncovered_files.len(),
        not_applicable: parts.result.not_applicable_files.len(),
        coverage_gaps: parts.result.coverage_gaps.len(),
        out_of_scope: parts.result.out_of_scope_reports().count(),
        groups: groups.len(),
    };
    let state_disposition = PlannedStateDisposition {
        source: "acknowledge-sealed-window",
        retry_files: parts.disposition.retry_files.iter().cloned().collect(),
        retry_targets: parts.disposition.retry_targets.clone(),
        retry_gaps: parts.disposition.retry_gaps.iter().cloned().collect(),
        handled_baseline_files: parts.disposition.handled_files.iter().cloned().collect(),
    };
    let (source_entry_count, source_entry_ids) = parts.source;
    Ok(BatchRunSummary {
        schema_version: 2,
        run: BatchRunIdentity {
            id: run_id,
            project_root: parts.project_root.to_path_buf(),
            summary_path,
            state_directory: parts.state_directory.to_path_buf(),
        },
        status: parts.status,
        source_entry_count,
        source_entry_ids,
        candidate_files: parts.candidates.to_vec(),
        counts,
        clean_files,
        auto_fixed_files,
        manual_fix_files,
        groups,
        artifact_paths,
        state_disposition,
        block: parts.block,
        rendered_messages: RenderedMessageMetadata {
            harness: parts.harness.to_string(),
            lowering: parts.lowering,
            buckets,
            user,
            agent,
            references_summary,
        },
        tools: parts.tools,
        result: parts.result,
    })
}

fn files_with_status(result: &DeferredRunResult, status: FileStatus) -> Vec<PathBuf> {
    result
        .files
        .values()
        .filter(|file| file.status == status)
        .map(|file| file.path.clone())
        .collect()
}

pub(crate) fn run_id(directory: &Path) -> hookkit_core::Result<String> {
    directory
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            invalid_data(format!(
                "run directory has no UTF-8 id: {}",
                directory.display()
            ))
        })
}
