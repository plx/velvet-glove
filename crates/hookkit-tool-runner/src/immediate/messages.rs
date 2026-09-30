//! Rendering a tool's configured message templates, with built-in fallbacks.

use crate::errors::{error_summary, invalid_data};
use crate::jobs::ToolContext;
use crate::paths::slash_path;
use hookkit_common::UserNotice;
use hookkit_core::HookkitError;
use minijinja::Environment;
use std::path::Path;

/// The tool identity a message template sees.
pub(super) struct TemplateTool<'a> {
    pub(super) name: &'a str,
    pub(super) id: &'a str,
    pub(super) project_root: &'a Path,
}

impl ToolContext<'_> {
    pub(super) fn template_tool(&self) -> TemplateTool<'_> {
        TemplateTool {
            name: &self.spec.display_name,
            id: &self.spec.id,
            project_root: self.project_root,
        }
    }
}

/// Values a message template may reference besides the tool.
#[derive(Default)]
pub(super) struct MessageArgs<'a> {
    pub(super) changed_files: &'a [String],
    pub(super) issue_files: &'a [String],
    pub(super) diagnostics_path: Option<&'a Path>,
    /// Phase, executable, and install hint of a failed or missing command.
    pub(super) phase_error: Option<(&'a str, &'a str, Option<&'a str>)>,
    pub(super) excerpt: &'a str,
}

pub(super) fn render_template(
    template: &str,
    tool: &TemplateTool<'_>,
    args: &MessageArgs<'_>,
) -> hookkit_core::Result<String> {
    let diagnostics_path_text = args
        .diagnostics_path
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let diagnostics_rel_path = args
        .diagnostics_path
        .and_then(|path| path.strip_prefix(tool.project_root).ok())
        .map(slash_path)
        .unwrap_or_default();
    let (phase, executable, install_hint) = args.phase_error.unwrap_or(("", "", None));
    let json_context = serde_json::json!({
        "tool": tool.name,
        "tool_id": tool.id,
        "changed_files": args.changed_files,
        "issue_files": args.issue_files,
        "diagnostics_path": diagnostics_path_text,
        "diagnostics_absolute_path": diagnostics_path_text,
        "diagnostics_rel_path": diagnostics_rel_path,
        "diagnostics_project_path": diagnostics_rel_path,
        "project_root": tool.project_root.to_string_lossy(),
        "phase": phase,
        "executable": executable,
        "install_hint": install_hint.unwrap_or(""),
        "excerpt": args.excerpt,
    });

    Environment::new()
        .render_str(template, &json_context)
        .map_err(|e| invalid_data(format!("failed to render message template: {e}")))
}

/// Render `template`, or the built-in `fallback` with a user notice when the
/// configured template cannot be rendered: the agent must still hear about
/// changed files and remaining issues.
pub(super) fn render_with_fallback(
    template: &str,
    fallback: &str,
    field: &str,
    tool: &TemplateTool<'_>,
    args: &MessageArgs<'_>,
    notices: &mut Vec<UserNotice>,
) -> String {
    render_template(template, tool, args).unwrap_or_else(|error| {
        notices.push(template_failure_notice(tool, field, &error));
        render_template(fallback, tool, args).unwrap_or_default()
    })
}

pub(super) fn template_failure_notice(
    tool: &TemplateTool<'_>,
    field: &str,
    error: &HookkitError,
) -> UserNotice {
    UserNotice::warning(format!(
        "velvet-glove: {}: messages.{field} could not be rendered ({}); used the default",
        tool.name,
        error_summary(&error.to_string())
    ))
}
