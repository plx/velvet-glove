//! Copier-owned session-state construction and typed capability adapters.
//!
//! Keep the family and entity versions stable while their persisted schemas
//! remain compatible. Bump the corresponding version before making an
//! incompatible change.

use hookkit_core::RuntimeContext;
use hookkit_session_state::{FamilyId, Result, SessionState, StateFamily, StateRoot};
use std::path::{Path, PathBuf};

/// On-disk namespace for this generated hook family.
pub const STATE_FAMILY: &str = "velvet-glove";

/// Persisted schema version for this generated hook family.
///
/// Bump this before making an incompatible change to any family-wide state.
pub const STATE_FAMILY_VERSION: u32 = 1;

/// Stable package-specific directory beneath HookKit's generated-state root.
pub const PACKAGE_STATE_DIRECTORY: &str = "velvet-glove";

/// Resolve the common state root used by every generated subcommand.
///
/// A `--state-dir` value is passed as `override_dir`. Without one, state is
/// isolated beneath `$TMPDIR/agent-hook-kit/generated/velvet-glove/`.
pub fn resolve_state_root(override_dir: Option<&Path>) -> StateRoot {
    let path = override_dir.map_or_else(default_state_directory, Path::to_path_buf);
    StateRoot::new(path)
}

/// Ensure typed state and capture the exact metadata available to this event.
pub fn ensure_session_state(
    context: &RuntimeContext<'_>,
    override_dir: Option<&Path>,
) -> Result<SessionState> {
    SessionState::ensure(context, resolve_state_root(override_dir))
}

/// Open this generated tool's independently versioned state family.
pub fn state_family(state: &SessionState) -> Result<StateFamily> {
    state.family(FamilyId::new(STATE_FAMILY, STATE_FAMILY_VERSION)?)
}

fn default_state_directory() -> PathBuf {
    std::env::temp_dir()
        .join("agent-hook-kit")
        .join("generated")
        .join(PACKAGE_STATE_DIRECTORY)
}

/// Open the package-isolated pending file-activity and reconciliation journals.
pub fn file_activity_store(
    context: &RuntimeContext<'_>,
    override_dir: Option<&Path>,
) -> hookkit_file_activity::Result<hookkit_file_activity::FileActivityStore> {
    hookkit_file_activity::FileActivityStore::ensure(context, resolve_state_root(override_dir))
}

/// Read HookKit's typed metadata for an ensured session.
pub fn session_metadata(state: &SessionState) -> Result<hookkit_session_state::SessionMetadata> {
    state.metadata()
}

/// Stable identity prefix of committed run bundles.
pub const RUN_ARTIFACTS: &str = "artifacts";

/// Persisted schema version of run summaries and their referenced artifacts.
///
/// Bump this before changing the committed `summary.json` schema.
pub const RUN_ARTIFACTS_VERSION: u32 = 1;

/// Start one run bundle for relative artifacts and a committed summary.
pub fn start_artifact_run(
    state: &SessionState,
    label: &str,
) -> Result<hookkit_session_state::RunBundle> {
    let label = format!("{RUN_ARTIFACTS}-v{RUN_ARTIFACTS_VERSION}-{label}");
    state_family(state)?.start_run(&label)
}
