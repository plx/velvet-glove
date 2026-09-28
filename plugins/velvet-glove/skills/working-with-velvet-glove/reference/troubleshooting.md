# Troubleshooting

Start with `velvet-glove doctor` in the affected repository. It prints the
discovered policy files and their merge order, the evaluated `run` list, where
each tool's executable resolves (or its install hint), the Pkl version, and
the state directory, and exits nonzero when the hooks cannot work.

- **Nothing happens:** no policy was found or `run` is empty. Run
  `velvet-glove init`, or add tools listed by `velvet-glove tools`.
- **A tool is reported missing:** hooks look for a bare program name in
  `settings.localBinDirs` (default `node_modules/.bin`, then `.venv/bin`,
  searched from the file's workspace up to the project root) before `PATH`.
  `doctor` marks such tools `(project-local)`. A copy elsewhere, such as
  `venv/bin`, is reported as not searched: add its directory to
  `settings.localBinDirs`, or install the tool on `PATH`.
- **Pkl errors:** install Pkl 0.31.1 or newer; `doctor` shows the evaluation
  error for a broken policy.
- **Wrong harness:** the plugin launcher picks Codex when `PLUGIN_ROOT` and
  `PLUGIN_DATA` are both set and Claude Code otherwise. Set
  `VELVET_GLOVE_HARNESS=claude|codex` to override.
- **Codex hooks never run:** newly installed or changed hooks must be reviewed
  under `/hooks` in the Codex CLI.
