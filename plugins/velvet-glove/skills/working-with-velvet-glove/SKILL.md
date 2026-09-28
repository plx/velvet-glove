---
name: working-with-velvet-glove
description: Install, configure, operate, or troubleshoot the Pkl-driven Velvet Glove formatting and linting hooks for Claude Code and Codex. Use when setting up Velvet Glove, editing its layered Pkl policy, understanding its hook lifecycle or manual-intervention reports, or diagnosing missing hooks and tools.
---

# Working with Velvet Glove

## Overview

Use Velvet Glove to record files changed during a coding session, run configured
formatters and linters at turn completion, and route unresolved findings back to
the coding agent. Treat this file as the overview; load only the reference that
matches the task.

## Workflow

1. Identify whether Claude Code or Codex is running the hook, and whether the
   plugin runs in deferred (default) or `VELVET_GLOVE_MODE=immediate` mode.
2. Run `velvet-glove doctor` in the repository: it checks Pkl (0.31.1 or
   newer), the layered Pkl configuration, the run list, and tool executables.
3. If there is no policy yet, run `velvet-glove init` (or `init --print` to
   preview) and review the generated `.velvet-glove/post-tool-use.pkl`.
4. Reproduce the lifecycle event or inspect the retained report.
5. Change configuration or installation state only within the user's requested scope.

## References

- For installation, prerequisites, and harness registration, read
  [installation.md](reference/installation.md).
- For Pkl policy discovery and customization, read
  [configuration.md](reference/configuration.md).
- For event-to-command mapping and state boundaries, read
  [hook-lifecycle.md](reference/hook-lifecycle.md).
- For auto-fix outcomes and retained manual findings, read
  [manual-intervention.md](reference/manual-intervention.md).
- For missing executables, hook loading, and report diagnosis, read
  [troubleshooting.md](reference/troubleshooting.md).
- For future prebuilt-binary distribution work, read
  [release-packaging.md](reference/release-packaging.md).
