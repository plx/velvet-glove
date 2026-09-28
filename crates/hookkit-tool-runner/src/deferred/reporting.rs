use super::execution::command_phase_label;
use super::{DeferredRunResult, FileResult, FileStatus};
use crate::excerpt;
use globset::{Glob, GlobSet, GlobSetBuilder};
use hookkit_pkl_config::schema as pkl;
use minijinja::Environment;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const CLEAN_USER: &str = "clean.user";
const CLEAN_AGENT: &str = "clean.agent";
const AUTO_USER: &str = "auto-fixed.user";
const AUTO_AGENT: &str = "auto-fixed.agent";
const MANUAL_USER: &str = "manual.user";
const MANUAL_AGENT: &str = "manual.agent";
const OPERATIONAL_USER: &str = "operational.user";
const OPERATIONAL_AGENT: &str = "operational.agent";
const MASTER_USER: &str = "master.user";
const MASTER_AGENT: &str = "master.agent";
/// Longest single-line reason quoted from an operational problem.
const MAX_REASON_CHARS: usize = 200;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ReportingError {
    #[error("invalid deferred reporting group `{group}` glob `{pattern}`: {message}")]
    InvalidGlob {
        group: String,
        pattern: String,
        message: String,
    },
    #[error("deferred reporting group ids must be nonempty and unique: `{0}`")]
    InvalidGroupId(String),
    #[error("deferred reporting fallback group `other` must be last")]
    OtherNotLast,
    #[error("invalid deferred reporting template `{name}`: {message}")]
    InvalidTemplate { name: String, message: String },
    #[error("could not build deferred reporting context: {0}")]
    Context(String),
    #[error("could not render deferred reporting template `{name}`: {message}")]
    Render { name: String, message: String },
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct RenderedPair {
    pub user: String,
    pub agent: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct RenderedBuckets {
    pub clean: RenderedPair,
    pub auto_fixed: RenderedPair,
    pub manual_fixes_needed: RenderedPair,
    pub operational_error: RenderedPair,
}

/// Why the current result would block turn completion, before loop guards.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct BlockReasons {
    pub manual: bool,
    pub operational: bool,
    pub coverage: bool,
}

impl BlockReasons {
    pub(crate) fn any(self) -> bool {
        self.manual || self.operational || self.coverage
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct RenderedMessages {
    pub buckets: RenderedBuckets,
    pub user: Option<String>,
    pub agent: Option<String>,
}

#[derive(Debug)]
struct CompiledGroup {
    id: String,
    display_name: String,
    matcher: Option<GlobSet>,
}

pub(crate) struct DeferredReporter {
    config: pkl::DeferredReporting,
    groups: Vec<CompiledGroup>,
    templates: Environment<'static>,
}

impl DeferredReporter {
    /// Compile every glob and template before the deferred engine can mutate
    /// files. Runtime rendering errors are still handled as operational
    /// configuration failures by the caller.
    pub(crate) fn new(config: &pkl::DeferredReporting) -> Result<Self, ReportingError> {
        let mut groups = Vec::new();
        let mut ids = BTreeSet::new();
        for (index, group) in config.groups.iter().enumerate() {
            if group.id.is_empty() || !ids.insert(group.id.clone()) {
                return Err(ReportingError::InvalidGroupId(group.id.clone()));
            }
            if group.id == "other" && index + 1 != config.groups.len() {
                return Err(ReportingError::OtherNotLast);
            }
            let matcher = if group.id == "other" {
                None
            } else {
                let mut builder = GlobSetBuilder::new();
                for pattern in &group.include {
                    let glob = Glob::new(pattern).map_err(|error| ReportingError::InvalidGlob {
                        group: group.id.clone(),
                        pattern: pattern.clone(),
                        message: error.to_string(),
                    })?;
                    builder.add(glob);
                }
                Some(
                    builder
                        .build()
                        .map_err(|error| ReportingError::InvalidGlob {
                            group: group.id.clone(),
                            pattern: group.include.join(", "),
                            message: error.to_string(),
                        })?,
                )
            };
            groups.push(CompiledGroup {
                id: group.id.clone(),
                display_name: group.display_name.clone(),
                matcher,
            });
        }
        if !ids.contains("other") {
            groups.push(CompiledGroup {
                id: "other".into(),
                display_name: "Other".into(),
                matcher: None,
            });
        }

        let mut templates = Environment::new();
        for (name, source) in [
            (CLEAN_USER, config.clean.user.clone()),
            (CLEAN_AGENT, config.clean.agent.clone()),
            (AUTO_USER, config.auto_fixed.user.clone()),
            (AUTO_AGENT, config.auto_fixed.agent.clone()),
            (MANUAL_USER, config.manual_fixes_needed.user.clone()),
            (MANUAL_AGENT, config.manual_fixes_needed.agent.clone()),
            (OPERATIONAL_USER, config.operational_error.user.clone()),
            (OPERATIONAL_AGENT, config.operational_error.agent.clone()),
            (MASTER_USER, config.master_user.clone()),
            (MASTER_AGENT, config.master_agent.clone()),
        ] {
            templates
                .add_template_owned(name, source)
                .map_err(|error| ReportingError::InvalidTemplate {
                    name: name.into(),
                    message: error.to_string(),
                })?;
        }
        Ok(Self {
            config: config.clone(),
            groups,
            templates,
        })
    }

    pub(crate) fn apply_groups(&self, result: &mut DeferredRunResult, project_root: &Path) {
        for file in result.files.values_mut() {
            let candidate = file.path.strip_prefix(project_root).unwrap_or(&file.path);
            file.display_path = candidate.to_string_lossy().replace('\\', "/");
            let group = self
                .groups
                .iter()
                .find(|group| {
                    group
                        .matcher
                        .as_ref()
                        .is_some_and(|matcher| matcher.is_match(candidate))
                })
                .or_else(|| self.groups.iter().find(|group| group.id == "other"))
                .expect("reporter always has an other group");
            file.group_id = group.id.clone();
        }
    }

    pub(crate) fn render(
        &self,
        result: &DeferredRunResult,
        run: TemplateRun<'_>,
    ) -> Result<RenderedMessages, ReportingError> {
        let mut context = self.context(result, run)?;
        let counts = context
            .get("counts")
            .and_then(Value::as_object)
            .expect("reporting context counts");
        let count = |name: &str| counts.get(name).and_then(Value::as_u64).unwrap_or(0);
        let render_empty = self.config.render_empty_buckets;
        let buckets = RenderedBuckets {
            clean: self.render_pair(
                CLEAN_USER,
                CLEAN_AGENT,
                render_empty || count("clean") > 0,
                &context,
            )?,
            auto_fixed: self.render_pair(
                AUTO_USER,
                AUTO_AGENT,
                render_empty || count("auto_fixed") > 0,
                &context,
            )?,
            manual_fixes_needed: self.render_pair(
                MANUAL_USER,
                MANUAL_AGENT,
                render_empty || count("manual_fixes_needed") > 0,
                &context,
            )?,
            operational_error: self.render_pair(
                OPERATIONAL_USER,
                OPERATIONAL_AGENT,
                render_empty || count("operational_errors") > 0,
                &context,
            )?,
        };
        let rendered_buckets = serde_json::to_value(&buckets)
            .map_err(|error| ReportingError::Context(error.to_string()))?;
        let user_list = [
            &buckets.clean.user,
            &buckets.auto_fixed.user,
            &buckets.manual_fixes_needed.user,
            &buckets.operational_error.user,
        ]
        .into_iter()
        .filter(|message| !message.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
        let agent_list = [
            &buckets.clean.agent,
            &buckets.auto_fixed.agent,
            &buckets.manual_fixes_needed.agent,
            &buckets.operational_error.agent,
        ]
        .into_iter()
        .filter(|message| !message.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
        let object = context
            .as_object_mut()
            .expect("reporting context is an object");
        object.insert("rendered_buckets".into(), rendered_buckets);
        object.insert(
            "rendered_bucket_lists".into(),
            json!({"user": user_list, "agent": agent_list}),
        );
        let user = nonempty(self.render_template(MASTER_USER, &context)?);
        let agent = nonempty(self.render_template(MASTER_AGENT, &context)?);
        Ok(RenderedMessages {
            buckets,
            user,
            agent,
        })
    }

    fn render_pair(
        &self,
        user_name: &str,
        agent_name: &str,
        render: bool,
        context: &Value,
    ) -> Result<RenderedPair, ReportingError> {
        if !render {
            return Ok(RenderedPair::default());
        }
        Ok(RenderedPair {
            user: self.render_template(user_name, context)?,
            agent: self.render_template(agent_name, context)?,
        })
    }

    fn render_template(&self, name: &str, context: &Value) -> Result<String, ReportingError> {
        self.templates
            .get_template(name)
            .and_then(|template| template.render(context))
            .map_err(|error| ReportingError::Render {
                name: name.into(),
                message: error.to_string(),
            })
    }

    fn context(
        &self,
        result: &DeferredRunResult,
        run: TemplateRun<'_>,
    ) -> Result<Value, ReportingError> {
        let clean_files = files_with_status(result, FileStatus::Clean);
        let auto_fixed_files = files_with_status(result, FileStatus::AutoFixed);
        let manual_fix_files = files_with_status(result, FileStatus::ManualFixesNeeded);
        let mut groups = Vec::new();
        let mut manual_groups = 0usize;
        for configured in &self.groups {
            let files = result
                .files
                .values()
                .filter(|file| file.group_id == configured.id)
                .collect::<Vec<_>>();
            if files.is_empty() {
                continue;
            }
            let manual = files
                .iter()
                .copied()
                .filter(|file| file.status == FileStatus::ManualFixesNeeded)
                .collect::<Vec<_>>();
            if !manual.is_empty() {
                manual_groups += 1;
            }
            let artifact_paths = associated_artifact_paths(result, &manual);
            groups.push(json!({
                "id": configured.id,
                "display_name": configured.display_name,
                "files": files,
                "count": files.len(),
                "manual_fix_files": manual,
                "artifact_paths": artifact_paths,
            }));
        }
        let artifact_paths = result
            .artifacts
            .values()
            .map(|artifact| artifact.absolute_path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let artifact_contents = result
            .artifacts
            .values()
            .map(|artifact| {
                (
                    artifact.absolute_path.to_string_lossy().into_owned(),
                    artifact.contents.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let roots = run
            .display_roots
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>();
        let issues = self.issue_entries(result, run.project_root, &roots);
        let problems = problem_entries(result);
        let out_of_scope = result
            .out_of_scope_reports()
            .map(|report| {
                json!({
                    "tool": report.tool_name,
                    "tool_id": report.tool_id,
                    "files": report
                        .out_of_scope_files
                        .iter()
                        .map(|path| display(path, run.project_root))
                        .collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_value(json!({
            "run": {
                "id": run.id,
                "project_root": run.project_root,
                "summary_path": run.summary_path,
                "state_directory": run.state_directory,
                "directory": run.directory,
            },
            "blocks": run.blocks,
            "issues": issues,
            "problems": problems,
            "out_of_scope": out_of_scope,
            "counts": {
                "clean": clean_files.len(),
                "auto_fixed": auto_fixed_files.len(),
                "manual_fixes_needed": manual_fix_files.len(),
                "manual_groups": manual_groups,
                "operational_errors": result.operational_problems.len(),
                "uncovered": result.uncovered_files.len(),
                "not_applicable": result.not_applicable_files.len(),
                "coverage_gaps": result.coverage_gaps.len(),
                "groups": groups.len(),
                "out_of_scope": out_of_scope.len(),
            },
            "files": result.files.values().collect::<Vec<_>>(),
            "buckets": {
                "clean": { "count": clean_files.len(), "files": clean_files },
                "auto_fixed": { "count": auto_fixed_files.len(), "files": auto_fixed_files },
                "manual_fixes_needed": { "count": manual_fix_files.len(), "files": manual_fix_files },
                "operational_error": {
                    "count": result.operational_problems.len(),
                    "problems": result.operational_problems.values().collect::<Vec<_>>(),
                },
            },
            "clean_files": clean_files,
            "auto_fixed_files": auto_fixed_files,
            "manual_fix_files": manual_fix_files,
            "uncovered_files": result.uncovered_files,
            "not_applicable_files": result.not_applicable_files,
            "groups": groups,
            "reports": result.reports,
            "artifacts": result.artifacts.values().collect::<Vec<_>>(),
            "artifact_paths": artifact_paths,
            "artifact_contents": artifact_contents,
            "operational_problems": result.operational_problems,
            "coverage_gaps": result.coverage_gaps,
        }))
        .map_err(|error| ReportingError::Context(error.to_string()))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TemplateRun<'a> {
    pub id: &'a str,
    pub project_root: &'a Path,
    pub summary_path: &'a Path,
    pub state_directory: &'a Path,
    /// Run bundle directory holding every command log.
    pub directory: &'a Path,
    /// Absolute prefixes rewritten to project-relative paths in excerpts.
    pub display_roots: &'a [PathBuf],
    pub blocks: BlockReasons,
}

impl DeferredReporter {
    /// One entry per distinct manual report, each with a bounded excerpt of
    /// the output of the check that decided it. The configured line and
    /// character limits apply to all excerpts together.
    pub(crate) fn issue_entries(
        &self,
        result: &DeferredRunResult,
        project_root: &Path,
        roots: &[&Path],
    ) -> Vec<IssueExcerpt> {
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        for report in result.manual_reports() {
            let artifact = result.latest_check_artifact(report);
            let output = artifact
                .map(|artifact| excerpt::normalize(&artifact.output, roots))
                .unwrap_or_default();
            let files = report
                .issue_files
                .iter()
                .map(|path| display(path, project_root))
                .collect::<Vec<_>>();
            if !seen.insert((report.tool_id.clone(), files.clone(), output.clone())) {
                continue;
            }
            let log_path =
                artifact.map(|artifact| artifact.absolute_path.to_string_lossy().into_owned());
            entries.push((report, files, output, log_path));
        }
        let outputs = entries
            .iter()
            .map(|(_, _, output, _)| output.clone())
            .collect::<Vec<_>>();
        let clipped = excerpt::clip_shared(
            &outputs,
            self.config.excerpt_max_lines as usize,
            self.config.excerpt_max_chars as usize,
        );
        entries
            .into_iter()
            .zip(clipped)
            .map(|((report, files, _, log_path), clipped)| IssueExcerpt {
                tool: report.tool_name.clone(),
                tool_id: report.tool_id.clone(),
                workflow: report.workflow_id.clone(),
                files,
                excerpt: excerpt::with_log_note(&clipped, log_path.as_deref()),
                truncated: clipped.truncated,
                log_path,
            })
            .collect()
    }
}

/// One manual report's files and a bounded excerpt of its deciding check
/// output, as the deferred templates (`issues`) and `check` see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssueExcerpt {
    /// Display name of the reporting tool.
    pub tool: String,
    /// Identifier of the reporting tool.
    pub tool_id: String,
    /// Workflow whose final check reported the issues.
    pub workflow: String,
    /// Project-relative files the issues are attributed to.
    pub files: Vec<String>,
    /// ANSI-free, project-relative, bounded check output; ends with a
    /// pointer to the full log when cut.
    pub excerpt: String,
    /// Whether the excerpt was cut to fit the budget.
    pub truncated: bool,
    /// Absolute path of the deciding check's full log.
    pub log_path: Option<String>,
}

/// One tool's operational problems, as the deferred templates (`problems`)
/// and `check` see them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProblemSummary {
    /// Display name of the tool, or `configuration`.
    pub tool: String,
    /// Identifier of the tool, when the problem belongs to one.
    pub tool_id: Option<String>,
    /// First line of the first problem's message, bounded.
    pub reason: String,
    /// Whether the executable could not be found.
    pub missing_tool: bool,
    /// Installation guidance for a missing tool.
    pub install_hint: Option<String>,
    /// Absolute path of a log supporting the problem.
    pub log_path: Option<String>,
    /// Number of problems recorded for this tool.
    pub count: usize,
}

/// One entry per tool with operational problems, in first-seen order.
pub(crate) fn problem_entries(result: &DeferredRunResult) -> Vec<ProblemSummary> {
    let mut entries = Vec::<ProblemSummary>::new();
    for problem in result.operational_problems.values() {
        if let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.tool_id == problem.tool_id)
        {
            entry.count += 1;
            continue;
        }
        // Point at the log of the command that failed (a failed remedy's,
        // not the final check that ran after it) when it is known.
        let artifacts = problem
            .artifact_ids
            .iter()
            .filter_map(|id| result.artifacts.get(id))
            .collect::<Vec<_>>();
        let log_path = artifacts
            .iter()
            .find(|artifact| problem.phase.as_deref() == Some(command_phase_label(artifact.phase)))
            .or(artifacts.first())
            .map(|artifact| artifact.absolute_path.to_string_lossy().into_owned());
        let reason = problem
            .message
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .trim();
        entries.push(ProblemSummary {
            tool: problem
                .tool_name
                .clone()
                .or_else(|| problem.tool_id.clone())
                .unwrap_or_else(|| "configuration".into()),
            tool_id: problem.tool_id.clone(),
            reason: excerpt::clip(reason, 1, MAX_REASON_CHARS).text,
            missing_tool: problem.missing_tool,
            install_hint: problem.install_hint.clone(),
            log_path,
            count: 1,
        });
    }
    entries
}

fn display(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn files_with_status(result: &DeferredRunResult, status: FileStatus) -> Vec<&FileResult> {
    result
        .files
        .values()
        .filter(|file| file.status == status)
        .collect()
}

fn associated_artifact_paths(result: &DeferredRunResult, files: &[&FileResult]) -> Vec<PathBuf> {
    files
        .iter()
        .flat_map(|file| file.reports.iter())
        .flat_map(|report| report.artifact_ids.iter())
        .filter_map(|id| result.artifacts.get(id))
        .map(|artifact| artifact.absolute_path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Trim surrounding whitespace so optional template lines can be emitted
/// with leading separators, and treat blank output as no message.
fn nonempty(message: String) -> Option<String> {
    let trimmed = message.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ArtifactClassification, CheckOutcome, CommandPhase, FileAssessment, OperationalProblem,
        RunArtifact, ToolReport,
    };

    fn artifact(id: &str, report_id: &str, path: &str, output: &str) -> RunArtifact {
        RunArtifact {
            id: id.into(),
            absolute_path: PathBuf::from(path),
            run_relative_path: PathBuf::from(format!("tools/{id}.log")),
            media_type: "text/plain".into(),
            tool_id: Some("tool".into()),
            workflow_id: Some("lint".into()),
            job_id: Some("000".into()),
            report_id: Some(report_id.into()),
            phase: CommandPhase::FinalCheck,
            classification: ArtifactClassification::Issues,
            exit_code: Some(1),
            program: Some("tool".into()),
            arguments: vec!["check".into()],
            working_directory: Some(PathBuf::from("/repo")),
            files: vec![PathBuf::from("/repo/example.cpp")],
            candidate_files: vec![PathBuf::from("/repo/example.cpp")],
            changed_files: Vec::new(),
            contents: format!("header\n{output}"),
            output: output.into(),
        }
    }

    fn manual_report(id: &str, tool: &str, files: &[&str]) -> ToolReport {
        ToolReport {
            id: id.into(),
            tool_id: tool.to_lowercase(),
            tool_name: tool.into(),
            workflow_id: "lint".into(),
            job_id: "000".into(),
            candidate_files: files.iter().map(PathBuf::from).collect(),
            changed_files: Vec::new(),
            initial_check: Some(CheckOutcome::Issues),
            fix_attempted: false,
            final_check: Some(CheckOutcome::Issues),
            conservative_attribution: false,
            issue_files: files.iter().map(PathBuf::from).collect(),
            out_of_scope_files: Vec::new(),
            unverified: false,
            artifact_ids: vec![format!("{id}-final-check")],
        }
    }

    const ROOTS: &[PathBuf] = &[];

    fn run<'a>() -> TemplateRun<'a> {
        TemplateRun {
            id: "run",
            project_root: Path::new("/repo"),
            summary_path: Path::new("/state/run/summary.json"),
            state_directory: Path::new("/state"),
            directory: Path::new("/state/run"),
            display_roots: ROOTS,
            blocks: BlockReasons::default(),
        }
    }

    fn render(result: &mut DeferredRunResult, config: &pkl::DeferredReporting) -> RenderedMessages {
        let reporter = DeferredReporter::new(config).unwrap();
        reporter.apply_groups(result, Path::new("/repo"));
        let roots = [PathBuf::from("/repo")];
        let mut run = run();
        run.display_roots = &roots;
        reporter.render(result, run).unwrap()
    }

    #[test]
    fn defaults_group_c_and_cpp_together_and_fallback_to_other() {
        let reporter = DeferredReporter::new(&pkl::DeferredReporting::default()).unwrap();
        let mut result = DeferredRunResult::default();
        for path in ["/repo/a.h", "/repo/b.cpp", "/repo/data.unknown"] {
            result.record_file(FileAssessment::new(path, FileStatus::Clean));
        }
        reporter.apply_groups(&mut result, Path::new("/repo"));
        assert_eq!(result.files[Path::new("/repo/a.h")].group_id, "c-cpp");
        assert_eq!(result.files[Path::new("/repo/b.cpp")].group_id, "c-cpp");
        assert_eq!(
            result.files[Path::new("/repo/data.unknown")].group_id,
            "other"
        );
    }

    #[test]
    fn first_matching_custom_group_wins() {
        let config = pkl::DeferredReporting {
            groups: vec![
                pkl::FileGroup {
                    id: "first".into(),
                    display_name: "First".into(),
                    include: vec!["**/*.rs".into()],
                },
                pkl::FileGroup {
                    id: "second".into(),
                    display_name: "Second".into(),
                    include: vec!["src/**".into()],
                },
            ],
            ..Default::default()
        };
        let reporter = DeferredReporter::new(&config).unwrap();
        let mut result = DeferredRunResult::default();
        result.record_file(FileAssessment::new("/repo/src/lib.rs", FileStatus::Clean));
        reporter.apply_groups(&mut result, Path::new("/repo"));
        assert_eq!(
            result.files[Path::new("/repo/src/lib.rs")].group_id,
            "first"
        );
    }

    #[test]
    fn clean_results_are_silent_for_both_audiences() {
        let mut result = DeferredRunResult::default();
        result.record_file(FileAssessment::new("/repo/one.rs", FileStatus::Clean));
        let rendered = render(&mut result, &pkl::DeferredReporting::default());
        assert!(rendered.user.is_none());
        assert!(rendered.agent.is_none());
    }

    #[test]
    fn auto_fixed_notice_names_files_and_fixing_tools_for_both_audiences() {
        let mut result = DeferredRunResult::default();
        result.record_file(FileAssessment::new("/repo/one.rs", FileStatus::Clean));
        let mut fixed = FileAssessment::new("/repo/src/a.py", FileStatus::AutoFixed);
        fixed.fixed_by = Some("Ruff".into());
        result.record_file(fixed);
        let mut formatted = FileAssessment::new("/repo/web/b.ts", FileStatus::AutoFixed);
        formatted.fixed_by = Some("Prettier".into());
        result.record_file(formatted);
        let rendered = render(&mut result, &pkl::DeferredReporting::default());
        let expected =
            "velvet-glove auto-fixed src/a.py (Ruff), web/b.ts (Prettier); re-read before editing.";
        assert_eq!(rendered.user.as_deref(), Some(expected));
        assert_eq!(rendered.agent.as_deref(), Some(expected));
    }

    #[test]
    fn manual_agent_message_quotes_bounded_final_check_excerpts() {
        let mut result = DeferredRunResult::default();
        let long = (1..=100)
            .map(|line| format!("/repo/example.cpp:{line}:1: \u{1b}[31merror\u{1b}[0m {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        for (id, tool, output) in [
            ("report-a", "Alpha", long.as_str()),
            ("report-b", "Beta", "/repo/example.cpp:3:1: warning"),
        ] {
            result.record_report(manual_report(id, tool, &["/repo/example.cpp"]));
            result.record_artifact(artifact(
                &format!("{id}-final-check"),
                id,
                &format!("/state/run/{id}.log"),
                output,
            ));
        }
        let config = pkl::DeferredReporting {
            excerpt_max_lines: 20,
            ..Default::default()
        };
        let rendered = render(&mut result, &config);
        let agent = rendered.agent.unwrap();
        assert!(
            agent.starts_with("velvet-glove found issues to fix before stopping:"),
            "{agent}"
        );
        assert!(agent.contains("Alpha: example.cpp\nexample.cpp:1:1: error 1\n"));
        assert!(
            agent.contains(
                "example.cpp:10:1: error 10\n…truncated; full log: /state/run/report-a.log"
            )
        );
        assert!(!agent.contains("error 11\n"));
        assert!(agent.contains("Beta: example.cpp\nexample.cpp:3:1: warning"));
        assert!(!agent.contains('\u{1b}'));
        assert!(!agent.contains("/repo/"));
        assert_eq!(
            rendered.user.as_deref(),
            Some("velvet-glove: 1 file needs manual fixes (example.cpp). Details: /state/run")
        );
    }

    #[test]
    fn identical_excerpts_from_shared_checks_are_quoted_once() {
        let mut result = DeferredRunResult::default();
        for id in ["format", "fix"] {
            result.record_report(manual_report(id, "Tool", &["/repo/example.cpp"]));
            result.record_artifact(artifact(
                &format!("{id}-final-check"),
                id,
                "/state/run/shared.log",
                "same output",
            ));
        }
        let agent = render(&mut result, &pkl::DeferredReporting::default())
            .agent
            .unwrap();
        assert_eq!(agent.matches("same output").count(), 1, "{agent}");
    }

    #[test]
    fn operational_problems_are_user_only_unless_they_block() {
        let mut result = DeferredRunResult::default();
        result.record_operational_problem(OperationalProblem {
            id: "missing".into(),
            tool_id: Some("ruff".into()),
            tool_name: Some("Ruff".into()),
            missing_tool: true,
            install_hint: Some("brew install ruff".into()),
            phase: Some("initial-check".into()),
            affected_files: vec!["/repo/a.py".into()],
            message: "ruff not found".into(),
            artifact_ids: Vec::new(),
        });
        let rendered = render(&mut result, &pkl::DeferredReporting::default());
        assert_eq!(
            rendered.user.as_deref(),
            Some("velvet-glove could not run Ruff (ruff not found; brew install ruff).")
        );
        assert!(rendered.agent.is_none());

        let reporter = DeferredReporter::new(&pkl::DeferredReporting::default()).unwrap();
        let mut blocking = run();
        blocking.blocks.operational = true;
        let agent = reporter.render(&result, blocking).unwrap().agent.unwrap();
        assert!(agent.starts_with("velvet-glove could not run Ruff"));
    }

    #[test]
    fn a_failed_remedy_points_at_its_own_log() {
        let mut result = DeferredRunResult::default();
        result.record_artifact(artifact("r-final-check", "r", "/logs/final-check.log", ""));
        let mut remedy = artifact("r-remedy", "r", "/logs/remedy.log", "");
        remedy.phase = CommandPhase::Remedy;
        result.record_artifact(remedy);
        result.record_operational_problem(OperationalProblem {
            id: "r-remedy".into(),
            tool_id: Some("clippy".into()),
            tool_name: Some("Clippy".into()),
            missing_tool: false,
            install_hint: None,
            phase: Some("remedy".into()),
            affected_files: vec!["/repo/src/main.rs".into()],
            message: "fix failed with exit code 101".into(),
            artifact_ids: vec!["r-final-check".into(), "r-remedy".into()],
        });
        let entries = problem_entries(&result);
        assert_eq!(entries[0].log_path.as_deref(), Some("/logs/remedy.log"));
    }

    #[test]
    fn out_of_scope_issues_get_a_terse_user_note_only() {
        let mut result = DeferredRunResult::default();
        let mut report = manual_report("clippy", "Clippy", &["/repo/src/a.rs"]);
        report.issue_files.clear();
        report.out_of_scope_files = vec!["/repo/src/untouched.rs".into()];
        result.record_report(report);
        let rendered = render(&mut result, &pkl::DeferredReporting::default());
        assert_eq!(
            rendered.user.as_deref(),
            Some(
                "velvet-glove: not blocking on issues outside the files changed this turn: Clippy (src/untouched.rs)."
            )
        );
        assert!(rendered.agent.is_none());
    }

    #[test]
    fn master_receives_raw_and_rendered_values_and_artifact_views_are_independent() {
        let mut config = pkl::DeferredReporting::default();
        config.clean.user = "sub={{ counts.clean }}".into();
        config.master_user = "{{ rendered_buckets.clean.user }}|{{ buckets.clean.count }}|{{ artifact_paths | length }}".into();
        config.master_agent = "{{ artifact_contents['/state/a.log'] }}".into();
        let mut result = DeferredRunResult::default();
        result.record_file(FileAssessment::new("/repo/a.rs", FileStatus::Clean));
        result.record_artifact(artifact("a", "report", "/state/a.log", "artifact bytes"));
        let rendered = render(&mut result, &config);
        assert_eq!(rendered.user.as_deref(), Some("sub=1|1|1"));
        assert_eq!(rendered.agent.as_deref(), Some("header\nartifact bytes"));
    }

    #[test]
    fn empty_template_suppresses_one_audience() {
        let mut config = pkl::DeferredReporting::default();
        config.manual_fixes_needed.user.clear();
        config.master_user = "{{ rendered_bucket_lists.user | join('') }}".into();
        let mut result = DeferredRunResult::default();
        result.record_file(FileAssessment::new(
            "/repo/a.rs",
            FileStatus::ManualFixesNeeded,
        ));
        let rendered = render(&mut result, &config);
        assert!(rendered.user.is_none());
        assert!(rendered.agent.is_some());
    }
}
