# Troubleshooting

Start with `velvet-glove doctor` in the affected repository. It prints the
discovered policy files and their merge order, the evaluated `run` list, where
each tool's executable resolves (or its install hint), the Pkl version, and
the state directory, and exits nonzero when the hooks cannot work.

- **Nothing happens:** no policy was found or `run` is empty. Run
  `velvet-glove init`, or add tools listed by `velvet-glove tools`.
- **A tool is reported missing:** hooks look for a bare program name in
  `settings.localBinDirs` (default `node_modules/.bin`, then `.venv/bin`,
  searched from the file's directory up to the project root) before `PATH`.
  `doctor` marks such tools `(project-local)`. A copy elsewhere, such as
  `venv/bin`, is reported as not searched: add its directory to
  `settings.localBinDirs`, or install the tool on `PATH`.
- **Pkl errors:** install Pkl 0.31.1 or newer; `doctor` shows the evaluation
  error for a broken policy, naming the policy file that failed.
- **Unexpected verdicts:** run `velvet-glove check FILES` (add `--json` for
  detail) to see what the Stop hook would report, and read the logs in the
  directory it prints. After a real Stop, the user message names the run
  directory holding every command log and `summary.json`.
- **Hand-registered hooks exit 1 with no output:** the pinned HookKit rejects
  a lone `CLAUDE_PLUGIN_ROOT` or `CLAUDE_PLUGIN_DATA` inherited from another
  plugin's environment. Use the plugin, or prefix the command with
  `env -u CLAUDE_PLUGIN_ROOT -u CLAUDE_PLUGIN_DATA`.
- **Wrong harness:** the plugin launcher picks Codex when `PLUGIN_ROOT` and
  `PLUGIN_DATA` are both set and Claude Code otherwise. Set
  `VELVET_GLOVE_HARNESS=claude|codex` to override.
- **Codex hooks never run:** newly installed or changed hooks must be reviewed
  under `/hooks` in the Codex CLI.
- **"Stop hook error occurred · ctrl+o to see":** Claude Code shows this
  generic toast for any non-empty Stop decision, including a normal, working
  "manual fixes needed" block. It does not mean the hook crashed; ctrl+o (or
  the run directory the block message names) shows the actual reason.
- **First run reformats a lot of the codebase:** with no tool config,
  formatters use their own defaults across every matching file, not just
  what an agent just touched. Expect one large diff the first time `check` or
  a Stop hook runs on a never-formatted project, then small, incremental
  diffs after.
- **A project-local tool is version-mismatched or still reported missing:**
  run the project's own install step (`uv sync`, `npm install`, …) before
  `velvet-glove init`/`doctor`; without it, resolution falls back to whatever
  same-named binary is on `PATH`, silently, with no version warning.
- **Monorepo / multiple workspaces:** tools resolve per workspace, searched
  from the directory of the files they'd run on up to the project root (so
  `frontend/node_modules/.bin/eslint` is found for `frontend/` files even
  though it is not at the project root). Run `velvet-glove doctor` to see
  where each tool in `run` actually resolved.
