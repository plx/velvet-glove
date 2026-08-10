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
export VELVET_GLOVE_FIXTURE_CONTEXTLINT_ROOT="$state_dir/contextlint-environment-node-24.19.0-contextlint-1.1.1"
export VELVET_GLOVE_FIXTURE_DCLINT_ROOT="$state_dir/dclint-environment-node-24.19.0-dclint-3.1.0"
errcheck_identity=$(jq -ce '
  first(.recipes[] | select(.id == "errcheck-macos-arm64"))
  | {id, toolId, version, installationSource, integrity}' "$registry")
errcheck_root=$(pinned_component_cache_root \
  "$state_dir" errcheck-1.20.0 "$errcheck_identity")
errcheck_bin="$errcheck_root/bin/errcheck"
errcheck_expected_sha256=$(printf '%s\n' "$errcheck_identity" | \
  jq -r '.integrity.builtArtifactSha256')
if [[ $errcheck_expected_sha256 != \
    4f369aeb1bd8454d6ebb6789fedd948ef216fe04c6be629d5016aca78908aa0c || \
  $(printf '%s\n' "$errcheck_identity" | jq -r '.integrity.sha256') != \
    50dbdc1e07128552bda3dad27dfaad9dca100d16869bf58485fe05ed4a45f0b6 || \
  $(printf '%s\n' "$errcheck_identity" | \
    jq -r '.integrity.buildToolchainComponentId') != errcheck-go ]]; then
  echo "error: errcheck recipe does not cross-link the reviewed proxy, artifact, and Go identity" >&2
  exit 1
fi
errcheck_path_prefix=
if [[ ,$selection, == *,errcheck/* ]]; then
  errcheck_path_prefix="$errcheck_root/bin:"
fi
goimports_identity=$(jq -ce '
  first(.recipes[] | select(.id == "goimports-macos-arm64"))
  | {id, toolId, version, installationSource, integrity}' "$registry")
goimports_root=$(pinned_component_cache_root \
  "$state_dir" goimports-0.48.0 "$goimports_identity")
goimports_bin="$goimports_root/bin/goimports"
goimports_expected_sha256=$(printf '%s\n' "$goimports_identity" | \
  jq -r '.integrity.builtArtifactSha256')
if [[ $goimports_expected_sha256 != \
    2d7d2892651e4452091f0fe8e280c7b6e14f3b6964854516fd7372442d57fd27 || \
  $(printf '%s\n' "$goimports_identity" | jq -r '.integrity.sha256') != \
    8529e7bd696890fd79d3e1c37c7d1a3e2e26fb4b392b5beebfa7134ad2f65755 || \
  $(printf '%s\n' "$goimports_identity" | \
    jq -r '.integrity.buildToolchainComponentId') != goimports-go ]]; then
  echo "error: goimports recipe does not cross-link the reviewed proxy, artifact, and Go identity" >&2
  exit 1
fi
goimports_path_prefix=
if [[ ,$selection, == *,goimports/* ]]; then
  goimports_path_prefix="$goimports_root/bin:"
fi
golines_identity=$(jq -ce '
  first(.recipes[] | select(.id == "golines-macos-arm64"))
  | {id, toolId, version, installationSource, integrity}' "$registry")
golines_root=$(pinned_component_cache_root \
  "$state_dir" golines-0.13.0-vg1 "$golines_identity")
golines_bin="$golines_root/bin/golines"
golines_expected_sha256=$(printf '%s\n' "$golines_identity" | \
  jq -r '.integrity.builtArtifactSha256')
if [[ $golines_expected_sha256 != \
    4d7bf2a59b9b48bfc234078498b3ddf6a412cf9bd0ce525945bb19d558f6ab75 || \
  $(printf '%s\n' "$golines_identity" | jq -r '.integrity.sha256') != \
    ec1933e0fb73cf0517fd007d325603007aa65ce430267a70fc78cfea43d9716e || \
  $(printf '%s\n' "$golines_identity" | jq -r '.integrity.patchSha256') != \
    c4a7fcf96b2f1a83440e824340e6d51e15ed34630415e044781a780fc7a2a4d3 || \
  $(printf '%s\n' "$golines_identity" | \
    jq -r '.integrity.buildToolchainComponentId') != golines-go ]]; then
  echo "error: golines recipe does not cross-link the reviewed source, patch, artifact, and Go identity" >&2
  exit 1
fi
golines_path_prefix=
if [[ ,$selection, == *,golines/* ]]; then
  golines_path_prefix="$golines_root/bin:"
fi
vacuum_provenance_path="$provisioning_dir/vacuum/provenance.json"
vacuum_identity=$(pinned_component_provenance_identity \
  jq \
  "$registry" \
  vacuum \
  "$vacuum_provenance_path" \
  crates/hookkit-pkl-config/validation/provisioning/vacuum/provenance.json)
vacuum_provenance_sha256=$(printf '%s\n' "$vacuum_identity" | \
  jq -r '.provenance.sha256')
vacuum_root=$(pinned_component_cache_root \
  "$state_dir" vacuum-0.30.0 "$vacuum_identity")
vacuum_bin="$vacuum_root/bin/vacuum"
vacuum_path_prefix=
if [[ ,$selection, == *,vacuum/* ]]; then
  vacuum_path_prefix="$vacuum_root/bin:"
fi
export VELVET_GLOVE_FIXTURE_ESLINT_ROOT="$state_dir/eslint-environment-node-24.19.0-eslint-10.8.1"
ghalint_path_prefix=
if [[ ,$selection, == *,ghalint-workflow/* ]]; then
  ghalint_path_prefix="$state_dir/ghalint-1.5.6-vg1/bin:"
fi
export PIP_CONFIG_FILE=/dev/null
export BUNDLE_APP_CONFIG="$state_dir/ruby-contract-asciidoctor-2.0.26-rubocop-1.30.1/config"
export BUNDLE_CACHE_PATH="$state_dir/ruby-contract-asciidoctor-2.0.26-rubocop-1.30.1/cache"
export BUNDLE_DEPLOYMENT=1
export BUNDLE_FROZEN=1
export BUNDLE_GEMFILE="$provisioning_dir/ruby/Gemfile"
export BUNDLE_PATH__SYSTEM=true
export BUNDLE_USER_HOME="$state_dir/ruby-contract-asciidoctor-2.0.26-rubocop-1.30.1/user"
export PATH="${ghalint_path_prefix}${vacuum_path_prefix}${errcheck_path_prefix}${goimports_path_prefix}${golines_path_prefix}$state_dir/betterleaks-1.7.3-vg1/bin:$state_dir/ruby-contract-asciidoctor-2.0.26-rubocop-1.30.1/bin:$state_dir/ruby-runtime-3.4.10-asciidoctor-2.0.26-rubocop-1.30.1/bin:$state_dir/rustfmt-1.8.0/bin:$state_dir/rust-toolchain-1.90.0/bin:$state_dir/node/node_modules/.bin:$state_dir/python-venv/bin:$PATH"

errcheck_expected_metadata_body=$'\tpath\tgithub.com/kisielk/errcheck\n\tmod\tgithub.com/kisielk/errcheck\tv1.20.0\th1:9rwHBNKzd4wkDWcROy3DvFGNqEPlkxBg305rvk7HabI=\n\tdep\tgolang.org/x/mod\tv0.35.0\th1:Ww1D637e6Pg+Zb2KrWfHQUnH2dQRLBQyAtpr/haaJeM=\n\tdep\tgolang.org/x/sync\tv0.20.0\th1:e0PTpb7pjO8GAtTs2dQ6jYa5BWYlMuX047Dco/pItO4=\n\tdep\tgolang.org/x/tools\tv0.44.0\th1:UP4ajHPIcuMjT1GqzDWRlalUEoY+uzoZKnhOjbIPD2c=\n\tbuild\t-buildmode=exe\n\tbuild\t-compiler=gc\n\tbuild\t-trimpath=true\n\tbuild\tDefaultGODEBUG=cryptocustomrand=1,tlssecpmlkem=0,urlstrictcolons=0\n\tbuild\tCGO_ENABLED=0\n\tbuild\tGOARCH=arm64\n\tbuild\tGOOS=darwin\n\tbuild\tGOARM64=v8.0'

validate_errcheck_metadata() {
  local metadata=$1
  local binary=$2
  if [[ ${metadata%%$'\n'*} != "$binary: go1.26.5" || \
    ${metadata#*$'\n'} != "$errcheck_expected_metadata_body" ]]; then
    echo "error: errcheck artifact module, dependency, or Go 1.26.5 build identity drifted" >&2
    return 1
  fi
}

validate_errcheck_binary() {
  local binary=$1
  local go_binary=$2
  local observed_sha256
  local metadata
  if [[ ! -f $binary || -L $binary || ! -x $binary ]]; then
    echo "error: controlled errcheck artifact is not an executable regular file: $binary" >&2
    return 1
  fi
  read -r observed_sha256 _ < <(/usr/bin/shasum -a 256 "$binary")
  if [[ $observed_sha256 != "$errcheck_expected_sha256" ]]; then
    echo "error: controlled errcheck artifact checksum mismatch" >&2
    return 1
  fi
  metadata=$("$go_binary" version -m "$binary")
  validate_errcheck_metadata "$metadata" "$binary"
}

goimports_expected_metadata_body=$'\tpath\tgolang.org/x/tools/cmd/goimports\n\tmod\tgolang.org/x/tools\tv0.48.0\th1:3+hClM1aLL5mjMKm5ovokw9epgRXPuu2tILgismM6RE=\n\tdep\tgolang.org/x/mod\tv0.38.0\th1:MECBjubtXD7yj4HrhIUcywNaGeNVUdfVnxmPajOk4yk=\n\tdep\tgolang.org/x/sync\tv0.22.0\th1:SZjpbeLmrCk4xhRSZFNZW5gFUeCeFgjekvI/+gfScek=\n\tdep\tgolang.org/x/telemetry\tv0.0.0-20260708182218-49f421fb7959\th1:RJhm5l6Fo4rmEIcndxDllNhhf/fAx8qIm4t6A7vpm2A=\n\tbuild\t-buildmode=exe\n\tbuild\t-compiler=gc\n\tbuild\t-trimpath=true\n\tbuild\tDefaultGODEBUG=cryptocustomrand=1,tlssecpmlkem=0,urlstrictcolons=0\n\tbuild\tCGO_ENABLED=0\n\tbuild\tGOARCH=arm64\n\tbuild\tGOOS=darwin\n\tbuild\tGOARM64=v8.0'

validate_goimports_metadata() {
  local metadata=$1
  local binary=$2
  if [[ ${metadata%%$'\n'*} != "$binary: go1.26.5" || \
    ${metadata#*$'\n'} != "$goimports_expected_metadata_body" ]]; then
    echo "error: goimports artifact module, dependency, or Go 1.26.5 build identity drifted" >&2
    return 1
  fi
}

validate_goimports_binary() {
  local binary=$1
  local go_binary=$2
  local observed_sha256
  local observed_size
  local metadata
  if [[ ! -f $binary || -L $binary || ! -x $binary ]]; then
    echo "error: controlled goimports artifact is not an executable regular file: $binary" >&2
    return 1
  fi
  read -r observed_sha256 _ < <(/usr/bin/shasum -a 256 "$binary")
  observed_size=$(/usr/bin/stat -f '%z' "$binary")
  if [[ $observed_sha256 != "$goimports_expected_sha256" || \
    $observed_size != 5814322 ]]; then
    echo "error: controlled goimports artifact checksum or size mismatch" >&2
    return 1
  fi
  metadata=$("$go_binary" version -m "$binary")
  validate_goimports_metadata "$metadata" "$binary"
}

golines_expected_metadata_body=$'\tpath\tgithub.com/segmentio/golines\n\tmod\tgithub.com/segmentio/golines\t(devel)\t\n\tdep\tgithub.com/alecthomas/kingpin/v2\tv2.4.0\th1:f48lwail6p8zpO1bC4TxtqACaGqHYA22qkHjHpqDjYY=\n\tdep\tgithub.com/alecthomas/units\tv0.0.0-20240927000941-0f3dac36c52b\th1:mimo19zliBX/vSQ6PWWSL9lK8qwHozUj03+zLoEB8O0=\n\tdep\tgithub.com/dave/dst\tv0.27.3\th1:P1HPoMza3cMEquVf9kKy8yXsFirry4zEnWOdYPOoIzY=\n\tdep\tgithub.com/fatih/structtag\tv1.2.0\th1:/OdNE99OxoI/PqaW/SuSK9uxxT3f/tcSZgon/ssNSx4=\n\tdep\tgithub.com/pmezard/go-difflib\tv1.0.0\th1:4DBwDE0NGyQoBHbLQYPwSUPoCMWR5BEzIk/f1lZbAQM=\n\tdep\tgithub.com/sirupsen/logrus\tv1.9.3\th1:dueUQJ1C2q9oE3F7wvmSGAaVtTmUizReu6fjN8uqzbQ=\n\tdep\tgithub.com/xhit/go-str2duration/v2\tv2.1.0\th1:lxklc02Drh6ynqX+DdPyp5pCKLUQpRT8bp8Ydu2Bstc=\n\tdep\tgolang.org/x/mod\tv0.27.0\th1:kb+q2PyFnEADO2IEF935ehFUXlWiNjJWtRNgBLSfbxQ=\n\tdep\tgolang.org/x/sync\tv0.16.0\th1:ycBJEhp9p4vXvUZNszeOq0kGTPghopOL8q0fq3vstxw=\n\tdep\tgolang.org/x/sys\tv0.44.0\th1:ildZl3J4uzeKP07r2F++Op7E9B29JRUy+a27EibtBTQ=\n\tdep\tgolang.org/x/term\tv0.43.0\th1:S4RLU2sB31O/NCl+zFN9Aru9A/Cq2aqKpTZJ6B+DwT4=\n\tdep\tgolang.org/x/tools\tv0.36.0\th1:kWS0uv/zsvHEle1LbV5LE8QujrxB3wfQyxHfhOk0Qkg=\n\tbuild\t-buildmode=exe\n\tbuild\t-compiler=gc\n\tbuild\t-trimpath=true\n\tbuild\tDefaultGODEBUG=cryptocustomrand=1,tlssecpmlkem=0,urlstrictcolons=0\n\tbuild\tCGO_ENABLED=0\n\tbuild\tGOARCH=arm64\n\tbuild\tGOOS=darwin\n\tbuild\tGOARM64=v8.0'

validate_golines_metadata() {
  local metadata=$1
  local binary=$2
  if [[ ${metadata%%$'\n'*} != "$binary: go1.26.5" || \
    ${metadata#*$'\n'} != "$golines_expected_metadata_body" ]]; then
    echo "error: golines artifact module, dependency, or Go 1.26.5 build identity drifted" >&2
    return 1
  fi
}

validate_golines_binary() {
  local binary=$1
  local go_binary=$2
  local observed_sha256
  local observed_size
  local metadata
  if [[ ! -f $binary || -L $binary || ! -x $binary ]]; then
    echo "error: controlled golines artifact is not an executable regular file: $binary" >&2
    return 1
  fi
  read -r observed_sha256 _ < <(/usr/bin/shasum -a 256 "$binary")
  observed_size=$(/usr/bin/stat -f '%z' "$binary")
  if [[ $observed_sha256 != "$golines_expected_sha256" || \
    $observed_size != 7341970 ]]; then
    echo "error: controlled golines artifact checksum or size mismatch" >&2
    return 1
  fi
  metadata=$(env GOENV=off GOWORK=off GOTOOLCHAIN=local \
    "$go_binary" version -m "$binary")
  validate_golines_metadata "$metadata" "$binary"
}

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
contextlint_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("contextlint") != null' >/dev/null; then
  contextlint_selected=true
fi
dclint_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("dclint") != null' >/dev/null; then
  dclint_selected=true
fi
vacuum_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("vacuum") != null' >/dev/null; then
  vacuum_selected=true
fi
eslint_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("eslint") != null' >/dev/null; then
  eslint_selected=true
fi
errcheck_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("errcheck") != null' >/dev/null; then
  errcheck_selected=true
fi
errcheck_go_bin=
if [[ $errcheck_selected == true ]]; then
  if ! pinned_component_cache_valid \
    "$errcheck_root" "$errcheck_identity" bin/errcheck; then
    echo "error: denied-network errcheck root does not match its exact recipe identity" >&2
    exit 1
  fi
  errcheck_go_bin=$(type -P go || true)
  if [[ -z $errcheck_go_bin || ! -f $errcheck_go_bin || \
    -L $errcheck_go_bin || ! -x $errcheck_go_bin ]]; then
    echo "error: denied-network errcheck lane cannot resolve its managed Go toolchain" >&2
    exit 1
  fi
  errcheck_go_real=$(python -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' \
    "$errcheck_go_bin")
  case $errcheck_go_real in
    "${MISE_DATA_DIR:?}"/*) ;;
    *)
      echo "error: denied-network errcheck Go resolves outside the managed mise root" >&2
      exit 1
      ;;
  esac
  if [[ $(env GOTOOLCHAIN=local "$errcheck_go_bin" version) != \
    "go version go1.26.5 darwin/arm64" ]]; then
    echo "error: denied-network errcheck lane is not using exact Go 1.26.5 Darwin arm64" >&2
    exit 1
  fi
  validate_errcheck_binary "$errcheck_bin" "$errcheck_go_bin"
fi
goimports_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("goimports") != null' >/dev/null; then
  goimports_selected=true
fi
goimports_go_bin=
if [[ $goimports_selected == true ]]; then
  if ! pinned_component_cache_valid \
    "$goimports_root" "$goimports_identity" bin/goimports; then
    echo "error: denied-network goimports root does not match its exact recipe identity" >&2
    exit 1
  fi
  goimports_go_bin=$(type -P go || true)
  if [[ -z $goimports_go_bin || ! -f $goimports_go_bin || \
    -L $goimports_go_bin || ! -x $goimports_go_bin ]]; then
    echo "error: denied-network goimports lane cannot resolve its managed Go toolchain" >&2
    exit 1
  fi
  goimports_go_real=$(python -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' \
    "$goimports_go_bin")
  case $goimports_go_real in
    "${MISE_DATA_DIR:?}"/*) ;;
    *)
      echo "error: denied-network goimports Go resolves outside the managed mise root" >&2
      exit 1
      ;;
  esac
  if [[ $(env GOTOOLCHAIN=local "$goimports_go_bin" version) != \
    "go version go1.26.5 darwin/arm64" ]]; then
    echo "error: denied-network goimports lane is not using exact Go 1.26.5 Darwin arm64" >&2
    exit 1
  fi
  validate_goimports_binary "$goimports_bin" "$goimports_go_bin"
fi
golines_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("golines") != null' >/dev/null; then
  golines_selected=true
fi
golines_go_bin=
if [[ $golines_selected == true ]]; then
  if ! pinned_component_cache_valid \
    "$golines_root" "$golines_identity" bin/golines; then
    echo "error: denied-network golines root does not match its exact recipe identity" >&2
    exit 1
  fi
  golines_go_bin=$(type -P go || true)
  if [[ -z $golines_go_bin || ! -f $golines_go_bin || \
    -L $golines_go_bin || ! -x $golines_go_bin ]]; then
    echo "error: denied-network golines lane cannot resolve its managed Go build toolchain" >&2
    exit 1
  fi
  golines_go_real=$(python -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' \
    "$golines_go_bin")
  case $golines_go_real in
    "${MISE_DATA_DIR:?}"/*) ;;
    *)
      echo "error: denied-network golines Go resolves outside the managed mise root" >&2
      exit 1
      ;;
  esac
  if [[ $(env GOENV=off GOWORK=off GOTOOLCHAIN=local "$golines_go_bin" version) != \
    "go version go1.26.5 darwin/arm64" ]]; then
    echo "error: denied-network golines lane is not using exact Go 1.26.5 Darwin arm64" >&2
    exit 1
  fi
  validate_golines_binary "$golines_bin" "$golines_go_bin"
  if [[ $(env -i PATH=/usr/bin:/bin "$golines_bin" --version) != \
    $'golines v0.13.0+velvet-glove.1\n\nbuild information:\n\tbuild date: 2025-08-21T21:22:01Z\n\tgit commit ref: 8f32f0f7e89c30f572c7f2cd3b2a48016b9d8bbf' ]]; then
    echo "error: denied-network golines failed its exact patched-version probe" >&2
    exit 1
  fi
fi
go_vet_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("go-vet") != null' >/dev/null; then
  go_vet_selected=true
fi
go_vet_go_bin=
if [[ $go_vet_selected == true ]]; then
  go_vet_go_bin=$(type -P go || true)
  if [[ -z $go_vet_go_bin || ! -f $go_vet_go_bin || \
    -L $go_vet_go_bin || ! -x $go_vet_go_bin ]]; then
    echo "error: denied-network go-vet lane cannot resolve its managed Go toolchain" >&2
    exit 1
  fi
  go_vet_go_real=$(python -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' \
    "$go_vet_go_bin")
  case $go_vet_go_real in
    "${MISE_DATA_DIR:?}"/*) ;;
    *)
      echo "error: denied-network go-vet Go resolves outside the managed mise root" >&2
      exit 1
      ;;
  esac
  if [[ $(env GOTOOLCHAIN=local "$go_vet_go_bin" version) != \
    "go version go1.26.5 darwin/arm64" ]]; then
    echo "error: denied-network go-vet lane is not using exact Go 1.26.5 Darwin arm64" >&2
    exit 1
  fi
fi
gofumpt_selected=false
if printf '%s\n' "$tool_ids" | jq -e 'index("gofumpt") != null' >/dev/null; then
  gofumpt_selected=true
fi
gofumpt_bin=
gofumpt_go_bin=
if [[ $gofumpt_selected == true ]]; then
  gofumpt_bin=$(type -P gofumpt || true)
  gofumpt_go_bin=$(type -P go || true)
  for binding in "$gofumpt_bin" "$gofumpt_go_bin"; do
    if [[ -z $binding || ! -f $binding || -L $binding || ! -x $binding ]]; then
      echo "error: denied-network gofumpt lane cannot resolve its managed formatter/Go closure" >&2
      exit 1
    fi
    binding_real=$(python -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$binding")
    case $binding_real in
      "${MISE_DATA_DIR:?}"/*) ;;
      *)
        echo "error: denied-network gofumpt closure resolves outside the managed mise root" >&2
        exit 1
        ;;
    esac
  done
  read -r gofumpt_sha256 _ < <(/usr/bin/shasum -a 256 "$gofumpt_bin")
  gofumpt_size=$(/usr/bin/stat -f '%z' "$gofumpt_bin")
  if [[ $gofumpt_sha256 != 18936628f195369a80a129c73ee33d23e39086286dab538781ba826effc7e10b || \
    $gofumpt_size != 3115666 || \
    $(env -i PATH=/usr/bin:/bin "$gofumpt_bin" -version) != "v0.11.0 (go1.26.5)" || \
    $(env -i PATH=/usr/bin:/bin GOTOOLCHAIN=local "$gofumpt_go_bin" version) != \
      "go version go1.26.5 darwin/arm64" ]]; then
    echo "error: denied-network gofumpt closure failed its exact binary identity" >&2
    exit 1
  fi
  gofumpt_metadata=$(env -i PATH=/usr/bin:/bin "$gofumpt_go_bin" version -m "$gofumpt_bin")
  gofumpt_dep_count=$(printf '%s\n' "$gofumpt_metadata" | /usr/bin/awk '$1 == "dep" {count++} END {print count + 0}')
  if [[ $gofumpt_dep_count != 3 || \
    $gofumpt_metadata != *': go1.26.5'* || \
    $gofumpt_metadata != *$'\tpath\tmvdan.cc/gofumpt'* || \
    $gofumpt_metadata != *$'\tmod\tmvdan.cc/gofumpt\tv0.11.0'* || \
    $gofumpt_metadata != *$'\tdep\tgolang.org/x/mod\tv0.38.0\th1:MECBjubtXD7yj4HrhIUcywNaGeNVUdfVnxmPajOk4yk='* || \
    $gofumpt_metadata != *$'\tdep\tgolang.org/x/sync\tv0.22.0\th1:SZjpbeLmrCk4xhRSZFNZW5gFUeCeFgjekvI/+gfScek='* || \
    $gofumpt_metadata != *$'\tdep\tgolang.org/x/tools\tv0.48.0\th1:3+hClM1aLL5mjMKm5ovokw9epgRXPuu2tILgismM6RE='* || \
    $gofumpt_metadata != *$'\tbuild\t-trimpath=true'* || \
    $gofumpt_metadata != *$'\tbuild\tCGO_ENABLED=0'* || \
    $gofumpt_metadata != *$'\tbuild\tGOARCH=arm64'* || \
    $gofumpt_metadata != *$'\tbuild\tGOOS=darwin'* || \
    $gofumpt_metadata != *$'\tbuild\tvcs.revision=5dca7d819315c5c6338d290ad2e7847f07438693'* || \
    $gofumpt_metadata != *$'\tbuild\tvcs.time=2026-07-27T08:46:00Z'* || \
    $gofumpt_metadata != *$'\tbuild\tvcs.modified=false'* ]]; then
    echo "error: denied-network gofumpt build metadata differs from the reviewed official asset" >&2
    exit 1
  fi
fi
shared_node_selected=false
if jq -e --argjson tools "$tool_ids" '
  [.recipes[] as $recipe
   | select(any($tools[]; . == $recipe.toolId))
   | select($recipe.environmentId == "macos-arm64-node")]
  | length > 0' "$registry" >/dev/null; then
  shared_node_selected=true
fi
prettier_node="$VELVET_GLOVE_FIXTURE_PRETTIER_ROOT/node/bin/node"
prettier_npm_cli="$VELVET_GLOVE_FIXTURE_PRETTIER_ROOT/node/lib/node_modules/npm/bin/npm-cli.js"
prettier_cli="$VELVET_GLOVE_FIXTURE_PRETTIER_ROOT/package/node_modules/prettier/bin/prettier.cjs"
contextlint_node="$VELVET_GLOVE_FIXTURE_CONTEXTLINT_ROOT/node/bin/node"
contextlint_npm_cli="$VELVET_GLOVE_FIXTURE_CONTEXTLINT_ROOT/node/lib/node_modules/npm/bin/npm-cli.js"
contextlint_cli="$VELVET_GLOVE_FIXTURE_CONTEXTLINT_ROOT/package/node_modules/@contextlint/cli/dist/index.js"
contextlint_cli_manifest="$VELVET_GLOVE_FIXTURE_CONTEXTLINT_ROOT/package/node_modules/@contextlint/cli/package.json"
dclint_node="$VELVET_GLOVE_FIXTURE_DCLINT_ROOT/node/bin/node"
dclint_npm_cli="$VELVET_GLOVE_FIXTURE_DCLINT_ROOT/node/lib/node_modules/npm/bin/npm-cli.js"
dclint_cli="$VELVET_GLOVE_FIXTURE_DCLINT_ROOT/package/node_modules/.bin/dclint"
eslint_node="$VELVET_GLOVE_FIXTURE_ESLINT_ROOT/node/bin/node"
eslint_npm_cli="$VELVET_GLOVE_FIXTURE_ESLINT_ROOT/node/lib/node_modules/npm/bin/npm-cli.js"
eslint_cli="$VELVET_GLOVE_FIXTURE_ESLINT_ROOT/package/node_modules/eslint/bin/eslint.js"

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
  if [[ -z $resolved && $contextlint_selected == true ]]; then
    case $program in
      contextlint-node)
        resolved="$contextlint_node"
        ;;
      contextlint-npm)
        resolved="$contextlint_npm_cli"
        ;;
      contextlint)
        resolved="$contextlint_cli"
        ;;
      node)
        if [[ $shared_node_selected == false && $prettier_selected == false ]]; then
          resolved="$contextlint_node"
        fi
        ;;
    esac
  fi
  if [[ -z $resolved && $dclint_selected == true ]]; then
    case $program in
      dclint-node)
        resolved="$dclint_node"
        ;;
      dclint-npm)
        resolved="$dclint_npm_cli"
        ;;
      dclint)
        resolved="$dclint_cli"
        ;;
      node)
        if [[ $shared_node_selected == false ]]; then
          resolved="$dclint_node"
        fi
        ;;
    esac
  fi
  if [[ -z $resolved && $vacuum_selected == true && $program == vacuum ]]; then
    resolved="$vacuum_bin"
  fi
  if [[ -z $resolved && $eslint_selected == true ]]; then
    case $program in
      eslint-node)
        resolved="$eslint_node"
        ;;
      eslint-npm)
        resolved="$eslint_npm_cli"
        ;;
      eslint)
        resolved="$eslint_cli"
        ;;
      node)
        if [[ $shared_node_selected == false && $prettier_selected == false && \
          $contextlint_selected == false && $dclint_selected == false ]]; then
          resolved="$eslint_node"
        fi
        ;;
      esac
  fi
  if [[ -z $resolved && $errcheck_selected == true && $program == errcheck ]]; then
    resolved="$errcheck_bin"
  fi
  if [[ -z $resolved && $goimports_selected == true && $program == goimports ]]; then
    resolved="$goimports_bin"
  fi
  if [[ -z $resolved && $golines_selected == true && $program == golines ]]; then
    resolved="$golines_bin"
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
        '([.recipes[] as $recipe
           | select(any($tools[]; . == $recipe.toolId))
           | $recipe.environmentId]
          | unique) as $environmentIds
         | [(.sharedComponents[],
              (.environments[] as $environment
               | select(any($environmentIds[]; . == $environment.id))
               | $environment.components[]))
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
  ([.recipes[] as $recipe
    | select(any($tools[]; . == $recipe.toolId))
    | $recipe.environmentId]
   | unique) as $environmentIds
  | (["cargo"]
     + [.sharedComponents[].probe.argv[0]]
     + [.environments[] as $environment
        | select(any($environmentIds[]; . == $environment.id))
        | $environment.components[].probe.argv[0]]
     + [.environments[] as $environment
        | select(any($environmentIds[]; . == $environment.id))
        | $environment.auxiliaryPrograms[]]
     + [.recipes[] as $recipe
        | select(any($tools[]; . == $recipe.toolId))
        | $recipe.caseExecutables[]])
  | unique[]' "$registry")

while IFS= read -r probe; do
  owner=$(printf '%s\n' "$probe" | jq -r '.owner')
  match_kind=$(printf '%s\n' "$probe" | jq -r '.probe.match')
  expected=$(printf '%s\n' "$probe" | jq -r '.probe.expected')
  probe_argv=()
  rust_197_probe=false
  prettier_probe=false
  contextlint_probe=false
  dclint_probe=false
  eslint_probe=false
  errcheck_probe=false
  goimports_probe=false
  golines_probe=false
  go_vet_probe=false
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
  if [[ $contextlint_selected == true ]]; then
    case $owner in
      contextlint-node)
        probe_argv=("$contextlint_node" "${probe_argv[@]:1}")
        contextlint_probe=true
        ;;
      contextlint-npm)
        probe_argv=("$contextlint_node" "$contextlint_npm_cli" "${probe_argv[@]:1}")
        contextlint_probe=true
        ;;
      contextlint)
        probe_argv=(
          "$contextlint_node"
          -p
          'JSON.parse(require("node:fs").readFileSync(process.argv[1])).version'
          "$contextlint_cli_manifest"
        )
        contextlint_probe=true
        ;;
    esac
  fi
  if [[ $dclint_selected == true ]]; then
    case $owner in
      dclint-node)
        probe_argv=("$dclint_node" "${probe_argv[@]:1}")
        dclint_probe=true
        ;;
      dclint-npm)
        probe_argv=("$dclint_node" "$dclint_npm_cli" "${probe_argv[@]:1}")
        dclint_probe=true
        ;;
      dclint)
        probe_argv=("$dclint_node" "$VELVET_GLOVE_FIXTURE_DCLINT_ROOT/package/node_modules/dclint/bin/dclint.cjs" "${probe_argv[@]:1}")
        dclint_probe=true
        ;;
    esac
  fi
  if [[ $vacuum_selected == true && $owner == vacuum ]]; then
    probe_argv=("$vacuum_bin" "${probe_argv[@]:1}")
  fi
  if [[ $eslint_selected == true ]]; then
    case $owner in
      eslint-node)
        probe_argv=("$eslint_node" "${probe_argv[@]:1}")
        eslint_probe=true
        ;;
      eslint-npm)
        probe_argv=("$eslint_node" "$eslint_npm_cli" "${probe_argv[@]:1}")
        eslint_probe=true
        ;;
      eslint)
        probe_argv=("$eslint_node" "$eslint_cli" "${probe_argv[@]:1}")
        eslint_probe=true
        ;;
      esac
  fi
  if [[ $errcheck_selected == true && $owner == errcheck ]]; then
    probe_argv=("$errcheck_go_bin" version -m "$errcheck_bin")
    errcheck_probe=true
  fi
  if [[ $goimports_selected == true && $owner == goimports ]]; then
    probe_argv=("$goimports_go_bin" version -m "$goimports_bin")
    goimports_probe=true
  fi
  if [[ $golines_selected == true && $owner == golines ]]; then
    probe_argv=("$golines_go_bin" version -m "$golines_bin")
    golines_probe=true
  fi
  if [[ $go_vet_selected == true && $owner == go-vet ]]; then
    probe_argv=("$go_vet_go_bin" version)
    go_vet_probe=true
  fi
  set +e
  if [[ $rust_197_probe == true ]]; then
    observed=$(env "DYLD_LIBRARY_PATH=$VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT/lib" "${probe_argv[@]}" 2>&1)
  elif [[ $go_vet_probe == true ]]; then
    observed=$(env GOTOOLCHAIN=local "${probe_argv[@]}" 2>&1)
  elif [[ $prettier_probe == true || $contextlint_probe == true || $dclint_probe == true || \
    $eslint_probe == true ]]; then
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
  if [[ $errcheck_probe == true ]]; then
    validate_errcheck_metadata "$observed" "$errcheck_bin"
    observed="$expected"
  fi
  if [[ $goimports_probe == true ]]; then
    validate_goimports_metadata "$observed" "$goimports_bin"
    observed="$expected"
  fi
  if [[ $golines_probe == true ]]; then
    validate_golines_metadata "$observed" "$golines_bin"
    if [[ $(env -i PATH=/usr/bin:/bin "$golines_bin" --version) != "$expected" ]]; then
      echo "error: golines exact patched-version probe drifted" >&2
      exit 1
    fi
    observed="$expected"
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
  + ([.recipes[] as $recipe
      | select(any($tools[]; . == $recipe.toolId))
      | $recipe.environmentId] as $environmentIds
     | [.environments[] as $environment
        | select(any($environmentIds[]; . == $environment.id))
        | $environment.components[]
        | {owner: .id, probe: .probe}])
  + [.recipes[] as $recipe
     | select(any($tools[]; . == $recipe.toolId))
     | {owner: $recipe.toolId, probe: $recipe.probe}]
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
  '[.recipes[] as $recipe
    | select(any($tools[]; . == $recipe.toolId))
    | $recipe.id]' "$registry")
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
  ([.recipes[] as $recipe
    | select(any($tools[]; . == $recipe.toolId))
    | $recipe.environmentId]
   | unique) as $environmentIds
  | ([.mise.lock]
     + [.sharedBootstrap[].lockfile]
     + [.sharedComponents[]
        | select(.integrity.kind != "host-program")
        | (.integrity.path, .integrity.moduleManifestPath, .integrity.moduleLockPath)]
     + [.environments[] as $environment
        | select(any($environmentIds[]; . == $environment.id))
        | ($environment.components[]
           | select(.integrity.kind != "host-program")
           | (.integrity.path, .integrity.moduleManifestPath, .integrity.moduleLockPath)),
          $environment.bootstrap[].lockfile]
     + [.recipes[] as $recipe
        | select(any($tools[]; . == $recipe.toolId))
        | ($recipe.integrity.path,
           $recipe.integrity.moduleManifestPath,
           $recipe.integrity.moduleLockPath)])
  | map(select(. != null))
  | unique[]' "$registry")
if printf '%s\n' "$tool_ids" | jq -e 'index("vacuum") != null' >/dev/null; then
  jq -cn \
    --arg path \
      "crates/hookkit-pkl-config/validation/provisioning/vacuum/provenance.json" \
    --arg sha256 "$vacuum_provenance_sha256" \
    '{path: $path, sha256: $sha256}' >>"$lock_digest_file"
fi
artifact_digests=$(jq -c --argjson tools "$tool_ids" '
  . as $registry
  | ([.recipes[] as $recipe
      | select(any($tools[]; . == $recipe.toolId))
      | $recipe.environmentId]
     | unique) as $environmentIds
  | [(
       .sharedComponents[],
       (.environments[] as $environment
        | select(any($environmentIds[]; . == $environment.id))
        | $environment.components[]),
       (.recipes[] as $recipe
        | select(any($tools[]; . == $recipe.toolId))
        | select($recipe.integrity.kind == "go-module-build")
        | $recipe)
     )
     | select(
         .integrity.kind == "sha256-archive"
         or .integrity.kind == "go-source-build"
         or .integrity.kind == "go-module-build"
       )
     | . as $artifact
     | {
         componentId: (.toolId // .id),
         recipeId: (if .toolId then .id else null end),
         version: .version,
         url: .integrity.url,
         sha256: .integrity.sha256,
         patchSha256: .integrity.patchSha256,
         moduleManifestSha256: .integrity.moduleManifestSha256,
         moduleLockSha256: .integrity.moduleLockSha256,
         builtArtifactSha256: .integrity.builtArtifactSha256,
         buildToolchainComponentId: .integrity.buildToolchainComponentId,
         buildToolchainVersion: (
           if .integrity.buildToolchainComponentId then
             .integrity.buildToolchainComponentId as $toolchainId
             | first(
                 ($registry.sharedComponents + [$registry.environments[].components[]])[]
                 | select(.id == $toolchainId)
               ).version
           else null end
         ),
         buildToolchainMiseTool: (
           if .integrity.buildToolchainComponentId then
             .integrity.buildToolchainComponentId as $toolchainId
             | first(
                 ($registry.sharedComponents + [$registry.environments[].components[]])[]
                 | select(.id == $toolchainId)
               ).miseTool
           else null end
         ),
         buildToolchainLockPath: (
           if .integrity.buildToolchainComponentId then
             .integrity.buildToolchainComponentId as $toolchainId
             | first(
                 ($registry.sharedComponents + [$registry.environments[].components[]])[]
                 | select(.id == $toolchainId)
               ).integrity.path
           else null end
         ),
         buildToolchainArtifactSha256: (
           if .integrity.buildToolchainComponentId then
             .integrity.buildToolchainComponentId as $toolchainId
             | first(
                 ($registry.sharedComponents + [$registry.environments[].components[]])[]
                 | select(.id == $toolchainId)
               ).integrity.sha256
           else null end
         )
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
