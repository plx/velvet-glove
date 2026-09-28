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
and known limitations. `velvet-glove tools [--json]` lists every entry's Pkl
key (the name used in `tools` and `run`, e.g. `cargoFmt`), id, file globs, and
whether its executable resolves on `PATH`.

## Generating and checking a policy

`velvet-glove init [--dir DIR] [--print] [--force]` writes a commented
`.velvet-glove/post-tool-use.pkl` for a project. It lists project files with
`git ls-files` (or a bounded walk that honors simple root `.gitignore`
patterns) and selects an enabled builtin when all of these hold:

- its `files` globs match at least one project file;
- every program it runs resolves on `PATH`; and
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
order, the evaluated `run` list with each tool's resolved executable or
install hint, the Pkl version, and the state directory. It exits nonzero when
Pkl is missing or older than 0.31.1, the policy fails to evaluate, or `run`
names a tool that no `tools` entry defines; an empty `run` list, disabled
entries, and missing executables are warnings.

## Deferred reports

`settings.deferredReporting` defines ordered file groups plus `clean`,
`autoFixed`, `manualFixesNeeded`, and `operationalError` user/agent templates.
`masterUser` and `masterAgent` combine the rendered buckets. Templates use
MiniJinja and receive run paths, counts, typed files, reports, artifacts,
groups, operational problems, and coverage gaps. Syntax is validated before
configured tools run; later rendering errors are committed as operational
artifacts.

Native Stop events have different output capacity:

| Harness | Allowed completion | Blocked completion |
| --- | --- | --- |
| Claude Code | user `systemMessage`; agent additional context | user `systemMessage`; agent `reason` and additional context |
| Codex | user `systemMessage`; no agent channel | user `systemMessage`; agent `reason` |
| Antigravity | no user or agent channel | one `reason` channel |

`strict` fails before pending-state acknowledgement when a configured audience
cannot be represented. The best-effort policies omit it, optionally emitting a
warning through a native channel. Every disposition is recorded in the run
summary.
