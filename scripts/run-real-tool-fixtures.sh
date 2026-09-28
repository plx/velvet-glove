#!/usr/bin/env bash
# Run under .github/real-tools/mise.toml (and, best-effort, its broad/
# sibling), or supply the same tools and selection yourself.
set -euo pipefail

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"
: "${VELVET_GLOVE_FIXTURE_TOOLS:?Select fixture tool IDs before running this lane}"
export VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS="${VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS:-all}"
export VELVET_GLOVE_FIXTURE_ARTIFACT_DIR="${VELVET_GLOVE_FIXTURE_ARTIFACT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/velvet-glove-real-tools.XXXXXX")}"
mkdir -p "$VELVET_GLOVE_FIXTURE_ARTIFACT_DIR"

{
  printf 'Selected tools: %s\n' "$VELVET_GLOVE_FIXTURE_TOOLS"
  printf 'Required tools: %s\n' "$VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS"
  # Version output is evidence about this run, never a binary-identity
  # gate. `mise ls` is generic and best-effort by construction: it reports
  # every tool each config declares and its resolved version, with no
  # per-tool --version flag to keep in sync as tools are added or removed
  # (several, e.g. hk, have no top-level --version anyway), and it never
  # fails the run over one unreadable entry.
  printf '\nCore tier (required; see .github/real-tools/mise.toml):\n'
  mise -C "$repository_root/.github/real-tools" ls || true
  if [ -f "$repository_root/.github/real-tools/broad/mise.toml" ]; then
    printf '\nBroad tier (best-effort; see .github/real-tools/broad/mise.toml):\n'
    mise -C "$repository_root/.github/real-tools/broad" ls || true
  fi
} 2>&1 | tee "$VELVET_GLOVE_FIXTURE_ARTIFACT_DIR/versions.txt"

cargo test --locked -p velvet-glove --test tool_fixtures run_all_tool_fixtures \
  -- --ignored --exact --nocapture 2>&1 | tee "$VELVET_GLOVE_FIXTURE_ARTIFACT_DIR/run.log"
