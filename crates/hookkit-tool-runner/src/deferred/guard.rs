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
    /// Whether this Stop was treated as a continuation of a blocked one.
    pub stop_hook_active: bool,
    pub next: LoopGuardState,
}

/// Decide whether a Stop that `wants_block` may block.
///
/// `native_stop_hook_active` is the harness's own flag. Without one
/// (Antigravity), only the Stop right after a block is presumed to continue
/// the chain: any allowed Stop ends the agent's turn, so the chain count is
/// cleared (keeping the fingerprint) and the next Stop starts a new chain.
pub(crate) fn decide(
    previous: &LoopGuardState,
    native_stop_hook_active: Option<bool>,
    wants_block: bool,
    fingerprint: &str,
    max_consecutive_blocks: u32,
) -> GuardDecision {
    let stop_hook_active = native_stop_hook_active.unwrap_or(previous.consecutive_blocks > 0);
    let mut decision = decide_with_flag(
        previous,
        stop_hook_active,
        wants_block,
        fingerprint,
        max_consecutive_blocks,
    );
    if native_stop_hook_active.is_none() && !decision.block {
        decision.next.consecutive_blocks = 0;
    }
    decision
}

fn decide_with_flag(
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
            stop_hook_active,
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
            stop_hook_active,
            next: held,
        };
    }
    if max_consecutive_blocks > 0 && chain >= max_consecutive_blocks {
        return GuardDecision {
            block: false,
            note: Some(format!(
                "velvet-glove: not blocking again after {chain} consecutive blocks."
            )),
            stop_hook_active,
            next: held,
        };
    }
    GuardDecision {
        block: true,
        note: None,
        stop_hook_active,
        next: LoopGuardState {
            fingerprint: Some(fingerprint.to_owned()),
            consecutive_blocks: chain + 1,
        },
    }
}

/// Fingerprint the issues that would block: each manual report's tool,
/// workflow, and blamed files together with a digest of each blamed file's
/// current bytes, plus any blocking operational problems and coverage gaps.
///
/// Hashing the blamed files rather than the check output means "the same
/// issues remain" exactly when the agent left those files untouched. Tool
/// output is too volatile to compare: timestamps, timings, seeds, and batch
/// composition (a retry re-checks only the requeued files) all change it
/// between identical Stops.
pub(crate) fn issue_fingerprint(
    result: &DeferredRunResult,
    blocks: BlockReasons,
    roots: &[&Path],
) -> String {
    let mut parts = Vec::<String>::new();
    if blocks.manual {
        // A set: the same tool/workflow/file reported by several jobs is one
        // issue for the guard.
        let mut manual = std::collections::BTreeSet::new();
        for report in result.manual_reports() {
            for path in &report.issue_files {
                let digest = std::fs::read(path)
                    .map(|bytes| excerpt::fingerprint([bytes]))
                    .unwrap_or_else(|_| "missing".into());
                manual.insert(format!(
                    "manual\0{}\0{}\0{}={digest}",
                    report.tool_id,
                    report.workflow_id,
                    excerpt::relativize(&path.to_string_lossy(), roots)
                ));
            }
        }
        parts.extend(manual);
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
        let decision = decide(&state("old", 2), Some(false), true, "new", 3);
        assert!(decision.block);
        assert_eq!(decision.next, state("new", 1));
    }

    #[test]
    fn identical_issues_after_a_block_are_allowed_with_a_note() {
        let decision = decide(&state("same", 1), Some(true), true, "same", 3);
        assert!(!decision.block);
        assert!(decision.note.unwrap().contains("same issues"));
        assert_eq!(decision.next, state("same", 1));
    }

    #[test]
    fn identical_issues_in_a_new_turn_block_again() {
        let decision = decide(&state("same", 1), Some(false), true, "same", 3);
        assert!(decision.block);
    }

    #[test]
    fn changed_issues_block_until_the_cap() {
        let decision = decide(&state("a", 2), Some(true), true, "b", 3);
        assert!(decision.block);
        assert_eq!(decision.next, state("b", 3));
        let capped = decide(&decision.next, Some(true), true, "c", 3);
        assert!(!capped.block);
        assert!(capped.note.unwrap().contains("3 consecutive blocks"));
        assert!(decide(&state("a", 50), Some(true), true, "b", 0).block);
    }

    #[test]
    fn clean_results_reset_the_guard() {
        let decision = decide(&state("a", 2), Some(true), false, "ignored", 3);
        assert!(!decision.block);
        assert!(decision.note.is_none());
        assert_eq!(decision.next, LoopGuardState::default());
    }

    #[test]
    fn without_a_native_flag_only_the_stop_after_a_block_continues_the_chain() {
        // Turn 1: the agent is blocked up to the cap on changing issues.
        let mut guard = LoopGuardState::default();
        let mut blocks = 0;
        for turn in 0..4 {
            let decision = decide(&guard, None, true, &format!("issues-{turn}"), 3);
            blocks += usize::from(decision.block);
            assert_eq!(decision.stop_hook_active, turn > 0);
            guard = decision.next;
        }
        assert_eq!(blocks, 3);
        assert_eq!(
            guard.consecutive_blocks, 0,
            "an allowed Stop ends the chain"
        );

        // A later turn with new issues starts a new chain and blocks.
        let later = decide(&guard, None, true, "new-issues", 3);
        assert!(later.block);
        assert!(!later.stop_hook_active);
        assert_eq!(later.next, state("new-issues", 1));

        // Identical issues right after a block are still allowed once.
        let same = decide(&later.next, None, true, "new-issues", 3);
        assert!(!same.block);
        assert_eq!(same.next, state("new-issues", 0));
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
