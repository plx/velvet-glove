//! Fixture-driven validation for Velvet Glove's built-in immediate workflows.
//!
//! The non-ignored tests fail closed on fixture discovery and prove, with a
//! hermetic executable, that every native protocol reaches a subprocess through
//! the real `velvet-glove` binary. The opt-in test additionally executes the
//! host's real tools against the checked-in golden corpus.

#[path = "support/process.rs"]
mod bounded_process;
mod support;

use bounded_process::{BoundedCommandError, BoundedOutput, run_with_timeout};
use hookkit_pkl_config::ToolSpec;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use support::native_events::{PostToolUseBuilder, ProtocolSurface, canonical_project};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const REAL_TOOL_SURFACES: &[ProtocolSurface] = &[ProtocolSurface::Claude, ProtocolSurface::Codex];
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const TIMEOUT_ENV: &str = "VELVET_GLOVE_FIXTURE_TIMEOUT_SECS";
const ARTIFACT_ENV: &str = "VELVET_GLOVE_FIXTURE_ARTIFACT_DIR";
const REQUIRED_TOOLS_ENV: &str = "VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS";
const SELECTION_ENV: &str = "VELVET_GLOVE_FIXTURE_SELECTION";
const REPORT_PREFIX: &str = "VELVET_GLOVE_FIXTURE_JSON=";
const PROBE_SENTINEL_ENV: &str = "VELVET_GLOVE_FIXTURE_PROBE_SENTINEL";
const PROBE_DIR_ENV: &str = "VELVET_GLOVE_FIXTURE_PROBE_DIR";
const JQ_TRACE_DIR_ENV: &str = "VELVET_GLOVE_JQ_TRACE_DIR";
const JQ_REAL_PROGRAM_ENV: &str = "VELVET_GLOVE_JQ_REAL_PROGRAM";
const JQ_LOGICAL_PROGRAM_ENV: &str = "VELVET_GLOVE_JQ_LOGICAL_PROGRAM";
const JQ_TRACE_SENTINEL_ENV: &str = "VELVET_GLOVE_JQ_TRACE_SENTINEL";
const JQ_TRACE_SENTINEL: &str = "jq-real-tool-fixture";
static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JqExpectedOutcome {
    Clean,
    Issues,
    OperationalFailure,
}

#[derive(Debug)]
struct JqContractCase {
    targets: &'static [(&'static str, i32)],
    extra_args: &'static [&'static str],
    outcome: JqExpectedOutcome,
    diagnostic_contains: Option<&'static str>,
}

fn jq_contract_case(case: &FixtureCase) -> Result<Option<JqContractCase>, String> {
    if case.tool != "jq" {
        return Ok(None);
    }
    let contract = match case.case.as_str() {
        "clean" => JqContractCase {
            targets: &[("example.json", 0)],
            extra_args: &[],
            outcome: JqExpectedOutcome::Clean,
            diagnostic_contains: None,
        },
        "invalid" => JqContractCase {
            targets: &[("example.json", 5)],
            extra_args: &[],
            outcome: JqExpectedOutcome::Issues,
            diagnostic_contains: Some("jq: parse error:"),
        },
        "operational-failure" => JqContractCase {
            targets: &[("example.json", 2)],
            extra_args: &["--indent", "9"],
            outcome: JqExpectedOutcome::OperationalFailure,
            diagnostic_contains: Some("jq: --indent takes a number between -1 and 7"),
        },
        "multi-file-fragments" => JqContractCase {
            targets: &[("example.1-open.json", 5), ("example.2-close.json", 5)],
            extra_args: &[],
            outcome: JqExpectedOutcome::Issues,
            diagnostic_contains: Some("jq: parse error:"),
        },
        other => {
            return Err(format!(
                "jq fixture {other:?} has no real-tool contract declaration"
            ));
        }
    };
    Ok(Some(contract))
}

#[test]
fn fixture_inventory_is_non_empty_and_has_no_orphans() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let catalog = discover_fixture_catalog(&fixtures_root(), &specs)
        .unwrap_or_else(|error| panic!("fixture discovery failed: {error}"));

    let report = serde_json::json!({
        "formatVersion": 1,
        "kind": "inventory",
        "totals": {
            "tools": catalog.tool_count,
            "cases": catalog.cases.len(),
            "fixtureSurfaces": REAL_TOOL_SURFACES.len(),
            "protocolProbeSurfaces": ProtocolSurface::ALL.len(),
            "plannedSurfaceCases": catalog.cases.len() * REAL_TOOL_SURFACES.len(),
        }
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
    assert_eq!(commands, ProtocolSurface::ALL.len());
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
fn discovery_rejects_goldens_for_unexecuted_surfaces() {
    let case = unique_temp_dir("velvet-glove-unsupported-golden");
    std::fs::write(case.join("example.txt"), "fixture").expect("fixture input");
    std::fs::write(case.join("antigravity.json"), "{}").expect("unsupported golden");

    let error = validate_supported_goldens(&case)
        .expect_err("goldens outside the real fixture matrix must fail closed");
    assert!(error.contains("antigravity"));
    assert!(error.contains("not executed"));

    let _ = std::fs::remove_dir_all(case);
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
    let case = FixtureCase {
        tool: "probe-tool".to_owned(),
        case: "failure-case".to_owned(),
        directory: PathBuf::new(),
        entry: PathBuf::new(),
        pkl_property: String::new(),
        spec: ToolSpec::default(),
    };

    let retained = retain_failure(&source, &artifact_root, &case, ProtocolSurface::Claude)
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
    let case = FixtureCase {
        tool: "fixture-tool".to_owned(),
        case: "setup-failure".to_owned(),
        directory: fixture_root.clone(),
        entry: PathBuf::from("example.txt"),
        pkl_property: "fixtureTool".to_owned(),
        spec: ToolSpec::default(),
    };
    let options = HarnessOptions {
        timeout: Duration::from_secs(1),
        artifact_dir: Some(artifact_root.clone()),
        required_tools: RequiredTools::default(),
        selection: FixtureSelection::default(),
    };

    let outcome = run_fixture_case(&case, ProtocolSurface::Claude, &options);
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
fn fixture_selection_filters_exact_cases_and_recounts_tools() {
    let catalog = FixtureCatalog {
        tool_count: 2,
        cases: vec![
            named_fixture_case("tool-a", "clean"),
            named_fixture_case("tool-a", "issues"),
            named_fixture_case("tool-b", "clean"),
        ],
    };
    let selection = FixtureSelection {
        tools: BTreeSet::new(),
        cases: BTreeSet::from([
            ("tool-a".to_owned(), "issues".to_owned()),
            ("tool-b".to_owned(), "clean".to_owned()),
        ]),
    };

    let selected = selection.apply(catalog).expect("valid selection");
    assert_eq!(selected.tool_count, 2);
    assert_eq!(selected.cases.len(), 2);
    assert_eq!(selected.cases[0].tool, "tool-a");
    assert_eq!(selected.cases[0].case, "issues");
    assert_eq!(selected.cases[1].tool, "tool-b");
    assert_eq!(selected.cases[1].case, "clean");
}

#[test]
fn fixture_selection_rejects_unknown_tools_and_cases() {
    let catalog = || FixtureCatalog {
        tool_count: 1,
        cases: vec![named_fixture_case("tool-a", "clean")],
    };
    let unknown_tool = FixtureSelection {
        tools: BTreeSet::from(["tool-b".to_owned()]),
        cases: BTreeSet::new(),
    }
    .apply(catalog())
    .expect_err("unknown tool must fail");
    assert!(unknown_tool.contains("tool-b"));

    let unknown_case = FixtureSelection {
        tools: BTreeSet::new(),
        cases: BTreeSet::from([("tool-a".to_owned(), "issues".to_owned())]),
    }
    .apply(catalog())
    .expect_err("unknown case must fail");
    assert!(unknown_case.contains("tool-a/issues"));
}

#[test]
fn requested_probe_failure_artifacts_are_retained() {
    let artifact_root = unique_temp_dir("velvet-glove-probe-artifact-root");
    let error = run_probe_attempt(ProtocolSurface::Claude, Some(&artifact_root), |root| {
        std::fs::create_dir_all(root.join("evidence"))
            .map_err(|error| format!("create probe evidence: {error}"))?;
        std::fs::write(root.join("evidence/input.json"), "{\"probe\":true}")
            .map_err(|error| format!("write probe evidence: {error}"))?;
        Err("intentional probe failure".to_owned())
    })
    .expect_err("failing probe must return an error");

    assert!(error.contains("intentional probe failure"));
    assert!(error.contains("retained probe artifacts"));
    let retained =
        sorted_entries(&artifact_root.join("probe/claude")).expect("retained probe directories");
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
    let outcomes = vec![
        FixtureOutcome::pass(&catalog.cases[0], ProtocolSurface::Claude),
        FixtureOutcome::skipped(
            &catalog.cases[0],
            ProtocolSurface::Codex,
            SkipReason {
                code: "executable-unavailable",
                detail: "missing fixture-tool".to_owned(),
            },
        ),
        FixtureOutcome::failed(
            &catalog.cases[1],
            ProtocolSurface::Claude,
            "golden mismatch",
        ),
        FixtureOutcome::pass(&catalog.cases[1], ProtocolSurface::Codex),
    ];

    let report = build_report(&catalog, &outcomes, ProtocolSurface::ALL.len());
    let totals = &report["totals"];
    assert_eq!(totals["plannedSurfaceCases"], 4);
    assert_eq!(totals["attemptedSurfaceCases"], 3);
    assert_eq!(totals["passed"], 2);
    assert_eq!(totals["skipped"], 1);
    assert_eq!(totals["failed"], 1);
    assert_eq!(report["skipReasons"]["executable-unavailable"], 1);
    assert_eq!(
        report["outcomes"][1]["reason"]["code"],
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

#[test]
fn machine_report_writes_a_stable_index_and_historical_copy() {
    let root = unique_temp_dir("velvet-glove-report-index");
    let report = serde_json::json!({"formatVersion": 1, "kind": "example"});

    let stable = write_report(&root, &report).expect("write machine report");

    assert_eq!(stable, root.join("report.json"));
    assert_eq!(
        serde_json::from_slice::<JsonValue>(&std::fs::read(&stable).unwrap()).unwrap(),
        report
    );
    let historical = sorted_entries(&root)
        .unwrap()
        .into_iter()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("report-") && name.ends_with(".json"))
        })
        .collect::<Vec<_>>();
    assert_eq!(historical.len(), 1);

    let _ = std::fs::remove_dir_all(root);
}

fn fixture_case(name: &str) -> FixtureCase {
    named_fixture_case("fixture-tool", name)
}

fn named_fixture_case(tool: &str, name: &str) -> FixtureCase {
    FixtureCase {
        tool: tool.to_owned(),
        case: name.to_owned(),
        directory: PathBuf::new(),
        entry: PathBuf::from("example.txt"),
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
    let catalog = options
        .selection
        .apply(catalog)
        .unwrap_or_else(|error| panic!("{error}"));
    options
        .required_tools
        .validate(&catalog.tool_ids())
        .unwrap_or_else(|error| panic!("{error}"));
    let probe_commands = run_probe_matrix(options.timeout, options.artifact_dir.as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(probe_commands > 0, "probe executed zero external commands");

    let mut availability = BTreeMap::<String, Result<(), Vec<String>>>::new();
    let mut outcomes = Vec::with_capacity(catalog.cases.len() * REAL_TOOL_SURFACES.len());
    for case in &catalog.cases {
        let available = availability
            .entry(case.tool.clone())
            .or_insert_with(|| check_tool_programs(&case.spec));
        for surface in REAL_TOOL_SURFACES {
            match available {
                Ok(()) => outcomes.push(run_fixture_case(case, *surface, &options)),
                Err(programs)
                    if options.selection.is_active()
                        || options.required_tools.requires(&case.tool) =>
                {
                    outcomes.push(FixtureOutcome::failed(
                        case,
                        *surface,
                        format!("required prerequisite unavailable: {}", programs.join(", ")),
                    ));
                }
                Err(programs) => outcomes.push(FixtureOutcome::skipped(
                    case,
                    *surface,
                    SkipReason {
                        code: "executable-unavailable",
                        detail: format!("programs not found on PATH: {}", programs.join(", ")),
                    },
                )),
            }
        }
    }

    let report = build_report(&catalog, &outcomes, probe_commands);
    print_outcomes(&outcomes);
    println!("{REPORT_PREFIX}{report}");
    if let Some(root) = &options.artifact_dir {
        let path = write_report(root, &report).unwrap_or_else(|error| panic!("{error}"));
        println!("machine-readable report: {}", path.display());
    }

    let planned = catalog.cases.len() * REAL_TOOL_SURFACES.len();
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

#[derive(Debug)]
struct HarnessOptions {
    timeout: Duration,
    artifact_dir: Option<PathBuf>,
    required_tools: RequiredTools,
    selection: FixtureSelection,
}

impl HarnessOptions {
    fn from_environment() -> Result<Self, String> {
        Ok(Self {
            timeout: configured_timeout()?,
            artifact_dir: configured_artifact_dir()?,
            required_tools: RequiredTools::from_environment()?,
            selection: FixtureSelection::from_environment()?,
        })
    }
}

#[derive(Debug, Default)]
struct FixtureSelection {
    tools: BTreeSet<String>,
    cases: BTreeSet<(String, String)>,
}

impl FixtureSelection {
    fn from_environment() -> Result<Self, String> {
        let Some(value) = std::env::var_os(SELECTION_ENV) else {
            return Ok(Self::default());
        };
        let value = value
            .into_string()
            .map_err(|_| format!("{SELECTION_ENV} must be UTF-8"))?;
        let mut selection = Self::default();
        for selector in value
            .split(',')
            .map(str::trim)
            .filter(|selector| !selector.is_empty())
        {
            let mut parts = selector.split('/');
            let tool = parts.next().unwrap_or_default();
            let case = parts.next();
            if tool.is_empty() || parts.next().is_some() || case.is_some_and(str::is_empty) {
                return Err(format!(
                    "{SELECTION_ENV} entries must be `tool-id` or `tool-id/case-id`; invalid entry {selector:?}"
                ));
            }
            match case {
                Some(case) => {
                    selection.cases.insert((tool.to_owned(), case.to_owned()));
                }
                None => {
                    selection.tools.insert(tool.to_owned());
                }
            }
        }
        if selection.tools.is_empty() && selection.cases.is_empty() {
            return Err(format!(
                "{SELECTION_ENV} must contain at least one `tool-id` or `tool-id/case-id`"
            ));
        }
        let redundant = selection
            .cases
            .iter()
            .filter(|(tool, _)| selection.tools.contains(tool))
            .map(|(tool, case)| format!("{tool}/{case}"))
            .collect::<Vec<_>>();
        if !redundant.is_empty() {
            return Err(format!(
                "{SELECTION_ENV} contains case selectors already covered by a tool selector: {}",
                redundant.join(", ")
            ));
        }
        Ok(selection)
    }

    fn is_active(&self) -> bool {
        !self.tools.is_empty() || !self.cases.is_empty()
    }

    fn apply(&self, catalog: FixtureCatalog) -> Result<FixtureCatalog, String> {
        if !self.is_active() {
            return Ok(catalog);
        }
        let available_tools = catalog.tool_ids();
        let requested_tools = self
            .tools
            .iter()
            .cloned()
            .chain(self.cases.iter().map(|(tool, _)| tool.clone()))
            .collect::<BTreeSet<_>>();
        let unknown_tools = requested_tools
            .difference(&available_tools)
            .cloned()
            .collect::<Vec<_>>();
        if !unknown_tools.is_empty() {
            return Err(format!(
                "{SELECTION_ENV} names tools without fixture cases: {}",
                unknown_tools.join(", ")
            ));
        }
        let available_cases = catalog
            .cases
            .iter()
            .map(|case| (case.tool.clone(), case.case.clone()))
            .collect::<BTreeSet<_>>();
        let unknown_cases = self
            .cases
            .difference(&available_cases)
            .map(|(tool, case)| format!("{tool}/{case}"))
            .collect::<Vec<_>>();
        if !unknown_cases.is_empty() {
            return Err(format!(
                "{SELECTION_ENV} names unknown fixture cases: {}",
                unknown_cases.join(", ")
            ));
        }
        let cases = catalog
            .cases
            .into_iter()
            .filter(|case| {
                self.tools.contains(&case.tool)
                    || self.cases.contains(&(case.tool.clone(), case.case.clone()))
            })
            .collect::<Vec<_>>();
        let tool_count = cases
            .iter()
            .map(|case| case.tool.as_str())
            .collect::<BTreeSet<_>>()
            .len();
        if cases.is_empty() {
            return Err(format!("{SELECTION_ENV} selected zero fixture cases"));
        }
        Ok(FixtureCatalog { tool_count, cases })
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
                "{REQUIRED_TOOLS_ENV} names tools without fixture cases: {}",
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
    entry: PathBuf,
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
            validate_supported_goldens(&directory)
                .map_err(|error| format!("{tool}/{case}: {error}"))?;
            let entry =
                find_entry_file(&directory).map_err(|error| format!("{tool}/{case}: {error}"))?;
            cases.push(FixtureCase {
                tool: tool.clone(),
                case,
                directory,
                entry,
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
    surface: ProtocolSurface,
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
    fn pass(case: &FixtureCase, surface: ProtocolSurface) -> Self {
        Self {
            tool: case.tool.clone(),
            case: case.case.clone(),
            surface,
            status: FixtureStatus::Pass,
            artifacts: None,
        }
    }

    fn skipped(case: &FixtureCase, surface: ProtocolSurface, reason: SkipReason) -> Self {
        Self {
            tool: case.tool.clone(),
            case: case.case.clone(),
            surface,
            status: FixtureStatus::Skip(reason),
            artifacts: None,
        }
    }

    fn failed(case: &FixtureCase, surface: ProtocolSurface, reason: impl Into<String>) -> Self {
        Self {
            tool: case.tool.clone(),
            case: case.case.clone(),
            surface,
            status: FixtureStatus::Fail(reason.into()),
            artifacts: None,
        }
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
            "surface": self.surface.cli_name(),
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
    fn prepare(case: &FixtureCase, surface: ProtocolSurface) -> Result<Self, FixtureSetupFailure> {
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
        if let Err(error) = copy_fixture_inputs(&case.directory, &case.directory, &project) {
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
    surface: ProtocolSurface,
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
    let result = run_fixture_case_inner(case, surface, options.timeout, &workspace);
    let outcome = match result {
        Ok(()) => FixtureOutcome::pass(case, surface),
        Err(error) => FixtureOutcome::failed(case, surface, error),
    };
    finalize_fixture_outcome(&workspace.root, case, surface, options, outcome)
}

fn finalize_fixture_outcome(
    root: &Path,
    case: &FixtureCase,
    surface: ProtocolSurface,
    options: &HarnessOptions,
    mut outcome: FixtureOutcome,
) -> FixtureOutcome {
    let mut preserve_temporary_evidence = false;
    let retain_success = case.tool == "jq"
        && matches!(outcome.status, FixtureStatus::Pass)
        && options.artifact_dir.is_some();

    if matches!(outcome.status, FixtureStatus::Fail(_)) || retain_success {
        let evidence = root.join("evidence");
        if let Err(error) = std::fs::create_dir_all(&evidence)
            .map_err(|error| format!("create fixture evidence directory: {error}"))
            .and_then(|()| write_json(&evidence.join("outcome.json"), &outcome.as_json()))
        {
            append_failure(&mut outcome, format!("write fixture evidence: {error}"));
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
                            "{error}; preserved temporary fixture evidence at {}",
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

fn run_fixture_case_inner(
    case: &FixtureCase,
    surface: ProtocolSurface,
    timeout: Duration,
    workspace: &FixtureWorkspace,
) -> Result<(), String> {
    let jq_contract = jq_contract_case(case)?;
    let config = write_pkl_config(
        &workspace.project,
        &case.tool,
        &case.pkl_property,
        jq_contract.as_ref(),
    )?;
    let input = build_fixture_input(case, surface, &workspace.project, jq_contract.as_ref())?;
    std::fs::write(workspace.evidence.join("input.json"), input.bytes())
        .map_err(|error| format!("write input evidence: {error}"))?;

    let jq_trace = jq_contract
        .as_ref()
        .map(|_| JqTraceHarness::prepare(workspace, &case.spec))
        .transpose()?;
    let before = jq_contract
        .as_ref()
        .map(|_| TreeSnapshot::read(&workspace.project))
        .transpose()?;
    if let Some(before) = &before {
        write_json(
            &workspace.evidence.join("workspace-before.json"),
            &before.as_json(),
        )?;
    }

    let binary = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary);
    command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(&config)
        .arg("post-tool-immediate");
    input.configure_command(&mut command);
    if let Some(trace) = &jq_trace {
        trace.configure(&mut command, "immediate-1")?;
    }
    let output = run_with_timeout(&mut command, input.bytes(), timeout, &workspace.evidence)
        .map_err(|error| format!("run {binary} for {surface}: {error}"))?;
    std::fs::write(
        workspace.evidence.join("exit.txt"),
        format!("{}\n", output.status.code().unwrap_or(-1)),
    )
    .map_err(|error| format!("write exit evidence: {error}"))?;
    verify_outputs(case, surface, &workspace.project, &output)?;

    let Some(contract) = jq_contract.as_ref() else {
        return Ok(());
    };
    let trace = jq_trace.as_ref().expect("jq contract has trace harness");
    verify_jq_trace(
        trace,
        "immediate-1",
        contract,
        &workspace.project,
        &workspace.evidence.join("immediate-1-trace.json"),
    )?;
    let after_first = TreeSnapshot::read(&workspace.project)?;
    let first_diff = before
        .as_ref()
        .expect("jq contract has before snapshot")
        .diff(&after_first);
    verify_jq_first_workspace_diff(contract, &first_diff)?;
    write_json(
        &workspace.evidence.join("workspace-after-immediate-1.json"),
        &after_first.as_json(),
    )?;
    write_json(
        &workspace.evidence.join("workspace-immediate-1-diff.json"),
        &first_diff.as_json(),
    )?;
    verify_jq_immediate_artifact(contract, &workspace.project)?;

    let repeat_dir = workspace.evidence.join("immediate-repeat");
    let mut repeat_command = Command::new(binary);
    repeat_command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(&config)
        .arg("post-tool-immediate");
    input.configure_command(&mut repeat_command);
    trace.configure(&mut repeat_command, "immediate-2")?;
    let repeated = run_with_timeout(&mut repeat_command, input.bytes(), timeout, &repeat_dir)
        .map_err(|error| format!("repeat {binary} for {surface}: {error}"))?;
    std::fs::write(
        repeat_dir.join("exit.txt"),
        format!("{}\n", repeated.status.code().unwrap_or(-1)),
    )
    .map_err(|error| format!("write repeated exit evidence: {error}"))?;
    verify_repeated_output(&output, &repeated, &workspace.project)?;
    verify_jq_trace(
        trace,
        "immediate-2",
        contract,
        &workspace.project,
        &workspace.evidence.join("immediate-2-trace.json"),
    )?;
    let after_second = TreeSnapshot::read(&workspace.project)?;
    let repeat_diff = after_first.diff(&after_second);
    if !repeat_diff.is_empty() {
        return Err(format!(
            "jq immediate repeat was not idempotent: {}",
            repeat_diff.describe()
        ));
    }
    write_json(
        &workspace.evidence.join("workspace-immediate-2-diff.json"),
        &repeat_diff.as_json(),
    )?;

    let mut deferred_contract = None;
    for attempt in 1..=2 {
        let observed = run_jq_deferred_attempt(
            surface,
            timeout,
            workspace,
            &config,
            &input,
            trace,
            contract,
            attempt,
            &after_second,
        )?;
        if let Some(expected) = &deferred_contract {
            if expected != &observed {
                return Err(format!(
                    "jq deferred repeat changed its semantic evidence\nfirst:\n{}\nsecond:\n{}",
                    serde_json::to_string_pretty(expected)
                        .unwrap_or_else(|_| format!("{expected:?}")),
                    serde_json::to_string_pretty(&observed)
                        .unwrap_or_else(|_| format!("{observed:?}")),
                ));
            }
        }
        deferred_contract = Some(observed);
    }
    write_json(
        &workspace.evidence.join("deferred-idempotence.json"),
        &serde_json::json!({
            "formatVersion": 1,
            "attempts": 2,
            "equal": true,
            "contract": deferred_contract,
        }),
    )?;
    Ok(())
}

fn build_fixture_input(
    case: &FixtureCase,
    surface: ProtocolSurface,
    project: &Path,
    jq_contract: Option<&JqContractCase>,
) -> Result<support::native_events::NativePostToolInput, String> {
    let mut builder = PostToolUseBuilder::new(surface, project, &case.entry).identity(
        "test-session",
        "test-turn",
        format!("{}-tool", case.tool),
    );
    if let Some(contract) = jq_contract {
        for (relative, _) in contract.targets {
            let target = project.join(relative);
            if !target.is_file() {
                return Err(format!(
                    "jq contract target is not a fixture file: {target:?}"
                ));
            }
        }
        if contract.targets.len() > 1 {
            let mut patch = String::from("*** Begin Patch\n");
            for (relative, _) in contract.targets {
                patch.push_str(&format!("*** Update File: {relative}\n@@\n"));
            }
            patch.push_str("*** End Patch\n");
            builder = builder.tool(
                "apply_patch",
                serde_json::json!({"patch": patch}),
                serde_json::json!({"exit_code": 0}),
            );
        }
    }
    builder.build()
}

struct JqTraceHarness {
    shim_dir: PathBuf,
    trace_root: PathBuf,
    real_program: PathBuf,
}

impl JqTraceHarness {
    fn prepare(workspace: &FixtureWorkspace, spec: &ToolSpec) -> Result<Self, String> {
        if spec.executable != "jq" {
            return Err(format!(
                "jq contract expected logical executable `jq`, got {:?}",
                spec.executable
            ));
        }
        let real_program = resolve_program(&spec.executable)
            .ok_or_else(|| "jq contract could not resolve pinned jq before tracing".to_owned())?;
        let real_program = real_program.canonicalize().map_err(|error| {
            format!("canonicalize jq contract executable {real_program:?}: {error}")
        })?;
        let shim_dir = workspace.root.join("jq-shim");
        let trace_root = workspace.root.join("jq-traces");
        std::fs::create_dir_all(&shim_dir)
            .map_err(|error| format!("create jq shim directory {shim_dir:?}: {error}"))?;
        std::fs::create_dir_all(&trace_root)
            .map_err(|error| format!("create jq trace directory {trace_root:?}: {error}"))?;
        let shim = shim_dir.join("jq");
        std::fs::write(&shim, include_bytes!("support/jq-trace.sh"))
            .map_err(|error| format!("write jq trace shim {shim:?}: {error}"))?;
        #[cfg(unix)]
        {
            let mut permissions = std::fs::metadata(&shim)
                .map_err(|error| format!("jq trace shim metadata {shim:?}: {error}"))?
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&shim, permissions)
                .map_err(|error| format!("make jq trace shim executable {shim:?}: {error}"))?;
        }
        Ok(Self {
            shim_dir,
            trace_root,
            real_program,
        })
    }

    fn configure(&self, command: &mut Command, label: &str) -> Result<(), String> {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(self.shim_dir.clone()).chain(std::env::split_paths(&inherited)),
        )
        .map_err(|error| format!("construct jq trace PATH: {error}"))?;
        let trace_dir = self.trace_root.join(label);
        std::fs::create_dir_all(&trace_dir)
            .map_err(|error| format!("create jq trace attempt {trace_dir:?}: {error}"))?;
        command
            .env("PATH", path)
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .env("NO_COLOR", "1")
            .env("CLICOLOR", "0")
            .env("FORCE_COLOR", "0")
            .env(JQ_TRACE_DIR_ENV, trace_dir)
            .env(JQ_REAL_PROGRAM_ENV, &self.real_program)
            .env(JQ_LOGICAL_PROGRAM_ENV, "jq")
            .env(JQ_TRACE_SENTINEL_ENV, JQ_TRACE_SENTINEL);
        Ok(())
    }
}

fn verify_jq_trace(
    harness: &JqTraceHarness,
    label: &str,
    contract: &JqContractCase,
    project: &Path,
    evidence_path: &Path,
) -> Result<(), String> {
    let trace_dir = harness.trace_root.join(label).join("invocations");
    let invocations = sorted_entries(&trace_dir)?
        .into_iter()
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    if invocations.len() != contract.targets.len() {
        return Err(format!(
            "jq {label} expected {} per-file invocation(s), observed {} at {trace_dir:?}",
            contract.targets.len(),
            invocations.len()
        ));
    }

    let mut expected = contract
        .targets
        .iter()
        .map(|(relative, status)| {
            let target = canonical_project(&project.join(relative));
            (target, *status)
        })
        .collect::<Vec<_>>();
    expected.sort_by(|left, right| left.0.cmp(&right.0));
    let cwd = canonical_project(project);
    let mut records = Vec::new();
    for (invocation, (target, expected_status)) in invocations.iter().zip(expected) {
        let record = invocation.path();
        assert_record(&record, "logical-program", "jq")?;
        for (name, expected) in [
            ("LANG", "C"),
            ("LC_ALL", "C"),
            ("TZ", "UTC"),
            ("NO_COLOR", "1"),
            ("CLICOLOR", "0"),
            ("FORCE_COLOR", "0"),
            (JQ_TRACE_SENTINEL_ENV, JQ_TRACE_SENTINEL),
        ] {
            assert_record(&record, &format!("env-{name}"), expected)?;
        }
        assert_record(
            &record,
            "real-program",
            harness.real_program.to_string_lossy().as_ref(),
        )?;
        assert_record(&record, "cwd", cwd.to_string_lossy().trim_end())?;
        let mut expected_args = vec!["empty".to_owned()];
        expected_args.extend(contract.extra_args.iter().map(|value| (*value).to_owned()));
        expected_args.push(target.to_string_lossy().into_owned());
        assert_record(&record, "argc", &expected_args.len().to_string())?;
        for (index, argument) in expected_args.iter().enumerate() {
            assert_record(&record, &format!("argv-{index}"), argument)?;
        }
        assert_record(&record, "status", &expected_status.to_string())?;
        let program = read_record(&record, "program")?;
        if Path::new(&program).file_name() != Some(OsStr::new("jq")) {
            return Err(format!(
                "jq {label} trace recorded unexpected shim program {program:?}"
            ));
        }
        records.push(serde_json::json!({
            "logicalProgram": "jq",
            "shimProgram": program,
            "realProgram": harness.real_program,
            "cwd": cwd,
            "argv": expected_args,
            "environment": {
                "LANG": "C",
                "LC_ALL": "C",
                "TZ": "UTC",
                "NO_COLOR": "1",
                "CLICOLOR": "0",
                "FORCE_COLOR": "0",
                JQ_TRACE_SENTINEL_ENV: JQ_TRACE_SENTINEL,
            },
            "exitCode": expected_status,
        }));
    }
    write_json(
        evidence_path,
        &serde_json::json!({
            "formatVersion": 1,
            "label": label,
            "invocations": records,
        }),
    )
}

fn read_record(record: &Path, name: &str) -> Result<String, String> {
    let path = record.join(name);
    std::fs::read_to_string(&path)
        .map(|value| value.trim_end().to_owned())
        .map_err(|error| format!("read jq trace record {path:?}: {error}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeSnapshot {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl TreeSnapshot {
    fn read(root: &Path) -> Result<Self, String> {
        let mut files = BTreeMap::new();
        collect_snapshot_files(root, root, &mut files)?;
        Ok(Self { files })
    }

    fn diff(&self, after: &Self) -> TreeDiff {
        let paths = self
            .files
            .keys()
            .chain(after.files.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();
        for path in paths {
            match (self.files.get(&path), after.files.get(&path)) {
                (None, Some(_)) => added.push(path),
                (Some(_), None) => removed.push(path),
                (Some(before), Some(after)) if before != after => changed.push(path),
                (Some(_), Some(_)) => {}
                (None, None) => unreachable!(),
            }
        }
        TreeDiff {
            added,
            removed,
            changed,
        }
    }

    fn as_json(&self) -> JsonValue {
        JsonValue::Object(
            self.files
                .iter()
                .map(|(path, bytes)| {
                    (
                        slash_path(path),
                        serde_json::json!({
                            "bytes": bytes.len(),
                            "hex": hex_bytes(bytes),
                        }),
                    )
                })
                .collect(),
        )
    }
}

#[derive(Debug)]
struct TreeDiff {
    added: Vec<PathBuf>,
    removed: Vec<PathBuf>,
    changed: Vec<PathBuf>,
}

impl TreeDiff {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    fn describe(&self) -> String {
        format!(
            "added={:?}, removed={:?}, changed={:?}",
            self.added, self.removed, self.changed
        )
    }

    fn as_json(&self) -> JsonValue {
        serde_json::json!({
            "added": self.added.iter().map(|path| slash_path(path)).collect::<Vec<_>>(),
            "removed": self.removed.iter().map(|path| slash_path(path)).collect::<Vec<_>>(),
            "changed": self.changed.iter().map(|path| slash_path(path)).collect::<Vec<_>>(),
        })
    }
}

fn collect_snapshot_files(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Result<(), String> {
    for entry in sorted_entries(current)? {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("snapshot file type for {path:?}: {error}"))?;
        if file_type.is_dir() {
            collect_snapshot_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| format!("snapshot relative path for {path:?}: {error}"))?
                .to_path_buf();
            let bytes =
                std::fs::read(&path).map_err(|error| format!("snapshot file {path:?}: {error}"))?;
            files.insert(relative, bytes);
        } else {
            return Err(format!("snapshot does not support {path:?}"));
        }
    }
    Ok(())
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn verify_jq_first_workspace_diff(
    contract: &JqContractCase,
    diff: &TreeDiff,
) -> Result<(), String> {
    if !diff.removed.is_empty() || !diff.changed.is_empty() {
        return Err(format!(
            "jq changed or removed existing workspace files: {}",
            diff.describe()
        ));
    }
    let expected_artifacts = usize::from(contract.outcome != JqExpectedOutcome::Clean);
    if diff.added.len() != expected_artifacts
        || diff.added.iter().any(|path| {
            !path.starts_with(Path::new(".velvet-glove/jq-agent-hook"))
                || path.extension() != Some(OsStr::new("txt"))
        })
    {
        return Err(format!(
            "jq workspace diff contained unexpected additions: {}",
            diff.describe()
        ));
    }
    Ok(())
}

fn verify_jq_immediate_artifact(contract: &JqContractCase, project: &Path) -> Result<(), String> {
    let directory = project.join(".velvet-glove/jq-agent-hook");
    let artifacts = if directory.is_dir() {
        sorted_entries(&directory)?
            .into_iter()
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.path())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let expected = usize::from(contract.outcome != JqExpectedOutcome::Clean);
    if artifacts.len() != expected {
        return Err(format!(
            "jq immediate expected {expected} diagnostic artifact(s), found {artifacts:?}"
        ));
    }
    let Some(path) = artifacts.first() else {
        return Ok(());
    };
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("read jq immediate artifact {path:?}: {error}"))?;
    let classification = match contract.outcome {
        JqExpectedOutcome::Clean => unreachable!(),
        JqExpectedOutcome::Issues => "classification: Some(Issues)",
        JqExpectedOutcome::OperationalFailure => "classification: Some(Failure)",
    };
    if !contents.contains(classification) {
        return Err(format!(
            "jq immediate artifact lacks {classification:?}:\n{contents}"
        ));
    }
    if let Some(needle) = contract.diagnostic_contains {
        if !contents.contains(needle) {
            return Err(format!(
                "jq immediate artifact lacks stable diagnostic {needle:?}:\n{contents}"
            ));
        }
    }
    Ok(())
}

fn verify_repeated_output(
    first: &BoundedOutput,
    second: &BoundedOutput,
    project: &Path,
) -> Result<(), String> {
    let aliases = workspace_path_aliases(project);
    let first_stdout = normalize(&String::from_utf8_lossy(&first.stdout), &aliases);
    let second_stdout = normalize(&String::from_utf8_lossy(&second.stdout), &aliases);
    let first_stderr = normalize(&String::from_utf8_lossy(&first.stderr), &aliases);
    let second_stderr = normalize(&String::from_utf8_lossy(&second.stderr), &aliases);
    if first.status.code() != second.status.code()
        || first_stdout != second_stdout
        || first_stderr != second_stderr
    {
        return Err(format!(
            "jq immediate repeat changed its observable result\nfirst stdout:\n{first_stdout}\nsecond stdout:\n{second_stdout}\nfirst stderr:\n{first_stderr}\nsecond stderr:\n{second_stderr}"
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_jq_deferred_attempt(
    surface: ProtocolSurface,
    timeout: Duration,
    workspace: &FixtureWorkspace,
    config: &Path,
    immediate_input: &support::native_events::NativePostToolInput,
    trace: &JqTraceHarness,
    contract: &JqContractCase,
    attempt: usize,
    project_baseline: &TreeSnapshot,
) -> Result<JsonValue, String> {
    let state_dir = workspace.root.join(format!("deferred-state-{attempt}"));
    seed_jq_pending_targets(&state_dir, surface, &workspace.project, contract)?;
    let turn_input = jq_turn_completion_input(surface, &workspace.project)?;
    let evidence = workspace.evidence.join(format!("deferred-{attempt}"));
    std::fs::create_dir_all(&evidence)
        .map_err(|error| format!("create jq deferred evidence {evidence:?}: {error}"))?;
    std::fs::write(evidence.join("input.json"), &turn_input)
        .map_err(|error| format!("write jq deferred input evidence: {error}"))?;

    let binary = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary);
    command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(config)
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("turn-completion");
    immediate_input.configure_command(&mut command);
    let trace_label = format!("deferred-{attempt}");
    trace.configure(&mut command, &trace_label)?;
    let output = run_with_timeout(&mut command, &turn_input, timeout, &evidence)
        .map_err(|error| format!("run deferred {binary} for {surface}: {error}"))?;
    std::fs::write(
        evidence.join("exit.txt"),
        format!("{}\n", output.status.code().unwrap_or(-1)),
    )
    .map_err(|error| format!("write jq deferred exit evidence: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "jq deferred {surface} attempt {attempt} exited {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    let native: JsonValue = serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "parse jq deferred {surface} attempt {attempt} native output: {error}\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })?;
    verify_jq_deferred_native(contract, &native, surface)?;
    verify_jq_trace(
        trace,
        &trace_label,
        contract,
        &workspace.project,
        &workspace
            .evidence
            .join(format!("deferred-{attempt}-trace.json")),
    )?;

    let summaries = files_named(&state_dir, "summary.json");
    if summaries.len() != 1 {
        return Err(format!(
            "jq deferred {surface} attempt {attempt} expected one summary, found {summaries:?}"
        ));
    }
    let summary_path = &summaries[0];
    let summary: JsonValue = serde_json::from_slice(
        &std::fs::read(summary_path)
            .map_err(|error| format!("read jq deferred summary {summary_path:?}: {error}"))?,
    )
    .map_err(|error| format!("parse jq deferred summary {summary_path:?}: {error}"))?;
    let semantic = verify_jq_deferred_summary(contract, &workspace.project, &summary)?;
    write_json(
        &workspace
            .evidence
            .join(format!("deferred-{attempt}-summary.json")),
        &summary,
    )?;
    write_json(
        &workspace
            .evidence
            .join(format!("deferred-{attempt}-semantic.json")),
        &semantic,
    )?;

    let after = TreeSnapshot::read(&workspace.project)?;
    let diff = project_baseline.diff(&after);
    if !diff.is_empty() {
        return Err(format!(
            "jq deferred {surface} attempt {attempt} mutated the fixture workspace: {}",
            diff.describe()
        ));
    }
    write_json(
        &workspace
            .evidence
            .join(format!("workspace-deferred-{attempt}-diff.json")),
        &diff.as_json(),
    )?;
    Ok(semantic)
}

fn seed_jq_pending_targets(
    state_dir: &Path,
    surface: ProtocolSurface,
    project: &Path,
    contract: &JqContractCase,
) -> Result<(), String> {
    let state = hookkit_session_state::SessionState::open(
        surface.harness_id(),
        hookkit_session_state::SessionIdentity::Session("test-session".into()),
        hookkit_session_state::StateRoot::new(state_dir),
    )
    .map_err(|error| format!("open jq deferred state for {surface}: {error}"))?;
    let store = hookkit_file_activity::FileActivityStore::from_state(state)
        .map_err(|error| format!("open jq file-activity state for {surface}: {error}"))?;
    let targets = contract
        .targets
        .iter()
        .map(|(relative, _)| {
            let path = canonical_project(&project.join(relative));
            hookkit_core::Utf8PathBuf::from_path_buf(path.clone())
                .map(hookkit_file_activity::FileActivityTarget::exact)
                .map_err(|path| format!("jq deferred target is not UTF-8: {path:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    store
        .requeue_targets("jq-real-tool-fixture", targets)
        .map(|_| ())
        .map_err(|error| format!("seed jq deferred targets for {surface}: {error}"))
}

fn jq_turn_completion_input(surface: ProtocolSurface, project: &Path) -> Result<Vec<u8>, String> {
    let value = match surface {
        ProtocolSurface::Claude => serde_json::json!({
            "session_id": "test-session",
            "transcript_path": "/tmp/velvet-glove-jq-fixture.jsonl",
            "cwd": project,
            "hook_event_name": "Stop",
            "stop_hook_active": false,
            "last_assistant_message": "done",
        }),
        ProtocolSurface::Codex => serde_json::json!({
            "session_id": "test-session",
            "transcript_path": "/tmp/velvet-glove-jq-fixture.jsonl",
            "cwd": project,
            "hook_event_name": "Stop",
            "model": "fixture-model",
            "turn_id": "test-turn",
            "permission_mode": "default",
            "stop_hook_active": false,
            "last_assistant_message": "done",
        }),
        ProtocolSurface::Antigravity => {
            return Err("jq real-tool lane does not execute Antigravity".to_owned());
        }
    };
    serde_json::to_vec(&value)
        .map_err(|error| format!("serialize jq deferred {surface} input: {error}"))
}

fn verify_jq_deferred_native(
    contract: &JqContractCase,
    native: &JsonValue,
    surface: ProtocolSurface,
) -> Result<(), String> {
    match contract.outcome {
        JqExpectedOutcome::Clean => {
            if native.get("decision").is_some() {
                return Err(format!(
                    "jq clean deferred {surface} unexpectedly blocked: {native}"
                ));
            }
            let message = native
                .get("systemMessage")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!("jq clean deferred {surface} lacked systemMessage: {native}")
                })?;
            if !message.contains("Checked") || !message.contains("clean") {
                return Err(format!(
                    "jq clean deferred {surface} had unexpected systemMessage: {message:?}"
                ));
            }
        }
        JqExpectedOutcome::Issues | JqExpectedOutcome::OperationalFailure => {
            if native.get("decision").and_then(JsonValue::as_str) != Some("block") {
                return Err(format!(
                    "jq deferred {surface} expected a native block: {native}"
                ));
            }
        }
    }
    Ok(())
}

fn verify_jq_deferred_summary(
    contract: &JqContractCase,
    project: &Path,
    summary: &JsonValue,
) -> Result<JsonValue, String> {
    let expected_status = match contract.outcome {
        JqExpectedOutcome::Clean => "clean",
        JqExpectedOutcome::Issues => "issues",
        JqExpectedOutcome::OperationalFailure => "operational-failure",
    };
    require_json_string(summary, "status", expected_status)?;
    let expected_targets = contract
        .targets
        .iter()
        .map(|(relative, status)| (canonical_project(&project.join(relative)), *status))
        .collect::<BTreeMap<_, _>>();
    require_path_array(summary, "candidateFiles", expected_targets.keys())?;

    let result = require_json_object(summary, "result")?;
    let artifacts = require_json_object_value(result, "artifacts")?;
    if artifacts.len() != expected_targets.len() {
        return Err(format!(
            "jq deferred expected {} artifacts, got {}: {artifacts:?}",
            expected_targets.len(),
            artifacts.len()
        ));
    }
    let reports = require_json_object_value(result, "reports")?;
    if reports.len() != expected_targets.len() {
        return Err(format!(
            "jq deferred expected {} reports, got {}: {reports:?}",
            expected_targets.len(),
            reports.len()
        ));
    }

    let cwd = canonical_project(project);
    let mut artifact_contracts = Vec::new();
    for (target, exit_code) in &expected_targets {
        let expected_classification = match *exit_code {
            0 => "clean",
            5 => "issues",
            2 => "failure",
            other => {
                return Err(format!("unsupported jq fixture exit code {other}"));
            }
        };
        let artifact = artifacts
            .values()
            .find(|artifact| json_path_array_equals(artifact, "candidateFiles", [target]))
            .ok_or_else(|| format!("jq deferred artifact missing for {target:?}: {artifacts:?}"))?;
        require_json_string(artifact, "toolId", "jq")?;
        require_json_string(artifact, "workflowId", "verify")?;
        require_json_string(artifact, "phase", "initial-check")?;
        require_json_string(artifact, "classification", expected_classification)?;
        require_json_i64(artifact, "exitCode", i64::from(*exit_code))?;
        require_json_string(artifact, "program", "jq")?;
        let mut expected_args = vec!["empty".to_owned()];
        expected_args.extend(contract.extra_args.iter().map(|value| (*value).to_owned()));
        expected_args.push(target.to_string_lossy().into_owned());
        require_string_array(artifact, "arguments", &expected_args)?;
        require_json_path(artifact, "workingDirectory", &cwd)?;
        require_path_array(artifact, "files", [target])?;
        require_path_array(artifact, "candidateFiles", [target])?;
        require_empty_array(artifact, "changedFiles")?;
        let contents = artifact
            .get("contents")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| format!("jq deferred artifact lacks text contents: {artifact}"))?;
        if let Some(needle) = contract.diagnostic_contains {
            if !contents.contains(needle) {
                return Err(format!(
                    "jq deferred artifact for {target:?} lacks {needle:?}:\n{contents}"
                ));
            }
        }
        let absolute = artifact
            .get("absolutePath")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| format!("jq deferred artifact lacks absolutePath: {artifact}"))?;
        let on_disk = std::fs::read_to_string(absolute)
            .map_err(|error| format!("read jq deferred artifact {absolute:?}: {error}"))?;
        if on_disk != contents {
            return Err(format!(
                "jq deferred artifact contents differ from {absolute:?}"
            ));
        }
        artifact_contracts.push(serde_json::json!({
            "target": target,
            "phase": "initial-check",
            "classification": expected_classification,
            "exitCode": exit_code,
            "program": "jq",
            "arguments": expected_args,
            "workingDirectory": cwd,
            "changedFiles": [],
            "contents": contents,
        }));

        let report = reports
            .values()
            .find(|report| json_path_array_equals(report, "candidateFiles", [target]))
            .ok_or_else(|| format!("jq deferred report missing for {target:?}: {reports:?}"))?;
        require_json_string(report, "toolId", "jq")?;
        require_json_string(report, "workflowId", "verify")?;
        require_path_array(report, "candidateFiles", [target])?;
        require_empty_array(report, "changedFiles")?;
        require_json_bool(report, "fixAttempted", false)?;
        require_json_bool(report, "conservativeAttribution", false)?;
        let expected_check = match *exit_code {
            0 => Some("clean"),
            5 => Some("issues"),
            2 => None,
            _ => unreachable!(),
        };
        require_optional_json_string(report, "initialCheck", expected_check)?;
        require_optional_json_string(report, "finalCheck", expected_check)?;
    }
    artifact_contracts
        .sort_by(|left, right| left["target"].as_str().cmp(&right["target"].as_str()));

    let files = require_json_object_value(result, "files")?;
    let operational = require_json_object_value(result, "operationalProblems")?;
    match contract.outcome {
        JqExpectedOutcome::Clean | JqExpectedOutcome::Issues => {
            if files.len() != expected_targets.len() || !operational.is_empty() {
                return Err(format!(
                    "jq deferred normal result shape mismatch: files={files:?}, operational={operational:?}"
                ));
            }
            let status = if contract.outcome == JqExpectedOutcome::Clean {
                "clean"
            } else {
                "manual-fixes-needed"
            };
            for target in expected_targets.keys() {
                let file = files
                    .values()
                    .find(|file| json_path_equals(file, "path", target))
                    .ok_or_else(|| format!("jq deferred file result missing for {target:?}"))?;
                require_json_string(file, "status", status)?;
                require_json_bool(file, "changedByRunner", false)?;
            }
        }
        JqExpectedOutcome::OperationalFailure => {
            if !files.is_empty() || operational.len() != expected_targets.len() {
                return Err(format!(
                    "jq deferred operational shape mismatch: files={files:?}, operational={operational:?}"
                ));
            }
            for target in expected_targets.keys() {
                let problem = operational
                    .values()
                    .find(|problem| json_path_array_equals(problem, "affectedFiles", [target]))
                    .ok_or_else(|| {
                        format!("jq deferred operational problem missing for {target:?}")
                    })?;
                require_json_string(problem, "toolId", "jq")?;
                require_json_string(problem, "phase", "initial-check")?;
            }
        }
    }

    Ok(serde_json::json!({
        "status": expected_status,
        "targets": expected_targets.keys().collect::<Vec<_>>(),
        "artifacts": artifact_contracts,
        "fileStatuses": files.values().map(|file| serde_json::json!({
            "path": file.get("path"),
            "status": file.get("status"),
            "changedByRunner": file.get("changedByRunner"),
        })).collect::<Vec<_>>(),
        "operationalProblems": operational.values().map(|problem| serde_json::json!({
            "toolId": problem.get("toolId"),
            "phase": problem.get("phase"),
            "affectedFiles": problem.get("affectedFiles"),
            "message": problem.get("message"),
        })).collect::<Vec<_>>(),
    }))
}

fn require_json_object<'a>(value: &'a JsonValue, field: &str) -> Result<&'a JsonValue, String> {
    let child = value
        .get(field)
        .ok_or_else(|| format!("missing JSON field {field:?}: {value}"))?;
    if !child.is_object() {
        return Err(format!("JSON field {field:?} is not an object: {child}"));
    }
    Ok(child)
}

fn require_json_object_value<'a>(
    value: &'a JsonValue,
    field: &str,
) -> Result<&'a serde_json::Map<String, JsonValue>, String> {
    value
        .get(field)
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("JSON field {field:?} is not an object: {value}"))
}

fn require_json_string(value: &JsonValue, field: &str, expected: &str) -> Result<(), String> {
    let actual = value.get(field).and_then(JsonValue::as_str);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} mismatch: expected {expected:?}, got {actual:?} in {value}"
        ))
    }
}

fn require_optional_json_string(
    value: &JsonValue,
    field: &str,
    expected: Option<&str>,
) -> Result<(), String> {
    let actual = value.get(field).and_then(JsonValue::as_str);
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} mismatch: expected {expected:?}, got {actual:?} in {value}"
        ))
    }
}

fn require_json_i64(value: &JsonValue, field: &str, expected: i64) -> Result<(), String> {
    let actual = value.get(field).and_then(JsonValue::as_i64);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} mismatch: expected {expected}, got {actual:?} in {value}"
        ))
    }
}

fn require_json_bool(value: &JsonValue, field: &str, expected: bool) -> Result<(), String> {
    let actual = value.get(field).and_then(JsonValue::as_bool);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} mismatch: expected {expected}, got {actual:?} in {value}"
        ))
    }
}

fn require_json_path(value: &JsonValue, field: &str, expected: &Path) -> Result<(), String> {
    if json_path_equals(value, field, expected) {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} path mismatch: expected {expected:?}, got {:?}",
            value.get(field)
        ))
    }
}

fn json_path_equals(value: &JsonValue, field: &str, expected: &Path) -> bool {
    value
        .get(field)
        .and_then(JsonValue::as_str)
        .is_some_and(|actual| Path::new(actual) == expected)
}

fn json_path_array_equals<'a>(
    value: &JsonValue,
    field: &str,
    expected: impl IntoIterator<Item = &'a PathBuf>,
) -> bool {
    let expected = expected
        .into_iter()
        .map(PathBuf::as_path)
        .collect::<Vec<_>>();
    value
        .get(field)
        .and_then(JsonValue::as_array)
        .is_some_and(|actual| {
            actual.len() == expected.len()
                && actual.iter().zip(expected).all(|(actual, expected)| {
                    actual
                        .as_str()
                        .is_some_and(|actual| Path::new(actual) == expected)
                })
        })
}

fn require_path_array<'a>(
    value: &JsonValue,
    field: &str,
    expected: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<(), String> {
    if json_path_array_equals(value, field, expected) {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} paths mismatch: got {:?} in {value}",
            value.get(field)
        ))
    }
}

fn require_string_array(value: &JsonValue, field: &str, expected: &[String]) -> Result<(), String> {
    let actual = value
        .get(field)
        .and_then(JsonValue::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .collect::<Vec<_>>()
        });
    if actual.as_deref()
        == Some(
            expected
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
        )
    {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} mismatch: expected {expected:?}, got {actual:?} in {value}"
        ))
    }
}

fn require_empty_array(value: &JsonValue, field: &str) -> Result<(), String> {
    if value
        .get(field)
        .and_then(JsonValue::as_array)
        .is_some_and(Vec::is_empty)
    {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} expected an empty array, got {:?}",
            value.get(field)
        ))
    }
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
        } else if path.file_name().and_then(OsStr::to_str) == Some(name) {
            found.push(path);
        }
    }
    found.sort();
    found
}

fn verify_outputs(
    case: &FixtureCase,
    surface: ProtocolSurface,
    project: &Path,
    output: &BoundedOutput,
) -> Result<(), String> {
    let actual_stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let actual_stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let actual_exit = output.status.code().unwrap_or(-1);
    let project_paths = workspace_path_aliases(project);

    let stdout_golden_path = case.directory.join(format!("{}.json", surface.cli_name()));
    let golden_stdout = if stdout_golden_path.exists() {
        std::fs::read_to_string(&stdout_golden_path)
            .map_err(|error| format!("read {stdout_golden_path:?}: {error}"))?
    } else {
        "{}".to_owned()
    };
    let golden_stdout = normalize(&golden_stdout, &project_paths);
    let actual_stdout_normalized = normalize(&actual_stdout, &project_paths);
    match (
        serde_json::from_str::<JsonValue>(&golden_stdout),
        serde_json::from_str::<JsonValue>(&actual_stdout_normalized),
    ) {
        (Ok(expected), Ok(actual)) if expected == actual => {}
        (Ok(expected), Ok(actual)) => {
            return Err(format!(
                "stdout JSON mismatch:\n  expected: {expected}\n  actual:   {actual}"
            ));
        }
        _ if golden_stdout.trim() == actual_stdout_normalized.trim() => {}
        _ => {
            return Err(format!(
                "stdout mismatch:\n  expected: {golden_stdout}\n  actual:   {actual_stdout_normalized}"
            ));
        }
    }

    let stderr_golden_path = case
        .directory
        .join(format!("{}.stderr.txt", surface.cli_name()));
    if stderr_golden_path.exists() {
        let expected = std::fs::read_to_string(&stderr_golden_path)
            .map_err(|error| format!("read {stderr_golden_path:?}: {error}"))?;
        let expected = normalize(&expected, &project_paths);
        let actual = normalize(&actual_stderr, &project_paths);
        if expected.trim() != actual.trim() {
            return Err(format!(
                "stderr mismatch:\n  expected:\n{expected}\n  actual:\n{actual}"
            ));
        }
    } else {
        let actual = normalize(&actual_stderr, &project_paths);
        if !actual.trim().is_empty() {
            return Err(format!(
                "stderr expected empty but got:\n{actual}\n(write {stderr_golden_path:?} to assert content)"
            ));
        }
    }

    let exit_golden_path = case.directory.join(format!("{}.exit", surface.cli_name()));
    let expected_exit = if exit_golden_path.exists() {
        std::fs::read_to_string(&exit_golden_path)
            .map_err(|error| format!("read {exit_golden_path:?}: {error}"))?
            .trim()
            .parse::<i32>()
            .map_err(|error| format!("parse {exit_golden_path:?}: {error}"))?
    } else {
        0
    };
    if actual_exit != expected_exit {
        return Err(format!(
            "exit code mismatch: expected {expected_exit}, got {actual_exit}\nstdout:\n{actual_stdout}\nstderr:\n{actual_stderr}"
        ));
    }

    let expected_root = case.directory.join("expected");
    if expected_root.exists() {
        verify_expected_tree(&expected_root, &expected_root, project)?;
    }
    Ok(())
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

fn write_pkl_config(
    project: &Path,
    tool: &str,
    property: &str,
    jq_contract: Option<&JqContractCase>,
) -> Result<PathBuf, String> {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir)
        .map_err(|error| format!("create config directory {config_dir:?}: {error}"))?;
    let tool_definition = if let Some(contract) = jq_contract {
        let extra_args = contract
            .extra_args
            .iter()
            .map(|argument| format!("\"{}\"", pkl_string(argument)))
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            r#"(Builtins.{property}) {{
    phases {{
      ["verify"] {{
        extraArgs = new Listing<String> {{ {extra_args} }}
      }}
    }}
  }}"#
        )
    } else {
        format!("Builtins.{property}")
    };
    let contract_settings = if jq_contract.is_some() {
        "  jobs = 1\n  fileActivity { filesystemMtime = false }\n"
    } else {
        ""
    };
    let body = format!(
        r#"amends "Config.pkl"
import "Builtins.pkl"

settings {{
{contract_settings}
  diagnosticsDirectory = ".velvet-glove/{tool}-agent-hook"
}}

tools {{
  ["{tool}"] = {tool_definition}
}}
run = new Listing<String> {{ "{tool}" }}
"#
    );
    let path = config_dir.join("post-tool-use.pkl");
    std::fs::write(&path, body).map_err(|error| format!("write post-tool-use.pkl: {error}"))?;
    Ok(path)
}

fn copy_fixture_inputs(root: &Path, current: &Path, target: &Path) -> Result<(), String> {
    for entry in sorted_entries(current)? {
        let path = entry.path();
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if current == root
            && (name == OsStr::new("expected")
                || name == OsStr::new("README.md")
                || is_golden_output(&name_text))
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {path:?}: {error}"))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("strip fixture prefix from {path:?}: {error}"))?;
        let destination = target.join(relative);
        if file_type.is_dir() {
            std::fs::create_dir_all(&destination)
                .map_err(|error| format!("create {destination:?}: {error}"))?;
            copy_fixture_inputs(root, &path, target)?;
        } else if file_type.is_file() {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("create {parent:?}: {error}"))?;
            }
            std::fs::copy(&path, &destination)
                .map_err(|error| format!("copy {path:?} to {destination:?}: {error}"))?;
        } else {
            return Err(format!("unsupported fixture entry type: {path:?}"));
        }
    }
    Ok(())
}

fn find_entry_file(directory: &Path) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    for entry in sorted_entries(directory)? {
        let path = entry.path();
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if name == OsStr::new("expected")
            || name == OsStr::new("README.md")
            || is_golden_output(&name_text)
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {path:?}: {error}"))?;
        if !file_type.is_file() {
            continue;
        }
        if name_text.starts_with("example.") {
            return Ok(PathBuf::from(name));
        }
        candidates.push(PathBuf::from(name));
    }
    candidates.sort();
    candidates.into_iter().next().ok_or_else(|| {
        format!("no entry file in {directory:?}; add an `example.<ext>` at the case root")
    })
}

fn is_golden_output(name: &str) -> bool {
    ProtocolSurface::ALL.iter().any(|surface| {
        name == format!("{}.json", surface.cli_name())
            || name == format!("{}.stderr.txt", surface.cli_name())
            || name == format!("{}.exit", surface.cli_name())
    })
}

fn validate_supported_goldens(directory: &Path) -> Result<(), String> {
    for entry in sorted_entries(directory)? {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {:?}: {error}", entry.path()))?;
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        for surface in ProtocolSurface::ALL {
            let is_surface_golden = name == format!("{}.json", surface.cli_name())
                || name == format!("{}.stderr.txt", surface.cli_name())
                || name == format!("{}.exit", surface.cli_name());
            if is_surface_golden && !REAL_TOOL_SURFACES.contains(&surface) {
                return Err(format!(
                    "{} golden is not executed by the real-tool fixture matrix",
                    surface.cli_name()
                ));
            }
        }
    }
    Ok(())
}

fn check_tool_programs(spec: &ToolSpec) -> Result<(), Vec<String>> {
    let mut programs = BTreeSet::from([spec.executable.as_str()]);
    programs.extend(
        spec.phases
            .values()
            .filter(|phase| phase.enabled)
            .filter_map(|phase| phase.program.as_deref()),
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
    for surface in ProtocolSurface::ALL {
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

fn probe_report(commands: usize, failure: Option<(ProtocolSurface, &str)>) -> JsonValue {
    serde_json::json!({
        "formatVersion": 1,
        "kind": "probe",
        "status": if failure.is_some() { "fail" } else { "pass" },
        "totals": {
            "protocolProbeSurfaces": ProtocolSurface::ALL.len(),
            "commandsExecuted": commands,
        },
        "failure": failure.map(|(surface, detail)| serde_json::json!({
            "surface": surface.cli_name(),
            "detail": detail,
        })),
    })
}

fn run_probe_case(
    surface: ProtocolSurface,
    timeout: Duration,
    artifact_root: Option<&Path>,
) -> Result<usize, String> {
    run_probe_attempt(surface, artifact_root, |root| {
        run_probe_case_inner(surface, timeout, root)
    })
}

fn run_probe_attempt(
    surface: ProtocolSurface,
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
                            "formatVersion": 1,
                            "surface": surface.cli_name(),
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
    surface: ProtocolSurface,
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
    let config = write_probe_config(&project, &probe)?;

    let input = PostToolUseBuilder::new(surface, &project, "example.fixture")
        .identity("probe-session", "probe-turn", "probe-tool")
        .build()?;
    std::fs::write(evidence.join("input.json"), input.bytes())
        .map_err(|error| format!("write probe input: {error}"))?;
    let sentinel = format!("surface:{}", surface.cli_name());
    let binary = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary);
    command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(config)
        .arg("post-tool-immediate");
    input.configure_command(&mut command);
    command
        .env(PROBE_DIR_ENV, &probe_dir)
        .env(PROBE_SENTINEL_ENV, &sentinel);
    let output = run_with_timeout(&mut command, input.bytes(), timeout, &evidence)
        .map_err(|error| format!("{surface} probe through {binary}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{surface} probe exited {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout: JsonValue = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("parse {surface} probe stdout as JSON: {error}"))?;
    if stdout != serde_json::json!({}) {
        return Err(format!(
            "{surface} probe expected {{}} stdout, got {stdout}"
        ));
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

fn write_probe_config(project: &Path, probe: &Path) -> Result<PathBuf, String> {
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
    let path = config_dir.join("post-tool-use.pkl");
    std::fs::write(&path, config).map_err(|error| format!("write probe config: {error}"))?;
    Ok(path)
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
    let output = result.map_err(|error| format!("required Pkl 0.31.1 unavailable: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "required Pkl prerequisite failed with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let version = String::from_utf8_lossy(&output.stdout);
    if !version.starts_with("Pkl 0.31.1 ") {
        return Err(format!(
            "required Pkl version is 0.31.1; found {}",
            version.trim()
        ));
    }
    Ok(())
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
    let mut by_surface = BTreeMap::<&str, [usize; 3]>::new();
    for outcome in outcomes {
        let counts = by_surface
            .entry(outcome.surface.cli_name())
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
                surface.to_owned(),
                serde_json::json!({
                    "passed": counts[0],
                    "skipped": counts[1],
                    "failed": counts[2],
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({
        "formatVersion": 1,
        "kind": "real-tool-fixtures",
        "totals": {
            "tools": catalog.tool_count,
            "cases": catalog.cases.len(),
            "fixtureSurfaces": REAL_TOOL_SURFACES.len(),
            "protocolProbeSurfaces": ProtocolSurface::ALL.len(),
            "plannedSurfaceCases": catalog.cases.len() * REAL_TOOL_SURFACES.len(),
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
                    "FAIL  {}/{} ({}):\n{reason}",
                    outcome.tool, outcome.case, outcome.surface
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
    surface: ProtocolSurface,
) -> Result<PathBuf, String> {
    let destination = artifact_root
        .join(sanitize_component(&case.tool))
        .join(sanitize_component(&case.case))
        .join(format!(
            "{}-{}-{}",
            surface.cli_name(),
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
    surface: ProtocolSurface,
) -> Result<PathBuf, String> {
    let destination = artifact_root
        .join("probe")
        .join(surface.cli_name())
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
    let historical_path = root.join(format!(
        "report-{}-{}.json",
        std::process::id(),
        unique_nonce()
    ));
    write_json(&historical_path, report)?;
    let stable_path = root.join("report.json");
    write_json(&stable_path, report)?;
    Ok(stable_path)
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

fn normalize(text: &str, project_aliases: &[String]) -> String {
    let mut output = text.to_owned();
    let mut aliases = project_aliases.iter().collect::<Vec<_>>();
    aliases.sort_by_key(|alias| std::cmp::Reverse(alias.len()));
    for alias in aliases {
        output = output.replace(alias, "<workspace>");
    }
    output
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
