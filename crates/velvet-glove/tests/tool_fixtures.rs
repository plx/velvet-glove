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
static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

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

fn run_fixture_case_inner(
    case: &FixtureCase,
    surface: ProtocolSurface,
    timeout: Duration,
    workspace: &FixtureWorkspace,
) -> Result<(), String> {
    let config = write_pkl_config(&workspace.project, &case.tool, &case.pkl_property)?;
    let input = PostToolUseBuilder::new(surface, &workspace.project, &case.entry)
        .identity("test-session", "test-turn", format!("{}-tool", case.tool))
        .build()?;
    std::fs::write(workspace.evidence.join("input.json"), input.bytes())
        .map_err(|error| format!("write input evidence: {error}"))?;

    let binary = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary);
    command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(config)
        .arg("post-tool-immediate");
    input.configure_command(&mut command);
    let output = run_with_timeout(&mut command, input.bytes(), timeout, &workspace.evidence)
        .map_err(|error| format!("run {binary} for {surface}: {error}"))?;
    std::fs::write(
        workspace.evidence.join("exit.txt"),
        format!("{}\n", output.status.code().unwrap_or(-1)),
    )
    .map_err(|error| format!("write exit evidence: {error}"))?;
    verify_outputs(case, surface, &workspace.project, &output)
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

fn write_pkl_config(project: &Path, tool: &str, property: &str) -> Result<PathBuf, String> {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir)
        .map_err(|error| format!("create config directory {config_dir:?}: {error}"))?;
    let body = format!(
        r#"amends "Config.pkl"
import "Builtins.pkl"

settings {{
  diagnosticsDirectory = ".velvet-glove/{tool}-agent-hook"
}}

tools {{
  ["{tool}"] = Builtins.{property}
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
