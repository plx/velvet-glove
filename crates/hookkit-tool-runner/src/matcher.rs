//! Glob-based file selection shared by the hooks and the CLI.

use crate::errors::invalid_data;
use crate::paths::slash_path;
use crate::spec::FileSelection;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;

/// File selection shared by the hooks and the CLI: globs match
/// project-relative, `/`-separated paths; an empty include list selects every
/// file; excludes always win.
pub struct FileMatcher {
    include: GlobSet,
    exclude: GlobSet,
    include_all: bool,
}

impl FileMatcher {
    /// Compile a selection; an invalid glob is an error.
    pub fn new(config: &FileSelection) -> hookkit_core::Result<Self> {
        Ok(Self {
            include: build_globset(&config.include)?,
            exclude: build_globset(&config.exclude)?,
            include_all: config.include.is_empty(),
        })
    }

    /// Whether `absolute_path` is selected. Globs match its path relative to
    /// `project_root`, so unanchored excludes such as `**/target/**` never
    /// fire on the directories *containing* the project. A path outside the
    /// project root is never selected: a project's policy applies only to
    /// its own files.
    pub fn matches(&self, absolute_path: &Path, project_root: &Path) -> bool {
        absolute_path
            .strip_prefix(project_root)
            .is_ok_and(|relative| self.matches_relative(&slash_path(relative)))
    }

    /// Whether a project-relative, `/`-separated path is selected.
    pub fn matches_relative(&self, relative: &str) -> bool {
        (self.include_all || self.include.is_match(relative)) && !self.exclude.is_match(relative)
    }
}

fn build_globset(patterns: &[String]) -> hookkit_core::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern)
            .map_err(|e| invalid_data(format!("invalid file glob `{pattern}`: {e}")))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|e| invalid_data(format!("invalid file glob set: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hookkit_pkl_config::schema as pkl;

    #[test]
    fn default_excludes_are_unanchored_and_ignore_directories_above_the_project() {
        let matcher = FileMatcher::new(&FileSelection {
            include: vec!["**/*.py".into()],
            exclude: pkl::default_excludes(),
        })
        .unwrap();
        let root = Path::new("/home/user/target/project");
        assert!(matcher.matches(&root.join("src/a.py"), root));
        assert!(
            !matcher.matches(Path::new("/home/user/scratch/plan.py"), root),
            "a file outside the project is never selected"
        );
        for excluded in [
            "node_modules/x.py",
            "web/node_modules/pkg/x.py",
            "svc/.venv/lib/x.py",
            "pkg/__pycache__/x.py",
            "crates/a/target/x.py",
            ".git/hooks/x.py",
            ".ruff_cache/0.16.6/x.py",
            "svc/.tox/py312/lib/x.py",
            "app/.next/server/x.py",
        ] {
            assert!(!matcher.matches(&root.join(excluded), root), "{excluded}");
        }
    }
}
