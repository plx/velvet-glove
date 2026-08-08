# Velvet Glove CLI

Deferred linting and formatting for coding agents.

`velvet-glove` is a Rust 2024 hook executable generated from
`agent-hook-kit`. It always takes an explicit event subcommand.
Cross-harness commands also require an explicit `--harness` value.
It never guesses the hook from input JSON.

## Commands

```sh
velvet-glove \
  --harness <claude|codex|antigravity> \
  --state-dir <state-directory> \
  post-tool
velvet-glove \
  --harness <claude|codex|antigravity> \
  --config <absolute-config-path> \
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

The runner requires `pkl` 0.31.1. The generated package
owns this policy:

`crates/velvet-glove/config/velvet-glove.pkl`

Hook processes do not necessarily start in this repository, so use an absolute
policy path in live registration while retaining this package-owned file. The
concrete tools and lowering policy are recorded in both the Pkl file and
`hookkit-template.manifest.yml`.

All selected state helpers share this package-specific default root:

`$TMPDIR/agent-hook-kit/generated/velvet-glove/`

Use `--state-dir` to override it. Every coordinated producer and consumer must
receive the same override. Persisted family/entity versions are visible in
`src/scaffold/state.rs`.

Every generated event-handler seam receives the parsed state directory as its
fourth argument, including stateless projects. A custom handler can open state
with `scaffold::state::ensure_session_state(context, state_dir)` and then use
any of the selected claims, sets, queues, artifacts, or aggregate helpers. The
stable argument keeps handler edits compatible when state capabilities are
added later.

Library-owned runner commands have no editable event-handler seam. When a
state capability needs custom application logic beyond the runner's coordinated
metadata, file-activity, or artifact work, the questionnaire requires another
selected event with a user-owned handler.

## Edit policy safely

Files under `src/scaffold/` and the top-level modules in `src/hooks/` are
Copier-owned wiring. Customize the per-event and per-archetype files in the
`src/hooks/aligned/`, `native/`, `pre_tool/`, and `state_types/` directories;
those seams are preserved on recopy, including when selections change. Files
left behind after deselection are harmless and become active again if that
choice is reselected.

Generated files under `config/` are also preserved because they are expected
to become project policy. If you change runner archetype, quality-tool, or
configuration answers later, reconcile the existing Pkl policy manually.

Hook stdout, stderr, and exit status are protocol outputs: do not use
`println!` or uncontrolled `eprintln!` in handlers. Return native decisions
through the generated adapter instead.

## Validate

Run these commands from the generated repository root:

```sh
cargo fmt \
  --manifest-path crates/velvet-glove/Cargo.toml \
  --all -- --check
cargo +1.85.0 check \
  --manifest-path crates/velvet-glove/Cargo.toml \
  --all-targets
cargo clippy \
  --manifest-path crates/velvet-glove/Cargo.toml \
  --all-targets --all-features -- -D warnings
cargo test \
  --manifest-path crates/velvet-glove/Cargo.toml \
  --all-targets
```

Update this generated project with:

```sh
uvx --from copier==9.17.1 copier update .
uvx --from copier==9.17.1 copier check-update --quiet .
uvx --from copier==9.17.1 copier recopy .
```

Use `update` for normal smart diffing. `recopy` deliberately regenerates the
managed tree from recorded answers and is intended for explicit answer/shape
changes; protected handler seams remain untouched.

After the first successful check, commit the repository's resulting
`Cargo.lock` so the selected HookKit source and transitive dependencies remain
reproducible. Generated CI may use `--locked` after that lockfile exists.

Treat `package_name`, `crate_path`, and `binary_name` as instance identity.
Changing them moves owned files, answers/workflow names, registration commands,
or state namespaces; perform that as an explicit migration or a fresh copy,
not as an ordinary interactive update.

All HookKit crates are pinned to commit `3429c4b30ae765155dbcb314161f50a51171dc23`.
