# Velvet Glove CLI

Deferred linting and formatting for coding agents.

`velvet-glove` is a Rust 2024 hook executable bootstrapped from
[`agent-hook-kit`](https://github.com/plx/agent-hook-kit). It always takes an
explicit event subcommand.
Cross-harness commands also require an explicit `--harness` value.
It never guesses the hook from input JSON.

## Commands

```sh
velvet-glove \
  --harness <claude|codex|antigravity> \
  [--config <config-path>] \
  post-tool-immediate
velvet-glove \
  --harness <claude|codex|antigravity> \
  --state-dir <state-directory> \
  post-tool
velvet-glove \
  --harness <claude|codex|antigravity> \
  [--config <config-path>] \
  --state-dir <state-directory> \
  turn-completion

velvet-glove \
  --harness <selected-claude-or-codex> \
  --state-dir <state-directory> \
  session-start-state
```

Use only the harnesses recorded in `hookkit-template.manifest.yml`. Register
the complete command line for the matching native hook event. The template
does not edit live Claude Code, Codex, or Antigravity configuration.

The runner requires `pkl` 0.31.1. Pass `--config` to bypass discovery and use
one explicit policy. When it is omitted, Velvet Glove discovers canonical
`.velvet-glove/post-tool-use.pkl` and `post-tool-use.local.pkl` files around
the event workspace; legacy `.agent-hook-kit` files are read first at lower
precedence. The generated package includes this example policy:

`crates/velvet-glove/config/velvet-glove.pkl`

Hook processes do not necessarily start in this repository. Use an absolute
path when registering this package-owned example explicitly, or copy an
adapted policy to the target repository's `.velvet-glove` directory.

All selected state helpers share this package-specific default root:

`$TMPDIR/velvet-glove/state/`

Use `--state-dir` to override it. Every coordinated producer and consumer must
receive the same override. Persisted family/entity versions are visible in
`src/scaffold/state.rs`.

Every generated event-handler seam receives the parsed state directory as its
fourth argument, including stateless projects. A custom handler can open state
with `scaffold::state::ensure_session_state(context, state_dir)` and then use
any of the selected claims, sets, queues, artifacts, or aggregate helpers. The
stable argument keeps handler edits compatible when state capabilities are
added later.

Runner commands have no editable event-handler seam. When a
state capability needs custom application logic beyond the runner's coordinated
metadata, file-activity, or artifact work, the questionnaire requires another
selected event with a user-owned handler.

## Edit policy safely

This wrapper began as HookKit's `deferred_quality` Copier starter, but the
migration deliberately changed the generated CLI, dispatch, and runner
adapters to add immediate execution and discovery. The two local product crates
are not Copier-managed. Run Copier updates on a branch and review them as a
three-way migration; a blind recopy can remove these intentional changes.

Generated files under `config/` are also preserved because they are expected
to become project policy. If you change runner archetype, quality-tool, or
configuration answers later, reconcile the existing Pkl policy manually.

Hook stdout, stderr, and exit status are protocol outputs: do not use
`println!` or uncontrolled `eprintln!` in handlers. Return native decisions
through the generated adapter instead.

## Validate

Run these commands from the generated repository root:

```sh
cargo fmt --all -- --check
cargo +1.85.0 check --workspace --all-targets --locked
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets

# Opt-in lane that executes controlled real formatter/linter versions.
cargo test -p velvet-glove --test tool_fixtures -- --ignored --nocapture
```

To compare against a newer template without immediately overwriting the tree:

```sh
uvx --from copier==9.17.1 copier check-update --quiet .
```

If an update is desired, use Copier's smart diff on a disposable branch and
manually preserve Velvet Glove's unified-command and local-crate changes.

After the first successful check, commit the repository's resulting
`Cargo.lock` so the selected HookKit source and transitive dependencies remain
reproducible. Generated CI may use `--locked` after that lockfile exists.

Treat `package_name`, `crate_path`, and `binary_name` as instance identity.
Changing them moves owned files, answers/workflow names, registration commands,
or state namespaces; perform that as an explicit migration or a fresh copy,
not as an ordinary interactive update.

All HookKit framework crates are pinned to commit
`83c49d46970602e8fb40a8afaeea521dfb7e9b61`.

The default state namespace intentionally does not import pending generations
from the old HookKit example. Finish or restart active sessions before changing
registrations. See the repository-level
[`docs/migrating-from-agent-hook-kit.md`](../../docs/migrating-from-agent-hook-kit.md)
for command, config, and state migration details.
