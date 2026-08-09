#!/bin/sh
set -eu

: "${VELVET_GLOVE_TOOL_TRACE_DIR:?missing tool trace directory}"
: "${VELVET_GLOVE_TOOL_REAL_PROGRAM:?missing real tool program}"
: "${VELVET_GLOVE_TOOL_LOGICAL_PROGRAM:?missing logical tool program}"
: "${VELVET_GLOVE_TOOL_TRACE_SENTINEL:?missing tool trace sentinel}"

invocations_dir="$VELVET_GLOVE_TOOL_TRACE_DIR/invocations"
/bin/mkdir -p "$invocations_dir"

index=1
while ! /bin/mkdir "$invocations_dir/$(printf '%04d' "$index")" 2>/dev/null; do
  index=$((index + 1))
done
record="$invocations_dir/$(printf '%04d' "$index")"

printf '%s\n' "$0" >"$record/program"
printf '%s\n' "$VELVET_GLOVE_TOOL_LOGICAL_PROGRAM" >"$record/logical-program"
printf '%s\n' "$VELVET_GLOVE_TOOL_REAL_PROGRAM" >"$record/real-program"
printf '%s\n' 'pass-through' >"$record/execution"
pwd -P >"$record/cwd"
printf '%s\n' "$#" >"$record/argc"
printf '%s\n' "${LANG-}" >"$record/env-LANG"
printf '%s\n' "${LC_ALL-}" >"$record/env-LC_ALL"
printf '%s\n' "${TZ-}" >"$record/env-TZ"
printf '%s\n' "${NO_COLOR-}" >"$record/env-NO_COLOR"
printf '%s\n' "${CLICOLOR-}" >"$record/env-CLICOLOR"
printf '%s\n' "${FORCE_COLOR-}" >"$record/env-FORCE_COLOR"
printf '%s\n' "${NODE_PATH-}" >"$record/env-NODE_PATH"
printf '%s\n' "${ASTRO_TELEMETRY_DISABLED-}" >"$record/env-ASTRO_TELEMETRY_DISABLED"
printf '%s\n' "${CI-}" >"$record/env-CI"
printf '%s\n' "${DEBUG-}" >"$record/env-DEBUG"
printf '%s\n' "${BETTERLEAKS_CONFIG-}" >"$record/env-BETTERLEAKS_CONFIG"
printf '%s\n' "${BETTERLEAKS_CONFIG_TOML-}" >"$record/env-BETTERLEAKS_CONFIG_TOML"
printf '%s\n' "${GITLEAKS_CONFIG-}" >"$record/env-GITLEAKS_CONFIG"
printf '%s\n' "${GITLEAKS_CONFIG_TOML-}" >"$record/env-GITLEAKS_CONFIG_TOML"
printf '%s\n' "${BIOME_BINARY-}" >"$record/env-BIOME_BINARY"
printf '%s\n' "${BIOME_THREADS-}" >"$record/env-BIOME_THREADS"
printf '%s\n' "${RAYON_NUM_THREADS-}" >"$record/env-RAYON_NUM_THREADS"
printf '%s\n' "${NODE_OPTIONS-}" >"$record/env-NODE_OPTIONS"
printf '%s\n' "${BIOME_CONFIG_PATH-}" >"$record/env-BIOME_CONFIG_PATH"
printf '%s\n' "${BIOME_LOG_FILE-}" >"$record/env-BIOME_LOG_FILE"
printf '%s\n' "${BIOME_LOG_PREFIX_NAME-}" >"$record/env-BIOME_LOG_PREFIX_NAME"
printf '%s\n' "${BIOME_LOG_PATH-}" >"$record/env-BIOME_LOG_PATH"
printf '%s\n' "${BIOME_LOG_LEVEL-}" >"$record/env-BIOME_LOG_LEVEL"
printf '%s\n' "${BIOME_LOG_KIND-}" >"$record/env-BIOME_LOG_KIND"
printf '%s\n' "${RUST_LOG-}" >"$record/env-RUST_LOG"
printf '%s\n' "${RUST_BACKTRACE-}" >"$record/env-RUST_BACKTRACE"
printf '%s\n' "${RUST_LIB_BACKTRACE-}" >"$record/env-RUST_LIB_BACKTRACE"
printf '%s\n' "$VELVET_GLOVE_TOOL_TRACE_SENTINEL" >"$record/env-VELVET_GLOVE_TOOL_TRACE_SENTINEL"

argument_index=0
for argument in "$@"; do
  printf '%s\n' "$argument" >"$record/argv-$argument_index"
  argument_index=$((argument_index + 1))
done

set +e
"$VELVET_GLOVE_TOOL_REAL_PROGRAM" "$@"
status=$?
set -e
printf '%s\n' "$status" >"$record/status"
exit "$status"
