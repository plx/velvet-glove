#!/usr/bin/env bash
# Run under .github/real-tools/mise.toml, or supply the same tools and selection.
set -euo pipefail

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"
: "${VELVET_GLOVE_FIXTURE_TOOLS:?Select fixture tool IDs before running this lane}"
export VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS="${VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS:-all}"
export VELVET_GLOVE_FIXTURE_ARTIFACT_DIR="${VELVET_GLOVE_FIXTURE_ARTIFACT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/velvet-glove-real-tools.XXXXXX")}"
mkdir -p "$VELVET_GLOVE_FIXTURE_ARTIFACT_DIR"

{
  printf 'Selected tools: %s\n' "$VELVET_GLOVE_FIXTURE_TOOLS"
  # Version output is evidence about this run, never a binary-identity gate.
  pkl --version
  rustc --version
  cargo fmt --version
  cargo clippy --version
  go version
  actionlint --version
  jq --version
} 2>&1 | tee "$VELVET_GLOVE_FIXTURE_ARTIFACT_DIR/versions.txt"

cargo test --locked -p velvet-glove --test tool_fixtures run_all_tool_fixtures \
  -- --ignored --exact --nocapture 2>&1 | tee "$VELVET_GLOVE_FIXTURE_ARTIFACT_DIR/run.log"
