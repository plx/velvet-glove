# Migrating from the HookKit example

Velvet Glove contains the former `hookkit-tool-runner` and
`hookkit-pkl-config` product code. The remaining HookKit crates stay pinned as
Git dependencies until HookKit is published.

## Command mapping

| Former executable | Velvet Glove command |
| --- | --- |
| `post-tool-use-agent-hook` | `velvet-glove post-tool-immediate` |
| `file-activity-agent-hook` | `velvet-glove post-tool` |
| `turn-completion-agent-hook` | `velvet-glove turn-completion` |
| `session-start-state-agent-hook` | `velvet-glove session-start-state` |

Put global options such as `--harness`, `--config`, and `--state-dir` before
the subcommand. The former executables are not installed by Velvet Glove;
update registrations to use the unified executable and matching subcommand.

## Configuration namespace

New configuration belongs under `.velvet-glove`. During migration,
`.agent-hook-kit` configuration is still read at lower precedence within each
home, project, or local layer. An explicit `--config PATH` continues to bypass
all discovery.

Copy and rename active project configuration rather than relying permanently
on the fallback:

```text
.agent-hook-kit/post-tool-use.pkl       -> .velvet-glove/post-tool-use.pkl
.agent-hook-kit/post-tool-use.local.pkl -> .velvet-glove/post-tool-use.local.pkl
```

The local file should remain uncommitted.

## State namespace

The default state root changed to `$TMPDIR/velvet-glove/state`, and the runner
family changed from `agent-hook-kit.batched-tools` to
`velvet-glove.batched-tools`. Existing pending generations are intentionally
not imported: they are transient, and blindly replaying them could run tools
against stale paths. Finish or restart active agent sessions when switching
registrations. To preserve an explicit state location, pass the same
`--state-dir` value to all three deferred commands.
