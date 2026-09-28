//! Version-control helpers shared by the deferred and immediate runners.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Upper bound on paths retried one at a time after a batch query fails.
const MAX_INDIVIDUAL_QUERIES: usize = 64;

/// Return the subset of `paths` that Git ignores in the work trees under
/// `root`.
///
/// Only paths under `root` are queried. Each path is asked of the repository
/// that owns it (the nearest directory holding `.git`, so submodules and
/// nested repositories answer for their own files), one `git check-ignore`
/// process per repository. When a batch query fails (for example, a path
/// beyond a symbolic link), its paths are retried individually so one bad
/// path cannot disable the filter for the others. Tracked files are never
/// reported as ignored. Outside a Git work tree, or when Git is unavailable
/// or fails, nothing is considered ignored.
pub(crate) fn git_ignored_paths(root: &Path, paths: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut repositories = BTreeMap::<PathBuf, Vec<PathBuf>>::new();
    for path in paths {
        if path
            .strip_prefix(root)
            .is_ok_and(|relative| !relative.as_os_str().is_empty())
        {
            repositories
                .entry(owning_repository(path, root))
                .or_default()
                .push(path.clone());
        }
    }
    let mut ignored = BTreeSet::new();
    let mut retries = 0usize;
    for (repository, paths) in repositories {
        match check_ignore(&repository, &paths) {
            Some(found) => ignored.extend(found),
            None => {
                for path in paths {
                    if retries == MAX_INDIVIDUAL_QUERIES {
                        break;
                    }
                    retries += 1;
                    ignored.extend(
                        check_ignore(&repository, std::slice::from_ref(&path)).unwrap_or_default(),
                    );
                }
            }
        }
    }
    ignored
}

/// The nearest directory from `path`'s parent up to `root` holding `.git`
/// (a directory, or a file for submodules and linked worktrees), else `root`.
fn owning_repository(path: &Path, root: &Path) -> PathBuf {
    path.ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(root))
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(root)
        .to_path_buf()
}

/// Ask Git in `repository` which of `paths` it ignores; `None` when the query
/// itself fails.
fn check_ignore(repository: &Path, paths: &[PathBuf]) -> Option<BTreeSet<PathBuf>> {
    let relative = paths
        .iter()
        .filter_map(|path| {
            let relative = path.strip_prefix(repository).ok()?;
            (!relative.as_os_str().is_empty()).then(|| (relative.to_path_buf(), path.clone()))
        })
        .collect::<Vec<_>>();
    if relative.is_empty() {
        return Some(BTreeSet::new());
    }
    let mut child = Command::new("git")
        .args(["check-ignore", "--stdin", "-z"])
        .current_dir(repository)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
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
    let output = output.ok()?;
    // 0: at least one path ignored; 1: none ignored; anything else is fatal
    // (for example, not a Git work tree, or a pathspec in a submodule).
    match output.status.code() {
        Some(0) => {}
        Some(1) => return Some(BTreeSet::new()),
        _ => return None,
    }
    let ignored = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| PathBuf::from(String::from_utf8_lossy(entry).into_owned()))
        .collect::<BTreeSet<_>>();
    Some(
        relative
            .into_iter()
            .filter(|(relative, _)| ignored.contains(relative))
            .map(|(_, absolute)| absolute)
            .collect(),
    )
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
    fn a_submodule_path_does_not_disable_filtering_for_the_superproject() {
        let root = temp_root("submodule");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        if !git(&root, &["init", "-q"]) || !git(&sub, &["init", "-q"]) {
            eprintln!("skipping test: git unavailable");
            return;
        }
        std::fs::write(root.join(".gitignore"), "dist/\n").unwrap();
        std::fs::write(sub.join(".gitignore"), "*.tmp\n").unwrap();
        std::fs::create_dir_all(root.join("dist")).unwrap();
        let built = root.join("dist/out.py");
        let nested_source = sub.join("f.py");
        let nested_scratch = sub.join("scratch.tmp");
        for path in [&built, &nested_source, &nested_scratch] {
            std::fs::write(path, "x\n").unwrap();
        }
        // Make `sub` a gitlink in the superproject, as a submodule would be.
        assert!(git(&sub, &["add", "f.py", ".gitignore"]));
        let committed = Command::new("git")
            .arg("-C")
            .arg(&sub)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "init",
            ])
            .output()
            .is_ok_and(|output| output.status.success());
        assert!(committed);
        assert!(git(&root, &["add", "sub"]));

        let ignored = git_ignored_paths(
            &root,
            &[built.clone(), nested_source, nested_scratch.clone()],
        );

        assert_eq!(ignored, BTreeSet::from([built, nested_scratch]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_beyond_a_symlink_does_not_disable_filtering_for_the_batch() {
        let root = temp_root("symlink");
        if !git(&root, &["init", "-q"]) {
            eprintln!("skipping test: git unavailable");
            return;
        }
        std::fs::write(root.join(".gitignore"), "dist/\n").unwrap();
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::create_dir_all(root.join("real")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        let built = root.join("dist/out.py");
        std::fs::write(&built, "x\n").unwrap();
        std::fs::write(root.join("real/a.py"), "x\n").unwrap();

        let ignored = git_ignored_paths(&root, &[built.clone(), root.join("link/a.py")]);

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
