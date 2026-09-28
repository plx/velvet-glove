# Installation

1. Install Pkl 0.31.1 or newer (`brew install pkl`, or see
   https://pkl-lang.org) and the `velvet-glove` binary:

   ```sh
   cargo install --locked --git https://github.com/plx/velvet-glove velvet-glove
   ```

2. Install the plugin, which registers the hooks:

   ```sh
   claude plugin marketplace add plx/velvet-glove
   claude plugin install velvet-glove@velvet-glove
   # or, for Codex (then review the new hooks under /hooks):
   codex plugin marketplace add plx/velvet-glove
   codex plugin add velvet-glove@velvet-glove
   ```

3. In the target repository, generate a policy and check it:

   ```sh
   velvet-glove init      # writes .velvet-glove/post-tool-use.pkl
   velvet-glove doctor    # config chain, run list, tool and Pkl status
   velvet-glove check     # run the Stop-time checks on changed files now
   ```

   `init --print` previews without writing; `init --force` regenerates.
   Without a policy the hooks run nothing. `check` exits 0 when everything is
   clean or auto-fixed, 1 when manual fixes remain, and 2 when a tool could
   not run.

## Modes

The plugin runs the deferred workflow by default: edits are recorded quietly
and the tools run once when the agent stops. Export `VELVET_GLOVE_MODE=immediate`
in the environment you start Claude Code or Codex from to run the tools after
every edit instead.

## Registering hooks by hand

Without the plugin, register the commands directly, always with an explicit
harness, e.g. for Claude Code in `.claude/settings.json`:

- deferred: `SessionStart` → `velvet-glove --harness claude session-start-state`,
  `PostToolUse` → `velvet-glove --harness claude post-tool`,
  `Stop` → `velvet-glove --harness claude turn-completion`;
- immediate: only `PostToolUse` → `velvet-glove --harness claude post-tool-immediate`.

Register one mode only; both at once runs the tools twice.
