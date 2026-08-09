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

Run one selected case in its pinned, controlled macOS environment with:

```sh
just tool-case black unformatted
```

Run all six pinned representative environments with `just
tool-representatives`. See the
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
run report is written there as well; successful case workspaces are still
removed.
