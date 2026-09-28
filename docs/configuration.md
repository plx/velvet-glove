# Configuration reference

Velvet Glove evaluates policy with Pkl 0.31.1 or newer. A policy imports the embedded
schema and built-in catalog by their staged names:

```pkl
amends "Config.pkl"
import "Builtins.pkl"

settings {
  diagnosticsDirectory = ".velvet-glove/post-tool-use"
  missingToolPolicy = "user-notice"
}

tools {
  ["ruff"] = (Builtins.ruff) {
    phases {
      ["fix"] {
        extraArgs = new Listing<String> { "--unfixable"; "F401" }
      }
    }
    workflows {
      ["lint"] {
        remedy {
          extraArgs = new Listing<String> { "--unfixable"; "F401" }
        }
      }
    }
  }
  ["prettier"] = Builtins.prettier
}

run = new Listing<String> { "ruff"; "prettier" }
```

`phases` drive `post-tool-immediate`. Explicit `workflows` drive deferred
`turn-completion`; compatible legacy phase sets are translated only when the
catalog validator can prove a read-only final check.

## Discovery and merge order

When `--config PATH` is present, Velvet Glove loads only that file and anchors
relative project behavior on the event workspace. Otherwise it merges:

1. home policy;
2. project policies from filesystem root to the event workspace; and
3. local policies from filesystem root to the event workspace.

Each layer reads the legacy `.agent-hook-kit` namespace first, then the
canonical `.velvet-glove` namespace. Canonical peers therefore win within a
layer, while normal home → project → local precedence remains intact. The two
filenames are `post-tool-use.pkl` and `post-tool-use.local.pkl`; the latter
should be ignored by version control.

A layer can discard inherited state with:

- `merge { resetAll = true }`;
- `merge { reset = new Listing { "tools"; "run" } }`;
- `merge { resetTools = new Listing { "ruff" } }`; or
- `merge { resetDeferredReporting = true }`.

## Runner settings

| Field | Default | Purpose |
| --- | --- | --- |
| `settings.jobs` | `0` | Maximum independent jobs; zero selects the runner default. |
| `settings.failFast` | `true` | Stop scheduling after an operational failure. |
| `settings.continueAfterIssues` | `true` | Continue with later tools after source issues. |
| `settings.exclude` | `.git/**`, `node_modules/**` | Global exclusions applied before tool filters. |
| `settings.loweringPolicy` | `best-effort-with-warnings` | Handle messages a native hook event cannot represent: `strict`, `best-effort`, or warning mode. |
| `settings.diagnosticsDirectory` | `.velvet-glove/post-tool-use` | Project-relative directory for complete diagnostic artifacts. |
| `settings.missingToolPolicy` | `user-notice` | Missing executable behavior: `user-notice`, `hard-failure`, or `harness-block`. |
| `settings.fileActivity.filesystemMtime` | `true` | Reconcile mtime evidence through a durable cutoff before Stop. |
| `settings.fileActivity.vcs` | `disabled` | Optional broad `git-dirty` fallback. |
| `settings.fileActivity.maxEntries` | `100000` | Bound recursive workspace expansion. |
| `settings.fileActivity.coverageGapPolicy` | `best-effort` | Warn and retain incomplete evidence, or use `strict` to block. |

## Built-in tools

The embedded catalog currently contains 134 reusable specifications, including
Ruff, Prettier, ESLint, Biome, Cargo fmt, and Cargo Clippy. Each enabled entry
either has explicit deferred workflows or a validated compatibility
translation. The generated [built-in workflow audit](builtin-deferred-workflow-audit.md)
is the authoritative inventory of commands, scopes, invocation granularity,
and known limitations.

## Deferred reports

The default Stop-time messages follow one contract:

| Result | Agent | User |
| --- | --- | --- |
| Clean | nothing | nothing |
| Auto-fixed only | `velvet-glove auto-fixed src/a.py (Ruff), web/b.ts (Prettier); re-read before editing.` | the same line |
| Manual fixes needed (blocks) | a short header plus, per tool, a bounded, ANSI-free, project-relative excerpt of the final check's output (`…truncated; full log: <path>` when cut) | file count, files, and the run directory |
| Tool missing, crashed, or misconfigured | nothing | one line naming the tool, the reason, and an install hint or log path |
| Issues only in files not changed this turn | nothing | a one-line "not blocking" note |

A file counts as auto-fixed only when a remedy changed its bytes and its final
check passed. When a batch or workspace check fails, its output decides the
blame: issues go to the candidate files it names; if it names only other
existing files, the issues are out of scope and never block; if it names no
file at all, every candidate is blamed. Git-ignored files (build outputs, for
example) are never candidates.

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
| `deferredReporting.blockOnOperationalErrors` | `false` | Also block Stop on tool crashes and configuration errors. |
| `deferredReporting.maxConsecutiveBlocks` | `3` | Blocks allowed in one chain of stop-hook continuations; `0` disables the cap. |
| `deferredReporting.excerptMaxLines` | `60` | Total final-check lines quoted to the agent across all issues. |
| `deferredReporting.excerptMaxChars` | `6000` | Total final-check characters quoted to the agent across all issues. |

Missing executables follow `settings.missingToolPolicy` at Stop too:
`user-notice` notifies without blocking or keeping the files pending,
`harness-block` blocks, and `hard-failure` fails the hook. Under
`settings.failFast`, an operational failure skips only the same tool's later
remedies; other tools still fix their files.

A Stop that follows a block (Claude and Codex `stop_hook_active`) is not
blocked again for an identical set of issues; the user is told instead, and the
unfixed files stay pending for the next turn.

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
