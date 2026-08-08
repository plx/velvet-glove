#!/bin/sh

set -u

event=${1:-}
case "$event" in
    post-tool | session-start-state | turn-completion) ;;
    *)
        echo "velvet-glove plugin: unsupported hook command: $event" >&2
        exit 64
        ;;
esac

case ${VELVET_GLOVE_HARNESS:-} in
    claude | codex)
        harness=$VELVET_GLOVE_HARNESS
        ;;
    "")
        if [ -n "${PLUGIN_ROOT:-}" ]; then
            harness=codex
        else
            harness=claude
        fi
        ;;
    *)
        echo "velvet-glove plugin: VELVET_GLOVE_HARNESS must be 'claude' or 'codex'" >&2
        exit 64
        ;;
esac

velvet_glove_bin=${VELVET_GLOVE_BIN:-velvet-glove}
if command -v "$velvet_glove_bin" >/dev/null 2>&1; then
    exec "$velvet_glove_bin" --harness "$harness" "$event"
fi

if [ "$event" = session-start-state ]; then
    echo "velvet-glove plugin: '$velvet_glove_bin' is not available; hooks are inactive. See https://github.com/plx/velvet-glove#install" >&2
elif [ "$event" = turn-completion ]; then
    # Stop hooks require JSON on successful no-op exits in both harnesses.
    printf '{}\n'
fi

exit 0
