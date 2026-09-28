//! Stop-hook loop guard.
//!
//! A blocked Stop makes the agent continue; its next Stop arrives with the
//! harness's `stop_hook_active` flag set. The guard remembers the issues that
//! caused the last block so an agent that cannot (or will not) fix them is not
//! blocked on the identical issues again, and caps consecutive blocks.

use super::{BlockReasons, DeferredRunResult};
use crate::excerpt;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Persisted guard state for one session family.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoopGuardState {
    /// Fingerprint of the issues behind the most recent block.
    pub fingerprint: Option<String>,
    /// Blocks issued in the current chain of stop-hook continuations.
    pub consecutive_blocks: u32,
}

impl LoopGuardState {
    /// Read persisted state; unreadable or missing state starts fresh.
    pub(crate) fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Best-effort atomic replacement; a lost write only weakens the guard.
    pub(crate) fn save(&self, path: &Path) {
        let Ok(bytes) = serde_json::to_vec(self) else {
            return;
        };
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        if std::fs::write(&temporary, bytes).is_ok() && std::fs::rename(&temporary, path).is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
    }
}

/// Outcome of applying the guard to a result that may block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuardDecision {
    pub block: bool,
    /// User-facing explanation when the guard allowed completion instead.
    pub note: Option<String>,
    pub next: LoopGuardState,
}

pub(crate) fn decide(
    previous: &LoopGuardState,
    stop_hook_active: bool,
    wants_block: bool,
    fingerprint: &str,
    max_consecutive_blocks: u32,
) -> GuardDecision {
    if !wants_block {
        return GuardDecision {
            block: false,
            note: None,
            next: LoopGuardState::default(),
        };
    }
    // A Stop without an active stop hook starts a new chain.
    let chain = if stop_hook_active {
        previous.consecutive_blocks
    } else {
        0
    };
    let held = LoopGuardState {
        fingerprint: Some(fingerprint.to_owned()),
        consecutive_blocks: chain,
    };
    if stop_hook_active && previous.fingerprint.as_deref() == Some(fingerprint) {
        return GuardDecision {
            block: false,
            note: Some(
                "velvet-glove: not blocking again; the same issues remain after the agent's last attempt."
                    .into(),
            ),
            next: held,
        };
    }
    if max_consecutive_blocks > 0 && chain >= max_consecutive_blocks {
        return GuardDecision {
            block: false,
            note: Some(format!(
                "velvet-glove: not blocking again after {chain} consecutive blocks."
            )),
            next: held,
        };
    }
    GuardDecision {
        block: true,
        note: None,
        next: LoopGuardState {
            fingerprint: Some(fingerprint.to_owned()),
            consecutive_blocks: chain + 1,
        },
    }
}

/// Fingerprint the issues that would block: each manual report's tool,
/// workflow, blamed files, and normalized decisive check output, plus any
/// blocking operational problems and coverage gaps.
pub(crate) fn issue_fingerprint(
    result: &DeferredRunResult,
    blocks: BlockReasons,
    roots: &[&Path],
) -> String {
    let mut parts = Vec::<String>::new();
    if blocks.manual {
        for report in result.manual_reports() {
            parts.push("manual".into());
            parts.push(report.tool_id.clone());
            parts.push(report.workflow_id.clone());
            parts.extend(
                report
                    .issue_files
                    .iter()
                    .map(|path| excerpt::relativize(&path.to_string_lossy(), roots)),
            );
            if let Some(artifact) = result.latest_check_artifact(report) {
                parts.push(excerpt::normalize(&artifact.output, roots));
            }
        }
    }
    if blocks.operational {
        for problem in result.operational_problems.values() {
            parts.push("operational".into());
            parts.push(problem.id.clone());
            parts.push(problem.message.clone());
        }
    }
    if blocks.coverage {
        for gap in result.coverage_gaps.values() {
            parts.push("coverage".into());
            parts.push(gap.message.clone());
        }
    }
    excerpt::fingerprint(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(fingerprint: &str, blocks: u32) -> LoopGuardState {
        LoopGuardState {
            fingerprint: Some(fingerprint.into()),
            consecutive_blocks: blocks,
        }
    }

    #[test]
    fn first_block_starts_a_chain() {
        let decision = decide(&state("old", 2), false, true, "new", 3);
        assert!(decision.block);
        assert_eq!(decision.next, state("new", 1));
    }

    #[test]
    fn identical_issues_after_a_block_are_allowed_with_a_note() {
        let decision = decide(&state("same", 1), true, true, "same", 3);
        assert!(!decision.block);
        assert!(decision.note.unwrap().contains("same issues"));
        assert_eq!(decision.next, state("same", 1));
    }

    #[test]
    fn identical_issues_in_a_new_turn_block_again() {
        let decision = decide(&state("same", 1), false, true, "same", 3);
        assert!(decision.block);
    }

    #[test]
    fn changed_issues_block_until_the_cap() {
        let decision = decide(&state("a", 2), true, true, "b", 3);
        assert!(decision.block);
        assert_eq!(decision.next, state("b", 3));
        let capped = decide(&decision.next, true, true, "c", 3);
        assert!(!capped.block);
        assert!(capped.note.unwrap().contains("3 consecutive blocks"));
        assert!(decide(&state("a", 50), true, true, "b", 0).block);
    }

    #[test]
    fn clean_results_reset_the_guard() {
        let decision = decide(&state("a", 2), true, false, "ignored", 3);
        assert!(!decision.block);
        assert!(decision.note.is_none());
        assert_eq!(decision.next, LoopGuardState::default());
    }

    #[test]
    fn state_round_trips_and_tolerates_garbage() {
        let path =
            std::env::temp_dir().join(format!("hookkit-loop-guard-{}.json", std::process::id()));
        state("x", 2).save(&path);
        assert_eq!(LoopGuardState::load(&path), state("x", 2));
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(LoopGuardState::load(&path), LoopGuardState::default());
        let _ = std::fs::remove_file(path);
    }
}
