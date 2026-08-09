repository_root := justfile_directory()

default:
    @just --list

# Run the complete local pre-PR check.
check:
    "{{ repository_root }}/scripts/release-check.sh"

# Provision and run one pinned real-tool fixture contract on macOS arm64.
tool-case TOOL CASE:
    "{{ repository_root }}/scripts/run-pinned-tool-contract.sh" "{{ TOOL }}" "{{ CASE }}"

# Run the behavior-rich representative contract for every pinned environment.
tool-representatives:
    "{{ repository_root }}/scripts/run-pinned-tool-contract.sh" --representatives

# Validate both agent marketplaces and their shared plugin bundle.
validate-plugins: validate-claude-plugin validate-codex-plugin test-plugin-launcher

# Validate Claude's marketplace, plugin, skill, and hook manifests strictly.
validate-claude-plugin:
    claude plugin validate --strict "{{ repository_root }}"
    claude plugin validate --strict "{{ repository_root }}/plugins/velvet-glove"

# Exercise Codex's real marketplace resolution and installation in isolation.
validate-codex-plugin:
    #!/usr/bin/env bash
    set -euo pipefail
    validation_home="$(mktemp -d "${TMPDIR:-/tmp}/velvet-glove-codex-home.XXXXXX")"
    trap 'rm -rf -- "$validation_home"' EXIT
    env CODEX_HOME="$validation_home" codex plugin marketplace add "{{ repository_root }}" --json
    env CODEX_HOME="$validation_home" codex plugin list --marketplace velvet-glove --available --json
    env CODEX_HOME="$validation_home" codex plugin add velvet-glove@velvet-glove --json

# Keep missing-binary behavior quiet and verify cross-harness dispatch.
test-plugin-launcher:
    #!/usr/bin/env bash
    set -euo pipefail
    launcher="{{ repository_root }}/plugins/velvet-glove/scripts/run-velvet-glove.sh"
    missing_binary="{{ repository_root }}/.context/velvet-glove-does-not-exist"
    test -z "$(VELVET_GLOVE_BIN="$missing_binary" sh "$launcher" post-tool 2>/dev/null)"
    test "$(VELVET_GLOVE_BIN="$missing_binary" sh "$launcher" turn-completion 2>/dev/null)" = '{}'
    test "$(PLUGIN_ROOT=/plugin VELVET_GLOVE_BIN=/bin/echo sh "$launcher" post-tool)" = '--harness codex post-tool'
    test "$(CLAUDE_PLUGIN_ROOT=/plugin VELVET_GLOVE_BIN=/bin/echo sh "$launcher" post-tool)" = '--harness claude post-tool'
