//! Verify that the embedded Pkl builtins evaluate to specs matching the
//! previous hand-written Rust `specs::ruff()`, `specs::prettier()`, etc.
//!
//! These tests run only when `pkl` is on PATH; otherwise they short-circuit
//! with an explanatory skip message so the suite doesn't fail in environments
//! without Pkl installed.

use hookkit_pkl_config::schema::{
    ArgToken, ArgvElement, CheckScope, ExitCodes, FileSelection, InvocationGranularity, Phase,
    PhaseMode, ToolSpec, UnexpectedExitPolicy, Workflow, WorkflowCommand, WriteBehavior,
};
use std::collections::BTreeMap;

fn pkl_available() -> bool {
    std::process::Command::new("pkl")
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

fn literal(s: &str) -> ArgvElement {
    ArgvElement::Literal(s.into())
}

fn token(t: ArgToken) -> ArgvElement {
    ArgvElement::Token(t)
}

fn argv_eq(actual: &[ArgvElement], expected: &[ArgvElement]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    actual.iter().zip(expected).all(|(a, e)| match (a, e) {
        (ArgvElement::Literal(a), ArgvElement::Literal(e)) => a == e,
        (ArgvElement::Token(a), ArgvElement::Token(e)) => a == e,
        _ => false,
    })
}

fn assert_argv(phase: &Phase, expected: Vec<ArgvElement>) {
    assert!(
        argv_eq(&phase.argv, &expected),
        "argv mismatch:\nactual:   {:?}\nexpected: {:?}",
        phase.argv,
        expected,
    );
}

fn assert_workflow_argv(command: &WorkflowCommand, expected: Vec<ArgvElement>) {
    assert!(
        argv_eq(&command.argv, &expected),
        "argv mismatch:\nactual:   {:?}\nexpected: {:?}",
        command.argv,
        expected,
    );
}

fn assert_exit_codes(actual: &ExitCodes, clean: &[i32], issues: &[i32], failure: &[i32]) {
    assert_eq!(actual.clean, clean, "clean codes");
    assert_eq!(actual.issues, issues, "issue codes");
    assert_eq!(actual.failure, failure, "failure codes");
}

#[test]
fn jq_uses_per_file_parse_validation_and_distinguishes_tool_failures() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let jq = spec(&specs, "jq");

    assert_eq!(jq.id, "jq");
    assert_eq!(jq.phase_invocation, InvocationGranularity::PerFile);
    assert!(jq.workflows.is_empty(), "jq uses compatibility translation");
    let verify = jq.phases.get("verify").expect("jq verify phase");
    assert_argv(
        verify,
        vec![
            literal("empty"),
            token(ArgToken::ExtraArgs),
            token(ArgToken::Files),
        ],
    );
    assert_exit_codes(&verify.exit_codes, &[0], &[5], &[1, 2, 3, 4]);
}

#[test]
fn vacuum_adapter_locks_the_pinned_read_only_batch_contract() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let vacuum = spec(&specs, "vacuum");

    assert_eq!(vacuum.id, "vacuum");
    assert_eq!(vacuum.display_name, "vacuum");
    assert_eq!(vacuum.executable, "vacuum");
    assert_eq!(vacuum.phase_invocation, InvocationGranularity::Batch);
    assert_eq!(
        vacuum.install_hint.as_deref(),
        Some(
            "use the checksum-pinned Vacuum 0.30.0 recipe documented in docs/pinned-tool-environments.md"
        )
    );
    assert!(
        vacuum.workflows.is_empty(),
        "Vacuum uses compatibility translation"
    );

    let verify = vacuum.phases.get("verify").expect("Vacuum verify phase");
    assert_eq!(verify.mode, PhaseMode::Verify);
    assert_eq!(verify.program.as_deref(), Some("python"));
    assert_eq!(verify.argv.len(), 7);
    assert_eq!(verify.argv[0], literal("-I"));
    assert_eq!(verify.argv[1], literal("-c"));
    let ArgvElement::Literal(adapter) = &verify.argv[2] else {
        panic!("Vacuum adapter must be a literal Python program")
    };
    for required in [
        "__VELVET_GLOVE_VACUUM_FILES__",
        "extra arguments are unsupported because Vacuum flags change the controlled contract",
        "selected path is not normalized and absolute",
        "os.path.realpath(path) != path",
        "getattr(os, \"O_NOFOLLOW\", 0)",
        "opened.st_nlink != 1",
        "data.decode(\"utf-8\")",
        "REFERENCE_KEYWORDS = (b\"$ref\", b\"$dynamicref\", b\"$recursiveref\")",
        "if any(keyword in lowered for keyword in REFERENCE_KEYWORDS)",
        "b\"\\\\x24\"",
        "b\"\\\\u0024\"",
        "b\"\\\\u00000024\"",
        "FILE_COUNT_LIMIT = 256",
        "selected batch exceeds the {FILE_COUNT_LIMIT}-file limit",
        "selected files exceed the {BATCH_LIMIT}-byte batch limit",
        "--config=vacuum.conf.yaml",
        "--base=.",
        "--no-update-check",
        "--remote=false",
        "--no-style",
        "--no-banner",
        "--details",
        "--errors",
        "--silent",
        "--all-results",
        "--no-clip",
        "--fail-severity=error",
        "--fix=false",
        "--turbo=false",
        "--hard-mode=false",
        "--allow-private-networks=false",
        "--allow-http=false",
        "tempfile.mkdtemp(prefix=\"velvet-glove-vacuum-\", dir=\"/tmp\")",
        "write_owned_file(os.path.join(private_root, \"vacuum.conf.yaml\"), b\"{}\\n\")",
        "private_relative = f\"inputs/{index:04d}{extension}\"",
        "os.fchmod(descriptor, 0o600)",
        "stat.S_IMODE(info.st_mode) != 0o600",
        "make_owned_directory(input_directory)",
        "os.chmod(path, 0o700)",
        "stat.S_IMODE(info.st_mode) != 0o700",
        "\"HOME\": private_home",
        "\"XDG_CONFIG_HOME\": private_config",
        "\"XDG_CACHE_HOME\": private_cache",
        "upper.startswith(\"VACUUM_\")",
        "upper.startswith(\"DYLD_\")",
        "upper.startswith(\"LD_\")",
        "start_new_session=True",
        "spawning = True",
        "process = child",
        "previous_cleaning = cleaning",
        "cleaning = True",
        "return stop_error",
        "apply_pending_signals",
        "selectors.DefaultSelector",
        "combined output exceeded {OUTPUT_LIMIT} bytes",
        "class AdapterSignal(BaseException)",
        "os.killpg(process_group, signum)",
        "child left same-group descendants after leader exit",
        "child process group survived SIGKILL",
        "selector.close()",
        "shutil.rmtree(private_root)",
        "signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)",
        "signal.sigpending()",
        "signal.sigwait({queued[0]})",
        "<vacuum-private>",
        "PRIVATE_ROOT_PATTERN.sub",
        "ANSI_ESCAPE.sub",
        "Vacuum mutated selected file",
        "for key in (\"content\", \"device\", \"inode\", \"mode\", \"mtime\", \"ctime\")",
        "Vacuum violation status lacked a stable rule diagnostic",
        "raise SystemExit(outer_status)",
    ] {
        assert!(
            adapter.contains(required),
            "Vacuum adapter omits {required:?}"
        );
    }
    assert_eq!(verify.argv[3], token(ArgToken::ToolExecutable));
    assert_eq!(verify.argv[4], token(ArgToken::ExtraArgs));
    assert_eq!(verify.argv[5], literal("__VELVET_GLOVE_VACUUM_FILES__"));
    assert_eq!(verify.argv[6], token(ArgToken::Files));
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[2]);
    assert_eq!(verify.exit_codes.unexpected, UnexpectedExitPolicy::Failure);
    assert_eq!(verify.writes, WriteBehavior::None);
    assert_eq!(vacuum.phase_order, vec!["verify"]);
}

#[test]
fn betterleaks_batches_paths_and_separates_findings_from_failures() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let betterleaks = spec(&specs, "betterleaks");

    assert_eq!(betterleaks.phase_invocation, InvocationGranularity::Batch);
    assert!(
        betterleaks.workflows.is_empty(),
        "betterleaks uses compatibility translation"
    );
    let verify = betterleaks
        .phases
        .get("verify")
        .expect("betterleaks verify phase");
    assert_eq!(verify.program.as_deref(), Some("python"));
    assert_eq!(verify.argv.len(), 7);
    assert_eq!(verify.argv[0], literal("-I"));
    assert_eq!(verify.argv[1], literal("-c"));
    let ArgvElement::Literal(adapter) = &verify.argv[2] else {
        panic!("betterleaks adapter must be a literal Python program")
    };
    for required in [
        "sys.argv[2:].count(marker) != 1",
        "--redact",
        "--verbose",
        "--no-color",
        "--no-banner",
        "--exit-code",
        "--log-level",
        "--legacy-print",
        "--baseline-path",
        "--report-path",
        "--report-format",
        "--diagnostics",
        "--validation",
        "not lowered.startswith(\"--\")",
        "\"=\" not in argument",
        "--redact=100",
        "--verbose=true",
        "--no-color=true",
        "--no-banner=true",
        "--exit-code=10",
        "--log-level=fatal",
        "--legacy-print=true",
        "subprocess.Popen",
        "stderr=subprocess.PIPE",
        "class AdapterSignal(BaseException)",
        "pending_signal = None",
        "def stop_child(initial_signal=None)",
        "child.wait(timeout=1)",
        "child.send_signal(initial_signal)",
        "child.terminate()",
        "child.kill()",
        "for name in (\"SIGHUP\", \"SIGINT\", \"SIGTERM\")",
        "previous_signal_handlers[signum] = signal.signal(signum, forward_signal)",
        "except AdapterSignal as error:",
        "except BaseException:",
        "br\"^[0-9]{1,2}:[0-9]{2}(?:AM|PM) FTL \"",
        "b\"<time> FTL \"",
        "sys.stderr.buffer.write(stable_line)",
        "os.path.islink(path) or not os.path.isfile(path)",
        "with open(path, \"rb\")",
        "child_environment.pop(variable, None)",
        "BETTERLEAKS_CONFIG_TOML",
        "GITLEAKS_CONFIG_TOML",
        "env=child_environment",
        "returncode if returncode >= 0 else 2",
    ] {
        assert!(
            adapter.contains(required),
            "betterleaks adapter omits {required:?}"
        );
    }
    assert_eq!(verify.argv[3], token(ArgToken::ToolExecutable));
    assert_eq!(verify.argv[4], token(ArgToken::ExtraArgs));
    assert_eq!(
        verify.argv[5],
        literal("__VELVET_GLOVE_BETTERLEAKS_FILES__")
    );
    assert_eq!(verify.argv[6], token(ArgToken::Files));
    assert_exit_codes(&verify.exit_codes, &[0], &[10], &[1, 2, 126]);
}

#[test]
fn asciidoctor_adapter_distinguishes_document_issues_from_cli_failures() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let asciidoctor = spec(&specs, "asciidoctor");

    assert_eq!(asciidoctor.phase_invocation, InvocationGranularity::Batch);
    assert!(
        asciidoctor.workflows.is_empty(),
        "asciidoctor uses compatibility translation"
    );
    let verify = asciidoctor
        .phases
        .get("verify")
        .expect("asciidoctor verify phase");
    assert_eq!(verify.program.as_deref(), Some("ruby"));
    assert_eq!(verify.argv.len(), 7);
    assert_eq!(verify.argv[0], literal("-ropen3"));
    assert_eq!(verify.argv[1], literal("-e"));
    let ArgvElement::Literal(adapter) = &verify.argv[2] else {
        panic!("asciidoctor adapter must be a literal Ruby program")
    };
    for required in [
        "Open3.capture3",
        "--safe-mode=safe",
        "--failure-level=FATAL",
        "--failure-level=WARNING",
        "--out-file=/dev/null",
        "argument == \"--\"",
        "short = argument.start_with?(\"-\") && !argument.start_with?(\"--\")",
        "argument.include?(\"h\")",
        "argument.include?(\"V\")",
        "argument.include?(\"v\")",
        "argument.include?(\"q\")",
        "argument.include?(\"?\")",
        "argument.start_with?(\"--h\")",
        "argument.start_with?(\"--v\")",
        "argument.start_with?(\"--q\")",
        "would bypass validation or diagnostic evidence",
        "exit 2",
    ] {
        assert!(
            adapter.contains(required),
            "asciidoctor adapter omits {required:?}"
        );
    }
    assert_eq!(verify.argv[3], literal("--"));
    assert_eq!(verify.argv[4], token(ArgToken::ToolExecutable));
    assert_eq!(verify.argv[5], token(ArgToken::ExtraArgs));
    assert_eq!(verify.argv[6], token(ArgToken::Files));
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[2]);
}

#[test]
fn astro_adapter_requires_a_completed_workspace_check() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let astro = spec(&specs, "astro");

    assert_eq!(astro.workspace_indicator.as_deref(), Some("package.json"));
    assert_eq!(astro.phase_invocation, InvocationGranularity::Workspace);
    assert!(
        astro.workflows.is_empty(),
        "astro uses compatibility translation"
    );

    let verify = astro.phases.get("verify").expect("astro verify phase");
    assert_eq!(verify.program.as_deref(), Some("node"));
    assert_eq!(verify.argv.len(), 7);
    assert_eq!(verify.argv[0], literal("--input-type=commonjs"));
    assert_eq!(verify.argv[1], literal("-e"));
    let ArgvElement::Literal(adapter) = &verify.argv[2] else {
        panic!("astro adapter must be a literal Node program")
    };
    for required in [
        "spawnSync",
        "ASTRO_TELEMETRY_DISABLED: \"1\"",
        "CI: \"1\"",
        "delete environment.DEBUG",
        "--silent",
        "--noSync",
        "--no-watch",
        "--root",
        "--minimumSeverity=error",
        "--minimumFailingSeverity=error",
        "const maxBufferBytes = 16 * 1024 * 1024",
        "maxBuffer: maxBufferBytes",
        "Result \\(([1-9][0-9]*) files?\\)",
        "child.status === 0 && footer && errors === 0",
        "child.status === 1 && footer && errors > 0",
        "process.exit(2)",
        "would bypass controlled project validation",
    ] {
        assert!(
            adapter.contains(required),
            "astro adapter omits {required:?}"
        );
    }
    assert_eq!(verify.argv[3], literal("--"));
    assert_eq!(verify.argv[4], token(ArgToken::ToolExecutable));
    assert_eq!(verify.argv[5], literal("check"));
    assert_eq!(verify.argv[6], token(ArgToken::ExtraArgs));
    assert!(
        !verify
            .argv
            .iter()
            .any(|element| element == &token(ArgToken::Files)),
        "astro must scan its workspace instead of accepting ignored file arguments"
    );
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[2]);
}

#[test]
fn buf_format_adapter_locks_workspace_mutation_and_diff_completion() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let buf = spec(&specs, "bufFormat");

    assert_eq!(buf.workspace_indicator.as_deref(), Some("buf.yaml"));
    assert_eq!(buf.phase_invocation, InvocationGranularity::Workspace);
    assert!(
        buf.workflows.is_empty(),
        "buf format uses compatibility translation"
    );

    let format = buf.phases.get("format").expect("buf format phase");
    let verify = buf.phases.get("verify").expect("buf verify phase");
    assert_eq!(format.program.as_deref(), Some("python"));
    assert_eq!(verify.program.as_deref(), Some("python"));
    assert_eq!(format.writes, WriteBehavior::Workspace);
    assert_eq!(verify.writes, WriteBehavior::None);
    assert_eq!(format.argv.len(), 8);
    assert_eq!(verify.argv.len(), 8);
    assert_eq!(format.argv[0], literal("-I"));
    assert_eq!(format.argv[1], literal("-c"));
    let ArgvElement::Literal(adapter) = &format.argv[2] else {
        panic!("buf format adapter must be a literal Python program")
    };
    assert_eq!(verify.argv[2], format.argv[2]);
    for required in [
        "shutil.which(requested_tool)",
        "extra arguments are unsupported",
        "--disable-symlinks",
        "--error-format=text",
        "--log-format=text",
        "config",
        "ls-modules",
        "--format=json",
        "parse_module_scope",
        "validate_module_coverage",
        "buf.yaml module scope omits workspace proto files",
        "--write",
        "--diff",
        "--exit-code",
        "\"PATH\": \"/usr/bin:/bin\"",
        "name.startswith(\"BUF_\")",
        "\"DIFF_OPTIONS\"",
        "velvet-glove-buf-cache",
        "path_info.st_nlink != 1",
        "config_info.st_nlink != 1",
        "canonical v1 or v2 version header",
        "exactly one YAML document",
        "runner-hard-skipped directory",
        "combined output exceeded",
        "class AdapterSignal(BaseException)",
        "signal_group(signal.SIGKILL)",
        "diff output has no blocks",
        "unified-diff block has no hunk",
        "diff path escapes workspace",
        "\\t<mtime>\\n",
        "returncode == 100",
    ] {
        assert!(adapter.contains(required), "buf adapter omits {required:?}");
    }
    for (phase, mode) in [(format, "write"), (verify, "verify")] {
        assert_eq!(phase.argv[3], token(ArgToken::ToolExecutable));
        assert_eq!(phase.argv[4], literal(mode));
        assert_eq!(phase.argv[5], token(ArgToken::ExtraArgs));
        assert_eq!(phase.argv[6], literal("__VELVET_GLOVE_BUF_WORKSPACE__"));
        assert_eq!(phase.argv[7], token(ArgToken::Workspace));
    }
    assert_exit_codes(&format.exit_codes, &[0], &[], &[2]);
    assert_exit_codes(&verify.exit_codes, &[0], &[100], &[2]);
}

fn spec(specs: &std::collections::BTreeMap<String, ToolSpec>, key: &str) -> ToolSpec {
    specs
        .get(key)
        .unwrap_or_else(|| panic!("missing builtin: {key}"))
        .clone()
}

#[test]
fn ruff_builtin_matches_rust_spec() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let ruff = spec(&specs, "ruff");

    assert_eq!(ruff.id, "ruff");
    assert_eq!(ruff.display_name, "Ruff");
    assert_eq!(ruff.executable, "ruff");
    assert_eq!(
        ruff.install_hint.as_deref(),
        Some("install ruff with `brew install ruff` or add it to the project")
    );
    assert_eq!(
        ruff.files,
        FileSelection {
            include: vec![
                "*.py".into(),
                "**/*.py".into(),
                "*.pyi".into(),
                "**/*.pyi".into(),
            ],
            exclude: vec![],
        },
        "ruff include globs",
    );
    assert!(ruff.workspace_indicator.is_none());

    let format = ruff.phases.get("format").expect("format phase");
    assert_eq!(format.mode, PhaseMode::Format);
    assert_argv(
        format,
        vec![
            literal("format"),
            literal("--quiet"),
            literal("--force-exclude"),
            token(ArgToken::ExtraArgs),
            token(ArgToken::Files),
        ],
    );
    assert_exit_codes(&format.exit_codes, &[0], &[], &[2]);
    assert_eq!(format.writes, WriteBehavior::TargetFiles);

    let fix = ruff.phases.get("fix").expect("fix phase");
    assert_eq!(fix.mode, PhaseMode::Fix);
    assert_argv(
        fix,
        vec![
            literal("check"),
            literal("--force-exclude"),
            literal("--fix"),
            token(ArgToken::ExtraArgs),
            token(ArgToken::Files),
        ],
    );
    assert_exit_codes(&fix.exit_codes, &[0], &[1], &[2]);
    assert_eq!(fix.writes, WriteBehavior::TargetFiles);

    let verify = ruff.phases.get("verify").expect("verify phase");
    assert_eq!(verify.mode, PhaseMode::Verify);
    assert_argv(
        verify,
        vec![
            literal("check"),
            literal("--force-exclude"),
            token(ArgToken::ExtraArgs),
            token(ArgToken::Files),
        ],
    );
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[2]);
    assert_eq!(verify.writes, WriteBehavior::None);

    assert_eq!(ruff.phase_order, vec!["format", "fix", "verify"]);
}

#[test]
fn prettier_adapter_locks_completion_config_and_target_scope() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let prettier = spec(&specs, "prettier");

    assert_eq!(prettier.id, "prettier");
    assert_eq!(prettier.display_name, "Prettier");
    assert_eq!(prettier.executable, "prettier");
    let include = &prettier.files.include;
    assert!(include.contains(&"*.ts".to_string()));
    assert!(include.contains(&"**/*.tsx".to_string()));
    assert!(include.contains(&"*.vue".to_string()));
    assert!(include.contains(&"*.json".to_string()));
    assert!(include.contains(&"*.md".to_string()));
    assert!(include.contains(&"*.vue".to_string()));
    assert!(!include.contains(&"*.astro".to_string()));
    assert!(!include.contains(&"*.svelte".to_string()));

    let format = prettier.phases.get("format").expect("format");
    assert_eq!(format.program.as_deref(), Some("python"));
    assert_eq!(format.argv.len(), 9);
    assert_eq!(format.argv[0], literal("-I"));
    assert_eq!(format.argv[1], literal("-c"));
    let ArgvElement::Literal(adapter) = &format.argv[2] else {
        panic!("Prettier adapter must be a literal Python program")
    };
    for required in [
        "__VELVET_GLOVE_PRETTIER_FILES__",
        "phase not in (\"format\", \"verify\")",
        "SAFE_CLI_OPTIONS",
        "SAFE_CONFIG_OPTIONS",
        "option can bypass the controlled contract",
        "explicit config must be JSON",
        "validate_config(document",
        "validated_config_bytes",
        "overrides are unsupported because config is copied outside the project",
        "tempfile.gettempdir",
        "temporary root must be outside the project",
        "tempfile.mkdtemp",
        "os.fchmod(private_fd, 0o600)",
        "os.unlink(private_config_path)",
        "os.rmdir(private_config_dir)",
        "contains unsupported option",
        "info.st_nlink != 1",
        "selected paths repeat or alias one file",
        "child_args = [node, tool, f\"--config={config_for_child}\"]",
        "--list-different",
        "--write",
        "--log-level=error",
        "--no-editorconfig",
        "--ignore-path=/dev/null",
        "--with-node-modules",
        "--no-color",
        "environment = {}",
        "\"PATH\": \"/usr/bin:/bin\"",
        "\"LANG\": \"C\"",
        "\"LC_ALL\": \"C\"",
        "\"TZ\": \"UTC\"",
        "\"TERM\": \"dumb\"",
        "selectors.DefaultSelector",
        "start_new_session=True",
        "class AdapterSignal(BaseException)",
        "os.killpg(process.pid, signum)",
        "signal.SIGKILL",
        "def process_group_exists",
        "def sweep_process_group",
        "native Prettier left same-group descendants after child exit",
        "signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)",
        "def drain_blocked_signals",
        "sanitize_private_output(stdout_bytes)",
        "sanitize_private_output(stderr_bytes)",
        "sanitize_private_error(adapter_error)",
        "os.path.basename(candidate).startswith(\"velvet-glove-prettier-config-\")",
        "Keep handled signals blocked through diagnostics and process exit",
        "combined output exceeded",
        "def validated_list_different",
        "native_status == 0",
        "native_status != 1 or native_stderr",
        "native Prettier format preflight",
        "child_arguments(config_for_child, \"list\")",
        "child_arguments(config_for_child, \"write\")",
        "phase == \"verify\" and returncode == 1",
        "list-different output repeats files",
        "list-different output names a file outside the selection",
        "prettier: formatting differs:",
        "native Prettier exited {returncode} without valid completion evidence",
    ] {
        assert!(
            adapter.contains(required),
            "Prettier adapter omits {required:?}"
        );
    }
    assert!(
        adapter
            .find("child_arguments(config_for_child, \"list\")")
            .expect("format preflight invocation")
            < adapter
                .find("child_arguments(config_for_child, \"write\")")
                .expect("format write invocation"),
        "Prettier format must finish its read-only list-different preflight before write"
    );
    assert!(
        adapter
            .find("os.unlink(private_config_path)")
            .expect("private config cleanup")
            < adapter
                .find("signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)")
                .expect("cleanup signal cutoff")
            && adapter
                .find("signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)")
                .expect("cleanup signal cutoff")
                < adapter
                    .find("for signum, handler in previous_handlers.items()")
                    .expect("signal handler restoration"),
        "Prettier must retain handlers through private cleanup and establish a signal cutoff before restoring them"
    );
    assert!(!adapter.contains("os.environ.copy"));
    assert_eq!(format.argv[3], literal("node"));
    assert_eq!(format.argv[4], token(ArgToken::ToolExecutable));
    assert_eq!(format.argv[5], literal("format"));
    assert_eq!(format.argv[6], token(ArgToken::ExtraArgs));
    assert_eq!(format.argv[7], literal("__VELVET_GLOVE_PRETTIER_FILES__"));
    assert_eq!(format.argv[8], token(ArgToken::Files));
    assert_exit_codes(&format.exit_codes, &[0], &[], &[2]);
    assert_eq!(format.writes, WriteBehavior::TargetFiles);

    let verify = prettier.phases.get("verify").expect("verify");
    assert_eq!(verify.program.as_deref(), Some("python"));
    assert_eq!(verify.argv[0], literal("-I"));
    assert_eq!(verify.argv[1], literal("-c"));
    assert_eq!(verify.argv[2], format.argv[2]);
    assert_eq!(verify.argv[3], literal("node"));
    assert_eq!(verify.argv[4], token(ArgToken::ToolExecutable));
    assert_eq!(verify.argv[5], literal("verify"));
    assert_eq!(verify.argv[6], token(ArgToken::ExtraArgs));
    assert_eq!(verify.argv[7], literal("__VELVET_GLOVE_PRETTIER_FILES__"));
    assert_eq!(verify.argv[8], token(ArgToken::Files));
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[2]);
    assert_eq!(verify.writes, WriteBehavior::None);
    assert_eq!(prettier.phase_order, vec!["format", "verify"]);
}

#[test]
fn eslint_builtin_matches_rust_spec() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let eslint = spec(&specs, "eslint");

    assert_eq!(eslint.id, "eslint");
    assert_eq!(eslint.display_name, "ESLint");
    assert_eq!(eslint.executable, "eslint");
    assert_eq!(
        eslint.files.include,
        vec![
            "*.js".to_string(),
            "**/*.js".into(),
            "*.cjs".into(),
            "**/*.cjs".into(),
            "*.mjs".into(),
            "**/*.mjs".into(),
        ],
    );

    let fix = eslint.phases.get("fix").expect("fix");
    assert_eq!(fix.program.as_deref(), Some("python"));
    assert_eq!(fix.argv.len(), 9);
    assert_eq!(fix.argv[0], literal("-I"));
    assert_eq!(fix.argv[1], literal("-c"));
    let ArgvElement::Literal(adapter) = &fix.argv[2] else {
        panic!("eslint adapter must be a literal Python program")
    };
    for required in [
        "__VELVET_GLOVE_ESLINT_FILES__",
        "extra arguments are unsupported",
        ".velvet-glove-eslint.json",
        "object_pairs_hook=reject_duplicate_pairs",
        "--no-config-lookup",
        "--no-ignore",
        "--no-inline-config",
        "--fix-dry-run",
        "--suppressions-location=",
        "--cache-location=",
        "native ESLint write differed from dry-run output",
        "native ESLint final diagnostics differ from fix dry-run",
        "temporary root must be outside the project",
        "start_new_session=True",
        "signal.pthread_sigmask",
        "drain_blocked_signals",
        "global stdout_bytes",
    ] {
        assert!(
            adapter.contains(required),
            "ESLint adapter must contain {required:?}"
        );
    }
    for forbidden in [
        "os.environ.copy",
        "*.jsx",
        "*.ts",
        "plugins",
        "parserOptions",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "ESLint adapter must not contain {forbidden:?}"
        );
    }
    assert_eq!(fix.argv[3], literal("node"));
    assert_eq!(fix.argv[4], token(ArgToken::ToolExecutable));
    assert_eq!(fix.argv[5], literal("fix"));
    assert_eq!(fix.argv[6], token(ArgToken::ExtraArgs));
    assert_eq!(fix.argv[7], literal("__VELVET_GLOVE_ESLINT_FILES__"));
    assert_eq!(fix.argv[8], token(ArgToken::Files));
    assert_exit_codes(&fix.exit_codes, &[0], &[1], &[2]);
    assert_eq!(fix.writes, WriteBehavior::TargetFiles);

    let verify = eslint.phases.get("verify").expect("verify");
    assert_eq!(verify.program.as_deref(), Some("python"));
    assert_eq!(verify.argv[0], literal("-I"));
    assert_eq!(verify.argv[1], literal("-c"));
    assert_eq!(verify.argv[2], fix.argv[2]);
    assert_eq!(verify.argv[3], literal("node"));
    assert_eq!(verify.argv[4], token(ArgToken::ToolExecutable));
    assert_eq!(verify.argv[5], literal("verify"));
    assert_eq!(verify.argv[6], token(ArgToken::ExtraArgs));
    assert_eq!(verify.argv[7], literal("__VELVET_GLOVE_ESLINT_FILES__"));
    assert_eq!(verify.argv[8], token(ArgToken::Files));
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[2]);
    assert_eq!(verify.writes, WriteBehavior::None);
    assert_eq!(eslint.phase_order, vec!["fix", "verify"]);
}

#[test]
fn biome_adapter_distinguishes_source_issues_and_verifies_mutations() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let biome = spec(&specs, "biome");

    assert_eq!(biome.id, "biome");
    assert_eq!(biome.display_name, "Biome");
    assert_eq!(biome.executable, "biome");
    assert_eq!(biome.phase_invocation, InvocationGranularity::Batch);
    assert!(
        biome.workflows.is_empty(),
        "biome uses compatibility translation"
    );

    let fix = biome.phases.get("fix").expect("fix");
    assert_eq!(fix.program.as_deref(), Some("python"));
    assert_eq!(fix.argv.len(), 8);
    assert_eq!(fix.argv[0], literal("-I"));
    assert_eq!(fix.argv[1], literal("-c"));
    let ArgvElement::Literal(adapter) = &fix.argv[2] else {
        panic!("biome adapter must be a literal Python program")
    };
    for required in [
        "__VELVET_GLOVE_BIOME_FILES__",
        "phase not in (\"fix\", \"verify\")",
        "os.lstat(path)",
        "not os.path.isfile(path) or os.path.islink(path)",
        "use a non-controlled long --name=value option",
        "option_name.startswith(\"--vcs-\")",
        "option_name.startswith(\"--files-\")",
        "option_name.endswith(\"-linter-enabled\")",
        "\"--only\"",
        "\"--skip\"",
        "--reporter=json",
        "--max-diagnostics=none",
        "--error-on-warnings",
        "--no-errors-on-unmatched",
        "child_args.append(\"--write\")",
        "child_args.extend(fixed)",
        "child_args.append(\"--\")",
        "environment.pop(name, None)",
        "BIOME_BINARY",
        "BIOME_THREADS",
        "RAYON_NUM_THREADS",
        "NODE_OPTIONS",
        "NODE_PATH",
        "BIOME_CONFIG_PATH",
        "selectors.DefaultSelector",
        "start_new_session=True",
        "class AdapterSignal(BaseException)",
        "raise AdapterSignal(signum)",
        "raise AdapterSignal(pending_signal)",
        "os.killpg(process.pid, signum)",
        "child.wait(timeout=1)",
        "signal.SIGKILL",
        "except AdapterSignal as error:",
        "pending_signal is not None and adapter_error is None",
        "combined output exceeded",
        "report.get(\"command\") == \"check\"",
        "type(summary.get(field)) is int and summary[field] >= 0",
        "\"scannerDuration\"",
        "processed = summary[\"changed\"] + summary[\"unchanged\"]",
        "processed == len(files)",
        "summary[\"skipped\"] == 0",
        "summary[\"diagnosticsNotPrinted\"] == 0",
        "counts_agree",
        "category in (\"parse\", \"format\")",
        "category.startswith(\"lint/\")",
        "category.startswith(\"assist/\")",
        "category.startswith(\"suppressions/\")",
        "summary.pop(\"duration\", None)",
        "summary.pop(\"scannerDuration\", None)",
        "raise SystemExit(outer_status)",
    ] {
        assert!(
            adapter.contains(required),
            "biome adapter omits {required:?}"
        );
    }
    assert_eq!(fix.argv[3], token(ArgToken::ToolExecutable));
    assert_eq!(fix.argv[4], literal("fix"));
    assert_eq!(fix.argv[5], token(ArgToken::ExtraArgs));
    assert_eq!(fix.argv[6], literal("__VELVET_GLOVE_BIOME_FILES__"));
    assert_eq!(fix.argv[7], token(ArgToken::Files));
    assert_exit_codes(&fix.exit_codes, &[0], &[1], &[2]);
    assert_eq!(fix.writes, WriteBehavior::TargetFiles);

    let verify = biome.phases.get("verify").expect("verify");
    assert_eq!(verify.program.as_deref(), Some("python"));
    assert_eq!(verify.argv[0], literal("-I"));
    assert_eq!(verify.argv[1], literal("-c"));
    assert_eq!(verify.argv[2], fix.argv[2]);
    assert_eq!(verify.argv[3], token(ArgToken::ToolExecutable));
    assert_eq!(verify.argv[4], literal("verify"));
    assert_eq!(verify.argv[5], token(ArgToken::ExtraArgs));
    assert_eq!(verify.argv[6], literal("__VELVET_GLOVE_BIOME_FILES__"));
    assert_eq!(verify.argv[7], token(ArgToken::Files));
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[2]);
    assert_eq!(verify.writes, WriteBehavior::None);
    assert_eq!(biome.phase_order, vec!["fix", "verify"]);
}

#[test]
fn contextlint_adapter_locks_workspace_coverage_and_private_completion() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let contextlint = spec(&specs, "contextlint");

    assert_eq!(contextlint.display_name, "Contextlint");
    assert_eq!(contextlint.executable, "contextlint");
    assert_eq!(
        contextlint.workspace_indicator.as_deref(),
        Some("contextlint.config.json")
    );
    assert_eq!(
        contextlint.phase_invocation,
        InvocationGranularity::Workspace
    );
    assert_eq!(
        contextlint.files.include,
        vec![
            "*.[mM][dD]",
            "**/*.[mM][dD]",
            "*.[mM][aA][rR][kK][dD][oO][wW][nN]",
            "**/*.[mM][aA][rR][kK][dD][oO][wW][nN]",
        ]
    );
    assert_eq!(
        contextlint.files.exclude,
        vec![
            ".git/**",
            "**/.git/**",
            "node_modules/**",
            "**/node_modules/**",
            ".velvet-glove/**",
            "**/.velvet-glove/**",
        ]
    );
    assert!(
        contextlint.workflows.is_empty(),
        "Contextlint intentionally exercises compatibility-deferred translation"
    );
    let verify = contextlint.phases.get("verify").expect("verify phase");
    assert_eq!(verify.program.as_deref(), Some("python"));
    assert_eq!(verify.mode, PhaseMode::Verify);
    assert_eq!(verify.writes, WriteBehavior::None);
    assert_eq!(verify.argv.len(), 9);
    assert_eq!(verify.argv[0], literal("-I"));
    assert_eq!(verify.argv[1], literal("-c"));
    let ArgvElement::Literal(adapter) = &verify.argv[2] else {
        panic!("Contextlint adapter must be an evaluated literal Python program")
    };
    for invariant in [
        "contextlint.config.json must declare at least one rule",
        "payload[0] != MARKER",
        "inventory_workspace(workspace)",
        r#"GLOB_MAGIC = frozenset("*?()[]{}\\")"#,
        "reject_glob_magic_path(indicator, \"workspace indicator\")",
        "reject_glob_magic_path(path, \"workspace inventory path\")",
        "reject_glob_magic_path(absolute, \"runner candidate\")",
        "reject_glob_magic_path(base, \"TMPDIR\")",
        "workspace inventory rejects symbolic links",
        "private SEC-001 completion probe did not return its exact pinned result",
        "--permission",
        "--allow-fs-read=",
        "@contextlint/cli/dist/index.js",
        "@contextlint/core",
        "status == 1) != has_errors",
        "1 if report else 0",
    ] {
        assert!(
            adapter.contains(invariant),
            "Contextlint adapter lost structural invariant {invariant:?}"
        );
    }
    let symlink_rejection = adapter
        .find("workspace inventory rejects symbolic links")
        .expect("Contextlint unconditional symlink rejection");
    let excluded_directory_skip = adapter[symlink_rejection..]
        .find("if child.name in EXCLUDED_DIRECTORIES:")
        .map(|offset| symlink_rejection + offset)
        .expect("Contextlint physical excluded-directory skip");
    assert!(symlink_rejection < excluded_directory_skip);
    let active_workspace = adapter
        .find("active_workspace = workspace")
        .expect("Contextlint active workspace assignment");
    let indicator_magic = adapter
        .find("reject_glob_magic_path(indicator, \"workspace indicator\")")
        .expect("Contextlint indicator glob-magic rejection");
    assert!(active_workspace < indicator_magic);
    assert_eq!(verify.argv[3], literal("node"));
    assert_eq!(verify.argv[4], token(ArgToken::ToolExecutable));
    assert_eq!(verify.argv[5], token(ArgToken::WorkspaceIndicator));
    assert_eq!(verify.argv[6], token(ArgToken::ExtraArgs));
    assert_eq!(
        verify.argv[7],
        literal("__VELVET_GLOVE_CONTEXTLINT_FILES__")
    );
    assert_eq!(verify.argv[8], token(ArgToken::Files));
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[2]);
}

#[test]
fn cargo_fmt_builtin_uses_workspace_indicator() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let cargo_fmt = spec(&specs, "cargoFmt");

    assert_eq!(cargo_fmt.id, "cargo-fmt");
    assert_eq!(cargo_fmt.display_name, "cargo fmt");
    assert_eq!(cargo_fmt.executable, "cargo");
    assert_eq!(
        cargo_fmt.install_hint.as_deref(),
        Some(
            "install exact Rust/Cargo 1.97.1 with cargo-fmt/rustfmt 1.9.0 and Python 3 isolated-mode support"
        )
    );
    assert_eq!(cargo_fmt.workspace_indicator.as_deref(), Some("Cargo.lock"));
    assert_eq!(cargo_fmt.phase_invocation, InvocationGranularity::Workspace);

    let format = cargo_fmt.phases.get("format").expect("format");
    assert_eq!(format.program.as_deref(), Some("python"));
    assert_eq!(format.argv.len(), 10);
    assert_eq!(format.argv[0], literal("-I"));
    assert_eq!(format.argv[1], literal("-c"));
    let ArgvElement::Literal(adapter) = &format.argv[2] else {
        panic!("cargo-fmt adapter must be a literal Python program")
    };
    for required in [
        "class AdapterSignal(BaseException)",
        "OUTPUT_LIMIT = 16 * 1024 * 1024",
        "start_new_session=True",
        "os.killpg(process.pid, signum)",
        "def sweep_process_group():",
        "child left same-group descendants after leader exit",
        "reported_signal = error.signum",
        "signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)",
        "initialization_mask = signal.pthread_sigmask(",
        "signal.sigpending()",
        "signal.sigwait({queued[0]})",
        "combined output exceeded",
        "extra arguments are unsupported",
        "stat.S_ISLNK(initial.st_mode)",
        "initial.st_nlink != 1",
        "cargo, cargo-fmt, and rustfmt must resolve from one launcher directory",
        "the paired {companion} executable is unavailable beside rustc",
        "Cargo home configuration is unsupported",
        "Cargo invocation-path configuration is unsupported",
        "getattr(os, \"O_NOFOLLOW\", 0)",
        "def directory_snapshot(path):",
        "workspace contains more than 8192 validated directories",
        "--format-version=1",
        "--locked",
        "--offline",
        "--files-with-diff",
        "create_coverage_workspace()",
        "const _: ()=();",
        "coverage workspace directory topology or modes differ from the validated workspace",
        "cargo-fmt does not cover the complete physical Rust source set",
        "workspace lock is not a unique regular file",
        "workspace manifest is not a unique canonical regular file",
        "non-member Cargo path dependency is unsupported",
        "cargo-fmt wrote stderr, so status cannot be classified as a formatting issue",
        "cargo-fmt changed-file report differs from workspace bytes",
        "if before[relative][\"mode\"] != after[relative][\"mode\"]",
        "cannot remove controlled Cargo Fmt directory",
        "rollback_eligible = True",
        "ns=(opened.st_atime_ns, snapshot[\"mtime\"])",
        "for key in (\"content\", \"mode\", \"mtime\")",
        "restored workspace paths differ from the baseline",
        "restored workspace directory topology differs from the baseline",
        "set_directory_mode(",
        "added_directories",
        "removed_directories",
        "touched_directories",
        "restore_workspace(baseline, baseline_directories)",
        "rollback failed",
        "os.path.basename(candidate).startswith(\"velvet-glove-cargo-fmt-\")",
        "sanitize_private_output(raw_stdout)",
        "sanitize_private_output(raw_stderr)",
    ] {
        assert!(
            adapter.contains(required),
            "cargo-fmt adapter omits {required:?}"
        );
    }
    let handler_install = adapter
        .find("previous_handlers[signum] = signal.signal(signum, forward_signal)")
        .expect("Cargo Fmt handler installation");
    let private_root_create = adapter
        .find("private_root = tempfile.mkdtemp")
        .expect("Cargo Fmt private-root creation");
    let rollback_enable = adapter
        .find("rollback_eligible = True")
        .expect("Cargo Fmt rollback eligibility");
    let real_child = adapter
        .find("format_status, format_stdout, format_stderr = run_child(format_args)")
        .expect("Cargo Fmt real child");
    let owned_cleanup = adapter
        .rfind("shutil.rmtree(private_root)")
        .expect("Cargo Fmt final private-root cleanup");
    let signal_cutoff = adapter
        .find("blocked_mask = signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)")
        .expect("Cargo Fmt final signal cutoff");
    let final_rollback = adapter
        .rfind("restore_workspace(baseline, baseline_directories)")
        .expect("Cargo Fmt final rollback");
    let handler_restore = adapter
        .rfind("signal.signal(signum, handler)")
        .expect("Cargo Fmt final handler restoration");
    assert!(handler_install < private_root_create);
    assert!(rollback_enable < real_child);
    assert!(owned_cleanup < signal_cutoff);
    assert!(signal_cutoff < final_rollback);
    assert!(final_rollback < handler_restore);
    let normal_wait = adapter
        .find("status = process.wait()")
        .expect("Cargo Fmt normal child wait");
    let descendant_sweep = adapter[normal_wait..]
        .find("left_descendants = sweep_process_group()")
        .map(|offset| normal_wait + offset)
        .expect("Cargo Fmt normal-exit process-group sweep");
    let child_clear = adapter[descendant_sweep..]
        .find("process = None")
        .map(|offset| descendant_sweep + offset)
        .expect("Cargo Fmt normal child clear");
    assert!(normal_wait < descendant_sweep);
    assert!(descendant_sweep < child_clear);
    assert_eq!(adapter.matches("raw_stdout = b\"\"").count(), 5);
    assert_eq!(format.argv[3], token(ArgToken::ToolExecutable));
    assert_eq!(format.argv[4], literal("cargo-fmt"));
    assert_eq!(format.argv[5], literal("rustfmt"));
    assert_eq!(format.argv[6], literal("format"));
    assert_eq!(format.argv[7], token(ArgToken::ExtraArgs));
    assert_eq!(
        format.argv[8],
        literal("__VELVET_GLOVE_CARGO_FMT_WORKSPACE__")
    );
    assert_eq!(format.argv[9], token(ArgToken::WorkspaceIndicator));
    assert_exit_codes(&format.exit_codes, &[0], &[1], &[2]);
    assert_eq!(format.writes, WriteBehavior::Workspace);

    let verify = cargo_fmt.phases.get("verify").expect("verify");
    assert_eq!(verify.program.as_deref(), Some("python"));
    assert_eq!(verify.argv[0], literal("-I"));
    assert_eq!(verify.argv[1], literal("-c"));
    assert_eq!(verify.argv[2], format.argv[2]);
    assert_eq!(verify.argv[3], token(ArgToken::ToolExecutable));
    assert_eq!(verify.argv[4], literal("cargo-fmt"));
    assert_eq!(verify.argv[5], literal("rustfmt"));
    assert_eq!(verify.argv[6], literal("verify"));
    assert_eq!(verify.argv[7], token(ArgToken::ExtraArgs));
    assert_eq!(verify.argv[8], format.argv[8]);
    assert_eq!(verify.argv[9], token(ArgToken::WorkspaceIndicator));
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[2]);
    assert_eq!(verify.writes, WriteBehavior::None);
    assert_eq!(cargo_fmt.phase_order, vec!["format", "verify"]);
}

#[test]
fn cargo_clippy_builtin_carries_custom_messages_and_unexpected_policy() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let clippy = spec(&specs, "cargoClippy");

    assert_eq!(clippy.id, "cargo-clippy");
    assert_eq!(clippy.display_name, "cargo clippy");
    assert_eq!(clippy.executable, "cargo");
    assert_eq!(clippy.workspace_indicator.as_deref(), Some("Cargo.toml"));
    assert_eq!(clippy.phase_invocation, InvocationGranularity::Workspace);

    let fix = clippy.phases.get("fix").expect("fix");
    assert_eq!(fix.program.as_deref(), Some("python"));
    assert_eq!(fix.argv.len(), 9);
    assert_eq!(fix.argv[0], literal("-I"));
    assert_eq!(fix.argv[1], literal("-c"));
    let ArgvElement::Literal(adapter) = &fix.argv[2] else {
        panic!("cargo-clippy adapter must be a literal Python program")
    };
    for required in [
        "class AdapterSignal(BaseException)",
        "start_new_session=True",
        "os.killpg(process.pid, signum)",
        "combined output exceeded",
        "cargo and cargo-clippy must resolve from one launcher directory",
        "rustc, rustdoc, and clippy-driver must resolve from one toolchain directory",
        "Cargo home configuration is unsupported",
        "Cargo invocation-path configuration is unsupported",
        "cannot initialize controlled Cargo execution",
        "getattr(os, \"O_NOFOLLOW\", 0)",
        "for key in (\"device\", \"inode\", \"mode\", \"size\", \"mtime\", \"sha256\")",
        "CLIPPY_CONF_DIR",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC_WORKSPACE_WRAPPER",
        "--format-version=1",
        "--keep-going",
        "--message-format=json",
        "--cap-lints=allow",
        "-Dwarnings",
        "Cargo JSON has no unique terminal build-finished record",
        "Cargo did not compile every physical workspace Rust source",
        "rule_targets.isdisjoint(selected_artifact_filenames)",
        "selected_dep_info_count",
        "MachineApplicable",
        "os.replace",
        "rollback also failed",
    ] {
        assert!(
            adapter.contains(required),
            "cargo-clippy adapter omits {required:?}"
        );
    }
    assert!(!adapter.contains("\"--fix\""));
    assert_eq!(fix.argv[3], token(ArgToken::ToolExecutable));
    assert_eq!(fix.argv[4], literal("cargo-clippy"));
    assert_eq!(fix.argv[5], literal("fix"));
    assert_eq!(fix.argv[6], token(ArgToken::ExtraArgs));
    assert_eq!(
        fix.argv[7],
        literal("__VELVET_GLOVE_CARGO_CLIPPY_WORKSPACE__")
    );
    assert_eq!(fix.argv[8], token(ArgToken::WorkspaceIndicator));
    assert_exit_codes(&fix.exit_codes, &[0], &[1], &[2]);
    assert_eq!(fix.exit_codes.unexpected, UnexpectedExitPolicy::Failure);
    assert_eq!(fix.writes, WriteBehavior::Workspace);

    let verify = clippy.phases.get("verify").expect("verify");
    assert_eq!(verify.program.as_deref(), Some("python"));
    assert_eq!(verify.argv.len(), 9);
    assert_eq!(verify.argv[0], literal("-I"));
    assert_eq!(verify.argv[1], literal("-c"));
    assert_eq!(verify.argv[2], fix.argv[2]);
    assert_eq!(verify.argv[3], token(ArgToken::ToolExecutable));
    assert_eq!(verify.argv[4], literal("cargo-clippy"));
    assert_eq!(verify.argv[5], literal("verify"));
    assert_eq!(verify.argv[6], token(ArgToken::ExtraArgs));
    assert_eq!(
        verify.argv[7],
        literal("__VELVET_GLOVE_CARGO_CLIPPY_WORKSPACE__")
    );
    assert_eq!(verify.argv[8], token(ArgToken::WorkspaceIndicator));
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[2]);
    assert_eq!(verify.writes, WriteBehavior::None);

    assert_eq!(
        clippy.messages.issues_agent,
        "cargo clippy reports validated Rust source issues; inspect diagnostics at {{ diagnostics_path }}."
    );
    assert_eq!(
        clippy.messages.issues_changed_agent,
        "cargo clippy applied validated suggestions to {{ changed_files | join(\", \") }} and issues remain; re-read changed files, then inspect diagnostics at {{ diagnostics_path }}."
    );
}

#[test]
fn catalog_validator_rejects_unchecked_remedies_unless_fallback_is_explicit() {
    let mut legacy = ToolSpec {
        id: "legacy".into(),
        display_name: "Legacy".into(),
        executable: "legacy".into(),
        ..ToolSpec::default()
    };
    legacy.phases.insert(
        "format".into(),
        Phase {
            mode: PhaseMode::Format,
            writes: WriteBehavior::TargetFiles,
            ..Phase::default()
        },
    );
    let mut specs = BTreeMap::from([("legacy".into(), legacy.clone())]);
    let error = hookkit_pkl_config::validate_builtin_catalog(&specs)
        .expect_err("unchecked remedy must fail");
    assert!(error.to_string().contains("no authoritative final check"));

    legacy.unverified_remedy_fallback = Some("upstream has no read-only mode".into());
    specs.insert("legacy".into(), legacy);
    hookkit_pkl_config::validate_builtin_catalog(&specs).expect("explicit fallback rationale");

    let mut explicit = ToolSpec {
        id: "explicit".into(),
        display_name: "Explicit".into(),
        executable: "explicit".into(),
        ..ToolSpec::default()
    };
    explicit.workflows.insert(
        "format".into(),
        Workflow {
            remedy: Some(WorkflowCommand {
                writes: WriteBehavior::TargetFiles,
                ..WorkflowCommand::default()
            }),
            ..Workflow::default()
        },
    );
    let error = hookkit_pkl_config::validate_builtin_catalog(&BTreeMap::from([(
        "explicit".into(),
        explicit,
    )]))
    .expect_err("explicit remedy without a check must fail");
    assert!(error.to_string().contains("has no authoritative check"));
}

#[test]
fn formerly_mutating_only_tools_and_ruff_have_authoritative_workflows() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");

    let gofmt = spec(&specs, "goFmt");
    let gofmt_workflow = gofmt.workflows.get("format").expect("gofmt workflow");
    let gofmt_check = gofmt_workflow.check.as_ref().expect("gofmt check");
    let gofmt_remedy = gofmt_workflow.remedy.as_ref().expect("gofmt remedy");
    assert_eq!(gofmt_check.program.as_deref(), Some("python"));
    assert_eq!(gofmt_remedy.program.as_deref(), Some("python"));
    assert!(gofmt_check.issues_on_stdout);
    assert_eq!(gofmt_check.writes, WriteBehavior::None);
    assert_eq!(gofmt_remedy.writes, WriteBehavior::TargetFiles);
    assert_workflow_argv(
        gofmt_check,
        vec![
            literal("-I"),
            literal("-c"),
            gofmt_check.argv[2].clone(),
            token(ArgToken::ToolExecutable),
            literal("verify"),
            token(ArgToken::ExtraArgs),
            literal("__VELVET_GLOVE_GOFMT_FILES__"),
            token(ArgToken::Files),
        ],
    );
    assert_workflow_argv(
        gofmt_remedy,
        vec![
            literal("-I"),
            literal("-c"),
            gofmt_check.argv[2].clone(),
            token(ArgToken::ToolExecutable),
            literal("write"),
            token(ArgToken::ExtraArgs),
            literal("__VELVET_GLOVE_GOFMT_FILES__"),
            token(ArgToken::Files),
        ],
    );
    let gofmt_script = match &gofmt_check.argv[2] {
        ArgvElement::Literal(script) => script,
        other => panic!("gofmt adapter script was not literal: {other:?}"),
    };
    assert!(gofmt_script.contains("run_child([tool, \"-l\", *files])"));
    assert!(gofmt_script.contains("run_child([tool, \"-w\", *files])"));
    assert!(gofmt_script.contains("info.st_nlink != 1"));
    assert!(gofmt_script.contains("name.startswith(\"GO\")"));
    assert!(gofmt_script.contains(
        "try:\n        if pending_signal is not None:\n            raise AdapterSignal(pending_signal)\n        process = subprocess.Popen("
    ));
    assert!(gofmt_script.contains(
        "            start_new_session=True,\n        )\n        if pending_signal is not None:\n            raise AdapterSignal(pending_signal)"
    ));
    assert_eq!(gofmt_workflow.check_scope, CheckScope::TargetFiles);
    assert_eq!(gofmt_workflow.invocation, InvocationGranularity::Batch);
    let gofmt_phase = gofmt.phases.get("format").expect("gofmt immediate phase");
    assert_eq!(gofmt_phase.program.as_deref(), Some("python"));
    assert_eq!(gofmt_phase.argv, gofmt_remedy.argv);

    for (key, check_prefix) in [
        ("goFumpt", "-l"),
        ("goImports", "-l"),
        ("goLines", "--dry-run"),
    ] {
        let tool = spec(&specs, key);
        let workflow = tool.workflows.get("format").expect("format workflow");
        let check = workflow.check.as_ref().expect("format check");
        let remedy = workflow.remedy.as_ref().expect("format remedy");
        assert!(check.issues_on_stdout, "{key} stdout issue adapter");
        assert_eq!(check.writes, WriteBehavior::None);
        assert_eq!(remedy.writes, WriteBehavior::TargetFiles);
        assert!(
            matches!(check.argv.first(), Some(ArgvElement::Literal(value)) if value == check_prefix)
        );
        assert_eq!(workflow.check_scope, CheckScope::TargetFiles);
        assert_eq!(workflow.invocation, InvocationGranularity::Batch);
    }

    let tidy = spec(&specs, "gomodTidy");
    assert_eq!(
        tidy.files.include,
        vec![
            "*.go",
            "**/*.go",
            "go.mod",
            "**/go.mod",
            "go.sum",
            "**/go.sum"
        ]
    );
    let tidy_workflow = tidy.workflows.get("tidy").expect("tidy workflow");
    assert_eq!(tidy_workflow.check_scope, CheckScope::Workspace);
    assert_eq!(tidy_workflow.invocation, InvocationGranularity::Workspace);
    assert_workflow_argv(
        tidy_workflow.check.as_ref().expect("tidy check"),
        vec![
            literal("mod"),
            literal("tidy"),
            literal("-diff"),
            token(ArgToken::ExtraArgs),
        ],
    );
    assert_eq!(
        tidy_workflow.remedy.as_ref().expect("tidy remedy").writes,
        WriteBehavior::Workspace
    );

    let yq = spec(&specs, "yq");
    let yq = yq.workflows.get("format").expect("yq workflow");
    let yq_check = yq.check.as_ref().expect("yq check");
    assert_eq!(yq_check.program.as_deref(), Some("sh"));
    assert!(
        yq_check
            .argv
            .iter()
            .any(|arg| matches!(arg, ArgvElement::Token(ArgToken::ToolExecutable)))
    );
    assert_eq!(yq.invocation, InvocationGranularity::PerFile);

    let ruff = spec(&specs, "ruff");
    assert_eq!(ruff.workflow_order, vec!["lint", "format"]);
    let lint = ruff.workflows.get("lint").expect("lint workflow");
    let format = ruff.workflows.get("format").expect("format workflow");
    assert!(matches!(
        lint.check.as_ref().and_then(|check| check.argv.first()),
        Some(ArgvElement::Literal(value)) if value == "check"
    ));
    assert!(matches!(
        format.check.as_ref().and_then(|check| check.argv.first()),
        Some(ArgvElement::Literal(value)) if value == "format"
    ));
    assert!(
        format
            .check
            .as_ref()
            .expect("format check")
            .argv
            .iter()
            .any(|arg| matches!(arg, ArgvElement::Literal(value) if value == "--check"))
    );
}

#[test]
fn dclint_builtin_uses_a_fail_closed_fix_subset_adapter() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let dclint = spec(&specs, "dclint");
    let workflow = dclint.workflows.get("fix").expect("dclint fix workflow");
    let check = workflow.check.as_ref().expect("dclint check");
    let remedy = workflow.remedy.as_ref().expect("dclint remedy");

    assert_eq!(check.program.as_deref(), Some("python"));
    assert_eq!(remedy.program.as_deref(), Some("python"));
    assert!(check.issues_on_stdout);
    assert!(!remedy.issues_on_stdout);
    assert_eq!(check.writes, WriteBehavior::None);
    assert_eq!(remedy.writes, WriteBehavior::TargetFiles);
    assert_eq!(workflow.check_scope, CheckScope::TargetFiles);
    assert_eq!(workflow.invocation, InvocationGranularity::Batch);
    assert_workflow_argv(
        check,
        vec![
            literal("-I"),
            literal("-c"),
            check.argv[2].clone(),
            token(ArgToken::ToolExecutable),
            literal("verify"),
            token(ArgToken::ProjectRoot),
            token(ArgToken::ExtraArgs),
            literal("__VELVET_GLOVE_DCLINT_FILES__"),
            token(ArgToken::Files),
        ],
    );
    assert_workflow_argv(
        remedy,
        vec![
            literal("-I"),
            literal("-c"),
            check.argv[2].clone(),
            token(ArgToken::ToolExecutable),
            literal("fix"),
            token(ArgToken::ProjectRoot),
            token(ArgToken::ExtraArgs),
            literal("__VELVET_GLOVE_DCLINT_FILES__"),
            token(ArgToken::Files),
        ],
    );

    let adapter = match &check.argv[2] {
        ArgvElement::Literal(script) => script,
        other => panic!("dclint adapter script was not literal: {other:?}"),
    };
    for required in [
        "\"invalid-yaml\": False",
        "\"invalid-schema\": False",
        "\"unknown-error\": False",
        "\"no-version-field\": False",
        "UNSAFE_WRITE_RULES = {\"no-version-field\"}",
        "fixable_files.append(expected_file)",
        "native_arguments(tool, config_path, fixable_files, True)",
        "native_arguments(tool, config_path, files, False)",
        "write_by_path[path] != verify_by_path[path]",
        "scan_workspace(project_root)",
        "workspace_diff(",
        "restore_workspace(",
        "SKIPPED_DIRECTORIES = {\".git\", \".velvet-glove\", \"node\" + \"_modules\", \"target\"}",
        "MAX_WORKSPACE_FILES = 8192",
        "MAX_WORKSPACE_DIRECTORIES = 8192",
        "allowed_relatives = {",
        "outside the proven fixable subset",
        "\"x-properties\"",
        "\"models\"",
        "set(order) != TOP_LEVEL_KEYS",
        "rules[rule] = [1, safe_options]",
        "rules.setdefault(\"no-version-field\", 0)",
        "3.1.0 fixer can delete nested extension data",
        "is assigned to multiple groups",
        "temporary = os.path.realpath(temporary)",
        "safe_path_text(temporary, \"resolved TMPDIR\")",
        "environment[\"TMPDIR\"] = os.path.dirname(private_config_dir)",
        "TMPDIR must be outside the retained project root",
        "info.st_nlink != 1",
        "os.path.commonpath((project_root, requested)) == project_root",
        "flags |= os.O_NOFOLLOW",
        "snapshot[\"mode\"] != 0o600",
        "name.startswith(\"NODE_\")",
        "name.startswith(\"DYLD_\")",
        "name.startswith(\"LD_\")",
        "combined dclint output exceeded",
        "start_new_session=True",
        "signal.pthread_sigmask(signal.SIG_BLOCK, HANDLED_SIGNALS)",
        "set(signal.sigpending()).intersection(HANDLED_SIGNALS)",
        "signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)",
    ] {
        assert!(
            adapter.contains(required),
            "missing adapter guard: {required}"
        );
    }
    assert!(adapter.contains(
        "if pending_signal is not None:\n            raise AdapterSignal(pending_signal)\n        if not hasattr(signal, \"pthread_sigmask\")"
    ));
    assert!(adapter.contains(
        "                start_new_session=True,\n            )\n        finally:\n            signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)\n        if pending_signal is not None:\n            raise AdapterSignal(pending_signal)"
    ));

    let fix = dclint.phases.get("fix").expect("dclint immediate fix");
    let verify = dclint
        .phases
        .get("verify")
        .expect("dclint immediate verification");
    assert_eq!(fix.program.as_deref(), Some("python"));
    assert_eq!(verify.program.as_deref(), Some("python"));
    assert_eq!(fix.argv, remedy.argv);
    assert_eq!(verify.argv, check.argv);
    assert_eq!(fix.writes, WriteBehavior::TargetFiles);
    assert_eq!(verify.writes, WriteBehavior::None);
}

#[test]
fn builtin_catalog_audit_is_current() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    hookkit_pkl_config::validate_builtin_catalog(&specs).expect("valid builtin catalog");
    let generated = hookkit_pkl_config::render_builtin_catalog_markdown(&specs);
    if std::env::var_os("HOOKKIT_PRINT_BUILTIN_AUDIT").is_some() {
        eprintln!("HOOKKIT_AUDIT_BEGIN\n{generated}HOOKKIT_AUDIT_END");
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/builtin-deferred-workflow-audit.md");
    let checked_in = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    assert_eq!(checked_in, generated, "regenerate {}", path.display());
}
