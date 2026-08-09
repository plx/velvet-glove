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
fn prettier_builtin_has_expected_extensions() {
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

    let format = prettier.phases.get("format").expect("format");
    assert_argv(
        format,
        vec![
            literal("--write"),
            token(ArgToken::ExtraArgs),
            token(ArgToken::Files),
        ],
    );
    assert_exit_codes(&format.exit_codes, &[0], &[], &[]);
    assert_eq!(format.writes, WriteBehavior::TargetFiles);

    let verify = prettier.phases.get("verify").expect("verify");
    assert_argv(
        verify,
        vec![
            literal("--check"),
            token(ArgToken::ExtraArgs),
            token(ArgToken::Files),
        ],
    );
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[]);
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
            "*.jsx".into(),
            "**/*.jsx".into(),
            "*.ts".into(),
            "**/*.ts".into(),
            "*.tsx".into(),
            "**/*.tsx".into(),
        ],
    );

    let fix = eslint.phases.get("fix").expect("fix");
    assert_argv(
        fix,
        vec![
            literal("--fix"),
            token(ArgToken::ExtraArgs),
            token(ArgToken::Files),
        ],
    );
    assert_exit_codes(&fix.exit_codes, &[0], &[1], &[]);
    assert_eq!(fix.writes, WriteBehavior::TargetFiles);

    let verify = eslint.phases.get("verify").expect("verify");
    assert_argv(
        verify,
        vec![token(ArgToken::ExtraArgs), token(ArgToken::Files)],
    );
    assert_exit_codes(&verify.exit_codes, &[0], &[1], &[]);
}

#[test]
fn biome_builtin_matches_rust_spec() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let biome = spec(&specs, "biome");

    assert_eq!(biome.id, "biome");
    assert_eq!(biome.display_name, "Biome");
    assert_eq!(biome.executable, "biome");

    let fix = biome.phases.get("fix").expect("fix");
    assert_argv(
        fix,
        vec![
            literal("check"),
            literal("--write"),
            literal("--no-errors-on-unmatched"),
            token(ArgToken::ExtraArgs),
            token(ArgToken::Files),
        ],
    );
    assert_exit_codes(&fix.exit_codes, &[0], &[1], &[]);
    assert_eq!(fix.writes, WriteBehavior::TargetFiles);

    let verify = biome.phases.get("verify").expect("verify");
    assert_argv(
        verify,
        vec![
            literal("check"),
            literal("--no-errors-on-unmatched"),
            token(ArgToken::ExtraArgs),
            token(ArgToken::Files),
        ],
    );
}

#[test]
fn cargo_fmt_builtin_uses_workspace_indicator() {
    require_pkl!();
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    let cargo_fmt = spec(&specs, "cargoFmt");

    assert_eq!(cargo_fmt.id, "cargo-fmt");
    assert_eq!(cargo_fmt.display_name, "cargo fmt");
    assert_eq!(cargo_fmt.executable, "cargo");
    assert_eq!(cargo_fmt.workspace_indicator.as_deref(), Some("Cargo.toml"));

    let format = cargo_fmt.phases.get("format").expect("format");
    assert_argv(
        format,
        vec![
            literal("fmt"),
            literal("--manifest-path"),
            token(ArgToken::WorkspaceIndicator),
            token(ArgToken::ExtraArgs),
        ],
    );
    assert_exit_codes(&format.exit_codes, &[0], &[], &[]);
    assert_eq!(format.writes, WriteBehavior::MatchingGlobs);

    let verify = cargo_fmt.phases.get("verify").expect("verify");
    assert_argv(
        verify,
        vec![
            literal("fmt"),
            literal("--check"),
            literal("--manifest-path"),
            token(ArgToken::WorkspaceIndicator),
            token(ArgToken::ExtraArgs),
        ],
    );
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

    let fix = clippy.phases.get("fix").expect("fix");
    assert_argv(
        fix,
        vec![
            literal("clippy"),
            literal("--manifest-path"),
            token(ArgToken::WorkspaceIndicator),
            literal("--fix"),
            literal("--allow-dirty"),
            literal("--allow-staged"),
            literal("--quiet"),
            token(ArgToken::ExtraArgs),
        ],
    );
    assert_exit_codes(&fix.exit_codes, &[0], &[101], &[]);
    assert_eq!(fix.exit_codes.unexpected, UnexpectedExitPolicy::Failure);
    assert_eq!(fix.writes, WriteBehavior::MatchingGlobs);

    let verify = clippy.phases.get("verify").expect("verify");
    assert_argv(
        verify,
        vec![
            literal("clippy"),
            literal("--manifest-path"),
            token(ArgToken::WorkspaceIndicator),
            literal("--quiet"),
            token(ArgToken::ExtraArgs),
        ],
    );
    assert_exit_codes(&verify.exit_codes, &[0], &[101], &[]);

    assert_eq!(
        clippy.messages.issues_agent,
        "cargo clippy reports issues; inspect diagnostics at {{ diagnostics_path }}."
    );
    assert_eq!(
        clippy.messages.issues_changed_agent,
        "cargo clippy changed {{ changed_files | join(\", \") }} and issues remain; re-read changed files, then inspect diagnostics at {{ diagnostics_path }}."
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

    for (key, check_prefix) in [
        ("goFmt", "-l"),
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
