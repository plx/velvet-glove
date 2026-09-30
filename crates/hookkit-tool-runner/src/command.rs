//! Rendering, locating, and running one external command, and its phase log.

use crate::paths::path_arg;
use crate::spec::{CommandArgTemplate, ExitCodePolicy, ToolPhase, UnexpectedExitPolicy};
use crate::{ToolContext, ToolJob};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhaseStatus {
    Clean,
    Issues,
    Failure,
}

#[derive(Debug, Clone)]
pub(crate) struct PhaseLog {
    pub(crate) phase: String,
    pub(crate) command: String,
    pub(crate) program: String,
    pub(crate) arguments: Vec<String>,
    pub(crate) status: Option<i32>,
    pub(crate) classification: Option<PhaseStatus>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct RenderedCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    env: Vec<(String, String)>,
    timeout: Option<Duration>,
}

pub(crate) fn render_command(
    phase: &ToolPhase,
    job: &ToolJob,
    context: &ToolContext<'_>,
) -> RenderedCommand {
    let program = resolve_program(
        phase.program.as_deref().unwrap_or(&context.spec.executable),
        job,
        context,
    );
    let mut args = Vec::new();
    for arg in &phase.args {
        match arg {
            CommandArgTemplate::Literal(value) => args.push(value.clone()),
            CommandArgTemplate::Files => args.extend(job.files.iter().map(|path| path_arg(path))),
            CommandArgTemplate::WorkspaceFiles => {
                args.extend(job.files.iter().map(|path| {
                    path.strip_prefix(&job.workspace_dir)
                        .map(path_arg)
                        .unwrap_or_else(|_| path_arg(path))
                }));
            }
            CommandArgTemplate::Workspace => args.push(path_arg(&job.workspace_dir)),
            CommandArgTemplate::WorkspaceIndicator => {
                if let Some(path) = &job.workspace_indicator {
                    args.push(path_arg(path));
                }
            }
            CommandArgTemplate::ProjectRoot => args.push(path_arg(context.project_root)),
            CommandArgTemplate::ToolExecutable => {
                args.push(resolve_program(&context.spec.executable, job, context))
            }
            CommandArgTemplate::ExtraArgs => args.extend(phase.extra_args.iter().cloned()),
        }
    }
    RenderedCommand {
        program,
        args,
        env: context.spec.env.clone(),
        timeout: context.spec.timeout,
    }
}

/// Resolve a bare program name against the configured project-local bin
/// directories, searched from each job file's directory and the job's
/// workspace up to the project root (see [`local_program`]). Anything else is
/// left to `PATH`, or run as a path relative to the command's directory.
fn resolve_program(program: &str, job: &ToolJob, context: &ToolContext<'_>) -> String {
    let mut starts = job
        .files
        .iter()
        .filter_map(|file| file.parent())
        .collect::<Vec<_>>();
    starts.push(&job.workspace_dir);
    let mut seen = BTreeSet::new();
    starts.retain(|start| seen.insert(*start));
    local_program(
        program,
        &starts,
        context.project_root,
        &context.spec.local_bin_dirs,
    )
    .map_or_else(|| program.to_owned(), |path| path_arg(&path))
}

/// Find a bare program name (one with no path separator) in project-local
/// bin directories the way the hooks do: for each `local_bin_dirs` entry in
/// order, every directory from each of `search_from` up to `project_root`,
/// nearest first; the first executable file wins. So a package's own
/// `node_modules/.bin/eslint` beats the repository root's. `None` means the
/// program runs as given: through `PATH`, or as a path.
pub fn local_program(
    program: &str,
    search_from: &[&Path],
    project_root: &Path,
    local_bin_dirs: &[String],
) -> Option<PathBuf> {
    if Path::new(program).components().count() != 1 {
        return None;
    }
    local_bin_dirs.iter().find_map(|bin_dir| {
        search_from.iter().find_map(|start| {
            start
                .ancestors()
                .take_while(|dir| dir.starts_with(project_root))
                .map(|dir| dir.join(bin_dir).join(program))
                .find(|candidate| is_executable_file(candidate))
        })
    })
}

/// Whether `path` is a file the hooks can execute (on Unix, one with an
/// execute bit).
pub fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

struct CommandOutput {
    status: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: Option<Duration>,
}

/// Longest wait for output after the command itself exits. Only a
/// descendant still holding the pipes (a backgrounded helper or a daemon
/// that did not detach) keeps them open that long; its later output is not
/// the command's.
const LINGERING_OUTPUT_GRACE: Duration = Duration::from_secs(2);

/// Shortest wait for output already written when the command exits.
const MIN_OUTPUT_GRACE: Duration = Duration::from_millis(100);

/// Run a command with captured output, killing it (and, on Unix, its process
/// group) if it outlives `command.timeout`. Output collection is bounded
/// too: once the command exits, output is gathered until its pipes close,
/// the timeout's deadline, or [`LINGERING_OUTPUT_GRACE`], whichever is first.
fn execute_command(command: &RenderedCommand, cwd: &Path) -> std::io::Result<CommandOutput> {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Instant;

    /// Output read so far from one pipe, and a signal once the pipe closes.
    struct Drain {
        buffer: Arc<Mutex<Vec<u8>>>,
        closed: mpsc::Receiver<()>,
    }

    fn drain(pipe: Option<impl Read + Send + 'static>) -> Drain {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let (sender, closed) = mpsc::channel();
        if let Some(mut pipe) = pipe {
            let shared = Arc::clone(&buffer);
            std::thread::spawn(move || {
                let mut chunk = [0u8; 8192];
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(read) => shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .extend_from_slice(&chunk[..read]),
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
                let _ = sender.send(());
            });
        }
        Drain { buffer, closed }
    }

    /// Everything read by `until`, even if the pipe is still open.
    fn collect(drain: Drain, until: Instant) -> Vec<u8> {
        let _ = drain
            .closed
            .recv_timeout(until.saturating_duration_since(Instant::now()));
        std::mem::take(
            &mut *drain
                .buffer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    let mut process = Command::new(&command.program);
    process
        .args(&command.args)
        .envs(command.env.iter().map(|(key, value)| (key, value)))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    let mut child = process.spawn()?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let deadline = command.timeout.map(|timeout| Instant::now() + timeout);
    let mut timed_out = None;
    let status = match (command.timeout, deadline) {
        (Some(timeout), Some(deadline)) => {
            let mut delay = Duration::from_millis(1);
            loop {
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                let now = Instant::now();
                if now >= deadline {
                    kill_process_tree(&mut child);
                    timed_out = Some(timeout);
                    break child.wait()?;
                }
                std::thread::sleep(delay.min(deadline - now));
                delay = (delay * 2).min(Duration::from_millis(20));
            }
        }
        _ => child.wait()?,
    };
    // A descendant may still hold the pipes after the command exits (or is
    // killed); take whatever output arrives promptly rather than waiting.
    let now = Instant::now();
    let grace = match (timed_out, deadline) {
        (Some(_), _) => Duration::from_secs(1),
        (None, Some(deadline)) => deadline
            .saturating_duration_since(now)
            .min(LINGERING_OUTPUT_GRACE),
        (None, None) => LINGERING_OUTPUT_GRACE,
    }
    .max(MIN_OUTPUT_GRACE);
    let until = now + grace;
    Ok(CommandOutput {
        status: status.code(),
        stdout: collect(stdout, until),
        stderr: collect(stderr, until),
        timed_out,
    })
}

fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = libc::pid_t::try_from(child.id()) {
            // SAFETY: `kill` has no memory-safety preconditions; the child was
            // spawned as the leader of its own process group.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
}

pub(crate) fn run_phase_command(
    phase: &ToolPhase,
    command: &RenderedCommand,
    cwd: &Path,
) -> PhaseLog {
    match execute_command(command, cwd) {
        Ok(output) if output.timed_out.is_some() => PhaseLog {
            phase: phase.id.clone(),
            command: display_command(&command.program, &command.args),
            program: command.program.clone(),
            arguments: command.args.clone(),
            status: output.status,
            classification: None,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            error: Some(format!(
                "timed out after {}s and was killed (settings.commandTimeoutSeconds / tool timeoutSeconds)",
                output.timed_out.unwrap_or_default().as_secs()
            )),
        },
        Ok(output) => {
            let status = output.status;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let mut classification = status.map(|code| classify_exit_code(&phase.exit_codes, code));
            if phase.issues_on_stdout
                && classification == Some(PhaseStatus::Clean)
                && !stdout.trim().is_empty()
            {
                classification = Some(PhaseStatus::Issues);
            }
            PhaseLog {
                phase: phase.id.clone(),
                command: display_command(&command.program, &command.args),
                program: command.program.clone(),
                arguments: command.args.clone(),
                status,
                classification,
                stdout,
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                error: None,
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => PhaseLog {
            phase: phase.id.clone(),
            command: display_command(&command.program, &command.args),
            program: command.program.clone(),
            arguments: command.args.clone(),
            status: None,
            classification: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some("not found".to_string()),
        },
        Err(e) => PhaseLog {
            phase: phase.id.clone(),
            command: display_command(&command.program, &command.args),
            program: command.program.clone(),
            arguments: command.args.clone(),
            status: None,
            classification: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(e.to_string()),
        },
    }
}

fn classify_exit_code(policy: &ExitCodePolicy, code: i32) -> PhaseStatus {
    if policy.clean.contains(&code) {
        PhaseStatus::Clean
    } else if policy.issues.contains(&code) {
        PhaseStatus::Issues
    } else if policy.failure.contains(&code) {
        PhaseStatus::Failure
    } else {
        match policy.unexpected {
            UnexpectedExitPolicy::Failure => PhaseStatus::Failure,
            UnexpectedExitPolicy::Issues => PhaseStatus::Issues,
        }
    }
}

pub(crate) fn format_logs(logs: &[PhaseLog]) -> String {
    let mut out = String::new();
    for log in logs {
        out.push_str(&format!(
            "[{phase}] command: {command}\nstatus: {status:?}\nclassification: {classification:?}\n",
            phase = log.phase,
            command = log.command,
            status = log.status,
            classification = log.classification
        ));
        if let Some(err) = &log.error {
            out.push_str(&format!("error: {err}\n"));
        }
        if !log.stdout.trim().is_empty() {
            out.push_str("stdout:\n");
            out.push_str(&log.stdout);
            if !log.stdout.ends_with('\n') {
                out.push('\n');
            }
        }
        if !log.stderr.trim().is_empty() {
            out.push_str("stderr:\n");
            out.push_str(&log.stderr);
            if !log.stderr.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push('\n');
    }
    out
}

fn display_command(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{PhaseMode, ToolSpec};
    use crate::test_support::{job_with_file, unique_test_directory};
    use hookkit_pkl_config::schema as pkl;
    use proptest::prelude::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    proptest! {
        /// Property: overlapping exit-code policy lists have a documented
        /// precedence (clean, then issues, then failure), and unlisted values
        /// use exactly the configured fallback.
        #[test]
        fn exit_code_classification_has_stable_precedence(
            clean in prop::collection::vec(any::<i32>(), 0..30),
            issues in prop::collection::vec(any::<i32>(), 0..30),
            failure in prop::collection::vec(any::<i32>(), 0..30),
            code in any::<i32>(),
            unexpected_issues in any::<bool>(),
        ) {
            let unexpected = if unexpected_issues {
                UnexpectedExitPolicy::Issues
            } else {
                UnexpectedExitPolicy::Failure
            };
            let policy = ExitCodePolicy {
                clean: clean.clone(),
                issues: issues.clone(),
                failure: failure.clone(),
                unexpected,
            };
            let expected = if clean.contains(&code) {
                PhaseStatus::Clean
            } else if issues.contains(&code) {
                PhaseStatus::Issues
            } else if failure.contains(&code) {
                PhaseStatus::Failure
            } else if unexpected_issues {
                PhaseStatus::Issues
            } else {
                PhaseStatus::Failure
            };

            prop_assert_eq!(classify_exit_code(&policy, code), expected);
        }
    }

    #[cfg(unix)]
    #[test]
    fn local_bin_dirs_resolve_nearest_first_before_path() {
        let root = unique_test_directory("local-bin");
        let workspace = root.join("packages/web");
        for dir in [
            root.join("node_modules/.bin"),
            workspace.join("node_modules/.bin"),
            workspace.join(".venv/bin"),
        ] {
            std::fs::create_dir_all(&dir).unwrap();
        }
        let write_tool = |path: PathBuf, mode: u32| {
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        };
        let root_eslint = write_tool(root.join("node_modules/.bin/eslint"), 0o755);
        let nested_eslint = write_tool(workspace.join("node_modules/.bin/eslint"), 0o755);
        let venv_eslint = write_tool(workspace.join(".venv/bin/eslint"), 0o755);
        write_tool(root.join("node_modules/.bin/not-executable"), 0o644);
        let ruff = write_tool(workspace.join(".venv/bin/ruff"), 0o755);

        let mut spec = ToolSpec::new("eslint", "ESLint", "eslint");
        spec.local_bin_dirs = pkl::default_local_bin_dirs();
        let context = ToolContext {
            spec: &spec,
            project_root: &root,
            global_diagnostics_dir: None,
        };
        let job = job_with_file(&workspace, "a.ts");
        let root_job = job_with_file(&root, "a.ts");
        let resolve = |program: &str, job: &ToolJob| resolve_program(program, job, &context);

        assert_eq!(resolve("eslint", &job), path_arg(&nested_eslint));
        assert_eq!(resolve("eslint", &root_job), path_arg(&root_eslint));
        assert_eq!(resolve("ruff", &job), path_arg(&ruff));
        assert_eq!(resolve("not-executable", &job), "not-executable");
        assert_eq!(resolve("absent", &job), "absent");
        assert_eq!(resolve("/usr/bin/env", &job), "/usr/bin/env");
        assert_ne!(resolve("eslint", &job), path_arg(&venv_eslint));

        // Without a workspace indicator the job runs from the project root,
        // but a package's own install still wins for its files.
        let package_job = ToolJob {
            workspace_dir: root.clone(),
            workspace_indicator: None,
            files: vec![workspace.join("src/a.ts")],
        };
        assert_eq!(resolve("eslint", &package_job), path_arg(&nested_eslint));
        // A policy above the repository still finds the repository's tools.
        let outer = local_program(
            "eslint",
            &[workspace.join("src").as_path()],
            root.parent().unwrap(),
            &pkl::default_local_bin_dirs(),
        );
        assert_eq!(outer, Some(nested_eslint.clone()));

        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn commands_receive_tool_env_and_are_killed_at_the_timeout() {
        let root = std::env::temp_dir();
        let phase = ToolPhase::new("verify", PhaseMode::Verify);
        let command = |script: &str, timeout_ms: u64| RenderedCommand {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: vec![("VELVET_GLOVE_PROBE".into(), "probe-value".into())],
            timeout: Some(Duration::from_millis(timeout_ms)),
        };

        let log = run_phase_command(
            &phase,
            &command("printf %s \"$VELVET_GLOVE_PROBE\"", 5_000),
            &root,
        );
        assert_eq!(log.stdout, "probe-value");
        assert_eq!(log.classification, Some(PhaseStatus::Clean));

        let started = std::time::Instant::now();
        let log = run_phase_command(
            &phase,
            &command("printf started; sleep 30 & wait", 200),
            &root,
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timeout must kill the command"
        );
        assert_eq!(log.classification, None);
        assert!(
            log.error.as_deref().unwrap().contains("timed out after"),
            "{log:?}"
        );
        assert_eq!(log.stdout, "started");

        // A descendant that outlives the command and keeps its output pipes
        // open must not hold the hook past the timeout (or, without one,
        // past a short grace period).
        for timeout in [Some(Duration::from_millis(1_500)), None] {
            let started = std::time::Instant::now();
            let log = run_phase_command(
                &phase,
                &RenderedCommand {
                    timeout,
                    ..command("(sleep 8) & echo checked; exit 0", 0)
                },
                &root,
            );
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "lingering descendant held the pipes for {:?} ({timeout:?})",
                started.elapsed()
            );
            assert_eq!(log.stdout, "checked\n");
            assert_eq!(log.classification, Some(PhaseStatus::Clean));
        }
    }
}
