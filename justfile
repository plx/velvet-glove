repository_root := justfile_directory()

default:
    @just --list

# Run the complete local pre-PR check.
check:
    "{{ repository_root }}/scripts/release-check.sh"

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

# Keep missing-binary behavior quiet and verify mode and cross-harness dispatch.
test-plugin-launcher:
    #!/usr/bin/env bash
    set -euo pipefail
    launcher="{{ repository_root }}/plugins/velvet-glove/scripts/run-velvet-glove.sh"
    missing="VELVET_GLOVE_BIN={{ repository_root }}/.context/velvet-glove-does-not-exist"
    echo_bin="VELVET_GLOVE_BIN=/bin/echo"
    # launch EVENT [VAR=value...]: run the launcher isolated from this shell's
    # own agent-hook environment.
    launch() { local event=$1; shift; env -i PATH="$PATH" "$@" sh "$launcher" "$event"; }
    rejects() { if launch "$@" 2>/dev/null; then echo "accepted: $*" >&2; exit 1; fi; }
    codex_plugin=(PLUGIN_ROOT=/p PLUGIN_DATA=/d CLAUDE_PLUGIN_ROOT=/p CLAUDE_PLUGIN_DATA=/d)
    # Missing binary: quiet, and Stop still returns protocol-safe JSON.
    test -z "$(launch post-tool "$missing" 2>/dev/null)"
    test "$(launch turn-completion "$missing" 2>/dev/null)" = '{}'
    test "$(launch session-start-state "$missing" 2>&1)" != ''
    # Harness detection.
    test "$(launch post-tool "$echo_bin")" = '--harness claude post-tool'
    test "$(launch post-tool CLAUDECODE=1 CLAUDE_PLUGIN_ROOT=/p "$echo_bin")" = '--harness claude post-tool'
    test "$(launch post-tool "${codex_plugin[@]}" CLAUDECODE=1 "$echo_bin")" = '--harness codex post-tool'
    test "$(launch post-tool PLUGIN_ROOT=/p "$echo_bin")" = '--harness claude post-tool'
    test "$(launch post-tool "${codex_plugin[@]}" VELVET_GLOVE_HARNESS=claude "$echo_bin")" = '--harness claude post-tool'
    # Deferred mode (default) forwards each event unchanged.
    test "$(launch turn-completion "$echo_bin")" = '--harness claude turn-completion'
    test "$(launch session-start-state "$echo_bin")" = '--harness claude session-start-state'
    test "$(launch post-tool-immediate "$echo_bin")" = '--harness claude post-tool-immediate'
    # Immediate mode checks after each edit; Stop and SessionStart are no-ops.
    immediate=VELVET_GLOVE_MODE=immediate
    test "$(launch post-tool "$immediate" "${codex_plugin[@]}" "$echo_bin")" = '--harness codex post-tool-immediate'
    test "$(launch turn-completion "$immediate" "$echo_bin")" = '{}'
    test -z "$(launch session-start-state "$immediate" "$echo_bin")"
    # Invalid input fails loudly with a non-blocking status.
    rejects post-tool VELVET_GLOVE_MODE=sometimes "$echo_bin"
    rejects post-tool VELVET_GLOVE_HARNESS=antigravity "$echo_bin"
    rejects pre-tool "$echo_bin"
