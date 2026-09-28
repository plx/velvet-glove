# Velvet Glove

Velvet Glove runs the formatters and linters you already have installed on
the files a coding agent edits, fixes what they can fix quietly, and tells
the agent only about what is left. It hooks into Claude Code and Codex
(Antigravity is supported without a plugin) and is configured with a small
[Pkl](https://pkl-lang.org) policy per project. It never installs, pins, or
wraps your tools.

By default it works in **deferred** mode: edits are recorded as the agent
works, and the tools run once when the agent tries to stop. If issues remain
that the tools cannot fix themselves, the agent is asked to fix them before
it finishes. An **immediate** mode runs the tools after every edit instead.

HookKit is not yet published as a crate. All upstream HookKit dependencies are
therefore pinned to Git commit
`828d8d6feacf60015ae325d798b2bc3f32b2bf3b`.

## Quickstart: use Velvet Glove in another project

1. **Install the binary and Pkl 0.31.1 or newer.**

   ```sh
   brew install pkl   # or see https://pkl-lang.org
   cargo install --locked --git https://github.com/plx/velvet-glove velvet-glove
   ```

2. **Register the hooks.** The plugin is simplest:

   ```sh
   claude plugin marketplace add plx/velvet-glove
   claude plugin install velvet-glove@velvet-glove
   # Codex: `codex plugin marketplace add plx/velvet-glove`,
   # `codex plugin add velvet-glove@velvet-glove`, then review the hooks in /hooks.
   ```

   Export `VELVET_GLOVE_MODE=immediate` in the shell you start the agent from
   to switch the plugin to immediate mode. To register hooks by hand instead
   (Claude Code shown; use `--harness codex` for Codex), add to
   `.claude/settings.json`:

   ```json
   {
     "hooks": {
       "SessionStart": [{ "hooks": [{ "type": "command", "command": "velvet-glove --harness claude session-start-state" }] }],
       "PostToolUse": [{ "hooks": [{ "type": "command", "command": "velvet-glove --harness claude post-tool" }] }],
       "Stop": [{ "hooks": [{ "type": "command", "command": "velvet-glove --harness claude turn-completion", "timeout": 900 }] }]
     }
   }
   ```

   For immediate mode, register only `PostToolUse` with
   `velvet-glove --harness claude post-tool-immediate`. Use one mode, not both.
   (Known issue: the pinned HookKit makes a hand-registered hook exit 1
   silently when another plugin leaks a lone `CLAUDE_PLUGIN_ROOT` or
   `CLAUDE_PLUGIN_DATA`; prefix the commands with
   `env -u CLAUDE_PLUGIN_ROOT -u CLAUDE_PLUGIN_DATA` if that happens.)

3. **Write, check, and try the project policy.** Nothing runs until a policy
   lists tools.

   ```sh
   cd your-project
   velvet-glove init     # detect fitting tools, write .velvet-glove/post-tool-use.pkl
   velvet-glove doctor   # Pkl version, config chain, run list, where each tool resolves
   velvet-glove check    # run the Stop-time checks on your changed files now
   ```

   `init` enables a builtin when the project has files it handles, its
   executable resolves (in `node_modules/.bin`, `.venv/bin`, or on `PATH`),
   and the project's config files point at it, or it is the standard choice
   (such as Ruff for Python). Commit the policy; keep personal tweaks in
   `.velvet-glove/post-tool-use.local.pkl` and add that to `.gitignore`.
   `velvet-glove check [FILES...]` applies fixes, prints a verdict per file,
   and exits 0 (clean or auto-fixed), 1 (manual fixes needed), or 2 (a tool
   could not run, or the policy is broken).

## What the agent and the user see

At Stop, in the default deferred mode:

| Result | Agent | User |
| --- | --- | --- |
| Clean | nothing | nothing |
| Auto-fixed only | nothing | `velvet-glove auto-fixed src/a.py (Ruff); re-read before editing.` |
| Manual fixes needed | Stop is blocked; the reason starts with the auto-fix line (if any) and lists each tool's files with a bounded, ANSI-free, project-relative excerpt of its final check output | the auto-fix line (if any), file count, files, and the run's log directory |
| Tool missing, crashed, or timed out; broken policy | nothing | one line with the tool, reason, and install hint or log path |
| Issues only in files not changed this turn | nothing | a one-line "not blocking" note |

Stop never blocks twice in a row on the same issues: when the agent's retry
leaves them unchanged (or after three consecutive blocks), the user gets a
note instead and the files stay queued for the next turn. An allowed Stop
tells the agent nothing about auto-fixes: any context there costs a model
turn spent acknowledging it, and Claude Code already tells the agent when a
file it read changed on disk. (`deferredReporting.autoFixed.agent` restores
the agent copy.)

In immediate mode the same contract applies per tool call, except that
nothing blocks: remaining issues reach the agent as context
(`velvet-glove: Ruff reports issues in src/a.py:` plus the excerpt), and the
user sees a line pointing at the full diagnostics.

Full tool output never goes into the agent's context. It is kept on disk
under `$TMPDIR/velvet-glove/`: `state/…/runs/<run>/` for each Stop (every
command log plus `summary.json`; the newest 20 runs per session are kept),
`state/post-tool-immediate/` for immediate mode, and `check/<run>/` for
`velvet-glove check`.

## Recipes

Override a builtin by amending it in `.velvet-glove/post-tool-use.pkl`.

Keep Ruff from flagging (and deleting) unused imports while the agent is
mid-edit, without changing the project's own Ruff configuration. The same
arguments go to every Ruff lint command so the check and the fix agree:

```pkl
local hookOnly = new Listing<String> { "--ignore"; "F401" }

tools {
  ["ruff"] = (Builtins.ruff) {
    workflows { ["lint"] { extraArgs = hookOnly } }   // deferred (Stop)
    phases {                                          // immediate
      ["fix"] { extraArgs = hookOnly }
      ["verify"] { extraArgs = hookOnly }
    }
  }
}
```

Allow unused imports in Clippy's hook runs (its commands end in `--`, so the
extra arguments are lint flags for both the fix and the check):

```pkl
tools {
  ["cargoClippy"] = (Builtins.cargoClippy) { extraArgs { "-A"; "unused_imports" } }
}
```

More recipes (excludes, timeouts, environment variables, custom tools) are
in the [configuration reference](docs/configuration.md#recipes).

## Tool support

The embedded catalog covers well over a hundred formatters and linters (Ruff,
Prettier, ESLint, Biome, cargo fmt, Clippy, gofmt, ShellCheck, and more);
`velvet-glove tools` lists them. Builtins are validated against real tools
one at a time: see [tool support status](docs/tool-support.md) for the
status of each builtin, and the generated
[built-in workflow audit](docs/builtin-deferred-workflow-audit.md) for the
exact commands.

## Reference

| Command | Purpose |
| --- | --- |
| `velvet-glove --harness H post-tool` | PostToolUse: quietly record file activity (deferred). |
| `velvet-glove --harness H turn-completion` | Stop: run the deferred workflows, then report or block. |
| `velvet-glove --harness H session-start-state` | SessionStart: record the session start (Claude Code, Codex). |
| `velvet-glove --harness H post-tool-immediate` | PostToolUse: run the tools on this call's files now. |
| `velvet-glove init [--print] [--force]` | Write a starter policy for the project. |
| `velvet-glove doctor` | Explain the setup and fail on hard problems. |
| `velvet-glove check [--json] [FILES...]` | Run the deferred workflows now, outside any hook. |
| `velvet-glove tools [--json]` | List the builtin catalog. |

`H` is `claude`, `codex`, or `antigravity`. Setup commands take `--dir DIR`;
`--config PATH` selects one policy file instead of discovery for
`turn-completion`, `post-tool-immediate`, `doctor`, and `check`. The
deferred commands share a state root,
`$TMPDIR/velvet-glove/state` by default (`--state-dir` overrides it on every
one). Antigravity has no SessionStart hook; its first PostToolUse sets the
session's lower bound.

Without `--config`, policies merge in this order, later winning:
`~/.velvet-glove/post-tool-use.pkl`, then every
`.velvet-glove/post-tool-use.pkl` from the filesystem root down to the
workspace, then the `.local.pkl` files in the same order (legacy
`.agent-hook-kit` files are still read, at lower precedence). See the
[configuration reference](docs/configuration.md) for the schema, settings,
and message templates, and the [architecture notes](docs/architecture.md)
for how the runners work. Users of the HookKit example should read the
[migration guide](docs/migrating-from-agent-hook-kit.md).

## Development

- `crates/velvet-glove`: the executable, CLI, and setup commands.
- `crates/hookkit-tool-runner`: the immediate and deferred runners.
- `crates/hookkit-pkl-config`: the Pkl schema, loader, and builtin catalog.

The other HookKit crates are not published yet and are pinned to Git commit
`83c49d46970602e8fb40a8afaeea521dfb7e9b61`.

```sh
just check    # full pre-PR check: plugins, fmt, clippy, tests, MSRV, docs, licenses
cargo test --locked --workspace --all-targets
# Real-tool fixture lane; runs the tools on your PATH:
VELVET_GLOVE_FIXTURE_TOOLS=ruff,jq cargo test -p velvet-glove --test tool_fixtures \
  run_all_tool_fixtures -- --ignored --exact --nocapture
```

The [real-tool CI lane](.github/workflows/real-tool-fixtures.yml) runs the
fixture cases for the reference tools weekly and on PRs that touch fixtures
or builtin specs; see the
[fixture README](crates/velvet-glove/tests/tool-fixtures/README.md). Run
`scripts/regen-licenses.sh` after dependency changes. Velvet Glove is dual
licensed under [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE); see
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).
