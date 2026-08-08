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

1. Identify whether Claude Code or Codex is running the hook.
2. Confirm that `velvet-glove` and Pkl 0.31.1 are available.
3. Locate the applicable layered Pkl configuration.
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
