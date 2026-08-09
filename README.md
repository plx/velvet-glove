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

<!-- markdownlint-disable MD013 -->

| Command | Native event | Purpose |
| --- | --- | --- |
| `post-tool-immediate` | PostToolUse | Run applicable checks and fixes immediately. |
| `post-tool` | PostToolUse | Quietly record file activity for deferred work. |
| `turn-completion` | Stop/turn completion | Reconcile activity, run batched workflows, and report or block. |
| `session-start-state` | SessionStart | Record an exact Claude/Codex session lower bound. |

<!-- markdownlint-enable MD013 -->

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

<!-- markdownlint-disable MD013 -->

| Purpose | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| Session lower bound | `session-start-state` | `session-start-state` | unavailable |
| Activity producer | `post-tool` | `post-tool` | `post-tool` |
| Deferred consumer | `turn-completion` | `turn-completion` | `turn-completion` |

<!-- markdownlint-enable MD013 -->

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

# Exact selected real-tool contracts (macOS 26+, Apple developer tools,
# Apple silicon, mise 2026.5.15).
just tool-case jq multi-file-fragments
just tool-case astro multi-file-project
just tool-case betterleaks multi-file
just tool-case biome multi-file
just tool-case prettier multi-file
just tool-case contextlint multi-file-project
just tool-case buf-format multi-file
just tool-case go-fmt multi-file
just tool-case cargo-clippy workspace-autofix
just tool-case cargo-fmt workspace-multi

# Sixteen behavior-rich contracts across twelve environments: jq data formats,
# Buf data formats, shared Node, dedicated Prettier, dedicated Contextlint,
# Python, Go, Rust, Cargo Clippy/Fmt, Ruby, security, and native macOS.
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

The separate Buf data-formats lane pins Buf 1.72.0 and the declared Apple
`/usr/bin/diff` prerequisite. Its isolated adapter rejects configurable flags,
proves every physical Proto file belongs to the effective module scope, locks
symlink-safe workspace formatting, and accepts status 100 only after parsing a
complete unified diff and removing its generated mtime fields. The
representative repairs selected and unselected in-scope files, records the
complete workspace diff, reruns the authoritative check, and proves a clean
idempotent repeat. Exact release/signature provenance and the config, syntax,
filesystem, and transaction limitations are in the
[Buf contract](docs/pinned-tool-environments.md#buf-format-validation-contract).

The Node lane also pins Astro 7.2.0, `@astrojs/check` 0.9.10, and TypeScript
6.0.3. Its evaluated adapter performs one error-only check for the whole
workspace, requires Astro's positive-file `Result` footer before accepting
status zero or one, and treats an incomplete or configuration-failed check as
an operational failure. The representative proves project scope with two clean
selected files and an unselected failing third file. Exact provenance,
integrities, repeat and no-mutation evidence, and the conservative attribution
and side-effect limitations are in the
[Astro contract](docs/pinned-tool-environments.md#astro-validation-contract).

The same Node lane pins Biome 2.5.7 and its macOS arm64 native package. An
isolated Python adapter locks Biome's JSON reporter and safe-fix controls,
requires a complete report for every selected file, distinguishes source
diagnostics from configuration failures, and runs an authoritative check after
each mutation. The representative proves one batched repair with exact changed
files, conservative candidate attribution, an untouched unselected sentinel,
and clean idempotent repeats. Provenance, integrity, command, mutation, and
security limitations are recorded in the
[Biome contract](docs/pinned-tool-environments.md#biome-validation-contract).

The dedicated Prettier lane pins Prettier 3.9.6 in a one-package npm graph and
runs it only through checksum-pinned Node 24.19.0 with bundled npm 11.17.0. Its
isolated adapter disables implicit configuration, ignores, plugins,
EditorConfig, cache, and pragma discovery; accepts only a narrow data-only JSON
configuration and option set; and requires a complete read-only
`--list-different` batch before any write. The four-case matrix proves clean,
dirty, configuration-failure, and mixed multi-file behavior on both immediate
and compatibility-deferred surfaces. Exact provenance, signal and descendant
cleanup, private-config handling, and the remaining file-replacement and late
partial-write limitations are in the
[Prettier contract](docs/pinned-tool-environments.md#prettier-validation-contract).

The dedicated Contextlint lane installs the exact `@contextlint/cli` and
`@contextlint/core` 1.1.1 pair in a separate locked npm graph and executes it
only through checksum-pinned Node 24.19.0. Its isolated adapter ignores config
include patterns for scope, inventories and snapshots every physical Markdown
file outside physical fixed-name exclusions, rejects Contextlint glob-magic
paths and every encountered symlink, copies the validated JSON config into a
private root, and requires an exact private SEC-001 completion probe before it
accepts project JSON. The four-case matrix proves clean, warning-only source,
cross-file/project, and operational-failure behavior on both immediate and
compatibility-deferred surfaces. Exact package integrities and tag provenance,
permission and trace bindings, signal/descendant cleanup, and the remaining
unwalked-subtree, workspace-topology/referenced-target replacement, and
escaped-process limitations are in the [Contextlint
contract](docs/pinned-tool-environments.md#contextlint-validation-contract).

The Go lane pins Go and gofmt 1.26.5. Its isolated adapter treats `gofmt -l`
stdout as the formatting signal while preserving native status-two failure
dominance, rejects link aliases and scope-changing arguments, and scrubs Go,
loader, and debug overrides. Every mutation is gated by a read-only batch
preflight; deferred writes additionally receive the explicit workflow's
authoritative final check, while immediate idempotence is proved by its clean
repeat. The representative selects one dirty and one clean file while
preserving an unselected dirty sentinel; the full matrix also proves a mixed
dirty-valid and parse-invalid batch cannot partially mutate. Exact archive/tag
provenance, command traces, environment controls, and filesystem limitations
are in the
[gofmt contract](docs/pinned-tool-environments.md#gofmt-validation-contract).

The dedicated Cargo Clippy lane pins the official Rust and Cargo 1.97.1
distribution with Clippy 0.1.97. Its isolated adapter runs frozen read-only
metadata and Clippy JSON checks, proves every physical workspace Rust source
was compiled through fresh dependency information, and applies only validated
non-overlapping `MachineApplicable` suggestions before the authoritative final
check. The representative repairs one selected and one unselected compiled
source while preserving a selected clean source, then proves exact workspace
diffs and clean idempotent repeats. The deliberately narrow single-package,
all-targets/all-features scope and execution/TOCTOU limitations are recorded in
the [Cargo Clippy contract](docs/pinned-tool-environments.md#cargo-clippy-validation-contract).

The same Rust 1.97.1 closure supplies cargo-fmt and rustfmt 1.9.0. The isolated
Cargo Fmt adapter performs locked metadata checks, then proves on a private
copy that `cargo fmt --all --check` reaches every physical Rust source before
it trusts a real check or mutation. The representative two-member workspace
repairs both a selected dirty source and an unselected dirty member source,
records that exact workspace diff, and proves the authoritative check and
idempotent repeat. A dedicated dormant `autobins = false` case proves the
adapter fails closed when Cargo Fmt omits a physical `.rs` file. Exact archive
and source provenance, eight-event child trace, status rules, and supported
workspace limitations are in the
[Cargo Fmt contract](docs/pinned-tool-environments.md#cargo-fmt-validation-contract).

The security lane reproducibly builds Betterleaks
`1.7.3+velvet-glove.1` from the checksum-pinned upstream v1.7.3 source and a
checksum-pinned dependency-closure patch with Go 1.26.5. Its adapter scans all
selected files in one batch, locks complete redaction and stable legacy output,
reserves status 10 for findings, removes ambient Betterleaks and Gitleaks
configuration variables, and accepts only non-controlled long-form configured
arguments. The source scan has no reachable package or symbol vulnerability
finding; the remaining `GO-2026-5932` result is a coarse module match whose
binary expansion names an OpenPGP package absent from the source dependency
graph. Exact source, patch, module-lock, build-artifact, command, and limitation
details are in the
[Betterleaks contract](docs/pinned-tool-environments.md#betterleaks-validation-contract).

The Ruby lane also pins the dependency-free Asciidoctor 2.0.26 gem by SHA-256.
Its checked command runs in safe mode through a small Ruby preflight adapter:
document warnings and nonfatal errors remain source issues, while Asciidoctor
usage and configuration failures are remapped to operational failures despite
the upstream CLI using status one for both. Exact provenance, batch behavior,
and the adapter and safe-mode limitations are in the
[Asciidoctor contract](docs/pinned-tool-environments.md#asciidoctor-validation-contract).

Run `scripts/regen-licenses.sh` after dependency changes. The generated
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) is checked in alongside
the dual [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) licenses.
