//! Tripwires enforcing the fixture-corpus budgets from
//! `docs/validation-architecture.md`.
//!
//! These limits are intentional. If one fails on work you are doing, the
//! default assumption is that the work is over budget — not that the budget
//! is wrong. Do NOT raise a limit or restructure fixtures to evade a check:
//! stop and open an issue for human review instead. Changes to this file
//! require the `guardrail-change` label (human sign-off) to pass CI.

use std::fs;
use std::path::{Path, PathBuf};

const DOC: &str = "docs/validation-architecture.md";

/// Pre-reboot baseline maxima were 4 cases per tool, 7 files per case,
/// ~28 KiB per case, and a ~5 KiB largest file. The caps below leave
/// generous headroom while staying categorically below the v1 explosion.
const MAX_CASES_PER_TOOL: usize = 12;
const MAX_FILES_PER_CASE: usize = 24;
const MAX_BYTES_PER_CASE: u64 = 128 * 1024;
const MAX_BYTES_PER_FILE: u64 = 64 * 1024;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tool-fixtures")
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read fixture dir") {
        let path = entry.expect("dir entry").path();
        let meta = fs::symlink_metadata(&path).expect("stat fixture entry");
        assert!(
            !meta.file_type().is_symlink(),
            "{} is a symlink; fixtures must contain only regular files and \
             directories. See {DOC}.",
            path.display(),
        );
        if meta.is_dir() {
            walk_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn fixture_corpus_fits_the_budgets() {
    let root = fixtures_root();
    let mut tools = 0usize;
    for entry in fs::read_dir(&root).expect("read tool-fixtures root") {
        let tool_dir = entry.expect("dir entry").path();
        if !tool_dir.is_dir() {
            continue; // e.g. README.md
        }
        tools += 1;
        let case_dirs: Vec<PathBuf> = fs::read_dir(&tool_dir)
            .expect("read tool dir")
            .map(|e| e.expect("dir entry").path())
            .filter(|p| p.is_dir())
            .collect();
        assert!(
            case_dirs.len() <= MAX_CASES_PER_TOOL,
            "{} has {} cases (budget: {MAX_CASES_PER_TOOL}). A tool needing \
             more cases than this is being validated beyond its behavioral \
             surface. See {DOC}; do not raise this budget — open an issue \
             for human review instead.",
            tool_dir.display(),
            case_dirs.len(),
        );
        for case_dir in case_dirs {
            let mut files = Vec::new();
            walk_files(&case_dir, &mut files);
            assert!(
                files.len() <= MAX_FILES_PER_CASE,
                "{} has {} files (budget: {MAX_FILES_PER_CASE}). See {DOC}; \
                 do not raise this budget — open an issue for human review \
                 instead.",
                case_dir.display(),
                files.len(),
            );
            let mut total = 0u64;
            for file in &files {
                let size = fs::metadata(file).expect("stat fixture file").len();
                assert!(
                    size <= MAX_BYTES_PER_FILE,
                    "{} is {size} bytes (budget: {MAX_BYTES_PER_FILE}). \
                     Fixtures are minimal examples, not vendored projects. \
                     See {DOC}; do not raise this budget — open an issue for \
                     human review instead.",
                    file.display(),
                );
                total += size;
            }
            assert!(
                total <= MAX_BYTES_PER_CASE,
                "{} totals {total} bytes (budget: {MAX_BYTES_PER_CASE}). See \
                 {DOC}; do not raise this budget — open an issue for human \
                 review instead.",
                case_dir.display(),
            );
        }
    }
    assert!(tools > 0, "no fixture tool directories found");
}
