#!/bin/sh
# Plugin hook launcher shared by Claude Code and Codex.
#
# Usage: run-velvet-glove.sh <post-tool|session-start-state|turn-completion|post-tool-immediate>
#
# VELVET_GLOVE_MODE selects the workflow:
#   deferred (default)  SessionStart records the session, PostToolUse quietly
#                       records edits, and Stop runs the tools once per turn.
#   immediate           PostToolUse runs the tools after every edit
#                       (post-tool-immediate); SessionStart and Stop are no-ops.
#
# The harness is chosen in this order:
#   1. VELVET_GLOVE_HARNESS=claude|codex, when set;
#   2. codex, when both PLUGIN_ROOT and PLUGIN_DATA are set (Codex injects this
#      pair into plugin hooks; Claude Code never sets these names);
#   3. claude otherwise (Claude Code plugin hooks get CLAUDE_PLUGIN_ROOT and
#      CLAUDECODE=1). Hooks registered by hand in Codex, outside the plugin,
#      receive no Codex-specific variables and must set VELVET_GLOVE_HARNESS.
#
# VELVET_GLOVE_BIN overrides the executable (default: velvet-glove on PATH).

set -u

usage_error() {
    echo "velvet-glove plugin: $1" >&2
    exit 64
}

event=${1:-}
case "$event" in
    post-tool | session-start-state | turn-completion | post-tool-immediate) ;;
    *) usage_error "unsupported hook command: $event" ;;
esac

case ${VELVET_GLOVE_MODE:-deferred} in
    deferred) command=$event ;;
    immediate)
        case "$event" in
            post-tool | post-tool-immediate) command=post-tool-immediate ;;
            *) command= ;; # SessionStart and Stop do nothing in immediate mode.
        esac
        ;;
    *) usage_error "VELVET_GLOVE_MODE must be 'deferred' or 'immediate'" ;;
esac

case ${VELVET_GLOVE_HARNESS:-} in
    claude | codex) harness=$VELVET_GLOVE_HARNESS ;;
    "")
        if [ -n "${PLUGIN_ROOT:-}" ] && [ -n "${PLUGIN_DATA:-}" ]; then
            harness=codex
        else
            harness=claude
        fi
        ;;
    *) usage_error "VELVET_GLOVE_HARNESS must be 'claude' or 'codex'" ;;
esac

velvet_glove_bin=${VELVET_GLOVE_BIN:-velvet-glove}
if command -v "$velvet_glove_bin" >/dev/null 2>&1; then
    if [ -n "$command" ]; then
        exec "$velvet_glove_bin" --harness "$harness" "$command"
    fi
elif [ "$event" = session-start-state ]; then
    echo "velvet-glove plugin: '$velvet_glove_bin' is not available; hooks are inactive. See https://github.com/plx/velvet-glove#install" >&2
fi

if [ "$event" = turn-completion ]; then
    # Stop hooks require JSON on successful no-op exits in both harnesses.
    printf '{}\n'
fi

exit 0
