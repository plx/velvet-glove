//! Immediate-mode diagnostic files and the artifacts that point at them.

use crate::jobs::ToolContext;
use crate::paths::absolute_from;
use hookkit_common::UserNotice;
use hookkit_common::message::{DiagnosticArtifact, DiagnosticReport};
use hookkit_core::RuntimeContext;
use hookkit_runtime::artifacts::{ArtifactKey, ArtifactManager};
use std::path::{Path, PathBuf};

/// Default home of immediate-mode diagnostics, outside the project.
pub(crate) fn immediate_log_directory() -> PathBuf {
    std::env::temp_dir()
        .join("velvet-glove")
        .join("state")
        .join("post-tool-immediate")
}

/// Write full diagnostics to the configured `diagnosticsDirectory`, or to the
/// default location (with a user notice) when that cannot be written.
pub(crate) fn write_diagnostics(
    label: &str,
    diagnostics: &str,
    context: &ToolContext<'_>,
    ctx: &RuntimeContext<'_>,
    notices: &mut Vec<UserNotice>,
) -> hookkit_core::Result<PathBuf> {
    let label = format!("{}-{label}", context.spec.id);
    let configured = context
        .spec
        .diagnostics_directory
        .as_deref()
        .or(context.global_diagnostics_dir)
        .map(|dir| absolute_from(Path::new(dir), context.project_root));
    if let Some(directory) = configured {
        match write_immediate_artifact(&directory, &label, diagnostics, ctx) {
            Ok(path) => return Ok(path),
            Err(error) => notices.push(UserNotice::warning(format!(
                "velvet-glove: cannot write diagnostics to {} ({error}); using the default location",
                directory.display()
            ))),
        }
    }
    write_immediate_artifact(&immediate_log_directory(), &label, diagnostics, ctx)
}

pub(crate) fn write_immediate_artifact(
    directory: &Path,
    label: &str,
    text: &str,
    ctx: &RuntimeContext<'_>,
) -> hookkit_core::Result<PathBuf> {
    let manager = ArtifactManager::new(directory)?;
    manager
        .write_text(&runner_artifact_key(ctx, label.to_owned()), text)
        .map_err(Into::into)
}

pub(crate) fn runner_artifact_key(ctx: &RuntimeContext<'_>, label: String) -> ArtifactKey {
    let session = ctx
        .session_id()
        .map(ToString::to_string)
        .or_else(|| ctx.conversation_id().map(ToString::to_string))
        .unwrap_or_else(|| "unknown-session".to_string());
    let mut key = ArtifactKey::new(session, label);
    if let Some(turn) = ctx.turn_id() {
        key = key.with_turn(turn.to_string());
    }
    if let Some(tool_call) = ctx.tool_call_id() {
        key = key.with_tool_use(tool_call.to_string());
    }
    key
}

pub(crate) fn report_with_artifact(
    title: String,
    diagnostics: String,
    artifact_path: PathBuf,
    project_root: &Path,
) -> DiagnosticReport {
    let mut artifact = DiagnosticArtifact::new(artifact_path.clone(), "text/plain");
    if let Ok(rel_path) = artifact_path.strip_prefix(project_root) {
        artifact = artifact.with_project_relative_path(rel_path);
    }
    artifact = artifact.with_summary(&title);
    DiagnosticReport::new(title, diagnostics).with_artifact(artifact)
}
