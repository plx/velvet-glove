#!/bin/sh
set -eu

: "${VELVET_GLOVE_FIXTURE_PROBE_DIR:?missing probe directory}"
: "${VELVET_GLOVE_FIXTURE_PROBE_SENTINEL:?missing probe sentinel}"

invocations="$VELVET_GLOVE_FIXTURE_PROBE_DIR/invocations"
/bin/mkdir -p "$invocations"
index=1
while ! /bin/mkdir "$invocations/$index" 2>/dev/null; do
  index=$((index + 1))
done
record="$invocations/$index"

printf '%s' "$0" > "$record/program"
pwd -P > "$record/cwd"
printf '%s' "$VELVET_GLOVE_FIXTURE_PROBE_SENTINEL" > "$record/sentinel"
printf '%s' "$#" > "$record/argc"

argument_index=0
for argument in "$@"; do
  printf '%s' "$argument" > "$record/argv-$argument_index"
  argument_index=$((argument_index + 1))
done
