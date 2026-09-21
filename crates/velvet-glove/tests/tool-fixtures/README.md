# Tool fixture format

Each fixture exercises one builtin tool against a small on-disk example and
verifies the runner's harness-specific output. Fixtures are auto-discovered by
the `tool_fixtures.rs` integration test.

Fixture layout and size are bounded by the tripwires in
[`tests/guardrails.rs`](../guardrails.rs); see
[`docs/validation-architecture.md`](../../../../docs/validation-architecture.md)
for the budgets and the overall validation contract.

The ordinary test lane validates the complete inventory and sends canonical,
typed Claude, Codex, and Antigravity inputs through the real `velvet-glove`
binary to a probe executable. The probe asserts the program, exact argv, cwd,
sentinel environment, and one invocation per protocol surface. The ignored
real-tool lane keeps the full golden matrix on Claude and Codex; Antigravity's
native lowering is covered by the probe without duplicating the full catalog.

## Directory layout

```
tests/tool-fixtures/<tool-id>/<example-name>/
  example.<ext>                # the input file (required)
  <supporting files>           # optional sibling files copied as-is
  expected/<rel-path>          # optional: expected post-run content of <rel-path>
  claude.json                  # optional: expected stdout JSON for --claude
  claude.stderr.txt            # optional: expected stderr (literal, normalized)
  claude.exit                  # optional: expected exit code (default 0)
  codex.json codex.stderr.txt codex.exit
```

- `<tool-id>` matches the tool's `id` field in `crates/hookkit-pkl-config/src/builtins/tools/<tool>.pkl` (e.g. `ruff`, `cargo-fmt`).
- The harness picks the entry file (the one cited in the synthesized
  `PostToolUse` event) by looking for a top-level file whose name starts with
  `example.`. If none exists, the first non-golden, non-`expected/` file at
  the fixture root is used.
- If a case has multiple top-level `example.*` files, the harness cites all of
  them in one synthetic shell-tool event so invocation granularity and
  per-file attribution can be exercised.
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

## Normalization placeholders

When comparing outputs, the harness substitutes the test's temp workspace
path with the literal `<workspace>` in both actual and golden, so paths in
golden files should use `<workspace>` for anything under the test project.

The session id is fixed at `test-session`, so golden files can reference it
directly without normalization.

## Running the lanes

Pkl 0.31.1 is a required prerequisite for both lanes. Run the hermetic
inventory and probe gates with:

```sh
cargo test -p velvet-glove --test tool_fixtures
```

Run the host-tool compatibility matrix explicitly with:

```sh
cargo test -p velvet-glove --test tool_fixtures run_all_tool_fixtures -- --ignored --exact --nocapture
```

Discovery fails on a missing or empty root, zero tools, zero cases, filesystem
errors, empty tool directories, and fixture directories without an enabled
builtin owner. Missing host tool programs remain structured skips unless the
environment provides them. Set
`VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS=all` (or a comma-separated tool-id list)
to promote unavailable selected programs to failures. Unknown or fixture-less
tool ids are configuration errors rather than silent no-ops.

Those skips describe only the ignored host-tool compatibility lane; a skip
here never counts as validation evidence for a tool.

Set `VELVET_GLOVE_FIXTURE_TOOLS` to a comma-separated list of fixture tool IDs
to run a subset. Unset it to retain the full host-tool matrix. Empty lists,
unknown IDs, and IDs without fixtures fail. Selection happens after complete
inventory validation and before executable discovery: other installed tools
are reported as `not-selected` and never executed. Explicit required IDs must
belong to the selection; `VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS=all` requires
every selected tool. Report totals still cover the complete fixture inventory,
including `not-selected` surface cases; they are not a coverage claim for the
entire enabled catalog.

Every subprocess is bounded to 60 seconds. Override that positive whole-second
limit with `VELVET_GLOVE_FIXTURE_TIMEOUT_SECS`.

Every run prints versioned JSON after the
`VELVET_GLOVE_FIXTURE_JSON=` prefix, including tool, case, surface,
pass/skip/fail, structured skip-reason, and probe-command totals. To retain a
failed case's workspace, generated config, native input, stdout, stderr, exit
status, and outcome JSON, set `VELVET_GLOVE_FIXTURE_ARTIFACT_DIR` to a writable
directory. Probe and fixture-setup failures are retained there too. A complete
run report is written there as well; successful case workspaces are still
removed.

## Scheduled real-tool CI

The `Real-tool fixtures` workflow runs weekly on Mondays at 07:23 UTC and can
be run manually from GitHub Actions. PRs changing the lane's workflow,
installation/reporting configuration, run script, or harness also run it so
infrastructure changes can be verified before merge. Other PRs retain the
ordinary hermetic lane.

The initial Ubuntu/macOS selection is `cargo-fmt`, `cargo-clippy`, `actionlint`,
`jq`, and `go-fmt`. All five are required; there are no platform exceptions in
this initial selection. Other fixture tools have the explicit `not-selected`
reason while v2 validation rolls out. The twelve enabled tools without fixture
directories are also outside this lane's coverage. This lane currently proves
immediate behavior on Claude and Codex, not deferred real-tool execution or
repeated-run idempotence. Antigravity is covered by the hermetic protocol probe.

CI-only installation and selection live in
[`.github/real-tools/mise.toml`](../../../../.github/real-tools/mise.toml).
Tool versions float within the configured major/minor series; Pkl stays at the
runner's required 0.31.1. No mise lockfile is used and CI does not cache tool
installations, so each run resolves patches afresh. Rust build dependencies are
cached separately. These choices do not affect users' tool installations.

To reproduce the selected lane locally with [mise](https://mise.jdx.dev/):

```sh
mise trust .github/real-tools/mise.toml
mise -C .github/real-tools install
mise -C .github/real-tools exec -- bash ../../scripts/run-real-tool-fixtures.sh
```

The script prints its artifact directory in the harness output; set
`VELVET_GLOVE_FIXTURE_ARTIFACT_DIR` to an absolute directory to choose it.
Use a fresh directory for each run. CI retains versions, full logs, JSON
results, and failure workspaces (including hidden generated policy and
diagnostics) for 14 days. Its job summary separates selected results from tools
outside the selection. Missing required tools and fixture failures fail the
job; setup/build/probe failures remain failures even without a complete report.

To add a tool, validate its contract following
[`docs/validation-architecture.md`](../../../../docs/validation-architecture.md),
then add its normal installation/version series and fixture ID to the mise
configuration, and its version command to `scripts/run-real-tool-fixtures.sh`.
Verify both hosted platforms. If normal installation is unavailable on a
platform, document the skip reason in the workflow and summary rather than
adding bespoke provisioning. Update the initial coverage list above as it grows.
When a patch release breaks a fixture, investigate the spec or volatile golden
output and record any version-specific limitation; do not tighten binary pins.
