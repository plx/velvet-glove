//! Per-tool jobs: grouping files by workspace and sizing the worker pool that runs them.

use crate::spec::{InvocationGranularity, ToolSpec, WorkspaceFallback};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct ToolContext<'a> {
    pub(crate) spec: &'a ToolSpec,
    pub(crate) project_root: &'a Path,
    pub(crate) global_diagnostics_dir: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolJob {
    pub(crate) workspace_dir: PathBuf,
    pub(crate) workspace_indicator: Option<PathBuf>,
    pub(crate) files: Vec<PathBuf>,
}

/// Group `paths` into jobs: by nearest workspace indicator when the tool has
/// one, where a file with no indicator above it is skipped or, with the
/// project-root fallback, grouped at the project root without a marker.
pub(crate) fn build_jobs(paths: &[PathBuf], project_root: &Path, spec: &ToolSpec) -> Vec<ToolJob> {
    if let Some(indicator) = &spec.workspace_indicator {
        let mut grouped = BTreeMap::<PathBuf, ToolJob>::new();
        for path in paths {
            let (workspace_dir, indicator_path) =
                match nearest_workspace_indicator(path, project_root, indicator) {
                    Some((workspace_dir, indicator_path)) => (workspace_dir, Some(indicator_path)),
                    None if spec.workspace_fallback == WorkspaceFallback::ProjectRoot => {
                        (project_root.to_path_buf(), None)
                    }
                    None => continue,
                };
            grouped
                .entry(workspace_dir.clone())
                .or_insert_with(|| ToolJob {
                    workspace_dir,
                    workspace_indicator: indicator_path,
                    files: Vec::new(),
                })
                .files
                .push(path.clone());
        }
        grouped.into_values().collect()
    } else {
        vec![ToolJob {
            workspace_dir: project_root.to_path_buf(),
            workspace_indicator: None,
            files: paths.to_vec(),
        }]
    }
}

/// Finds the nearest ancestor of `path` (up to `project_root`) whose
/// `indicator`-relative file exists, returning both that ancestor (the
/// workspace root) and the indicator file itself. `indicator` may be a
/// multi-component relative path (e.g. `sorbet/config`), so the workspace
/// root is the directory the search matched *from*, not simply the
/// indicator file's immediate parent — that would land inside `sorbet/`
/// instead of the app root for a nested indicator like that one.
fn nearest_workspace_indicator(
    path: &Path,
    project_root: &Path,
    indicator: &str,
) -> Option<(PathBuf, PathBuf)> {
    // Never look above the project root, or outside it for an outside file.
    if !path.starts_with(project_root) {
        return None;
    }
    let mut current = path.parent();
    while let Some(dir) = current {
        let candidate = dir.join(indicator);
        if candidate.is_file() {
            return Some((dir.to_path_buf(), candidate));
        }
        if dir == project_root {
            break;
        }
        current = dir.parent();
    }
    None
}

/// Upper bound for `jobs = 0` (auto).
const AUTO_JOBS_CAP: usize = 8;

/// Resolve `settings.jobs` to a worker-thread count for a batch of `job_count`
/// independent jobs.
///
/// `jobs = 0` selects "auto": the available parallelism, capped at
/// [`AUTO_JOBS_CAP`]. `jobs = n >= 1` runs up to `n` jobs concurrently. Both
/// are capped at `job_count` since extra workers would have nothing to claim.
pub(crate) fn resolve_worker_count(jobs_setting: u32, job_count: usize) -> usize {
    if job_count == 0 {
        return 0;
    }
    let requested = match jobs_setting {
        0 => std::thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .min(AUTO_JOBS_CAP),
        n => n as usize,
    };
    requested.clamp(1, job_count)
}

pub(crate) fn invocation_jobs(
    base_jobs: &[ToolJob],
    invocation: InvocationGranularity,
) -> Vec<ToolJob> {
    if invocation != InvocationGranularity::PerFile {
        return base_jobs.to_vec();
    }
    base_jobs
        .iter()
        .flat_map(|job| {
            job.files.iter().cloned().map(|file| ToolJob {
                workspace_dir: job.workspace_dir.clone(),
                workspace_indicator: job.workspace_indicator.clone(),
                files: vec![file],
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_test_directory;
    use proptest::prelude::*;

    #[test]
    fn nearest_workspace_indicator_resolves_nested_indicators_to_their_own_directory() {
        // A single-component indicator (e.g. "Cargo.toml") sitting directly in
        // the workspace root, and a nested one (e.g. "sorbet/config") one
        // level deeper, must both resolve `workspace_dir` to the same app
        // root — not to the indicator's immediate parent, which for the
        // nested case would be the "sorbet" directory itself.
        let root = unique_test_directory("nearest-workspace-indicator");
        std::fs::create_dir_all(root.join("sorbet")).unwrap();
        std::fs::write(root.join("Gemfile"), "source 'https://rubygems.org'\n").unwrap();
        std::fs::write(root.join("sorbet/config"), "--dir\n.\n").unwrap();
        let file = root.join("example.rb");
        std::fs::write(&file, "# typed: true\n").unwrap();

        let (single_dir, single_indicator) =
            nearest_workspace_indicator(&file, &root, "Gemfile").expect("Gemfile found");
        assert_eq!(single_dir, root);
        assert_eq!(single_indicator, root.join("Gemfile"));

        let (nested_dir, nested_indicator) =
            nearest_workspace_indicator(&file, &root, "sorbet/config").expect("config found");
        assert_eq!(nested_dir, root);
        assert_eq!(nested_indicator, root.join("sorbet/config"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn files_without_an_indicator_run_from_the_project_root_only_with_the_fallback() {
        let root = unique_test_directory("workspace-fallback");
        std::fs::create_dir_all(root.join("frontend/src")).unwrap();
        std::fs::write(root.join("frontend/package.json"), "{}\n").unwrap();
        let nested = root.join("frontend/src/x.js");
        let loose = root.join("README.md");
        let paths = [nested.clone(), loose.clone()];
        let spec = ToolSpec::new("tool", "Tool", "tool").with_workspace_indicator("package.json");

        let skipped = build_jobs(&paths, &root, &spec);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].workspace_dir, root.join("frontend"));
        assert_eq!(skipped[0].files, std::slice::from_ref(&nested));

        let spec = spec.with_workspace_fallback(WorkspaceFallback::ProjectRoot);
        let jobs = build_jobs(&paths, &root, &spec);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].workspace_dir, root);
        assert_eq!(jobs[0].workspace_indicator, None);
        assert_eq!(jobs[0].files, [loose]);
        assert_eq!(jobs[1].workspace_dir, root.join("frontend"));
        assert_eq!(
            jobs[1].workspace_indicator,
            Some(root.join("frontend/package.json"))
        );
        assert_eq!(jobs[1].files, [nested]);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_worker_count_honors_jobs_setting() {
        // auto (0) uses the available parallelism, capped at 8.
        let auto = std::thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .min(AUTO_JOBS_CAP);
        assert_eq!(resolve_worker_count(0, 64), auto);
        assert!(resolve_worker_count(0, 5) <= 5);
        // explicit serial.
        assert_eq!(resolve_worker_count(1, 5), 1);
        // bounded parallelism up to the requested count.
        assert_eq!(resolve_worker_count(4, 5), 4);
        // never spin up more workers than there are jobs.
        assert_eq!(resolve_worker_count(8, 5), 5);
        assert_eq!(resolve_worker_count(2, 1), 1);
        // no jobs means no workers.
        assert_eq!(resolve_worker_count(4, 0), 0);
        assert_eq!(resolve_worker_count(0, 0), 0);
    }

    proptest! {
        /// Property: worker selection is total and bounded. A non-empty batch
        /// always gets at least one worker, never more workers than jobs, and
        /// explicit settings are honored up to that cap (`0` means serial auto).
        #[test]
        fn worker_count_is_bounded(jobs_setting in any::<u32>(), job_count in any::<usize>()) {
            let actual = resolve_worker_count(jobs_setting, job_count);
            let expected = if job_count == 0 {
                0
            } else if jobs_setting == 0 {
                resolve_worker_count(0, usize::MAX).min(job_count)
            } else {
                usize::try_from(jobs_setting).unwrap_or(usize::MAX).min(job_count)
            };

            prop_assert_eq!(actual, expected);
            prop_assert!(jobs_setting != 0 || actual <= AUTO_JOBS_CAP);
            prop_assert!(actual <= job_count);
            prop_assert_eq!(actual == 0, job_count == 0);
        }
    }

    #[test]
    fn per_file_invocation_splits_jobs_without_losing_workspace_context() {
        let root = PathBuf::from("/tmp/hookkit-per-file-jobs");
        let marker = root.join("package.json");
        let base = ToolJob {
            workspace_dir: root.clone(),
            workspace_indicator: Some(marker.clone()),
            files: vec![root.join("first.json"), root.join("second.json")],
        };

        let jobs = invocation_jobs(std::slice::from_ref(&base), InvocationGranularity::PerFile);

        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].workspace_dir, root);
        assert_eq!(jobs[0].workspace_indicator.as_ref(), Some(&marker));
        assert_eq!(jobs[0].files, [base.files[0].clone()]);
        assert_eq!(jobs[1].files, [base.files[1].clone()]);
        assert_eq!(
            invocation_jobs(std::slice::from_ref(&base), InvocationGranularity::Batch)[0].files,
            base.files
        );
    }
}
