#!/usr/bin/env bash
# Generate one "[tool v2]" tracking issue per enabled builtin tool spec.
#
# Usage:
#   scripts/generate-tool-validation-issues.sh            # dry run: print titles
#   scripts/generate-tool-validation-issues.sh --create   # create missing issues
#
# Environment:
#   MILESTONE  target milestone title (default: "Tool validation v2")
#   LABEL      label to apply (default: "tool-contract")
#
# Tools that already have an issue with the same title (open or closed) are
# skipped, so the script is idempotent. Issue bodies encode the v2 contract;
# see docs/validation-architecture.md. This script is a guarded path: changes
# require the guardrail-change label.

set -euo pipefail

MILESTONE="${MILESTONE:-Tool validation v2}"
LABEL="${LABEL:-tool-contract}"
MODE="${1:---dry-run}"

root="$(git rev-parse --show-toplevel)"
tools_dir="$root/crates/hookkit-pkl-config/src/builtins/tools"

body_for() {
  local id="$1" spec="$2"
  cat <<EOF
Validate the \`$id\` builtin under the v2 contract. Read
[\`docs/validation-architecture.md\`](../blob/main/docs/validation-architecture.md)
first; match the closest archetype worked example in structure and scale.

Spec: \`$spec\`

## Tier 1 (mandatory, hermetic)

- [ ] Rendered-command golden assertions for every phase and workflow command.
- [ ] Fixture cases: \`clean\`, one representative issue case, and
      \`operational-failure\` (misclassification-proof). Add \`multi-file\`
      only if batch/workspace behavior differs materially.
- [ ] For fixers: \`expected/\` post-state on the issue case.

## Tier 2 (real tool, scheduled lane)

- [ ] If the tool installs via mise/npm/pip/cargo/gem on hosted runners, add
      it to the scheduled real-tool lane with a loose version pin.
- [ ] Otherwise add a documented skip with the reason. A skip is an
      acceptable steady state.

## Constraints

- Budget guidance: ~300 changed lines total. Spec stays ≤ 200 lines; fixture
  caps are enforced by \`tests/guardrails.rs\`.
- Spec changes only to fix a bug a fixture demonstrates.
- A prior validation attempt may exist at tag \`archive/tool-validation-v1\`:
  mine its fixture inputs and semantic findings per the salvage protocol; do
  not port its adapters, pins, or provisioning.
- If a guardrail fails, stop and open an issue instead of adjusting anything.
EOF
}

created=0 skipped=0
for spec in "$tools_dir"/*.pkl; do
  if grep -q 'enabled = false' "$spec"; then
    continue
  fi
  id="$(grep -m1 -oE 'id = "[^"]+"' "$spec" | cut -d'"' -f2)"
  if [ -z "$id" ]; then
    echo "warning: no id found in $spec" >&2
    continue
  fi
  title="[tool v2] $id: declarative contract + fixtures"
  if [ "$MODE" != "--create" ]; then
    echo "$title"
    continue
  fi
  existing="$(gh issue list --state all --search "\"$title\" in:title" \
    --json title --jq "[.[] | select(.title == \"$title\")] | length")"
  if [ "$existing" != "0" ]; then
    skipped=$((skipped + 1))
    continue
  fi
  rel="${spec#"$root"/}"
  gh issue create --title "$title" --label "$LABEL" --milestone "$MILESTONE" \
    --body "$(body_for "$id" "$rel")" >/dev/null
  created=$((created + 1))
  echo "created: $title"
  sleep 2
done

if [ "$MODE" = "--create" ]; then
  echo "done: $created created, $skipped already existed"
fi
