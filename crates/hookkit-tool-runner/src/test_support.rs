//! Helpers shared by the crate's unit tests.

use crate::jobs::ToolJob;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unique_test_directory(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "velvet-glove-runner-{label}-{}-{nanos}",
        std::process::id()
    ))
}

pub(crate) fn job_with_file(root: &Path, name: &str) -> ToolJob {
    ToolJob {
        workspace_dir: root.to_path_buf(),
        workspace_indicator: None,
        files: vec![root.join(name)],
    }
}
