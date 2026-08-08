use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[derive(Debug)]
pub struct BoundedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug)]
pub enum BoundedCommandError {
    Setup(String),
    Spawn(String),
    Stdin(String),
    Wait(String),
    Capture(String),
    Timeout {
        duration: Duration,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
}

impl fmt::Display for BoundedCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Setup(message)
            | Self::Spawn(message)
            | Self::Stdin(message)
            | Self::Wait(message)
            | Self::Capture(message) => formatter.write_str(message),
            Self::Timeout {
                duration,
                stdout,
                stderr,
            } => write!(
                formatter,
                "timed out after {:.3}s\nstdout:\n{}\nstderr:\n{}",
                duration.as_secs_f64(),
                String::from_utf8_lossy(stdout),
                String::from_utf8_lossy(stderr),
            ),
        }
    }
}

pub fn run_with_timeout(
    command: &mut Command,
    stdin: &[u8],
    timeout: Duration,
    capture_dir: &Path,
) -> Result<BoundedOutput, BoundedCommandError> {
    std::fs::create_dir_all(capture_dir).map_err(|error| {
        BoundedCommandError::Setup(format!("create capture directory {capture_dir:?}: {error}"))
    })?;
    let stdout_path = capture_dir.join("stdout");
    let stderr_path = capture_dir.join("stderr");
    let stdout = File::create(&stdout_path).map_err(|error| {
        BoundedCommandError::Setup(format!("create stdout capture {stdout_path:?}: {error}"))
    })?;
    let stderr = File::create(&stderr_path).map_err(|error| {
        BoundedCommandError::Setup(format!("create stderr capture {stderr_path:?}: {error}"))
    })?;

    command
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command.spawn().map_err(|error| {
        BoundedCommandError::Spawn(format!("spawn {:?}: {error}", command.get_program()))
    })?;
    if let Some(mut child_stdin) = child.stdin.take() {
        if let Err(error) = child_stdin.write_all(stdin) {
            terminate(&mut child);
            let _ = child.wait();
            return Err(BoundedCommandError::Stdin(format!(
                "write stdin for {:?}: {error}",
                command.get_program()
            )));
        }
    }

    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            terminate(&mut child);
            let _ = child.wait();
            let stdout = read_capture(&stdout_path)?;
            let stderr = read_capture(&stderr_path)?;
            return Err(BoundedCommandError::Timeout {
                duration: timeout,
                stdout,
                stderr,
            });
        }
        Err(error) => {
            terminate(&mut child);
            let _ = child.wait();
            return Err(BoundedCommandError::Wait(format!(
                "wait for {:?}: {error}",
                command.get_program()
            )));
        }
    };

    Ok(BoundedOutput {
        status,
        stdout: read_capture(&stdout_path)?,
        stderr: read_capture(&stderr_path)?,
    })
}

fn read_capture(path: &Path) -> Result<Vec<u8>, BoundedCommandError> {
    std::fs::read(path).map_err(|error| {
        BoundedCommandError::Capture(format!("read subprocess capture {path:?}: {error}"))
    })
}

fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .arg("-KILL")
            .arg(format!("-{}", child.id()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}
