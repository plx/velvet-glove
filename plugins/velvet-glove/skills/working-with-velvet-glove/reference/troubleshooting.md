# Troubleshooting

Start with `velvet-glove doctor` in the affected repository. It prints the
discovered policy files and their merge order, the evaluated `run` list, where
each tool's executable resolves (or its install hint), the Pkl version, and
the state directory, and exits nonzero when the hooks cannot work.

- **Nothing happens:** no policy was found or `run` is empty. Run
  `velvet-glove init`, or add tools listed by `velvet-glove tools`.
- **A tool is reported missing:** hooks resolve executables on `PATH` only.
  A copy under `node_modules/.bin` or `.venv/bin` is not used; put it on
  `PATH` for the agent's session or install it globally.
- **Pkl errors:** install Pkl 0.31.1 or newer; `doctor` shows the evaluation
  error for a broken policy.
- **Wrong harness:** the plugin launcher picks Codex when `PLUGIN_ROOT` and
  `PLUGIN_DATA` are both set and Claude Code otherwise. Set
  `VELVET_GLOVE_HARNESS=claude|codex` to override.
- **Codex hooks never run:** newly installed or changed hooks must be reviewed
  under `/hooks` in the Codex CLI.
