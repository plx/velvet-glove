# Tool fixture format

Each fixture case runs one builtin tool against a small on-disk example and
checks what a correct spec does with it: whether the files the synthetic edit
cites (and any other files the tool changes or blames) end up clean,
auto-fixed, or needing manual fixes, or whether the tool failed operationally.
The file contents after the run are checked too. `tool_fixtures.rs`
auto-discovers every case.

Cases assert **semantics, not bytes**. They never compare stdout or stderr
transcripts. Message wording belongs to the UX templates, and tool output
varies between tool versions. Budgets and the overall contract are defined in
[`docs/validation-architecture.md`](../../../../docs/validation-architecture.md)
and enforced by [`tests/guardrails.rs`](../guardrails.rs).

## Directory layout

```
tests/tool-fixtures/<tool-id>/<case-name>/
  case.json                    # required: the expected outcome (below)
  example.<ext>                # input file(s) the synthetic tool call cites by default
  <supporting files>           # optional files copied as-is (configs, manifests)
  expected/<rel-path>          # optional: expected post-run content of <rel-path>
```

- `<tool-id>` is the tool's `id` in
  `crates/hookkit-pkl-config/src/builtins/tools/<tool>.pkl`, for example
  `ruff` or `cargo-fmt`.
- **Cited files** are the files the synthetic tool call says the agent
  edited. By default that is every top-level `example.*` file, or, with no
  `example.*`, the first top-level input file (for example `Dockerfile`).
  `cite` in `case.json` replaces the default with an explicit list, which may
  name nested inputs such as `member/src/lib.rs`. With several cited files,
  one Bash call writes them all, which exercises invocation granularity and
  per-file attribution.
- **Copied files.** Everything except `case.json`, a case `README.md` and
  `expected/` is copied into a temporary workspace at the same relative path.
  That includes subdirectories and a `.velvet-glove/post-tool-use.local.pkl`
  overlay, which is how `operational-failure` cases break a tool.
- Legacy byte goldens (`claude.json`, `codex.stderr.txt`, `*.exit`, …) are
  rejected during discovery.

## `case.json`

```json
{ "outcome": "manual" }
```

| Key | Meaning |
| --- | --- |
| `outcome` | Required. The aggregate (worst) result: `clean`, `auto-fixed`, `manual` or `operational`. It covers the cited files plus any other file the run reports changed or blamed (see the deferred lane below). |
| `cite` | Optional. The case-relative input files the synthetic tool call cites, replacing the default `example.*` citation. |
| `files` | Optional, for mixed or workspace cases. Maps a file to its exact outcome (`clean`, `auto-fixed` or `manual`). A cited file may take any of the three. A non-cited file must be `auto-fixed` or `manual`, because it is reported only when the tool changes or blames it. |
| `immediate`, `deferred` | Optional booleans, default `true`. Set one to `false` to skip that lane for the case; a `note` is then required. |
| `note` | A free-text explanation of the case. It becomes the reason shown for a skipped lane. |

For example, a workspace tool that rewrites a sibling crate while the edited
file is already clean:

```json
{"outcome": "auto-fixed", "files": {"example.rs": "clean", "member/src/lib.rs": "auto-fixed"}}
```

Discovery rejects:
- unknown keys;
- a `cite` that is empty, repeats a path, or names anything other than an
  input file (including `expected/` post-state and `case.json`);
- `files` entries that are not input files, or that expect a non-cited file
  to be `clean`;
- per-file outcomes worse than the aggregate;
- a `files` map that names every cited file but whose worst entry differs
  from `outcome`, so name the non-cited files that set the aggregate too;
- per-file entries in an `operational` case.

## What each lane asserts

Each case runs on three surfaces, in this order:

**`deferred-claude`** (primary) is the flow the shipped plugin uses:
`session-start-state`, then one native Claude `PostToolUse` citing the example
files through `post-tool`, then `turn-completion` (Stop). The case gets its own
`--state-dir`. The harness reads the run's `summary.json` and checks:
- every hook exits 0;
- for `operational`: at least one operational problem, and no cited file
  classified `manual-fixes-needed`;
- otherwise: no operational problems, every cited file assessed, and each
  `files` entry matched exactly. The worst status must equal `outcome`. That
  aggregate is taken over the cited files plus every non-cited file that
  `summary.json` reports `auto-fixed` or `manual-fixes-needed`. A workspace
  tool that rewrites or blames another file therefore counts. An issue that
  exists only in a file the agent did not touch is out of scope: the runtime
  does not blame it, and it does not block;
- Stop output is checked only for its block decision: `manual` must block
  (`decision=block`), and `clean` and `auto-fixed` must not.
- Loop guard: after a block, a second Stop with `stop_hook_active: true`
  and no edits in between (the agent's unsuccessful retry) must not block
  again.

The generated config disables the filesystem-mtime fallback, so the only
candidates are the files the tool call cited.

**`immediate-claude` and `immediate-codex`** (secondary) run
`post-tool-immediate`. They check that the hook exits 0 and that stdout has
the right shape: `clean` must say nothing to the agent (no
`hookSpecificOutput`, `decision` or `reason`; a user-only `systemMessage`,
such as the note about issues only in untouched files, is allowed),
`auto-fixed` and `manual` must produce something other than `{}`, and
`operational` is not checked. Both runners treat issues that exist only in
files the call did not change as out of scope, so
`cargo-clippy/untouched-file-issue` runs on every lane.

**Post-state (all lanes).** When `expected/` exists, each file in it must
match the workspace file after the run. Without `expected/`, every input must
be unchanged, unless the case is `auto-fixed`.

## Capturing `expected/`

To seed missing post-state for review, set
`VELVET_GLOVE_FIXTURE_CAPTURE_EXPECTED=1`. After a run that passes
semantically, an `auto-fixed` or `manual` (partial-fix) case with no
`expected/` gets every changed input copied into `expected/`. Deferred runs
first, so the capture reflects the plugin's flow. The run prints `CAPTURED …`
for each case. Review the diff before committing.

## Running the lanes

Pkl 0.31.1 or newer is required by both lanes.

**Hermetic gates.** This checks the inventory and `case.json` validity. It
also sends a probe through the real binary on every protocol surface:
immediate on Claude, Codex and Antigravity, and deferred on Claude.

```sh
cargo test -p velvet-glove --test tool_fixtures
```

**Real tools.** This runs the selected cases against the tools on your
`PATH`, for example a subset of tools you have installed:

```sh
VELVET_GLOVE_FIXTURE_TOOLS=ruff,shellcheck,jq \
  cargo test -p velvet-glove --test tool_fixtures run_all_tool_fixtures -- --ignored --exact --nocapture
```

Environment knobs:

- `VELVET_GLOVE_FIXTURE_TOOLS`: a comma-separated list of fixture tool IDs.
  Unset runs every tool. Empty lists, unknown IDs and IDs without fixtures
  fail. Selection happens after full inventory validation and before
  executable discovery, and unselected tools are reported as `not-selected`
  and never executed.
- `VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS`: `all`, or a list of IDs, whose
  missing programs become failures instead of `executable-unavailable` skips.
  Every required ID must also be selected.
- `VELVET_GLOVE_FIXTURE_TIMEOUT_SECS`: the per-subprocess limit, a positive
  whole number of seconds. The default is 60.
- `VELVET_GLOVE_FIXTURE_ARTIFACT_DIR`: a directory for the run report and for
  failed cases. Each failed case keeps its workspace, generated config, deferred
  state (including `summary.json` and logs), and per-hook input, stdout,
  stderr and exit status. Probe and setup failures are kept too. Workspaces
  from passing cases are removed.
- `VELVET_GLOVE_FIXTURE_CAPTURE_EXPECTED=1`: see above.

A skip is never validation evidence for a tool.

### Report

Each run prints versioned JSON after the `VELVET_GLOVE_FIXTURE_JSON=` prefix
(`formatVersion` 2):
- `surfaces` lists the surface names.
- `totals` reconciles the planned count (cases × surfaces) with the passed,
  skipped and failed counts.
- `bySurface` and `skipReasons` break those counts down.
- Each entry in `outcomes` records the `tool`, `case`, `surface`, `lane`,
  `protocol`, `expected` outcome, `status`, structured `reason`, and any
  retained `artifacts`.

Totals cover the whole fixture inventory, including `not-selected` cases. They
are not a coverage claim for the enabled catalog.

## Scheduled real-tool CI

The `Real-tool fixtures` workflow runs weekly on Mondays at 07:23 UTC, and you
can also start it manually from GitHub Actions. PRs that change the lane's
workflow, installation or reporting configuration, run script, harness,
fixture cases (`tests/tool-fixtures/**`), or builtin tool specs
(`crates/hookkit-pkl-config/src/builtins/tools/**`) also run it, so those
changes are verified before merge; it only runs the CI-selected tools, so
this stays cheap. Other PRs run only the ordinary hermetic lane.

The Ubuntu and macOS selection is every fixture tool (computed by the
workflow, not hand-maintained). Installation splits into two tiers:

- **Core** ([`.github/real-tools/mise.toml`](../../../../.github/real-tools/mise.toml)):
  a small set of tools with simple, prebuilt aqua/mise-registry binaries (or
  that are subcommands of a toolchain already declared there), stable enough
  to require. Core tools are listed in that file's
  `VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS`; a broken install or a fixture
  failure there fails the job.
- **Broad** ([`.github/real-tools/broad/mise.toml`](../../../../.github/real-tools/broad/mise.toml)):
  everything else that has a reachable mise backend — npm, pipx, gem, cargo
  and go-install backends, extra language runtimes (Node, Python, Ruby,
  Java, Erlang/Elixir, Deno, Zig, Dart, Gleam, Cue), and `hk` for the
  hk-util fixture tools. It installs with `continue-on-error` and is never
  required, so one flaky or unavailable install there becomes a structured
  skip for that tool's fixture instead of failing the job.

A few fixture tools need no installation at all: `detect-private-key`,
`check-merge-conflict` and `python-debug-statements` shell out to `grep`,
and `mise` exercises the mise binary the lane already bootstraps.

Documented platform skips (see the workflow for the exact mechanism): both
`swiftlint` and `swift-format` only run on macOS (`swiftlint` has no Linux
release; `swift-format` needs the Xcode/Swift toolchain macos-latest
preinstalls, absent on ubuntu-latest); `alejandra` and `luacheck` only run
on Linux (their release assets are Linux-only); `xmllint` needs
`libxml2-utils` from apt on Linux but ships with the OS on macOS; `hlint`'s
macOS leg runs an x86_64 asset under Rosetta 2 (no arm64 release), so the
workflow pre-installs Rosetta 2 on macOS runners.

A handful of fixture tools have no reachable mise/aqua/npm/pipx/gem/cargo/
go-install backend at all and stay permanently unprovisioned, reporting
`executable-unavailable`: `nil` and `nixf-diagnose` (Nix-flake-only
distribution), `nixfmt` (no crates.io package and no per-platform release
asset) and `php-cs` (no aqua-registry entry; upstream needs a PHP toolchain
this lane doesn't otherwise provision). See
[`.github/real-tools/broad/mise.toml`](../../../../.github/real-tools/broad/mise.toml)
for the full reasoning on each.

Every selected case runs on the deferred and immediate lanes. The lane does
not yet test repeated-run idempotence. Antigravity is covered only by the
hermetic protocol probe.

Tool versions float within their configured major/minor series (or `latest`
in the broad tier). Pkl stays at the runner's required 0.31.1. There is no
mise lockfile and CI does not cache tool installations, so each run resolves
versions afresh. Rust build dependencies are cached separately. None of this
affects users' tool installations.

To reproduce the selected lane locally with [mise](https://mise.jdx.dev/):

```sh
mise trust .github/real-tools/mise.toml .github/real-tools/broad/mise.toml
mise -C .github/real-tools install
mise -C .github/real-tools/broad install   # best-effort; some tools may fail here
eval "$(mise -C .github/real-tools env -s bash)"
eval "$(mise -C .github/real-tools/broad env -s bash)"
bash scripts/run-real-tool-fixtures.sh
```

The script prints its artifact directory. To choose it, set
`VELVET_GLOVE_FIXTURE_ARTIFACT_DIR` to an absolute directory, and use a fresh
directory for each run. CI keeps versions (from `mise ls` on both tiers),
full logs, JSON results and failure workspaces (including hidden generated
policy and state) for 14 days. The job summary shows per-surface counts for
each tool and lists failure reasons. Missing required tools and fixture
failures fail the job. Setup, build and probe failures stay failures even
when there is no complete report.

To add a tool:
1. Validate its contract following
   [`docs/validation-architecture.md`](../../../../docs/validation-architecture.md).
2. Add its normal installation and version series, and its fixture ID, to
   the core or broad mise configuration (core only if it is a small,
   reliable, prebuilt-binary install; broad otherwise).
3. Verify it on both hosted platforms.
4. Promote it to core's `VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS` only once it
   is proven stable in the broad tier across both platforms.

If normal installation is unavailable on a platform, document the skip reason
as a comment in the mise configuration (and in the platform-skip list above)
rather than adding bespoke provisioning. When a patch release breaks a case,
check the spec or the case's outcome and record any version-specific
limitation; do not tighten binary pins.
