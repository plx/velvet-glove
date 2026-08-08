# Velvet Glove

Velvet Glove batches linting and formatting work so coding agents can stay
responsive while they edit. It can also run the same configured workflows
immediately after an individual tool call. Native hook adapters are provided
for Claude Code, Codex, and Antigravity through
[`agent-hook-kit`](https://github.com/plx/agent-hook-kit).

HookKit is not yet published as a crate. All upstream HookKit dependencies are
therefore pinned to Git commit
`83c49d46970602e8fb40a8afaeea521dfb7e9b61`.

## Install

Install Pkl 0.31.1, then build locally or install the public Git source:

```sh
cargo install --locked --git https://github.com/plx/velvet-glove velvet-glove
```

For development, `cargo build --release -p velvet-glove` writes the executable
to `target/release/velvet-glove`.

## Commands

Every invocation explicitly selects its harness and event:

| Command | Native event | Purpose |
| --- | --- | --- |
| `post-tool-immediate` | PostToolUse | Run applicable checks and fixes immediately. |
| `post-tool` | PostToolUse | Quietly record file activity for deferred work. |
| `turn-completion` | Stop/turn completion | Reconcile activity, run batched workflows, and report or block. |
| `session-start-state` | SessionStart | Record an exact Claude/Codex session lower bound. |

```sh
cargo build --release -p velvet-glove --bin velvet-glove

velvet-glove --harness claude post-tool-immediate
velvet-glove --harness codex post-tool
velvet-glove --harness codex turn-completion
velvet-glove --harness claude session-start-state
```

Antigravity does not expose a precise SessionStart hook. Its first observed
PostToolUse event supplies the best available lower bound instead.

## Configuration

Velvet Glove requires Pkl 0.31.1. Pass `--config PATH` to use one policy file
and bypass discovery. Without it, configuration is merged in this order:

1. legacy `~/.agent-hook-kit/post-tool-use.pkl`, then
   `~/.velvet-glove/post-tool-use.pkl`;
2. legacy project files from root to leaf, then canonical
   `<ancestor>/.velvet-glove/post-tool-use.pkl` files from root to leaf;
3. legacy local files from root to leaf, then canonical
   `<ancestor>/.velvet-glove/post-tool-use.local.pkl` files from root to leaf.

Within each layer, canonical `.velvet-glove` files win over their legacy
peers; local files still override project files. New projects should only
write `.velvet-glove`; the legacy read path exists to ease migration. The
generated example policy is
[`crates/velvet-glove/config/velvet-glove.pkl`](crates/velvet-glove/config/velvet-glove.pkl).

The embedded catalog contains immediate phases and deferred workflows for a
broad set of formatters and linters. See the generated
[built-in workflow audit](docs/builtin-deferred-workflow-audit.md) and the
[configuration reference](docs/configuration.md).

## Deferred hook suite

The three deferred commands must share the same state root. The default is
`$TMPDIR/velvet-glove/state`; use `--state-dir PATH` on every command to
override it.

| Purpose | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| Session lower bound | `session-start-state` | `session-start-state` | unavailable |
| Activity producer | `post-tool` | `post-tool` | `post-tool` |
| Deferred consumer | `turn-completion` | `turn-completion` | `turn-completion` |

The consumer commits command artifacts and `summary.json` before changing the
pending window. Clean and auto-fixed work is acknowledged; manual issues,
operational failures, and unresolved coverage gaps are retained for retry.
See [the architecture notes](docs/architecture.md) for the transaction and
native-lowering details. Existing HookKit-example users should also read the
[migration guide](docs/migrating-from-agent-hook-kit.md).

## Workspace

- `crates/velvet-glove` owns the public executable and unified CLI.
- `crates/hookkit-tool-runner` contains the migrated execution engine behind
  the public wrapper.
- `crates/hookkit-pkl-config` embeds the Pkl schema and built-in tool catalog.

The source tree was bootstrapped from HookKit's Copier template. Generated
scaffold files remain recorded in `.copier-answers.yml`; the migrated product
crates and policy are maintained here.

## Validate

```sh
cargo fmt --all -- --check
cargo +1.85.0 check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets

# Optional real-tool compatibility lane; requires controlled tool versions.
cargo test -p velvet-glove --test tool_fixtures -- --ignored --nocapture
```

Run `scripts/regen-licenses.sh` after dependency changes. The generated
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) is checked in alongside
the dual [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) licenses.
