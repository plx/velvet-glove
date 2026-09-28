//! Version-control helpers shared by the deferred and immediate runners.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Return the subset of `paths` that Git ignores in the work tree containing
/// `root`.
///
/// Only paths under `root` are queried, in one `git check-ignore` process.
/// Tracked files are never reported as ignored. Outside a Git work tree, or
/// when Git is unavailable or fails, nothing is considered ignored.
pub(crate) fn git_ignored_paths(root: &Path, paths: &[PathBuf]) -> BTreeSet<PathBuf> {
    let relative = paths
        .iter()
        .filter_map(|path| {
            let relative = path.strip_prefix(root).ok()?;
            (!relative.as_os_str().is_empty()).then(|| (relative.to_path_buf(), path.clone()))
        })
        .collect::<Vec<_>>();
    if relative.is_empty() {
        return BTreeSet::new();
    }
    let Ok(mut child) = Command::new("git")
        .args(["check-ignore", "--stdin", "-z"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return BTreeSet::new();
    };
    let mut input = Vec::new();
    for (path, _) in &relative {
        input.extend_from_slice(path.to_string_lossy().as_bytes());
        input.push(0);
    }
    // Write on a separate thread so a large result cannot fill the stdout
    // pipe while this thread is still blocked writing stdin.
    let stdin = child.stdin.take();
    let writer = std::thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(&input);
        }
    });
    let output = child.wait_with_output();
    let _ = writer.join();
    let Ok(output) = output else {
        return BTreeSet::new();
    };
    // 0: at least one path ignored; 1: none ignored; anything else is fatal
    // (for example, not a Git work tree).
    if output.status.code() != Some(0) {
        return BTreeSet::new();
    }
    let ignored = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| PathBuf::from(String::from_utf8_lossy(entry).into_owned()))
        .collect::<BTreeSet<_>>();
    relative
        .into_iter()
        .filter(|(relative, _)| ignored.contains(relative))
        .map(|(_, absolute)| absolute)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .is_ok_and(|output| output.status.success())
    }

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hookkit-vcs-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::canonicalize(root).unwrap()
    }

    #[test]
    fn reports_only_ignored_untracked_paths_under_root() {
        let root = temp_root("ignored");
        if !git(&root, &["init", "-q"]) {
            eprintln!("skipping test: git unavailable");
            return;
        }
        std::fs::write(root.join(".gitignore"), "dist/\n*.log\n").unwrap();
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let built = root.join("dist/bundle.js");
        let source = root.join("src/main.js");
        let tracked_log = root.join("kept.log");
        for path in [&built, &source, &tracked_log] {
            std::fs::write(path, "x\n").unwrap();
        }
        assert!(git(&root, &["add", "-f", "kept.log"]));
        let outside = std::env::temp_dir().join("outside.js");

        let ignored = git_ignored_paths(
            &root,
            &[built.clone(), source, tracked_log, outside.clone()],
        );

        assert_eq!(ignored, BTreeSet::from([built]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn outside_a_work_tree_nothing_is_ignored() {
        let root = temp_root("plain");
        let file = root.join("a.py");
        std::fs::write(&file, "x\n").unwrap();
        std::fs::write(root.join(".gitignore"), "*.py\n").unwrap();
        // The temp directory itself may live inside some unrelated work tree
        // on developer machines; only assert when Git agrees it is not one.
        if git(&root, &["rev-parse", "--is-inside-work-tree"]) {
            return;
        }
        assert!(git_ignored_paths(&root, &[file]).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
