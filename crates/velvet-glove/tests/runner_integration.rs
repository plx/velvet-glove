//! End-to-end tests for Velvet Glove's immediate and deferred workflows.
//!
//! These tests pipe native hook JSON through the unified executable and verify
//! stdout, stderr, exit codes, artifacts, and durable state behavior.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use support::native_events::{PostToolUseBuilder, ProtocolSurface};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn run_example(command_name: &str, fixture: &[u8], extra_args: &[&str]) -> std::process::Output {
    let binary_path = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary_path);
    command.args(unified_args(command_name, extra_args));
    configure_hook_environment(&mut command, command_name, fixture, extra_args);
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(fixture).unwrap();
            child.wait_with_output()
        })
        .unwrap_or_else(|e| panic!("failed to run velvet-glove {command_name}: {e}"))
}

fn spawn_example(command_name: &str, fixture: &[u8], extra_args: &[&str]) -> std::process::Child {
    use std::io::Write;

    let binary_path = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary_path);
    command.args(unified_args(command_name, extra_args));
    configure_hook_environment(&mut command, command_name, fixture, extra_args);
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("failed to run velvet-glove {command_name}: {error}"));
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(fixture)
        .expect("write fixture");
    child
}

fn unified_args(command_name: &str, extra_args: &[&str]) -> Vec<String> {
    let mut args = extra_args
        .iter()
        .map(|argument| match *argument {
            "--claude" => "--harness=claude".to_owned(),
            "--codex" => "--harness=codex".to_owned(),
            "--antigravity" => "--harness=antigravity".to_owned(),
            argument => argument.to_owned(),
        })
        .collect::<Vec<_>>();
    assert!(
        matches!(
            command_name,
            "post-tool-immediate" | "post-tool" | "turn-completion" | "session-start-state"
        ),
        "unknown Velvet Glove command {command_name}",
    );
    args.push(command_name.to_owned());
    args
}

fn configure_hook_environment(
    command: &mut Command,
    command_name: &str,
    fixture: &[u8],
    extra_args: &[&str],
) {
    clear_modeled_hook_environment(command);
    let harness = extra_args
        .iter()
        .find_map(|argument| {
            argument.strip_prefix("--harness=").or_else(|| {
                matches!(*argument, "--claude" | "--codex" | "--antigravity")
                    .then(|| argument.trim_start_matches("--"))
            })
        })
        .or_else(|| command_name.split_once('-').map(|(prefix, _)| prefix));

    let Some(harness @ "claude") = harness else {
        return;
    };
    let input: serde_json::Value = serde_json::from_slice(fixture)
        .unwrap_or_else(|error| panic!("invalid {harness} integration fixture: {error}"));
    let field = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| input.get(*name).and_then(serde_json::Value::as_str))
            .unwrap_or_else(|| {
                panic!(
                    "{harness} integration fixture is missing string field {}",
                    names.join(" or ")
                )
            })
    };
    let session_id = field(&["session_id", "sessionId"]);
    let project_dir = field(&["cwd"]);

    match harness {
        "claude" => {
            command
                .env("CLAUDECODE", "1")
                .env("CLAUDE_CODE_CHILD_SESSION", "1")
                .env("CLAUDE_CODE_SESSION_ID", session_id)
                .env("CLAUDE_PROJECT_DIR", project_dir);

            let event = field(&["hook_event_name", "hookEventName"]);
            if matches!(
                event,
                "SessionStart" | "Setup" | "CwdChanged" | "FileChanged"
            ) {
                command.env("CLAUDE_ENV_FILE", format!("{project_dir}/.claude-hook-env"));
            }
        }
        _ => unreachable!(),
    }
}

fn clear_modeled_hook_environment(command: &mut Command) {
    const EXACT_NAMES: &[&str] = &[
        "CLAUDECODE",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_PROJECT_DIR",
        "CLAUDE_ENV_FILE",
        "CLAUDE_EFFORT",
        "TRACEPARENT",
        "CLAUDE_CODE_REMOTE",
        "CLAUDE_CODE_REMOTE_SESSION_ID",
        "CLAUDE_CODE_BRIDGE_SESSION_ID",
        "CLAUDE_PLUGIN_ROOT",
        "CLAUDE_PLUGIN_DATA",
        "PLUGIN_ROOT",
        "PLUGIN_DATA",
    ];
    for name in EXACT_NAMES {
        command.env_remove(name);
    }
    for name in std::env::vars_os().filter_map(|(name, _)| name.into_string().ok()) {
        if name.starts_with("CLAUDE_PLUGIN_OPTION_") {
            command.env_remove(name);
        }
    }
}

fn temp_project(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "velvet-glove-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("failed to create temp project");
    path
}

fn wait_for_path(path: &Path) {
    for _ in 0..500 {
        if path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("timed out waiting for {}", path.display());
}

fn run_git(project: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_executable(project: &Path, name: &str, body: &str) -> PathBuf {
    let bin_dir = project.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("failed to create bin dir");
    let path = bin_dir.join(name);
    std::fs::write(&path, body).unwrap_or_else(|e| panic!("failed to write {name}: {e}"));

    #[cfg(unix)]
    {
        let mut perms = std::fs::metadata(&path)
            .unwrap_or_else(|e| panic!("{name} metadata: {e}"))
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap_or_else(|e| panic!("chmod {name}: {e}"));
    }

    path
}

fn write_fake_ruff(project: &Path) -> PathBuf {
    let bin_dir = project.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("failed to create bin dir");
    let fake = bin_dir.join("ruff");
    std::fs::write(
        &fake,
        r#"#!/usr/bin/env bash
set -u
mode="${1:-}"
shift || true
file="${@: -1}"

if [[ "$mode" == "format" ]]; then
  check=0
  for arg in "$@"; do
    if [[ "$arg" == "--check" ]]; then
      check=1
    fi
  done
  if grep -q "format_crash" "$file"; then
    echo "format crashed" >&2
    exit 2
  fi
  if grep -q "needs_format" "$file"; then
    if [[ "$check" == "1" ]]; then
      echo "Would reformat: $file"
      exit 1
    else
      perl -0pi -e 's/needs_format/formatted/g' "$file"
      echo "1 file reformatted"
    fi
  else
    echo "1 file left unchanged"
  fi
  exit 0
fi

if [[ "$mode" == "check" ]]; then
  fix=0
  unfixable_f401=0
  prev=""
  for arg in "$@"; do
    if [[ "$arg" == "--fix" ]]; then
      fix=1
    fi
    if [[ "$prev" == "--unfixable" && "$arg" == "F401" ]]; then
      unfixable_f401=1
    fi
    prev="$arg"
  done

  if grep -q "wait_for_release" "$file"; then
    : > "${file}.started"
    for _ in $(seq 1 1000); do
      [[ -e "${file}.release" ]] && break
      sleep 0.01
    done
    [[ -e "${file}.release" ]] || exit 2
  fi

  if grep -q "check_crash" "$file"; then
    echo "check crashed" >&2
    exit 2
  fi

  if grep -q "manual_issue" "$file"; then
    if grep -q "large_diagnostic" "$file"; then
      head -c 131072 /dev/zero | tr '\0' x >&2
      echo >&2
    fi
    echo "${file}:1:1: F821 undefined name manual_issue" >&2
    exit 1
  fi

  if grep -q "unused_import" "$file"; then
    if [[ "$fix" == "1" && "$unfixable_f401" == "0" ]]; then
      # Like real import removal, this fix can leave formatting behind.
      perl -0pi -e 's/^.*unused_import.*\n?//mg; s/dirties_format/needs_format/g' "$file"
      echo "Found 1 error (1 fixed)"
      exit 0
    fi
    echo "${file}:1:1: F401 unused import" >&2
    exit 1
  fi

  echo "All checks passed!"
  exit 0
fi

echo "unknown fake ruff mode: $mode" >&2
exit 2
"#,
    )
    .expect("failed to write fake ruff");

    #[cfg(unix)]
    {
        let mut perms = std::fs::metadata(&fake)
            .expect("fake ruff metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake, perms).expect("failed to chmod fake ruff");
    }

    fake
}

/// Write a Pkl config that wires the embedded ruff builtin to a custom
/// executable (typically a fake bash script). `extra_phase` is an optional
/// Pkl snippet inserted inside the `phases { ... }` block.
fn write_ruff_hook_config(project: &Path, fake_ruff: &Path, extra_phase: &str) {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).expect("failed to create config dir");
    let escaped = fake_ruff.to_string_lossy().replace('\\', "\\\\");
    let config = format!(
        r#"amends "Config.pkl"
import "Builtins.pkl"

settings {{
  diagnosticsDirectory = ".velvet-glove/ruff-agent-hook"
}}

tools {{
  ["ruff"] = (Builtins.ruff) {{
    executable = "{escaped}"
    phases {{
{extra_phase}
    }}
    messages {{
      issuesAgent = "fix {{{{ issue_files | join(\", \") }}}}; diagnostics {{{{ diagnostics_rel_path }}}}"
      issuesChangedAgent = "re-read {{{{ changed_files | join(\", \") }}}}, then fix {{{{ issue_files | join(\", \") }}}}; diagnostics {{{{ diagnostics_rel_path }}}}"
    }}
  }}
}}
run = new Listing {{ "ruff" }}
"#
    );
    std::fs::write(config_dir.join("post-tool-use.pkl"), config)
        .expect("failed to write post-tool-use.pkl");
}

fn add_deferred_reporting_config(project: &Path, reporting_body: &str) {
    let path = project.join(".velvet-glove/post-tool-use.pkl");
    let config = std::fs::read_to_string(&path).expect("read generated hook config");
    let replacement = format!(
        "settings {{\n  deferredReporting = new DeferredReporting {{\n{reporting_body}\n  }}"
    );
    let config = config.replacen("settings {", &replacement, 1);
    std::fs::write(path, config).expect("write deferred reporting config");
}

fn add_runner_setting(project: &Path, setting: &str) {
    let path = project.join(".velvet-glove/post-tool-use.pkl");
    let config = std::fs::read_to_string(&path).expect("read generated hook config");
    let replacement = format!("settings {{\n  {setting}");
    let config = config.replacen("settings {", &replacement, 1);
    std::fs::write(path, config).expect("write runner setting");
}

fn replace_file_activity_settings(project: &Path, body: &str) {
    let path = project.join(".velvet-glove/post-tool-use.pkl");
    let config = std::fs::read_to_string(&path).expect("read generated hook config");
    let config = config.replacen(
        "fileActivity { filesystemMtime = false }",
        &format!("fileActivity {{ {body} }}"),
        1,
    );
    std::fs::write(path, config).expect("write file activity settings");
}

fn write_per_file_ruff_hook_config(project: &Path, fake_ruff: &Path) {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).expect("failed to create config dir");
    let escaped = fake_ruff.to_string_lossy().replace('\\', "\\\\");
    let config = format!(
        r#"amends "Config.pkl"
import "Builtins.pkl"

settings {{
  fileActivity {{ filesystemMtime = false }}
}}

tools {{
  ["ruff"] = (Builtins.ruff) {{
    executable = "{escaped}"
    workflows {{
      ["lint"] = new Workflow {{
        check = new WorkflowCommand {{
          argv = new Listing {{ "check"; new Files {{}} }}
          exitCodes {{
            clean = new Listing {{ 0 }}
            issues = new Listing {{ 1 }}
            failure = new Listing {{ 2 }}
          }}
        }}
        remedy = new WorkflowCommand {{
          argv = new Listing {{ "check"; "--fix"; new Files {{}} }}
          exitCodes {{
            clean = new Listing {{ 0 }}
            issues = new Listing {{ 1 }}
            failure = new Listing {{ 2 }}
          }}
          writes = "target-files"
        }}
        invocation = "per-file"
      }}
    }}
    workflowOrder = new Listing {{ "lint" }}
  }}
}}
run = new Listing {{ "ruff" }}
"#
    );
    std::fs::write(config_dir.join("post-tool-use.pkl"), config)
        .expect("failed to write post-tool-use.pkl");
}

fn write_selective_operational_hook_config(project: &Path, fake_ruff: &Path) {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).expect("failed to create config dir");
    let clean_executable = fake_ruff.to_string_lossy().replace('\\', "\\\\");
    let crashing_executable = write_executable(
        project,
        "crashing-checker",
        "#!/bin/sh\necho 'checker crashed' >&2\nexit 2\n",
    )
    .to_string_lossy()
    .replace('\\', "\\\\");
    let config = format!(
        r#"amends "Config.pkl"

settings {{
  fileActivity {{ filesystemMtime = false }}
}}

tools {{
  ["clean-python"] = new ToolSpec {{
    id = "clean-python"
    displayName = "Clean Python"
    executable = "{clean_executable}"
    files {{ include = new Listing {{ "**/*.py" }} }}
    workflows {{
      ["lint"] = new Workflow {{
        check = new WorkflowCommand {{
          argv = new Listing {{ "check"; new Files {{}} }}
          exitCodes {{ issues = new Listing {{ 1 }}; failure = new Listing {{ 2 }} }}
        }}
        invocation = "per-file"
      }}
    }}
    workflowOrder = new Listing {{ "lint" }}
  }}
  ["crashing-rust"] = new ToolSpec {{
    id = "crashing-rust"
    displayName = "Crashing Rust"
    executable = "{crashing_executable}"
    files {{ include = new Listing {{ "**/*.rs" }} }}
    workflows {{
      ["lint"] = new Workflow {{
        check = new WorkflowCommand {{
          argv = new Listing {{ "check"; new Files {{}} }}
          exitCodes {{ issues = new Listing {{ 1 }}; failure = new Listing {{ 2 }} }}
        }}
        invocation = "per-file"
      }}
    }}
    workflowOrder = new Listing {{ "lint" }}
  }}
}}
run = new Listing {{ "clean-python"; "crashing-rust" }}
"#
    );
    std::fs::write(config_dir.join("post-tool-use.pkl"), config)
        .expect("failed to write post-tool-use.pkl");
}

/// A missing checker listed before the builtin Ruff spec (backed by the fake
/// ruff), under the given `missingToolPolicy`.
fn write_missing_tool_with_ruff_config(project: &Path, fake_ruff: &Path, policy: &str) {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).expect("failed to create config dir");
    let ruff = fake_ruff.to_string_lossy().replace('\\', "\\\\");
    let missing = project
        .join("bin/definitely-missing-checker")
        .to_string_lossy()
        .replace('\\', "\\\\");
    let config = format!(
        r#"amends "Config.pkl"
import "Builtins.pkl"

settings {{
  fileActivity {{ filesystemMtime = false }}
  missingToolPolicy = "{policy}"
}}

tools {{
  ["ghost"] = new ToolSpec {{
    id = "ghost"
    displayName = "Ghost"
    executable = "{missing}"
    installHint = "install ghost first"
    files {{ include = new Listing {{ "**/*.py" }} }}
    workflows {{
      ["lint"] = new Workflow {{
        check = new WorkflowCommand {{ argv = new Listing {{ "check"; new Files {{}} }} }}
      }}
    }}
    workflowOrder = new Listing {{ "lint" }}
  }}
  ["ruff"] = (Builtins.ruff) {{ executable = "{ruff}" }}
}}
run = new Listing {{ "ghost"; "ruff" }}
"#
    );
    std::fs::write(config_dir.join("post-tool-use.pkl"), config)
        .expect("failed to write post-tool-use.pkl");
}

fn write_artifact_linking_hook_config(
    project: &Path,
    fake_ruff: &Path,
    tool_ids: &[&str],
    invocation: &str,
) {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).expect("failed to create config dir");
    let executable = fake_ruff.to_string_lossy().replace('\\', "\\\\");
    let tools = tool_ids
        .iter()
        .map(|id| {
            format!(
                r#"  ["{id}"] = new ToolSpec {{
    id = "{id}"
    displayName = "{id}"
    executable = "{executable}"
    files {{ include = new Listing {{ "**/*.py" }} }}
    workflows {{
      ["lint"] = new Workflow {{
        check = new WorkflowCommand {{
          argv = new Listing {{ "check"; new Files {{}} }}
          exitCodes {{ issues = new Listing {{ 1 }}; failure = new Listing {{ 2 }} }}
        }}
        invocation = "{invocation}"
      }}
    }}
    workflowOrder = new Listing {{ "lint" }}
  }}"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let run = tool_ids
        .iter()
        .map(|id| format!("\"{id}\""))
        .collect::<Vec<_>>()
        .join("; ");
    let config = format!(
        r#"amends "Config.pkl"

settings {{ fileActivity {{ filesystemMtime = false }} }}

tools {{
{tools}
}}
run = new Listing {{ {run} }}
"#
    );
    std::fs::write(config_dir.join("post-tool-use.pkl"), config)
        .expect("failed to write post-tool-use.pkl");
}

fn post_tool_use_fixture(harness: &str, project: &Path, rel_path: &str) -> Vec<u8> {
    let surface = ProtocolSurface::parse(harness).unwrap_or_else(|error| panic!("{error}"));
    let identity_prefix = match surface {
        ProtocolSurface::Claude => "claude-ruff",
        ProtocolSurface::Codex => "codex-ruff",
        ProtocolSurface::Antigravity => "antigravity-ruff",
    };
    PostToolUseBuilder::new(surface, project, rel_path)
        .identity(
            format!("{identity_prefix}-test"),
            format!("{identity_prefix}-turn"),
            format!("{identity_prefix}-tool"),
        )
        .build()
        .unwrap_or_else(|error| panic!("failed to build {surface} fixture: {error}"))
        .into_bytes()
}

/// Parse an immediate-mode response: the JSON stdout plus its user-only
/// `systemMessage` (empty when absent). Immediate mode never writes stderr.
fn immediate_response(output: &std::process::Output) -> (serde_json::Value, String) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "immediate mode must keep stderr empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("immediate output should be JSON");
    let user = json["systemMessage"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    (json, user)
}

/// Contents of the single diagnostics file in `project/<directory>` whose name
/// ends with `suffix`.
fn read_diagnostics(project: &Path, directory: &str, suffix: &str) -> String {
    let directory = project.join(directory);
    let matches = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("read {directory:?}: {error}"))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(suffix))
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "{suffix} in {directory:?}: {matches:?}");
    std::fs::read_to_string(&matches[0]).unwrap()
}

fn codex_post_tool_case(
    project: &Path,
    tool_name: &str,
    tool_use_id: &str,
    tool_input: serde_json::Value,
) -> Vec<u8> {
    PostToolUseBuilder::new(ProtocolSurface::Codex, project, "src/codex-tool-call")
        .identity("codex-ruff-test", "codex-ruff-turn", tool_use_id)
        .tool(tool_name, tool_input, serde_json::json!({"exit_code": 0}))
        .build()
        .unwrap_or_else(|error| panic!("failed to build Codex tool fixture: {error}"))
        .into_bytes()
}

fn turn_completion_fixture(harness: &str, project: &Path) -> Vec<u8> {
    stop_fixture(harness, project, false)
}

/// A Stop event; `stop_hook_active` marks a Stop that follows a hook block.
fn stop_fixture(harness: &str, project: &Path, stop_hook_active: bool) -> Vec<u8> {
    let fixture = match harness {
        "claude" => serde_json::json!({
            "session_id": "claude-ruff-test",
            "transcript_path": "/tmp/claude-ruff-test.jsonl",
            "cwd": project.to_string_lossy(),
            "hook_event_name": "Stop",
            "stop_hook_active": stop_hook_active,
            "last_assistant_message": "done"
        }),
        "codex" => serde_json::json!({
            "session_id": "codex-ruff-test",
            "transcript_path": "/tmp/codex-ruff-test.jsonl",
            "cwd": project.to_string_lossy(),
            "hook_event_name": "Stop",
            "model": "gpt-test",
            "turn_id": "codex-ruff-turn",
            "permission_mode": "default",
            "stop_hook_active": stop_hook_active,
            "last_assistant_message": "done"
        }),
        "antigravity" => serde_json::json!({
            "conversationId": "antigravity-ruff-test",
            "workspacePaths": [project.to_string_lossy()],
            "transcriptPath": project.join("antigravity-transcript.jsonl").to_string_lossy(),
            "artifactDirectoryPath": project.join("antigravity-artifacts").to_string_lossy(),
            "executionNum": 1,
            "terminationReason": "agent-finished",
            "fullyIdle": true
        }),
        _ => panic!("unknown harness {harness}"),
    };
    serde_json::to_vec(&fixture).unwrap()
}

fn seed_pending_file(state_dir: &Path, harness: &str, path: &Path) {
    seed_pending_target(
        state_dir,
        harness,
        hookkit_file_activity::FileActivityTarget::exact(
            hookkit_core::Utf8PathBuf::from_path_buf(path.to_path_buf()).unwrap(),
        ),
    );
}

fn seed_pending_target(
    state_dir: &Path,
    harness: &str,
    target: hookkit_file_activity::FileActivityTarget,
) {
    let (harness, identity) = match harness {
        "claude" => (
            hookkit_core::HarnessId::CLAUDE_CODE,
            hookkit_session_state::SessionIdentity::Session("claude-ruff-test".into()),
        ),
        "codex" => (
            hookkit_core::HarnessId::CODEX,
            hookkit_session_state::SessionIdentity::Session("codex-ruff-test".into()),
        ),
        "antigravity" => (
            hookkit_core::HarnessId::ANTIGRAVITY,
            hookkit_session_state::SessionIdentity::Conversation("antigravity-ruff-test".into()),
        ),
        _ => panic!("unknown harness {harness}"),
    };
    let state = hookkit_session_state::SessionState::open(
        harness,
        identity,
        hookkit_session_state::StateRoot::new(state_dir),
    )
    .unwrap();
    let store = hookkit_file_activity::FileActivityStore::from_state(state).unwrap();
    store.requeue_targets("integration-test", [target]).unwrap();
}

fn prepare_deferred_ruff_case(
    harness: &str,
    name: &str,
    files: &[(&str, &str)],
) -> (PathBuf, PathBuf, String) {
    let project = temp_project(&format!("{name}-{harness}"));
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_per_file_ruff_hook_config(&project, &fake_ruff);
    for (relative, contents) in files {
        let path = project.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        seed_pending_file(&state_dir, harness, &path);
    }
    (project, state_dir, state_arg)
}

fn run_deferred_case(harness: &str, project: &Path, state_arg: &str) -> std::process::Output {
    let harness_arg = format!("--{harness}");
    run_example(
        "turn-completion",
        &turn_completion_fixture(harness, project),
        &[harness_arg.as_str(), "--state-dir", state_arg],
    )
}

fn only_summary(state_dir: &Path) -> serde_json::Value {
    let summaries = files_named(state_dir, "summary.json");
    assert_eq!(summaries.len(), 1, "expected exactly one deferred summary");
    serde_json::from_slice(&std::fs::read(&summaries[0]).unwrap()).unwrap()
}

fn session_journal_len(state_dir: &Path, harness: &str, session: &str) -> usize {
    hookkit_session_state::SessionState::open(
        hookkit_core::HarnessId::new(harness).unwrap(),
        hookkit_session_state::SessionIdentity::Session(session.into()),
        hookkit_session_state::StateRoot::new(state_dir),
    )
    .map_err(hookkit_file_activity::FileActivityError::from)
    .and_then(hookkit_file_activity::FileActivityStore::from_state)
    .and_then(|store| {
        store
            .pending()
            .with_entity(|view| {
                Ok(hookkit_session_state::EntityOutcome::retain(
                    view.events().len(),
                ))
            })
            .map_err(Into::into)
    })
    .unwrap()
}

fn files_named(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_named(&path, name));
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            found.push(path);
        }
    }
    found.sort();
    found
}

fn pkl_available() -> bool {
    Command::new("pkl")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

macro_rules! require_pkl {
    () => {
        if !pkl_available() {
            eprintln!("skipping test: pkl binary not on PATH");
            return;
        }
    };
}

// --- Velvet Glove runner integration tests ---

#[test]
fn file_activity_agent_hook_records_all_supported_posttool_paths() {
    let project = temp_project("post-tool");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let harnesses = [
        ("claude", "claude-code", "claude-ruff-test"),
        ("codex", "codex", "codex-ruff-test"),
        ("antigravity", "antigravity", "antigravity-ruff-test"),
    ];

    for (harness, harness_id, session) in harnesses {
        let fixture = post_tool_use_fixture(harness, &project, "src/main.rs");
        let harness_arg = format!("--harness={harness}");
        let output = run_example(
            "post-tool",
            &fixture,
            &[harness_arg.as_str(), "--state-dir", state_arg.as_str()],
        );
        assert!(output.status.success(), "{harness} tracker should succeed");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::json!({})
        );

        let identity = if harness == "antigravity" {
            hookkit_session_state::SessionIdentity::Conversation(session.into())
        } else {
            hookkit_session_state::SessionIdentity::Session(session.into())
        };
        let state = hookkit_session_state::SessionState::open(
            hookkit_core::HarnessId::new(harness_id).unwrap(),
            identity,
            hookkit_session_state::StateRoot::new(&state_dir),
        )
        .unwrap();
        let store = hookkit_file_activity::FileActivityStore::from_state(state).unwrap();
        store
            .pending()
            .with_entity(|view| {
                assert_eq!(view.events().len(), 1);
                assert!(
                    view.state().targets().contains(
                        &hookkit_file_activity::FileActivityTarget::exact(
                            hookkit_core::Utf8PathBuf::from_path_buf(project.join("src/main.rs"))
                                .unwrap()
                        )
                    )
                );
                Ok(hookkit_session_state::EntityOutcome::retain(()))
            })
            .unwrap();
    }

    let compatibility = run_example(
        "post-tool",
        &post_tool_use_fixture("codex", &project, "src/compatibility.rs"),
        &["--harness=codex", "--state-dir", state_arg.as_str()],
    );
    assert!(compatibility.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&compatibility.stdout).unwrap(),
        serde_json::json!({})
    );
}

#[test]
fn file_activity_observer_persists_shared_writer_patch_shell_and_gap_analysis_quietly() {
    let project = temp_project("file-activity-shared-analysis");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let cases = [
        (
            "Write",
            "writer",
            serde_json::json!({"file_path": "src/writer.rs", "content": "fn main() {}"}),
        ),
        (
            "apply_patch",
            "patch",
            serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: src/patched.rs\n@@\n-old\n+new\n*** End Patch"
            }),
        ),
        (
            "Bash",
            "shell",
            serde_json::json!({"command": "echo ok > src/shell.txt"}),
        ),
        (
            "Read",
            "read-only",
            serde_json::json!({"file_path": "src/read-only.rs"}),
        ),
        (
            "Bash",
            "dynamic-gap",
            serde_json::json!({"command": "echo ok > src/known.txt; mystery $TARGET"}),
        ),
    ];
    for (tool, id, input) in cases {
        let output = run_example(
            "post-tool",
            &codex_post_tool_case(&project, tool, id, input),
            &["--codex", "--state-dir", state_arg.as_str()],
        );
        assert!(
            output.status.success(),
            "{id}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::json!({})
        );
        assert!(output.stderr.is_empty(), "{id} observer must remain quiet");
    }

    let state = hookkit_session_state::SessionState::open(
        hookkit_core::HarnessId::CODEX,
        hookkit_session_state::SessionIdentity::Session("codex-ruff-test".into()),
        hookkit_session_state::StateRoot::new(&state_dir),
    )
    .unwrap();
    let store = hookkit_file_activity::FileActivityStore::from_state(state).unwrap();
    store
        .pending()
        .with_entity(|view| {
            let targets = view.state().targets();
            for relative in [
                "src/writer.rs",
                "src/patched.rs",
                "src/shell.txt",
                "src/known.txt",
            ] {
                assert!(
                    targets.contains(&hookkit_file_activity::FileActivityTarget::exact(
                        hookkit_core::Utf8PathBuf::from_path_buf(project.join(relative)).unwrap()
                    ))
                );
            }
            assert!(
                !targets.contains(&hookkit_file_activity::FileActivityTarget::exact(
                    hookkit_core::Utf8PathBuf::from_path_buf(project.join("src/read-only.rs"))
                        .unwrap()
                ))
            );
            assert!(view.state().has_gaps());
            let evidence = view
                .events()
                .iter()
                .filter_map(|record| match record.event() {
                    hookkit_file_activity::FileActivityEvent::Evidence(evidence) => Some(evidence),
                    hookkit_file_activity::FileActivityEvent::Gap(_)
                    | hookkit_file_activity::FileActivityEvent::Retry(_) => None,
                })
                .collect::<Vec<_>>();
            assert!(evidence.iter().any(|item| {
                item.source == hookkit_file_activity::FileActivitySource::StructuredToolInput
            }));
            assert!(
                evidence
                    .iter()
                    .any(|item| item.source == hookkit_file_activity::FileActivitySource::Patch)
            );
            assert!(evidence.iter().any(|item| {
                item.source == hookkit_file_activity::FileActivitySource::ShellInference
            }));
            Ok(hookkit_session_state::EntityOutcome::retain(()))
        })
        .unwrap();
}

#[test]
fn bundled_start_observer_and_turn_runner_share_one_explicit_state_root() {
    require_pkl!();
    let project = temp_project("bundled-deferred-suite-state-root");
    let state_dir = project.join("shared-state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_per_file_ruff_hook_config(&project, &fake_ruff);
    let start = serde_json::to_vec(&serde_json::json!({
        "session_id": "codex-ruff-test",
        "transcript_path": "/tmp/codex-ruff-test.jsonl",
        "cwd": project.to_string_lossy(),
        "hook_event_name": "SessionStart",
        "model": "gpt-test",
        "permission_mode": "default",
        "source": "startup"
    }))
    .unwrap();
    let started = run_example(
        "session-start-state",
        &start,
        &["--harness=codex", &format!("--state-dir={state_arg}")],
    );
    assert!(started.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&started.stdout).unwrap(),
        serde_json::json!({})
    );

    let file = project.join("src/clean.py");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "print('clean')\n").unwrap();
    let observed = run_example(
        "post-tool",
        &post_tool_use_fixture("codex", &project, "src/clean.py"),
        &["--harness=codex", &format!("--state-dir={state_arg}")],
    );
    assert!(observed.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&observed.stdout).unwrap(),
        serde_json::json!({})
    );
    assert_eq!(
        session_journal_len(&state_dir, "codex", "codex-ruff-test"),
        1
    );

    let stopped = run_example(
        "turn-completion",
        &turn_completion_fixture("codex", &project),
        &["--harness=codex", &format!("--state-dir={state_arg}")],
    );
    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(response, serde_json::json!({}), "clean runs are silent");
    assert_eq!(
        session_journal_len(&state_dir, "codex", "codex-ruff-test"),
        0
    );
    let summary = only_summary(&state_dir);
    assert_eq!(summary["counts"]["clean"], 1);
    assert!(Path::new(summary["run"]["stateDirectory"].as_str().unwrap()).starts_with(&state_dir));
}

// --- turn-completion consuming session state ---

#[test]
fn turn_completion_no_pending_work_emits_each_exact_native_no_op() {
    require_pkl!();
    for harness in ["claude", "codex", "antigravity"] {
        let project = temp_project(&format!("turn-completion-no-pending-{harness}"));
        let state_dir = project.join("state");
        let state_arg = state_dir.to_string_lossy().into_owned();
        let output = run_deferred_case(harness, &project, &state_arg);
        assert!(output.status.success(), "{harness}: {:?}", output.stderr);
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if harness == "antigravity" {
            assert_eq!(response, serde_json::json!({"decision": "stop"}));
        } else {
            assert_eq!(response, serde_json::json!({}));
        }
        assert!(files_named(&state_dir, "summary.json").is_empty());
    }
}

#[test]
fn turn_completion_allowed_bucket_matrix_uses_native_audience_channels() {
    require_pkl!();
    for harness in ["claude", "codex", "antigravity"] {
        for (case, files, expected_clean, expected_auto) in [
            ("clean", vec![("src/clean.py", "print('clean')\n")], 1, 0),
            (
                "auto",
                vec![("src/dirty.py", "import os  # unused_import\n")],
                0,
                1,
            ),
            (
                "mixed",
                vec![
                    ("src/clean.py", "print('clean')\n"),
                    ("src/dirty.py", "import os  # unused_import\n"),
                ],
                1,
                1,
            ),
        ] {
            let (project, state_dir, state_arg) = prepare_deferred_ruff_case(
                harness,
                &format!("turn-completion-allowed-{case}"),
                &files,
            );
            let output = run_deferred_case(harness, &project, &state_arg);
            assert!(
                output.status.success(),
                "{harness}/{case}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let notice = "velvet-glove auto-fixed src/dirty.py (Ruff); re-read before editing.";
            match (harness, expected_auto) {
                ("antigravity", 0) => {
                    assert_eq!(response, serde_json::json!({"decision": "stop"}));
                }
                ("antigravity", _) => {
                    assert_eq!(response["decision"], "stop");
                    assert!(
                        response["reason"]
                            .as_str()
                            .unwrap()
                            .contains("omitted user deferred Stop message")
                    );
                }
                (_, 0) => assert_eq!(
                    response,
                    serde_json::json!({}),
                    "{harness}/{case}: clean runs are silent"
                ),
                ("claude", _) => {
                    assert_eq!(
                        response,
                        serde_json::json!({
                            "systemMessage": notice,
                            "hookSpecificOutput": {
                                "hookEventName": "Stop",
                                "additionalContext": notice,
                            },
                        }),
                        "{case}"
                    );
                }
                _ => assert_eq!(
                    response,
                    serde_json::json!({"systemMessage": notice}),
                    "{harness}/{case}: an omitted agent copy of the user notice needs no warning"
                ),
            }
            let summary = only_summary(&state_dir);
            assert_eq!(summary["status"], "clean");
            assert_eq!(summary["counts"]["clean"], expected_clean);
            assert_eq!(summary["counts"]["autoFixed"], expected_auto);
            assert_eq!(summary["renderedMessages"]["lowering"]["blocked"], false);
        }
    }
}

#[test]
fn turn_completion_one_window_contains_clean_auto_fixed_and_manual_files() {
    require_pkl!();
    let project = temp_project("turn-completion-three-normal-buckets");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_per_file_ruff_hook_config(&project, &fake_ruff);
    std::fs::create_dir_all(project.join("src")).unwrap();
    for (path, contents) in [
        ("src/clean.py", "print('clean')\n"),
        ("src/auto.py", "import os  # unused_import\n"),
        ("src/manual.py", "print(manual_issue)\n"),
    ] {
        std::fs::write(project.join(path), contents).unwrap();
        let observed = run_example(
            "post-tool",
            &post_tool_use_fixture("codex", &project, path),
            &["--codex", "--state-dir", state_arg.as_str()],
        );
        assert!(observed.status.success());
    }

    let stopped = run_deferred_case("codex", &project, &state_arg);

    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(response["decision"], "block");
    let user = response["systemMessage"].as_str().unwrap();
    assert!(!user.contains("clean.py"), "{user}");
    assert!(user.contains("velvet-glove auto-fixed src/auto.py (Ruff)"));
    assert!(user.contains("1 file needs manual fixes (src/manual.py). Details: "));
    let reason = response["reason"].as_str().unwrap();
    assert!(reason.contains("velvet-glove auto-fixed src/auto.py (Ruff)"));
    assert!(reason.contains("Ruff: src/manual.py\nsrc/manual.py:1:1: F821 undefined name"));
    assert!(!reason.contains("clean.py"), "{reason}");
    let summary = only_summary(&state_dir);
    assert_eq!(summary["counts"]["clean"], 1);
    assert_eq!(summary["counts"]["autoFixed"], 1);
    assert_eq!(summary["counts"]["manualFixesNeeded"], 1);
    let statuses = summary["result"]["files"]
        .as_object()
        .unwrap()
        .values()
        .map(|file| file["status"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        statuses,
        std::collections::BTreeSet::from(["auto-fixed", "clean", "manual-fixes-needed"])
    );
}

#[test]
fn turn_completion_blocked_manual_and_operational_matrix_is_native() {
    require_pkl!();
    for harness in ["claude", "codex", "antigravity"] {
        for (case, contents, expected_status, blocking_operational) in [
            ("manual", "print(manual_issue)\n", "issues", false),
            (
                "operational",
                "print('check_crash')\n",
                "operational-failure",
                true,
            ),
        ] {
            let (project, state_dir, state_arg) = prepare_deferred_ruff_case(
                harness,
                &format!("turn-completion-blocked-{case}"),
                &[("src/result.py", contents)],
            );
            if blocking_operational {
                add_deferred_reporting_config(&project, "    blockOnOperationalErrors = true");
            }
            let output = run_deferred_case(harness, &project, &state_arg);
            assert!(
                output.status.success(),
                "{harness}/{case}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            match harness {
                "claude" | "codex" => assert_eq!(response["decision"], "block"),
                "antigravity" => assert_eq!(response["decision"], "continue"),
                _ => unreachable!(),
            }
            assert!(!response["reason"].as_str().unwrap().is_empty());
            assert!(
                response.get("hookSpecificOutput").is_none(),
                "{harness}/{case}: blocks carry the agent message only in reason"
            );
            if harness != "antigravity" {
                assert!(!response["systemMessage"].as_str().unwrap().is_empty());
            }
            let summary = only_summary(&state_dir);
            assert_eq!(summary["status"], expected_status);
            assert_eq!(summary["renderedMessages"]["lowering"]["blocked"], true);
            assert_eq!(
                summary["renderedMessages"]["lowering"]["agent"]["status"],
                "emitted"
            );
        }
    }
}

#[test]
fn turn_completion_operational_errors_notify_the_user_without_blocking() {
    require_pkl!();
    for harness in ["claude", "codex"] {
        let (project, state_dir, state_arg) = prepare_deferred_ruff_case(
            harness,
            "turn-completion-operational-notice",
            &[("src/result.py", "print('check_crash')\n")],
        );
        let output = run_deferred_case(harness, &project, &state_arg);
        assert!(output.status.success(), "{harness}");
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let user = response["systemMessage"].as_str().unwrap();
        assert!(
            user.starts_with(
                "velvet-glove could not run Ruff (lint.check failed with exit code 2; log: "
            ),
            "{harness}: {user}"
        );
        assert_eq!(
            response.as_object().unwrap().len(),
            1,
            "{harness}: only the user hears about operational errors: {response}"
        );
        let summary = only_summary(&state_dir);
        assert_eq!(summary["status"], "operational-failure");
        assert_eq!(summary["block"]["blocked"], false);
    }
}

#[test]
fn turn_completion_lowering_policies_and_empty_agent_are_explicit() {
    require_pkl!();

    let (project, state_dir, state_arg) = prepare_deferred_ruff_case(
        "codex",
        "turn-completion-strict-unrepresentable",
        &[("src/dirty.py", "import os  # unused_import\n")],
    );
    add_runner_setting(&project, r#"loweringPolicy = "strict""#);
    let strict = run_deferred_case("codex", &project, &state_arg);
    assert!(!strict.status.success());
    assert!(
        strict.stdout.is_empty(),
        "strict failure must not corrupt stdout"
    );
    let summary = only_summary(&state_dir);
    assert_eq!(
        summary["renderedMessages"]["lowering"]["agent"]["status"],
        "unrepresentable"
    );
    assert!(summary["renderedMessages"]["lowering"]["strictError"].is_string());
    assert!(session_journal_len(&state_dir, "codex", "codex-ruff-test") >= 1);

    let (project, state_dir, state_arg) = prepare_deferred_ruff_case(
        "codex",
        "turn-completion-best-effort-omission",
        &[("src/dirty.py", "import os  # unused_import\n")],
    );
    add_runner_setting(&project, r#"loweringPolicy = "best-effort""#);
    let best_effort = run_deferred_case("codex", &project, &state_arg);
    assert!(best_effort.status.success());
    let response: serde_json::Value = serde_json::from_slice(&best_effort.stdout).unwrap();
    assert!(
        response["systemMessage"]
            .as_str()
            .unwrap()
            .contains("velvet-glove auto-fixed")
    );
    assert!(
        !response["systemMessage"]
            .as_str()
            .unwrap()
            .contains("hookkit: omitted")
    );
    let summary = only_summary(&state_dir);
    assert_eq!(
        summary["renderedMessages"]["lowering"]["agent"]["status"],
        "omitted"
    );
    assert!(
        summary["renderedMessages"]["lowering"]["warnings"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let (project, state_dir, state_arg) = prepare_deferred_ruff_case(
        "codex",
        "turn-completion-empty-agent",
        &[("src/manual.py", "print(manual_issue)\n")],
    );
    add_runner_setting(&project, r#"loweringPolicy = "strict""#);
    add_deferred_reporting_config(
        &project,
        r#"    manualFixesNeeded = new TemplatePair { agent = "" }"#,
    );
    let empty_agent = run_deferred_case("codex", &project, &state_arg);
    assert!(
        empty_agent.status.success(),
        "{}",
        String::from_utf8_lossy(&empty_agent.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&empty_agent.stdout).unwrap();
    assert_eq!(response["decision"], "block");
    let reason = response["reason"].as_str().unwrap();
    assert!(
        reason.starts_with("velvet-glove: formatter/linter problems remain")
            && reason.contains(" Details: "),
        "a block always explains itself: {reason:?}"
    );
    let summary = only_summary(&state_dir);
    assert_eq!(
        summary["renderedMessages"]["lowering"]["agent"]["status"],
        "emitted"
    );
}

#[test]
fn turn_completion_batch_autofixes_then_acknowledges_the_exact_snapshot() {
    require_pkl!();
    let project = temp_project("turn-completion-autofix");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");
    std::fs::create_dir_all(project.join("src")).unwrap();
    let file = project.join("src/dirty.py");
    std::fs::write(&file, "import os  # unused_import\nprint('needs_format')\n").unwrap();

    let tracked = run_example(
        "post-tool",
        &post_tool_use_fixture("claude", &project, "src/dirty.py"),
        &["--harness=claude", "--state-dir", state_arg.as_str()],
    );
    assert!(tracked.status.success());
    assert_eq!(
        session_journal_len(&state_dir, "claude-code", "claude-ruff-test"),
        1
    );

    let stopped = run_example(
        "turn-completion",
        &turn_completion_fixture("claude", &project),
        &["--claude", "--state-dir", state_arg.as_str()],
    );
    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    let notice = "velvet-glove auto-fixed src/dirty.py (Ruff); re-read before editing.";
    assert_eq!(response["systemMessage"], notice);
    assert_eq!(response["hookSpecificOutput"]["additionalContext"], notice);
    let rewritten = std::fs::read_to_string(file).unwrap();
    assert!(rewritten.contains("formatted"));
    assert!(!rewritten.contains("unused_import"));
    assert_eq!(
        session_journal_len(&state_dir, "claude-code", "claude-ruff-test"),
        0
    );
    let summaries = files_named(&state_dir, "summary.json");
    assert_eq!(summaries.len(), 1);
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summaries[0]).unwrap()).unwrap();
    assert_eq!(summary["status"], "clean");
    assert_eq!(
        summary["stateDisposition"]["source"],
        "acknowledge-sealed-window"
    );

    let second = run_example(
        "turn-completion",
        &turn_completion_fixture("claude", &project),
        &["--claude", "--state-dir", state_arg.as_str()],
    );
    assert!(second.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&second.stdout).unwrap(),
        serde_json::json!({})
    );
    assert_eq!(
        files_named(&state_dir, "summary.json").len(),
        1,
        "the runner's own writes must not resurrect an auto-fixed file"
    );
}

#[test]
fn turn_completion_handled_baseline_suppresses_unchanged_git_dirty_fallback() {
    require_pkl!();
    let project = temp_project("turn-completion-git-dirty-baseline");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_per_file_ruff_hook_config(&project, &fake_ruff);
    replace_file_activity_settings(&project, r#"filesystemMtime = false; vcs = "git-dirty""#);
    std::fs::create_dir_all(project.join("src")).unwrap();
    let file = project.join("src/tracked.py");
    std::fs::write(&file, "print('original')\n").unwrap();
    run_git(&project, &["init", "-q"]);
    run_git(&project, &["add", "."]);
    run_git(
        &project,
        &[
            "-c",
            "user.name=HookKit",
            "-c",
            "user.email=hookkit@example.invalid",
            "commit",
            "-qm",
            "baseline",
        ],
    );
    std::fs::write(&file, "print('agent edit')\n").unwrap();
    let observed = run_example(
        "post-tool",
        &post_tool_use_fixture("codex", &project, "src/tracked.py"),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(observed.status.success());

    let first = run_deferred_case("codex", &project, &state_arg);
    assert!(first.status.success());
    assert_eq!(only_summary(&state_dir)["counts"]["clean"], 1);
    assert!(
        String::from_utf8_lossy(
            &Command::new("git")
                .arg("-C")
                .arg(&project)
                .args(["status", "--short", "--", "src/tracked.py"])
                .output()
                .unwrap()
                .stdout
        )
        .contains("src/tracked.py")
    );

    let second = run_deferred_case("codex", &project, &state_arg);

    assert!(second.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&second.stdout).unwrap(),
        serde_json::json!({})
    );
    assert_eq!(
        files_named(&state_dir, "summary.json").len(),
        1,
        "an unchanged handled Git-dirty file must not run checks again"
    );
}

#[test]
fn invalid_deferred_template_syntax_fails_before_any_remedy_runs() {
    require_pkl!();
    let project = temp_project("turn-completion-invalid-reporting-syntax");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");
    add_deferred_reporting_config(&project, r#"    clean = new TemplatePair { user = "{{" }"#);
    std::fs::create_dir_all(project.join("src")).unwrap();
    let file = project.join("src/dirty.py");
    std::fs::write(&file, "import os  # unused_import\n").unwrap();
    let tracked = run_example(
        "post-tool",
        &post_tool_use_fixture("codex", &project, "src/dirty.py"),
        &["--harness=codex", "--state-dir", state_arg.as_str()],
    );
    assert!(tracked.status.success());

    let stopped = run_example(
        "turn-completion",
        &turn_completion_fixture("codex", &project),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert!(response.get("decision").is_none(), "{response}");
    let user = response["systemMessage"].as_str().unwrap();
    assert!(
        user.starts_with(
            "velvet-glove reporting configuration is invalid; checks were skipped (invalid deferred reporting template `clean.user`"
        ),
        "{user}"
    );
    assert!(user.ends_with("config-error.log"), "{user}");
    assert!(
        std::fs::read_to_string(file)
            .unwrap()
            .contains("unused_import")
    );
    assert_eq!(files_named(&state_dir, "config-error.log").len(), 1);
    let summary_path = files_named(&state_dir, "summary.json").pop().unwrap();
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(summary_path).unwrap()).unwrap();
    assert_eq!(summary["status"], "operational-failure");
    assert_eq!(summary["result"]["artifacts"].as_object().unwrap().len(), 1);
    assert!(
        session_journal_len(&state_dir, "codex", "codex-ruff-test") >= 1,
        "configuration failures keep the work pending"
    );
}

#[test]
fn broken_pkl_config_is_a_user_notice_without_blocking() {
    require_pkl!();
    let project = temp_project("turn-completion-broken-pkl");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    std::fs::create_dir_all(project.join(".velvet-glove")).unwrap();
    std::fs::write(
        project.join(".velvet-glove/post-tool-use.pkl"),
        "amends \"Config.pkl\"\nsettings { jobs = \"not a number\" }\n",
    )
    .unwrap();
    std::fs::create_dir_all(project.join("src")).unwrap();
    let file = project.join("src/a.py");
    std::fs::write(&file, "print('a')\n").unwrap();
    seed_pending_file(&state_dir, "claude", &file);

    let stopped = run_deferred_case("claude", &project, &state_arg);

    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    let user = response["systemMessage"].as_str().unwrap();
    assert!(
        user.starts_with("velvet-glove configuration failed to load; checks were skipped ("),
        "{user}"
    );
    assert!(!user.contains("Deferred reporting"), "{user}");
    assert_eq!(response.as_object().unwrap().len(), 1, "{response}");
    assert_eq!(only_summary(&state_dir)["status"], "operational-failure");
}

#[test]
fn deferred_template_render_failure_is_a_durable_operational_error() {
    require_pkl!();
    let project = temp_project("turn-completion-reporting-render-failure");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");
    add_deferred_reporting_config(
        &project,
        r#"    masterUser = "{{ unavailable_reporting_function() }}""#,
    );
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/clean.py"), "print('clean')\n").unwrap();
    let tracked = run_example(
        "post-tool",
        &post_tool_use_fixture("codex", &project, "src/clean.py"),
        &["--harness=codex", "--state-dir", state_arg.as_str()],
    );
    assert!(tracked.status.success());

    let stopped = run_example(
        "turn-completion",
        &turn_completion_fixture("codex", &project),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert!(response.get("decision").is_none(), "{response}");
    assert!(
        response["systemMessage"]
            .as_str()
            .unwrap()
            .starts_with("velvet-glove could not render its report (")
    );
    let reporting_logs = files_named(&state_dir, "reporting-error.log");
    assert_eq!(reporting_logs.len(), 1);
    assert!(
        !std::fs::read_to_string(&reporting_logs[0])
            .unwrap()
            .is_empty()
    );
    let summary_path = files_named(&state_dir, "summary.json").pop().unwrap();
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(summary_path).unwrap()).unwrap();
    assert_eq!(summary["status"], "operational-failure");
    assert_eq!(summary["result"]["files"].as_object().unwrap().len(), 1);
    let artifacts = summary["result"]["artifacts"].as_object().unwrap();
    assert!(
        artifacts.len() >= 2,
        "tool artifacts must survive rendering failure"
    );
    assert!(artifacts.values().any(|artifact| {
        artifact["classification"] == "configuration-error"
            && artifact["absolutePath"]
                .as_str()
                .unwrap()
                .ends_with("reporting-error.log")
    }));
    assert!(session_journal_len(&state_dir, "codex", "codex-ruff-test") >= 1);
}

#[test]
fn turn_completion_mtime_fallback_finds_files_without_tool_observations() {
    require_pkl!();
    let project = temp_project("turn-completion-mtime-fallback");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let start = serde_json::to_vec(&serde_json::json!({
        "session_id": "claude-ruff-test",
        "transcript_path": "/tmp/claude-ruff-test.jsonl",
        "cwd": project.to_string_lossy(),
        "hook_event_name": "SessionStart",
        "source": "startup",
        "model": "claude-test"
    }))
    .unwrap();
    let started = run_example(
        "session-start-state",
        &start,
        &["--claude", "--state-dir", state_arg.as_str()],
    );
    assert!(started.status.success());

    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");
    std::fs::create_dir_all(project.join("src")).unwrap();
    let file = project.join("src/unobserved.py");
    std::fs::write(&file, "import os  # unused_import\nprint('needs_format')\n").unwrap();

    let stopped = run_example(
        "turn-completion",
        &turn_completion_fixture("claude", &project),
        &["--claude", "--state-dir", state_arg.as_str()],
    );
    assert!(stopped.status.success());
    let rewritten = std::fs::read_to_string(file).unwrap();
    assert!(rewritten.contains("formatted"));
    assert!(!rewritten.contains("unused_import"));
    assert_eq!(
        session_journal_len(&state_dir, "claude-code", "claude-ruff-test"),
        0
    );
}

#[test]
fn turn_completion_batch_retains_issues_and_points_at_detailed_logs() {
    require_pkl!();
    let project = temp_project("turn-completion-issues");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");
    std::fs::create_dir_all(project.join("src")).unwrap();
    let file = project.join("src/broken.py");
    std::fs::write(&file, "print(manual_issue)\n").unwrap();

    let tracked = run_example(
        "post-tool",
        &post_tool_use_fixture("codex", &project, "src/broken.py"),
        &["--harness=codex", "--state-dir", state_arg.as_str()],
    );
    assert!(tracked.status.success());

    let stopped = run_example(
        "turn-completion",
        &turn_completion_fixture("codex", &project),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(response["decision"], "block");
    assert!(
        response["systemMessage"]
            .as_str()
            .unwrap()
            .starts_with("velvet-glove: 1 file needs manual fixes (src/broken.py). Details: ")
    );
    let reason = response["reason"].as_str().unwrap();
    assert_eq!(
        reason,
        "velvet-glove found issues to fix before stopping:\n\nRuff: src/broken.py\nsrc/broken.py:1:1: F821 undefined name manual_issue",
        "the agent gets the final check's relativized output, not log paths"
    );
    assert!(
        session_journal_len(&state_dir, "codex", "codex-ruff-test") >= 1,
        "retained window includes tool evidence and fallback observations"
    );
    let summaries = files_named(&state_dir, "summary.json");
    assert_eq!(summaries.len(), 1);
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summaries[0]).unwrap()).unwrap();
    assert_eq!(summary["status"], "issues");
    assert_eq!(summary["schemaVersion"], 2);
    assert!(summary.get("acknowledged").is_none());
    assert!(summary.get("artifactContents").is_none());
    assert_eq!(
        summary["stateDisposition"]["source"],
        "acknowledge-sealed-window"
    );
    assert_eq!(
        summary["renderedMessages"]["lowering"]["policy"],
        "best-effort-with-warnings"
    );
    assert_eq!(
        summary["renderedMessages"]["lowering"]["user"]["status"],
        "emitted"
    );
    assert_eq!(
        summary["renderedMessages"]["lowering"]["agent"]["status"],
        "emitted"
    );
    assert!(
        summary["renderedMessages"]["user"]
            .as_str()
            .unwrap()
            .contains("manual fixes")
    );
    assert_eq!(summary["renderedMessages"]["agent"], reason);
    assert_eq!(summary["block"]["blocked"], true);
    assert_eq!(summary["block"]["reasons"]["manual"], true);
    let artifacts = summary["result"]["artifacts"].as_object().unwrap();
    assert!(artifacts.len() >= 4);
    for artifact in artifacts.values() {
        assert!(
            artifact.get("contents").is_none(),
            "log contents live only in the log files"
        );
    }
    assert!(artifacts.values().any(|artifact| {
        std::fs::read_to_string(artifact["absolutePath"].as_str().unwrap())
            .unwrap()
            .contains("F821 undefined name manual_issue")
    }));

    std::fs::write(&file, "print('fixed')\n").unwrap();
    let retried = run_example(
        "turn-completion",
        &turn_completion_fixture("codex", &project),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(retried.status.success());
    let retried_response: serde_json::Value = serde_json::from_slice(&retried.stdout).unwrap();
    assert_eq!(retried_response, serde_json::json!({}));
    let summaries = files_named(&state_dir, "summary.json");
    let retried_summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summaries[1]).unwrap()).unwrap();
    assert_eq!(retried_summary["counts"]["clean"], 1);
    assert_eq!(
        session_journal_len(&state_dir, "codex", "codex-ruff-test"),
        0
    );
}

#[test]
fn turn_completion_selectively_discharges_clean_and_retries_manual_files() {
    require_pkl!();
    let project = temp_project("turn-completion-selective");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_per_file_ruff_hook_config(&project, &fake_ruff);
    std::fs::create_dir_all(project.join("src")).unwrap();
    let clean = project.join("src/clean.py");
    let manual = project.join("src/manual.py");
    std::fs::write(&clean, "print('clean')\n").unwrap();
    std::fs::write(&manual, "print(manual_issue)\n").unwrap();

    for file in ["src/clean.py", "src/manual.py"] {
        let tracked = run_example(
            "post-tool",
            &post_tool_use_fixture("codex", &project, file),
            &["--harness=codex", "--state-dir", state_arg.as_str()],
        );
        assert!(tracked.status.success());
    }
    let stopped = run_example(
        "turn-completion",
        &turn_completion_fixture("codex", &project),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(response["decision"], "block");

    let state = hookkit_session_state::SessionState::open(
        hookkit_core::HarnessId::CODEX,
        hookkit_session_state::SessionIdentity::Session("codex-ruff-test".into()),
        hookkit_session_state::StateRoot::new(&state_dir),
    )
    .unwrap();
    let store = hookkit_file_activity::FileActivityStore::from_state(state).unwrap();
    store
        .pending()
        .with_entity(|view| {
            assert_eq!(view.state().targets().len(), 1);
            assert!(
                view.state()
                    .targets()
                    .contains(&hookkit_file_activity::FileActivityTarget::exact(
                        hookkit_core::Utf8PathBuf::from_path_buf(
                            std::fs::canonicalize(&manual).unwrap()
                        )
                        .unwrap()
                    ))
            );
            Ok(hookkit_session_state::EntityOutcome::retain(()))
        })
        .unwrap();

    let summaries = files_named(&state_dir, "summary.json");
    assert_eq!(summaries.len(), 1);
    let first: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summaries[0]).unwrap()).unwrap();
    let files = first["result"]["files"].as_object().unwrap();
    assert_eq!(files.len(), 2);
    assert!(files.values().any(|file| file["status"] == "clean"));
    assert!(
        files
            .values()
            .any(|file| file["status"] == "manual-fixes-needed")
    );

    std::fs::write(&manual, "print('fixed')\n").unwrap();
    let retried = run_example(
        "turn-completion",
        &turn_completion_fixture("codex", &project),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(retried.status.success());
    let retried_response: serde_json::Value = serde_json::from_slice(&retried.stdout).unwrap();
    assert_eq!(retried_response, serde_json::json!({}));
    assert_eq!(
        session_journal_len(&state_dir, "codex", "codex-ruff-test"),
        0
    );
    let summaries = files_named(&state_dir, "summary.json");
    assert_eq!(summaries.len(), 2);
    let second: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summaries[1]).unwrap()).unwrap();
    assert_eq!(second["candidateFiles"].as_array().unwrap().len(), 1);
    assert!(
        second["candidateFiles"][0]
            .as_str()
            .unwrap()
            .ends_with("manual.py")
    );
}

#[test]
fn turn_completion_preserves_observation_appended_while_stop_is_running() {
    require_pkl!();
    let project = temp_project("turn-completion-concurrent-observation");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_per_file_ruff_hook_config(&project, &fake_ruff);
    std::fs::create_dir_all(project.join("src")).unwrap();
    let slow = project.join("src/slow.py");
    let concurrent = project.join("src/concurrent.py");
    std::fs::write(&slow, "print('wait_for_release')\n").unwrap();
    std::fs::write(&concurrent, "print('concurrent')\n").unwrap();
    let observed = run_example(
        "post-tool",
        &post_tool_use_fixture("codex", &project, "src/slow.py"),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(observed.status.success());

    let fixture = turn_completion_fixture("codex", &project);
    let child = spawn_example(
        "turn-completion",
        &fixture,
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    wait_for_path(&project.join("src/slow.py.started"));
    let concurrent_observation = run_example(
        "post-tool",
        &post_tool_use_fixture("codex", &project, "src/concurrent.py"),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(concurrent_observation.status.success());
    std::fs::write(project.join("src/slow.py.release"), "release\n").unwrap();
    let first = child.wait_with_output().expect("wait for running Stop");

    assert!(first.status.success());
    let first_summary = only_summary(&state_dir);
    assert_eq!(first_summary["candidateFiles"].as_array().unwrap().len(), 1);
    assert!(
        first_summary["candidateFiles"][0]
            .as_str()
            .unwrap()
            .ends_with("slow.py")
    );
    assert_eq!(
        session_journal_len(&state_dir, "codex", "codex-ruff-test"),
        1,
        "the post-seal observation must remain in the active generation"
    );

    let second = run_deferred_case("codex", &project, &state_arg);

    assert!(second.status.success());
    let summaries = files_named(&state_dir, "summary.json");
    assert_eq!(summaries.len(), 2);
    let second_summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summaries[1]).unwrap()).unwrap();
    assert_eq!(
        second_summary["candidateFiles"].as_array().unwrap().len(),
        1
    );
    assert!(
        second_summary["candidateFiles"][0]
            .as_str()
            .unwrap()
            .ends_with("concurrent.py")
    );
}

#[test]
fn turn_completion_operational_failure_retries_only_affected_files() {
    require_pkl!();
    let project = temp_project("turn-completion-selective-operational");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_selective_operational_hook_config(&project, &fake_ruff);
    std::fs::create_dir_all(project.join("src")).unwrap();
    let clean = project.join("src/clean.py");
    let operational = project.join("src/operational.rs");
    std::fs::write(&clean, "print('clean')\n").unwrap();
    std::fs::write(&operational, "fn main() {}\n").unwrap();

    for file in ["src/clean.py", "src/operational.rs"] {
        let tracked = run_example(
            "post-tool",
            &post_tool_use_fixture("codex", &project, file),
            &["--harness=codex", "--state-dir", state_arg.as_str()],
        );
        assert!(tracked.status.success());
    }
    let stopped = run_example(
        "turn-completion",
        &turn_completion_fixture("codex", &project),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert!(response.get("decision").is_none(), "{response}");
    assert!(
        response["systemMessage"].as_str().unwrap().starts_with(
            "velvet-glove could not run Crashing Rust (lint.check failed with exit code 2; log: "
        ),
        "{response}"
    );

    let state = hookkit_session_state::SessionState::open(
        hookkit_core::HarnessId::CODEX,
        hookkit_session_state::SessionIdentity::Session("codex-ruff-test".into()),
        hookkit_session_state::StateRoot::new(&state_dir),
    )
    .unwrap();
    let store = hookkit_file_activity::FileActivityStore::from_state(state).unwrap();
    store
        .pending()
        .with_entity(|view| {
            assert_eq!(view.state().targets().len(), 1);
            assert!(
                view.state()
                    .targets()
                    .contains(&hookkit_file_activity::FileActivityTarget::exact(
                        hookkit_core::Utf8PathBuf::from_path_buf(
                            std::fs::canonicalize(&operational).unwrap()
                        )
                        .unwrap()
                    ))
            );
            Ok(hookkit_session_state::EntityOutcome::retain(()))
        })
        .unwrap();

    let summaries = files_named(&state_dir, "summary.json");
    assert_eq!(summaries.len(), 1);
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summaries[0]).unwrap()).unwrap();
    assert_eq!(summary["status"], "operational-failure");
    assert_eq!(summary["result"]["files"].as_object().unwrap().len(), 1);
    assert_eq!(
        summary["result"]["operationalProblems"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    let artifacts = summary["result"]["artifacts"].as_object().unwrap();
    assert_eq!(artifacts.len(), 2);
    assert!(
        artifacts
            .values()
            .any(|artifact| artifact["classification"] == "clean")
    );
    let failure = artifacts
        .values()
        .find(|artifact| artifact["classification"] == "failure")
        .expect("failure artifact");
    assert!(
        std::fs::read_to_string(failure["absolutePath"].as_str().unwrap())
            .unwrap()
            .contains("checker crashed")
    );
}

#[test]
fn turn_completion_missing_tool_follows_missing_tool_policy() {
    require_pkl!();
    for policy in ["user-notice", "harness-block", "hard-failure"] {
        let project = temp_project(&format!("turn-completion-missing-tool-{policy}"));
        let state_dir = project.join("state");
        let state_arg = state_dir.to_string_lossy().into_owned();
        let fake_ruff = write_fake_ruff(&project);
        write_missing_tool_with_ruff_config(&project, &fake_ruff, policy);
        std::fs::create_dir_all(project.join("src")).unwrap();
        let file = project.join("src/dirty.py");
        std::fs::write(&file, "import os  # unused_import\n").unwrap();
        seed_pending_file(&state_dir, "claude", &file);

        let stopped = run_deferred_case("claude", &project, &state_arg);

        assert!(
            !std::fs::read_to_string(&file)
                .unwrap()
                .contains("unused_import"),
            "{policy}: a missing tool must not stop another tool's autofix"
        );
        let summary = only_summary(&state_dir);
        assert_eq!(summary["counts"]["autoFixed"], 1, "{policy}");
        let problem = summary["result"]["operationalProblems"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        assert_eq!(problem["missingTool"], true);
        assert_eq!(problem["installHint"], "install ghost first");
        let pending = session_journal_len(&state_dir, "claude-code", "claude-ruff-test");
        if policy == "hard-failure" {
            assert!(!stopped.status.success());
            assert!(stopped.stdout.is_empty());
            continue;
        }
        assert!(stopped.status.success(), "{policy}");
        let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
        let user = response["systemMessage"].as_str().unwrap();
        assert!(
            user.contains("velvet-glove auto-fixed src/dirty.py (Ruff)"),
            "{policy}: {user}"
        );
        assert!(
            user.contains("velvet-glove could not run Ghost (")
                && user.contains("definitely-missing-checker not found; install ghost first)."),
            "{policy}: {user}"
        );
        if policy == "user-notice" {
            assert!(response.get("decision").is_none(), "{response}");
            assert_eq!(pending, 0, "a missing tool does not keep files pending");
        } else {
            assert_eq!(response["decision"], "block");
            assert!(
                response["reason"]
                    .as_str()
                    .unwrap()
                    .contains("velvet-glove could not run Ghost"),
                "{response}"
            );
            assert!(pending >= 1);
        }
    }
}

#[test]
fn turn_completion_loop_guard_stops_reblocking_unchanged_issues() {
    require_pkl!();
    let (project, state_dir, state_arg) = prepare_deferred_ruff_case(
        "claude",
        "turn-completion-loop-guard",
        &[("src/manual.py", "print(manual_issue)\n")],
    );
    let stop = |active: bool| {
        let output = run_example(
            "turn-completion",
            &stop_fixture("claude", &project, active),
            &["--claude", "--state-dir", state_arg.as_str()],
        );
        assert!(output.status.success());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };

    let first = stop(false);
    assert_eq!(first["decision"], "block");
    let repeated = stop(true);
    assert!(repeated.get("decision").is_none(), "{repeated}");
    assert!(repeated.get("hookSpecificOutput").is_none(), "{repeated}");
    let user = repeated["systemMessage"].as_str().unwrap();
    assert!(user.contains("1 file needs manual fixes"), "{user}");
    assert!(
        user.ends_with(
            "velvet-glove: not blocking again; the same issues remain after the agent's last attempt."
        ),
        "{user}"
    );
    assert!(
        session_journal_len(&state_dir, "claude-code", "claude-ruff-test") >= 1,
        "unfixed files stay pending"
    );
    let next_turn = stop(false);
    assert_eq!(
        next_turn["decision"], "block",
        "a new turn starts a new chain"
    );

    // Changing issues keep blocking until the consecutive-block cap.
    add_deferred_reporting_config(&project, "    maxConsecutiveBlocks = 2");
    let mut responses = Vec::new();
    for index in 0..3 {
        let path = project.join(format!("src/more-{index}.py"));
        std::fs::write(&path, "print(manual_issue)\n").unwrap();
        seed_pending_file(&state_dir, "claude", &path);
        responses.push(stop(index > 0));
    }
    assert_eq!(responses[0]["decision"], "block");
    assert_eq!(responses[1]["decision"], "block");
    assert!(responses[2].get("decision").is_none(), "{}", responses[2]);
    assert!(
        responses[2]["systemMessage"]
            .as_str()
            .unwrap()
            .ends_with("velvet-glove: not blocking again after 2 consecutive blocks.")
    );
}

#[test]
fn turn_completion_loop_guard_presumes_continuation_without_a_native_flag() {
    require_pkl!();
    let (project, _state_dir, state_arg) = prepare_deferred_ruff_case(
        "antigravity",
        "turn-completion-loop-guard-antigravity",
        &[("src/manual.py", "print(manual_issue)\n")],
    );
    let first = run_deferred_case("antigravity", &project, &state_arg);
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["decision"], "continue");
    let second = run_deferred_case("antigravity", &project, &state_arg);
    let second: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(
        second["decision"], "stop",
        "identical issues right after a block must not loop: {second}"
    );
}

#[test]
fn turn_completion_reruns_format_after_a_lint_fix_dirties_it() {
    require_pkl!();
    let project = temp_project("turn-completion-lint-dirties-format");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");
    std::fs::create_dir_all(project.join("src")).unwrap();
    let file = project.join("src/lint.py");
    std::fs::write(
        &file,
        "import os  # unused_import\nprint('dirties_format')\n",
    )
    .unwrap();
    seed_pending_file(&state_dir, "codex", &file);

    let stopped = run_deferred_case("codex", &project, &state_arg);

    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(
        response,
        serde_json::json!({
            "systemMessage": "velvet-glove auto-fixed src/lint.py (Ruff); re-read before editing."
        }),
        "a lint fix that dirties formatting must not be reported as a manual fix"
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "print('formatted')\n"
    );
    let summary = only_summary(&state_dir);
    assert_eq!(summary["counts"]["autoFixed"], 1);
    assert!(
        !files_named(&state_dir, "recheck.log").is_empty(),
        "the format check reruns before its remedy decision"
    );
}

#[test]
fn turn_completion_batch_blames_only_the_files_the_output_names() {
    require_pkl!();
    let project = temp_project("turn-completion-batch-attribution");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_artifact_linking_hook_config(&project, &fake_ruff, &["batch-check"], "batch");
    std::fs::create_dir_all(project.join("src")).unwrap();
    // The fake checker inspects the last file of a batch.
    for (name, contents) in [
        ("src/first.py", "print('first')\n"),
        ("src/second.py", "print(manual_issue)\n"),
    ] {
        std::fs::write(project.join(name), contents).unwrap();
        seed_pending_file(&state_dir, "codex", &project.join(name));
    }

    let stopped = run_deferred_case("codex", &project, &state_arg);

    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(response["decision"], "block");
    assert!(
        !response["reason"].as_str().unwrap().contains("first.py"),
        "{response}"
    );
    let summary = only_summary(&state_dir);
    assert_eq!(summary["counts"]["clean"], 1);
    assert_eq!(summary["counts"]["manualFixesNeeded"], 1);
    assert!(
        summary["manualFixFiles"][0]
            .as_str()
            .unwrap()
            .ends_with("src/second.py")
    );
}

#[test]
fn turn_completion_skips_git_ignored_candidates() {
    require_pkl!();
    let (project, state_dir, state_arg) = prepare_deferred_ruff_case(
        "codex",
        "turn-completion-git-ignored",
        &[
            ("dist/generated.py", "print(manual_issue)\n"),
            ("src/kept.py", "print('kept')\n"),
        ],
    );
    run_git(&project, &["init", "-q"]);
    std::fs::write(project.join(".gitignore"), "dist/\n").unwrap();

    let stopped = run_deferred_case("codex", &project, &state_arg);

    assert!(stopped.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&stopped.stdout).unwrap(),
        serde_json::json!({})
    );
    let summary = only_summary(&state_dir);
    let candidates = summary["candidateFiles"].as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].as_str().unwrap().ends_with("src/kept.py"));
    assert!(
        summary["result"]["notApplicableFiles"][0]
            .as_str()
            .unwrap()
            .ends_with("dist/generated.py")
    );
}

#[test]
fn turn_completion_prunes_old_run_bundles() {
    require_pkl!();
    let (project, state_dir, state_arg) = prepare_deferred_ruff_case(
        "codex",
        "turn-completion-run-retention",
        &[("src/a.py", "print('a')\n")],
    );
    assert!(
        run_deferred_case("codex", &project, &state_arg)
            .status
            .success()
    );
    let first = files_named(&state_dir, "summary.json").pop().unwrap();
    let runs = first.parent().unwrap().parent().unwrap().to_path_buf();
    for index in 0..25 {
        std::fs::create_dir_all(runs.join(format!("{}-1-{index}-turn-completion", 1000 + index)))
            .unwrap();
    }
    let file = project.join("src/b.py");
    std::fs::write(&file, "print('b')\n").unwrap();
    seed_pending_file(&state_dir, "codex", &file);

    assert!(
        run_deferred_case("codex", &project, &state_arg)
            .status
            .success()
    );

    let remaining = std::fs::read_dir(&runs).unwrap().count();
    assert_eq!(remaining, 20);
    assert!(first.exists(), "the newest runs are kept");
    assert_eq!(files_named(&state_dir, "summary.json").len(), 2);
}

#[test]
fn turn_completion_per_file_batch_isolates_one_operational_failure() {
    require_pkl!();
    let project = temp_project("turn-completion-partial-batch-failure");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_per_file_ruff_hook_config(&project, &fake_ruff);
    std::fs::create_dir_all(project.join("src")).unwrap();
    let clean = project.join("src/clean.py");
    let failed = project.join("src/failed.py");
    std::fs::write(&clean, "print('clean')\n").unwrap();
    std::fs::write(&failed, "print('check_crash')\n").unwrap();
    for path in [&clean, &failed] {
        seed_pending_file(&state_dir, "codex", path);
    }

    let stopped = run_deferred_case("codex", &project, &state_arg);

    assert!(stopped.status.success());
    let summary = only_summary(&state_dir);
    assert_eq!(summary["result"]["files"].as_object().unwrap().len(), 2);
    let normal_paths = summary["result"]["files"]
        .as_object()
        .unwrap()
        .values()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(normal_paths.iter().any(|path| path.ends_with("clean.py")));
    assert!(
        normal_paths.iter().any(|path| path.ends_with("failed.py")),
        "a successful independent format check remains a normal result"
    );
    let problem = summary["result"]["operationalProblems"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(problem["affectedFiles"].as_array().unwrap().len(), 1);
    assert!(
        problem["affectedFiles"][0]
            .as_str()
            .unwrap()
            .ends_with("failed.py")
    );
    let state = hookkit_session_state::SessionState::open(
        hookkit_core::HarnessId::CODEX,
        hookkit_session_state::SessionIdentity::Session("codex-ruff-test".into()),
        hookkit_session_state::StateRoot::new(&state_dir),
    )
    .unwrap();
    let store = hookkit_file_activity::FileActivityStore::from_state(state).unwrap();
    store
        .pending()
        .with_entity(|view| {
            assert_eq!(
                view.state().targets(),
                &std::collections::BTreeSet::from([
                    hookkit_file_activity::FileActivityTarget::exact(
                        hookkit_core::Utf8PathBuf::from_path_buf(
                            std::fs::canonicalize(&failed).unwrap(),
                        )
                        .unwrap(),
                    ),
                ])
            );
            Ok(hookkit_session_state::EntityOutcome::retain(()))
        })
        .unwrap();
}

#[test]
fn turn_completion_records_uncovered_deleted_unresolved_and_truncated_activity() {
    require_pkl!();

    for case in ["uncovered", "deleted", "unresolved", "truncated"] {
        let project = temp_project(&format!("turn-completion-coverage-{case}"));
        let state_dir = project.join("state");
        let state_arg = state_dir.to_string_lossy().into_owned();
        let fake_ruff = write_fake_ruff(&project);
        write_per_file_ruff_hook_config(&project, &fake_ruff);
        if case == "truncated" {
            replace_file_activity_settings(&project, "filesystemMtime = false; maxEntries = 1");
        }
        std::fs::create_dir_all(project.join("src/nested")).unwrap();
        std::fs::write(project.join("src/nested/one.py"), "print('one')\n").unwrap();
        std::fs::write(project.join("src/nested/two.py"), "print('two')\n").unwrap();

        match case {
            "uncovered" => {
                let path = project.join("src/note.unknown");
                std::fs::write(&path, "not covered\n").unwrap();
                seed_pending_file(&state_dir, "codex", &path);
            }
            "deleted" => {
                let path = project.join("src/deleted.py");
                std::fs::write(&path, "print('gone')\n").unwrap();
                seed_pending_file(&state_dir, "codex", &path);
                std::fs::remove_file(path).unwrap();
            }
            "unresolved" => seed_pending_target(
                &state_dir,
                "codex",
                hookkit_file_activity::FileActivityTarget::Path {
                    path: hookkit_core::Utf8PathBuf::from_path_buf(
                        project.join("missing-directory"),
                    )
                    .unwrap(),
                    scope: hookkit_file_activity::FileActivityScope::Descendants,
                },
            ),
            "truncated" => seed_pending_target(
                &state_dir,
                "codex",
                hookkit_file_activity::FileActivityTarget::Workspace {
                    root: Some(hookkit_core::Utf8PathBuf::from_path_buf(project.clone()).unwrap()),
                },
            ),
            _ => unreachable!(),
        }

        let stopped = run_deferred_case("codex", &project, &state_arg);

        assert!(
            stopped.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&stopped.stderr)
        );
        let summary = only_summary(&state_dir);
        match case {
            "uncovered" => assert_eq!(
                summary["result"]["uncoveredFiles"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            ),
            "deleted" => assert_eq!(
                summary["result"]["notApplicableFiles"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            ),
            "unresolved" => {
                assert!(summary["counts"]["coverageGaps"].as_u64().unwrap() >= 1);
                assert!(
                    summary["stateDisposition"]["retryTargets"]
                        .as_array()
                        .is_some_and(|targets| !targets.is_empty())
                );
            }
            "truncated" => {
                assert!(summary["counts"]["coverageGaps"].as_u64().unwrap() >= 1);
                assert!(
                    summary["stateDisposition"]["retryGaps"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|gap| gap.as_str().unwrap().contains("traversal budget"))
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn turn_completion_links_distinct_tool_artifacts_to_one_file() {
    require_pkl!();
    let project = temp_project("turn-completion-multi-tool-artifacts");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_artifact_linking_hook_config(
        &project,
        &fake_ruff,
        &["first-check", "second-check"],
        "per-file",
    );
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/shared.py"), "print('clean')\n").unwrap();
    let tracked = run_example(
        "post-tool",
        &post_tool_use_fixture("codex", &project, "src/shared.py"),
        &["--harness=codex", "--state-dir", state_arg.as_str()],
    );
    assert!(tracked.status.success());
    let stopped = run_example(
        "turn-completion",
        &turn_completion_fixture("codex", &project),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(stopped.status.success());

    let summary_path = files_named(&state_dir, "summary.json").pop().unwrap();
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(summary_path).unwrap()).unwrap();
    let file = summary["result"]["files"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    let reports = file["reports"].as_array().unwrap();
    assert_eq!(reports.len(), 2);
    let artifact_ids = reports
        .iter()
        .flat_map(|report| report["artifactIds"].as_array().unwrap())
        .map(|id| id.as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(artifact_ids.len(), 2);
    assert_eq!(summary["artifactPaths"].as_array().unwrap().len(), 2);
}

#[test]
fn turn_completion_applies_custom_group_bucket_and_master_templates() {
    require_pkl!();
    let project = temp_project("turn-completion-custom-reporting");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_per_file_ruff_hook_config(&project, &fake_ruff);
    add_deferred_reporting_config(
        &project,
        r#"    groups = new Listing<FileGroup> {
      new FileGroup { id = "special-python"; displayName = "Special Python"; include = new Listing { "**/*.py" } }
      new FileGroup { id = "other"; displayName = "Other"; include = new Listing { "**" } }
    }
    manualFixesNeeded = new TemplatePair {
      user = "BUCKET-U {{ manual_fix_files[0].displayPath }} {{ groups[0].display_name }}"
      agent = "BUCKET-A {{ manual_fix_files[0].groupId }} {{ artifact_paths | length }}"
    }
    masterUser = "MASTER-U {{ rendered_buckets.manual_fixes_needed.user }}"
    masterAgent = "MASTER-A {{ rendered_buckets.manual_fixes_needed.agent }}""#,
    );
    std::fs::create_dir_all(project.join("src")).unwrap();
    let file = project.join("src/custom.py");
    std::fs::write(&file, "print(manual_issue)\n").unwrap();
    seed_pending_file(&state_dir, "codex", &file);

    let stopped = run_deferred_case("codex", &project, &state_arg);

    assert!(stopped.status.success());
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(
        response["systemMessage"],
        "MASTER-U BUCKET-U src/custom.py Special Python"
    );
    assert!(
        response["reason"]
            .as_str()
            .unwrap()
            .starts_with("MASTER-A BUCKET-A special-python ")
    );
    let summary = only_summary(&state_dir);
    let file = summary["result"]["files"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(file["groupId"], "special-python");
}

#[test]
fn turn_completion_keeps_large_diagnostics_in_artifacts_not_native_context() {
    require_pkl!();
    let project = temp_project("turn-completion-large-diagnostics");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_per_file_ruff_hook_config(&project, &fake_ruff);
    std::fs::create_dir_all(project.join("src")).unwrap();
    let file = project.join("src/large.py");
    std::fs::write(&file, "print(manual_issue)  # large_diagnostic\n").unwrap();
    seed_pending_file(&state_dir, "codex", &file);

    let stopped = run_deferred_case("codex", &project, &state_arg);

    assert!(stopped.status.success());
    assert!(
        stopped.stdout.len() < 16_384,
        "native output must stay concise"
    );
    let response: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert!(!response["systemMessage"].as_str().unwrap().contains("xxxx"));
    let reason = response["reason"].as_str().unwrap();
    assert!(reason.len() < 2_000, "agent excerpt is bounded: {reason}");
    assert!(reason.contains("…truncated; full log: "), "{reason}");
    assert!(reason.contains("src/large.py:1:1: F821 undefined name manual_issue"));
    let summary = only_summary(&state_dir);
    assert!(
        std::fs::metadata(files_named(&state_dir, "summary.json").pop().unwrap())
            .unwrap()
            .len()
            < 100_000,
        "the summary does not repeat log contents"
    );
    assert!(
        summary["result"]["artifacts"]
            .as_object()
            .unwrap()
            .values()
            .any(
                |artifact| std::fs::metadata(artifact["absolutePath"].as_str().unwrap())
                    .unwrap()
                    .len()
                    > 100_000
            )
    );
}

#[test]
fn turn_completion_reuses_one_batch_artifact_for_multiple_files() {
    require_pkl!();
    let project = temp_project("turn-completion-batch-artifact");
    let state_dir = project.join("state");
    let state_arg = state_dir.to_string_lossy().into_owned();
    let fake_ruff = write_fake_ruff(&project);
    write_artifact_linking_hook_config(&project, &fake_ruff, &["batch-check"], "batch");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/first.py"), "print('first')\n").unwrap();
    std::fs::write(project.join("src/second.py"), "print('second')\n").unwrap();
    for file in ["src/first.py", "src/second.py"] {
        let tracked = run_example(
            "post-tool",
            &post_tool_use_fixture("codex", &project, file),
            &["--harness=codex", "--state-dir", state_arg.as_str()],
        );
        assert!(tracked.status.success());
    }
    let stopped = run_example(
        "turn-completion",
        &turn_completion_fixture("codex", &project),
        &["--codex", "--state-dir", state_arg.as_str()],
    );
    assert!(stopped.status.success());

    let summary_path = files_named(&state_dir, "summary.json").pop().unwrap();
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(summary_path).unwrap()).unwrap();
    let files = summary["result"]["files"].as_object().unwrap();
    assert_eq!(files.len(), 2);
    let artifact_ids = files
        .values()
        .map(|file| file["reports"][0]["artifactIds"][0].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(artifact_ids.len(), 1);
    let artifact = summary["result"]["artifacts"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(artifact["candidateFiles"].as_array().unwrap().len(), 2);
    assert_eq!(artifact["classification"], "clean");
}

// --- post-tool-immediate driven by Pkl configs ---

#[test]
fn post_tool_use_clean_python_file_is_quiet() {
    require_pkl!();
    let project = temp_project("ruff-clean");
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("clean.py"), "print('ok')\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/clean.py"),
        &["--claude"],
    );

    assert!(output.status.success());
    let stdout: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("clean output should be JSON");
    assert_eq!(stdout, serde_json::json!({}));
    assert!(
        String::from_utf8_lossy(&output.stderr).trim().is_empty(),
        "clean unchanged files should stay quiet"
    );
}

#[test]
fn post_tool_use_antigravity_runs_tools_for_the_originating_call_scope() {
    require_pkl!();
    let project = temp_project("ruff-antigravity-tool-call");
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");

    let file = project.join("src/dirty.py");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "print('needs_format')\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("antigravity", &project, "src/dirty.py"),
        &["--antigravity"],
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({})
    );
    assert_eq!(
        std::fs::read_to_string(file).unwrap(),
        "print('formatted')\n"
    );
}

#[test]
fn post_tool_use_autofix_sends_concise_agent_feedback_when_supported() {
    require_pkl!();
    let project = temp_project("ruff-autofix");
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("dirty.py"),
        "import os  # unused_import\nprint('needs_format')\n",
    )
    .unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/dirty.py"),
        &["--claude"],
    );

    let (json, user) = immediate_response(&output);
    let line = "velvet-glove auto-fixed src/dirty.py (Ruff); re-read before editing.";
    assert_eq!(json["hookSpecificOutput"]["additionalContext"], line);
    assert_eq!(user, line);
    let rewritten = std::fs::read_to_string(src.join("dirty.py")).unwrap();
    assert!(rewritten.contains("formatted"));
    assert!(!rewritten.contains("unused_import"));
}

#[test]
fn post_tool_use_manual_issues_write_diagnostics_and_render_template() {
    require_pkl!();
    let project = temp_project("ruff-manual");
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("broken.py"), "print(manual_issue)\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("codex", &project, "src/broken.py"),
        &["--codex"],
    );

    let (json, user) = immediate_response(&output);
    let context = json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("fix src/broken.py"));
    assert!(context.contains(".velvet-glove/ruff-agent-hook"));
    assert!(user.contains("Ruff: issues remain in src/broken.py; diagnostics: "));
    assert!(
        !user.contains("F821"),
        "full diagnostics stay out of the notice"
    );

    let diagnostics = std::fs::read_to_string(project.join(
        ".velvet-glove/ruff-agent-hook/codex-ruff-test_codex-ruff-turn_codex-ruff-tool_ruff-tool-issues.txt",
    ))
    .unwrap();
    assert!(diagnostics.contains("F821 undefined name manual_issue"));
}

/// The Ruff builtin on `fake_ruff` with default messages and diagnostics
/// location; `settings` is inserted verbatim inside `settings { ... }`.
fn write_default_ruff_config(project: &Path, fake_ruff: &Path, settings: &str) {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).unwrap();
    let escaped = fake_ruff.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        config_dir.join("post-tool-use.pkl"),
        format!(
            r#"amends "Config.pkl"
import "Builtins.pkl"

settings {{ {settings} }}
tools {{ ["ruff"] = (Builtins.ruff) {{ executable = "{escaped}" }} }}
run {{ "ruff" }}
"#
        ),
    )
    .unwrap();
}

#[test]
fn post_tool_use_manual_issues_quote_a_bounded_project_relative_excerpt() {
    require_pkl!();
    let project = temp_project("ruff-excerpt");
    let fake_ruff = write_fake_ruff(&project);
    write_default_ruff_config(&project, &fake_ruff, "");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/broken.py"), "print(manual_issue)\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/broken.py"),
        &["--claude"],
    );

    let (json, user) = immediate_response(&output);
    let context = json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    // Only the verify phase decides, so the fix phase's copy is not quoted.
    assert_eq!(
        context,
        "velvet-glove: Ruff reports issues in src/broken.py:\nsrc/broken.py:1:1: F821 undefined name manual_issue"
    );
    assert!(user.contains("Ruff: issues remain in src/broken.py; diagnostics: "));

    // A cut excerpt stays within the configured budget and points at the log.
    let bounded = temp_project("ruff-excerpt-bounded");
    let fake_ruff = write_fake_ruff(&bounded);
    write_default_ruff_config(
        &bounded,
        &fake_ruff,
        "deferredReporting { excerptMaxChars = 300 }",
    );
    std::fs::create_dir_all(bounded.join("src")).unwrap();
    std::fs::write(
        bounded.join("src/big.py"),
        "print(manual_issue)  # large_diagnostic\n",
    )
    .unwrap();
    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("codex", &bounded, "src/big.py"),
        &["--codex"],
    );
    let (json, _) = immediate_response(&output);
    let context = json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.chars().count() < 800, "{context}");
    assert!(
        context.contains("\n…truncated; full log: ")
            && context.contains("velvet-glove/state/post-tool-immediate"),
        "{context}"
    );
}

#[test]
fn post_tool_use_skips_git_ignored_files() {
    require_pkl!();
    let project = temp_project("ruff-git-ignored");
    let fake_ruff = write_fake_ruff(&project);
    write_default_ruff_config(&project, &fake_ruff, "");
    run_git(&project, &["init", "-q"]);
    std::fs::write(project.join(".gitignore"), "dist/\n").unwrap();
    std::fs::create_dir_all(project.join("dist")).unwrap();
    std::fs::write(project.join("dist/generated.py"), "print(manual_issue)\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "dist/generated.py"),
        &["--claude"],
    );

    let (json, user) = immediate_response(&output);
    assert_eq!(json, serde_json::json!({}));
    assert!(user.is_empty());
}

#[test]
fn post_tool_use_can_pass_phase_extra_args_for_unfixable_rules() {
    require_pkl!();
    let project = temp_project("ruff-unfixable");
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(
        &project,
        &fake_ruff,
        r#"      ["fix"] {
        extraArgs = new Listing<String> { "--unfixable"; "F401" }
      }
      ["verify"] {
        extraArgs = new Listing<String> { "--unfixable"; "F401" }
      }
"#,
    );

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("imports.py"), "import os  # unused_import\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/imports.py"),
        &["--claude"],
    );

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("should be JSON");
    assert!(
        json["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("fix src/imports.py")
    );
    assert!(
        std::fs::read_to_string(src.join("imports.py"))
            .unwrap()
            .contains("unused_import")
    );
    assert!(output.stderr.is_empty());
    assert!(
        read_diagnostics(
            &project,
            ".velvet-glove/ruff-agent-hook",
            "ruff-tool-issues.txt"
        )
        .contains("F401 unused import")
    );
}

#[test]
fn post_tool_use_reports_changed_files_and_remaining_issues() {
    require_pkl!();
    let project = temp_project("ruff-changed-issues");
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("broken_dirty.py"),
        "print('needs_format')\nprint(manual_issue)\n",
    )
    .unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/broken_dirty.py"),
        &["--claude"],
    );

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("should be JSON");
    let context = json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("re-read src/broken_dirty.py"));
    assert!(context.contains("fix src/broken_dirty.py"));
    assert!(
        std::fs::read_to_string(src.join("broken_dirty.py"))
            .unwrap()
            .contains("formatted")
    );
}

#[test]
fn post_tool_use_reports_missing_tool_to_user_without_failing_hook() {
    require_pkl!();
    let project = temp_project("ruff-missing");
    write_ruff_hook_config(
        &project,
        &project.join("bin").join("definitely-missing-ruff"),
        "",
    );

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("dirty.py"), "print('needs_format')\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/dirty.py"),
        &["--claude"],
    );

    let (json, user) = immediate_response(&output);
    assert!(
        json.get("hookSpecificOutput").is_none(),
        "agent hears nothing"
    );
    assert!(user.contains("unavailable"));
    assert!(user.contains("definitely-missing-ruff"));
}

#[test]
fn post_tool_use_reports_tool_failure_with_diagnostics() {
    require_pkl!();
    let project = temp_project("ruff-failure");
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("crash.py"), "print('format_crash')\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("codex", &project, "src/crash.py"),
        &["--codex"],
    );

    let (json, user) = immediate_response(&output);
    assert!(
        json.get("hookSpecificOutput").is_none(),
        "agent hears nothing"
    );
    assert!(user.contains("phase `format` failed"));
    let diagnostics = std::fs::read_to_string(project.join(
        ".velvet-glove/ruff-agent-hook/codex-ruff-test_codex-ruff-turn_codex-ruff-tool_ruff-tool-failure.txt",
    ))
    .unwrap();
    assert!(diagnostics.contains("format crashed"));
}

#[test]
fn post_tool_use_reports_changes_made_before_later_phase_failure() {
    require_pkl!();
    let project = temp_project("changed-before-failure");
    let changer = write_executable(
        &project,
        "changer",
        r#"#!/usr/bin/env bash
file="${@: -1}"
printf "changed\n" >> "$file"
exit 0
"#,
    );
    let failer = write_executable(
        &project,
        "failer",
        r#"#!/usr/bin/env bash
echo "verify crashed" >&2
exit 2
"#,
    );

    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).unwrap();
    let changer = changer.to_string_lossy().replace('\\', "\\\\");
    let failer = failer.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        config_dir.join("post-tool-use.pkl"),
        format!(
            r#"amends "Config.pkl"

settings {{
  diagnosticsDirectory = ".velvet-glove/post-tool-use"
}}

tools {{
  ["combo"] = new ToolSpec {{
    id = "combo"
    displayName = "Combo"
    executable = "{changer}"
    files {{ include = new Listing<String> {{ "*.py"; "**/*.py" }} }}
    phases {{
      ["format"] = new Phase {{
        mode = "format"
        program = "{changer}"
        argv = new Listing<String | ArgToken> {{ new Files {{}} }}
        writes = "target-files"
      }}
      ["verify"] = new Phase {{
        mode = "verify"
        program = "{failer}"
        argv = new Listing<String | ArgToken> {{ new Files {{}} }}
        exitCodes {{ clean = new Listing<Int> {{ 0 }}; failure = new Listing<Int> {{ 2 }} }}
      }}
    }}
    phaseOrder = new Listing<String> {{ "format"; "verify" }}
  }}
}}
run = new Listing<String> {{ "combo" }}
"#
        ),
    )
    .unwrap();

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.py"), "original\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/a.py"),
        &["--claude"],
    );

    assert!(output.status.success());
    assert!(
        std::fs::read_to_string(src.join("a.py"))
            .unwrap()
            .contains("changed")
    );
    let (json, user) = immediate_response(&output);
    assert!(
        json["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("velvet-glove auto-fixed src/a.py (Combo)")
    );
    assert!(user.contains("Combo: phase `verify` failed"));
    assert!(
        read_diagnostics(
            &project,
            ".velvet-glove/post-tool-use",
            "combo-tool-failure.txt"
        )
        .contains("verify crashed")
    );
}

#[test]
fn post_tool_use_fail_fast_stops_after_operational_failure() {
    require_pkl!();
    let project = temp_project("fail-fast");
    let failer = write_executable(
        &project,
        "failer",
        r#"#!/usr/bin/env bash
echo "tool crashed" >&2
exit 2
"#,
    );
    let changer = write_executable(
        &project,
        "changer",
        r#"#!/usr/bin/env bash
file="${@: -1}"
printf "changed\n" >> "$file"
exit 0
"#,
    );

    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).unwrap();
    let failer = failer.to_string_lossy().replace('\\', "\\\\");
    let changer = changer.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        config_dir.join("post-tool-use.pkl"),
        format!(
            r#"amends "Config.pkl"

settings {{
  failFast = true
  diagnosticsDirectory = ".velvet-glove/post-tool-use"
}}

tools {{
  ["failer"] = new ToolSpec {{
    id = "failer"
    displayName = "Failer"
    executable = "{failer}"
    files {{ include = new Listing<String> {{ "*.py"; "**/*.py" }} }}
    phases {{
      ["verify"] = new Phase {{
        mode = "verify"
        argv = new Listing<String | ArgToken> {{ new Files {{}} }}
        exitCodes {{ clean = new Listing<Int> {{ 0 }}; failure = new Listing<Int> {{ 2 }} }}
      }}
    }}
  }}
  ["changer"] = new ToolSpec {{
    id = "changer"
    displayName = "Changer"
    executable = "{changer}"
    files {{ include = new Listing<String> {{ "*.py"; "**/*.py" }} }}
    phases {{
      ["format"] = new Phase {{
        mode = "format"
        argv = new Listing<String | ArgToken> {{ new Files {{}} }}
        writes = "target-files"
      }}
    }}
  }}
}}
run = new Listing<String> {{ "failer"; "changer" }}
"#
        ),
    )
    .unwrap();

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.py"), "original\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/a.py"),
        &["--claude"],
    );

    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(src.join("a.py")).unwrap(),
        "original\n"
    );
    let (_, user) = immediate_response(&output);
    assert!(user.contains("Failer: phase `verify` failed"));
    assert!(!user.contains("Changer"));
}

#[cfg(unix)]
#[test]
fn post_tool_use_kills_timed_out_local_tools_and_keeps_diagnostics_out_of_the_project() {
    require_pkl!();
    let project = temp_project("timeout-local-bin");
    write_executable(
        &project,
        "hang",
        "#!/bin/sh\nprintf 'started %s\\n' \"$HANG_LABEL\"\nexec sleep 30\n",
    );
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("post-tool-use.pkl"),
        r#"amends "Config.pkl"

settings {
  commandTimeoutSeconds = 1
  localBinDirs { "bin" }
}

tools {
  ["hang"] = new ToolSpec {
    id = "hang"
    displayName = "Hang"
    executable = "hang"
    env { ["HANG_LABEL"] = "from-env" }
    files { include { "**/*.py" } }
    phases {
      ["verify"] = new Phase { mode = "verify"; argv { new Files {} } }
    }
  }
}
run { "hang" }
"#,
    )
    .unwrap();
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/a.py"), "print('ok')\n").unwrap();

    let started = std::time::Instant::now();
    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/a.py"),
        &["--claude"],
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(20));

    let (json, user) = immediate_response(&output);
    assert!(
        json.get("hookSpecificOutput").is_none(),
        "agent hears nothing"
    );
    assert!(
        user.contains("Hang: phase `verify` failed (timed out after 1s"),
        "{user}"
    );
    let diagnostics_path = user.rsplit("diagnostics: ").next().unwrap();
    assert!(
        diagnostics_path.contains("velvet-glove/state/post-tool-immediate"),
        "{diagnostics_path}"
    );
    let diagnostics = std::fs::read_to_string(diagnostics_path).unwrap();
    assert!(diagnostics.contains("started from-env"), "{diagnostics}");
    assert!(diagnostics.contains(&project.join("bin/hang").to_string_lossy().into_owned()));
    assert!(
        !project.join(".velvet-glove/post-tool-use").exists(),
        "diagnostics must not be written inside the project by default"
    );
}

#[test]
fn post_tool_use_continue_after_issues_false_stops_later_tools() {
    require_pkl!();
    let project = temp_project("stop-after-issues");
    let issuer = write_executable(
        &project,
        "issuer",
        r#"#!/usr/bin/env bash
echo "${1}: issue" >&2
exit 1
"#,
    );
    let changer = write_executable(
        &project,
        "changer",
        r#"#!/usr/bin/env bash
file="${@: -1}"
printf "changed\n" >> "$file"
exit 0
"#,
    );

    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).unwrap();
    let issuer = issuer.to_string_lossy().replace('\\', "\\\\");
    let changer = changer.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        config_dir.join("post-tool-use.pkl"),
        format!(
            r#"amends "Config.pkl"

settings {{
  continueAfterIssues = false
  diagnosticsDirectory = ".velvet-glove/post-tool-use"
}}

tools {{
  ["issuer"] = new ToolSpec {{
    id = "issuer"
    displayName = "Issuer"
    executable = "{issuer}"
    files {{ include = new Listing<String> {{ "*.py"; "**/*.py" }} }}
    phases {{
      ["verify"] = new Phase {{
        mode = "verify"
        argv = new Listing<String | ArgToken> {{ new Files {{}} }}
        exitCodes {{ clean = new Listing<Int> {{ 0 }}; issues = new Listing<Int> {{ 1 }} }}
      }}
    }}
  }}
  ["changer"] = new ToolSpec {{
    id = "changer"
    displayName = "Changer"
    executable = "{changer}"
    files {{ include = new Listing<String> {{ "*.py"; "**/*.py" }} }}
    phases {{
      ["format"] = new Phase {{
        mode = "format"
        argv = new Listing<String | ArgToken> {{ new Files {{}} }}
        writes = "target-files"
      }}
    }}
  }}
}}
run = new Listing<String> {{ "issuer"; "changer" }}
"#
        ),
    )
    .unwrap();

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.py"), "original\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/a.py"),
        &["--claude"],
    );

    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(src.join("a.py")).unwrap(),
        "original\n"
    );
    let (_, user) = immediate_response(&output);
    assert!(user.contains("Issuer: issues remain in src/a.py"));
    assert!(!user.contains("Changer"));
}

#[test]
fn post_tool_use_config_error_is_a_user_notice_and_skipped_for_read_only_calls() {
    require_pkl!();
    let project = temp_project("unknown-run-entry");
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("post-tool-use.pkl"),
        r#"amends "Config.pkl"
run = new Listing<String> { "rff" }
"#,
    )
    .unwrap();

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.py"), "print('ok')\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/a.py"),
        &["--claude"],
    );

    let (json, user) = immediate_response(&output);
    assert!(
        json.get("hookSpecificOutput").is_none(),
        "agent hears nothing"
    );
    assert!(user.starts_with("error: velvet-glove: configuration error; no tools ran:"));
    assert!(user.contains("run names unknown tool `rff`"), "{user}");

    // A call that touches no files returns before the (broken) policy is
    // even evaluated.
    let read = PostToolUseBuilder::new(ProtocolSurface::Claude, &project, "src/a.py")
        .identity("claude-ruff-test", "claude-ruff-turn", "claude-ruff-read")
        .tool(
            "Read",
            serde_json::json!({"file_path": project.join("src/a.py")}),
            serde_json::json!({"type": "text"}),
        )
        .build()
        .unwrap()
        .into_bytes();
    let output = run_example("post-tool-immediate", &read, &["--claude"]);
    let (json, _) = immediate_response(&output);
    assert_eq!(json, serde_json::json!({}));
}

#[test]
fn post_tool_use_codex_emits_posttool_agent_context() {
    require_pkl!();
    let project = temp_project("ruff-codex");
    let fake_ruff = write_fake_ruff(&project);
    write_ruff_hook_config(&project, &fake_ruff, "");

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("dirty.py"), "import os  # unused_import\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("codex", &project, "src/dirty.py"),
        &["--codex"],
    );

    let (stdout, user) = immediate_response(&output);
    assert_eq!(stdout["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    let line = "velvet-glove auto-fixed src/dirty.py (Ruff); re-read before editing.";
    assert_eq!(stdout["hookSpecificOutput"]["additionalContext"], line);
    assert_eq!(user, line);
}

#[test]
fn post_tool_use_hard_failure_policy_fails_the_hook() {
    require_pkl!();
    let project = temp_project("ruff-hard-failure");
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("post-tool-use.pkl"),
        r#"amends "Config.pkl"
import "Builtins.pkl"

settings {
  missingToolPolicy = "hard-failure"
}

tools {
  ["ruff"] = (Builtins.ruff) {
    executable = "definitely-missing-ruff"
  }
}
run = new Listing { "ruff" }
"#,
    )
    .unwrap();

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("dirty.py"), "print('needs_format')\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/dirty.py"),
        &["--claude"],
    );

    assert!(
        !output.status.success(),
        "hard-failure should fail the hook"
    );
    assert!(output.stdout.is_empty());
    assert!(
        output.stderr.is_empty(),
        "operational failures use the runtime diagnostics sink"
    );
}

#[test]
fn post_tool_use_harness_block_policy_emits_blocking_exit_code() {
    require_pkl!();
    let project = temp_project("ruff-harness-block");
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("post-tool-use.pkl"),
        r#"amends "Config.pkl"
import "Builtins.pkl"

settings {
  missingToolPolicy = "harness-block"
}

tools {
  ["ruff"] = (Builtins.ruff) {
    executable = "definitely-missing-ruff"
  }
}
run = new Listing { "ruff" }
"#,
    )
    .unwrap();

    let src = project.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("dirty.py"), "print('needs_format')\n").unwrap();

    let output = run_example(
        "post-tool-immediate",
        &post_tool_use_fixture("claude", &project, "src/dirty.py"),
        &["--claude"],
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "harness-block should exit 2 (blocking) instead of 0 or 1"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unavailable"));
}
