# Tool validation architecture

This document defines how builtin tool specs are validated, the budgets that
keep that validation proportionate, and the change policy for both. It is the
authoritative reference for anyone — human or agent — working on builtin tool
contracts. Read it before touching `crates/hookkit-pkl-config/src/builtins/`
or `crates/velvet-glove/tests/tool-fixtures/`.

## Product invariants

Velvet Glove runs the formatters and linters *the user already has installed*,
autofixes quietly, and reports residual issues tersely to the agent (verbosely
to the user). Everything in this document serves that product. In particular:

- A builtin spec describes how to invoke the user's tool. It never bundles,
  wraps, verifies, or replaces that tool.
- Users run whatever tool versions they have. Specs must be written against
  each tool's stable documented interface, not a pinned build.
- Validation exists to prove *our spec* is correct, not to prove the *user's
  binary* is authentic.

## Background: the v1 attempt

The first pass at tool validation (milestone "Tool validation v1", archived at
git tag `archive/tool-validation-v1`) escalated into per-tool hermetic Python
adapters embedded inside the shipped specs, binary hash pinning, patched
upstream forks, and a provisioning lane that only ran on one exact host
configuration. It was abandoned deliberately: it validated the wrong thing
(binary identity instead of spec correctness), leaked validation machinery
into runtime behavior, and made per-tool cost explode. The salvage protocol
below explains how to reuse the good parts. Do not reintroduce the pattern.

## The two-tier validation contract

### Tier 1 — hermetic, every PR, hosted runners

Runs in ordinary CI on `ubuntu-latest` and `macos-latest` with no real tools
installed (only `pkl`). It proves the spec is well-formed and that the runner
does the right thing with it:

1. **Schema and catalog validation.** The evaluated Pkl catalog passes
   `validate_builtin_catalog` and the structural tests in
   `crates/hookkit-pkl-config/tests/builtins.rs`.
2. **Rendered-command assertions.** For each tool: golden assertions on the
   exact argv rendered from the spec (see the existing per-tool tests in
   `builtins.rs`).
3. **Harness protocol tests.** The fixture harness sends typed native inputs
   through the real binary to a probe executable and asserts program, argv,
   cwd, and environment (`crates/velvet-glove/tests/tool_fixtures.rs`).

Tier 1 is mandatory for every enabled tool. It requires no network and no
tool binaries, so it can never flake on tool availability.

### Tier 2 — real tools, scheduled lane

A scheduled (nightly/weekly) workflow on hosted runners installs real tools
with **loose pins** (a floating patch level within a tested major/minor, via
mise / npm / pip / cargo / gem) and runs the fixture cases against them:

- Assert **semantics**, not bytes: issue detected vs. clean, operational
  failure not misclassified as a source issue, files mutated to expected
  content, autofix idempotent where the tool promises it.
- Golden transcripts may normalize volatile output (versions, paths, timing).
- A tool that cannot be provisioned on hosted runners gets a **documented
  skip with a reason** in the workflow, not a bespoke environment. Skips are
  visible in the lane's summary; they are allowed and honest.
- When a tool's new release breaks a fixture, that is the lane doing its job:
  fix the spec or record a version-specific note — do not pin harder.

### Fixture case vocabulary

Each tool's fixtures live in
`crates/velvet-glove/tests/tool-fixtures/<tool-id>/<case>/`. A typical tool
needs three or four cases:

- `clean` — well-formed input, expect a clean pass.
- one representative issue case (name it after the issue, e.g. `unformatted`,
  `missing-import`, `type-issue`) — expect issue classification; for fixers,
  include `expected/` post-state.
- `operational-failure` — bad config / unparseable input, expect operational
  classification (never misread as a source issue).
- optionally `multi-file` — when batch/workspace behavior is materially
  different from single-file.

More cases are allowed when a tool genuinely has more distinct behaviors, but
the guardrail caps below still apply.

## Per-tool budgets

These are enforced mechanically by `crates/hookkit-pkl-config/tests/guardrails.rs`
and `crates/velvet-glove/tests/guardrails.rs`:

| Budget | Limit |
| --- | --- |
| Spec size (`tools/<tool>.pkl`) | ≤ 200 lines |
| Multiline string literals in specs | none |
| Hex literals ≥ 40 chars (hash pinning) in specs | none |
| `program` override | spec executable, or a reviewed allowlist entry |
| Argv literal token | ≤ 500 chars, no newlines |
| Fixture cases per tool | ≤ 12 |
| Files per fixture case | ≤ 24 |
| Bytes per fixture case | ≤ 128 KiB |
| Bytes per fixture file | ≤ 64 KiB |
| Symlinks in fixtures | none |

As guidance (not mechanically enforced): a per-tool validation PR should land
in roughly **300 changed lines**. If a tool seems to need much more, stop and
open an issue describing why before writing the code.

## Anti-goals

These are hard "must nots". Each one was violated by the v1 attempt and each
violation had real costs:

- **No wrapper programs.** A spec's commands invoke the tool itself (or a
  reviewed, one-line shell shim like yq's diff pipeline). Never an embedded
  Python/Node/shell *program* that reimplements or guards the tool.
- **No version or hash pinning in shipped specs.** No SHA-256s, no exact
  version preflights, no build-metadata checks. `installHint` names the tool
  and its mainstream install command, nothing more.
- **No new runtime dependencies.** Validating a tool must not make the shipped
  product depend on anything new (interpreters, checksum chains, forks).
- **No forked or patched upstream tools.** If an upstream tool can't be
  validated as released, document the limitation and move on.
- **No host-locked validation.** Everything must run on GitHub-hosted runners.
  A lane that requires a specific machine is not CI.
- **No spec changes without a demonstrated bug.** Validation work may only
  change a runtime spec to fix a behavior a fixture demonstrates. The fixture
  proves the bug; the diff fixes it; nothing else moves.
- **No skip-elimination crusades.** An explained skip is an acceptable
  steady state. "No unexplained skips" must never become "no skips".

## Archetypes

Five hand-reviewed worked examples cover the recurring tool shapes. When
validating a new tool, find its archetype and match that example's structure
and scale:

| Archetype | Example tool | Shape |
| --- | --- | --- |
| Batch formatter | `cargo-fmt` / `rustfmt` | check via diff/`--check`, fix via write flag |
| Workspace linter with autofix | `cargo-clippy` | workspace-scoped check, `--fix` remedy |
| Plain checker | `actionlint` | no remedy, exit-code + diagnostics classification |
| Per-file checker | `jq` | one invocation per file |
| Stdout-diff formatter | `gofmt` | `-l`/diff on stdout signals issues at exit 0 |

## Salvage protocol

The v1 attempt is preserved at `archive/tool-validation-v1`. When redoing a
tool that v1 covered, mine it — don't rediscover:

```sh
git show archive/tool-validation-v1:crates/hookkit-pkl-config/src/builtins/tools/<tool>.pkl
git ls-tree -r --name-only archive/tool-validation-v1 crates/velvet-glove/tests/tool-fixtures/<tool-id>/
```

Worth extracting:

- **Fixture inputs and expected outputs** (clean / issue / operational-failure
  / multi-file cases) — usually reusable nearly verbatim.
- **Semantic findings** — exit-code meanings, stdout-vs-stderr signaling,
  partial-batch mutation behavior, check-scope/invalidation subtleties. Encode
  these in the spec's declarative fields and in fixture assertions.
- **Upstream provenance notes** from the v1 PR descriptions (versions tested,
  known quirks).

Never port: the embedded adapter programs, hash/version pinning, provisioning
recipes, or golden transcripts that assert adapter-specific output.

## Guardrails and change policy

The mechanical budgets live in two test files:

- `crates/hookkit-pkl-config/tests/guardrails.rs` — spec-side tripwires
- `crates/velvet-glove/tests/guardrails.rs` — fixture-side tripwires

These files, this document, the issue generator
(`scripts/generate-tool-validation-issues.sh`), and the guardrail CI check
(`.github/workflows/guardrail-check.yml`) are **guarded paths**: a PR that
modifies any of them fails CI unless a human has applied the
`guardrail-change` label.

If a tripwire fails on work you're doing, the default assumption is that the
work is over budget — not that the budget is wrong. **Stop, leave the
guardrail alone, and open an issue** explaining what you were doing and why it
doesn't fit. Raising a limit, restructuring code to evade a check, weakening
an assertion, or relabeling work to dodge a guarded path all require explicit
human sign-off via the label. This policy exists because the v1 attempt showed
how easily autonomous work ratchets its own scope; the guardrails are the
ratchet's pawl.
