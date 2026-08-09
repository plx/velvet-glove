# Tool fixture format

Each fixture exercises one builtin tool against a small on-disk example and
verifies the runner's harness-specific output. Fixtures are auto-discovered by
the `tool_fixtures.rs` integration test.

Every `<tool-id>/<example-name>` directory is also declared in the catalog
[`manifest.json`](../../../hookkit-pkl-config/validation/manifest.json).
A nonignored manifest test rejects undeclared or missing fixture tools and
cases. Fixture presence is inventory only: by itself it does not count as
rendered-command or pinned-real-tool evidence in the generated validation
coverage report.

The ordinary test lane validates the complete inventory and sends canonical,
typed Claude, Codex, and Antigravity inputs through the real `velvet-glove`
binary to a probe executable. The probe asserts the program, exact argv, cwd,
sentinel environment, and one invocation per protocol surface. The ignored
real-tool lane keeps the full golden matrix on Claude and Codex; Antigravity's
native lowering is covered by the probe without duplicating the full catalog.

## Directory layout

```text
tests/tool-fixtures/<tool-id>/<example-name>/
  example.<ext>                # the input file (required)
  <supporting files>           # optional sibling files copied as-is
  expected/<rel-path>          # optional: expected post-run content of <rel-path>
  claude.json                  # optional: expected stdout JSON for --claude
  claude.stderr.txt            # optional: expected stderr (literal, normalized)
  claude.exit                  # optional: expected exit code (default 0)
  codex.json codex.stderr.txt codex.exit
```

- `<tool-id>` matches the tool's `id` field in
  `crates/hookkit-pkl-config/src/builtins/tools/<tool>.pkl` (for example,
  `ruff` or `cargo-fmt`).
- The harness picks the entry file (the one cited in the synthesized
  `PostToolUse` event) by first looking for a top-level file whose name starts
  with `example.`. This preserves the established multi-file fixtures. If no
  top-level marker exists, it recursively requires exactly one nested
  `example.*` file so project-shaped fixtures can use conventional source
  directories such as `src/pages/`. Ambiguous nested markers fail closed. If
  there is no marker at either depth, the first non-golden,
  non-`expected/` file at the fixture root is used.
- Every non-golden, non-`expected/` file (including subdirectory contents like
  `src/main.rs` or `Cargo.toml`) is copied into the test's temp workspace at
  the same relative path.

## Golden output files

`<harness>.json` is the expected normalized stdout. The harness parses both
golden and actual as JSON for structural comparison so whitespace and key order
don't matter.

`<harness>.stderr.txt` is the expected normalized stderr (literal trimmed
comparison). Absent means stderr is expected to be empty.

`<harness>.exit` is a single line with the expected exit code. Absent means 0.

If `<harness>.json` is missing, the harness expects the native no-op response
`{}`. This makes a newly supported structured response fail visibly until its
golden is reviewed.

## `expected/` post-state mirror

Any files inside `expected/<rel-path>` are compared against the post-run
content of `<rel-path>` in the temp workspace. Use this when the tool rewrites
a file (autofix, formatting) and you want to assert the result.

For retained mutating contracts, the mirror is also the exact changed-file
allowlist: every mirrored path must change, no unmirrored source path may
change, and the second immediate/deferred run must produce an empty diff. This
lets a multi-file fixture retain clean or intentionally dirty unselected
sentinels without weakening the complete-workspace-diff assertion.

The mirror may include files outside the event's candidate set when an
evaluated remedy declares `matching-globs` or `workspace` writes. The retained
deferred evidence must then report those snapshot-discovered files in
`changedFiles` and in the file-result union while preserving the original
candidate list. Remedies declared `target-files` remain restricted to event
candidates.

## Normalization placeholders

When comparing outputs, the harness substitutes the test's temp workspace
path with the literal `<workspace>` in both actual and golden, so paths in
golden files should use `<workspace>` for anything under the test project.

The session id is fixed at `test-session`, so golden files can reference it
directly without normalization.

When `NODE_PATH` supplies a controlled pinned package graph, its raw and
canonical roots are normalized to `<node_modules>`. This keeps operational
stack paths reproducible without hiding which package-relative module emitted a
diagnostic.

## Running the lanes

Pkl 0.31.1 is a required prerequisite for both lanes. Run the hermetic
inventory and probe gates with:

```sh
cargo test -p velvet-glove --test tool_fixtures
```

Run the host-tool compatibility matrix explicitly with:

```sh
cargo test -p velvet-glove --test tool_fixtures \
  run_all_tool_fixtures -- --ignored --exact --nocapture
```

Run one selected case in its pinned, controlled macOS environment with:

```sh
just tool-case jq multi-file-fragments
```

Run the pinned representative contracts across their controlled environments
with `just tool-representatives`. See the
[pinned environment guide](../../../../docs/pinned-tool-environments.md) for
versions, integrity locks, platform constraints, bootstrap steps, active network
denial, and evidence output.

Discovery fails on a missing or empty root, zero tools, zero cases, filesystem
errors, empty tool directories, and fixture directories without an enabled
builtin owner. Missing host tool programs remain structured skips until the
pinned provisioning lane supplies them. Set
`VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS=all` (or a comma-separated tool-id list)
to promote unavailable selected programs to failures. Unknown or fixture-less
tool ids are configuration errors rather than silent no-ops.

`VELVET_GLOVE_FIXTURE_SELECTION` accepts a comma-separated list of `tool-id` or
`tool-id/case-id` selectors. It rejects unknown and redundant selectors, filters
the discovered catalog before execution, and automatically makes every selected
tool required. The pinned driver sets this variable; direct use remains useful
when debugging an already controlled environment.

Those skips describe only the ignored host-tool compatibility lane. The
validation manifest records unmet supported-catalog requirements as explicit
gaps; a skip here never promotes a coverage tier to covered.

Every subprocess is bounded to 60 seconds. Override that positive whole-second
limit with `VELVET_GLOVE_FIXTURE_TIMEOUT_SECS`.

Every run prints versioned JSON after the
`VELVET_GLOVE_FIXTURE_JSON=` prefix, including tool, case, surface,
pass/skip/fail, structured skip-reason, and probe-command totals. To retain a
failed case's workspace, generated config, native input, stdout, stderr, exit
status, and outcome JSON, set `VELVET_GLOVE_FIXTURE_ARTIFACT_DIR` to a writable
directory. Probe and fixture-setup failures are retained there too. A complete
run report is written to the stable `report.json` path, with a timestamped copy
alongside it. Successful jq, Asciidoctor, Astro, Betterleaks, Biome, Buf
Format, and gofmt contract cases are retained too. Their evidence includes
exact pass-through program/argv/cwd/environment traces (including
Asciidoctor's nested FATAL preflight and WARNING validation, Astro's single
nested project check, and
Betterleaks' marker-delimited batch adapter with locked redaction and finding
status plus inherited-config scrubbing, and its distinct status-1 missing-config
failure with the adapter's production-canonicalized `<time> FTL` diagnostic,
plus Biome's isolated mode-and-files adapter, locked JSON-report suffix, and
fully scrubbed child control/log environment),
complete workspace snapshots and diffs for repeated immediate runs,
and two independent compatibility-deferred
summaries plus their semantic idempotence comparison. Biome mutating cases
retain independent pristine baselines for the immediate `fix` → `verify`
pipeline and compatibility-deferred `initial-check` → `remedy` →
`final-check` lifecycle. They bind exact post-remedy bytes and changed paths,
then prove either a verify-only clean fixed-state rerun or an unchanged full
rerun for persistent source issues. Astro traces additionally
bind `NODE_PATH` to the same controlled `node_modules` graph as the pinned
Astro executable, verify all three required package manifests, and record
disabled telemetry, non-interactive CI mode, and a cleared debug channel. Other
successful case workspaces are removed.

gofmt traces bind the pinned executable behind an isolated Python adapter and
record its fully scrubbed Go, loader, debug, locale, telemetry, and toolchain
environment. Verify commands contain one native `-l`; write commands must
trace a read-only `-l` preflight immediately before `-w`, while a status-two
preflight must never reach `-w`. Immediate runs and explicit deferred
`initial-check` → `remedy` → `final-check` attempts use independent pristine
baselines. The four cases bind clean stdout, stdout-signaled dirty paths,
failure dominance when a dirty filename precedes a parse diagnostic, exact
multi-file mutation, an untouched unselected sentinel, the deferred
authoritative final check, and semantic idempotence on both surfaces.

Buf Format traces bind its isolated workspace adapter, absolute managed Buf
executable, a `buf config ls-modules --log-format=text --format=json` scope
preflight before every native format attempt, fixed formatter flags, Apple
`diff` prerequisite, sanitized child `PATH`, scrubbed
`BUF_*`/`DIFF_OPTIONS`/`DEBUG`, and a fixture-private `BUF_CACHE_DIR`. Dirty
diagnostics must contain complete, sorted unified-diff blocks whose dynamic
header mtimes were replaced with `<mtime>`. The multi-file case selects two
candidates while also changing a workspace file outside that candidate set, so
retained reports prove workspace write scope, conservative workspace attribution,
and the full candidate-plus-changed file union.

Cargo Clippy traces bind two distinct launchers from one case-only Rust 1.97.1
toolchain: `cargo metadata` preflights the selected `Cargo.toml`, then the
paired `cargo-clippy` performs a read-only, frozen JSON coverage probe with
lint levels capped, followed by the authoritative `-Dwarnings` check. Fix and
verify modes use those same native probes; mutation is adapter-internal. The
adapter runs all children from a private target
directory, preserves only a config-free controlled `CARGO_HOME`, binds the
paired `cargo`/`rustc`/`rustdoc` executables, supplies an empty private Clippy
configuration when the workspace has none, and scrubs Cargo, Rust, Clippy,
cache-wrapper, loader, and debug overrides. The four-case matrix distinguishes
clean completion, a persistent non-machine-applicable source lint, a semantic
Clippy configuration failure, and a workspace-scoped autofix. The autofix case
selects one dirty and one clean source while also repairing an unselected
compiled source; its hostile `.cargo/config.toml` proves that ambient
`rustflags` and forced tool/config variables cannot bypass the isolated check.
The expected mirror is the complete two-file change allowlist, and the normal
mutating lifecycle proves authoritative post-remedy verification plus
immediate and compatibility-deferred idempotence.

Cargo Fmt traces bind Cargo, cargo-fmt, rustfmt, and rustc from the same pinned
Rust 1.97.1 component-set-qualified root. Each completed invocation records
the adapter's root metadata and coverage-copy metadata/format preflights, plus
cargo-fmt's nested metadata and rustfmt children. The five-case matrix covers
clean and dirty packages, invalid rustfmt configuration, a fail-closed dormant
`autobins = false` source, and workspace-wide multi-member mutation. The
evaluated lifecycle probe additionally binds process-group cleanup, bounded
output, alias and extra-argument rejection, byte-plus-mode and mtime-only
mutation rejection, retained-directory add/remove/mode rejection, normalized
unwritable-TMPDIR diagnostics, deterministic initialization and post-cleanup
signal cutoffs, and private-root cleanup under repeated TERM. It proves exact
file content/mode/mtime plus retained-directory topology/mode rollback and
deterministic reporting when that best-effort rollback fails. Every normally
completed child is checked for and sweeps remaining same-process-group
descendants; a closed-stdio delayed-mutation orphan is rejected, while a child
that deliberately escapes into a new session or process group remains outside
the adapter's containment guarantee. File and directory inode identities and
directory mtimes are not rollback fields, and subtrees named `.git`,
`.velvet-glove`, `node_modules`, or `target` remain deliberately outside the
retained topology and file snapshot scope.
