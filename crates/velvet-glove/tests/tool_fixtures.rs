//! Fixture-driven validation for Velvet Glove's built-in tool specs.
//!
//! The non-ignored tests fail closed on fixture discovery (including each
//! case's `case.json` semantic expectation) and prove, with a hermetic
//! executable, that every native protocol surface — immediate and deferred —
//! reaches a subprocess through the real `velvet-glove` binary. The opt-in
//! test additionally executes the host's real tools against the checked-in
//! cases and asserts semantics (per-file outcome, post-state), never bytes.

#[path = "support/process.rs"]
mod bounded_process;
mod support;

use bounded_process::{BoundedCommandError, run_with_timeout};
use hookkit_pkl_config::ToolSpec;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use support::native_events::{
    NativePostToolInput, PostToolUseBuilder, ProtocolSurface, canonical_project, shell_quote,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Surfaces every real-tool case runs on, in order. Deferred runs first so a
/// requested `expected/` capture reflects the flow the shipped plugin uses.
const FIXTURE_SURFACES: [FixtureSurface; 3] = [
    FixtureSurface::deferred(ProtocolSurface::Claude),
    FixtureSurface::immediate(ProtocolSurface::Claude),
    FixtureSurface::immediate(ProtocolSurface::Codex),
];
/// Surfaces the hermetic probe drives through the real binary.
const PROBE_SURFACES: [FixtureSurface; 4] = [
    FixtureSurface::immediate(ProtocolSurface::Claude),
    FixtureSurface::immediate(ProtocolSurface::Codex),
    FixtureSurface::immediate(ProtocolSurface::Antigravity),
    FixtureSurface::deferred(ProtocolSurface::Claude),
];
const REPORT_FORMAT_VERSION: u64 = 2;
const CASE_SPEC: &str = "case.json";
const FIXTURE_SESSION: &str = "test-session";
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const TIMEOUT_ENV: &str = "VELVET_GLOVE_FIXTURE_TIMEOUT_SECS";
const ARTIFACT_ENV: &str = "VELVET_GLOVE_FIXTURE_ARTIFACT_DIR";
const REQUIRED_TOOLS_ENV: &str = "VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS";
const SELECTED_TOOLS_ENV: &str = "VELVET_GLOVE_FIXTURE_TOOLS";
const CAPTURE_ENV: &str = "VELVET_GLOVE_FIXTURE_CAPTURE_EXPECTED";
const REPORT_PREFIX: &str = "VELVET_GLOVE_FIXTURE_JSON=";
const PROBE_SENTINEL_ENV: &str = "VELVET_GLOVE_FIXTURE_PROBE_SENTINEL";
const PROBE_DIR_ENV: &str = "VELVET_GLOVE_FIXTURE_PROBE_DIR";
static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Which hook flow a surface drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    /// `session-start-state` → `post-tool` → `turn-completion` (the plugin).
    Deferred,
    /// `post-tool-immediate`.
    Immediate,
}

/// One hook flow on one native protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureSurface {
    lane: Lane,
    protocol: ProtocolSurface,
}

impl FixtureSurface {
    const fn deferred(protocol: ProtocolSurface) -> Self {
        Self {
            lane: Lane::Deferred,
            protocol,
        }
    }

    const fn immediate(protocol: ProtocolSurface) -> Self {
        Self {
            lane: Lane::Immediate,
            protocol,
        }
    }
}

impl fmt::Display for FixtureSurface {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lane = match self.lane {
            Lane::Deferred => "deferred",
            Lane::Immediate => "immediate",
        };
        write!(formatter, "{lane}-{}", self.protocol.cli_name())
    }
}

/// Expected semantic outcome, ordered by severity for normal outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    Clean,
    AutoFixed,
    Manual,
    Operational,
}

impl Outcome {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "clean" => Ok(Self::Clean),
            "auto-fixed" => Ok(Self::AutoFixed),
            "manual" => Ok(Self::Manual),
            "operational" => Ok(Self::Operational),
            _ => Err(format!(
                "unknown outcome {value:?}; use clean, auto-fixed, manual, or operational"
            )),
        }
    }

    /// Maps a deferred `summary.json` per-file status.
    fn from_file_status(value: &str) -> Option<Self> {
        match value {
            "clean" => Some(Self::Clean),
            "auto-fixed" => Some(Self::AutoFixed),
            "manual-fixes-needed" => Some(Self::Manual),
            _ => None,
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Clean => "clean",
            Self::AutoFixed => "auto-fixed",
            Self::Manual => "manual",
            Self::Operational => "operational",
        })
    }
}

/// A case's `case.json`: what a correct spec does with its cited files.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CaseSpec {
    /// Aggregate (worst) outcome across the cited files and every non-cited
    /// file the run reports changed or blamed.
    outcome: Outcome,
    /// Exact per-file outcomes: cited files, or non-cited files the run is
    /// expected to change or blame.
    files: BTreeMap<String, Outcome>,
    immediate: bool,
    deferred: bool,
    note: Option<String>,
}

impl CaseSpec {
    fn new(outcome: Outcome) -> Self {
        Self {
            outcome,
            files: BTreeMap::new(),
            immediate: true,
            deferred: true,
            note: None,
        }
    }

    /// Parses `case.json` against the case's input files (case-relative
    /// paths) and returns the cited files with the expectation.
    fn parse(text: &str, inputs: &[String]) -> Result<(Vec<String>, Self), String> {
        let value: JsonValue =
            serde_json::from_str(text).map_err(|error| format!("{CASE_SPEC}: {error}"))?;
        let object = value
            .as_object()
            .ok_or_else(|| format!("{CASE_SPEC} must be a JSON object"))?;
        if let Some(key) = object.keys().find(|key| {
            !matches!(
                key.as_str(),
                "outcome" | "cite" | "files" | "immediate" | "deferred" | "note"
            )
        }) {
            return Err(format!("{CASE_SPEC}: unknown key {key:?}"));
        }
        let outcome = object
            .get("outcome")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| format!("{CASE_SPEC}: missing string `outcome`"))
            .and_then(|value| Outcome::parse(value).map_err(|e| format!("{CASE_SPEC}: {e}")))?;
        let cited = match object.get("cite") {
            None => default_cited(inputs)?,
            Some(cite) => parse_cite(cite, inputs)?,
        };
        let mut spec = Self::new(outcome);
        if let Some(files) = object.get("files") {
            let files = files
                .as_object()
                .ok_or_else(|| format!("{CASE_SPEC}: `files` must map case files to outcomes"))?;
            for (file, value) in files {
                if !inputs.contains(file) {
                    return Err(format!(
                        "{CASE_SPEC}: `files` names {file:?}, which is not an input file of the case"
                    ));
                }
                let file_outcome = value
                    .as_str()
                    .ok_or_else(|| format!("{CASE_SPEC}: outcome for {file:?} must be a string"))
                    .and_then(|value| {
                        Outcome::parse(value).map_err(|e| format!("{CASE_SPEC}: {e}"))
                    })?;
                if file_outcome == Outcome::Operational || outcome == Outcome::Operational {
                    return Err(format!(
                        "{CASE_SPEC}: per-file outcomes are clean, auto-fixed, or manual and \
                         require a non-operational aggregate"
                    ));
                }
                if file_outcome == Outcome::Clean && !cited.contains(file) {
                    return Err(format!(
                        "{CASE_SPEC}: `files` expects non-cited {file:?} to be clean, but a \
                         non-cited file is reported only when changed or blamed; expect \
                         auto-fixed or manual, or cite it"
                    ));
                }
                spec.files.insert(file.clone(), file_outcome);
            }
            let worst = spec.files.values().max().copied();
            let names_every_cited = cited.iter().all(|file| spec.files.contains_key(file));
            if worst > Some(outcome) || (names_every_cited && worst != Some(outcome)) {
                return Err(format!(
                    "{CASE_SPEC}: per-file outcomes disagree with aggregate `{outcome}` (when \
                     `files` names every cited file, also name the non-cited files that set \
                     the aggregate)"
                ));
            }
        }
        for (key, slot) in [
            ("immediate", &mut spec.immediate),
            ("deferred", &mut spec.deferred),
        ] {
            match object.get(key) {
                None => {}
                Some(JsonValue::Bool(value)) => *slot = *value,
                Some(_) => return Err(format!("{CASE_SPEC}: `{key}` must be a boolean")),
            }
        }
        match object.get("note") {
            None => {}
            Some(JsonValue::String(note)) if !note.trim().is_empty() => {
                spec.note = Some(note.clone());
            }
            Some(_) => return Err(format!("{CASE_SPEC}: `note` must be a non-empty string")),
        }
        if !spec.immediate && !spec.deferred {
            return Err(format!("{CASE_SPEC}: a case must run on at least one lane"));
        }
        if (!spec.immediate || !spec.deferred) && spec.note.is_none() {
            return Err(format!(
                "{CASE_SPEC}: explain why a lane is skipped in `note`"
            ));
        }
        Ok((cited, spec))
    }

    const fn runs_on(&self, lane: Lane) -> bool {
        match lane {
            Lane::Deferred => self.deferred,
            Lane::Immediate => self.immediate,
        }
    }
}

#[test]
fn fixture_inventory_is_non_empty_and_has_no_orphans() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let catalog = discover_fixture_catalog(&fixtures_root(), &specs)
        .unwrap_or_else(|error| panic!("fixture discovery failed: {error}"));

    let mut outcomes = BTreeMap::<String, usize>::new();
    for case in &catalog.cases {
        *outcomes.entry(case.expect.outcome.to_string()).or_default() += 1;
    }
    let report = serde_json::json!({
        "formatVersion": REPORT_FORMAT_VERSION,
        "kind": "inventory",
        "surfaces": surface_names(),
        "totals": {
            "tools": catalog.tool_count,
            "cases": catalog.cases.len(),
            "fixtureSurfaces": FIXTURE_SURFACES.len(),
            "protocolProbeSurfaces": PROBE_SURFACES.len(),
            "plannedSurfaceCases": catalog.cases.len() * FIXTURE_SURFACES.len(),
        },
        "expectedOutcomes": outcomes,
    });
    println!("{REPORT_PREFIX}{report}");
}

#[test]
fn probe_reaches_external_command_on_every_surface() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    let artifact_dir = configured_artifact_dir().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let commands = run_probe_matrix(timeout, artifact_dir.as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(commands, PROBE_SURFACES.len());
}

#[test]
fn subprocess_timeout_retains_partial_output() {
    let root = unique_temp_dir("velvet-glove-timeout-test");
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "printf started; printf diagnostic >&2; sleep 5"]);
    let result = run_with_timeout(
        &mut command,
        &[],
        Duration::from_millis(500),
        &root.join("capture"),
    );
    match result {
        Err(BoundedCommandError::Timeout { stdout, stderr, .. }) => {
            assert_eq!(stdout, b"started");
            assert_eq!(stderr, b"diagnostic");
        }
        other => panic!("expected a bounded timeout, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[cfg(unix)]
fn subprocess_timeout_terminates_descendants() {
    let root = unique_temp_dir("velvet-glove-timeout-descendant-test");
    let marker = root.join("descendant-survived");
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("(sleep 1; printf leaked > \"$1\") & printf started; wait")
        .arg("fixture-timeout")
        .arg(&marker);

    let result = run_with_timeout(
        &mut command,
        &[],
        Duration::from_millis(300),
        &root.join("capture"),
    );
    assert!(
        matches!(result, Err(BoundedCommandError::Timeout { .. })),
        "expected process-tree timeout, got {result:?}"
    );
    std::thread::sleep(Duration::from_millis(1_100));
    assert!(
        !marker.exists(),
        "a timed-out descendant continued mutating files"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn discovery_rejects_zero_cases_and_orphan_tools() {
    let empty_root = unique_temp_dir("velvet-glove-empty-fixtures");
    std::fs::create_dir_all(&empty_root).expect("empty fixture root");
    let empty_error = discover_fixture_catalog(&empty_root, &BTreeMap::new())
        .expect_err("empty fixture inventory must fail");
    assert!(empty_error.contains("zero tool directories"));

    let orphan_root = unique_temp_dir("velvet-glove-orphan-fixtures");
    let orphan_case = orphan_root.join("orphan-tool/example");
    std::fs::create_dir_all(&orphan_case).expect("orphan fixture root");
    std::fs::write(orphan_case.join("example.txt"), "fixture").expect("orphan input");
    let orphan_error = discover_fixture_catalog(&orphan_root, &BTreeMap::new())
        .expect_err("orphan fixture must fail");
    assert!(orphan_error.contains("orphan fixture tool directory"));

    let _ = std::fs::remove_dir_all(empty_root);
    let _ = std::fs::remove_dir_all(orphan_root);
}

#[test]
fn discovery_requires_case_specs_and_rejects_legacy_goldens() {
    let case = unique_temp_dir("velvet-glove-case-spec");
    std::fs::write(case.join("example.txt"), "fixture").expect("fixture input");
    let error = load_case(&case).expect_err("a case without case.json must fail closed");
    assert!(error.contains(CASE_SPEC), "{error}");

    std::fs::write(case.join(CASE_SPEC), r#"{"outcome": "clean"}"#).expect("case spec");
    let (cited, spec) = load_case(&case).expect("valid case");
    assert_eq!(cited, ["example.txt"]);
    assert_eq!(spec, CaseSpec::new(Outcome::Clean));
    assert_eq!(input_files(&case).unwrap(), [PathBuf::from("example.txt")]);

    for legacy in ["claude.json", "codex.stderr.txt", "antigravity.exit"] {
        std::fs::write(case.join(legacy), "{}").expect("legacy golden");
        let error = load_case(&case).expect_err("byte goldens must fail closed");
        assert!(error.contains(legacy), "{error}");
        std::fs::remove_file(case.join(legacy)).expect("remove legacy golden");
    }

    for dir in ["member", "expected/member"] {
        std::fs::create_dir_all(case.join(dir)).expect("nested fixture directory");
        std::fs::write(case.join(dir).join("lib.txt"), "fixture").expect("nested file");
    }
    let cite = |paths: &str| {
        let text = format!(r#"{{"outcome": "manual", "cite": [{paths}]}}"#);
        std::fs::write(case.join(CASE_SPEC), text).expect("case spec");
        load_case(&case)
    };
    let (cited, _) = cite(r#""member/lib.txt", "example.txt""#).expect("nested cite");
    assert_eq!(cited, ["member/lib.txt", "example.txt"]);
    for (paths, reason) in [
        (r#""expected/member/lib.txt""#, "expected/ holds post-state"),
        (r#""member/missing.txt""#, "not an input file"),
        (r#""case.json""#, "not an input file"),
    ] {
        let error = cite(paths).expect_err("invalid cite must fail closed");
        assert!(error.contains(reason), "{error}");
    }

    let _ = std::fs::remove_dir_all(case);
}

#[test]
fn case_specs_are_strict() {
    let inputs = ["example.a.json", "example.b.json", "member/lib.json"].map(str::to_owned);
    let (cited, mixed) = CaseSpec::parse(
        r#"{"outcome": "manual", "files": {"example.a.json": "clean", "example.b.json": "manual"}}"#,
        &inputs,
    )
    .expect("mixed multi-file case");
    assert_eq!(cited, ["example.a.json", "example.b.json"]);
    assert_eq!(mixed.files["example.a.json"], Outcome::Clean);
    let (_, deferred_only) = CaseSpec::parse(
        r#"{"outcome": "clean", "immediate": false, "note": "deferred-only behavior"}"#,
        &inputs,
    )
    .expect("explained lane skip");
    assert!(deferred_only.runs_on(Lane::Deferred) && !deferred_only.runs_on(Lane::Immediate));
    let (cited, _) = CaseSpec::parse(
        r#"{"outcome": "manual", "cite": ["member/lib.json"]}"#,
        &inputs,
    )
    .expect("explicit nested cite");
    assert_eq!(cited, ["member/lib.json"]);
    let (_, workspace) = CaseSpec::parse(
        r#"{"outcome": "auto-fixed", "files": {"example.a.json": "clean", "example.b.json": "clean", "member/lib.json": "auto-fixed"}}"#,
        &inputs,
    )
    .expect("a non-cited file the tool changes sets the aggregate");
    assert_eq!(workspace.files["member/lib.json"], Outcome::AutoFixed);

    for invalid in [
        "[]",
        "{}",
        r#"{"outcome": "broken"}"#,
        r#"{"outcome": "clean", "golden": "{}"}"#,
        r#"{"outcome": "clean", "files": {"other.json": "clean"}}"#,
        r#"{"outcome": "auto-fixed", "files": {"example.a.json": "manual"}}"#,
        r#"{"outcome": "manual", "files": {"example.a.json": "clean", "example.b.json": "clean"}}"#,
        r#"{"outcome": "operational", "files": {"example.a.json": "clean"}}"#,
        r#"{"outcome": "clean", "immediate": false}"#,
        r#"{"outcome": "clean", "immediate": false, "deferred": false, "note": "x"}"#,
        r#"{"outcome": "clean", "deferred": "no", "note": "x"}"#,
        r#"{"outcome": "clean", "cite": []}"#,
        r#"{"outcome": "clean", "cite": "example.a.json"}"#,
        r#"{"outcome": "clean", "cite": [1]}"#,
        r#"{"outcome": "clean", "cite": ["example.a.json", "example.a.json"]}"#,
        r#"{"outcome": "clean", "cite": ["/fixture/example.a.json"]}"#,
        r#"{"outcome": "auto-fixed", "files": {"member/lib.json": "clean"}}"#,
        r#"{"outcome": "auto-fixed", "files": {"example.a.json": "clean", "example.b.json": "clean"}}"#,
        r#"{"outcome": "manual", "cite": ["member/lib.json"], "files": {"example.a.json": "clean"}}"#,
    ] {
        assert!(
            CaseSpec::parse(invalid, &inputs).is_err(),
            "{invalid} must be rejected"
        );
    }
}

#[test]
fn deferred_checks_assert_per_file_semantics() {
    let project = Path::new("/fixture/workspace");
    let cited = ["example.a.json".to_owned(), "example.b.json".to_owned()];
    let summary = |a: &str, b: &str, operational: bool| {
        let problems = if operational {
            serde_json::json!({"p1": {"toolId": "fixture-tool", "message": "phase failed"}})
        } else {
            serde_json::json!({})
        };
        serde_json::json!({"result": {
            "files": {
                "/fixture/workspace/example.a.json": {"status": a},
                "/fixture/workspace/example.b.json": {"status": b},
            },
            "operationalProblems": problems,
        }})
    };
    let blocked = serde_json::json!({"decision": "block", "reason": "fix it"});
    let allowed = serde_json::json!({});
    let check = |spec: &CaseSpec, stop: &JsonValue, summary: &JsonValue| {
        check_deferred_run(spec, &cited, project, stop, summary)
    };

    let clean = CaseSpec::new(Outcome::Clean);
    assert!(check(&clean, &allowed, &summary("clean", "clean", false)).is_ok());
    let error = check(&clean, &allowed, &summary("clean", "clean", true)).unwrap_err();
    assert!(error.contains("unexpected operational"), "{error}");
    let partial = serde_json::json!({"result": {"files": {
        "/fixture/workspace/example.a.json": {"status": "clean"},
    }}});
    let error = check(&clean, &allowed, &partial).unwrap_err();
    assert!(error.contains("example.b.json: not assessed"), "{error}");

    let mut mixed = CaseSpec::new(Outcome::Manual);
    mixed
        .files
        .insert("example.a.json".to_owned(), Outcome::Clean);
    let summary_mixed = summary("clean", "manual-fixes-needed", false);
    assert!(check(&mixed, &blocked, &summary_mixed).is_ok());
    let error = check(&mixed, &allowed, &summary_mixed).unwrap_err();
    assert!(error.contains("decision=block"), "{error}");
    let misattributed = summary("manual-fixes-needed", "manual-fixes-needed", false);
    let error = check(&mixed, &blocked, &misattributed).unwrap_err();
    assert!(error.contains("example.a.json: expected clean"), "{error}");

    let auto_fixed = CaseSpec::new(Outcome::AutoFixed);
    let error = check(&auto_fixed, &allowed, &summary("clean", "clean", false)).unwrap_err();
    assert!(
        error.contains("expected auto-fixed, observed clean"),
        "{error}"
    );
    // A workspace remedy that rewrote a file beyond the cited ones counts.
    let mut workspace = summary("clean", "clean", false);
    workspace["result"]["files"]["/fixture/workspace/member/src/lib.rs"] =
        serde_json::json!({"status": "auto-fixed"});
    assert!(check(&auto_fixed, &allowed, &workspace).is_ok());
    let error = check(&clean, &allowed, &workspace).unwrap_err();
    assert!(
        error.contains("expected clean, observed auto-fixed"),
        "{error}"
    );

    // Non-cited files join the aggregate only when changed or blamed.
    let workspace = |status: &str| {
        serde_json::json!({"result": {"files": {
            "/fixture/workspace/example.a.json": {"status": "clean"},
            "/fixture/workspace/example.b.json": {"status": "clean"},
            "/fixture/workspace/member/lib.json": {"status": status},
        }}})
    };
    assert!(check(&clean, &allowed, &workspace("clean")).is_ok());
    let error = check(&clean, &allowed, &workspace("auto-fixed")).unwrap_err();
    assert!(
        error.contains("expected clean, observed auto-fixed"),
        "{error}"
    );
    assert!(check(&auto_fixed, &allowed, &workspace("auto-fixed")).is_ok());
    let manual = CaseSpec::new(Outcome::Manual);
    assert!(check(&manual, &blocked, &workspace("manual-fixes-needed")).is_ok());
    let mut spread = CaseSpec::new(Outcome::AutoFixed);
    for (file, outcome) in [
        ("example.a.json", Outcome::Clean),
        ("member/lib.json", Outcome::AutoFixed),
    ] {
        spread.files.insert(file.to_owned(), outcome);
    }
    assert!(check(&spread, &allowed, &workspace("auto-fixed")).is_ok());
    let error = check(&spread, &blocked, &workspace("manual-fixes-needed")).unwrap_err();
    assert!(
        error.contains("member/lib.json: expected auto-fixed, observed manual"),
        "{error}"
    );
    let error = check(&spread, &allowed, &summary("clean", "clean", false)).unwrap_err();
    assert!(
        error.contains("member/lib.json: expected auto-fixed, but summary.json does not report"),
        "{error}"
    );

    let operational = CaseSpec::new(Outcome::Operational);
    assert!(check(&operational, &blocked, &summary("clean", "clean", true)).is_ok());
    let error = check(&operational, &allowed, &summary("clean", "clean", false)).unwrap_err();
    assert!(error.contains("no operational problem"), "{error}");
    let error = check(
        &operational,
        &blocked,
        &summary("manual-fixes-needed", "clean", true),
    )
    .unwrap_err();
    assert!(error.contains("misclassified"), "{error}");
}

#[test]
fn deferred_manual_cases_assert_the_loop_guard() {
    let blocked = serde_json::json!({"decision": "block", "reason": "fix it"});
    let allowed = serde_json::json!({"systemMessage": "not blocking again"});
    let manual = CaseSpec::new(Outcome::Manual);
    assert!(check_loop_guard(&manual, Some(&allowed)).is_ok());
    let error = check_loop_guard(&manual, Some(&blocked)).unwrap_err();
    assert!(error.contains("blocked again"), "{error}");
    let error = check_loop_guard(&manual, None).unwrap_err();
    assert!(error.contains("no continuation"), "{error}");
    for outcome in [Outcome::Clean, Outcome::AutoFixed, Outcome::Operational] {
        assert!(check_loop_guard(&CaseSpec::new(outcome), None).is_ok());
    }
}

#[test]
fn immediate_checks_assert_coarse_output_shape() {
    let empty = serde_json::json!({});
    let context = serde_json::json!({"hookSpecificOutput": {"additionalContext": "x"}});
    assert!(check_immediate_stdout(Outcome::Clean, &empty).is_ok());
    assert!(check_immediate_stdout(Outcome::Clean, &context).is_err());
    let user_note = serde_json::json!({"systemMessage": "velvet-glove: not reporting issues outside the files this call changed"});
    assert!(check_immediate_stdout(Outcome::Clean, &user_note).is_ok());
    assert!(check_immediate_stdout(Outcome::AutoFixed, &context).is_ok());
    assert!(check_immediate_stdout(Outcome::Manual, &empty).is_err());
    assert!(check_immediate_stdout(Outcome::Operational, &empty).is_ok());
    assert!(check_immediate_stdout(Outcome::Operational, &context).is_ok());
}

#[test]
fn post_state_compares_expected_unchanged_and_captures_on_request() {
    let case_dir = unique_temp_dir("velvet-glove-post-state-case");
    let project = unique_temp_dir("velvet-glove-post-state-project");
    std::fs::write(case_dir.join("example.txt"), "before\n").expect("fixture input");
    std::fs::write(project.join("example.txt"), "after\n").expect("post-run file");
    let mut case = fixture_case("post-state");
    case.directory = case_dir.clone();

    case.expect = CaseSpec::new(Outcome::Manual);
    let error = verify_post_state(&case, &project, false).unwrap_err();
    assert!(
        error.contains("example.txt") && error.contains(CAPTURE_ENV),
        "{error}"
    );
    case.expect = CaseSpec::new(Outcome::AutoFixed);
    assert!(verify_post_state(&case, &project, false).is_ok());

    assert!(verify_post_state(&case, &project, true).is_ok());
    assert_eq!(
        std::fs::read_to_string(case_dir.join("expected/example.txt")).unwrap(),
        "after\n"
    );
    std::fs::write(project.join("example.txt"), "different\n").expect("drifted file");
    let error = verify_post_state(&case, &project, true).unwrap_err();
    assert!(error.contains("post-run file mismatch"), "{error}");

    let _ = std::fs::remove_dir_all(case_dir);
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn requested_failure_artifacts_copy_actionable_evidence() {
    let source = unique_temp_dir("velvet-glove-artifact-source");
    let artifact_root = unique_temp_dir("velvet-glove-artifact-root");
    std::fs::create_dir_all(source.join("workspace/.velvet-glove")).expect("fixture workspace");
    std::fs::create_dir_all(source.join("evidence")).expect("fixture evidence");
    std::fs::write(
        source.join("workspace/.velvet-glove/post-tool-use.pkl"),
        "config",
    )
    .expect("fixture config");
    std::fs::write(source.join("evidence/input.json"), "{}").expect("fixture input evidence");
    std::fs::write(source.join("evidence/stderr"), "failure").expect("fixture stderr evidence");
    let case = fixture_case("failure-case");

    let retained = retain_failure(&source, &artifact_root, &case, FIXTURE_SURFACES[0])
        .expect("retain requested artifacts");
    assert_eq!(
        std::fs::read_to_string(retained.join("evidence/input.json")).unwrap(),
        "{}"
    );
    assert_eq!(
        std::fs::read_to_string(retained.join("evidence/stderr")).unwrap(),
        "failure"
    );
    assert!(
        retained
            .join("workspace/.velvet-glove/post-tool-use.pkl")
            .is_file()
    );

    let _ = std::fs::remove_dir_all(source);
    let _ = std::fs::remove_dir_all(artifact_root);
}

#[test]
#[cfg(unix)]
fn setup_failures_are_retained_when_requested() {
    let fixture_root = unique_temp_dir("velvet-glove-setup-failure-fixture");
    let artifact_root = unique_temp_dir("velvet-glove-setup-failure-artifacts");
    std::os::unix::fs::symlink("missing-target", fixture_root.join("example.txt"))
        .expect("fixture symlink");
    let mut case = fixture_case("setup-failure");
    case.directory = fixture_root.clone();
    let options = HarnessOptions {
        timeout: Duration::from_secs(1),
        artifact_dir: Some(artifact_root.clone()),
        required_tools: RequiredTools::default(),
        selected_tools: None,
        capture_expected: false,
    };

    let outcome = run_fixture_case(&case, FIXTURE_SURFACES[0], &options);
    assert!(matches!(outcome.status, FixtureStatus::Fail(_)));
    let retained = outcome.artifacts.expect("retained setup failure artifacts");
    assert!(retained.join("evidence/outcome.json").is_file());
    assert!(!retained.join("workspace/example.txt").exists());

    let _ = std::fs::remove_dir_all(fixture_root);
    let _ = std::fs::remove_dir_all(artifact_root);
}

#[test]
fn temporary_directories_are_unique_across_parallel_callers() {
    let paths = (0..16)
        .map(|_| {
            std::thread::spawn(|| {
                (0..16)
                    .map(|_| unique_temp_dir("velvet-glove-parallel-temp-test"))
                    .collect::<Vec<_>>()
            })
        })
        .flat_map(|thread| thread.join().expect("temporary directory worker"))
        .collect::<Vec<_>>();
    let unique = paths.iter().collect::<BTreeSet<_>>();

    assert_eq!(unique.len(), paths.len());
    assert!(paths.iter().all(|path| path.is_dir()));
    for path in paths {
        let _ = std::fs::remove_dir(path);
    }
}

#[test]
fn required_tools_reject_unknown_fixture_ids() {
    let required = RequiredTools {
        all: false,
        names: BTreeSet::from(["known-tool".to_owned(), "typo-tool".to_owned()]),
    };
    let available = BTreeSet::from(["known-tool".to_owned()]);

    let error = required
        .validate(&available)
        .expect_err("unknown required tools must fail closed");
    assert!(error.contains("typo-tool"));
    assert!(error.contains(REQUIRED_TOOLS_ENV));
}

#[test]
fn tool_selection_rejects_empty_and_unknown_lists() {
    let available = BTreeSet::from(["jq".to_owned(), "go-fmt".to_owned()]);
    assert_eq!(select_fixture_tools(None, &available).unwrap(), available);
    assert_eq!(
        select_fixture_tools(Some(" jq, jq "), &available).unwrap(),
        BTreeSet::from(["jq".to_owned()])
    );
    for value in ["", " , ", "jq,typo", "all"] {
        let error = select_fixture_tools(Some(value), &available).unwrap_err();
        assert!(error.contains(SELECTED_TOOLS_ENV), "{error}");
    }
}

#[test]
fn selection_precedes_availability_and_required_tools_fail_closed() {
    let mut selected = fixture_case("missing-tool");
    selected.spec.executable = "/velvet-glove-test/nonexistent-tool".to_owned();
    let mut excluded = fixture_case("must-not-run");
    excluded.tool = "excluded-tool".to_owned();
    excluded.spec.executable = "/bin/sh".to_owned();
    let catalog = FixtureCatalog {
        tool_count: 2,
        cases: vec![selected, excluded],
    };
    let selection = BTreeSet::from(["fixture-tool".to_owned()]);
    let mut options = HarnessOptions {
        timeout: Duration::from_secs(1),
        artifact_dir: None,
        required_tools: RequiredTools {
            all: true,
            names: BTreeSet::new(),
        },
        selected_tools: None,
        capture_expected: false,
    };
    let lanes = FIXTURE_SURFACES.len();
    let outcomes = run_selected_fixtures(&catalog, &selection, &options);
    assert!(
        outcomes[..lanes]
            .iter()
            .all(|o| matches!(o.status, FixtureStatus::Fail(_)))
    );
    assert!(outcomes[lanes..].iter().all(|o| matches!(
        &o.status, FixtureStatus::Skip(reason) if reason.code == "not-selected"
    )));
    let report = build_report(&catalog, &outcomes, PROBE_SURFACES.len());
    assert_eq!(report["totals"]["failed"], lanes);
    assert_eq!(report["skipReasons"]["not-selected"], lanes);

    options.required_tools = RequiredTools::default();
    let outcomes = run_selected_fixtures(&catalog, &selection, &options);
    assert!(outcomes[..lanes].iter().all(|o| matches!(
        &o.status, FixtureStatus::Skip(reason) if reason.code == "executable-unavailable"
    )));
    let required = RequiredTools {
        all: false,
        names: BTreeSet::from(["excluded-tool".to_owned()]),
    };
    assert!(required.validate(&selection).is_err());
}

#[test]
fn requested_probe_failure_artifacts_are_retained() {
    let artifact_root = unique_temp_dir("velvet-glove-probe-artifact-root");
    let error = run_probe_attempt(PROBE_SURFACES[0], Some(&artifact_root), |root| {
        std::fs::create_dir_all(root.join("evidence"))
            .map_err(|error| format!("create probe evidence: {error}"))?;
        std::fs::write(root.join("evidence/input.json"), "{\"probe\":true}")
            .map_err(|error| format!("write probe evidence: {error}"))?;
        Err("intentional probe failure".to_owned())
    })
    .expect_err("failing probe must return an error");

    assert!(error.contains("intentional probe failure"));
    assert!(error.contains("retained probe artifacts"));
    let retained = sorted_entries(&artifact_root.join("probe/immediate-claude"))
        .expect("retained probe directories");
    assert_eq!(retained.len(), 1);
    assert_eq!(
        std::fs::read_to_string(retained[0].path().join("evidence/input.json")).unwrap(),
        "{\"probe\":true}"
    );
    assert!(
        retained[0]
            .path()
            .join("evidence/probe-outcome.json")
            .is_file()
    );

    let _ = std::fs::remove_dir_all(artifact_root);
}

#[test]
fn machine_report_reconciles_totals_and_structured_skips() {
    let catalog = FixtureCatalog {
        tool_count: 1,
        cases: vec![fixture_case("case-a"), fixture_case("case-b")],
    };
    let [deferred, claude, codex] = FIXTURE_SURFACES;
    let outcomes = vec![
        FixtureOutcome::pass(&catalog.cases[0], deferred),
        FixtureOutcome::pass(&catalog.cases[0], claude),
        FixtureOutcome::skipped(
            &catalog.cases[0],
            codex,
            SkipReason {
                code: "executable-unavailable",
                detail: "missing fixture-tool".to_owned(),
            },
        ),
        FixtureOutcome::failed(&catalog.cases[1], deferred, "outcome mismatch"),
        FixtureOutcome::pass(&catalog.cases[1], claude),
        FixtureOutcome::pass(&catalog.cases[1], codex),
    ];

    let report = build_report(&catalog, &outcomes, PROBE_SURFACES.len());
    let totals = &report["totals"];
    assert_eq!(report["formatVersion"], REPORT_FORMAT_VERSION);
    assert_eq!(
        report["surfaces"],
        serde_json::json!(["deferred-claude", "immediate-claude", "immediate-codex"])
    );
    assert_eq!(totals["plannedSurfaceCases"], 6);
    assert_eq!(totals["attemptedSurfaceCases"], 5);
    assert_eq!(totals["passed"], 4);
    assert_eq!(totals["skipped"], 1);
    assert_eq!(totals["failed"], 1);
    assert_eq!(report["bySurface"]["deferred-claude"]["failed"], 1);
    assert_eq!(report["skipReasons"]["executable-unavailable"], 1);
    assert_eq!(report["outcomes"][2]["surface"], "immediate-codex");
    assert_eq!(report["outcomes"][2]["lane"], "immediate");
    assert_eq!(report["outcomes"][2]["expected"], "clean");
    assert_eq!(
        report["outcomes"][2]["reason"]["code"],
        "executable-unavailable"
    );
    assert_eq!(
        totals["plannedSurfaceCases"].as_u64(),
        Some(
            totals["passed"].as_u64().unwrap()
                + totals["skipped"].as_u64().unwrap()
                + totals["failed"].as_u64().unwrap()
        )
    );
}

fn fixture_case(name: &str) -> FixtureCase {
    FixtureCase {
        tool: "fixture-tool".to_owned(),
        case: name.to_owned(),
        directory: PathBuf::new(),
        cited: vec!["example.txt".to_owned()],
        expect: CaseSpec::new(Outcome::Clean),
        pkl_property: "fixtureTool".to_owned(),
        spec: ToolSpec::default(),
    }
}

#[test]
#[ignore = "real-tool compatibility lane; requires controlled PATH versions"]
fn run_all_tool_fixtures() {
    let options = HarnessOptions::from_environment().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(options.timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let catalog = discover_fixture_catalog(&fixtures_root(), &specs)
        .unwrap_or_else(|error| panic!("fixture discovery failed: {error}"));
    let selected = select_fixture_tools(options.selected_tools.as_deref(), &catalog.tool_ids())
        .unwrap_or_else(|error| panic!("{error}"));
    options
        .required_tools
        .validate(&selected)
        .unwrap_or_else(|error| panic!("{error}"));
    let probe_commands = run_probe_matrix(options.timeout, options.artifact_dir.as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(probe_commands > 0, "probe executed zero external commands");

    let outcomes = run_selected_fixtures(&catalog, &selected, &options);

    let report = build_report(&catalog, &outcomes, probe_commands);
    print_outcomes(&outcomes);
    println!("{REPORT_PREFIX}{report}");
    if let Some(root) = &options.artifact_dir {
        let path = write_report(root, &report).unwrap_or_else(|error| panic!("{error}"));
        println!("machine-readable report: {}", path.display());
    }

    let planned = catalog.cases.len() * FIXTURE_SURFACES.len();
    assert_eq!(
        outcomes.len(),
        planned,
        "surface-case totals must reconcile"
    );
    let attempted = outcomes
        .iter()
        .filter(|outcome| !matches!(outcome.status, FixtureStatus::Skip(_)))
        .count();
    assert!(attempted > 0, "real-tool lane attempted zero surface cases");

    let failures = outcomes
        .iter()
        .filter_map(|outcome| match &outcome.status {
            FixtureStatus::Fail(reason) => Some(format!(
                "{}/{} ({}): {reason}",
                outcome.tool, outcome.case, outcome.surface
            )),
            FixtureStatus::Pass | FixtureStatus::Skip(_) => None,
        })
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "{} fixture surface(s) failed:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

fn run_selected_fixtures(
    catalog: &FixtureCatalog,
    selected: &BTreeSet<String>,
    options: &HarnessOptions,
) -> Vec<FixtureOutcome> {
    let mut availability = BTreeMap::<String, Result<(), Vec<String>>>::new();
    let mut outcomes = Vec::with_capacity(catalog.cases.len() * FIXTURE_SURFACES.len());
    for case in &catalog.cases {
        for surface in FIXTURE_SURFACES {
            if !selected.contains(&case.tool) {
                outcomes.push(FixtureOutcome::skipped(
                    case,
                    surface,
                    SkipReason {
                        code: "not-selected",
                        detail: format!("outside {SELECTED_TOOLS_ENV}; not validated by this run"),
                    },
                ));
                continue;
            }
            if !case.expect.runs_on(surface.lane) {
                outcomes.push(FixtureOutcome::skipped(
                    case,
                    surface,
                    SkipReason {
                        code: "excluded-by-case",
                        detail: case.expect.note.clone().unwrap_or_default(),
                    },
                ));
                continue;
            }
            let available = availability
                .entry(case.tool.clone())
                .or_insert_with(|| check_tool_programs(&case.spec));
            match available {
                Ok(()) => outcomes.push(run_fixture_case(case, surface, options)),
                Err(programs) if options.required_tools.requires(&case.tool) => {
                    outcomes.push(FixtureOutcome::failed(
                        case,
                        surface,
                        format!("required prerequisite unavailable: {}", programs.join(", ")),
                    ));
                }
                Err(programs) => outcomes.push(FixtureOutcome::skipped(
                    case,
                    surface,
                    SkipReason {
                        code: "executable-unavailable",
                        detail: format!("programs not found on PATH: {}", programs.join(", ")),
                    },
                )),
            }
        }
    }
    outcomes
}

fn select_fixture_tools(
    value: Option<&str>,
    available: &BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    let Some(value) = value else {
        return Ok(available.clone());
    };
    let selected = value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if selected.is_empty() {
        return Err(format!(
            "{SELECTED_TOOLS_ENV} must select at least one tool"
        ));
    }
    let unknown = selected.difference(available).cloned().collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(format!(
            "{SELECTED_TOOLS_ENV} names tools without fixture cases: {}",
            unknown.join(", ")
        ));
    }
    Ok(selected)
}

#[derive(Debug)]
struct HarnessOptions {
    timeout: Duration,
    artifact_dir: Option<PathBuf>,
    required_tools: RequiredTools,
    selected_tools: Option<String>,
    /// Write missing `expected/` post-state for human review.
    capture_expected: bool,
}

impl HarnessOptions {
    fn from_environment() -> Result<Self, String> {
        Ok(Self {
            timeout: configured_timeout()?,
            artifact_dir: configured_artifact_dir()?,
            required_tools: RequiredTools::from_environment()?,
            selected_tools: std::env::var_os(SELECTED_TOOLS_ENV)
                .map(|value| {
                    value
                        .into_string()
                        .map_err(|_| format!("{SELECTED_TOOLS_ENV} must be UTF-8"))
                })
                .transpose()?,
            capture_expected: match std::env::var_os(CAPTURE_ENV) {
                None => false,
                Some(value) if value == "0" || value.is_empty() => false,
                Some(value) if value == "1" => true,
                Some(value) => return Err(format!("{CAPTURE_ENV} must be 0 or 1, got {value:?}")),
            },
        })
    }
}

#[derive(Debug, Default)]
struct RequiredTools {
    all: bool,
    names: BTreeSet<String>,
}

impl RequiredTools {
    fn from_environment() -> Result<Self, String> {
        let Some(value) = std::env::var_os(REQUIRED_TOOLS_ENV) else {
            return Ok(Self::default());
        };
        let value = value
            .into_string()
            .map_err(|_| format!("{REQUIRED_TOOLS_ENV} must be UTF-8"))?;
        let mut required = Self::default();
        for name in value
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if name == "all" {
                required.all = true;
            } else {
                required.names.insert(name.to_owned());
            }
        }
        if !required.all && required.names.is_empty() && !value.trim().is_empty() {
            return Err(format!(
                "{REQUIRED_TOOLS_ENV} must be `all` or a comma-separated tool-id list"
            ));
        }
        Ok(required)
    }

    fn requires(&self, tool: &str) -> bool {
        self.all || self.names.contains(tool)
    }

    fn validate(&self, available: &BTreeSet<String>) -> Result<(), String> {
        let unknown = self
            .names
            .difference(available)
            .cloned()
            .collect::<Vec<_>>();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{REQUIRED_TOOLS_ENV} names tools without selected fixture cases: {}",
                unknown.join(", ")
            ))
        }
    }
}

#[derive(Debug)]
struct FixtureCatalog {
    tool_count: usize,
    cases: Vec<FixtureCase>,
}

impl FixtureCatalog {
    fn tool_ids(&self) -> BTreeSet<String> {
        self.cases.iter().map(|case| case.tool.clone()).collect()
    }
}

#[derive(Debug)]
struct FixtureCase {
    tool: String,
    case: String,
    directory: PathBuf,
    /// Case-relative files the synthetic tool call says the agent wrote.
    cited: Vec<String>,
    expect: CaseSpec,
    pkl_property: String,
    spec: ToolSpec,
}

fn builtin_index() -> Result<BTreeMap<String, (String, ToolSpec)>, String> {
    let specs = hookkit_pkl_config::builtin_specs()
        .map_err(|error| format!("load builtin tool specs: {error}"))?;
    let mut by_id = BTreeMap::new();
    for (property, spec) in specs {
        let id = spec.id.clone();
        if by_id.insert(id.clone(), (property, spec)).is_some() {
            return Err(format!("duplicate builtin tool id {id}"));
        }
    }
    Ok(by_id)
}

fn discover_fixture_catalog(
    root: &Path,
    specs: &BTreeMap<String, (String, ToolSpec)>,
) -> Result<FixtureCatalog, String> {
    if !root.is_dir() {
        return Err(format!(
            "required fixture root is not a directory: {root:?}"
        ));
    }
    let mut cases = Vec::new();
    let mut tool_count = 0;
    for tool_entry in sorted_entries(root)? {
        let name = tool_entry.file_name();
        if name == OsStr::new("README.md") && tool_entry.path().is_file() {
            continue;
        }
        let file_type = tool_entry
            .file_type()
            .map_err(|error| format!("file type for {:?}: {error}", tool_entry.path()))?;
        if !file_type.is_dir() {
            return Err(format!(
                "orphan fixture-root entry is not a tool directory: {:?}",
                tool_entry.path()
            ));
        }
        let tool = name
            .into_string()
            .map_err(|name| format!("tool directory name is not UTF-8: {name:?}"))?;
        let Some((property, spec)) = specs.get(&tool) else {
            return Err(format!(
                "orphan fixture tool directory has no builtin spec: {tool}"
            ));
        };
        if !spec.enabled {
            return Err(format!(
                "orphan fixture tool directory targets disabled spec: {tool}"
            ));
        }
        tool_count += 1;

        let before = cases.len();
        for case_entry in sorted_entries(&tool_entry.path())? {
            let case_name = case_entry.file_name();
            if case_name == OsStr::new("README.md") && case_entry.path().is_file() {
                continue;
            }
            let file_type = case_entry
                .file_type()
                .map_err(|error| format!("file type for {:?}: {error}", case_entry.path()))?;
            if !file_type.is_dir() {
                return Err(format!(
                    "orphan entry in tool fixture directory {tool}: {:?}",
                    case_entry.path()
                ));
            }
            let case = case_name
                .into_string()
                .map_err(|name| format!("case directory name is not UTF-8: {name:?}"))?;
            let directory = case_entry.path();
            let (cited, expect) =
                load_case(&directory).map_err(|error| format!("{tool}/{case}: {error}"))?;
            cases.push(FixtureCase {
                tool: tool.clone(),
                case,
                directory,
                cited,
                expect,
                pkl_property: property.clone(),
                spec: spec.clone(),
            });
        }
        if cases.len() == before {
            return Err(format!(
                "fixture tool directory contains zero cases: {tool}"
            ));
        }
    }
    if tool_count == 0 {
        return Err("fixture discovery found zero tool directories".to_owned());
    }
    if cases.is_empty() {
        return Err("fixture discovery found zero cases".to_owned());
    }
    Ok(FixtureCatalog { tool_count, cases })
}

fn sorted_entries(path: &Path) -> Result<Vec<std::fs::DirEntry>, String> {
    let entries = std::fs::read_dir(path)
        .map_err(|error| format!("read fixture directory {path:?}: {error}"))?;
    let mut entries = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read entry in fixture directory {path:?}: {error}"))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    Ok(entries)
}

#[derive(Debug)]
struct FixtureOutcome {
    tool: String,
    case: String,
    surface: FixtureSurface,
    expected: Outcome,
    status: FixtureStatus,
    artifacts: Option<PathBuf>,
}

#[derive(Debug)]
enum FixtureStatus {
    Pass,
    Skip(SkipReason),
    Fail(String),
}

#[derive(Debug)]
struct SkipReason {
    code: &'static str,
    detail: String,
}

impl FixtureOutcome {
    fn new(case: &FixtureCase, surface: FixtureSurface, status: FixtureStatus) -> Self {
        Self {
            tool: case.tool.clone(),
            case: case.case.clone(),
            surface,
            expected: case.expect.outcome,
            status,
            artifacts: None,
        }
    }

    fn pass(case: &FixtureCase, surface: FixtureSurface) -> Self {
        Self::new(case, surface, FixtureStatus::Pass)
    }

    fn skipped(case: &FixtureCase, surface: FixtureSurface, reason: SkipReason) -> Self {
        Self::new(case, surface, FixtureStatus::Skip(reason))
    }

    fn failed(case: &FixtureCase, surface: FixtureSurface, reason: impl Into<String>) -> Self {
        Self::new(case, surface, FixtureStatus::Fail(reason.into()))
    }

    fn as_json(&self) -> JsonValue {
        let (status, detail) = match &self.status {
            FixtureStatus::Pass => ("pass", JsonValue::Null),
            FixtureStatus::Skip(reason) => (
                "skip",
                serde_json::json!({"code": reason.code, "detail": reason.detail}),
            ),
            FixtureStatus::Fail(reason) => ("fail", serde_json::json!({"detail": reason})),
        };
        serde_json::json!({
            "tool": self.tool,
            "case": self.case,
            "surface": self.surface.to_string(),
            "lane": match self.surface.lane {
                Lane::Deferred => "deferred",
                Lane::Immediate => "immediate",
            },
            "protocol": self.surface.protocol.cli_name(),
            "expected": self.expected.to_string(),
            "status": status,
            "reason": detail,
            "artifacts": self.artifacts.as_ref().map(|path| path.to_string_lossy()),
        })
    }
}

struct FixtureWorkspace {
    root: PathBuf,
    project: PathBuf,
    evidence: PathBuf,
}

struct FixtureSetupFailure {
    root: PathBuf,
    detail: String,
}

impl FixtureWorkspace {
    fn prepare(case: &FixtureCase, surface: FixtureSurface) -> Result<Self, FixtureSetupFailure> {
        let root = unique_temp_dir(&format!(
            "velvet-glove-fixture-{}-{}-{surface}",
            case.tool, case.case
        ));
        let project = root.join("workspace");
        let evidence = root.join("evidence");
        if let Err(error) = std::fs::create_dir_all(&project) {
            return Err(FixtureSetupFailure {
                root,
                detail: format!("create fixture workspace {project:?}: {error}"),
            });
        }
        if let Err(error) = std::fs::create_dir_all(&evidence) {
            return Err(FixtureSetupFailure {
                root,
                detail: format!("create fixture evidence {evidence:?}: {error}"),
            });
        }
        if let Err(error) = copy_fixture_inputs(&case.directory, &project) {
            return Err(FixtureSetupFailure {
                root,
                detail: error,
            });
        }
        Ok(Self {
            root,
            project,
            evidence,
        })
    }
}

fn run_fixture_case(
    case: &FixtureCase,
    surface: FixtureSurface,
    options: &HarnessOptions,
) -> FixtureOutcome {
    let workspace = match FixtureWorkspace::prepare(case, surface) {
        Ok(workspace) => workspace,
        Err(failure) => {
            return finalize_fixture_outcome(
                &failure.root,
                case,
                surface,
                options,
                FixtureOutcome::failed(case, surface, failure.detail),
            );
        }
    };
    let lane = write_pkl_config(&workspace.project, &case.tool, &case.pkl_property)
        .and_then(|()| run_lane(case, surface, options.timeout, &workspace));
    // Only a semantically passing run may seed `expected/` for review.
    let capture = options.capture_expected && lane.is_ok();
    let post_state = verify_post_state(case, &workspace.project, capture);
    let outcome = match (lane, post_state) {
        (Ok(()), Ok(())) => FixtureOutcome::pass(case, surface),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => FixtureOutcome::failed(case, surface, error),
        (Err(lane), Err(post_state)) => {
            FixtureOutcome::failed(case, surface, format!("{lane}\n{post_state}"))
        }
    };
    finalize_fixture_outcome(&workspace.root, case, surface, options, outcome)
}

fn finalize_fixture_outcome(
    root: &Path,
    case: &FixtureCase,
    surface: FixtureSurface,
    options: &HarnessOptions,
    mut outcome: FixtureOutcome,
) -> FixtureOutcome {
    let mut preserve_temporary_evidence = false;

    if matches!(outcome.status, FixtureStatus::Fail(_)) {
        let evidence = root.join("evidence");
        if let Err(error) = std::fs::create_dir_all(&evidence)
            .map_err(|error| format!("create failure evidence directory: {error}"))
            .and_then(|()| write_json(&evidence.join("outcome.json"), &outcome.as_json()))
        {
            append_failure(&mut outcome, format!("write failure evidence: {error}"));
        }
        if let Some(artifact_root) = &options.artifact_dir {
            match retain_failure(root, artifact_root, case, surface) {
                Ok(path) => {
                    outcome.artifacts = Some(path.clone());
                    if let Err(error) =
                        write_json(&path.join("evidence/outcome.json"), &outcome.as_json())
                    {
                        append_failure(&mut outcome, format!("update retained outcome: {error}"));
                    }
                }
                Err(error) => {
                    preserve_temporary_evidence = true;
                    append_failure(
                        &mut outcome,
                        format!(
                            "{error}; preserved temporary evidence at {}",
                            root.display()
                        ),
                    );
                }
            }
        }
    }
    if !preserve_temporary_evidence {
        let _ = std::fs::remove_dir_all(root);
    }
    outcome
}

/// The synthetic tool call citing the case's files, as the agent's edit.
fn post_tool_input(
    case: &FixtureCase,
    protocol: ProtocolSurface,
    project: &Path,
) -> Result<NativePostToolInput, String> {
    let mut input = PostToolUseBuilder::new(protocol, project, &case.cited[0]).identity(
        FIXTURE_SESSION,
        "test-turn",
        format!("{}-tool", case.tool),
    );
    if case.cited.len() > 1 {
        let command = case
            .cited
            .iter()
            .map(|path| format!("printf fixture > {}", shell_quote(path)))
            .collect::<Vec<_>>()
            .join("; ");
        input = input.tool(
            "Bash",
            serde_json::json!({ "command": command }),
            serde_json::json!({ "exit_code": 0 }),
        );
    }
    input.build()
}

/// Runs one lane for a case and checks its lane-specific semantics.
fn run_lane(
    case: &FixtureCase,
    surface: FixtureSurface,
    timeout: Duration,
    workspace: &FixtureWorkspace,
) -> Result<(), String> {
    let input = post_tool_input(case, surface.protocol, &workspace.project)?;
    match surface.lane {
        Lane::Deferred => {
            let run = run_deferred_flow(&workspace.root, &workspace.project, &input, timeout, &[])?;
            check_deferred_run(
                &case.expect,
                &case.cited,
                &workspace.project,
                &run.stop,
                &run.summary,
            )?;
            check_loop_guard(&case.expect, run.continuation.as_ref())
        }
        Lane::Immediate => {
            let stdout = run_hook(
                "post-tool-immediate",
                None,
                &input,
                input.bytes(),
                timeout,
                &workspace.evidence,
                &[],
            )?;
            check_immediate_stdout(case.expect.outcome, &stdout)
        }
    }
}

/// Coarse channel shape only: message wording belongs to the UX templates.
/// A clean case may still carry a user-only `systemMessage` (for example the
/// note about issues in files the call did not change), but nothing may reach
/// the agent.
fn check_immediate_stdout(outcome: Outcome, stdout: &JsonValue) -> Result<(), String> {
    let silent = stdout.as_object().is_some_and(serde_json::Map::is_empty);
    let reaches_agent = stdout.get("hookSpecificOutput").is_some()
        || stdout.get("decision").is_some()
        || stdout.get("reason").is_some();
    match outcome {
        Outcome::Clean if reaches_agent => Err(format!(
            "clean case must say nothing to the agent, got {stdout}"
        )),
        Outcome::AutoFixed | Outcome::Manual if silent => Err(format!(
            "{outcome} case produced no native output, so the agent is never told"
        )),
        _ => Ok(()),
    }
}

struct DeferredRun {
    stop: JsonValue,
    summary: JsonValue,
    /// Output of the retry Stop (`stop_hook_active`) run after a block.
    continuation: Option<JsonValue>,
}

/// Drives the plugin's hook sequence with a private state directory under
/// `root`: `session-start-state`, one `post-tool` observation, then Stop,
/// and, when that Stop blocks, the agent's retry Stop.
fn run_deferred_flow(
    root: &Path,
    project: &Path,
    post_tool: &NativePostToolInput,
    timeout: Duration,
    env: &[(&str, &OsStr)],
) -> Result<DeferredRun, String> {
    if post_tool.surface() != ProtocolSurface::Claude {
        return Err(format!(
            "deferred fixtures drive Claude lifecycle events, not {}",
            post_tool.surface()
        ));
    }
    let state = root.join("state");
    let evidence = root.join("evidence");
    let lifecycle = |event: &str, fields: JsonValue| {
        let mut input = serde_json::json!({
            "session_id": post_tool.session_id(),
            "transcript_path": project.join(".fixture-transcript.jsonl"),
            "cwd": project,
            "hook_event_name": event,
        });
        if let (Some(input), Some(fields)) = (input.as_object_mut(), fields.as_object()) {
            input.extend(fields.clone());
        }
        input.to_string().into_bytes()
    };
    let env_file = root.join("claude-env");
    let mut start_env = env.to_vec();
    start_env.push(("CLAUDE_ENV_FILE", env_file.as_os_str()));
    run_hook(
        "session-start-state",
        Some(&state),
        post_tool,
        &lifecycle(
            "SessionStart",
            serde_json::json!({"source": "startup", "model": "fixture-model"}),
        ),
        timeout,
        &evidence.join("session-start-state"),
        &start_env,
    )?;
    run_hook(
        "post-tool",
        Some(&state),
        post_tool,
        post_tool.bytes(),
        timeout,
        &evidence.join("post-tool"),
        env,
    )?;
    let stop = run_hook(
        "turn-completion",
        Some(&state),
        post_tool,
        &lifecycle(
            "Stop",
            serde_json::json!({"stop_hook_active": false, "last_assistant_message": "done"}),
        ),
        timeout,
        &evidence.join("turn-completion"),
        env,
    )?;
    let summaries = files_named(&state, "summary.json")?;
    let [summary] = summaries.as_slice() else {
        return Err(format!(
            "expected one deferred summary.json under {state:?}, found {}; Stop stdout: {stop}",
            summaries.len()
        ));
    };
    let summary = std::fs::read(summary)
        .map_err(|error| format!("read {summary:?}: {error}"))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|error| format!("parse {summary:?}: {error}"))
        })?;
    // A block makes the agent continue; its next Stop carries
    // `stop_hook_active`. With no edits in between, the same issues remain.
    let continuation = if is_block(&stop) {
        Some(run_hook(
            "turn-completion",
            Some(&state),
            post_tool,
            &lifecycle(
                "Stop",
                serde_json::json!({"stop_hook_active": true, "last_assistant_message": "tried"}),
            ),
            timeout,
            &evidence.join("turn-completion-continuation"),
            env,
        )?)
    } else {
        None
    };
    Ok(DeferredRun {
        stop,
        summary,
        continuation,
    })
}

fn is_block(stop: &JsonValue) -> bool {
    stop.get("decision").and_then(JsonValue::as_str) == Some("block")
}

/// The loop guard: after a manual block, the agent's unchanged retry (a Stop
/// with `stop_hook_active`) must be allowed rather than blocked again.
fn check_loop_guard(expect: &CaseSpec, continuation: Option<&JsonValue>) -> Result<(), String> {
    match (expect.outcome, continuation) {
        (Outcome::Manual, None) => Err("manual case: no continuation Stop was run".to_owned()),
        (Outcome::Manual, Some(stop)) if is_block(stop) => Err(format!(
            "loop guard: a Stop with stop_hook_active=true after the block blocked again: {stop}"
        )),
        _ => Ok(()),
    }
}

/// Runs one hook command with the native input's harness environment and
/// returns its stdout JSON. Any nonzero exit is a failure.
fn run_hook(
    command_name: &str,
    state_dir: Option<&Path>,
    identity: &NativePostToolInput,
    payload: &[u8],
    timeout: Duration,
    evidence: &Path,
    env: &[(&str, &OsStr)],
) -> Result<JsonValue, String> {
    std::fs::create_dir_all(evidence)
        .map_err(|error| format!("create evidence {evidence:?}: {error}"))?;
    std::fs::write(evidence.join("input.json"), payload)
        .map_err(|error| format!("write {command_name} input evidence: {error}"))?;
    let binary = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary);
    command.args(["--harness", identity.surface().cli_name()]);
    if let Some(state_dir) = state_dir {
        command.arg("--state-dir").arg(state_dir);
    }
    command.arg(command_name);
    identity.configure_command(&mut command);
    for (name, value) in env {
        command.env(name, value);
    }
    let output = run_with_timeout(&mut command, payload, timeout, evidence)
        .map_err(|error| format!("{command_name} ({}): {error}", identity.surface()))?;
    let _ = std::fs::write(
        evidence.join("exit.txt"),
        format!("{}\n", output.status.code().unwrap_or(-1)),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "{command_name} exited {:?}\nstdout:\n{stdout}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_str(&stdout)
        .map_err(|error| format!("{command_name} stdout is not JSON ({error}):\n{stdout}"))
}

/// Compares a deferred run's `summary.json` per-file statuses and operational
/// problems with the case's expectation. The aggregate covers the cited files
/// plus any non-cited file reported auto-fixed or manual-fixes-needed. Stop
/// output is checked only for its coarse block decision; its wording belongs
/// to the UX templates.
fn check_deferred_run(
    expect: &CaseSpec,
    cited: &[String],
    project: &Path,
    stop: &JsonValue,
    summary: &JsonValue,
) -> Result<(), String> {
    let result = summary
        .get("result")
        .ok_or("summary.json has no `result` object")?;
    let aliases = workspace_path_aliases(project);
    let mut observed = BTreeMap::new();
    for (path, file) in result
        .get("files")
        .and_then(JsonValue::as_object)
        .into_iter()
        .flatten()
    {
        let status = file.get("status").and_then(JsonValue::as_str).unwrap_or("");
        let outcome = Outcome::from_file_status(status)
            .ok_or_else(|| format!("summary.json: unknown status {status:?} for {path}"))?;
        observed.insert(project_relative(path, &aliases), outcome);
    }
    let operational = result
        .get("operationalProblems")
        .and_then(JsonValue::as_object)
        .into_iter()
        .flatten()
        .map(|(id, problem)| {
            let message = problem.get("message").and_then(JsonValue::as_str);
            format!("{id}: {}", message.unwrap_or("(no message)"))
        })
        .collect::<Vec<_>>();
    let blocked = is_block(stop);

    let mut problems = Vec::new();
    if expect.outcome == Outcome::Operational {
        if operational.is_empty() {
            problems.push(
                "expected an operational problem, but summary.json records no operational problem"
                    .to_owned(),
            );
        }
        for file in cited {
            if observed.get(file) == Some(&Outcome::Manual) {
                problems.push(format!(
                    "{file}: operational failure misclassified as manual-fixes-needed"
                ));
            }
        }
    } else {
        if !operational.is_empty() {
            problems.push(format!(
                "unexpected operational problem(s): {}",
                operational.join("; ")
            ));
        }
        let mut worst = None;
        for file in cited {
            let Some(actual) = observed.get(file).copied() else {
                problems.push(format!(
                    "{file}: not assessed (absent from summary.json files; uncovered or not applicable)"
                ));
                continue;
            };
            worst = worst.max(Some(actual));
        }
        // A workspace tool may change or blame files the agent did not cite;
        // those count toward the aggregate. Untouched clean files do not.
        for (file, actual) in &observed {
            if !cited.contains(file) && *actual > Outcome::Clean {
                worst = worst.max(Some(*actual));
            }
        }
        for (file, expected) in &expect.files {
            match observed.get(file) {
                Some(actual) if actual != expected => {
                    problems.push(format!("{file}: expected {expected}, observed {actual}"));
                }
                None if !cited.contains(file) => problems.push(format!(
                    "{file}: expected {expected}, but summary.json does not report it changed or blamed"
                )),
                _ => {}
            }
        }
        // Workspace-wide remedies rewrite files beyond the edited ones, and
        // the run reports those files too.
        for (file, outcome) in &observed {
            if !cited.contains(file) {
                worst = worst.max(Some(*outcome));
            }
        }
        match worst {
            Some(worst) if worst != expect.outcome => problems.push(format!(
                "aggregate: expected {}, observed {worst}",
                expect.outcome
            )),
            _ => {}
        }
        match (expect.outcome == Outcome::Manual, blocked) {
            (true, false) => {
                problems.push("manual case did not block Stop (no decision=block)".to_owned())
            }
            (false, true) => problems.push(format!("{} case blocked Stop", expect.outcome)),
            _ => {}
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    let observed = observed
        .iter()
        .map(|(file, outcome)| format!("{file}={outcome}"))
        .collect::<Vec<_>>();
    Err(format!(
        "deferred outcome mismatch:\n  {}\nobserved files: [{}]; operational: [{}]; blocked: {blocked}",
        problems.join("\n  "),
        observed.join(", "),
        operational.join("; "),
    ))
}

fn project_relative(path: &str, aliases: &[String]) -> String {
    aliases
        .iter()
        .filter_map(|alias| path.strip_prefix(alias.as_str()))
        .filter_map(|rest| rest.strip_prefix('/'))
        .min_by_key(|rest| rest.len())
        .unwrap_or(path)
        .to_owned()
}

/// Checks post-run content: `expected/` when present, otherwise unchanged
/// inputs unless the case is auto-fixed. With `capture`, a missing
/// `expected/` is written from an auto-fixed or manual (partial-fix) run.
fn verify_post_state(case: &FixtureCase, project: &Path, capture: bool) -> Result<(), String> {
    let expected_root = case.directory.join("expected");
    if expected_root.exists() {
        return verify_expected_tree(&expected_root, &expected_root, project);
    }
    let changed = changed_inputs(&case.directory, project)?;
    if changed.is_empty() {
        return Ok(());
    }
    if capture && matches!(case.expect.outcome, Outcome::AutoFixed | Outcome::Manual) {
        for relative in &changed {
            let destination = expected_root.join(relative);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("create {parent:?}: {error}"))?;
            }
            std::fs::copy(project.join(relative), &destination)
                .map_err(|error| format!("capture {relative:?} into {destination:?}: {error}"))?;
        }
        println!(
            "CAPTURED {}/{} expected/: {} (review before committing)",
            case.tool,
            case.case,
            changed.join(", ")
        );
        return Ok(());
    }
    if case.expect.outcome == Outcome::AutoFixed {
        return Ok(());
    }
    Err(format!(
        "{} changed during a {} case without expected/ post-state; review the change and \
         capture it with {CAPTURE_ENV}=1",
        changed.join(", "),
        case.expect.outcome
    ))
}

fn changed_inputs(case_dir: &Path, project: &Path) -> Result<Vec<String>, String> {
    let mut changed = Vec::new();
    for relative in input_files(case_dir)? {
        let before = std::fs::read(case_dir.join(&relative))
            .map_err(|error| format!("read fixture input {relative:?}: {error}"))?;
        let after = std::fs::read(project.join(&relative)).ok();
        if after.as_deref() != Some(before.as_slice()) {
            changed.push(relative.to_string_lossy().into_owned());
        }
    }
    Ok(changed)
}

fn verify_expected_tree(root: &Path, current: &Path, project: &Path) -> Result<(), String> {
    for entry in sorted_entries(current)? {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {path:?}: {error}"))?;
        if file_type.is_dir() {
            verify_expected_tree(root, &path, project)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("strip expected prefix from {path:?}: {error}"))?;
        let actual_path = project.join(relative);
        let expected = std::fs::read_to_string(&path)
            .map_err(|error| format!("read expected {path:?}: {error}"))?;
        let actual = std::fs::read_to_string(&actual_path).map_err(|error| {
            format!("read post-run {actual_path:?} for expected/{relative:?}: {error}")
        })?;
        if expected != actual {
            return Err(format!(
                "post-run file mismatch for {relative:?}:\n  expected:\n{expected}\n  actual:\n{actual}"
            ));
        }
    }
    Ok(())
}

fn write_pkl_config(project: &Path, tool: &str, property: &str) -> Result<(), String> {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir)
        .map_err(|error| format!("create config directory {config_dir:?}: {error}"))?;
    // Deferred candidates come only from the cited tool call: the mtime
    // fallback would also pick up freshly copied supporting files.
    let body = format!(
        r#"amends "Config.pkl"
import "Builtins.pkl"

settings {{
  diagnosticsDirectory = ".velvet-glove/{tool}-agent-hook"
  fileActivity {{ filesystemMtime = false }}
}}

tools {{
  ["{tool}"] = Builtins.{property}
}}
run = new Listing<String> {{ "{tool}" }}
"#
    );
    std::fs::write(config_dir.join("post-tool-use.pkl"), body)
        .map_err(|error| format!("write post-tool-use.pkl: {error}"))
}

/// Validates a case directory and returns its cited files and expectation.
fn load_case(directory: &Path) -> Result<(Vec<String>, CaseSpec), String> {
    for entry in sorted_entries(directory)? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_legacy_golden(&name) {
            return Err(format!(
                "legacy byte golden {name}: cases assert semantics in {CASE_SPEC}, not \
                 transcripts; delete it (see tests/tool-fixtures/README.md)"
            ));
        }
    }
    let inputs = input_files(directory)?
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let text = std::fs::read_to_string(directory.join(CASE_SPEC)).map_err(|error| {
        format!("read {CASE_SPEC} (every case declares its expected outcome there): {error}")
    })?;
    CaseSpec::parse(&text, &inputs)
}

fn is_legacy_golden(name: &str) -> bool {
    ProtocolSurface::ALL.iter().any(|surface| {
        ["json", "stderr.txt", "exit"]
            .iter()
            .any(|extension| name == format!("{}.{extension}", surface.cli_name()))
    })
}

/// Without `cite`: the top-level `example.*` inputs, or else the first
/// top-level input file.
fn default_cited(inputs: &[String]) -> Result<Vec<String>, String> {
    let top_level = inputs
        .iter()
        .filter(|path| Path::new(path).components().count() == 1)
        .collect::<Vec<_>>();
    let examples = top_level
        .iter()
        .filter(|name| name.starts_with("example."))
        .map(|name| (*name).clone())
        .collect::<Vec<_>>();
    if !examples.is_empty() {
        return Ok(examples);
    }
    top_level
        .first()
        .map(|file| vec![(*file).clone()])
        .ok_or_else(|| {
            "no top-level input file to cite; add an `example.<ext>` at the case root or \
             list inputs in `cite`"
                .to_owned()
        })
}

/// An explicit `cite`: distinct case-relative input files, never `expected/`.
fn parse_cite(value: &JsonValue, inputs: &[String]) -> Result<Vec<String>, String> {
    let entries = value
        .as_array()
        .filter(|entries| !entries.is_empty())
        .ok_or_else(|| format!("{CASE_SPEC}: `cite` must be a non-empty list of case files"))?;
    let mut cited = Vec::with_capacity(entries.len());
    for entry in entries {
        let path = entry
            .as_str()
            .ok_or_else(|| format!("{CASE_SPEC}: `cite` entries must be strings"))?;
        if Path::new(path).starts_with("expected") {
            return Err(format!(
                "{CASE_SPEC}: `cite` names {path:?}, but expected/ holds post-state, not inputs"
            ));
        }
        if !inputs.iter().any(|input| input == path) {
            return Err(format!(
                "{CASE_SPEC}: `cite` names {path:?}, which is not an input file of the case"
            ));
        }
        if cited.iter().any(|file| file == path) {
            return Err(format!("{CASE_SPEC}: `cite` names {path:?} twice"));
        }
        cited.push(path.to_owned());
    }
    Ok(cited)
}

/// Case-relative paths of every fixture input: all files except
/// `case.json`, a case `README.md`, and the `expected/` tree.
fn input_files(case_dir: &Path) -> Result<Vec<PathBuf>, String> {
    fn collect(root: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in sorted_entries(current)? {
            let path = entry.path();
            let name = entry.file_name();
            if current == root
                && (name == OsStr::new("expected")
                    || name == OsStr::new("README.md")
                    || name == OsStr::new(CASE_SPEC))
            {
                continue;
            }
            let file_type = entry
                .file_type()
                .map_err(|error| format!("file type for {path:?}: {error}"))?;
            if file_type.is_dir() {
                collect(root, &path, files)?;
            } else if file_type.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| format!("strip fixture prefix from {path:?}: {error}"))?;
                files.push(relative.to_path_buf());
            } else {
                return Err(format!("unsupported fixture entry type: {path:?}"));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    collect(case_dir, case_dir, &mut files)?;
    Ok(files)
}

fn copy_fixture_inputs(case_dir: &Path, project: &Path) -> Result<(), String> {
    for relative in input_files(case_dir)? {
        let source = case_dir.join(&relative);
        let destination = project.join(&relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create {parent:?}: {error}"))?;
        }
        std::fs::copy(&source, &destination)
            .map_err(|error| format!("copy {source:?} to {destination:?}: {error}"))?;
    }
    Ok(())
}

fn files_named(root: &Path, name: &str) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for entry in sorted_entries(root)? {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_named(&path, name)?);
        } else if entry.file_name() == OsStr::new(name) {
            found.push(path);
        }
    }
    Ok(found)
}

fn check_tool_programs(spec: &ToolSpec) -> Result<(), Vec<String>> {
    let mut programs = BTreeSet::from([spec.executable.as_str()]);
    programs.extend(
        spec.phases
            .values()
            .filter(|phase| phase.enabled)
            .filter_map(|phase| phase.program.as_deref()),
    );
    programs.extend(
        spec.workflows
            .values()
            .filter(|workflow| workflow.enabled)
            .flat_map(|workflow| workflow.check.iter().chain(workflow.remedy.iter()))
            .filter_map(|command| command.program.as_deref()),
    );
    let missing = programs
        .into_iter()
        .filter(|program| resolve_program(program).is_none())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

fn resolve_program(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.components().count() > 1 {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().ok()?.join(path)
        };
        return is_executable(&candidate).then_some(candidate);
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(program))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn run_probe_matrix(timeout: Duration, artifact_root: Option<&Path>) -> Result<usize, String> {
    let mut commands = 0;
    for surface in PROBE_SURFACES {
        match run_probe_case(surface, timeout, artifact_root) {
            Ok(executed) => commands += executed,
            Err(mut error) => {
                let report = probe_report(commands, Some((surface, &error)));
                println!("{REPORT_PREFIX}{report}");
                if let Some(root) = artifact_root {
                    match write_report(root, &report) {
                        Ok(path) => error.push_str(&format!(
                            "; machine-readable failure report: {}",
                            path.display()
                        )),
                        Err(report_error) => error.push_str(&format!(
                            "; failed to retain machine-readable probe report: {report_error}"
                        )),
                    }
                }
                return Err(error);
            }
        }
    }
    let report = probe_report(commands, None);
    println!("{REPORT_PREFIX}{report}");
    if commands == 0 {
        return Err("probe executed zero external commands".to_owned());
    }
    Ok(commands)
}

fn probe_report(commands: usize, failure: Option<(FixtureSurface, &str)>) -> JsonValue {
    serde_json::json!({
        "formatVersion": REPORT_FORMAT_VERSION,
        "kind": "probe",
        "status": if failure.is_some() { "fail" } else { "pass" },
        "surfaces": PROBE_SURFACES.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "totals": {
            "protocolProbeSurfaces": PROBE_SURFACES.len(),
            "commandsExecuted": commands,
        },
        "failure": failure.map(|(surface, detail)| serde_json::json!({
            "surface": surface.to_string(),
            "detail": detail,
        })),
    })
}

fn run_probe_case(
    surface: FixtureSurface,
    timeout: Duration,
    artifact_root: Option<&Path>,
) -> Result<usize, String> {
    run_probe_attempt(surface, artifact_root, |root| {
        run_probe_case_inner(surface, timeout, root)
    })
}

fn run_probe_attempt(
    surface: FixtureSurface,
    artifact_root: Option<&Path>,
    execute: impl FnOnce(&Path) -> Result<usize, String>,
) -> Result<usize, String> {
    let root = unique_temp_dir(&format!("velvet-glove-probe-{surface}"));
    match execute(&root) {
        Ok(commands) => {
            let _ = std::fs::remove_dir_all(&root);
            Ok(commands)
        }
        Err(mut error) => {
            let evidence = root.join("evidence");
            if let Err(write_error) = std::fs::create_dir_all(&evidence)
                .map_err(|write_error| format!("create probe evidence: {write_error}"))
                .and_then(|()| {
                    write_json(
                        &evidence.join("probe-outcome.json"),
                        &serde_json::json!({
                            "formatVersion": REPORT_FORMAT_VERSION,
                            "surface": surface.to_string(),
                            "status": "fail",
                            "detail": error,
                        }),
                    )
                })
            {
                error.push_str(&format!("; failed to write probe outcome: {write_error}"));
            }
            if let Some(destination_root) = artifact_root {
                match retain_probe_failure(&root, destination_root, surface) {
                    Ok(destination) => {
                        let _ = std::fs::remove_dir_all(&root);
                        error.push_str(&format!(
                            "; retained probe artifacts: {}",
                            destination.display()
                        ));
                    }
                    Err(retain_error) => error.push_str(&format!(
                        "; {retain_error}; preserved temporary probe artifacts: {}",
                        root.display()
                    )),
                }
            } else {
                let _ = std::fs::remove_dir_all(&root);
            }
            Err(error)
        }
    }
}

fn run_probe_case_inner(
    surface: FixtureSurface,
    timeout: Duration,
    root: &Path,
) -> Result<usize, String> {
    let project = root.join("workspace");
    let evidence = root.join("evidence");
    let probe_dir = root.join("probe");
    std::fs::create_dir_all(&project)
        .map_err(|error| format!("create probe workspace {project:?}: {error}"))?;
    std::fs::create_dir_all(&evidence)
        .map_err(|error| format!("create probe evidence {evidence:?}: {error}"))?;
    std::fs::create_dir_all(&probe_dir)
        .map_err(|error| format!("create probe directory {probe_dir:?}: {error}"))?;
    let target = project.join("example.fixture");
    std::fs::write(&target, "fixture\n")
        .map_err(|error| format!("write probe fixture {target:?}: {error}"))?;

    let probe = probe_dir.join("fixture-probe");
    std::fs::write(&probe, include_bytes!("support/fixture-probe.sh"))
        .map_err(|error| format!("write probe executable {probe:?}: {error}"))?;
    #[cfg(unix)]
    {
        let mut permissions = std::fs::metadata(&probe)
            .map_err(|error| format!("probe metadata {probe:?}: {error}"))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&probe, permissions)
            .map_err(|error| format!("make probe executable {probe:?}: {error}"))?;
    }
    write_probe_config(&project, &probe)?;

    let input = PostToolUseBuilder::new(surface.protocol, &project, "example.fixture")
        .identity("probe-session", "probe-turn", "probe-tool")
        .build()?;
    let sentinel = format!("surface:{surface}");
    let env = [
        (PROBE_DIR_ENV, probe_dir.as_os_str()),
        (PROBE_SENTINEL_ENV, OsStr::new(&sentinel)),
    ];
    match surface.lane {
        Lane::Immediate => {
            let stdout = run_hook(
                "post-tool-immediate",
                None,
                &input,
                input.bytes(),
                timeout,
                &evidence,
                &env,
            )
            .map_err(|error| format!("{surface} probe: {error}"))?;
            if stdout != serde_json::json!({}) {
                return Err(format!(
                    "{surface} probe expected {{}} stdout, got {stdout}"
                ));
            }
        }
        Lane::Deferred => {
            let run = run_deferred_flow(root, &project, &input, timeout, &env)
                .map_err(|error| format!("{surface} probe: {error}"))?;
            check_deferred_run(
                &CaseSpec::new(Outcome::Clean),
                &["example.fixture".to_owned()],
                &project,
                &run.stop,
                &run.summary,
            )
            .map_err(|error| format!("{surface} probe: {error}"))?;
        }
    }

    let invocations_dir = probe_dir.join("invocations");
    let invocations = sorted_entries(&invocations_dir)?
        .into_iter()
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    if invocations.len() != 1 {
        return Err(format!(
            "{surface} probe expected exactly one invocation, observed {} at {invocations_dir:?}",
            invocations.len()
        ));
    }
    let record = invocations[0].path();
    assert_record(&record, "program", probe.to_string_lossy().as_ref())?;
    assert_record(
        &record,
        "cwd",
        canonical_project(&project).to_string_lossy().trim_end(),
    )?;
    assert_record(&record, "sentinel", &sentinel)?;
    assert_record(&record, "argc", "2")?;
    assert_record(&record, "argv-0", "--fixture-contract")?;
    assert_record(
        &record,
        "argv-1",
        canonical_project(&target).to_string_lossy().as_ref(),
    )?;
    Ok(1)
}

fn write_probe_config(project: &Path, probe: &Path) -> Result<(), String> {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir)
        .map_err(|error| format!("create probe config directory {config_dir:?}: {error}"))?;
    let probe = pkl_string(probe.to_string_lossy().as_ref());
    let config = format!(
        r#"amends "Config.pkl"

settings {{
  jobs = 1
  fileActivity {{ filesystemMtime = false }}
}}

tools {{
  ["fixture-probe"] = new ToolSpec {{
    id = "fixture-probe"
    displayName = "fixture probe"
    executable = "{probe}"
    files {{ include = new Listing {{ "*.fixture"; "**/*.fixture" }} }}
    phases {{
      ["verify"] = new Phase {{
        mode = "verify"
        argv = new Listing {{ "--fixture-contract"; new Files {{}} }}
      }}
    }}
    phaseOrder = new Listing {{ "verify" }}
  }}
}}
run = new Listing {{ "fixture-probe" }}
"#
    );
    std::fs::write(config_dir.join("post-tool-use.pkl"), config)
        .map_err(|error| format!("write probe config: {error}"))
}

fn assert_record(record: &Path, name: &str, expected: &str) -> Result<(), String> {
    let path = record.join(name);
    let actual = std::fs::read_to_string(&path)
        .map_err(|error| format!("read probe record {path:?}: {error}"))?;
    if actual.trim_end() == expected {
        Ok(())
    } else {
        Err(format!(
            "probe {name} mismatch: expected {expected:?}, got {:?}",
            actual.trim_end()
        ))
    }
}

fn pkl_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn require_pkl(timeout: Duration) -> Result<(), String> {
    let root = unique_temp_dir("velvet-glove-pkl-prerequisite");
    let mut command = Command::new("pkl");
    command.arg("--version");
    let result = run_with_timeout(&mut command, &[], timeout, &root);
    let _ = std::fs::remove_dir_all(&root);
    let output = result.map_err(|error| format!("required Pkl >= 0.31.1 unavailable: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "required Pkl prerequisite failed with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let version = String::from_utf8_lossy(&output.stdout);
    if !pkl_version_is_supported(&version) {
        return Err(format!(
            "required Pkl version is 0.31.1 or newer; found {}",
            version.trim()
        ));
    }
    Ok(())
}

/// Accepts `Pkl <major>.<minor>.<patch> ...` at or above 0.31.1.
fn pkl_version_is_supported(banner: &str) -> bool {
    let Some(version) = banner
        .strip_prefix("Pkl ")
        .and_then(|rest| rest.split_whitespace().next())
    else {
        return false;
    };
    let parts: Vec<u64> = version
        .split('.')
        .map_while(|part| part.parse().ok())
        .collect();
    let [major, minor, patch] = parts[..] else {
        return false;
    };
    (major, minor, patch) >= (0, 31, 1)
}

fn configured_timeout() -> Result<Duration, String> {
    let Some(value) = std::env::var_os(TIMEOUT_ENV) else {
        return Ok(Duration::from_secs(DEFAULT_TIMEOUT_SECS));
    };
    let value = value
        .into_string()
        .map_err(|_| format!("{TIMEOUT_ENV} must be UTF-8"))?;
    let seconds = value
        .parse::<u64>()
        .map_err(|error| format!("invalid {TIMEOUT_ENV}={value:?}: {error}"))?;
    if seconds == 0 {
        return Err(format!("{TIMEOUT_ENV} must be greater than zero"));
    }
    Ok(Duration::from_secs(seconds))
}

fn configured_artifact_dir() -> Result<Option<PathBuf>, String> {
    match std::env::var_os(ARTIFACT_ENV) {
        None => Ok(None),
        Some(value) if value.is_empty() => Err(format!("{ARTIFACT_ENV} must not be empty")),
        Some(value) => {
            let path = PathBuf::from(value);
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|error| format!("resolve {ARTIFACT_ENV}: {error}"))?
                    .join(path)
            };
            std::fs::create_dir_all(&path)
                .map_err(|error| format!("create {ARTIFACT_ENV} {path:?}: {error}"))?;
            Ok(Some(path))
        }
    }
}

fn build_report(
    catalog: &FixtureCatalog,
    outcomes: &[FixtureOutcome],
    probe_commands: usize,
) -> JsonValue {
    let mut passed = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut skip_reasons = BTreeMap::<&str, usize>::new();
    let mut by_surface = BTreeMap::<String, [usize; 3]>::new();
    for outcome in outcomes {
        let counts = by_surface
            .entry(outcome.surface.to_string())
            .or_insert([0, 0, 0]);
        match &outcome.status {
            FixtureStatus::Pass => {
                passed += 1;
                counts[0] += 1;
            }
            FixtureStatus::Skip(reason) => {
                skipped += 1;
                counts[1] += 1;
                *skip_reasons.entry(reason.code).or_default() += 1;
            }
            FixtureStatus::Fail(_) => {
                failed += 1;
                counts[2] += 1;
            }
        }
    }
    let surface_totals = by_surface
        .into_iter()
        .map(|(surface, counts)| {
            (
                surface,
                serde_json::json!({
                    "passed": counts[0],
                    "skipped": counts[1],
                    "failed": counts[2],
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({
        "formatVersion": REPORT_FORMAT_VERSION,
        "kind": "real-tool-fixtures",
        "surfaces": surface_names(),
        "totals": {
            "tools": catalog.tool_count,
            "cases": catalog.cases.len(),
            "fixtureSurfaces": FIXTURE_SURFACES.len(),
            "protocolProbeSurfaces": PROBE_SURFACES.len(),
            "plannedSurfaceCases": catalog.cases.len() * FIXTURE_SURFACES.len(),
            "attemptedSurfaceCases": passed + failed,
            "passed": passed,
            "skipped": skipped,
            "failed": failed,
            "probeCommandsExecuted": probe_commands,
        },
        "bySurface": surface_totals,
        "skipReasons": skip_reasons,
        "outcomes": outcomes.iter().map(FixtureOutcome::as_json).collect::<Vec<_>>(),
    })
}

fn surface_names() -> Vec<String> {
    FIXTURE_SURFACES.iter().map(ToString::to_string).collect()
}

fn print_outcomes(outcomes: &[FixtureOutcome]) {
    for outcome in outcomes {
        match &outcome.status {
            FixtureStatus::Pass => println!(
                "PASS  {}/{} ({})",
                outcome.tool, outcome.case, outcome.surface
            ),
            FixtureStatus::Skip(reason) => println!(
                "SKIP  {}/{} ({}): {} ({})",
                outcome.tool, outcome.case, outcome.surface, reason.detail, reason.code
            ),
            FixtureStatus::Fail(reason) => {
                eprintln!(
                    "FAIL  {}/{} ({}, expected {}):\n{reason}",
                    outcome.tool, outcome.case, outcome.surface, outcome.expected
                );
                if let Some(path) = &outcome.artifacts {
                    eprintln!("retained artifacts: {}", path.display());
                }
            }
        }
    }
}

fn retain_failure(
    source: &Path,
    artifact_root: &Path,
    case: &FixtureCase,
    surface: FixtureSurface,
) -> Result<PathBuf, String> {
    let destination = artifact_root
        .join(sanitize_component(&case.tool))
        .join(sanitize_component(&case.case))
        .join(format!(
            "{surface}-{}-{}",
            std::process::id(),
            unique_nonce()
        ));
    copy_tree(source, &destination).map_err(|error| {
        format!(
            "retain requested failure artifacts at {destination:?}: {error}; temporary evidence was {source:?}"
        )
    })?;
    Ok(destination)
}

fn retain_probe_failure(
    source: &Path,
    artifact_root: &Path,
    surface: FixtureSurface,
) -> Result<PathBuf, String> {
    let destination = artifact_root
        .join("probe")
        .join(surface.to_string())
        .join(format!("{}-{}", std::process::id(), unique_nonce()));
    copy_tree(source, &destination).map_err(|error| {
        format!(
            "retain requested probe artifacts at {destination:?}: {error}; temporary evidence is {source:?}"
        )
    })?;
    Ok(destination)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination)
        .map_err(|error| format!("create {destination:?}: {error}"))?;
    for entry in sorted_entries(source)? {
        let path = entry.path();
        let target = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {path:?}: {error}"))?;
        if file_type.is_dir() {
            copy_tree(&path, &target)?;
        } else if file_type.is_file() {
            std::fs::copy(&path, &target)
                .map_err(|error| format!("copy {path:?} to {target:?}: {error}"))?;
        } else {
            return Err(format!("cannot retain unsupported entry {path:?}"));
        }
    }
    Ok(())
}

fn write_report(root: &Path, report: &JsonValue) -> Result<PathBuf, String> {
    let path = root.join(format!(
        "report-{}-{}.json",
        std::process::id(),
        unique_nonce()
    ));
    write_json(&path, report)?;
    Ok(path)
}

fn write_json(path: &Path, value: &JsonValue) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("serialize JSON for {path:?}: {error}"))?;
    std::fs::write(path, bytes).map_err(|error| format!("write {path:?}: {error}"))
}

fn append_failure(outcome: &mut FixtureOutcome, extra: String) {
    match &mut outcome.status {
        FixtureStatus::Fail(reason) => {
            reason.push('\n');
            reason.push_str(&extra);
        }
        FixtureStatus::Pass | FixtureStatus::Skip(_) => {
            outcome.status = FixtureStatus::Fail(extra);
        }
    }
}

fn workspace_path_aliases(project: &Path) -> Vec<String> {
    let mut aliases = vec![project.to_string_lossy().into_owned()];
    if let Ok(canonical) = project.canonicalize() {
        let canonical = canonical.to_string_lossy().into_owned();
        if !aliases.contains(&canonical) {
            aliases.push(canonical);
        }
    }
    aliases
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tool-fixtures")
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    loop {
        let candidate = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            unique_nonce()
        ));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create temporary directory {candidate:?}: {error}"),
        }
    }
}

fn unique_nonce() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{timestamp}-{counter}")
}
