//! Config discovery rules.
//!
//! Discovery layers (each merged over the previous, last-write-wins):
//!
//! 1. **Home/global** — legacy `~/.agent-hook-kit/post-tool-use.pkl`, then
//!    canonical `~/.velvet-glove/post-tool-use.pkl`
//! 2. **Project chain** — walking up from `cwd`, legacy files root → leaf,
//!    then canonical `.velvet-glove/post-tool-use.pkl` files root → leaf
//! 3. **Local chain** — walking up from `cwd`, legacy files root → leaf,
//!    then canonical `.velvet-glove/post-tool-use.local.pkl` files root → leaf
//!
//! Canonical files are evaluated later and therefore override their legacy
//! peers within the same layer; ordinary home → project → local precedence is
//! preserved across layers.
//!
//! When `--config PATH` is passed, the entire chain is bypassed and only that
//! file is used.

use std::path::{Path, PathBuf};

/// Filename used for inherited home and project configuration.
pub const PROJECT_CONFIG_NAME: &str = "post-tool-use.pkl";
/// Filename used for local, normally uncommitted configuration.
pub const LOCAL_CONFIG_NAME: &str = "post-tool-use.local.pkl";
/// Directory searched at the home and project-ancestor levels.
pub const CONFIG_DIR: &str = ".velvet-glove";
/// Former HookKit directory read at lower precedence during migration.
pub const LEGACY_CONFIG_DIR: &str = ".agent-hook-kit";

/// One step in the config discovery chain.
#[derive(Debug, Clone)]
pub struct DiscoveredConfig {
    /// Existing configuration file.
    pub path: PathBuf,
    /// Discovery layer that supplied the file.
    pub kind: DiscoveredKind,
}

/// Layer in the configuration discovery and precedence chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveredKind {
    /// User-wide configuration under the home directory.
    Home,
    /// Inherited project configuration.
    Project,
    /// Local project configuration, merged after every project file.
    Local,
}

/// Discover the configs that should be loaded, in merge order (earliest first).
pub fn discover(cwd: &Path) -> Vec<DiscoveredConfig> {
    let mut chain = Vec::new();

    for home in [legacy_home_config_path(), home_config_path()]
        .into_iter()
        .flatten()
    {
        if home.is_file() {
            chain.push(DiscoveredConfig {
                path: home,
                kind: DiscoveredKind::Home,
            });
        }
    }

    // Walk ancestors root-first so child configs override parent configs.
    let mut ancestors: Vec<PathBuf> = cwd.ancestors().map(Path::to_path_buf).collect();
    ancestors.reverse();

    for config_dir in [LEGACY_CONFIG_DIR, CONFIG_DIR] {
        for ancestor in &ancestors {
            let candidate = ancestor.join(config_dir).join(PROJECT_CONFIG_NAME);
            if candidate.is_file() {
                chain.push(DiscoveredConfig {
                    path: candidate,
                    kind: DiscoveredKind::Project,
                });
            }
        }
    }

    for config_dir in [LEGACY_CONFIG_DIR, CONFIG_DIR] {
        for ancestor in &ancestors {
            let candidate = ancestor.join(config_dir).join(LOCAL_CONFIG_NAME);
            if candidate.is_file() {
                chain.push(DiscoveredConfig {
                    path: candidate,
                    kind: DiscoveredKind::Local,
                });
            }
        }
    }

    chain
}

/// Returns the conventional home configuration path, if a home directory exists.
pub fn home_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(CONFIG_DIR).join(PROJECT_CONFIG_NAME))
}

fn legacy_home_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(LEGACY_CONFIG_DIR).join(PROJECT_CONFIG_NAME))
}

/// Project root associated with a discovered config (or the cwd as fallback).
///
/// Home configs and `--config PATH` files do not imply a project root; callers
/// should use `cwd` in those cases.
pub fn project_root_for(config: &DiscoveredConfig, cwd: &Path) -> PathBuf {
    match config.kind {
        DiscoveredKind::Home => cwd.to_path_buf(),
        DiscoveredKind::Project | DiscoveredKind::Local => config
            .path
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| cwd.to_path_buf()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velvet-glove-pkl-discovery-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_config(dir: &Path, name: &str) -> PathBuf {
        let config_dir = dir.join(CONFIG_DIR);
        std::fs::create_dir_all(&config_dir).unwrap();
        let path = config_dir.join(name);
        std::fs::write(&path, "// stub").unwrap();
        path
    }

    fn write_config_in(dir: &Path, config_dir: &str, name: &str) -> PathBuf {
        let config_dir = dir.join(config_dir);
        std::fs::create_dir_all(&config_dir).unwrap();
        let path = config_dir.join(name);
        std::fs::write(&path, "// stub").unwrap();
        path
    }

    #[test]
    fn discovers_project_then_local_in_root_first_order() {
        let root = temp_dir("project-local");
        let nested = root.join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        let root_project = write_config(&root, PROJECT_CONFIG_NAME);
        let inner_project = write_config(&nested, PROJECT_CONFIG_NAME);
        let root_local = write_config(&root, LOCAL_CONFIG_NAME);

        let chain = discover(&nested);
        let kinds: Vec<_> = chain.iter().map(|c| (c.kind, c.path.clone())).collect();
        assert!(
            kinds.contains(&(DiscoveredKind::Project, root_project)),
            "root project missing: {kinds:?}"
        );
        assert!(
            kinds.contains(&(DiscoveredKind::Project, inner_project)),
            "nested project missing: {kinds:?}"
        );
        assert!(
            kinds.contains(&(DiscoveredKind::Local, root_local)),
            "root local missing: {kinds:?}"
        );

        // Projects should appear before locals, and within each kind ancestors
        // should appear before descendants.
        let project_positions: Vec<usize> = chain
            .iter()
            .enumerate()
            .filter_map(|(i, c)| (c.kind == DiscoveredKind::Project).then_some(i))
            .collect();
        let local_positions: Vec<usize> = chain
            .iter()
            .enumerate()
            .filter_map(|(i, c)| (c.kind == DiscoveredKind::Local).then_some(i))
            .collect();
        if let (Some(&p_max), Some(&l_min)) =
            (project_positions.iter().max(), local_positions.iter().min())
        {
            assert!(p_max < l_min, "project should come before local: {chain:?}");
        }

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn canonical_namespace_has_higher_precedence_than_legacy_namespace() {
        let root = temp_dir("canonical-after-legacy");
        let legacy_project = write_config_in(&root, LEGACY_CONFIG_DIR, PROJECT_CONFIG_NAME);
        let canonical_project = write_config_in(&root, CONFIG_DIR, PROJECT_CONFIG_NAME);
        let legacy_local = write_config_in(&root, LEGACY_CONFIG_DIR, LOCAL_CONFIG_NAME);
        let canonical_local = write_config_in(&root, CONFIG_DIR, LOCAL_CONFIG_NAME);

        let paths: Vec<_> = discover(&root)
            .into_iter()
            .map(|entry| entry.path)
            .filter(|path| path.starts_with(&root))
            .collect();

        assert_eq!(
            paths,
            vec![
                legacy_project,
                canonical_project,
                legacy_local,
                canonical_local,
            ]
        );

        std::fs::remove_dir_all(&root).ok();
    }
}
