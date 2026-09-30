//! The immediate runner's accumulated output and its semantic outcome.

use super::messages::{MessageArgs, TemplateTool, render_with_fallback};
use crate::excerpt;
use hookkit_common::UserNotice;
use hookkit_common::message::DiagnosticReport;
use hookkit_pkl_config::schema as pkl;
use std::path::{Path, PathBuf};

/// Accumulated common output produced by the post-tool runner.
///
/// Fields are runner-owned so callers receive this value through
/// [`RunnerDomainOutcome`] and lower it with the selected harness workflow.
#[derive(Debug, Default)]
pub struct RunnerPostToolUseOutput {
    pub(crate) notices: Vec<UserNotice>,
    pub(crate) agent_feedback: Vec<AgentFeedback>,
    pub(crate) diagnostics: Vec<DiagnosticReport>,
    pub(crate) auto_fixed: Vec<AutoFixed>,
    pub(crate) harness_block: Option<String>,
    pub(crate) lowering: pkl::LoweringPolicy,
    excerpt_limits: ExcerptLimits,
}

/// One agent-facing line: rendered, or a tool's remaining issues whose
/// excerpt is cut only once every tool has run, so all of them share the
/// excerpt budget fairly.
#[derive(Debug)]
pub(crate) enum AgentFeedback {
    Rendered(String),
    Issues(Box<PendingIssues>),
}

/// A tool's remaining issues, rendered through its `issuesAgent` or
/// `issuesChangedAgent` template once the excerpt is known.
#[derive(Debug)]
pub(crate) struct PendingIssues {
    pub(crate) template: String,
    /// Built-in template used when `template` fails to render.
    pub(crate) fallback: String,
    /// Which `messages` field `template` came from, for error notices.
    pub(crate) field: &'static str,
    pub(crate) tool: String,
    pub(crate) tool_id: String,
    pub(crate) project_root: PathBuf,
    pub(crate) changed_files: Vec<String>,
    pub(crate) issue_files: Vec<String>,
    pub(crate) diagnostics: PathBuf,
    pub(crate) output: String,
}

impl PendingIssues {
    fn render(&self, excerpt: &str, notices: &mut Vec<UserNotice>) -> String {
        let tool = TemplateTool {
            name: &self.tool,
            id: &self.tool_id,
            project_root: &self.project_root,
        };
        let args = MessageArgs {
            changed_files: &self.changed_files,
            issue_files: &self.issue_files,
            diagnostics_path: Some(&self.diagnostics),
            excerpt,
            ..MessageArgs::default()
        };
        render_with_fallback(
            &self.template,
            &self.fallback,
            self.field,
            &tool,
            &args,
            notices,
        )
    }
}

/// Agent excerpt limits shared by every tool in one immediate run, plus the
/// absolute prefixes that excerpts rewrite to project-relative paths. The
/// limits are the deferred reporter's (`deferredReporting.excerptMax*`) and
/// are divided among the tools that report issues exactly as at Stop.
#[derive(Debug, Default)]
pub(crate) struct ExcerptLimits {
    lines: usize,
    chars: usize,
    roots: Vec<PathBuf>,
}

impl ExcerptLimits {
    pub(crate) fn new(reporting: &pkl::DeferredReporting, roots: Vec<PathBuf>) -> Self {
        Self {
            lines: reporting.excerpt_max_lines as usize,
            chars: reporting.excerpt_max_chars as usize,
            roots,
        }
    }

    /// Bounded, ANSI-free, project-relative excerpts of `outputs`, one per
    /// output, sharing the limits; a cut excerpt points at its log.
    fn excerpts(&self, outputs: &[(&str, &Path)]) -> Vec<String> {
        let roots = self.roots.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        let normalized = outputs
            .iter()
            .map(|(output, _)| excerpt::normalize(output, &roots))
            .collect::<Vec<_>>();
        excerpt::clip_shared(&normalized, self.lines, self.chars)
            .iter()
            .zip(outputs)
            .map(|(clipped, (_, log))| {
                excerpt::with_log_note(clipped, Some(&log.to_string_lossy()))
            })
            .collect()
    }
}

/// Files one tool changed and left clean.
#[derive(Debug)]
pub(crate) struct AutoFixed {
    pub(crate) tool: String,
    pub(crate) files: Vec<String>,
    /// Whether the agent learns about it through the shared auto-fix line
    /// (the tool keeps the default `cleanChangedAgent` template).
    pub(crate) in_agent_line: bool,
}

/// One terse line naming the auto-fixed files (at most
/// [`pkl::AUTO_FIXED_LISTED_FILES`], as at Stop) and the tools that changed
/// each: `velvet-glove auto-fixed a.py (Ruff), b.ts (Prettier); re-read
/// before editing.`
pub(crate) fn auto_fixed_line<'a>(
    entries: impl IntoIterator<Item = &'a AutoFixed>,
) -> Option<String> {
    let mut files = Vec::<(&str, Vec<&str>)>::new();
    for entry in entries {
        for file in &entry.files {
            match files.iter_mut().find(|(known, _)| known == file) {
                Some((_, tools)) if !tools.contains(&entry.tool.as_str()) => {
                    tools.push(&entry.tool);
                }
                Some(_) => {}
                None => files.push((file, vec![&entry.tool])),
            }
        }
    }
    (!files.is_empty()).then(|| {
        let listed = files
            .iter()
            .take(pkl::AUTO_FIXED_LISTED_FILES)
            .map(|(file, tools)| format!("{file} ({})", tools.join(", ")))
            .collect::<Vec<_>>()
            .join(", ");
        let more = match files.len().saturating_sub(pkl::AUTO_FIXED_LISTED_FILES) {
            0 => String::new(),
            more => format!(" and {more} more"),
        };
        format!("velvet-glove auto-fixed {listed}{more}; re-read before editing.")
    })
}

impl RunnerPostToolUseOutput {
    pub(crate) fn new(lowering: pkl::LoweringPolicy) -> Self {
        Self {
            lowering,
            ..Self::default()
        }
    }

    pub(crate) fn with_user_notice(mut self, notice: UserNotice) -> Self {
        self.notices.push(notice);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_agent_feedback(mut self, feedback: impl Into<String>) -> Self {
        self.agent_feedback
            .push(AgentFeedback::Rendered(feedback.into()));
        self
    }

    #[cfg(test)]
    pub(crate) fn with_diagnostic_report(mut self, report: DiagnosticReport) -> Self {
        self.diagnostics.push(report);
        self
    }

    pub(crate) fn with_harness_block(mut self, message: impl Into<String>) -> Self {
        self.harness_block = Some(message.into());
        self
    }

    #[cfg(test)]
    pub(crate) fn with_auto_fixed(mut self, auto_fixed: AutoFixed) -> Self {
        self.auto_fixed.push(auto_fixed);
        self
    }

    pub(crate) fn with_excerpt_limits(mut self, limits: ExcerptLimits) -> Self {
        self.excerpt_limits = limits;
        self
    }

    /// Render every pending issue message, dividing the excerpt budget
    /// among them. Returns the agent-facing lines in order.
    pub(crate) fn rendered_agent_feedback(&mut self) -> Vec<String> {
        let feedback = std::mem::take(&mut self.agent_feedback);
        let pending = feedback
            .iter()
            .filter_map(|entry| match entry {
                AgentFeedback::Issues(issues) => {
                    Some((issues.output.as_str(), issues.diagnostics.as_path()))
                }
                AgentFeedback::Rendered(_) => None,
            })
            .collect::<Vec<_>>();
        let mut excerpts = self.excerpt_limits.excerpts(&pending).into_iter();
        feedback
            .iter()
            .map(|entry| match entry {
                AgentFeedback::Rendered(text) => text.clone(),
                AgentFeedback::Issues(issues) => {
                    let excerpt = excerpts.next().unwrap_or_default();
                    issues.render(&excerpt, &mut self.notices)
                }
            })
            .collect()
    }
}

/// Runner-owned semantic result. Tool policy and classification deliberately do
/// not leak into core/common crates.
#[derive(Debug)]
pub enum RunnerDomainOutcome {
    /// No messages, diagnostics, or block decision were produced.
    Clean,
    /// Common output should be lowered to the selected harness.
    Report(RunnerPostToolUseOutput),
    /// The configured policy requests a harness-native block decision.
    HarnessBlock {
        /// Reason presented through the harness decision mechanism.
        message: String,
        /// Additional notices, feedback, and diagnostics to lower.
        output: RunnerPostToolUseOutput,
    },
    /// Runner execution failed independently of tool-reported issues.
    OperationalFailure {
        /// Human-readable failure diagnostic.
        message: String,
    },
    /// The selected harness cannot represent or execute this workflow.
    UnsupportedHarness {
        /// Selected harness identifier.
        harness: String,
        /// Explanation of the unsupported behavior.
        reason: String,
    },
}

pub(crate) fn is_empty_output(output: &RunnerPostToolUseOutput) -> bool {
    output.notices.is_empty()
        && output.agent_feedback.is_empty()
        && output.diagnostics.is_empty()
        && output.auto_fixed.is_empty()
        && output.harness_block.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immediate::lowering::format_notice;
    use crate::paths::display_roots;

    #[test]
    fn immediate_excerpts_are_plain_project_relative_and_share_one_budget() {
        let reporting = pkl::DeferredReporting {
            excerpt_max_lines: 12,
            ..Default::default()
        };
        let limits = ExcerptLimits::new(
            &reporting,
            display_roots(Path::new("/private/repo"), Path::new("/repo")),
        );
        let log = Path::new("/tmp/vg/issues.txt");
        let long = (1..=20)
            .map(|line| format!("b.py:{line}: E{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let excerpts = limits.excerpts(&[
            (
                "\u{1b}[31m/private/repo/src/a.py:1:1\u{1b}[0m: E1\n/repo/src/a.py:2:1: E2\n",
                log,
            ),
            (&long, log),
            ("c.py:1: E5", log),
        ]);
        assert_eq!(excerpts[0], "src/a.py:1:1: E1\nsrc/a.py:2:1: E2");
        // A verbose tool gets its share, not the whole budget ...
        assert!(
            excerpts[1].ends_with("…truncated; full log: /tmp/vg/issues.txt"),
            "{}",
            excerpts[1]
        );
        assert_eq!(
            excerpts[1].lines().count(),
            5 + 1,
            "its share plus the log note"
        );
        // ... so a later tool is still quoted.
        assert_eq!(excerpts[2], "c.py:1: E5");
    }

    #[test]
    fn pending_issue_messages_fall_back_to_the_default_template() {
        let mut output = RunnerPostToolUseOutput::default()
            .with_excerpt_limits(ExcerptLimits::new(&Default::default(), Vec::new()));
        output
            .agent_feedback
            .push(AgentFeedback::Issues(Box::new(PendingIssues {
                template: "{{ tool | nosuchfilter }}".into(),
                fallback: pkl::default_issues_changed_agent(),
                field: "issuesChangedAgent",
                tool: "Fmt".into(),
                tool_id: "fmt".into(),
                project_root: PathBuf::from("/repo"),
                changed_files: vec!["src/a.txt".into()],
                issue_files: vec!["src/a.txt".into()],
                diagnostics: PathBuf::from("/tmp/fmt.txt"),
                output: "src/a.txt:1: bad".into(),
            })));
        let rendered = output.rendered_agent_feedback();
        assert_eq!(
            rendered,
            vec![
                "velvet-glove: Fmt changed src/a.txt (re-read before editing); issues remain in src/a.txt:\nsrc/a.txt:1: bad"
            ]
        );
        assert!(
            format_notice(&output.notices[0]).starts_with(
                "velvet-glove: Fmt: messages.issuesChangedAgent could not be rendered"
            ),
            "{:?}",
            output.notices
        );
    }

    #[test]
    fn auto_fix_line_names_at_most_ten_files() {
        let files = (0..14)
            .map(|n| format!("src/f{n:02}.rs"))
            .collect::<Vec<_>>();
        let line = auto_fixed_line(&[AutoFixed {
            tool: "cargo fmt".into(),
            files,
            in_agent_line: true,
        }])
        .unwrap();
        assert!(line.contains("src/f09.rs (cargo fmt)"), "{line}");
        assert!(!line.contains("src/f10.rs"), "{line}");
        assert!(
            line.ends_with(" and 4 more; re-read before editing."),
            "{line}"
        );
    }
}
