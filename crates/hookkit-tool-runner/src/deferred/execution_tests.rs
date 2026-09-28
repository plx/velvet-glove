use super::FileStatus;
use super::execution::{DeferredExecution, ScheduledWorkflow, execute_deferred_workflows};
use crate::{
    CheckScope, CommandArgTemplate, ExitCodePolicy, PhaseMode, ToolJob, ToolPhase, ToolSpec,
    UnexpectedExitPolicy, WriteBehavior,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    executable: PathBuf,
    trace: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "hookkit-deferred-{name}-{}-{sequence}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create fixture root");
        // Runner candidates are canonical paths; mirror that here.
        let root = std::fs::canonicalize(root).expect("canonical fixture root");
        let executable = root.join("fake-workflow");
        std::fs::write(
            &executable,
            r#"#!/bin/sh
case "$1" in
  -P)
    sed 's/DIRTY/CLEAN/g' "$2"
    exit $?
    ;;
  -iP)
    shift
    for file in "$@"; do
      sed 's/DIRTY/CLEAN/g' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    done
    exit $?
    ;;
esac
trace=$1
action=$2
shift 2
printf '%s\n' "$action" >> "$trace"
case "$action" in
  check)
    for file in "$@"; do
      if grep -Eq 'DIRTY|MANUAL' "$file"; then exit 1; fi
    done
    ;;
  stdout-check)
    for file in "$@"; do
      if grep -q 'DIRTY' "$file"; then printf '%s\n' "$file"; fi
    done
    ;;
  check-a)
    for file in "$@"; do
      if grep -q 'BAD_A' "$file"; then exit 1; fi
    done
    ;;
  check-b)
    for file in "$@"; do
      if grep -q 'BAD_B' "$file"; then exit 1; fi
    done
    ;;
  check-workspace)
    if grep -R -q 'BAD_A' "$1"; then exit 1; fi
    ;;
  check-report)
    status=0
    for file in "$@"; do
      if grep -Eq 'DIRTY|MANUAL' "$file"; then printf '%s:1:1: issue\n' "$file"; status=1; fi
    done
    exit $status
    ;;
  check-other)
    for file in "$@"; do
      if grep -q 'MANUAL' "$file"; then printf 'other.rs:2:1: issue\n'; exit 1; fi
    done
    ;;
  lint-fix)
    for file in "$@"; do
      sed 's/DIRTY/CLEAN/g; s/FORMATTED/UNFORMATTED/g' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    done
    ;;
  format-check)
    for file in "$@"; do
      if grep -q 'UNFORMATTED' "$file"; then exit 1; fi
    done
    ;;
  format-fix)
    for file in "$@"; do
      sed 's/UNFORMATTED/FORMATTED/g' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    done
    ;;
  fix)
    for file in "$@"; do
      sed 's/DIRTY/CLEAN/g' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    done
    ;;
  partial)
    for file in "$@"; do
      sed 's/DIRTY/MANUAL/g' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    done
    ;;
  nochange)
    ;;
  fix-b)
    for file in "$@"; do
      sed 's/BAD_B/BAD_A/g' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    done
    ;;
  failfix)
    for file in "$@"; do
      sed 's/DIRTY/CLEAN/g' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    done
    exit 2
    ;;
  crash)
    exit 2
    ;;
esac
exit 0
"#,
        )
        .expect("write fake workflow");
        let mut permissions = std::fs::metadata(&executable)
            .expect("fake metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("make fake executable");
        Self {
            trace: root.join("trace.log"),
            root,
            executable,
        }
    }

    fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, contents).expect("write candidate");
        path
    }

    fn trace_lines(&self) -> Vec<String> {
        std::fs::read_to_string(&self.trace)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn command(
    fixture: &Fixture,
    id: &str,
    action: &str,
    writes: WriteBehavior,
    workspace_arg: bool,
) -> ToolPhase {
    let mut args = vec![
        CommandArgTemplate::literal(fixture.executable.to_string_lossy()),
        CommandArgTemplate::literal(fixture.trace.to_string_lossy()),
        CommandArgTemplate::literal(action),
    ];
    args.push(if workspace_arg {
        CommandArgTemplate::Workspace
    } else {
        CommandArgTemplate::Files
    });
    ToolPhase {
        id: id.into(),
        mode: if writes == WriteBehavior::None {
            PhaseMode::Verify
        } else {
            PhaseMode::Fix
        },
        program: Some("/bin/sh".into()),
        args,
        exit_codes: ExitCodePolicy {
            clean: vec![0],
            issues: vec![1],
            failure: vec![2],
            unexpected: UnexpectedExitPolicy::Failure,
        },
        issues_on_stdout: false,
        writes,
        extra_args: Vec::new(),
        enabled: true,
    }
}

fn scheduled(
    fixture: &Fixture,
    tool_index: usize,
    file: PathBuf,
    check_action: &str,
    remedy_action: Option<&str>,
) -> ScheduledWorkflow {
    scheduled_with_scope(
        fixture,
        tool_index,
        vec![file],
        check_action,
        remedy_action,
        CheckScope::TargetFiles,
        false,
    )
}

fn scheduled_with_scope(
    fixture: &Fixture,
    tool_index: usize,
    files: Vec<PathBuf>,
    check_action: &str,
    remedy_action: Option<&str>,
    check_scope: CheckScope,
    workspace_arg: bool,
) -> ScheduledWorkflow {
    let spec = Arc::new(ToolSpec::new(
        format!("tool-{tool_index}"),
        format!("Tool {tool_index}"),
        fixture.executable.to_string_lossy(),
    ));
    ScheduledWorkflow {
        tool_index,
        workflow_index: 0,
        job_index: 0,
        spec,
        workflow_id: "workflow".into(),
        check: Some(command(
            fixture,
            "check",
            check_action,
            WriteBehavior::None,
            workspace_arg,
        )),
        remedy: remedy_action
            .map(|action| command(fixture, "remedy", action, WriteBehavior::TargetFiles, false)),
        check_scope,
        compatibility_translation: false,
        job: ToolJob {
            workspace_dir: fixture.root.clone(),
            workspace_indicator: None,
            files,
        },
        project_root: fixture.root.clone(),
    }
}

fn only_status(execution: &DeferredExecution, file: &PathBuf) -> Option<FileStatus> {
    execution.result.files.get(file).map(|result| result.status)
}

#[test]
fn initially_clean_skips_remedy() {
    let fixture = Fixture::new("clean");
    let file = fixture.file("clean.rs", "CLEAN\n");
    let plan = vec![scheduled(&fixture, 0, file.clone(), "check", Some("fix"))];
    let execution = execute_deferred_workflows(&plan, 1, true);
    assert_eq!(only_status(&execution, &file), Some(FileStatus::Clean));
    assert_eq!(fixture.trace_lines(), vec!["check"]);
}

#[test]
fn dirty_fixable_and_partly_fixable_use_one_remedy_then_final_check() {
    let fixture = Fixture::new("fixes");
    let fixed = fixture.file("fixed.rs", "DIRTY\n");
    let partial = fixture.file("partial.rs", "DIRTY\n");
    let plan = vec![
        scheduled(&fixture, 0, fixed.clone(), "check", Some("fix")),
        scheduled(&fixture, 1, partial.clone(), "check", Some("partial")),
    ];
    let execution = execute_deferred_workflows(&plan, 2, true);
    assert_eq!(only_status(&execution, &fixed), Some(FileStatus::AutoFixed));
    assert_eq!(
        only_status(&execution, &partial),
        Some(FileStatus::ManualFixesNeeded)
    );
    let trace = fixture.trace_lines();
    assert_eq!(trace.iter().filter(|line| *line == "fix").count(), 1);
    assert_eq!(trace.iter().filter(|line| *line == "partial").count(), 1);
}

#[test]
fn stdout_only_check_can_trigger_remedy_and_final_verification() {
    let fixture = Fixture::new("stdout-check");
    let file = fixture.file("dirty.go", "DIRTY\n");
    let mut workflow = scheduled(&fixture, 0, file.clone(), "stdout-check", Some("fix"));
    workflow.check.as_mut().expect("check").issues_on_stdout = true;

    let execution = execute_deferred_workflows(&[workflow], 1, true);

    assert_eq!(only_status(&execution, &file), Some(FileStatus::AutoFixed));
    assert_eq!(
        fixture.trace_lines(),
        vec!["stdout-check", "fix", "stdout-check"]
    );
}

#[test]
fn shell_comparator_uses_configured_tool_and_preserves_source_during_check() {
    let fixture = Fixture::new("shell-comparator");
    let file = fixture.file("dirty.yaml", "value: DIRTY\n");
    let spec = Arc::new(ToolSpec::new(
        "yq",
        "yq",
        fixture.executable.to_string_lossy(),
    ));
    let check = ToolPhase {
        id: "format.check".into(),
        mode: PhaseMode::Verify,
        program: Some("sh".into()),
        args: vec![
            CommandArgTemplate::literal("-c"),
            CommandArgTemplate::literal(
                "tool=$1; file=$2; shift 3; tmp=$(mktemp \"${TMPDIR:-/tmp}/hookkit-yq-test.XXXXXX\") || exit 2; trap 'rm -f \"$tmp\"' 0 HUP INT TERM; \"$tool\" -P \"$@\" \"$file\" >\"$tmp\" || exit 2; diff -u \"$file\" \"$tmp\"",
            ),
            CommandArgTemplate::literal("yq-check"),
            CommandArgTemplate::ToolExecutable,
            CommandArgTemplate::Files,
            CommandArgTemplate::literal("--"),
            CommandArgTemplate::ExtraArgs,
        ],
        exit_codes: ExitCodePolicy {
            clean: vec![0],
            issues: vec![1],
            failure: vec![2],
            unexpected: UnexpectedExitPolicy::Failure,
        },
        issues_on_stdout: false,
        writes: WriteBehavior::None,
        extra_args: Vec::new(),
        enabled: true,
    };
    let remedy = ToolPhase {
        id: "format.remedy".into(),
        mode: PhaseMode::Fix,
        program: None,
        args: vec![
            CommandArgTemplate::literal("-iP"),
            CommandArgTemplate::ExtraArgs,
            CommandArgTemplate::Files,
        ],
        exit_codes: ExitCodePolicy::default(),
        issues_on_stdout: false,
        writes: WriteBehavior::TargetFiles,
        extra_args: Vec::new(),
        enabled: true,
    };
    let plan = [ScheduledWorkflow {
        tool_index: 0,
        workflow_index: 0,
        job_index: 0,
        spec,
        workflow_id: "format".into(),
        check: Some(check),
        remedy: Some(remedy),
        check_scope: CheckScope::TargetFiles,
        compatibility_translation: false,
        job: ToolJob {
            workspace_dir: fixture.root.clone(),
            workspace_indicator: None,
            files: vec![file.clone()],
        },
        project_root: fixture.root.clone(),
    }];

    let execution = execute_deferred_workflows(&plan, 1, true);

    assert_eq!(only_status(&execution, &file), Some(FileStatus::AutoFixed));
    assert_eq!(
        std::fs::read_to_string(file).expect("fixed yaml"),
        "value: CLEAN\n"
    );
}

#[test]
fn dirty_without_remedy_and_noop_remedy_are_manual() {
    let fixture = Fixture::new("manual");
    let no_remedy = fixture.file("no-remedy.rs", "DIRTY\n");
    let no_change = fixture.file("no-change.rs", "DIRTY\n");
    let plan = vec![
        scheduled(&fixture, 0, no_remedy.clone(), "check", None),
        scheduled(&fixture, 1, no_change.clone(), "check", Some("nochange")),
    ];
    let execution = execute_deferred_workflows(&plan, 1, true);
    assert_eq!(
        only_status(&execution, &no_remedy),
        Some(FileStatus::ManualFixesNeeded)
    );
    assert_eq!(
        only_status(&execution, &no_change),
        Some(FileStatus::ManualFixesNeeded)
    );
    let noop_report = execution
        .result
        .reports
        .values()
        .find(|report| report.tool_id == "tool-1")
        .expect("noop report");
    assert!(noop_report.fix_attempted);
    assert!(noop_report.changed_files.is_empty());
}

#[test]
fn operational_initial_check_never_runs_remedy() {
    let fixture = Fixture::new("initial-failure");
    let file = fixture.file("file.rs", "DIRTY\n");
    let plan = vec![scheduled(&fixture, 0, file, "crash", Some("fix"))];
    let execution = execute_deferred_workflows(&plan, 1, true);
    assert!(execution.result.files.is_empty());
    assert!(execution.result.has_operational_problems());
    assert_eq!(fixture.trace_lines(), vec!["crash"]);
}

#[test]
fn fail_fast_stops_only_the_failing_tools_later_remedies() {
    let fixture = Fixture::new("initial-failure-stops-remedies");
    let earlier = fixture.file("earlier.rs", "DIRTY\n");
    let failed = fixture.file("failed.rs", "DIRTY\n");
    let sibling = fixture.file("sibling.rs", "DIRTY\n");
    let later = fixture.file("later.rs", "DIRTY\n");
    let mut same_tool = scheduled(&fixture, 1, sibling.clone(), "check", Some("fix"));
    same_tool.job_index = 1;
    let plan = vec![
        scheduled(&fixture, 0, earlier.clone(), "check", Some("fix")),
        scheduled(&fixture, 1, failed, "crash", Some("fix")),
        same_tool,
        scheduled(&fixture, 2, later.clone(), "check", Some("fix")),
    ];

    let execution = execute_deferred_workflows(&plan, 1, true);

    assert_eq!(
        only_status(&execution, &earlier),
        Some(FileStatus::AutoFixed)
    );
    assert_eq!(only_status(&execution, &later), Some(FileStatus::AutoFixed));
    assert_eq!(only_status(&execution, &sibling), None);
    assert_eq!(
        std::fs::read_to_string(&sibling).expect("read skipped sibling"),
        "DIRTY\n"
    );
    let problems = &execution.result.operational_problems;
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems
            .values()
            .all(|problem| problem.tool_id.as_deref() == Some("tool-1"))
    );
    assert_eq!(
        fixture.trace_lines(),
        vec![
            "check", "crash", "check", "check", "fix", "fix", "check", "check"
        ]
    );
}

#[test]
fn missing_executable_is_a_missing_tool_problem_with_install_hint() {
    let fixture = Fixture::new("missing-tool");
    let file = fixture.file("file.rs", "DIRTY\n");
    let mut workflow = scheduled(&fixture, 0, file, "check", Some("fix"));
    let mut spec = (*workflow.spec).clone();
    spec.install_hint = Some("install the tool".into());
    workflow.spec = Arc::new(spec);
    workflow.check.as_mut().expect("check").program = Some(
        fixture
            .root
            .join("missing-program")
            .to_string_lossy()
            .into_owned(),
    );

    let execution = execute_deferred_workflows(&[workflow], 1, true);

    let problem = execution
        .result
        .operational_problems
        .values()
        .next()
        .expect("problem");
    assert!(problem.missing_tool);
    assert!(problem.message.ends_with("missing-program not found"));
    assert_eq!(problem.install_hint.as_deref(), Some("install the tool"));
    assert_eq!(problem.tool_name.as_deref(), Some("Tool 0"));
}

#[test]
fn remedy_that_dirties_a_clean_later_check_reruns_it_before_deciding() {
    let fixture = Fixture::new("cross-workflow-recheck");
    let file = fixture.file("file.py", "DIRTY FORMATTED\n");
    let mut format = scheduled(
        &fixture,
        0,
        file.clone(),
        "format-check",
        Some("format-fix"),
    );
    format.workflow_index = 1;
    format.workflow_id = "format".into();
    format.check.as_mut().expect("check").id = "format.check".into();
    let plan = vec![
        scheduled(&fixture, 0, file.clone(), "check", Some("lint-fix")),
        format,
    ];

    let execution = execute_deferred_workflows(&plan, 1, true);

    assert_eq!(only_status(&execution, &file), Some(FileStatus::AutoFixed));
    assert_eq!(
        std::fs::read_to_string(&file).expect("read fixed file"),
        "CLEAN FORMATTED\n"
    );
    assert_eq!(
        fixture.trace_lines(),
        vec![
            "check",
            "format-check",
            "lint-fix",
            "format-check",
            "format-fix",
            "check",
            "format-check"
        ]
    );
    assert!(
        execution
            .logs
            .iter()
            .any(|log| log.phase == super::CommandPhase::Recheck)
    );
}

#[test]
fn identical_checks_within_one_stage_run_once() {
    let fixture = Fixture::new("shared-verifier");
    let file = fixture.file("file.rs", "DIRTY\n");
    let mut second = scheduled(&fixture, 0, file.clone(), "check", Some("nochange"));
    second.workflow_index = 1;
    let plan = vec![
        scheduled(&fixture, 0, file.clone(), "check", Some("fix")),
        second,
    ];

    let execution = execute_deferred_workflows(&plan, 2, true);

    assert_eq!(only_status(&execution, &file), Some(FileStatus::AutoFixed));
    assert_eq!(
        fixture.trace_lines(),
        vec!["check", "fix", "nochange", "check"]
    );
    assert_eq!(
        execution
            .logs
            .iter()
            .filter(|log| log.phase == super::CommandPhase::FinalCheck)
            .count(),
        2,
        "each workflow keeps its own final-check log"
    );
}

#[test]
fn batch_remedy_marks_only_changed_files_auto_fixed() {
    let fixture = Fixture::new("batch-auto-fixed");
    let dirty = fixture.file("dirty.rs", "DIRTY\n");
    let clean = fixture.file("clean.rs", "CLEAN\n");
    let plan = vec![scheduled_with_scope(
        &fixture,
        0,
        vec![clean.clone(), dirty.clone()],
        "check",
        Some("fix"),
        CheckScope::TargetFiles,
        false,
    )];

    let execution = execute_deferred_workflows(&plan, 1, true);

    assert_eq!(only_status(&execution, &dirty), Some(FileStatus::AutoFixed));
    assert_eq!(only_status(&execution, &clean), Some(FileStatus::Clean));
}

#[test]
fn batch_issues_are_attributed_to_the_files_the_output_names() {
    let fixture = Fixture::new("batch-attribution");
    let manual = fixture.file("manual.rs", "MANUAL\n");
    let clean = fixture.file("clean.rs", "CLEAN\n");
    let silent = fixture.file("silent.rs", "MANUAL\n");
    let named = scheduled_with_scope(
        &fixture,
        0,
        vec![clean.clone(), manual.clone()],
        "check-report",
        None,
        CheckScope::TargetFiles,
        false,
    );
    let unnamed = scheduled_with_scope(
        &fixture,
        1,
        vec![clean.clone(), silent.clone()],
        "check",
        None,
        CheckScope::TargetFiles,
        false,
    );

    let execution = execute_deferred_workflows(&[named, unnamed], 1, true);

    assert_eq!(
        only_status(&execution, &manual),
        Some(FileStatus::ManualFixesNeeded)
    );
    assert_eq!(
        only_status(&execution, &silent),
        Some(FileStatus::ManualFixesNeeded)
    );
    let named_report = &execution.result.reports["000-tool-0-000-000"];
    assert_eq!(named_report.issue_files, vec![manual]);
    assert!(!named_report.conservative_attribution);
    let unnamed_report = &execution.result.reports["001-tool-1-000-000"];
    assert!(unnamed_report.conservative_attribution);
    assert_eq!(
        only_status(&execution, &clean),
        Some(FileStatus::ManualFixesNeeded),
        "a check that names no file is attributed to every candidate"
    );
}

#[test]
fn issues_naming_only_other_files_are_out_of_scope() {
    let fixture = Fixture::new("out-of-scope");
    let other = fixture.file("other.rs", "untouched\n");
    let candidate = fixture.file("candidate.rs", "MANUAL\n");
    let plan = vec![scheduled(
        &fixture,
        0,
        candidate.clone(),
        "check-other",
        None,
    )];

    let execution = execute_deferred_workflows(&plan, 1, true);

    assert_eq!(only_status(&execution, &candidate), Some(FileStatus::Clean));
    assert!(!execution.result.has_manual_fixes());
    let report = execution.result.reports.values().next().expect("report");
    assert_eq!(report.out_of_scope_files, vec![other]);
    assert_eq!(execution.result.out_of_scope_reports().count(), 1);
}

#[test]
fn operational_initial_check_allows_later_remedies_without_fail_fast() {
    let fixture = Fixture::new("initial-failure-continues-remedies");
    let earlier = fixture.file("earlier.rs", "DIRTY\n");
    let failed = fixture.file("failed.rs", "DIRTY\n");
    let later = fixture.file("later.rs", "DIRTY\n");
    let plan = vec![
        scheduled(&fixture, 0, earlier.clone(), "check", Some("fix")),
        scheduled(&fixture, 1, failed, "crash", Some("fix")),
        scheduled(&fixture, 2, later.clone(), "check", Some("fix")),
    ];

    let execution = execute_deferred_workflows(&plan, 1, false);

    assert!(execution.result.has_operational_problems());
    assert_eq!(
        only_status(&execution, &earlier),
        Some(FileStatus::AutoFixed)
    );
    assert_eq!(only_status(&execution, &later), Some(FileStatus::AutoFixed));
    assert_eq!(
        std::fs::read_to_string(earlier).expect("read earlier fixed candidate"),
        "CLEAN\n"
    );
    assert_eq!(
        std::fs::read_to_string(later).expect("read fixed candidate"),
        "CLEAN\n"
    );
    assert_eq!(
        fixture.trace_lines(),
        vec!["check", "crash", "check", "fix", "fix", "check", "check"]
    );
}

#[test]
fn failed_remedy_keeps_changed_files_and_operational_problem() {
    let fixture = Fixture::new("remedy-failure");
    let file = fixture.file("file.rs", "DIRTY\n");
    let plan = vec![scheduled(
        &fixture,
        0,
        file.clone(),
        "check",
        Some("failfix"),
    )];
    let execution = execute_deferred_workflows(&plan, 1, true);
    assert!(execution.result.has_operational_problems());
    let report = execution.result.reports.values().next().expect("report");
    assert_eq!(report.changed_files, vec![file]);
    assert!(report.fix_attempted);
}

#[test]
fn formatter_only_workflow_reports_unverified_auto_fixes_and_never_blocks() {
    let fixture = Fixture::new("formatter-only");
    let dirty = fixture.file("dirty.rs", "DIRTY\n");
    let clean = fixture.file("clean.rs", "CLEAN\n");
    let mut scheduled = scheduled_with_scope(
        &fixture,
        0,
        vec![dirty.clone(), clean.clone()],
        "check",
        Some("fix"),
        CheckScope::TargetFiles,
        false,
    );
    scheduled.check = None;
    scheduled.compatibility_translation = true;
    let execution = execute_deferred_workflows(&[scheduled], 1, true);
    assert!(!execution.result.has_operational_problems());
    assert!(!execution.result.has_manual_fixes());
    assert_eq!(only_status(&execution, &dirty), Some(FileStatus::AutoFixed));
    assert_eq!(only_status(&execution, &clean), Some(FileStatus::Clean));
    let report = execution.result.reports.values().next().expect("report");
    assert!(report.fix_attempted && report.unverified);
    assert_eq!(report.changed_files, vec![dirty]);
    assert!(report.final_check.is_none());
    assert_eq!(fixture.trace_lines(), vec!["fix"], "no check exists to run");
}

#[test]
fn failing_formatter_only_remedy_is_still_operational() {
    let fixture = Fixture::new("formatter-only-failure");
    let file = fixture.file("file.rs", "DIRTY\n");
    let mut scheduled = scheduled(&fixture, 0, file.clone(), "check", Some("crash"));
    scheduled.check = None;
    scheduled.compatibility_translation = true;
    let execution = execute_deferred_workflows(&[scheduled], 1, true);
    assert!(execution.result.has_operational_problems());
    assert!(execution.result.files.is_empty());
}

#[test]
fn later_write_invalidates_prior_check_but_unrelated_write_does_not() {
    let fixture = Fixture::new("invalidation");
    let shared = fixture.file("shared.rs", "BAD_B\n");
    let unrelated = fixture.file("unrelated.rs", "BAD_B\n");
    let plan = vec![
        scheduled(&fixture, 0, shared.clone(), "check-a", None),
        scheduled(&fixture, 1, shared.clone(), "check-b", Some("fix-b")),
        scheduled(&fixture, 2, unrelated, "check-b", Some("fix-b")),
    ];
    let execution = execute_deferred_workflows(&plan, 3, true);
    assert_eq!(
        only_status(&execution, &shared),
        Some(FileStatus::ManualFixesNeeded)
    );
    assert_eq!(
        fixture
            .trace_lines()
            .iter()
            .filter(|line| *line == "check-a")
            .count(),
        2,
        "shared later write must rerun the earlier clean check"
    );

    let fixture = Fixture::new("unrelated-invalidation");
    let clean = fixture.file("clean.rs", "CLEAN\n");
    let dirty = fixture.file("dirty.rs", "BAD_B\n");
    let plan = vec![
        scheduled(&fixture, 0, clean, "check-a", None),
        scheduled(&fixture, 1, dirty, "check-b", Some("fix-b")),
    ];
    let _ = execute_deferred_workflows(&plan, 2, true);
    assert_eq!(
        fixture
            .trace_lines()
            .iter()
            .filter(|line| *line == "check-a")
            .count(),
        1
    );
}

#[test]
fn workspace_check_is_conservatively_invalidated() {
    let fixture = Fixture::new("workspace");
    let workspace_candidate = fixture.file("workspace.rs", "CLEAN\n");
    let dirty = fixture.file("dirty.rs", "BAD_B\n");
    let plan = vec![
        scheduled_with_scope(
            &fixture,
            0,
            vec![workspace_candidate.clone()],
            "check-workspace",
            None,
            CheckScope::Workspace,
            true,
        ),
        {
            let mut workflow = scheduled(&fixture, 1, dirty, "check-b", Some("fix-b"));
            workflow.remedy.as_mut().expect("remedy").writes = WriteBehavior::Workspace;
            workflow
        },
    ];
    let execution = execute_deferred_workflows(&plan, 2, true);
    assert_eq!(
        only_status(&execution, &workspace_candidate),
        Some(FileStatus::ManualFixesNeeded)
    );
    assert_eq!(
        fixture
            .trace_lines()
            .iter()
            .filter(|line| *line == "check-workspace")
            .count(),
        2
    );
}

#[test]
fn parallel_and_serial_jobs_produce_the_same_ordered_result() {
    let fixture = Fixture::new("parallel");
    let files = (0..6)
        .map(|index| fixture.file(&format!("file-{index}.rs"), "DIRTY\n"))
        .collect::<Vec<_>>();
    let plan = files
        .iter()
        .enumerate()
        .map(|(index, file)| scheduled(&fixture, index, file.clone(), "check", Some("fix")))
        .collect::<Vec<_>>();
    let serial = execute_deferred_workflows(&plan, 1, true).result;
    for file in &files {
        std::fs::write(file, "DIRTY\n").expect("reset candidate");
    }
    let parallel = execute_deferred_workflows(&plan, 4, true).result;
    assert_eq!(serial, parallel);
}
