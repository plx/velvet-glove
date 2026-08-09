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

## Agent plugins

This repository is an experimental plugin marketplace for both Claude Code and
Codex. Both marketplace entries install the shared `velvet-glove` plugin, which
registers the deferred SessionStart, PostToolUse, and Stop workflow and includes
the `working-with-velvet-glove` skill skeleton.

The plugin does not bundle prebuilt executables yet. Install `velvet-glove` and
Pkl 0.31.1 separately; if `velvet-glove` is not on `PATH`, its launcher warns at
session start and otherwise exits as a protocol-safe no-op.

```sh
# Claude Code
claude plugin marketplace add plx/velvet-glove
claude plugin install velvet-glove@velvet-glove

# Codex
codex plugin marketplace add plx/velvet-glove
codex plugin add velvet-glove@velvet-glove
```

Codex requires newly installed or changed command hooks to be reviewed before
they run. Open `/hooks` in the Codex CLI after installing the plugin.

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
[built-in workflow audit](docs/builtin-deferred-workflow-audit.md), the
[validation coverage report](docs/builtin-validation-coverage.md), the
[pinned environment guide](docs/pinned-tool-environments.md), and the
[configuration reference](docs/configuration.md). The coverage report keeps
schema, rendered-command, and pinned-real-tool evidence separate; existing
host-dependent fixtures are inventory rather than pinned evidence.

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
# Marketplace, plugin, skill, hook, and launcher checks. Requires the Claude
# Code and Codex CLIs.
just validate-plugins

# Complete local pre-PR check, including the plugin checks above.
just check

# Exact selected real-tool contract (macOS 26+, Apple developer tools,
# Apple silicon, mise 2026.5.15).
just tool-case jq multi-file-fragments

# Seven behavior-rich contracts: data formats, Node, Python, Go, Rust, Ruby,
# and native macOS.
just tool-representatives

cargo fmt --all -- --check
cargo +1.85.0 check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets

# Optional host-PATH compatibility lane; missing programs are structured skips.
cargo test -p velvet-glove --test tool_fixtures -- --ignored --nocapture
```

The data-formats representative pins the official jq 1.8.2 macOS arm64
release and its GitHub SLSA provenance, then exercises `jq empty` once per
file. The `-e` option is intentionally absent: `empty` produces no result, so
`jq -e empty` exits four even for valid input. Per-file execution prevents jq
from joining adjacent file bytes into one parse stream. Within an individual
file, the contract still accepts an empty stream or multiple
whitespace-separated top-level values rather than requiring exactly one JSON
document. The exact artifact URLs, SHA-256 digests, status mapping, and
provenance identity are in the
[pinned environment guide](docs/pinned-tool-environments.md#jq-validation-contract).

Run `scripts/regen-licenses.sh` after dependency changes. The generated
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) is checked in alongside
the dual [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) licenses.
