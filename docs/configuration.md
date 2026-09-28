# Configuration reference

Velvet Glove evaluates policy with Pkl 0.31.1 or newer. A policy amends the
embedded schema (`Config.pkl`) and imports the built-in catalog
(`Builtins.pkl`); both names resolve because Velvet Glove stages them next to
your file before evaluating it.

**Nothing runs until a policy names tools in `run`.** With no policy file, or
with an empty `run`, every hook is a silent no-op. A minimal policy:

```pkl
amends "Config.pkl"
import "Builtins.pkl"

tools {
  ["ruff"] = Builtins.ruff
  ["cargoFmt"] = Builtins.cargoFmt
}

run { "ruff"; "cargoFmt" }
```

- `tools` keys are your names for tools; `run` lists those keys in execution
  order. Every `run` entry must name a `tools` key.
- Built-ins are properties of `Builtins`, named after the module file in
  lower camel case: `tools/cargo_fmt.pkl` is `Builtins.cargoFmt`,
  `tools/ruff.pkl` is `Builtins.ruff`. The tool's `id` (`cargo-fmt`) is what
  messages and artifact names show.
- The [built-in workflow audit](builtin-deferred-workflow-audit.md) lists
  every built-in with its commands; the specs themselves live in
  [`crates/hookkit-pkl-config/src/builtins/tools/`](../crates/hookkit-pkl-config/src/builtins/tools/).

Enabled tools named in `run` are validated when the policy loads: unknown
`run` entries, `phaseOrder`/`workflowOrder` entries that name nothing, an
invalid glob in a tool's `files` or in `settings.exclude`, an exit code in two
classes, a verify phase or workflow check that declares writes, and a mutating
phase or remedy without a write scope are all rejected with an itemised error.
A tool whose `workflows` are all disabled is valid and runs only in immediate
mode. The hooks report a load error to the user only, naming the policy file
that failed: no tool runs, and neither the tool call nor Stop is ever blocked
(`deferredReporting.blockOnOperationalErrors` cannot apply, because it is read
from the policy that failed). `doctor` and `check` print it and exit nonzero.

## Discovery and merge order

When `--config PATH` is present, Velvet Glove loads only that file and anchors
relative project behavior on the event workspace (for `doctor` and `check`,
on `--dir`). Otherwise it merges:

1. the home policy, `~/.velvet-glove/post-tool-use.pkl`;
2. project policies, `<dir>/.velvet-glove/post-tool-use.pkl`, for every
   directory from the filesystem root down to the event workspace; and
3. local policies, `<dir>/.velvet-glove/post-tool-use.local.pkl`, in the same
   root-to-leaf order. Keep these out of version control.

The walk skips your home directory itself, so the home policy is loaded once
and never makes `$HOME` look like a project. The **project root** (which
relative globs, `ProjectRoot`, and relative `diagnosticsDirectory` values
resolve against) is the nearest directory holding a project or local policy,
or the event workspace when there is none.

Each layer reads the legacy `.agent-hook-kit` namespace first, then the
canonical `.velvet-glove` namespace, so canonical peers win within a layer.

Later layers patch earlier ones:

| Section | How a later layer applies |
| --- | --- |
| `settings.*` | Field by field: a field you set replaces the inherited value. |
| `settings.exclude` | Appends to the inherited list, which starts from the defaults. |
| `settings.deferredReporting` | Field by field, down to each template. |
| `tools` | Whole entries by key: a later `["ruff"]` replaces an earlier one. |
| `run` | Replaced when non-empty; an empty `run` inherits. |

A layer can discard inherited state first with `merge`:

- `merge { resetAll = true }`: start from defaults;
- `merge { reset { "settings"; "tools"; "run" } }`: reset selected sections;
- `merge { resetTools { "ruff" } }`: drop named tools;
- `merge { resetExclude = true }`: drop inherited and default excludes, so
  this layer's `settings.exclude` replaces the list;
- `merge { resetDeferredReporting = true }`: restore reporting defaults.

## Runner settings

| Field | Default | Purpose |
| --- | --- | --- |
| `settings.jobs` | `0` | Concurrent jobs (one per workspace or file). `0` is auto: available parallelism, capped at 8. `1` runs serially. In immediate mode, jobs within one tool run concurrently and tools run one after another; at Stop, the checks of every tool share one pool, while remedies run one at a time in `run` order. |
| `settings.commandTimeoutSeconds` | `120` | Wall-clock limit per external command. A command that exceeds it is killed (on Unix, with its whole process group) and reported as an operational failure. `0` disables the limit. |
| `settings.localBinDirs` | `node_modules/.bin`, `.venv/bin` | Project-local executable directories searched before `PATH`; see [Executable resolution](#executable-resolution). A layer that sets it replaces the list. |
| `settings.exclude` | `**/<dir>/**` for `.git`, `node_modules`, `.venv`, `__pycache__`, `target`, and the tool caches below | Global exclusions, matched against project-relative paths before tool filters. Additions append; see `merge.resetExclude`. |
| `settings.failFast` | `true` | In immediate mode, stop scheduling later tools after an operational failure. At Stop, skip only the failing workflow's later remedies; the tool's other workflows and other tools still run. |
| `settings.continueAfterIssues` | `true` | Continue with later tools after source issues (immediate mode). |
| `settings.missingToolPolicy` | `user-notice` | Missing executable: `user-notice`, `hard-failure` (the hook fails), or `harness-block`. Applies to both hooks. |
| `settings.diagnosticsDirectory` | unset | Immediate-mode full diagnostics. Unset keeps them outside the project, in `$TMPDIR/velvet-glove/state/post-tool-immediate`; a relative path resolves from the project root. |
| `settings.loweringPolicy` | `best-effort-with-warnings` | Messages a native event cannot represent: `strict`, `best-effort`, or warning mode. |
| `settings.fileActivity.filesystemMtime` | `true` | At Stop, also treat files modified since the last Stop as candidates. |
| `settings.fileActivity.vcs` | `disabled` | Optional broad `git-dirty` fallback. |
| `settings.fileActivity.maxEntries` | `100000` | Bound recursive workspace expansion. |
| `settings.fileActivity.ignoredDirectoryNames` | `.context`, `.git`, `.hg`, `.svn`, `node_modules`, `target`, `.venv`, `__pycache__`, and the tool caches below | Directory names the mtime scan and workspace expansion never enter. Setting it replaces the list. |
| `settings.fileActivity.coverageGapPolicy` | `best-effort` | Record incomplete evidence silently, or use `strict` to block. |

The default excludes cover version-control internals (`.git`), installed
JavaScript dependencies (`node_modules`), the conventional Python virtual
environment (`.venv`), Python bytecode caches (`__pycache__`), and Cargo or
Maven build output (`target`). They are unanchored, so nested copies such as
`web/node_modules/` are excluded too.

Both lists also cover tool caches and build output that tools regenerate
and nobody edits: `.ruff_cache`, `.mypy_cache`, and `.pytest_cache` (rewritten
on every Ruff, mypy, or pytest run, so an unpruned scan finds "changed" files
there after every Stop), `.tox` and `.nox` (whole virtual environments),
`.gradle` (Gradle's project cache; `build.gradle*` and `gradle/` stay
included), `.build` (SwiftPM build products and dependency checkouts), `.next`
(Next.js output), and `.turbo` (Turborepo cache). Inside a Git repository,
Git-ignored files are dropped anyway; the lists matter outside Git and for
scan cost. An edit the agent makes inside one of these directories is still
observed, but no tool selects it.

## Tool specs

A tool spec (`ToolSpec` in `Config.pkl`) describes how to run one external
tool. Override a built-in by amending it, `(Builtins.ruff) { ... }`, or write
one from scratch with `new ToolSpec { ... }`.

| Field | Purpose |
| --- | --- |
| `id`, `displayName` | Identifier used in artifacts; name used in messages. |
| `executable`, `installHint` | Program to run and the hint shown when it is missing. |
| `files.include`, `files.exclude` | Globs over project-relative paths; empty `include` selects every file. |
| `workspaceIndicator` | Marker file (e.g. `Cargo.toml`); files are grouped by their nearest marker, up to the project root. Files without one are skipped. |
| `phases`, `phaseOrder`, `phaseInvocation` | Commands for the immediate PostToolUse hook, in order. Unlisted phases run after listed ones, by mode (`format`, `fix`, `verify`, `check-only`), then name. |
| `workflows`, `workflowOrder` | Check/remedy pairs for the deferred Stop hook. Without `workflows`, the deferred hook translates `phases`: each mutating phase becomes a remedy checked by the last verify phase. |
| `extraArgs` | Arguments added to **every** command's `ExtraArgs` token. |
| `env` | Environment variables for every command, e.g. `env { ["RUFF_CACHE_DIR"] = "/tmp/ruff" }`. |
| `timeoutSeconds` | Per-command timeout for this tool; overrides `settings.commandTimeoutSeconds` (`0` disables it). |
| `messages` | MiniJinja templates for immediate-mode agent feedback. |
| `enabled` | `false` skips the tool even when it is in `run`. |

A `Phase` has `mode`, `argv`, optional `program`, `exitCodes`, `writes`,
`enabled`, and `extraArgs`. A `Workflow` has a read-only `check`, an optional
`remedy` (both `WorkflowCommand`s with the same command fields plus
`issuesOnStdout`), `checkScope`, `invocation`, `enabled`, and `extraArgs`.

`exitCodes` classifies each exit status as `clean` (default `0`), `issues`,
or `failure`; anything else follows `unexpected` (default `failure`). Source
issues are for the agent; failures are operational and go to the user.
`writes` (`none`, `target-files`, `matching-globs`, `workspace`) tells the
runner which files to snapshot so it can report what a command changed; every
mutating phase and remedy needs one, and every check must be `none`.

### Argument tokens

`argv` mixes literal strings with tokens, expanded per job without a shell:

| Token | Expands to |
| --- | --- |
| `new Files {}` | Absolute paths of the job's files. |
| `new WorkspaceFiles {}` | The same files, relative to the job's workspace. |
| `new Workspace {}` | The job's workspace directory (also the command's working directory). |
| `new WorkspaceIndicator {}` | Path of the marker file that defined the workspace. |
| `new ProjectRoot {}` | The project root. |
| `new ToolExecutable {}` | The resolved tool executable. |
| `new ExtraArgs {}` | The tool's `extraArgs`, then the workflow's, then the phase's or command's own. |

Without a `workspaceIndicator`, commands run in the project root.

### Executable resolution

A bare program name (no `/`) is looked up in each `settings.localBinDirs`
entry, in order, from each checked file's directory (and the job's workspace)
up to the project root, nearest first; the first executable file wins. So
with the defaults, a package's own `node_modules/.bin/eslint` beats the
repository root's even for a tool without a `workspaceIndicator`, and either
beats `.venv/bin/eslint` or `PATH`. Names found in none of those directories
are run through `PATH`; a path with a `/` is run as given, relative to the
command's working directory (the project root without a
`workspaceIndicator`). `doctor`, `tools`, and `init` resolve programs the same
way from the project root.

## Recipes

### Relax a lint rule only inside the hook

To stop Ruff from flagging (and auto-deleting) unused imports while an agent
is mid-edit, without changing the project's own `ruff` configuration, ignore
the rule in every Ruff lint command:

```pkl
local hookOnly = new Listing<String> { "--ignore"; "F401" }

tools {
  ["ruff"] = (Builtins.ruff) {
    // Deferred Stop hook: the lint workflow's check and remedy.
    workflows { ["lint"] { extraArgs = hookOnly } }
    // Immediate PostToolUse hook: the lint fix and verify phases.
    phases {
      ["fix"] { extraArgs = hookOnly }
      ["verify"] { extraArgs = hookOnly }
    }
  }
}
run { "ruff" }
```

Ruff's `format` workflow and phase do not accept `--ignore`, which is why the
arguments go on the lint workflow and phases rather than on the tool. Use
`--ignore`, not `--unfixable`: `--unfixable F401` keeps the import but the
check still reports it, so the file ends up needing a manual fix.

For a tool whose commands all accept the same flags, tool-level `extraArgs`
is enough. Cargo Clippy's commands end in `--`, so its extra arguments are
lint flags for both the fix and the verify pass:

```pkl
tools {
  ["cargoClippy"] = (Builtins.cargoClippy) { extraArgs { "-A"; "unused_imports" } }
}
```

### Set environment variables or a longer timeout

```pkl
settings { commandTimeoutSeconds = 60 }

tools {
  ["mypy"] = (Builtins.mypy) {
    env { ["MYPY_CACHE_DIR"] = "/tmp/mypy-cache" }
    timeoutSeconds = 300
  }
}
```

### Exclude more files, or replace the defaults

```pkl
settings { exclude { "**/generated/**"; "vendor/**" } }
```

adds two patterns to the defaults. To replace the list entirely:

```pkl
merge { resetExclude = true }
settings { exclude { "**/.git/**" } }
```

### Define a tool

```pkl
tools {
  ["kdlfmt"] = new ToolSpec {
    id = "kdlfmt"
    displayName = "kdlfmt"
    executable = "kdlfmt"
    files { include { "*.kdl"; "**/*.kdl" } }
    phases {
      ["format"] = new Phase {
        mode = "format"
        argv { "format"; new ExtraArgs {}; new Files {} }
        writes = "target-files"
      }
      ["verify"] = new Phase {
        mode = "verify"
        argv { "check"; new ExtraArgs {}; new Files {} }
        exitCodes { issues { 1 } }
      }
    }
    phaseOrder { "format"; "verify" }
  }
}
run { "kdlfmt" }
```

The deferred hook translates these phases into a workflow. Give a tool a
verify phase (or explicit `workflows` with a `check`) so the deferred hook can
confirm its fixes. A formatter defined with only mutating phases still works
at Stop: its fix runs on the changed files, files it rewrites are reported as
auto-fixed without a confirming check (`"unverified": true` on the report in
`summary.json`), and it never blocks. Only a failing fix command is an
operational problem. (Built-in specs must have a check.)

## Immediate hook output

`post-tool-immediate` runs the tools whose globs match the files a tool call
changed inside the project; files outside the project root (plans, memory
files, scratch files, sibling repositories) are never touched. Calls that
change no files, such as reads and searches, and calls that touch only
Git-ignored files return at once without evaluating any policy.

| Outcome | Agent (`additionalContext`) | User (`systemMessage`) |
| --- | --- | --- |
| Clean | nothing | nothing |
| Auto-fixed | `velvet-glove auto-fixed src/a.py (Ruff); re-read before editing.` (at most 10 files, then `and N more`) | the same line |
| Issues remain | `velvet-glove: Ruff reports issues in src/a.py:` plus a bounded excerpt of the deciding check's output | `velvet-glove: Ruff: issues remain in src/a.py; diagnostics: <path>` |
| Issues only in files the call did not change | nothing | `velvet-glove: not reporting issues outside the files this call changed: cargo clippy (src/lib.rs).` |
| Tool missing, crashed, timed out | nothing | `velvet-glove could not run Ruff (ruff not found; <install hint>).` or `(<phase> failed with exit code N; log: <path>)`, as at Stop |
| Policy error | nothing | `velvet-glove: configuration error; no tools ran (pkl eval failed for <policy file>: <first error line>). Details: <log>` |

Issues are blamed on the files the deciding output names, as at Stop: a
workspace-wide check that reports a pre-existing issue in another file is not
pinned on the file the call changed. The excerpt is the output of the verify
phase that found the issues (or, for a tool without one, of the phases that
did), with ANSI escapes removed and project paths made relative. The
`deferredReporting.excerptMaxLines`/`excerptMaxChars` budget is divided among
the tools that report issues in one call exactly as at Stop (an equal share
each, at least 5 lines and 400 characters while budget remains). Output
that does not fit its share first has repeated lines collapsed (the first
copy ends in `(repeated N times)`), so noise such as a warning printed once
per target cannot crowd out the real error; a cut excerpt ends with
`…truncated; full log: <path>`. The texts come from the
tool's `messages.issuesAgent` / `issuesChangedAgent` templates, which receive
`excerpt` alongside `tool`, `changed_files`, `issue_files`, and the
`diagnostics_*` paths. A template that fails to render falls back to the
built-in wording with a user notice, and an unwritable `diagnosticsDirectory`
falls back to the default location, so the agent always hears about changed
files. Under `missingToolPolicy = "harness-block"`, the blocking message also
carries the auto-fix line and feedback from tools that ran before it.

Clean output is `{}` with empty stderr. Full command output goes only to the
diagnostics file. Immediate mode never fails the hook or feeds an error back
unless `missingToolPolicy` asks for it (`hard-failure` or `harness-block`). A tool that sets its own
`messages.cleanChangedAgent` gets that text instead of the shared auto-fix
line.

## Built-in tools

The embedded catalog currently contains 134 reusable specifications (122
enabled), including Ruff, Prettier, ESLint, Biome, Cargo fmt, and Cargo
Clippy. Each enabled entry either has explicit deferred workflows or a
validated compatibility translation. The generated
[built-in workflow audit](builtin-deferred-workflow-audit.md) is the
authoritative inventory of commands, scopes, invocation granularity, and
known limitations; [tool support status](tool-support.md) tracks which
builtins have been validated against real tools. `velvet-glove tools [--json]` lists every entry's Pkl
key (the name used in `tools` and `run`, e.g. `cargoFmt`), id, file globs, and
where its executable resolves (`project-local` under the default
`localBinDirs`, `found` on `PATH`, or `missing`).

## Generating and checking a policy

`velvet-glove init [--dir DIR] [--print] [--force]` writes a commented
`.velvet-glove/post-tool-use.pkl` for a project. It lists project files with
`git ls-files` (or a bounded walk that honors simple root `.gitignore`
patterns) and selects an enabled builtin when all of these hold:

- its `files` globs match at least one project file;
- every program it runs resolves as the hooks would resolve it (the default
  `localBinDirs` at the project root, then `PATH`); and
- one of its detection indicators is present, or it is the default tool for
  its role and no tool sharing that role has an indicator.

Detection metadata lives in each builtin's optional `detect` block and never
affects hook execution:

| Field | Meaning |
| --- | --- |
| `indicators` | Project-relative globs, typically config files (`ruff.toml`, `.prettierrc.*`, `**/Cargo.toml`). |
| `contains` | File → text, e.g. `["package.json"] = "\"eslint\""` or `["pyproject.toml"] = "[tool.ruff"`. |
| `role` | Mutually exclusive slot such as `python-lint`, `js-format`, or `go-lint`. |
| `default` | Chosen for its role when no role member has an indicator. |
| `note` | Why the tool is opt-in or config-only; shown by `init`. |

Defaults are reserved for canonical, local-only tools that need no project
configuration (for example Ruff, gofmt, go vet, ShellCheck, hadolint, terraform
fmt, nixfmt, and xmllint). Tools that reach the network, apply disruptive
automatic fixes, or are drafts — lychee, govulncheck, pinact, typos, knip,
gitleaks, deadnix, gomod-tidy, and similar — are only selected when their own
configuration file is present, or never. The generated file names the reason
for each choice and lists installed alternatives and wanted-but-missing tools
as commented-out entries. `init` evaluates the file with Pkl before writing it
and refuses to overwrite an existing policy without `--force`.

`velvet-glove doctor [--dir DIR]` prints the discovered policy files in merge
order, the evaluated `run` list with each tool's resolved executable (marked
`(project-local)` when it comes from `settings.localBinDirs`) or install hint,
the Pkl version, and the state directory. It exits nonzero when
Pkl is missing or older than 0.31.1, the policy fails to evaluate, or `run`
names a tool that no `tools` entry defines; an empty `run` list, disabled
entries, and missing executables are warnings.

## Running the checks by hand

`velvet-glove [--config PATH] check [--dir DIR] [--json] [FILES...]` runs the
policy's Stop-time workflows (check, remedy, final check) right now, with the
same engine as the Stop hook but outside any hook: it reads no hook payload
and never touches session state. It checks the named files (relative to
`DIR`; directories expand to their non-ignored files) or, with no `FILES`, the
Git work tree's modified, staged, and untracked files under `DIR`. Automatic
fixes are applied, as at Stop.

It prints one line per file (`clean`, `auto-fixed by Ruff`, `needs manual
fixes`, or `not checked: … could not run`), the same bounded excerpts the
agent would see for remaining issues, any tool problems, and the directory
holding every command log and a `summary.json`
(`$TMPDIR/velvet-glove/check/<run>`, newest 20 kept). `--json` prints the
same information as one object (`status`, `exitCode`, `files`, `issues`,
`problems`, `outOfScope`, `logDirectory`, `summaryPath`); `status` is
`clean`, `auto-fixed`, `manual`, `operational`, or, when nothing could run,
`error` with an `error` message.

| Exit | Meaning |
| --- | --- |
| `0` | Every file is clean or was auto-fixed. |
| `1` | Manual fixes remain. |
| `2` | A tool could not run (missing, crashed, timed out), the policy failed to load, or a named file does not exist. |

Use it to try a policy before relying on the hooks, in CI, or to validate a
tool spec against real files.

## Deferred reports

The default Stop-time messages follow one contract:

| Result | Agent | User |
| --- | --- | --- |
| Clean | nothing | nothing |
| Auto-fixed only | nothing | `velvet-glove auto-fixed src/a.py (Ruff), web/b.ts (Prettier); re-read before editing.` |
| Manual fixes needed (blocks) | the auto-fix line (if any), then a short header plus, per tool, a bounded, ANSI-free, project-relative excerpt of the final check's output (`…truncated; full log: <path>` when cut) | the auto-fix line (if any), file count, files, and the run directory |
| Tool missing, crashed, or misconfigured | nothing | one line naming the tool, the reason, and an install hint or log path |
| Issues only in files not changed this turn | nothing | a one-line "not blocking" note |

The default `autoFixed.agent` template renders only when the Stop blocks
(`{% if blocks.manual or blocks.operational or blocks.coverage %}`), so the
agent re-reads fixed files before fixing the rest. On an allowed Stop, agent
context would cost a model turn spent acknowledging it (and Claude Code
already tells the agent when a file it read changed on disk); set
`autoFixed = new TemplatePair { agent = "…" }` to send it anyway. Immediate
mode keeps its agent line, which rides on the tool result.

A file counts as auto-fixed only when a remedy changed its bytes and its final
check passed. When a batch or workspace check fails, its output decides the
blame: issues go to the candidate files it names; if it names only other
existing files, the issues are out of scope and never block; if it names no
file at all, every candidate is blamed. Git-ignored files (build outputs, for
example) are never candidates.

Each Stop that runs tools writes every command's log and a `summary.json`
(the complete result, block decision, and rendered messages) to a run
directory under the state root, `$TMPDIR/velvet-glove/state/…/runs/<run>/`;
each session keeps its newest 20 runs. The user message for a block names
that directory.

`settings.deferredReporting` defines ordered file groups plus `clean`,
`autoFixed`, `manualFixesNeeded`, and `operationalError` user/agent templates.
`masterUser` and `masterAgent` combine the rendered buckets. Templates use
MiniJinja and receive run paths (including `run.directory`), counts, typed
files (auto-fixed files carry `fixedBy` tool names), reports, artifacts,
groups, operational problems, and coverage gaps, plus:

- `issues`: one entry per manual report with `tool`, `files`, `excerpt`,
  `truncated`, and `log_path` of the deciding check;
- `problems`: one entry per failing tool with `tool`, `reason`,
  `missing_tool`, `install_hint`, and `log_path`;
- `out_of_scope`: `tool` and `files` for issues blamed on unchanged files;
- `blocks`: whether `manual`, `operational`, or `coverage` results block.

Syntax is validated before configured tools run; later rendering errors are
committed as operational artifacts.

| Field | Default | Purpose |
| --- | --- | --- |
| `deferredReporting.blockOnOperationalErrors` | `false` | Also block Stop on tool crashes, timeouts, and reporting or tool-plan errors in a policy that loaded. A policy that fails to load never blocks. |
| `deferredReporting.maxConsecutiveBlocks` | `3` | Blocks allowed in one chain of stop-hook continuations; `0` disables the cap. |
| `deferredReporting.excerptMaxLines` | `60` | Total check-output lines quoted to the agent across all issues (at Stop, and per call in immediate mode). |
| `deferredReporting.excerptMaxChars` | `6000` | Total check-output characters quoted to the agent across all issues (likewise). |

Missing executables follow `settings.missingToolPolicy` at Stop too:
`user-notice` notifies without blocking or keeping the files pending,
`harness-block` blocks, and `hard-failure` fails the hook. Under
`settings.failFast`, an operational failure skips only the same workflow's
later remedies; the tool's other workflows (Ruff's lint when its format check
cannot run) and other tools still fix their files.

A Stop that follows a block (Claude and Codex `stop_hook_active`; for
Antigravity, the Stop right after a block) is not blocked again for an
identical set of issues; the user is told instead, and the unfixed files stay
pending for the next turn. Antigravity has no such flag, so any allowed Stop
ends the chain there: the Stop after it starts a new one and can block again.
Numbers on output lines that name no blamed file (timings, random seeds,
counters) are ignored when deciding whether the issues are identical.

Native Stop events have different output capacity:

| Harness | Allowed completion | Blocked completion |
| --- | --- | --- |
| Claude Code | user `systemMessage`; agent additional context | user `systemMessage`; agent `reason` |
| Codex | user `systemMessage`; no agent channel | user `systemMessage`; agent `reason` |
| Antigravity | no user or agent channel | one `reason` channel |

A blocked completion always carries a nonempty `reason`; an empty agent
template falls back to a generic reason with the run directory.

`strict` fails before pending-state acknowledgement when a configured audience
cannot be represented. The best-effort policies omit it, optionally emitting a
warning through a native channel (no warning is added when the omitted agent
line is exactly the user line). Every disposition is recorded in the run
summary.
