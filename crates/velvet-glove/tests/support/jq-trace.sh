#!/bin/sh
set -eu

: "${VELVET_GLOVE_JQ_TRACE_DIR:?missing jq trace directory}"
: "${VELVET_GLOVE_JQ_REAL_PROGRAM:?missing real jq program}"
: "${VELVET_GLOVE_JQ_LOGICAL_PROGRAM:?missing logical jq program}"
: "${VELVET_GLOVE_JQ_TRACE_SENTINEL:?missing jq trace sentinel}"

invocations_dir="$VELVET_GLOVE_JQ_TRACE_DIR/invocations"
/bin/mkdir -p "$invocations_dir"

index=1
while ! /bin/mkdir "$invocations_dir/$(printf '%04d' "$index")" 2>/dev/null; do
  index=$((index + 1))
done
record="$invocations_dir/$(printf '%04d' "$index")"

printf '%s\n' "$0" >"$record/program"
printf '%s\n' "$VELVET_GLOVE_JQ_LOGICAL_PROGRAM" >"$record/logical-program"
printf '%s\n' "$VELVET_GLOVE_JQ_REAL_PROGRAM" >"$record/real-program"
pwd -P >"$record/cwd"
printf '%s\n' "$#" >"$record/argc"
printf '%s\n' "${LANG-}" >"$record/env-LANG"
printf '%s\n' "${LC_ALL-}" >"$record/env-LC_ALL"
printf '%s\n' "${TZ-}" >"$record/env-TZ"
printf '%s\n' "${NO_COLOR-}" >"$record/env-NO_COLOR"
printf '%s\n' "${CLICOLOR-}" >"$record/env-CLICOLOR"
printf '%s\n' "${FORCE_COLOR-}" >"$record/env-FORCE_COLOR"
printf '%s\n' "$VELVET_GLOVE_JQ_TRACE_SENTINEL" >"$record/env-VELVET_GLOVE_JQ_TRACE_SENTINEL"

argument_index=0
for argument in "$@"; do
  printf '%s\n' "$argument" >"$record/argv-$argument_index"
  argument_index=$((argument_index + 1))
done

set +e
"$VELVET_GLOVE_JQ_REAL_PROGRAM" "$@"
status=$?
set -e
printf '%s\n' "$status" >"$record/status"
exit "$status"
