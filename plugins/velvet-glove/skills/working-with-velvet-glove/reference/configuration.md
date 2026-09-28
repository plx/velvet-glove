# Configuration

Policies are Pkl files that amend `Config.pkl` and import `Builtins.pkl`
(Pkl 0.31.1 or newer). Nothing runs until `run` lists tools:

```pkl
amends "Config.pkl"
import "Builtins.pkl"

tools { ["ruff"] = Builtins.ruff }
run { "ruff" }
```

Without `--config`, layers merge in this order, later winning:
`~/.velvet-glove/post-tool-use.pkl`, then each
`.velvet-glove/post-tool-use.pkl` from the filesystem root down to the
workspace, then the `post-tool-use.local.pkl` files in the same order (keep
those out of version control). `velvet-glove doctor` prints the chain it found.

Common changes, each made by amending a builtin inside `tools`:

- Hook-only rule changes: `extraArgs` on the tool, a workflow, or a phase, e.g.
  `(Builtins.cargoClippy) { extraArgs { "-A"; "unused_imports" } }`. For Ruff,
  put `--ignore F401` on the `lint` workflow and the `fix`/`verify` phases so
  the check and the fix agree.
- Paths: `files { exclude { "generated/**" } }` per tool, or
  `settings { exclude { ... } }` globally (appends to the defaults, which
  already cover `.git`, `node_modules`, `.venv`, `target`, and tool caches).
- Executables: `settings.localBinDirs` (default `node_modules/.bin`,
  `.venv/bin`) is searched before `PATH`.
- Time and environment: `timeoutSeconds` and `env` per tool,
  `settings.commandTimeoutSeconds` globally (default 120).
- Messages: `settings.deferredReporting` templates and excerpt budgets.

After editing, run `velvet-glove doctor`, then `velvet-glove check` on a few
files. The full schema and recipes are in `docs/configuration.md` in the
Velvet Glove repository.
