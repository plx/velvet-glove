//! Path helpers: normalization, display, command-argument forms, and the default state root.

use hookkit_session_state::StateRoot;
use std::path::{Component, Path, PathBuf};

/// Absolute prefixes that excerpts rewrite to project-relative paths: the
/// canonical project root and the spelling the harness or config used.
pub(crate) fn display_roots(canonical: &Path, spelled: &Path) -> Vec<PathBuf> {
    let mut roots = vec![canonical.to_path_buf()];
    if spelled != canonical {
        roots.push(spelled.to_path_buf());
    }
    roots
}

pub(crate) fn path_arg(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

pub(crate) fn absolute_from(path: &Path, base: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

pub(crate) fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn rel_display(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .map(slash_path)
        .unwrap_or_else(|_| slash_path(path))
}

pub(crate) fn state_root(override_dir: Option<&Path>) -> StateRoot {
    StateRoot::new(override_dir.map_or_else(
        || std::env::temp_dir().join("velvet-glove").join("state"),
        Path::to_path_buf,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property: lexical normalization for not-yet-created output paths is
        /// idempotent, absolute, and cannot retain traversal above root.
        #[test]
        fn non_existing_output_path_normalization_is_stable(
            segments in prop::collection::vec(prop_oneof![Just(".".to_owned()), Just("..".to_owned()), "[a-z]{1,8}"], 0..30),
        ) {
            let path = PathBuf::from(format!(
                "/hookkit-property-path-that-does-not-exist/{}/{}",
                std::process::id(),
                segments.join("/")
            ));
            let once = normalize_path(&path);
            let twice = normalize_path(&once);

            prop_assert_eq!(&once, &twice);
            prop_assert!(once.is_absolute());
            let contains_traversal = once.components().any(|component| {
                matches!(component, Component::CurDir | Component::ParentDir)
            });
            prop_assert!(!contains_traversal);
        }
    }
}
