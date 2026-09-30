//! Before/after file snapshots over the paths a tool's commands may write.

use crate::jobs::{ToolContext, ToolJob};
use crate::matcher::FileMatcher;
use crate::spec::{FileSelection, WriteBehavior};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct Snapshot {
    files: BTreeMap<PathBuf, Option<Vec<u8>>>,
}

impl Snapshot {
    pub(crate) fn read(paths: &BTreeSet<PathBuf>) -> Self {
        let files = paths
            .iter()
            .map(|path| {
                let bytes = if path.is_file() {
                    std::fs::read(path).ok()
                } else {
                    None
                };
                (path.clone(), bytes)
            })
            .collect();
        Self { files }
    }

    pub(crate) fn changed_files(&self, after: &Self) -> Vec<PathBuf> {
        self.files
            .keys()
            .chain(after.files.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|path| self.files.get(path) != after.files.get(path))
            .collect()
    }
}

/// Every file an enabled phase of the tool may write.
pub(crate) fn snapshot_scope(job: &ToolJob, context: &ToolContext<'_>) -> BTreeSet<PathBuf> {
    let mut writes = context
        .spec
        .phases
        .iter()
        .filter(|phase| phase.enabled)
        .map(|phase| phase.writes)
        .collect::<Vec<_>>();
    writes.sort_by_key(|writes| *writes as u8);
    writes.dedup();
    writes
        .into_iter()
        .flat_map(|writes| write_scope(writes, job, context))
        .collect()
}

/// Files a command declaring `writes` may change, snapshotted around it to
/// learn what it changed. Glob- and workspace-wide writers are snapshotted
/// from [`write_root`], so a workspace-wide fixer's writes in sibling
/// packages are seen and reported too.
pub(crate) fn write_scope(
    writes: WriteBehavior,
    job: &ToolJob,
    context: &ToolContext<'_>,
) -> BTreeSet<PathBuf> {
    match writes {
        WriteBehavior::None => BTreeSet::new(),
        WriteBehavior::TargetFiles => job.files.iter().cloned().collect(),
        WriteBehavior::MatchingGlobs => collect_matching_files(
            &write_root(job, context),
            context.project_root,
            &context.spec.file_selection,
        ),
        WriteBehavior::Workspace => collect_workspace_files(&write_root(job, context)),
    }
}

/// Where a workspace-wide command can reach: the outermost directory from
/// the job's workspace up to the project root that holds the tool's
/// workspace indicator. `cargo --workspace`, Go workspaces, and npm
/// workspaces act on every member from there even when the job's workspace
/// is one member. Without an indicator, the job's workspace.
fn write_root(job: &ToolJob, context: &ToolContext<'_>) -> PathBuf {
    let Some(indicator) = &context.spec.workspace_indicator else {
        return job.workspace_dir.clone();
    };
    job.workspace_dir
        .ancestors()
        .take_while(|dir| dir.starts_with(context.project_root))
        .filter(|dir| dir.join(indicator).is_file())
        .last()
        .unwrap_or(&job.workspace_dir)
        .to_path_buf()
}

/// Files under `base` that the selection matches, with globs applied to
/// project-relative paths exactly as for candidates.
fn collect_matching_files(
    base: &Path,
    project_root: &Path,
    selection: &FileSelection,
) -> BTreeSet<PathBuf> {
    let matcher = match FileMatcher::new(selection) {
        Ok(matcher) => matcher,
        Err(_) => return BTreeSet::new(),
    };
    walk_files(base)
        .into_iter()
        .filter(|path| matcher.matches(path, project_root))
        .collect()
}

fn collect_workspace_files(base: &Path) -> BTreeSet<PathBuf> {
    walk_files(base).into_iter().collect()
}

fn walk_files(base: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(base)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".git" | "target" | "node_modules")
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .collect()
}
