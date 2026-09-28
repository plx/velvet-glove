# Velvet Glove CLI

`velvet-glove` is the single executable behind Velvet Glove's hooks and its
setup commands. Hook commands always name the native event explicitly and
require `--harness`; the binary never guesses the event from input JSON.

## Hook commands

```sh
velvet-glove --harness <claude|codex|antigravity> [--config PATH] post-tool-immediate
velvet-glove --harness <claude|codex|antigravity> [--state-dir DIR] post-tool
velvet-glove --harness <claude|codex|antigravity> [--config PATH] [--state-dir DIR] turn-completion
velvet-glove --harness <claude|codex> [--state-dir DIR] session-start-state
```

`post-tool-immediate` runs the configured tools after one tool call. The
deferred suite (`session-start-state`, `post-tool`, `turn-completion`) records
edits during a turn and runs the tools once at Stop; every command in the
suite must use the same state root, `$TMPDIR/velvet-glove/state/` by default.
Options that a command would ignore (`--config` on `post-tool`, `--state-dir`
on `post-tool-immediate`) are rejected instead.

Hook stdout, stderr, and exit status are protocol outputs. Do not add
`println!` or uncontrolled `eprintln!` calls to hook paths.

## Setup commands

```sh
velvet-glove tools [--dir DIR] [--json]                  # builtin catalog and where each executable resolves
velvet-glove init [--dir DIR] [--print] [--force]        # write .velvet-glove/post-tool-use.pkl
velvet-glove [--config PATH] [--state-dir DIR] doctor [--dir DIR]
velvet-glove [--config PATH] check [--dir DIR] [--json] [FILES...]
```

These write for a person (or, with `--json`, a script) and do not take
`--harness`. `check` runs the Stop-time workflows on the named files, or on
Git's changed and untracked files, outside any hook, and exits 0 (clean or
auto-fixed), 1 (manual fixes needed), or 2 (a tool could not run, or the
policy is broken). See
[the configuration reference](../../docs/configuration.md#generating-and-checking-a-policy)
for how `init` selects tools, what `doctor` checks, and what `check` prints.

Pkl 0.31.1 or newer is required. Without `--config`, policies are discovered
from `.velvet-glove/post-tool-use.pkl` and `post-tool-use.local.pkl` around
the event workspace (or `--dir`), after `~/.velvet-glove/post-tool-use.pkl`;
legacy `.agent-hook-kit` files are read first, at lower precedence. An
example policy lives at [`config/velvet-glove.pkl`](config/velvet-glove.pkl).

## Layout

- `src/scaffold/` — CLI parsing, dispatch, and thin adapters to the runners in
  `hookkit-tool-runner`.
- `src/commands/` — the `tools`, `doctor`, `init`, and `check` commands.
- `src/hooks/aligned/` — no-op portable handlers kept for the aligned
  protocol conformance tests.

The crate started from HookKit's `deferred_quality` Copier template
(`.copier-answers.yml` records the answers), but the CLI, dispatch, and
runner adapters have since diverged deliberately; do not re-apply the
template blindly. The remaining HookKit framework crates are pinned to commit
`83c49d46970602e8fb40a8afaeea521dfb7e9b61`; see
[the migration guide](../../docs/migrating-from-agent-hook-kit.md).

## Validate

From the repository root:

```sh
just check
cargo test -p velvet-glove --test tool_fixtures -- --ignored --nocapture  # real tools
```
