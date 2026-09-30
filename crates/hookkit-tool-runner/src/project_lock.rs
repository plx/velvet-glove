//! Advisory per-project lock shared by the hooks and `velvet-glove check`.

use crate::excerpt;
use std::path::Path;

/// Advisory lock serializing the tool runs of every hook and `check`
/// invocation on one project, so two sessions' fixers never rewrite the same
/// files at once and neither session's before/after snapshots record the
/// other's writes as its own. Released on drop; best effort (no lock when the
/// lock file cannot be opened, and none off Unix).
pub(crate) struct ProjectLock {
    _file: Option<std::fs::File>,
}

pub(crate) fn lock_project(project_root: &Path) -> ProjectLock {
    let directory = std::env::temp_dir()
        .join("velvet-glove")
        .join("project-locks");
    let name = excerpt::fingerprint([project_root.to_string_lossy().as_bytes()]);
    let file = std::fs::create_dir_all(&directory).ok().and_then(|()| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(format!("{name}.lock")))
            .ok()
    });
    #[cfg(unix)]
    if let Some(file) = &file {
        use std::os::unix::io::AsRawFd;
        loop {
            // SAFETY: `flock` has no memory-safety preconditions; the
            // descriptor stays open for the lifetime of the guard.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0
                || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                break;
            }
        }
    }
    ProjectLock { _file: file }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_test_directory;
    use std::time::Duration;

    #[cfg(unix)]
    #[test]
    fn project_lock_serializes_runs_on_the_same_project_only() {
        let project = unique_test_directory("project-lock");
        let first = lock_project(&project);
        let (sender, receiver) = std::sync::mpsc::channel();
        let contender = project.clone();
        let handle = std::thread::spawn(move || {
            let _second = lock_project(&contender);
            sender.send(()).unwrap();
        });
        assert!(
            receiver.recv_timeout(Duration::from_millis(300)).is_err(),
            "a second run on the same project must wait"
        );
        drop(first);
        receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("the second run proceeds once the first finishes");
        handle.join().unwrap();

        let _held = lock_project(&project);
        let _other = lock_project(&project.join("other"));
        let _ = std::fs::remove_dir_all(project);
    }
}
