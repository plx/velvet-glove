# Hook lifecycle

The plugin's launcher maps each native event to one command:

| Event | Deferred mode (default) | Immediate mode |
| --- | --- | --- |
| SessionStart | `session-start-state`: record the session's start | nothing |
| PostToolUse | `post-tool`: quietly record which files the call touched | `post-tool-immediate`: run the tools on those files now |
| Stop | `turn-completion`: run the tools on everything recorded, then report or block | `{}` |

Deferred commands share a state root, `$TMPDIR/velvet-glove/state` by
default. At Stop, candidates are the recorded files plus files modified since
the last Stop (an mtime scan that skips VCS metadata, dependency trees, build
output, and tool caches); Git-ignored files never count. Each tool's check
runs, then its automatic fix where needed, then a final check. Every command's
log and a `summary.json` go into a run directory under the state root.

Outputs follow one contract: clean is silent; auto-fixes produce one line for
the user, which the agent sees only when Stop blocks (so it re-reads before
fixing the rest); manual issues block Stop, with a bounded excerpt of the
final check for the agent and a count, file list, and run directory for the
user; tool and configuration problems only notify the user. Immediate mode
never blocks: auto-fixes and remaining issues reach the agent as context, the
issues with the same kind of excerpt.

Hook stdout is protocol JSON; `velvet-glove check` runs the same Stop-time
engine by hand, outside any hook and its state.
