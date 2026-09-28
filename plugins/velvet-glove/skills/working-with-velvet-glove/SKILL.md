---
name: working-with-velvet-glove
description: Install, configure, operate, or troubleshoot the Pkl-driven Velvet Glove formatting and linting hooks for Claude Code and Codex. Use when setting up Velvet Glove, editing its layered Pkl policy, understanding its hook lifecycle or manual-intervention reports, or diagnosing missing hooks and tools.
---

# Working with Velvet Glove

## Overview

Velvet Glove runs the formatters and linters the user already has installed on
the files the agent edits. In the default deferred mode it records edits and
runs the tools once when the agent stops: clean runs are silent, automatic
fixes produce one line, and only issues that need a manual fix block the stop,
with a short excerpt of the tool output. `VELVET_GLOVE_MODE=immediate` runs
the tools after every edit instead. Treat this file as the overview; load only
the reference that matches the task.

## Workflow

1. Identify whether Claude Code or Codex is running the hook, and whether the
   plugin runs in deferred (default) or `VELVET_GLOVE_MODE=immediate` mode.
2. Run `velvet-glove doctor` in the repository: it checks Pkl (0.31.1 or
   newer), the layered Pkl configuration, the run list, and where each tool's
   executable resolves.
3. If there is no policy yet, run `velvet-glove init` (or `init --print` to
   preview) and review the generated `.velvet-glove/post-tool-use.pkl`.
4. Reproduce what the Stop hook would do with `velvet-glove check [FILES...]`,
   or inspect the run directory named in the last report.
5. Change configuration or installation state only within the user's requested scope.

## References

- For installation, prerequisites, and harness registration, read
  [installation.md](reference/installation.md).
- For Pkl policy discovery and customization, read
  [configuration.md](reference/configuration.md).
- For event-to-command mapping and state boundaries, read
  [hook-lifecycle.md](reference/hook-lifecycle.md).
- For auto-fix outcomes and blocked stops, read
  [manual-intervention.md](reference/manual-intervention.md).
- For missing executables, hook loading, and report diagnosis, read
  [troubleshooting.md](reference/troubleshooting.md).
- For prebuilt-binary distribution status, read
  [release-packaging.md](reference/release-packaging.md).
