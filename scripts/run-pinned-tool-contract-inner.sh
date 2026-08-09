#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "error: internal pinned contract runner received invalid arguments" >&2
  exit 2
fi

repository_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
provisioning_dir="$repository_root/crates/hookkit-pkl-config/validation/provisioning"
registry="$provisioning_dir/recipes.json"
cache_helpers="$repository_root/scripts/pinned-tool-cache.sh"
state_dir=$1
artifact_dir=$2
selection=$3
mise_version=$4

# shellcheck source=scripts/pinned-tool-cache.sh
source "$cache_helpers"

export HOME=${HOME:?controlled HOME is required}
export LANG=C
export LC_ALL=C
export TZ=UTC
export TERM=dumb
export NO_COLOR=1
export CLICOLOR=0
export FORCE_COLOR=0
export TMPDIR="$HOME/tmp"
export XDG_CACHE_HOME="$HOME/.cache"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_STATE_HOME="$HOME/.local/state"
export CARGO_HOME="$state_dir/cargo-home"
export CARGO_NET_OFFLINE=true
export CARGO_TARGET_DIR="$state_dir/cargo-target"
export DYLD_LIBRARY_PATH="$state_dir/rust-toolchain-1.90.0/lib"
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_SYSTEM=/dev/null
export NPM_CONFIG_USERCONFIG=/dev/null
export NODE_PATH="$state_dir/node/node_modules"
export ASTRO_TELEMETRY_DISABLED=1
cargo_clippy_toolchain_identity=$(pinned_component_install_identity \
  jq "$registry" cargo-clippy-toolchain)
export VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT
VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT=$(pinned_component_cache_root \
  "$state_dir" cargo-clippy-toolchain-1.97.1 "$cargo_clippy_toolchain_identity")
export VELVET_GLOVE_FIXTURE_PRETTIER_ROOT="$state_dir/prettier-environment-node-24.19.0-prettier-3.9.6"
export PIP_CONFIG_FILE=/dev/null
export BUNDLE_APP_CONFIG="$state_dir/ruby-contract-asciidoctor-2.0.26-rubocop-1.30.1/config"
export BUNDLE_CACHE_PATH="$state_dir/ruby-contract-asciidoctor-2.0.26-rubocop-1.30.1/cache"
export BUNDLE_DEPLOYMENT=1
export BUNDLE_FROZEN=1
export BUNDLE_GEMFILE="$provisioning_dir/ruby/Gemfile"
export BUNDLE_PATH__SYSTEM=true
export BUNDLE_USER_HOME="$state_dir/ruby-contract-asciidoctor-2.0.26-rubocop-1.30.1/user"
export PATH="$state_dir/betterleaks-1.7.3-vg1/bin:$state_dir/ruby-contract-asciidoctor-2.0.26-rubocop-1.30.1/bin:$state_dir/ruby-runtime-3.4.10-asciidoctor-2.0.26-rubocop-1.30.1/bin:$state_dir/rustfmt-1.8.0/bin:$state_dir/rust-toolchain-1.90.0/bin:$state_dir/node/node_modules/.bin:$state_dir/python-venv/bin:$PATH"

mkdir -p "$artifact_dir" "$artifact_dir/fixtures" "$CARGO_TARGET_DIR" "$TMPDIR"
observed_file="$TMPDIR/observed-versions.jsonl"
resolved_file="$TMPDIR/resolved-executables.jsonl"
lock_digest_file="$TMPDIR/dependency-lock-digests.jsonl"
: >"$observed_file"
: >"$resolved_file"
: >"$lock_digest_file"

network_error=
set +e
network_error=$(python -c 'import socket; s = socket.socket(socket.AF_INET, socket.SOCK_STREAM); s.connect(("127.0.0.1", 1))' 2>&1)
network_status=$?
set -e
if [[ $network_status -eq 0 ]]; then
  echo "error: the pinned contract sandbox allowed an outbound AF_INET connection" >&2
  exit 1
fi
case $network_error in
  *"Operation not permitted"*|*"Permission denied"*) ;;
  *)
    echo "error: network denial probe failed for an unexpected reason: $network_error" >&2
    exit 1
    ;;
esac
echo "network denial probe: pass"

tool_ids=$(jq -cn --arg selection "$selection" '$selection | split(",") | map(split("/")[0])')
rust_197_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("cargo-clippy") != null or index("cargo-fmt") != null' >/dev/null; then
  rust_197_selected=true
fi
prettier_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("prettier") != null' >/dev/null; then
  prettier_selected=true
fi
shared_node_selected=false
if jq -e --argjson tools "$tool_ids" '
  any(.recipes[];
      (.toolId as $tool | $tools | index($tool)) != null
      and .environmentId == "macos-arm64-node")' "$registry" >/dev/null; then
  shared_node_selected=true
fi
prettier_node="$VELVET_GLOVE_FIXTURE_PRETTIER_ROOT/node/bin/node"
prettier_npm_cli="$VELVET_GLOVE_FIXTURE_PRETTIER_ROOT/node/lib/node_modules/npm/bin/npm-cli.js"
prettier_cli="$VELVET_GLOVE_FIXTURE_PRETTIER_ROOT/package/node_modules/prettier/bin/prettier.cjs"

while IFS= read -r program; do
  resolved=
  if [[ $rust_197_selected == true ]]; then
    case $program in
      cargo | cargo-clippy | cargo-fmt | clippy-driver | rustc | rustdoc | rustfmt)
        resolved="$VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT/bin/$program"
        ;;
    esac
  fi
  if [[ -z $resolved && $prettier_selected == true ]]; then
    case $program in
      prettier-node)
        resolved="$prettier_node"
        ;;
      prettier-npm)
        resolved="$prettier_npm_cli"
        ;;
      prettier)
        resolved="$prettier_cli"
        ;;
      node)
        if [[ $shared_node_selected == false ]]; then
          resolved="$prettier_node"
        fi
        ;;
    esac
  fi
  if [[ -z $resolved ]]; then
    resolved=$(type -P "$program" || true)
  fi
  if [[ -z $resolved ]]; then
    echo "error: declared executable is unavailable: $program" >&2
    exit 1
  fi
  resolved_real=$(python -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$resolved")
  resolution_kind=managed
  case $resolved_real in
    "$state_dir"/*|"${MISE_DATA_DIR:?}"/*) ;;
    *)
      if jq -e --arg path "$resolved_real" \
        --argjson tools "$tool_ids" \
        '([.recipes[] | select(.toolId as $tool | $tools | index($tool)) | .environmentId]
          | unique) as $environmentIds
         | [(.sharedComponents[],
              (.environments[]
               | select(.id as $id | $environmentIds | index($id))
               | .components[]))
            | select(.integrity.kind == "host-program")
            | .integrity.path]
         | index($path) != null' "$registry" >/dev/null; then
        resolution_kind=declared-host-prerequisite
      else
        echo "error: $program resolves outside controlled roots: $resolved -> $resolved_real" >&2
        exit 1
      fi
      ;;
  esac
  jq -cn \
    --arg program "$program" \
    --arg path "$resolved" \
    --arg realPath "$resolved_real" \
    --arg kind "$resolution_kind" \
    '{program: $program, path: $path, realPath: $realPath, kind: $kind}' >>"$resolved_file"
done < <(jq -r --argjson tools "$tool_ids" '
  ([.recipes[] | select(.toolId as $tool | $tools | index($tool)) | .environmentId]
   | unique) as $environmentIds
  | (["cargo"]
     + [.sharedComponents[].probe.argv[0]]
     + [.environments[]
        | select(.id as $id | $environmentIds | index($id))
        | .components[].probe.argv[0]]
     + [.environments[]
        | select(.id as $id | $environmentIds | index($id))
        | .auxiliaryPrograms[]]
     + [.recipes[]
        | select(.toolId as $tool | $tools | index($tool))
        | .caseExecutables[]])
  | unique[]' "$registry")

while IFS= read -r probe; do
  owner=$(printf '%s\n' "$probe" | jq -r '.owner')
  match_kind=$(printf '%s\n' "$probe" | jq -r '.probe.match')
  expected=$(printf '%s\n' "$probe" | jq -r '.probe.expected')
  probe_argv=()
  rust_197_probe=false
  prettier_probe=false
  while IFS= read -r argument; do
    probe_argv+=("$argument")
  done < <(printf '%s\n' "$probe" | jq -r '.probe.argv[]')
  if [[ $rust_197_selected == true ]]; then
    case $owner in
      cargo-clippy-toolchain)
        probe_argv[0]="$VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT/bin/rustc"
        rust_197_probe=true
        ;;
      cargo-clippy-cargo)
        probe_argv[0]="$VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT/bin/cargo"
        rust_197_probe=true
        ;;
      clippy | cargo-clippy)
        probe_argv[0]="$VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT/bin/clippy-driver"
        rust_197_probe=true
        ;;
      cargo-fmt-driver | cargo-fmt)
        probe_argv[0]="$VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT/bin/cargo-fmt"
        rust_197_probe=true
        ;;
      cargo-fmt-rustfmt)
        probe_argv[0]="$VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT/bin/rustfmt"
        rust_197_probe=true
        ;;
    esac
  fi
  if [[ $prettier_selected == true ]]; then
    case $owner in
      prettier-node)
        probe_argv=("$prettier_node" "${probe_argv[@]:1}")
        prettier_probe=true
        ;;
      prettier-npm)
        probe_argv=("$prettier_node" "$prettier_npm_cli" "${probe_argv[@]:1}")
        prettier_probe=true
        ;;
      prettier)
        probe_argv=("$prettier_node" "$prettier_cli" "${probe_argv[@]:1}")
        prettier_probe=true
        ;;
    esac
  fi
  set +e
  if [[ $rust_197_probe == true ]]; then
    observed=$(env "DYLD_LIBRARY_PATH=$VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT/lib" "${probe_argv[@]}" 2>&1)
  elif [[ $prettier_probe == true ]]; then
    observed=$(env -i \
      "HOME=$HOME" \
      "USER=${USER:-runner}" \
      "LANG=C" \
      "LC_ALL=C" \
      "TZ=UTC" \
      "TERM=dumb" \
      "PATH=/usr/bin:/bin" \
      "NO_COLOR=1" \
      "CLICOLOR=0" \
      "FORCE_COLOR=0" \
      "${probe_argv[@]}" 2>&1)
  else
    observed=$("${probe_argv[@]}" 2>&1)
  fi
  probe_status=$?
  set -e
  if [[ $probe_status -ne 0 ]]; then
    echo "error: version probe failed for $owner: ${probe_argv[*]}" >&2
    echo "$observed" >&2
    exit 1
  fi
  case $match_kind in
    exact)
      [[ $observed == "$expected" ]] || {
        echo "error: $owner version mismatch: expected '$expected', observed '$observed'" >&2
        exit 1
      }
      ;;
    prefix)
      [[ $observed == "$expected"* ]] || {
        echo "error: $owner version mismatch: expected prefix '$expected', observed '$observed'" >&2
        exit 1
      }
      ;;
    major-at-least)
      observed_first_line=${observed%%$'\n'*}
      observed_major=$(printf '%s\n' "$observed_first_line" | sed -E 's/^[^0-9]*([0-9]+).*/\1/')
      if [[ ! $expected =~ ^[0-9]+$ || ! $observed_major =~ ^[0-9]+$ || $observed_major -lt $expected ]]; then
        echo "error: $owner version mismatch: expected major >=$expected, observed '$observed'" >&2
        exit 1
      fi
      ;;
    *)
      echo "error: unsupported version probe match for $owner: $match_kind" >&2
      exit 1
      ;;
  esac
  echo "version probe: $owner = ${observed%%$'\n'*}"
  jq -cn \
    --arg owner "$owner" \
    --argjson argv "$(printf '%s\n' "$probe" | jq -c '.probe.argv')" \
    --arg observed "$observed" \
    '{owner: $owner, argv: $argv, observed: $observed}' >>"$observed_file"
done < <(jq -c --argjson tools "$tool_ids" '
  [.sharedComponents[] | {owner: .id, probe: .probe}]
  + ([.recipes[] | select(.toolId as $tool | $tools | index($tool)) | .environmentId] as $environmentIds
     | [.environments[]
        | select(.id as $id | $environmentIds | index($id))
        | .components[]
        | {owner: .id, probe: .probe}])
  + [.recipes[]
     | select(.toolId as $tool | $tools | index($tool))
     | {owner: .toolId, probe: .probe}]
  | unique_by([.owner, .probe.argv])[]' "$registry")

export VELVET_GLOVE_FIXTURE_SELECTION="$selection"
export VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS=all
export VELVET_GLOVE_FIXTURE_ARTIFACT_DIR="$artifact_dir/fixtures"
cd /
"$state_dir/rust-toolchain-1.90.0/bin/cargo" test --locked --offline -p velvet-glove --test tool_fixtures \
  --manifest-path "$repository_root/Cargo.toml" \
  run_all_tool_fixtures -- --ignored --exact --nocapture

fixture_report="$artifact_dir/fixtures/report.json"
if [[ ! -f $fixture_report ]]; then
  echo "error: fixture lane did not emit its stable machine-readable report" >&2
  exit 1
fi
fixture_report_sha256=$(shasum -a 256 "$fixture_report" | awk '{print $1}')

lock_sha256=$(shasum -a 256 "$provisioning_dir/mise.lock" | awk '{print $1}')
generated_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
os_version=$(sw_vers -productVersion)
recipe_ids=$(jq -c --argjson tools "$tool_ids" \
  '[.recipes[] | select(.toolId as $tool | $tools | index($tool)) | .id]' "$registry")
while IFS= read -r lock_path; do
  case $lock_path in
    /* | *..*)
      echo "error: unsafe dependency lock path in recipe registry: $lock_path" >&2
      exit 1
      ;;
  esac
  if [[ ! -f $repository_root/$lock_path ]]; then
    echo "error: declared dependency lock is unavailable: $lock_path" >&2
    exit 1
  fi
  lock_digest=$(shasum -a 256 "$repository_root/$lock_path" | awk '{print $1}')
  jq -cn --arg path "$lock_path" --arg sha256 "$lock_digest" \
    '{path: $path, sha256: $sha256}' >>"$lock_digest_file"
done < <(jq -r --argjson tools "$tool_ids" '
  ([.recipes[] | select(.toolId as $tool | $tools | index($tool)) | .environmentId]
   | unique) as $environmentIds
  | ([.mise.lock]
     + [.sharedBootstrap[].lockfile]
     + [.sharedComponents[]
        | select(.integrity.kind != "host-program")
        | (.integrity.path, .integrity.moduleManifestPath, .integrity.moduleLockPath)]
     + [.environments[]
        | select(.id as $id | $environmentIds | index($id))
        | (.components[]
           | select(.integrity.kind != "host-program")
           | (.integrity.path, .integrity.moduleManifestPath, .integrity.moduleLockPath)),
          .bootstrap[].lockfile]
     + [.recipes[]
        | select(.toolId as $tool | $tools | index($tool))
        | (.integrity.path, .integrity.moduleManifestPath, .integrity.moduleLockPath)])
  | map(select(. != null))
  | unique[]' "$registry")
artifact_digests=$(jq -c --argjson tools "$tool_ids" '
  ([.recipes[] | select(.toolId as $tool | $tools | index($tool)) | .environmentId]
   | unique) as $environmentIds
  | [(.sharedComponents[],
      (.environments[]
       | select(.id as $id | $environmentIds | index($id))
       | .components[]))
     | select(.integrity.kind == "sha256-archive" or .integrity.kind == "go-source-build")
     | {
         componentId: .id,
         version: .version,
         url: .integrity.url,
         sha256: .integrity.sha256,
         patchSha256: .integrity.patchSha256,
         moduleManifestSha256: .integrity.moduleManifestSha256,
         moduleLockSha256: .integrity.moduleLockSha256,
         builtArtifactSha256: .integrity.builtArtifactSha256,
         buildToolchainComponentId: .integrity.buildToolchainComponentId
       }
       | with_entries(select(.value != null))]
  | unique_by(.componentId)' "$registry")
jq -n \
  --arg schemaVersion "1" \
  --arg generatedAt "$generated_at" \
  --arg selection "$selection" \
  --arg miseVersion "$mise_version" \
  --arg miseLockSha256 "$lock_sha256" \
  --arg osVersion "$os_version" \
  --arg osVersionConstraint ">=26" \
  --arg architecture "$(uname -m)" \
  --arg cargoTargetDir "$CARGO_TARGET_DIR" \
  --arg fixtureReportSha256 "$fixture_report_sha256" \
  --argjson recipeIds "$recipe_ids" \
  --argjson artifactDigests "$artifact_digests" \
  --slurpfile observedVersions "$observed_file" \
  --slurpfile resolvedExecutables "$resolved_file" \
  --slurpfile dependencyLockDigests "$lock_digest_file" \
  '{
    schemaVersion: ($schemaVersion | tonumber),
    generatedAt: $generatedAt,
    selection: ($selection | split(",")),
    recipeIds: $recipeIds,
    platform: "macos",
    osVersion: $osVersion,
    osVersionConstraint: $osVersionConstraint,
    architecture: $architecture,
    miseVersion: $miseVersion,
    miseLockSha256: $miseLockSha256,
    controlledEnvironment: true,
    neutralTemporaryHome: true,
    cargoInvocationDirectory: "/",
    cargoTargetDir: $cargoTargetDir,
    fixtureReport: {
      path: "fixtures/report.json",
      sha256: $fixtureReportSha256
    },
    sandboxBackend: "mise-deny-net",
    activeNetworkDenial: true,
    artifactDigests: $artifactDigests,
    dependencyLockDigests: $dependencyLockDigests,
    observedVersions: $observedVersions,
    resolvedExecutables: $resolvedExecutables,
    outcome: "passed"
  }' >"$artifact_dir/pinned-environment.json.tmp"
mv "$artifact_dir/pinned-environment.json.tmp" "$artifact_dir/pinned-environment.json"
