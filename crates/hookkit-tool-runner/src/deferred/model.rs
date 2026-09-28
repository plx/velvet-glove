use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Final normal source disposition for one file.
///
/// The declaration order is the aggregation severity order. Operational
/// failures deliberately live outside this relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileStatus {
    /// Every applicable workflow completed without finding issues.
    Clean,
    /// A remedy changed the file and the final check passed.
    AutoFixed,
    /// At least one applicable workflow still reports issues.
    ManualFixesNeeded,
}

impl FileStatus {
    /// Combine independent normal results using worst-wins severity.
    pub fn join(self, other: Self) -> Self {
        self.max(other)
    }
}

/// Result of one authoritative, non-mutating check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckOutcome {
    /// The check found no actionable issues.
    Clean,
    /// The check found actionable issues.
    Issues,
}

/// The command role represented by an artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandPhase {
    /// First authoritative check before any remedy.
    InitialCheck,
    /// Check rerun before the remedy decision because an earlier remedy
    /// changed files in this workflow's scope.
    Recheck,
    /// Automatic repair command.
    Remedy,
    /// Authoritative check after a remedy.
    FinalCheck,
    /// Compatibility command combining multiple semantic roles.
    Combined,
    /// Configuration validation rather than an external tool command.
    Configuration,
}

/// Semantic classification assigned to a durable command artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactClassification {
    /// Command completed without finding issues.
    Clean,
    /// Command completed and found actionable issues.
    Issues,
    /// Command ran but failed operationally.
    Failure,
    /// Command could not be spawned.
    SpawnError,
    /// Workflow configuration prevented execution.
    ConfigurationError,
    /// Result could not be classified more precisely.
    Unclassified,
}

/// A durable command report exposed to summaries and templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArtifact {
    /// Stable artifact identifier within the run.
    pub id: String,
    /// Absolute path to the durable artifact.
    pub absolute_path: PathBuf,
    /// Artifact path relative to the run directory.
    pub run_relative_path: PathBuf,
    /// Media type describing the artifact contents.
    pub media_type: String,
    /// Tool associated with the artifact, when applicable.
    pub tool_id: Option<String>,
    /// Workflow associated with the artifact, when applicable.
    pub workflow_id: Option<String>,
    /// Deterministic job associated with the artifact, when applicable.
    pub job_id: Option<String>,
    /// Tool report associated with the artifact, when applicable.
    pub report_id: Option<String>,
    /// Command role represented by the artifact.
    pub phase: CommandPhase,
    /// Semantic classification of the command result.
    pub classification: ArtifactClassification,
    /// Process exit code, when a process was started and exited normally.
    pub exit_code: Option<i32>,
    /// Executable invoked to produce the artifact.
    pub program: Option<String>,
    /// Arguments passed to the executable.
    pub arguments: Vec<String>,
    /// Working directory used for the command.
    pub working_directory: Option<PathBuf>,
    /// Files directly assigned to the command invocation.
    pub files: Vec<PathBuf>,
    /// Files considered candidates for result attribution.
    pub candidate_files: Vec<PathBuf>,
    /// Files observed to change while the command ran.
    pub changed_files: Vec<PathBuf>,
    /// Durable textual artifact contents. The artifact file is the durable
    /// copy, so run summaries do not repeat it.
    #[serde(skip)]
    pub contents: String,
    /// Raw combined standard output and error of the command, used for
    /// bounded agent excerpts. Not serialized.
    #[serde(skip)]
    pub output: String,
}

impl RunArtifact {
    /// Whether this artifact records a non-mutating check command.
    pub fn is_check(&self) -> bool {
        matches!(
            self.phase,
            CommandPhase::InitialCheck | CommandPhase::Recheck | CommandPhase::FinalCheck
        )
    }
}

/// Stable link from a file result to one tool/workflow report.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolReportRef {
    /// Identifier of the referenced tool report.
    pub report_id: String,
    /// Artifacts that support the referenced report.
    pub artifact_ids: Vec<String>,
}

impl ToolReportRef {
    /// Creates a report reference with sorted, deduplicated artifact identifiers.
    pub fn new(
        report_id: impl Into<String>,
        artifact_ids: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut artifact_ids = artifact_ids.into_iter().collect::<Vec<_>>();
        artifact_ids.sort();
        artifact_ids.dedup();
        Self {
            report_id: report_id.into(),
            artifact_ids,
        }
    }
}

/// One tool workflow applied to one deterministic job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolReport {
    /// Stable report identifier within the run.
    pub id: String,
    /// Tool that produced the report.
    pub tool_id: String,
    /// Human-readable tool name.
    pub tool_name: String,
    /// Workflow that produced the report.
    pub workflow_id: String,
    /// Deterministic job that produced the report.
    pub job_id: String,
    /// Files eligible for attribution to this report.
    pub candidate_files: Vec<PathBuf>,
    /// Files observed to change during the workflow.
    pub changed_files: Vec<PathBuf>,
    /// Outcome of the initial authoritative check, when one ran.
    pub initial_check: Option<CheckOutcome>,
    /// Whether the workflow attempted an automatic remedy.
    pub fix_attempted: bool,
    /// Outcome of the final authoritative check, when one ran.
    pub final_check: Option<CheckOutcome>,
    /// Whether issues were attributed to every candidate because the check
    /// output named no file.
    pub conservative_attribution: bool,
    /// Files to which remaining issues are attributed.
    #[serde(default)]
    pub issue_files: Vec<PathBuf>,
    /// Files outside this workflow's candidates that the final check output
    /// blamed when it named no candidate; such issues never block.
    #[serde(default)]
    pub out_of_scope_files: Vec<PathBuf>,
    /// Durable artifacts supporting this report.
    pub artifact_ids: Vec<String>,
}

impl ToolReport {
    /// Sorts and deduplicates path and artifact collections.
    pub fn normalize(&mut self) {
        sort_paths(&mut self.candidate_files);
        sort_paths(&mut self.changed_files);
        sort_paths(&mut self.issue_files);
        sort_paths(&mut self.out_of_scope_files);
        self.artifact_ids.sort();
        self.artifact_ids.dedup();
    }

    /// Returns a stable link to this report and its artifacts.
    pub fn reference(&self) -> ToolReportRef {
        ToolReportRef::new(self.id.clone(), self.artifact_ids.clone())
    }
}

/// One normal per-file result after joining all applicable workflows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileResult {
    /// Absolute or runner-normalized file path.
    pub path: PathBuf,
    /// User-facing normalized display path.
    pub display_path: String,
    /// Deferred reporting group identifier.
    pub group_id: String,
    /// Worst normal outcome across applicable workflows.
    pub status: FileStatus,
    /// Whether the runner changed this file.
    pub changed_by_runner: bool,
    /// Display names of tools whose remedies changed this file.
    #[serde(default)]
    pub fixed_by: Vec<String>,
    /// Tool reports contributing to this result.
    pub reports: Vec<ToolReportRef>,
}

/// One normal contribution to a per-file aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAssessment {
    /// File receiving this contribution.
    pub path: PathBuf,
    /// User-facing normalized display path.
    pub display_path: String,
    /// Deferred reporting group identifier.
    pub group_id: String,
    /// Normal outcome contributed by one workflow.
    pub status: FileStatus,
    /// Whether the contributing workflow changed this file.
    pub changed_by_runner: bool,
    /// Display name of the tool whose remedy changed this file.
    pub fixed_by: Option<String>,
    /// Tool report supporting this contribution, when available.
    pub report: Option<ToolReportRef>,
}

impl FileAssessment {
    /// Creates a file assessment with default display and grouping metadata.
    pub fn new(path: impl Into<PathBuf>, status: FileStatus) -> Self {
        let path = path.into();
        Self {
            display_path: display_path(&path),
            path,
            group_id: "other".into(),
            status,
            changed_by_runner: false,
            fixed_by: None,
            report: None,
        }
    }
}

/// Environment, configuration, spawn, or tool failure outside source status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationalProblem {
    /// Stable problem identifier within the run.
    pub id: String,
    /// Tool associated with the problem, when applicable.
    pub tool_id: Option<String>,
    /// Human-readable name of the associated tool, when applicable.
    #[serde(default)]
    pub tool_name: Option<String>,
    /// Whether the tool executable could not be found.
    #[serde(default)]
    pub missing_tool: bool,
    /// Installation guidance for a missing tool, when configured.
    #[serde(default)]
    pub install_hint: Option<String>,
    /// Command phase associated with the problem, when applicable.
    pub phase: Option<String>,
    /// Files potentially affected by the problem.
    pub affected_files: Vec<PathBuf>,
    /// Human-readable problem description.
    pub message: String,
    /// Durable artifacts supporting the problem.
    pub artifact_ids: Vec<String>,
}

impl OperationalProblem {
    /// Sorts and deduplicates file and artifact collections.
    pub fn normalize(&mut self) {
        sort_paths(&mut self.affected_files);
        self.artifact_ids.sort();
        self.artifact_ids.dedup();
    }
}

/// A known incompleteness in candidate discovery or scope materialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageGap {
    /// Stable gap identifier within the run.
    pub id: String,
    /// Unresolved target associated with the gap, when available.
    pub target: Option<String>,
    /// Human-readable description of incomplete coverage.
    pub message: String,
    /// Whether the gap was retained for a future deferred attempt.
    pub retained: bool,
}

/// Complete runner-owned semantic result before rendering or native lowering.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeferredRunResult {
    /// Normal per-file results keyed by path.
    pub files: BTreeMap<PathBuf, FileResult>,
    /// Tool/workflow reports keyed by report identifier.
    pub reports: BTreeMap<String, ToolReport>,
    /// Operational failures keyed by problem identifier.
    pub operational_problems: BTreeMap<String, OperationalProblem>,
    /// Candidate files to which no workflow could be applied conclusively.
    pub uncovered_files: BTreeSet<PathBuf>,
    /// Candidate files deliberately excluded as inapplicable.
    pub not_applicable_files: BTreeSet<PathBuf>,
    /// Candidate-discovery and scope-materialization gaps keyed by identifier.
    pub coverage_gaps: BTreeMap<String, CoverageGap>,
    /// Durable run artifacts keyed by artifact identifier.
    pub artifacts: BTreeMap<String, RunArtifact>,
}

impl DeferredRunResult {
    /// Joins one normal workflow contribution into the per-file result map.
    pub fn record_file(&mut self, assessment: FileAssessment) {
        self.uncovered_files.remove(&assessment.path);
        self.not_applicable_files.remove(&assessment.path);
        match self.files.get_mut(&assessment.path) {
            Some(existing) => {
                existing.status = existing.status.join(assessment.status);
                existing.changed_by_runner |= assessment.changed_by_runner;
                if let Some(tool) = assessment.fixed_by {
                    sorted_insert(&mut existing.fixed_by, tool);
                }
                if existing.display_path.is_empty() {
                    existing.display_path = assessment.display_path;
                }
                if existing.group_id == "other" && assessment.group_id != "other" {
                    existing.group_id = assessment.group_id;
                }
                if let Some(report) = assessment.report {
                    sorted_insert(&mut existing.reports, report);
                }
            }
            None => {
                let reports = assessment.report.into_iter().collect();
                self.files.insert(
                    assessment.path.clone(),
                    FileResult {
                        path: assessment.path,
                        display_path: assessment.display_path,
                        group_id: assessment.group_id,
                        status: assessment.status,
                        changed_by_runner: assessment.changed_by_runner,
                        fixed_by: assessment.fixed_by.into_iter().collect(),
                        reports,
                    },
                );
            }
        }
    }

    /// Attach one completed workflow report to its files. Files in
    /// `issue_files` need manual fixes; other candidates are clean. Any file a
    /// remedy actually changed is at least auto-fixed, including
    /// snapshot-discovered writes outside the original candidates.
    pub fn record_report(&mut self, mut report: ToolReport) {
        report.normalize();
        let reference = report.reference();
        let changed = report
            .changed_files
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let issues = report.issue_files.iter().cloned().collect::<BTreeSet<_>>();
        let paths = report
            .candidate_files
            .iter()
            .chain(report.changed_files.iter())
            .cloned()
            .collect::<BTreeSet<_>>();
        for path in paths {
            let changed_by_runner = changed.contains(&path);
            let mut status = if issues.contains(&path) {
                FileStatus::ManualFixesNeeded
            } else {
                FileStatus::Clean
            };
            if changed_by_runner {
                status = status.join(FileStatus::AutoFixed);
            }
            let mut assessment = FileAssessment::new(path, status);
            assessment.changed_by_runner = changed_by_runner;
            assessment.fixed_by = changed_by_runner.then(|| report.tool_name.clone());
            assessment.report = Some(reference.clone());
            self.record_file(assessment);
        }
        self.reports.insert(report.id.clone(), report);
    }

    /// Records a normalized operational problem outside normal file status.
    pub fn record_operational_problem(&mut self, mut problem: OperationalProblem) {
        problem.normalize();
        self.operational_problems
            .insert(problem.id.clone(), problem);
    }

    /// Marks a path uncovered unless it already has a conclusive disposition.
    pub fn record_uncovered(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        if !self.files.contains_key(&path) && !self.not_applicable_files.contains(&path) {
            self.uncovered_files.insert(path);
        }
    }

    /// Marks a path not applicable and removes competing normal dispositions.
    pub fn record_not_applicable(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.files.remove(&path);
        self.uncovered_files.remove(&path);
        self.not_applicable_files.insert(path);
    }

    /// Records a candidate-discovery or scope-materialization gap.
    pub fn record_coverage_gap(&mut self, gap: CoverageGap) {
        self.coverage_gaps.insert(gap.id.clone(), gap);
    }

    /// Records an artifact after normalizing its path collections.
    pub fn record_artifact(&mut self, mut artifact: RunArtifact) {
        sort_paths(&mut artifact.files);
        sort_paths(&mut artifact.candidate_files);
        sort_paths(&mut artifact.changed_files);
        self.artifacts.insert(artifact.id.clone(), artifact);
    }

    /// Returns whether any file still needs manual fixes.
    pub fn has_manual_fixes(&self) -> bool {
        self.files
            .values()
            .any(|file| file.status == FileStatus::ManualFixesNeeded)
    }

    /// Returns whether the run encountered any operational problem.
    pub fn has_operational_problems(&self) -> bool {
        !self.operational_problems.is_empty()
    }

    /// Returns the artifact of the latest check that decided `report`.
    pub fn latest_check_artifact(&self, report: &ToolReport) -> Option<&RunArtifact> {
        report
            .artifact_ids
            .iter()
            .filter_map(|id| self.artifacts.get(id))
            .filter(|artifact| artifact.is_check())
            .max_by_key(|artifact| artifact.phase)
    }

    /// Reports whose remaining issues are attributed to changed or candidate
    /// files, in deterministic report order.
    pub fn manual_reports(&self) -> impl Iterator<Item = &ToolReport> {
        self.reports.values().filter(|report| {
            report.final_check == Some(CheckOutcome::Issues) && !report.issue_files.is_empty()
        })
    }

    /// Reports whose final-check issues name only files outside the run.
    pub fn out_of_scope_reports(&self) -> impl Iterator<Item = &ToolReport> {
        self.reports.values().filter(|report| {
            report.final_check == Some(CheckOutcome::Issues)
                && report.issue_files.is_empty()
                && !report.out_of_scope_files.is_empty()
        })
    }
}

fn sorted_insert<T: Ord>(values: &mut Vec<T>, value: T) {
    match values.binary_search(&value) {
        Ok(_) => {}
        Err(index) => values.insert(index, value),
    }
}

fn sort_paths(paths: &mut Vec<PathBuf>) {
    paths.sort();
    paths.dedup();
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(id: &str, candidates: &[&str], changed: &[&str], issues: &[&str]) -> ToolReport {
        ToolReport {
            id: id.into(),
            tool_id: id.into(),
            tool_name: format!("{id}-name"),
            workflow_id: "workflow".into(),
            job_id: "job".into(),
            candidate_files: candidates.iter().map(PathBuf::from).collect(),
            changed_files: changed.iter().map(PathBuf::from).collect(),
            initial_check: Some(CheckOutcome::Issues),
            fix_attempted: !changed.is_empty(),
            final_check: Some(if issues.is_empty() {
                CheckOutcome::Clean
            } else {
                CheckOutcome::Issues
            }),
            conservative_attribution: false,
            issue_files: issues.iter().map(PathBuf::from).collect(),
            out_of_scope_files: Vec::new(),
            artifact_ids: vec![format!("{id}-artifact")],
        }
    }

    #[test]
    fn file_status_join_is_explicit_worst_wins_order() {
        let statuses = [
            FileStatus::Clean,
            FileStatus::AutoFixed,
            FileStatus::ManualFixesNeeded,
        ];
        for left in statuses {
            for right in statuses {
                assert_eq!(left.join(right), left.max(right));
                assert_eq!(left.join(right), right.join(left));
            }
        }
        assert_eq!(
            statuses
                .into_iter()
                .reduce(FileStatus::join)
                .expect("statuses"),
            FileStatus::ManualFixesNeeded
        );
    }

    #[test]
    fn aggregation_is_deterministic_and_keeps_every_report() {
        let build = |reverse: bool| {
            let mut result = DeferredRunResult::default();
            let reports = [
                report("formatter", &["src/a.rs"], &["src/a.rs"], &[]),
                report("linter", &["src/a.rs"], &[], &["src/a.rs"]),
            ];
            let order: &[usize] = if reverse { &[1, 0] } else { &[0, 1] };
            for index in order {
                result.record_report(reports[*index].clone());
            }
            result
        };
        let forward = build(false);
        let reverse = build(true);
        assert_eq!(forward, reverse);
        let file = forward.files.get(Path::new("src/a.rs")).expect("file");
        assert_eq!(file.status, FileStatus::ManualFixesNeeded);
        assert!(file.changed_by_runner);
        assert_eq!(file.fixed_by, vec!["formatter-name".to_owned()]);
        assert_eq!(file.reports.len(), 2);
    }

    #[test]
    fn issues_attach_only_to_attributed_files_and_others_stay_clean() {
        let mut result = DeferredRunResult::default();
        result.record_report(report(
            "batch",
            &["src/b.rs", "src/a.rs"],
            &[],
            &["src/a.rs"],
        ));
        assert_eq!(result.files.len(), 2);
        assert_eq!(
            result.files[Path::new("src/a.rs")].status,
            FileStatus::ManualFixesNeeded
        );
        assert_eq!(
            result.files[Path::new("src/b.rs")].status,
            FileStatus::Clean
        );
        for file in result.files.values() {
            assert_eq!(file.reports[0].report_id, "batch");
        }
        assert_eq!(result.manual_reports().count(), 1);
    }

    #[test]
    fn only_changed_files_are_auto_fixed() {
        let mut result = DeferredRunResult::default();
        result.record_report(report(
            "workspace",
            &["src/a.rs", "src/b.rs"],
            &["src/a.rs", "Cargo.lock"],
            &[],
        ));
        for (path, status) in [
            ("src/a.rs", FileStatus::AutoFixed),
            ("Cargo.lock", FileStatus::AutoFixed),
            ("src/b.rs", FileStatus::Clean),
        ] {
            let file = &result.files[Path::new(path)];
            assert_eq!(file.status, status, "{path}");
            assert_eq!(file.changed_by_runner, status == FileStatus::AutoFixed);
        }
    }

    #[test]
    fn out_of_scope_issues_are_neither_manual_nor_blocking() {
        let mut result = DeferredRunResult::default();
        let mut outside = report("outside", &["src/a.rs"], &[], &[]);
        outside.final_check = Some(CheckOutcome::Issues);
        outside.out_of_scope_files = vec!["src/untouched.rs".into()];
        result.record_report(outside);
        assert_eq!(
            result.files[Path::new("src/a.rs")].status,
            FileStatus::Clean
        );
        assert!(!result.has_manual_fixes());
        assert_eq!(result.out_of_scope_reports().count(), 1);
    }

    #[test]
    fn operational_problems_do_not_change_normal_status() {
        let mut result = DeferredRunResult::default();
        result.record_report(report("ok", &["src/a.rs"], &[], &[]));
        result.record_operational_problem(OperationalProblem {
            id: "missing".into(),
            tool_id: Some("missing-tool".into()),
            tool_name: Some("Missing".into()),
            missing_tool: true,
            install_hint: None,
            phase: Some("check".into()),
            affected_files: vec!["src/a.rs".into()],
            message: "missing executable".into(),
            artifact_ids: Vec::new(),
        });
        assert_eq!(
            result.files[Path::new("src/a.rs")].status,
            FileStatus::Clean
        );
        assert!(result.has_operational_problems());
    }

    #[test]
    fn uncovered_and_deleted_files_are_not_called_clean() {
        let mut result = DeferredRunResult::default();
        result.record_uncovered("README.unknown");
        result.record_not_applicable("deleted.rs");
        assert!(result.files.is_empty());
        assert!(result.uncovered_files.contains(Path::new("README.unknown")));
        assert!(
            result
                .not_applicable_files
                .contains(Path::new("deleted.rs"))
        );
    }
}
