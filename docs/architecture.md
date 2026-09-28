# Velvet Glove runner design

The runner owns tool selection, recursive file discovery, Pkl policy, execution,
classification, diagnostics artifacts, and the `RunnerDomainOutcome` semantic
result. Core, native, common, and runtime crates do not depend on those policies.

Per-harness lowering is intentionally explicit. The aligned runtime preserves a
lossless Claude, Codex, or Antigravity input arm; runner-local policy
discovers paths from that exact value; and the runner constructs the
corresponding native output arm. No generic output envelope or universal
lowering layer participates.
Antigravity `PostToolUse` includes the originating tool call, so the runner can
derive call-scoped file candidates from its name and arguments. It still has no
tool result or changed-file list, and its empty output object cannot carry user
or agent messages. Strict lowering rejects those messages and best-effort omits
them. Best-effort-with-warnings writes a versioned, collision-safe JSON loss
record beneath the event's exact `artifactDirectoryPath`, then uses successful
protocol stderr to point to that record while preserving exact `{}` stdout.
Failure to persist the record fails the hook instead of silently dropping it.

The runtime context supplies exact harness, snapshot, event, and contract identity
plus only context fields declared by the native event. Operational diagnostics are
kept out of protocol stdout; user-visible stderr is emitted only through an exact
native output, while verbose remaining-tool output belongs in runner artifacts.

To add a tool, extend the Pkl catalog and its fake-executable orchestration cases;
real-tool fixtures belong to the opt-in compatibility lane. Give the spec a
`detect` block so `velvet-glove init` can suggest it; hooks never read that
block. To add a harness,
first prove how its native event yields changed files, then add an explicit final
lowering arm and exact native fixture tests.

Performance budgets are not yet release guarantees. The hermetic smoke lane checks
behavior but does not currently record startup, clean no-op, or subprocess timing;
add a dedicated benchmark/measurement lane before publishing performance claims.

## Immediate PostToolUse

`velvet-glove post-tool-immediate` observes the call's exact file candidates
first and returns `{}` without evaluating Pkl when there are none. Otherwise
it runs each `run` tool's phases over the matching files, one tool after
another; a tool's independent jobs run in parallel (`jobs = 0` is available
parallelism, capped at 8). Claude and Codex output uses the native channels:
agent text in `hookSpecificOutput.additionalContext`, user notices in the
user-only `systemMessage`, and never exit-0 stderr. Auto-fixes collapse into
one line for both audiences; full command output goes only to diagnostics
files, which default to `$TMPDIR/velvet-glove/state/post-tool-immediate`.
Policy load errors become a user notice rather than a hook failure.

Both hooks share command plumbing: bare program names resolve through
`settings.localBinDirs` before `PATH`, each tool's `env` is applied, and every
command has a wall-clock timeout (`settings.commandTimeoutSeconds`, per-tool
`timeoutSeconds`). A timed-out command is killed with its process group and
reported as an operational failure.

## Turn-completion batching

`velvet-glove turn-completion` reuses the same Pkl catalog and execution engine,
but obtains candidate files from the `hookkit-file-activity` pending entity
maintained by `velvet-glove post-tool`. That quiet aligned
PostToolUse observer delegates structured, patch, and shell analysis to
`hookkit-file-activity::observe_post_tool` and the shared tool-access layer.
The immediate runner uses the same observation path for exact file candidates
instead of maintaining a second open-payload walker. Before taking the entity view, it
reconciles workspace mtimes from the prior durable cursor, using current-session
start metadata only as the first lower bound. The aligned lifecycle is Claude,
Codex, or Antigravity Stop. Antigravity lacks a precise session-start
producer but its PostToolUse tool-call evidence can feed the tracker directly.

One runner-family advisory lock serializes stop attempts for a native session.
The consumer seals NDJSON generations and obtains their cached set projection
before executing tools. Candidates that Git ignores (one `git check-ignore`
call; a no-op outside a work tree) are dropped as not applicable.
Stop-time `workflows` are distinct from the immediate
runner's legacy `phases`: all non-mutating initial checks run first, then one
ordered remedy pass. Before a workflow whose check was clean decides against a
remedy, it reruns that check if an earlier remedy wrote into its scope (for
example, a Ruff lint fix that leaves a file unformatted). Snapshot-discovered
writes then invalidate intersecting target-file or workspace checks for one
authoritative final sweep. Identical check commands within one stage (the
compatibility translation pairs several mutators with one verifier) run once.
With `failFast`, an operational failure skips only the same tool's later
remedies. Check stages retain bounded job parallelism and deterministic result
ordering. The complex deferred policy is split across `deferred/model.rs`,
`deferred/execution.rs`, `deferred/attribution.rs`, `deferred/reporting.rs`,
`deferred/guard.rs`, and `deferred/lowering.rs`; `excerpt.rs` and `vcs.rs` hold
helpers both runners can share. The main module retains CLI, state
transaction, artifact, and immediate-runner orchestration so the two product
paths share conversion and process plumbing.

When a builtin has no explicit `workflows`, catalog validation proves its
compatibility translation has a read-only final phase before it can ship as
enabled. The generated
[`builtin-deferred-workflow-audit.md`](builtin-deferred-workflow-audit.md)
records every command, inferred or explicit scope, invocation granularity, and
known limitation. Immediate PostToolUse continues to use legacy `phases`.

Every executed deferred command writes its own artifact under a deterministic
tool/workflow/job/phase path in a unique run bundle. Artifact metadata includes
structured argv, working directory, candidate and changed files, exit code,
classification, and its report identity; the full output lives only in the log
file. One report/artifact can therefore be linked by every attributed file,
while a file covered by several tools retains all distinct links.

A file is auto-fixed only when a remedy changed its bytes and its final check
passed. A failing check's output decides attribution: issues belong to the
candidate (or remedy-changed) files it names; output naming only other
existing files is out of scope and does not block; output naming no file is
conservatively attributed to every candidate.

The runner commits `summary.json` only after every command artifact is durable
and before changing pending state. The summary contains run identity, counts,
normal buckets, current groups, artifact paths, the complete result model, the
block decision, rendered-message metadata, and the planned source disposition.
The runner then appends stable retry evidence for only manual, operationally
incomplete, and unresolved work (files whose only problem is a missing tool
under the default `user-notice` policy are not retried), records content-based
handled baselines for discharged work, and acknowledges the sealed source
generations. New observations written during execution are outside the snapshot
and remain pending independently. Mtime and opt-in Git-dirty reconciliation
suppress only fingerprints that still match a handled baseline; direct
observations always requeue the path. Each session family keeps its 20 newest
run bundles, and session directories idle for a week are removed.

Only manual issues block by default. Operational problems notify the user
(`deferredReporting.blockOnOperationalErrors` and `missingToolPolicy =
"harness-block"` opt into blocking), and strict coverage policy blocks on gaps.
A loop guard in the family's session scope records the fingerprint of the
issues behind the last block (tool, workflow, blamed files, normalized final
check output). When the harness reports `stop_hook_active` and the fingerprint
is unchanged, or `maxConsecutiveBlocks` is reached, completion is allowed with
a user note instead of another block. Antigravity has no such flag, so a Stop
right after a block is presumed to continue the chain.

Coverage gaps use the Pkl `fileActivity.coverageGapPolicy`. The default
`best-effort` policy retains and records incomplete targets in the summary
without messaging anyone or treating resolved clean files as manual. `strict`
also blocks Stop until the gap clears. Recursive target expansion is bounded by
`fileActivity.maxEntries`; exhaustion is both summarized and requeued.

### Exact Stop lowering

Rendered deferred messages are lowered without a common output envelope. The
capability matrix is:

| Native event | Allowed user | Allowed agent | Blocked user | Blocked agent |
| --- | --- | --- | --- | --- |
| Claude Stop | `systemMessage` | `hookSpecificOutput.additionalContext` | `systemMessage` | `reason` |
| Codex Stop | `systemMessage` | unavailable | `systemMessage` | `reason` |
| Antigravity Stop | unavailable | unavailable | unavailable | `reason` |

`loweringPolicy = "strict"` turns any nonempty unavailable audience into a
hook failure after committing the summary but before changing pending state.
`"best-effort"` omits that audience. `"best-effort-with-warnings"` also emits
an omission warning through `systemMessage` when available, unless the omitted
agent line is identical to the emitted user line; Antigravity can only use its
single `reason` fallback and cannot preserve audience separation. A blocked
completion never has an empty `reason`. The summary records emitted, omitted,
empty, or unrepresentable status for each audience. Allowed completion stays
allowed under both best-effort modes.

## Direct checks

`velvet-glove check` calls `hookkit_tool_runner::run_check`, which reuses the
deferred planner (`build_deferred_plan`), executor, artifact writer, and the
reporter's excerpt and problem summaries on an explicit candidate list. It
skips everything hook-specific: no native input, session state, file-activity
window, loop guard, or lowering. Logs and a `summary.json` go to a fresh
`$TMPDIR/velvet-glove/check/<millis>-<pid>` directory. The command itself
only chooses the files (explicit, expanded directories, or `git status`) and
renders the report as text or JSON.
