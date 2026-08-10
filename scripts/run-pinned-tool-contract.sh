#!/usr/bin/env bash
# shellcheck disable=SC2016 # Dollar-prefixed names in single quotes are jq variables.
set -euo pipefail

repository_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
provisioning_dir="$repository_root/crates/hookkit-pkl-config/validation/provisioning"
registry="$provisioning_dir/recipes.json"
inner_runner="$repository_root/scripts/run-pinned-tool-contract-inner.sh"
cache_helpers="$repository_root/scripts/pinned-tool-cache.sh"
state_dir=${VELVET_GLOVE_PINNED_TOOL_STATE_DIR:-"$repository_root/target/pinned-tool-environments"}
artifact_dir=${VELVET_GLOVE_PINNED_TOOL_ARTIFACT_DIR:-"$state_dir/artifacts"}

# shellcheck source=scripts/pinned-tool-cache.sh
source "$cache_helpers"

usage() {
  echo "usage: $0 TOOL CASE" >&2
  echo "       $0 --representatives" >&2
  exit 2
}

if [[ $# -eq 1 && $1 == "--representatives" ]]; then
  mode=representatives
  requested_tool=
  requested_case=
elif [[ $# -eq 2 && -n $1 && -n $2 ]]; then
  mode=selected
  requested_tool=$1
  requested_case=$2
else
  usage
fi

if [[ $(uname -s) != Darwin || $(uname -m) != arm64 ]]; then
  echo "error: pinned contract recipes currently require macOS on arm64" >&2
  exit 1
fi
macos_major=$(sw_vers -productVersion | cut -d. -f1)
if [[ ! $macos_major =~ ^[0-9]+$ || $macos_major -lt 26 ]]; then
  echo "error: pinned contract recipes require macOS 26 or newer" >&2
  exit 1
fi

mise_bin=${MISE_BIN:-}
if [[ -z $mise_bin ]]; then
  mise_bin=$(command -v mise || true)
fi
if [[ -z $mise_bin || ! -x $mise_bin ]]; then
  echo "error: mise 2026.5.15 is required (set MISE_BIN to its absolute path)" >&2
  exit 1
fi
mise_version=$("$mise_bin" --version | awk 'NR == 1 { print $1 }')
if [[ $mise_version != 2026.5.15 ]]; then
  echo "error: mise 2026.5.15 is required; observed $mise_version" >&2
  exit 1
fi
export PATH=/usr/bin:/bin

case $state_dir in
  /*) ;;
  *) state_dir="$repository_root/$state_dir" ;;
esac
case $artifact_dir in
  /*) ;;
  *) artifact_dir="$repository_root/$artifact_dir" ;;
esac
mkdir -p "$state_dir" "$artifact_dir"
state_dir=$(CDPATH='' cd -- "$state_dir" && pwd -P)
artifact_dir=$(CDPATH='' cd -- "$artifact_dir" && pwd -P)
run_home=$(mktemp -d "/private/tmp/velvet-glove-pinned.XXXXXX")
rust_extract_dir=
clippy_extract_dir=
prettier_extract_dir=
contextlint_extract_dir=
dclint_extract_dir=
vacuum_extract_dir=
eslint_extract_dir=
ruby_extract_dir=
betterleaks_build_dir=
ghalint_build_dir=
errcheck_build_dir=
goimports_build_dir=
golines_build_dir=
cleanup() {
  case $run_home in
    /private/tmp/velvet-glove-pinned.*) rm -rf -- "$run_home" ;;
  esac
  case $rust_extract_dir in
    "$state_dir"/rust-extract.*) rm -rf -- "$rust_extract_dir" ;;
  esac
  case $clippy_extract_dir in
    "$state_dir"/clippy-extract.*) rm -rf -- "$clippy_extract_dir" ;;
  esac
  case $prettier_extract_dir in
    "$state_dir"/prettier-extract.*) rm -rf -- "$prettier_extract_dir" ;;
  esac
  case $contextlint_extract_dir in
    "$state_dir"/contextlint-extract.*) rm -rf -- "$contextlint_extract_dir" ;;
  esac
  case $dclint_extract_dir in
    "$state_dir"/dclint-extract.*) rm -rf -- "$dclint_extract_dir" ;;
  esac
  case $vacuum_extract_dir in
    "$state_dir"/vacuum-extract.*) rm -rf -- "$vacuum_extract_dir" ;;
  esac
  case $eslint_extract_dir in
    "$state_dir"/eslint-extract.*) rm -rf -- "$eslint_extract_dir" ;;
  esac
  case $ruby_extract_dir in
    "$state_dir"/ruby-extract.*) rm -rf -- "$ruby_extract_dir" ;;
  esac
  case $betterleaks_build_dir in
    "$state_dir"/betterleaks-build-1.7.3-vg1) rm -rf -- "$betterleaks_build_dir" ;;
  esac
  case $ghalint_build_dir in
    "$state_dir"/ghalint-build-1.5.6-vg1) rm -rf -- "$ghalint_build_dir" ;;
  esac
  case $errcheck_build_dir in
    "$state_dir"/errcheck-build-1.20.0) rm -rf -- "$errcheck_build_dir" ;;
  esac
  case $goimports_build_dir in
    "$state_dir"/goimports-build-0.48.0) rm -rf -- "$goimports_build_dir" ;;
  esac
  case $golines_build_dir in
    "$state_dir"/golines-build-0.13.0-vg1) rm -rf -- "$golines_build_dir" ;;
  esac
}
trap cleanup EXIT INT TERM

mkdir -p \
  "$run_home/.cache" \
  "$run_home/.config" \
  "$run_home/.local/state" \
  "$run_home/tmp" \
  "$state_dir/cargo-home" \
  "$state_dir/cargo-target" \
  "$state_dir/downloads" \
  "$state_dir/mise/cache" \
  "$state_dir/mise/config" \
  "$state_dir/mise/data" \
  "$state_dir/mise/state"
: >"$state_dir/mise/config/empty.toml"

case_env=(
  "HOME=$run_home"
  "USER=runner"
  "SHELL=/bin/bash"
  "TERM=dumb"
  "LANG=C"
  "LC_ALL=C"
  "TZ=UTC"
  "TMPDIR=$run_home/tmp"
  "XDG_CACHE_HOME=$run_home/.cache"
  "XDG_CONFIG_HOME=$run_home/.config"
  "XDG_STATE_HOME=$run_home/.local/state"
  "PATH=/usr/bin:/bin"
  "MISE_CACHE_DIR=$state_dir/mise/cache"
  "MISE_CONFIG_DIR=$state_dir/mise/config"
  "MISE_DATA_DIR=$state_dir/mise/data"
  "MISE_STATE_DIR=$state_dir/mise/state"
  "MISE_GLOBAL_CONFIG_FILE=$state_dir/mise/config/empty.toml"
  "MISE_SYSTEM_CONFIG_FILE=$state_dir/mise/config/empty.toml"
  "MISE_TRUSTED_CONFIG_PATHS=$provisioning_dir"
  "MISE_EXPERIMENTAL=1"
  "MISE_LOCKED=1"
  "MISE_NO_HOOKS=1"
  "MISE_AUTO_INSTALL=0"
  "NO_COLOR=1"
  "CLICOLOR=0"
  "FORCE_COLOR=0"
)
provisioning_env=(
  "${case_env[@]}"
  "PATH=/usr/bin:/bin"
)

provision_exec() {
  env -i "${provisioning_env[@]}" \
    "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env -- "$@"
}

install_tools() {
  env -i "${provisioning_env[@]}" \
    "$mise_bin" -C "$provisioning_dir" install --locked "$@"
}

echo "==> Installing the checksum-locked selector parser (provisioning network enabled)"
install_tools jq@1.8.2
jq_root=$(env -i "${provisioning_env[@]}" "$mise_bin" where jq@1.8.2)
jq_bin="$jq_root/jq"
case $jq_bin in
  "$state_dir"/*) ;;
  *)
    echo "error: pinned jq resolved outside the controlled state: $jq_bin" >&2
    exit 1
    ;;
esac
if [[ ! -x $jq_bin ]]; then
  echo "error: pinned jq executable is unavailable: $jq_bin" >&2
  exit 1
fi

fetch_component_archive() {
  local component_id=$1
  local component
  local url
  local expected_sha256
  local archive_format
  local archive
  local partial
  local observed_sha256

  case $component_id in
    *[!a-z0-9-]* | '')
      echo "error: unsafe archive component id: $component_id" >&2
      return 1
      ;;
  esac
  component=$("$jq_bin" -ce --arg id "$component_id" '
    first((.sharedComponents + [.environments[].components[]])[] | select(.id == $id))
    | select(.integrity.kind == "sha256-archive" or .integrity.kind == "go-source-build")' "$registry")
  url=$(printf '%s\n' "$component" | "$jq_bin" -r '.integrity.url')
  expected_sha256=$(printf '%s\n' "$component" | "$jq_bin" -r '.integrity.sha256')
  archive_format=$(printf '%s\n' "$component" | "$jq_bin" -r '.integrity.archiveFormat')
  archive="$state_dir/downloads/$component_id.$archive_format"

  if [[ -f $archive ]]; then
    read -r observed_sha256 _ < <(/usr/bin/shasum -a 256 "$archive")
    if [[ $observed_sha256 != "$expected_sha256" ]]; then
      echo "warning: removing corrupt cached archive for $component_id" >&2
      rm -f -- "$archive"
    fi
  fi
  if [[ ! -f $archive ]]; then
    partial=$(mktemp "$state_dir/downloads/$component_id.partial.XXXXXX")
    echo "==> Downloading $component_id from its versioned release URL" >&2
    if ! env -i "${provisioning_env[@]}" \
      /usr/bin/curl --fail --location --show-error --output "$partial" "$url"; then
      rm -f -- "$partial"
      return 1
    fi
    read -r observed_sha256 _ < <(/usr/bin/shasum -a 256 "$partial")
    if [[ $observed_sha256 != "$expected_sha256" ]]; then
      echo "error: $component_id archive checksum mismatch" >&2
      rm -f -- "$partial"
      return 1
    fi
    mv "$partial" "$archive"
  fi

  printf '%s\n' "$archive"
}

verify_macho_closure() {
  local root=$1
  local owner=$2
  local allowed=false
  local allowed_prefix
  local allowed_prefixes=()
  local candidate
  local dependency
  local dependency_path

  while IFS= read -r allowed_prefix; do
    allowed_prefixes+=("$allowed_prefix")
  done < <("$jq_bin" -r --arg id "$owner" '
    first((.sharedComponents + [.environments[].components[]] + .recipes)[] | select(.id == $id))
    | .integrity.allowedDylibPrefixes[]' "$registry")
  if [[ ${#allowed_prefixes[@]} -eq 0 ]]; then
    echo "error: $owner declares no allowed dynamic-library roots" >&2
    return 1
  fi

  while IFS= read -r -d '' candidate; do
    if [[ $(/usr/bin/file -b "$candidate") != *Mach-O* ]]; then
      continue
    fi
    while IFS= read -r dependency; do
      dependency=${dependency#"${dependency%%[![:space:]]*}"}
      dependency_path=${dependency%% *}
      case $dependency_path in
        @rpath/* | @loader_path/* | @executable_path/* | "$root"/*) continue ;;
      esac
      allowed=false
      for allowed_prefix in "${allowed_prefixes[@]}"; do
        if [[ $dependency_path == "$allowed_prefix"* ]]; then
          allowed=true
          break
        fi
      done
      if [[ $allowed != true ]]; then
        echo "error: $owner links outside its controlled closure: $candidate -> $dependency_path" >&2
        return 1
      fi
    done < <(/usr/bin/otool -L "$candidate" | /usr/bin/sed -n '2,$p')
  done < <(/usr/bin/find "$root" -type f \( -perm -111 -o -name '*.dylib' -o -name '*.bundle' \) -print0)
}

if [[ $mode == representatives ]]; then
  selection=$("$jq_bin" -r \
    '[.recipes[] | "\(.toolId)/\(.representativeCase)"] | join(",")' "$registry")
else
  if ! "$jq_bin" -e \
    --arg tool "$requested_tool" \
    --arg case "$requested_case" \
    '[.recipes[] | select(.toolId == $tool and (.cases | index($case)))]
     | if length == 1 then .[0] else error("unknown pinned tool/case selector: \($tool)/\($case)") end' \
    "$registry" >/dev/null; then
    exit 2
  fi
  selection="$requested_tool/$requested_case"
fi

groups=$("$jq_bin" -r --arg selection "$selection" \
  '($selection | split(",") | map(split("/")[0])) as $tools
   | [.recipes[] as $recipe
      | select(any($tools[]; . == $recipe.toolId))
      | $recipe.environmentId] as $environmentIds
   | [.environments[] as $environment
      | select(any($environmentIds[]; . == $environment.id))
      | $environment.provisioningGroup]
   | unique | join(",")' "$registry")

vacuum_selected=false
if "$jq_bin" -en --arg selection "$selection" \
  '$selection | split(",") | map(split("/")[0]) | index("vacuum") != null' \
  >/dev/null; then
  vacuum_selected=true
fi
errcheck_selected=false
if "$jq_bin" -en --arg selection "$selection" \
  '$selection | split(",") | map(split("/")[0]) | index("errcheck") != null' \
  >/dev/null; then
  errcheck_selected=true
fi
goimports_selected=false
if "$jq_bin" -en --arg selection "$selection" \
  '$selection | split(",") | map(split("/")[0]) | index("goimports") != null' \
  >/dev/null; then
  goimports_selected=true
fi
golines_selected=false
if "$jq_bin" -en --arg selection "$selection" \
  '$selection | split(",") | map(split("/")[0]) | index("golines") != null' \
  >/dev/null; then
  golines_selected=true
fi

tool_specs=()
while IFS= read -r tool_spec; do
  tool_specs+=("$tool_spec")
done < <("$jq_bin" -r --arg selection "$selection" '
  ($selection | split(",") | map(split("/")[0])) as $tools
  | ([.recipes[] as $recipe
      | select(any($tools[]; . == $recipe.toolId))
      | $recipe.environmentId]
     | unique) as $environmentIds
  | (.sharedComponents
     + [.environments[] as $environment
        | select(any($environmentIds[]; . == $environment.id))
        | $environment.components[]])
  | map(select(.miseTool != null) | .miseTool)
  | unique[]' "$registry")
if [[ ${#tool_specs[@]} -eq 0 ]]; then
  echo "error: no pinned mise tools declared for $selection" >&2
  exit 1
fi

echo "==> Installing checksum-locked mise tools for $selection"
install_tools "${tool_specs[@]}"

needs_group() {
  case ",$groups," in
    *",$1,"*) return 0 ;;
    *) return 1 ;;
  esac
}

component_integrity_json() {
  local component_id=$1
  "$jq_bin" -c --arg id "$component_id" '
    first((.sharedComponents + [.environments[].components[]])[] | select(.id == $id))
    | {id, version, integrity}' "$registry"
}

recipe_integrity_json() {
  local recipe_id=$1
  "$jq_bin" -ce --arg id "$recipe_id" '
    first(.recipes[] | select(.id == $id))
    | {id, toolId, version, installationSource, integrity}' "$registry"
}

validate_errcheck_binary() {
  local binary=$1
  local go_binary=$2
  local expected_sha256=$3
  local observed_sha256
  local metadata
  local metadata_body
  local expected_metadata_body

  if [[ ! -f $binary || -L $binary || ! -x $binary ]]; then
    echo "error: controlled errcheck artifact is not an executable regular file: $binary" >&2
    return 1
  fi
  read -r observed_sha256 _ < <(/usr/bin/shasum -a 256 "$binary")
  if [[ $observed_sha256 != "$expected_sha256" ]]; then
    echo "error: controlled errcheck artifact checksum mismatch" >&2
    return 1
  fi
  metadata=$(env -i "${provisioning_env[@]}" \
    "PATH=${go_binary%/bin/go}/bin:/usr/bin:/bin" \
    "GOTOOLCHAIN=local" \
    "$go_binary" version -m "$binary")
  if [[ ${metadata%%$'\n'*} != "$binary: go1.26.5" ]]; then
    echo "error: errcheck artifact was not linked by locked Go 1.26.5" >&2
    return 1
  fi
  metadata_body=${metadata#*$'\n'}
  expected_metadata_body=$'\tpath\tgithub.com/kisielk/errcheck\n\tmod\tgithub.com/kisielk/errcheck\tv1.20.0\th1:9rwHBNKzd4wkDWcROy3DvFGNqEPlkxBg305rvk7HabI=\n\tdep\tgolang.org/x/mod\tv0.35.0\th1:Ww1D637e6Pg+Zb2KrWfHQUnH2dQRLBQyAtpr/haaJeM=\n\tdep\tgolang.org/x/sync\tv0.20.0\th1:e0PTpb7pjO8GAtTs2dQ6jYa5BWYlMuX047Dco/pItO4=\n\tdep\tgolang.org/x/tools\tv0.44.0\th1:UP4ajHPIcuMjT1GqzDWRlalUEoY+uzoZKnhOjbIPD2c=\n\tbuild\t-buildmode=exe\n\tbuild\t-compiler=gc\n\tbuild\t-trimpath=true\n\tbuild\tDefaultGODEBUG=cryptocustomrand=1,tlssecpmlkem=0,urlstrictcolons=0\n\tbuild\tCGO_ENABLED=0\n\tbuild\tGOARCH=arm64\n\tbuild\tGOOS=darwin\n\tbuild\tGOARM64=v8.0'
  if [[ $metadata_body != "$expected_metadata_body" ]]; then
    echo "error: errcheck artifact module or build metadata differs from the exact closure" >&2
    return 1
  fi
}

validate_goimports_binary() {
  local binary=$1
  local go_binary=$2
  local expected_sha256=$3
  local observed_sha256
  local observed_size
  local metadata
  local metadata_body
  local expected_metadata_body

  if [[ ! -f $binary || -L $binary || ! -x $binary ]]; then
    echo "error: controlled goimports artifact is not an executable regular file: $binary" >&2
    return 1
  fi
  read -r observed_sha256 _ < <(/usr/bin/shasum -a 256 "$binary")
  observed_size=$(/usr/bin/stat -f '%z' "$binary")
  if [[ $observed_sha256 != "$expected_sha256" || $observed_size != 5814322 ]]; then
    echo "error: controlled goimports artifact checksum or size mismatch" >&2
    return 1
  fi
  metadata=$(env -i "${provisioning_env[@]}" \
    "PATH=${go_binary%/bin/go}/bin:/usr/bin:/bin" \
    "GOTOOLCHAIN=local" \
    "$go_binary" version -m "$binary")
  if [[ ${metadata%%$'\n'*} != "$binary: go1.26.5" ]]; then
    echo "error: goimports artifact was not linked by locked Go 1.26.5" >&2
    return 1
  fi
  metadata_body=${metadata#*$'\n'}
  expected_metadata_body=$'\tpath\tgolang.org/x/tools/cmd/goimports\n\tmod\tgolang.org/x/tools\tv0.48.0\th1:3+hClM1aLL5mjMKm5ovokw9epgRXPuu2tILgismM6RE=\n\tdep\tgolang.org/x/mod\tv0.38.0\th1:MECBjubtXD7yj4HrhIUcywNaGeNVUdfVnxmPajOk4yk=\n\tdep\tgolang.org/x/sync\tv0.22.0\th1:SZjpbeLmrCk4xhRSZFNZW5gFUeCeFgjekvI/+gfScek=\n\tdep\tgolang.org/x/telemetry\tv0.0.0-20260708182218-49f421fb7959\th1:RJhm5l6Fo4rmEIcndxDllNhhf/fAx8qIm4t6A7vpm2A=\n\tbuild\t-buildmode=exe\n\tbuild\t-compiler=gc\n\tbuild\t-trimpath=true\n\tbuild\tDefaultGODEBUG=cryptocustomrand=1,tlssecpmlkem=0,urlstrictcolons=0\n\tbuild\tCGO_ENABLED=0\n\tbuild\tGOARCH=arm64\n\tbuild\tGOOS=darwin\n\tbuild\tGOARM64=v8.0'
  if [[ $metadata_body != "$expected_metadata_body" ]]; then
    echo "error: goimports artifact module or build metadata differs from the exact closure" >&2
    return 1
  fi
}

validate_golines_binary() {
  local binary=$1
  local go_binary=$2
  local expected_sha256=$3
  local expected_version=$4
  local observed_sha256
  local observed_size
  local metadata
  local metadata_body
  local expected_metadata_body

  if [[ ! -f $binary || -L $binary || ! -x $binary ]]; then
    echo "error: controlled golines artifact is not an executable regular file: $binary" >&2
    return 1
  fi
  read -r observed_sha256 _ < <(/usr/bin/shasum -a 256 "$binary")
  observed_size=$(/usr/bin/stat -f '%z' "$binary")
  if [[ $observed_sha256 != "$expected_sha256" || $observed_size != 7341970 ]]; then
    echo "error: controlled golines artifact checksum or size mismatch" >&2
    return 1
  fi
  if [[ $(env -i PATH=/usr/bin:/bin "$binary" --version) != "$expected_version" ]]; then
    echo "error: controlled golines artifact failed its exact patched-version probe" >&2
    return 1
  fi
  metadata=$(env -i "${provisioning_env[@]}" \
    "PATH=${go_binary%/bin/go}/bin:/usr/bin:/bin" \
    "GOENV=off" \
    "GOWORK=off" \
    "GOTOOLCHAIN=local" \
    "$go_binary" version -m "$binary")
  if [[ ${metadata%%$'\n'*} != "$binary: go1.26.5" ]]; then
    echo "error: golines artifact was not linked by locked Go 1.26.5" >&2
    return 1
  fi
  metadata_body=${metadata#*$'\n'}
  expected_metadata_body=$'\tpath\tgithub.com/segmentio/golines\n\tmod\tgithub.com/segmentio/golines\t(devel)\t\n\tdep\tgithub.com/alecthomas/kingpin/v2\tv2.4.0\th1:f48lwail6p8zpO1bC4TxtqACaGqHYA22qkHjHpqDjYY=\n\tdep\tgithub.com/alecthomas/units\tv0.0.0-20240927000941-0f3dac36c52b\th1:mimo19zliBX/vSQ6PWWSL9lK8qwHozUj03+zLoEB8O0=\n\tdep\tgithub.com/dave/dst\tv0.27.3\th1:P1HPoMza3cMEquVf9kKy8yXsFirry4zEnWOdYPOoIzY=\n\tdep\tgithub.com/fatih/structtag\tv1.2.0\th1:/OdNE99OxoI/PqaW/SuSK9uxxT3f/tcSZgon/ssNSx4=\n\tdep\tgithub.com/pmezard/go-difflib\tv1.0.0\th1:4DBwDE0NGyQoBHbLQYPwSUPoCMWR5BEzIk/f1lZbAQM=\n\tdep\tgithub.com/sirupsen/logrus\tv1.9.3\th1:dueUQJ1C2q9oE3F7wvmSGAaVtTmUizReu6fjN8uqzbQ=\n\tdep\tgithub.com/xhit/go-str2duration/v2\tv2.1.0\th1:lxklc02Drh6ynqX+DdPyp5pCKLUQpRT8bp8Ydu2Bstc=\n\tdep\tgolang.org/x/mod\tv0.27.0\th1:kb+q2PyFnEADO2IEF935ehFUXlWiNjJWtRNgBLSfbxQ=\n\tdep\tgolang.org/x/sync\tv0.16.0\th1:ycBJEhp9p4vXvUZNszeOq0kGTPghopOL8q0fq3vstxw=\n\tdep\tgolang.org/x/sys\tv0.44.0\th1:ildZl3J4uzeKP07r2F++Op7E9B29JRUy+a27EibtBTQ=\n\tdep\tgolang.org/x/term\tv0.43.0\th1:S4RLU2sB31O/NCl+zFN9Aru9A/Cq2aqKpTZJ6B+DwT4=\n\tdep\tgolang.org/x/tools\tv0.36.0\th1:kWS0uv/zsvHEle1LbV5LE8QujrxB3wfQyxHfhOk0Qkg=\n\tbuild\t-buildmode=exe\n\tbuild\t-compiler=gc\n\tbuild\t-trimpath=true\n\tbuild\tDefaultGODEBUG=cryptocustomrand=1,tlssecpmlkem=0,urlstrictcolons=0\n\tbuild\tCGO_ENABLED=0\n\tbuild\tGOARCH=arm64\n\tbuild\tGOOS=darwin\n\tbuild\tGOARM64=v8.0'
  if [[ $metadata_body != "$expected_metadata_body" ]]; then
    echo "error: golines artifact module or build metadata differs from the exact closure" >&2
    return 1
  fi
}

rust_archive=$(fetch_component_archive rust)
rust_root="$state_dir/rust-toolchain-1.90.0"
rust_identity=$(component_integrity_json rust)
if [[ -e $rust_root && ! -d $rust_root ]]; then
  echo "error: controlled Rust root is not a directory: $rust_root" >&2
  exit 1
fi
if [[ ! -d $rust_root ]]; then
  echo "==> Installing the checksum-verified Rust archive"
  rust_extract_dir=$(mktemp -d "$state_dir/rust-extract.XXXXXX")
  mkdir -p "$rust_extract_dir/rust"
  /usr/bin/tar -xf "$rust_archive" -C "$rust_extract_dir/rust"
  rust_archive_root=$("$jq_bin" -r '.sharedComponents[] | select(.id == "rust") | .integrity.archiveRoot' "$registry")
  rust_install_root="$rust_extract_dir/install"
  env -i "${provisioning_env[@]}" /bin/bash \
    "$rust_extract_dir/rust/$rust_archive_root/install.sh" \
    --prefix="$rust_install_root" \
    --components=rustc,rust-std-aarch64-apple-darwin,cargo \
    --disable-ldconfig
  printf '%s\n' "$rust_identity" >"$rust_install_root/.velvet-glove-artifacts.json"
  verify_macho_closure "$rust_install_root" rust
  mv "$rust_install_root" "$rust_root"
  rm -rf -- "$rust_extract_dir"
  rust_extract_dir=
fi
if [[ ! -x $rust_root/bin/cargo || ! -x $rust_root/bin/rustc ]]; then
  echo "error: controlled Rust installation is incomplete: $rust_root" >&2
  exit 1
fi
if [[ ! -f $rust_root/.velvet-glove-artifacts.json ]] || \
  [[ $(<"$rust_root/.velvet-glove-artifacts.json") != "$rust_identity" ]]; then
  echo "error: controlled Rust installation does not match the declared archives: $rust_root" >&2
  exit 1
fi
verify_macho_closure "$rust_root" rust

if needs_group rust; then
  rustfmt_archive=$(fetch_component_archive rustfmt)
  rustfmt_root="$state_dir/rustfmt-1.8.0"
  rustfmt_identity=$(component_integrity_json rustfmt)
  if [[ -e $rustfmt_root && ! -d $rustfmt_root ]]; then
    echo "error: controlled rustfmt root is not a directory: $rustfmt_root" >&2
    exit 1
  fi
  if [[ ! -d $rustfmt_root ]]; then
    echo "==> Installing the checksum-verified rustfmt archive"
    rust_extract_dir=$(mktemp -d "$state_dir/rust-extract.XXXXXX")
    /usr/bin/tar -xf "$rustfmt_archive" -C "$rust_extract_dir"
    rustfmt_archive_root=$("$jq_bin" -r '.environments[].components[] | select(.id == "rustfmt") | .integrity.archiveRoot' "$registry")
    rustfmt_install_root="$rust_extract_dir/install"
    env -i "${provisioning_env[@]}" /bin/bash \
      "$rust_extract_dir/$rustfmt_archive_root/install.sh" \
      --prefix="$rustfmt_install_root" \
      --components=rustfmt-preview \
      --disable-ldconfig
    printf '%s\n' "$rustfmt_identity" >"$rustfmt_install_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$rustfmt_install_root" rustfmt
    mv "$rustfmt_install_root" "$rustfmt_root"
    rm -rf -- "$rust_extract_dir"
    rust_extract_dir=
  fi
  if [[ ! -x $rustfmt_root/bin/rustfmt ]]; then
    echo "error: controlled rustfmt installation is incomplete: $rustfmt_root" >&2
    exit 1
  fi
  if [[ ! -f $rustfmt_root/.velvet-glove-artifacts.json ]] || \
    [[ $(<"$rustfmt_root/.velvet-glove-artifacts.json") != "$rustfmt_identity" ]]; then
    echo "error: controlled rustfmt installation does not match the declared archive: $rustfmt_root" >&2
    exit 1
  fi
  verify_macho_closure "$rustfmt_root" rustfmt
  env -i "${provisioning_env[@]}" \
    "DYLD_LIBRARY_PATH=$rust_root/lib" \
    "$rustfmt_root/bin/rustfmt" --version >/dev/null
fi

if needs_group cargo-clippy; then
  clippy_archive=$(fetch_component_archive cargo-clippy-toolchain)
  clippy_identity=$(pinned_component_install_identity \
    "$jq_bin" "$registry" cargo-clippy-toolchain)
  clippy_install_components=$(
    printf '%s\n' "$clippy_identity" | "$jq_bin" -r '.installedComponents | join(",")'
  )
  clippy_root=$(pinned_component_cache_root \
    "$state_dir" cargo-clippy-toolchain-1.97.1 "$clippy_identity")
  clippy_required_executables=(
    bin/cargo
    bin/cargo-clippy
    bin/cargo-fmt
    bin/clippy-driver
    bin/rustc
    bin/rustdoc
    bin/rustfmt
  )
  if [[ -e $clippy_root && ( ! -d $clippy_root || -L $clippy_root ) ]]; then
    echo "error: controlled cargo-clippy toolchain root is not a directory: $clippy_root" >&2
    exit 1
  fi
  if [[ ! -d $clippy_root ]]; then
    echo "==> Installing the checksum-verified Rust/Cargo/Clippy 1.97.1 archive"
    clippy_extract_dir=$(mktemp -d "$state_dir/clippy-extract.XXXXXX")
    /usr/bin/tar -xf "$clippy_archive" -C "$clippy_extract_dir"
    clippy_archive_root=$("$jq_bin" -r '.environments[].components[] | select(.id == "cargo-clippy-toolchain") | .integrity.archiveRoot' "$registry")
    clippy_install_root="$clippy_extract_dir/install"
    env -i "${provisioning_env[@]}" /bin/bash \
      "$clippy_extract_dir/$clippy_archive_root/install.sh" \
      --prefix="$clippy_install_root" \
      --components="$clippy_install_components" \
      --disable-ldconfig
    printf '%s\n' "$clippy_identity" >"$clippy_install_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$clippy_install_root" cargo-clippy-toolchain
    mv "$clippy_install_root" "$clippy_root"
    rm -rf -- "$clippy_extract_dir"
    clippy_extract_dir=
  fi
  if ! pinned_component_cache_valid \
    "$clippy_root" "$clippy_identity" "${clippy_required_executables[@]}"; then
    echo "error: controlled cargo-clippy toolchain does not match the declared archive and installed component set: $clippy_root" >&2
    exit 1
  fi
  verify_macho_closure "$clippy_root" cargo-clippy-toolchain
  env -i "${provisioning_env[@]}" \
    "DYLD_LIBRARY_PATH=$clippy_root/lib" \
    "$clippy_root/bin/rustc" --version >/dev/null
  env -i "${provisioning_env[@]}" \
    "DYLD_LIBRARY_PATH=$clippy_root/lib" \
    "$clippy_root/bin/cargo" --version >/dev/null
  env -i "${provisioning_env[@]}" \
    "DYLD_LIBRARY_PATH=$clippy_root/lib" \
    "$clippy_root/bin/clippy-driver" --version >/dev/null
  env -i "${provisioning_env[@]}" \
    "DYLD_LIBRARY_PATH=$clippy_root/lib" \
    "$clippy_root/bin/cargo-fmt" --version >/dev/null
  env -i "${provisioning_env[@]}" \
    "DYLD_LIBRARY_PATH=$clippy_root/lib" \
    "$clippy_root/bin/rustfmt" --version >/dev/null
fi

echo "==> Fetching the Cargo.lock graph with the pinned Cargo binary"
(
  cd /
  env -i "${provisioning_env[@]}" \
    "CARGO_HOME=$state_dir/cargo-home" \
    "CARGO_TARGET_DIR=$state_dir/cargo-target" \
    "PATH=$rust_root/bin:/usr/bin:/bin" \
    "$rust_root/bin/cargo" fetch --locked --manifest-path "$repository_root/Cargo.toml"
)

if needs_group node; then
  echo "==> Installing the npm integrity-locked Node graph"
  mkdir -p "$state_dir/node"
  cp "$provisioning_dir/node/package.json" "$state_dir/node/package.json"
  cp "$provisioning_dir/node/package-lock.json" "$state_dir/node/package-lock.json"
  provision_exec npm ci --ignore-scripts --prefix "$state_dir/node"
fi

prettier_root="$state_dir/prettier-environment-node-24.19.0-prettier-3.9.6"
if needs_group prettier; then
  prettier_node_archive=$(fetch_component_archive prettier-node)
  prettier_node_identity=$(component_integrity_json prettier-node)
  prettier_npm_identity=$(component_integrity_json prettier-npm)
  prettier_package_json="$provisioning_dir/prettier/package.json"
  prettier_package_lock="$provisioning_dir/prettier/package-lock.json"
  prettier_npm_global_config="$run_home/npm-globalconfig"
  : >"$prettier_npm_global_config"
  read -r prettier_package_sha256 _ < <(/usr/bin/shasum -a 256 "$prettier_package_json")
  read -r prettier_lock_sha256 _ < <(/usr/bin/shasum -a 256 "$prettier_package_lock")
  prettier_identity=$("$jq_bin" -cn \
    --argjson node "$prettier_node_identity" \
    --argjson npm "$prettier_npm_identity" \
    --arg packageSha256 "$prettier_package_sha256" \
    --arg packageLockSha256 "$prettier_lock_sha256" \
    '{node: $node, npm: $npm, prettier: {version: "3.9.6", packageSha256: $packageSha256, packageLockSha256: $packageLockSha256}}')
  if [[ -e $prettier_root && ! -d $prettier_root ]]; then
    echo "error: controlled Prettier environment root is not a directory: $prettier_root" >&2
    exit 1
  fi
  if [[ ! -d $prettier_root ]]; then
    echo "==> Installing the checksum-verified Node 24.19.0 and npm integrity-locked Prettier 3.9.6 closure"
    prettier_extract_dir=$(mktemp -d "$state_dir/prettier-extract.XXXXXX")
    prettier_install_root="$prettier_extract_dir/install"
    mkdir -p "$prettier_install_root/package"
    /usr/bin/tar -xf "$prettier_node_archive" -C "$prettier_extract_dir"
    prettier_archive_root=$(printf '%s\n' "$prettier_node_identity" | \
      "$jq_bin" -r '.integrity.archiveRoot')
    mv "$prettier_extract_dir/$prettier_archive_root" "$prettier_install_root/node"
    cp "$prettier_package_json" "$prettier_install_root/package/package.json"
    cp "$prettier_package_lock" "$prettier_install_root/package/package-lock.json"
    env -i "${provisioning_env[@]}" \
      "NPM_CONFIG_USERCONFIG=/dev/null" \
      "NPM_CONFIG_GLOBALCONFIG=$prettier_npm_global_config" \
      "NPM_CONFIG_CACHE=$state_dir/npm-cache/prettier-3.9.6" \
      "$prettier_install_root/node/bin/node" \
      "$prettier_install_root/node/lib/node_modules/npm/bin/npm-cli.js" \
      ci --ignore-scripts --no-audit --no-fund --prefix "$prettier_install_root/package"
    read -r observed_prettier_package_sha256 _ < <(
      /usr/bin/shasum -a 256 "$prettier_install_root/package/package.json"
    )
    read -r observed_prettier_lock_sha256 _ < <(
      /usr/bin/shasum -a 256 "$prettier_install_root/package/package-lock.json"
    )
    if [[ $observed_prettier_package_sha256 != "$prettier_package_sha256" || \
      $observed_prettier_lock_sha256 != "$prettier_lock_sha256" ]]; then
      echo "error: npm ci changed the exact Prettier package manifest or lock" >&2
      exit 1
    fi
    printf '%s\n' "$prettier_identity" >"$prettier_install_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$prettier_install_root/node" prettier-node
    mv "$prettier_install_root" "$prettier_root"
    rm -rf -- "$prettier_extract_dir"
    prettier_extract_dir=
  fi
  prettier_node="$prettier_root/node/bin/node"
  prettier_npm_cli="$prettier_root/node/lib/node_modules/npm/bin/npm-cli.js"
  prettier_cli="$prettier_root/package/node_modules/prettier/bin/prettier.cjs"
  if [[ ! -x $prettier_node || ! -f $prettier_npm_cli || ! -f $prettier_cli ]]; then
    echo "error: controlled Prettier environment is incomplete: $prettier_root" >&2
    exit 1
  fi
  if [[ ! -f $prettier_root/.velvet-glove-artifacts.json ]] || \
    [[ $(<"$prettier_root/.velvet-glove-artifacts.json") != "$prettier_identity" ]]; then
    echo "error: controlled Prettier environment does not match the declared Node archive and npm lock: $prettier_root" >&2
    exit 1
  fi
  read -r observed_prettier_package_sha256 _ < <(
    /usr/bin/shasum -a 256 "$prettier_root/package/package.json"
  )
  read -r observed_prettier_lock_sha256 _ < <(
    /usr/bin/shasum -a 256 "$prettier_root/package/package-lock.json"
  )
  if [[ $observed_prettier_package_sha256 != "$prettier_package_sha256" || \
    $observed_prettier_lock_sha256 != "$prettier_lock_sha256" ]]; then
    echo "error: controlled Prettier environment manifest or lock digest drifted" >&2
    exit 1
  fi
  if [[ $(find "$prettier_root/package/node_modules" -mindepth 1 -maxdepth 1 -type d ! -name .bin -print | LC_ALL=C sort) != \
    "$prettier_root/package/node_modules/prettier" ]]; then
    echo "error: controlled Prettier npm graph is not the declared one-package closure" >&2
    exit 1
  fi
  if [[ $(readlink "$prettier_root/package/node_modules/.bin/prettier") != \
    "../prettier/bin/prettier.cjs" ]]; then
    echo "error: controlled Prettier npm bin link escapes the declared package" >&2
    exit 1
  fi
  verify_macho_closure "$prettier_root/node" prettier-node
  if [[ $(env -i "${provisioning_env[@]}" "$prettier_node" --version) != "v24.19.0" ]]; then
    echo "error: controlled Prettier Node runtime failed its exact version probe" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" "$prettier_node" "$prettier_npm_cli" --version) != "11.17.0" ]]; then
    echo "error: controlled Prettier npm runtime failed its exact version probe" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" "$prettier_node" "$prettier_cli" --version) != "3.9.6" ]]; then
    echo "error: controlled Prettier CLI failed its exact version probe" >&2
    exit 1
  fi
fi

contextlint_root="$state_dir/contextlint-environment-node-24.19.0-contextlint-1.1.1"
if needs_group contextlint; then
  contextlint_node_archive=$(fetch_component_archive contextlint-node)
  contextlint_node_identity=$(component_integrity_json contextlint-node)
  contextlint_npm_identity=$(component_integrity_json contextlint-npm)
  contextlint_package_json="$provisioning_dir/contextlint/package.json"
  contextlint_package_lock="$provisioning_dir/contextlint/package-lock.json"
  contextlint_npm_global_config="$run_home/npm-globalconfig"
  : >"$contextlint_npm_global_config"
  read -r contextlint_package_sha256 _ < <(/usr/bin/shasum -a 256 "$contextlint_package_json")
  read -r contextlint_lock_sha256 _ < <(/usr/bin/shasum -a 256 "$contextlint_package_lock")
  contextlint_cli_integrity='sha512-QCyjqmdaoanH9L8AduX2jH7vRm2yryHpxroLai0PHHP2lijBTG96UEICCuSIHbkoQ4FXulrokQst5+eTf34v9g=='
  contextlint_core_integrity='sha512-ui2ymL90ZlV260NZD8pgki6fwCUM1bX2wj1LbDy5H4u7w8JyTvxIBORxzhWlklDUmsXf1wVxIZXdbvuRYRsqfQ=='
  if ! "$jq_bin" -e \
    --arg cliIntegrity "$contextlint_cli_integrity" \
    --arg coreIntegrity "$contextlint_core_integrity" '
      .lockfileVersion == 3
      and .packages[""].engines.node == "24.19.0"
      and .packages[""].dependencies == {
        "@contextlint/cli": "1.1.1",
        "@contextlint/core": "1.1.1"
      }
      and .packages["node_modules/@contextlint/cli"].version == "1.1.1"
      and .packages["node_modules/@contextlint/cli"].integrity == $cliIntegrity
      and .packages["node_modules/@contextlint/core"].version == "1.1.1"
      and .packages["node_modules/@contextlint/core"].integrity == $coreIntegrity
    ' "$contextlint_package_lock" >/dev/null; then
    echo "error: committed Contextlint npm lock does not bind the exact CLI/core 1.1.1 graph" >&2
    exit 1
  fi
  contextlint_identity=$("$jq_bin" -cn \
    --argjson node "$contextlint_node_identity" \
    --argjson npm "$contextlint_npm_identity" \
    --arg packageSha256 "$contextlint_package_sha256" \
    --arg packageLockSha256 "$contextlint_lock_sha256" \
    --arg cliIntegrity "$contextlint_cli_integrity" \
    --arg coreIntegrity "$contextlint_core_integrity" \
    '{node: $node, npm: $npm, contextlint: {version: "1.1.1", cliVersion: "1.1.1", coreVersion: "1.1.1", cliIntegrity: $cliIntegrity, coreIntegrity: $coreIntegrity, packageSha256: $packageSha256, packageLockSha256: $packageLockSha256}}')
  if [[ -e $contextlint_root && ! -d $contextlint_root ]]; then
    echo "error: controlled Contextlint environment root is not a directory: $contextlint_root" >&2
    exit 1
  fi
  if [[ ! -d $contextlint_root ]]; then
    echo "==> Installing the checksum-verified Node 24.19.0 and npm integrity-locked Contextlint 1.1.1 closure"
    contextlint_extract_dir=$(mktemp -d "$state_dir/contextlint-extract.XXXXXX")
    contextlint_install_root="$contextlint_extract_dir/install"
    mkdir -p "$contextlint_install_root/package"
    /usr/bin/tar -xf "$contextlint_node_archive" -C "$contextlint_extract_dir"
    contextlint_archive_root=$(printf '%s\n' "$contextlint_node_identity" | \
      "$jq_bin" -r '.integrity.archiveRoot')
    mv "$contextlint_extract_dir/$contextlint_archive_root" "$contextlint_install_root/node"
    cp "$contextlint_package_json" "$contextlint_install_root/package/package.json"
    cp "$contextlint_package_lock" "$contextlint_install_root/package/package-lock.json"
    env -i "${provisioning_env[@]}" \
      "NPM_CONFIG_USERCONFIG=/dev/null" \
      "NPM_CONFIG_GLOBALCONFIG=$contextlint_npm_global_config" \
      "NPM_CONFIG_CACHE=$state_dir/npm-cache/contextlint-1.1.1" \
      "$contextlint_install_root/node/bin/node" \
      "$contextlint_install_root/node/lib/node_modules/npm/bin/npm-cli.js" \
      ci --ignore-scripts --no-audit --no-fund --prefix "$contextlint_install_root/package"
    read -r observed_contextlint_package_sha256 _ < <(
      /usr/bin/shasum -a 256 "$contextlint_install_root/package/package.json"
    )
    read -r observed_contextlint_lock_sha256 _ < <(
      /usr/bin/shasum -a 256 "$contextlint_install_root/package/package-lock.json"
    )
    if [[ $observed_contextlint_package_sha256 != "$contextlint_package_sha256" || \
      $observed_contextlint_lock_sha256 != "$contextlint_lock_sha256" ]]; then
      echo "error: npm ci changed the exact Contextlint package manifest or lock" >&2
      exit 1
    fi
    printf '%s\n' "$contextlint_identity" >"$contextlint_install_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$contextlint_install_root/node" contextlint-node
    mv "$contextlint_install_root" "$contextlint_root"
    rm -rf -- "$contextlint_extract_dir"
    contextlint_extract_dir=
  fi
  contextlint_node="$contextlint_root/node/bin/node"
  contextlint_npm_cli="$contextlint_root/node/lib/node_modules/npm/bin/npm-cli.js"
  contextlint_cli="$contextlint_root/package/node_modules/@contextlint/cli/dist/index.js"
  contextlint_core_manifest="$contextlint_root/package/node_modules/@contextlint/core/package.json"
  if [[ ! -x $contextlint_node || ! -f $contextlint_npm_cli || ! -f $contextlint_cli || \
    ! -f $contextlint_core_manifest ]]; then
    echo "error: controlled Contextlint environment is incomplete: $contextlint_root" >&2
    exit 1
  fi
  if [[ ! -f $contextlint_root/.velvet-glove-artifacts.json ]] || \
    [[ $(<"$contextlint_root/.velvet-glove-artifacts.json") != "$contextlint_identity" ]]; then
    echo "error: controlled Contextlint environment does not match the declared Node archive and npm lock: $contextlint_root" >&2
    exit 1
  fi
  read -r observed_contextlint_package_sha256 _ < <(
    /usr/bin/shasum -a 256 "$contextlint_root/package/package.json"
  )
  read -r observed_contextlint_lock_sha256 _ < <(
    /usr/bin/shasum -a 256 "$contextlint_root/package/package-lock.json"
  )
  if [[ $observed_contextlint_package_sha256 != "$contextlint_package_sha256" || \
    $observed_contextlint_lock_sha256 != "$contextlint_lock_sha256" ]]; then
    echo "error: controlled Contextlint environment manifest or lock digest drifted" >&2
    exit 1
  fi
  if [[ $(readlink "$contextlint_root/package/node_modules/.bin/contextlint") != \
    "../@contextlint/cli/dist/index.js" ]]; then
    echo "error: controlled Contextlint npm bin link escapes the declared CLI package" >&2
    exit 1
  fi
  if ! "$jq_bin" -e '
      .name == "@contextlint/cli"
      and .version == "1.1.1"
      and .type == "module"
      and .bin == {contextlint: "dist/index.js"}
      and .dependencies["@contextlint/core"] == "1.1.1"
    ' "$contextlint_root/package/node_modules/@contextlint/cli/package.json" >/dev/null || \
    ! "$jq_bin" -e '
      .name == "@contextlint/core"
      and .version == "1.1.1"
      and .type == "module"
    ' "$contextlint_core_manifest" >/dev/null; then
    echo "error: controlled Contextlint installed package pair drifted from exact 1.1.1" >&2
    exit 1
  fi
  env -i "${provisioning_env[@]}" \
    "$contextlint_node" "$contextlint_npm_cli" ls --all --prefix "$contextlint_root/package" >/dev/null
  verify_macho_closure "$contextlint_root/node" contextlint-node
  if [[ $(env -i "${provisioning_env[@]}" "$contextlint_node" --version) != "v24.19.0" ]]; then
    echo "error: controlled Contextlint Node runtime failed its exact version probe" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" "$contextlint_node" "$contextlint_npm_cli" --version) != "11.17.0" ]]; then
    echo "error: controlled Contextlint npm runtime failed its exact version probe" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" "$contextlint_node" -p \
    'JSON.parse(require("node:fs").readFileSync(process.argv[1])).version' \
    "$contextlint_root/package/node_modules/@contextlint/cli/package.json") != "1.1.1" ]] || \
    [[ $(env -i "${provisioning_env[@]}" "$contextlint_node" -p \
    'JSON.parse(require("node:fs").readFileSync(process.argv[1])).version' \
    "$contextlint_core_manifest") != "1.1.1" ]]; then
    echo "error: controlled Contextlint CLI/core pair failed its exact version probe" >&2
    exit 1
  fi
fi

dclint_root="$state_dir/dclint-environment-node-24.19.0-dclint-3.1.0"
if needs_group dclint; then
  dclint_node_archive=$(fetch_component_archive dclint-node)
  dclint_node_identity=$(component_integrity_json dclint-node)
  dclint_npm_identity=$(component_integrity_json dclint-npm)
  dclint_package_json="$provisioning_dir/dclint/package.json"
  dclint_package_lock="$provisioning_dir/dclint/package-lock.json"
  dclint_npm_global_config="$run_home/npm-globalconfig"
  : >"$dclint_npm_global_config"
  read -r dclint_package_sha256 _ < <(/usr/bin/shasum -a 256 "$dclint_package_json")
  read -r dclint_lock_sha256 _ < <(/usr/bin/shasum -a 256 "$dclint_package_lock")
  dclint_identity=$("$jq_bin" -cn \
    --argjson node "$dclint_node_identity" \
    --argjson npm "$dclint_npm_identity" \
    --arg packageSha256 "$dclint_package_sha256" \
    --arg packageLockSha256 "$dclint_lock_sha256" \
    '{node: $node, npm: $npm, dclint: {version: "3.1.0", packageSha256: $packageSha256, packageLockSha256: $packageLockSha256}}')
  if [[ -e $dclint_root && ( ! -d $dclint_root || -L $dclint_root ) ]]; then
    echo "error: controlled dclint environment root is not a real directory: $dclint_root" >&2
    exit 1
  fi
  if [[ ! -d $dclint_root ]]; then
    echo "==> Installing the checksum-verified Node 24.19.0 and npm integrity-locked dclint 3.1.0 closure"
    dclint_extract_dir=$(mktemp -d "$state_dir/dclint-extract.XXXXXX")
    dclint_install_root="$dclint_extract_dir/install"
    mkdir -p "$dclint_install_root/package"
    /usr/bin/tar -xf "$dclint_node_archive" -C "$dclint_extract_dir"
    dclint_archive_root=$(printf '%s\n' "$dclint_node_identity" | \
      "$jq_bin" -r '.integrity.archiveRoot')
    mv "$dclint_extract_dir/$dclint_archive_root" "$dclint_install_root/node"
    cp "$dclint_package_json" "$dclint_install_root/package/package.json"
    cp "$dclint_package_lock" "$dclint_install_root/package/package-lock.json"
    env -i "${provisioning_env[@]}" \
      "NPM_CONFIG_USERCONFIG=/dev/null" \
      "NPM_CONFIG_GLOBALCONFIG=$dclint_npm_global_config" \
      "NPM_CONFIG_CACHE=$state_dir/npm-cache/dclint-3.1.0" \
      "$dclint_install_root/node/bin/node" \
      "$dclint_install_root/node/lib/node_modules/npm/bin/npm-cli.js" \
      ci --ignore-scripts --no-audit --no-fund --prefix "$dclint_install_root/package"
    read -r observed_dclint_package_sha256 _ < <(
      /usr/bin/shasum -a 256 "$dclint_install_root/package/package.json"
    )
    read -r observed_dclint_lock_sha256 _ < <(
      /usr/bin/shasum -a 256 "$dclint_install_root/package/package-lock.json"
    )
    if [[ $observed_dclint_package_sha256 != "$dclint_package_sha256" || \
      $observed_dclint_lock_sha256 != "$dclint_lock_sha256" ]]; then
      echo "error: npm ci changed the exact dclint package manifest or lock" >&2
      exit 1
    fi
    printf '%s\n' "$dclint_identity" >"$dclint_install_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$dclint_install_root/node" dclint-node
    mv "$dclint_install_root" "$dclint_root"
    rm -rf -- "$dclint_extract_dir"
    dclint_extract_dir=
  fi
  dclint_node="$dclint_root/node/bin/node"
  dclint_npm_cli="$dclint_root/node/lib/node_modules/npm/bin/npm-cli.js"
  dclint_cli="$dclint_root/package/node_modules/dclint/bin/dclint.cjs"
  dclint_bin_link="$dclint_root/package/node_modules/.bin/dclint"
  if [[ ! -x $dclint_node || ! -f $dclint_npm_cli || ! -f $dclint_cli || \
    ! -x $dclint_bin_link || ! -L $dclint_bin_link ]]; then
    echo "error: controlled dclint environment is incomplete: $dclint_root" >&2
    exit 1
  fi
  if [[ ! -f $dclint_root/.velvet-glove-artifacts.json ]] || \
    [[ $(<"$dclint_root/.velvet-glove-artifacts.json") != "$dclint_identity" ]]; then
    echo "error: controlled dclint environment does not match the declared Node archive and npm lock: $dclint_root" >&2
    exit 1
  fi
  read -r observed_dclint_package_sha256 _ < <(
    /usr/bin/shasum -a 256 "$dclint_root/package/package.json"
  )
  read -r observed_dclint_lock_sha256 _ < <(
    /usr/bin/shasum -a 256 "$dclint_root/package/package-lock.json"
  )
  if [[ $observed_dclint_package_sha256 != "$dclint_package_sha256" || \
    $observed_dclint_lock_sha256 != "$dclint_lock_sha256" ]]; then
    echo "error: controlled dclint environment manifest or lock digest drifted" >&2
    exit 1
  fi
  if [[ $(readlink "$dclint_bin_link") != "../dclint/bin/dclint.cjs" ]]; then
    echo "error: controlled dclint npm bin link escapes the declared package" >&2
    exit 1
  fi
  verify_macho_closure "$dclint_root/node" dclint-node
  if [[ $(env -i "${provisioning_env[@]}" "$dclint_node" --version) != "v24.19.0" ]]; then
    echo "error: controlled dclint Node runtime failed its exact version probe" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" "$dclint_node" "$dclint_npm_cli" --version) != "11.17.0" ]]; then
    echo "error: controlled dclint npm runtime failed its exact version probe" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" "$dclint_node" "$dclint_cli" --version) != "3.1.0" ]]; then
    echo "error: controlled dclint CLI failed its exact version probe" >&2
    exit 1
  fi
fi

if [[ $vacuum_selected == true ]]; then
  vacuum_provenance="$provisioning_dir/vacuum/provenance.json"
  if [[ ! -f $vacuum_provenance || -L $vacuum_provenance ]]; then
    echo "error: Vacuum provenance record is unavailable or not a regular file" >&2
    exit 1
  fi
  vacuum_archive=$(fetch_component_archive vacuum)
  vacuum_identity=$(pinned_component_provenance_identity \
    "$jq_bin" \
    "$registry" \
    vacuum \
    "$vacuum_provenance" \
    crates/hookkit-pkl-config/validation/provisioning/vacuum/provenance.json)
  vacuum_component_identity=$(printf '%s\n' "$vacuum_identity" | \
    "$jq_bin" -c '.component')
  if ! "$jq_bin" -e --argjson component "$vacuum_component_identity" '
    .schemaVersion == 1
    and .release.version == $component.version
    and .archive.url == $component.integrity.url
    and .archive.sha256 == $component.integrity.sha256
    and .darwin.minimumOsVersion == $component.integrity.minOsVersion
    and .darwin.allowedDylibPrefixes == $component.integrity.allowedDylibPrefixes
    and .probe.argv == ["vacuum", "version"]
    and .probe.expected == $component.version' "$vacuum_provenance" >/dev/null; then
    echo "error: Vacuum provenance record disagrees with the pinned component" >&2
    exit 1
  fi
  vacuum_root=$(pinned_component_cache_root \
    "$state_dir" vacuum-0.30.0 "$vacuum_identity")
  if [[ -e $vacuum_root && ( ! -d $vacuum_root || -L $vacuum_root ) ]]; then
    echo "error: controlled Vacuum root is not a directory: $vacuum_root" >&2
    exit 1
  fi
  if [[ ! -d $vacuum_root ]]; then
    echo "==> Installing the checksum-verified Vacuum 0.30.0 archive"
    vacuum_extract_dir=$(mktemp -d "$state_dir/vacuum-extract.XXXXXX")
    vacuum_archive_members=$(/usr/bin/tar -tzf "$vacuum_archive")
    vacuum_expected_members=$("$jq_bin" -r '.archive.members[]' "$vacuum_provenance")
    if [[ $vacuum_archive_members != "$vacuum_expected_members" ]]; then
      echo "error: Vacuum archive members differ from the reviewed release closure" >&2
      exit 1
    fi
    mkdir -p "$vacuum_extract_dir/archive" "$vacuum_extract_dir/install/bin" \
      "$vacuum_extract_dir/install/share"
    /usr/bin/tar -xzf "$vacuum_archive" -C "$vacuum_extract_dir/archive"
    for vacuum_member in LICENSE README.md vacuum; do
      if [[ ! -f $vacuum_extract_dir/archive/$vacuum_member || \
        -L $vacuum_extract_dir/archive/$vacuum_member ]]; then
        echo "error: Vacuum archive member is not a regular file: $vacuum_member" >&2
        exit 1
      fi
    done
    mv "$vacuum_extract_dir/archive/vacuum" "$vacuum_extract_dir/install/bin/vacuum"
    mv "$vacuum_extract_dir/archive/LICENSE" "$vacuum_extract_dir/install/share/LICENSE"
    mv "$vacuum_extract_dir/archive/README.md" "$vacuum_extract_dir/install/README.md"
    chmod 755 "$vacuum_extract_dir/install/bin/vacuum"
    chmod 644 "$vacuum_extract_dir/install/share/LICENSE" \
      "$vacuum_extract_dir/install/README.md"
    printf '%s\n' "$vacuum_identity" \
      >"$vacuum_extract_dir/install/.velvet-glove-artifacts.json"
    verify_macho_closure "$vacuum_extract_dir/install" vacuum
    mv "$vacuum_extract_dir/install" "$vacuum_root"
    rm -rf -- "$vacuum_extract_dir"
    vacuum_extract_dir=
  fi
  if ! pinned_component_cache_valid \
    "$vacuum_root" "$vacuum_identity" bin/vacuum; then
    echo "error: controlled Vacuum installation does not match the declared archive and provenance: $vacuum_root" >&2
    exit 1
  fi
  if [[ ! -f $vacuum_root/share/LICENSE || -L $vacuum_root/share/LICENSE || \
    ! -f $vacuum_root/README.md || -L $vacuum_root/README.md || \
    -n $(/usr/bin/find "$vacuum_root" -type l -print -quit) || \
    $(/usr/bin/find "$vacuum_root" -type f | /usr/bin/wc -l | /usr/bin/tr -d ' ') != 4 || \
    $(/usr/bin/find "$vacuum_root" -type d | /usr/bin/wc -l | /usr/bin/tr -d ' ') != 3 || \
    -n $(/usr/bin/find "$vacuum_root" -mindepth 1 ! -type d ! -type f -print -quit) ]]; then
    echo "error: controlled Vacuum installation has an incomplete or linked closure" >&2
    exit 1
  fi
  vacuum_expected_binary_sha256=$("$jq_bin" -r '.archive.binarySha256' "$vacuum_provenance")
  vacuum_expected_license_sha256=$("$jq_bin" -r '.archive.licenseSha256' "$vacuum_provenance")
  vacuum_expected_readme_sha256=$("$jq_bin" -r '.archive.readmeSha256' "$vacuum_provenance")
  read -r vacuum_observed_binary_sha256 _ < <(
    /usr/bin/shasum -a 256 "$vacuum_root/bin/vacuum"
  )
  read -r vacuum_observed_license_sha256 _ < <(
    /usr/bin/shasum -a 256 "$vacuum_root/share/LICENSE"
  )
  read -r vacuum_observed_readme_sha256 _ < <(
    /usr/bin/shasum -a 256 "$vacuum_root/README.md"
  )
  if [[ $vacuum_observed_binary_sha256 != "$vacuum_expected_binary_sha256" || \
    $vacuum_observed_license_sha256 != "$vacuum_expected_license_sha256" || \
    $vacuum_observed_readme_sha256 != "$vacuum_expected_readme_sha256" ]]; then
    echo "error: controlled Vacuum binary, license, or README digest drifted" >&2
    exit 1
  fi
  if [[ $(/usr/bin/lipo -archs "$vacuum_root/bin/vacuum") != \
    "$("$jq_bin" -r '.darwin.architecture' "$vacuum_provenance")" ]]; then
    echo "error: controlled Vacuum binary is not the reviewed thin arm64 image" >&2
    exit 1
  fi
  vacuum_observed_minos=$(/usr/bin/otool -l "$vacuum_root/bin/vacuum" | \
    /usr/bin/awk '$1 == "cmd" && $2 == "LC_BUILD_VERSION" { found = 1; next }
      found && $1 == "minos" { print $2; exit }')
  if [[ $vacuum_observed_minos != \
    "$("$jq_bin" -r '.darwin.minimumOsVersion' "$vacuum_provenance")" ]]; then
    echo "error: controlled Vacuum binary minimum macOS version drifted" >&2
    exit 1
  fi
  vacuum_codesign_metadata=$(/usr/bin/codesign -dvvv "$vacuum_root/bin/vacuum" 2>&1)
  vacuum_runtime_flag=$("$jq_bin" -r '.darwin.hardenedRuntimeFlag' "$vacuum_provenance")
  vacuum_team_identifier=$("$jq_bin" -r '.darwin.teamIdentifier' "$vacuum_provenance")
  if [[ $vacuum_codesign_metadata != *"Format=Mach-O thin (arm64)"* || \
    $vacuum_codesign_metadata != *"flags=$vacuum_runtime_flag"* || \
    $vacuum_codesign_metadata != *"TeamIdentifier=$vacuum_team_identifier"* ]]; then
    echo "error: controlled Vacuum embedded code-signing metadata drifted" >&2
    exit 1
  fi
  verify_macho_closure "$vacuum_root" vacuum
  set +e
  vacuum_observed_version=$(env -i "${provisioning_env[@]}" \
    "$vacuum_root/bin/vacuum" version 2>&1)
  vacuum_probe_status=$?
  set -e
  if [[ $vacuum_probe_status -ne 0 || $vacuum_observed_version != \
    "$("$jq_bin" -r '.probe.expected' "$vacuum_provenance")" ]]; then
    echo "error: controlled Vacuum binary failed its exact version probe" >&2
    exit 1
  fi
fi

eslint_root="$state_dir/eslint-environment-node-24.19.0-eslint-10.8.1"
if needs_group eslint; then
  eslint_node_archive=$(fetch_component_archive eslint-node)
  eslint_node_identity=$(component_integrity_json eslint-node)
  eslint_npm_identity=$(component_integrity_json eslint-npm)
  eslint_package_json="$provisioning_dir/eslint/package.json"
  eslint_package_lock="$provisioning_dir/eslint/package-lock.json"
  eslint_npm_global_config="$run_home/npm-globalconfig"
  eslint_integrity='sha512-wqA7W2jbsC/BnV9Iv1UZpKVFkO1AdNoSmYW8NWG4HNOBbkAMvIqDZ27pI2f07dqn583NcIC44ckjAcOXDL1QbQ=='
  eslint_shasum='fb37d514c19b6dd5b2d6b70169fd26fddfa97967'
  eslint_git_head='c049dc3c4294da7afe3d920a1a5fdeba388f4983'
  : >"$eslint_npm_global_config"
  read -r eslint_package_sha256 _ < <(/usr/bin/shasum -a 256 "$eslint_package_json")
  read -r eslint_lock_sha256 _ < <(/usr/bin/shasum -a 256 "$eslint_package_lock")
  if ! "$jq_bin" -e --arg integrity "$eslint_integrity" '
      .lockfileVersion == 3
      and .packages[""].engines.node == "24.19.0"
      and .packages[""].engines.npm == "11.17.0"
      and .packages[""].dependencies == {eslint: "10.8.1"}
      and .packages["node_modules/eslint"].version == "10.8.1"
      and .packages["node_modules/eslint"].resolved == "https://registry.npmjs.org/eslint/-/eslint-10.8.1.tgz"
      and .packages["node_modules/eslint"].integrity == $integrity
    ' "$eslint_package_lock" >/dev/null; then
    echo "error: committed ESLint npm lock does not bind the exact 10.8.1 registry graph" >&2
    exit 1
  fi
  eslint_identity=$("$jq_bin" -cn \
    --argjson node "$eslint_node_identity" \
    --argjson npm "$eslint_npm_identity" \
    --arg packageSha256 "$eslint_package_sha256" \
    --arg packageLockSha256 "$eslint_lock_sha256" \
    --arg integrity "$eslint_integrity" \
    --arg shasum "$eslint_shasum" \
    --arg gitHead "$eslint_git_head" \
    '{node: $node, npm: $npm, eslint: {version: "10.8.1", published: "2026-08-07", integrity: $integrity, shasum: $shasum, gitHead: $gitHead, packageSha256: $packageSha256, packageLockSha256: $packageLockSha256}}')
  if [[ -e $eslint_root && ( ! -d $eslint_root || -L $eslint_root ) ]]; then
    echo "error: controlled ESLint environment root is not a real directory: $eslint_root" >&2
    exit 1
  fi
  if [[ ! -d $eslint_root ]]; then
    echo "==> Installing the checksum-verified Node 24.19.0 and npm integrity-locked ESLint 10.8.1 closure"
    eslint_extract_dir=$(mktemp -d "$state_dir/eslint-extract.XXXXXX")
    eslint_install_root="$eslint_extract_dir/install"
    mkdir -p "$eslint_install_root/package"
    /usr/bin/tar -xf "$eslint_node_archive" -C "$eslint_extract_dir"
    eslint_archive_root=$(printf '%s\n' "$eslint_node_identity" | \
      "$jq_bin" -r '.integrity.archiveRoot')
    mv "$eslint_extract_dir/$eslint_archive_root" "$eslint_install_root/node"
    cp "$eslint_package_json" "$eslint_install_root/package/package.json"
    cp "$eslint_package_lock" "$eslint_install_root/package/package-lock.json"
    env -i "${provisioning_env[@]}" \
      "NPM_CONFIG_USERCONFIG=/dev/null" \
      "NPM_CONFIG_GLOBALCONFIG=$eslint_npm_global_config" \
      "NPM_CONFIG_CACHE=$state_dir/npm-cache/eslint-10.8.1" \
      "$eslint_install_root/node/bin/node" \
      "$eslint_install_root/node/lib/node_modules/npm/bin/npm-cli.js" \
      ci --ignore-scripts --no-audit --no-fund --prefix "$eslint_install_root/package"
    read -r observed_eslint_package_sha256 _ < <(
      /usr/bin/shasum -a 256 "$eslint_install_root/package/package.json"
    )
    read -r observed_eslint_lock_sha256 _ < <(
      /usr/bin/shasum -a 256 "$eslint_install_root/package/package-lock.json"
    )
    if [[ $observed_eslint_package_sha256 != "$eslint_package_sha256" || \
      $observed_eslint_lock_sha256 != "$eslint_lock_sha256" ]]; then
      echo "error: npm ci changed the exact ESLint package manifest or lock" >&2
      exit 1
    fi
    printf '%s\n' "$eslint_identity" >"$eslint_install_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$eslint_install_root/node" eslint-node
    mv "$eslint_install_root" "$eslint_root"
    rm -rf -- "$eslint_extract_dir"
    eslint_extract_dir=
  fi
  eslint_node="$eslint_root/node/bin/node"
  eslint_npm_cli="$eslint_root/node/lib/node_modules/npm/bin/npm-cli.js"
  eslint_cli="$eslint_root/package/node_modules/eslint/bin/eslint.js"
  eslint_bin_link="$eslint_root/package/node_modules/.bin/eslint"
  if [[ ! -x $eslint_node || ! -f $eslint_npm_cli || ! -f $eslint_cli || \
    ! -x $eslint_bin_link || ! -L $eslint_bin_link ]]; then
    echo "error: controlled ESLint environment is incomplete: $eslint_root" >&2
    exit 1
  fi
  if [[ ! -f $eslint_root/.velvet-glove-artifacts.json ]] || \
    [[ $(<"$eslint_root/.velvet-glove-artifacts.json") != "$eslint_identity" ]]; then
    echo "error: controlled ESLint environment does not match the declared Node archive and npm lock: $eslint_root" >&2
    exit 1
  fi
  read -r observed_eslint_package_sha256 _ < <(
    /usr/bin/shasum -a 256 "$eslint_root/package/package.json"
  )
  read -r observed_eslint_lock_sha256 _ < <(
    /usr/bin/shasum -a 256 "$eslint_root/package/package-lock.json"
  )
  if [[ $observed_eslint_package_sha256 != "$eslint_package_sha256" || \
    $observed_eslint_lock_sha256 != "$eslint_lock_sha256" ]]; then
    echo "error: controlled ESLint environment manifest or lock digest drifted" >&2
    exit 1
  fi
  if [[ $(readlink "$eslint_bin_link") != "../eslint/bin/eslint.js" ]]; then
    echo "error: controlled ESLint npm bin link escapes the declared package" >&2
    exit 1
  fi
  if [[ $("$jq_bin" -r '.version' "$eslint_root/package/node_modules/eslint/package.json") != "10.8.1" ]]; then
    echo "error: controlled ESLint package manifest failed its exact version check" >&2
    exit 1
  fi
  verify_macho_closure "$eslint_root/node" eslint-node
  if [[ $(env -i "${provisioning_env[@]}" "$eslint_node" --version) != "v24.19.0" ]]; then
    echo "error: controlled ESLint Node runtime failed its exact version probe" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" "$eslint_node" "$eslint_npm_cli" --version) != "11.17.0" ]]; then
    echo "error: controlled ESLint npm runtime failed its exact version probe" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" "$eslint_node" "$eslint_cli" --version) != "v10.8.1" ]]; then
    echo "error: controlled ESLint CLI failed its exact version probe" >&2
    exit 1
  fi
fi

if needs_group python; then
  echo "==> Installing the hash-locked Python wheel graph"
  provision_exec python -m venv --clear "$state_dir/python-venv"
  provision_exec "$state_dir/python-venv/bin/python" -m pip install \
    --require-hashes \
    --only-binary=:all: \
    --requirement "$provisioning_dir/python/requirements-macos-arm64.txt"
fi

if needs_group security; then
  betterleaks_archive=$(fetch_component_archive betterleaks)
  betterleaks_root="$state_dir/betterleaks-1.7.3-vg1"
  betterleaks_identity=$(component_integrity_json betterleaks)
  betterleaks_binary="$betterleaks_root/bin/betterleaks"
  betterleaks_expected_sha256=$(printf '%s\n' "$betterleaks_identity" | \
    "$jq_bin" -r '.integrity.builtArtifactSha256')
  if [[ -e $betterleaks_root && ! -d $betterleaks_root ]]; then
    echo "error: controlled Betterleaks root is not a directory: $betterleaks_root" >&2
    exit 1
  fi
  if [[ ! -d $betterleaks_root ]]; then
    echo "==> Building the checksum-locked Betterleaks source closure"
    betterleaks_build_dir="$state_dir/betterleaks-build-1.7.3-vg1"
    if [[ -e $betterleaks_build_dir ]]; then
      echo "warning: removing stale transactional Betterleaks build directory" >&2
      rm -rf -- "$betterleaks_build_dir"
    fi
    betterleaks_staging_root="$betterleaks_build_dir/install"
    betterleaks_staging_binary="$betterleaks_staging_root/bin/betterleaks"
    mkdir -p "$betterleaks_build_dir" "$betterleaks_staging_root/bin"
    /usr/bin/tar -xf "$betterleaks_archive" -C "$betterleaks_build_dir"
    betterleaks_archive_root=$(printf '%s\n' "$betterleaks_identity" | \
      "$jq_bin" -r '.integrity.archiveRoot')
    betterleaks_source="$betterleaks_build_dir/source"
    mv "$betterleaks_build_dir/$betterleaks_archive_root" "$betterleaks_source"

    betterleaks_manifest_path=$(printf '%s\n' "$betterleaks_identity" | \
      "$jq_bin" -r '.integrity.moduleManifestPath')
    betterleaks_lock_path=$(printf '%s\n' "$betterleaks_identity" | \
      "$jq_bin" -r '.integrity.moduleLockPath')
    betterleaks_manifest_sha256=$(printf '%s\n' "$betterleaks_identity" | \
      "$jq_bin" -r '.integrity.moduleManifestSha256')
    betterleaks_lock_sha256=$(printf '%s\n' "$betterleaks_identity" | \
      "$jq_bin" -r '.integrity.moduleLockSha256')
    betterleaks_patch_path=$(printf '%s\n' "$betterleaks_identity" | \
      "$jq_bin" -r '.integrity.path')
    betterleaks_patch_sha256=$(printf '%s\n' "$betterleaks_identity" | \
      "$jq_bin" -r '.integrity.patchSha256')
    read -r observed_betterleaks_patch_sha256 _ < <(
      /usr/bin/shasum -a 256 "$repository_root/$betterleaks_patch_path"
    )
    if [[ $observed_betterleaks_patch_sha256 != "$betterleaks_patch_sha256" ]]; then
      echo "error: Betterleaks closure patch checksum mismatch" >&2
      exit 1
    fi
    env -i "${provisioning_env[@]}" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      /usr/bin/patch \
        -d "$betterleaks_source" \
        -p1 \
        -i "$repository_root/$betterleaks_patch_path"
    read -r observed_betterleaks_manifest_sha256 _ < <(
      /usr/bin/shasum -a 256 "$betterleaks_source/go.mod"
    )
    read -r observed_betterleaks_lock_sha256 _ < <(
      /usr/bin/shasum -a 256 "$betterleaks_source/go.sum"
    )
    if [[ $observed_betterleaks_manifest_sha256 != "$betterleaks_manifest_sha256" || \
      $observed_betterleaks_lock_sha256 != "$betterleaks_lock_sha256" ]]; then
      echo "error: Betterleaks patched module closure checksum mismatch" >&2
      exit 1
    fi
    if ! /usr/bin/cmp -s "$repository_root/$betterleaks_manifest_path" "$betterleaks_source/go.mod" || \
      ! /usr/bin/cmp -s "$repository_root/$betterleaks_lock_path" "$betterleaks_source/go.sum"; then
      echo "error: Betterleaks applied closure differs from the checked module inputs" >&2
      exit 1
    fi

    go_root=$(env -i "${provisioning_env[@]}" "$mise_bin" where go@1.26.5)
    go_bin="$go_root/bin/go"
    case $go_bin in
      "$state_dir"/*) ;;
      *)
        echo "error: pinned Go resolved outside the controlled state: $go_bin" >&2
        exit 1
        ;;
    esac
    if [[ ! -x $go_bin ]]; then
      echo "error: pinned Go executable is unavailable: $go_bin" >&2
      exit 1
    fi
    mkdir -p "$state_dir/betterleaks-go-mod-cache" "$state_dir/betterleaks-go-build-cache"
    betterleaks_go_env=(
      "${provisioning_env[@]}"
      "PATH=$go_root/bin:/usr/bin:/bin"
      "GOTOOLCHAIN=local"
      "GOFLAGS=-mod=readonly"
      "CGO_ENABLED=0"
      "GOOS=darwin"
      "GOARCH=arm64"
      "SOURCE_DATE_EPOCH=1785516069"
      "GOMODCACHE=$state_dir/betterleaks-go-mod-cache"
      "GOCACHE=$state_dir/betterleaks-go-build-cache"
    )
    env -i "${betterleaks_go_env[@]}" \
      "GOPROXY=https://proxy.golang.org" \
      "GOSUMDB=sum.golang.org" \
      "$go_bin" -C "$betterleaks_source" mod download all
    env -i "${betterleaks_go_env[@]}" \
      "GOPROXY=off" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      "$go_bin" -C "$betterleaks_source" mod verify
    env -i "${betterleaks_go_env[@]}" \
      "GOPROXY=off" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      "$go_bin" -C "$betterleaks_source" build \
          -trimpath \
          -buildvcs=false \
          -ldflags '-s -w -buildid= -X=github.com/betterleaks/betterleaks/version.Version=1.7.3+velvet-glove.1' \
          -o "$betterleaks_staging_binary" \
          .
    read -r observed_betterleaks_sha256 _ < <(
      /usr/bin/shasum -a 256 "$betterleaks_staging_binary"
    )
    if [[ $observed_betterleaks_sha256 != "$betterleaks_expected_sha256" ]]; then
      echo "error: reproducible Betterleaks artifact checksum mismatch" >&2
      exit 1
    fi
    betterleaks_build_metadata=$(
      env -i "${betterleaks_go_env[@]}" "$go_bin" version -m "$betterleaks_staging_binary"
    )
    betterleaks_build_metadata_first_line=${betterleaks_build_metadata%%$'\n'*}
    if [[ $betterleaks_build_metadata_first_line != *': go1.26.5' || \
      $betterleaks_build_metadata != *$'\tdep\tgithub.com/klauspost/compress\tv1.18.7'* || \
      $betterleaks_build_metadata != *$'\tdep\tgolang.org/x/text\tv0.39.0'* ]]; then
      echo "error: Betterleaks binary module metadata does not match the declared closure" >&2
      exit 1
    fi
    printf '%s\n' "$betterleaks_identity" >"$betterleaks_staging_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$betterleaks_staging_root" betterleaks
    mv "$betterleaks_staging_root" "$betterleaks_root"
    rm -rf -- "$betterleaks_build_dir"
    betterleaks_build_dir=
  fi
  if [[ ! -x $betterleaks_binary ]]; then
    echo "error: controlled Betterleaks installation is incomplete: $betterleaks_root" >&2
    exit 1
  fi
  if [[ ! -f $betterleaks_root/.velvet-glove-artifacts.json ]] || \
    [[ $(<"$betterleaks_root/.velvet-glove-artifacts.json") != "$betterleaks_identity" ]]; then
    echo "error: controlled Betterleaks installation does not match the declared source build" >&2
    exit 1
  fi
  read -r observed_betterleaks_sha256 _ < <(/usr/bin/shasum -a 256 "$betterleaks_binary")
  if [[ $observed_betterleaks_sha256 != "$betterleaks_expected_sha256" ]]; then
    echo "error: controlled Betterleaks artifact checksum mismatch" >&2
    exit 1
  fi
  verify_macho_closure "$betterleaks_root" betterleaks
fi

if needs_group github-actions; then
  ghalint_archive=$(fetch_component_archive ghalint-workflow)
  ghalint_root="$state_dir/ghalint-1.5.6-vg1"
  ghalint_identity=$(component_integrity_json ghalint-workflow)
  ghalint_binary="$ghalint_root/bin/ghalint"
  ghalint_expected_sha256=$(printf '%s\n' "$ghalint_identity" | \
    "$jq_bin" -r '.integrity.builtArtifactSha256')
  if [[ -e $ghalint_root && ! -d $ghalint_root ]]; then
    echo "error: controlled ghalint root is not a directory: $ghalint_root" >&2
    exit 1
  fi
  if [[ ! -d $ghalint_root ]]; then
    echo "==> Building the checksum-locked ghalint source closure"
    ghalint_build_dir="$state_dir/ghalint-build-1.5.6-vg1"
    if [[ -e $ghalint_build_dir ]]; then
      echo "warning: removing stale transactional ghalint build directory" >&2
      rm -rf -- "$ghalint_build_dir"
    fi
    ghalint_staging_root="$ghalint_build_dir/install"
    ghalint_staging_binary="$ghalint_staging_root/bin/ghalint"
    mkdir -p "$ghalint_build_dir" "$ghalint_staging_root/bin"
    /usr/bin/tar -xf "$ghalint_archive" -C "$ghalint_build_dir"
    ghalint_archive_root=$(printf '%s\n' "$ghalint_identity" | \
      "$jq_bin" -r '.integrity.archiveRoot')
    ghalint_source="$ghalint_build_dir/source"
    mv "$ghalint_build_dir/$ghalint_archive_root" "$ghalint_source"

    ghalint_manifest_path=$(printf '%s\n' "$ghalint_identity" | \
      "$jq_bin" -r '.integrity.moduleManifestPath')
    ghalint_lock_path=$(printf '%s\n' "$ghalint_identity" | \
      "$jq_bin" -r '.integrity.moduleLockPath')
    ghalint_manifest_sha256=$(printf '%s\n' "$ghalint_identity" | \
      "$jq_bin" -r '.integrity.moduleManifestSha256')
    ghalint_lock_sha256=$(printf '%s\n' "$ghalint_identity" | \
      "$jq_bin" -r '.integrity.moduleLockSha256')
    ghalint_patch_path=$(printf '%s\n' "$ghalint_identity" | \
      "$jq_bin" -r '.integrity.path')
    ghalint_patch_sha256=$(printf '%s\n' "$ghalint_identity" | \
      "$jq_bin" -r '.integrity.patchSha256')
    read -r observed_ghalint_patch_sha256 _ < <(
      /usr/bin/shasum -a 256 "$repository_root/$ghalint_patch_path"
    )
    if [[ $observed_ghalint_patch_sha256 != "$ghalint_patch_sha256" ]]; then
      echo "error: ghalint closure patch checksum mismatch" >&2
      exit 1
    fi
    env -i "${provisioning_env[@]}" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      /usr/bin/patch \
        -d "$ghalint_source" \
        -p1 \
        -i "$repository_root/$ghalint_patch_path"
    read -r observed_ghalint_manifest_sha256 _ < <(
      /usr/bin/shasum -a 256 "$ghalint_source/go.mod"
    )
    read -r observed_ghalint_lock_sha256 _ < <(
      /usr/bin/shasum -a 256 "$ghalint_source/go.sum"
    )
    if [[ $observed_ghalint_manifest_sha256 != "$ghalint_manifest_sha256" || \
      $observed_ghalint_lock_sha256 != "$ghalint_lock_sha256" ]]; then
      echo "error: ghalint patched module closure checksum mismatch" >&2
      exit 1
    fi
    if ! /usr/bin/cmp -s "$repository_root/$ghalint_manifest_path" "$ghalint_source/go.mod" || \
      ! /usr/bin/cmp -s "$repository_root/$ghalint_lock_path" "$ghalint_source/go.sum"; then
      echo "error: ghalint applied closure differs from the checked module inputs" >&2
      exit 1
    fi

    go_root=$(env -i "${provisioning_env[@]}" "$mise_bin" where go@1.26.5)
    go_bin="$go_root/bin/go"
    case $go_bin in
      "$state_dir"/*) ;;
      *)
        echo "error: pinned Go resolved outside the controlled state: $go_bin" >&2
        exit 1
        ;;
    esac
    if [[ ! -x $go_bin ]]; then
      echo "error: pinned Go executable is unavailable: $go_bin" >&2
      exit 1
    fi
    mkdir -p "$state_dir/ghalint-go-mod-cache" "$state_dir/ghalint-go-build-cache"
    ghalint_go_env=(
      "${provisioning_env[@]}"
      "PATH=$go_root/bin:/usr/bin:/bin"
      "GOTOOLCHAIN=local"
      "GOFLAGS=-mod=readonly"
      "CGO_ENABLED=0"
      "GOOS=darwin"
      "GOARCH=arm64"
      "SOURCE_DATE_EPOCH=1777591460"
      "GOMODCACHE=$state_dir/ghalint-go-mod-cache"
      "GOCACHE=$state_dir/ghalint-go-build-cache"
    )
    env -i "${ghalint_go_env[@]}" \
      "GOPROXY=https://proxy.golang.org" \
      "GOSUMDB=sum.golang.org" \
      "$go_bin" -C "$ghalint_source" mod download all
    env -i "${ghalint_go_env[@]}" \
      "GOPROXY=off" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      "$go_bin" -C "$ghalint_source" mod verify
    env -i "${ghalint_go_env[@]}" \
      "GOPROXY=off" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      "$go_bin" -C "$ghalint_source" build \
          -trimpath \
          -buildvcs=false \
          -ldflags '-s -w -buildid= -X=main.version=1.5.6+velvet-glove.1' \
          -o "$ghalint_staging_binary" \
          ./cmd/ghalint
    read -r observed_ghalint_sha256 _ < <(
      /usr/bin/shasum -a 256 "$ghalint_staging_binary"
    )
    if [[ $observed_ghalint_sha256 != "$ghalint_expected_sha256" ]]; then
      echo "error: reproducible ghalint artifact checksum mismatch" >&2
      exit 1
    fi
    ghalint_build_metadata=$(
      env -i "${ghalint_go_env[@]}" "$go_bin" version -m "$ghalint_staging_binary"
    )
    ghalint_build_metadata_first_line=${ghalint_build_metadata%%$'\n'*}
    if [[ $ghalint_build_metadata_first_line != *': go1.26.5' || \
      $ghalint_build_metadata != *$'\tpath\tgithub.com/suzuki-shunsuke/ghalint/cmd/ghalint'* || \
      $ghalint_build_metadata != *$'\tdep\tgolang.org/x/text\tv0.39.0'* || \
      $ghalint_build_metadata != *$'\tbuild\t-trimpath=true'* || \
      $ghalint_build_metadata != *$'\tbuild\tCGO_ENABLED=0'* ]]; then
      echo "error: ghalint binary module metadata does not match the declared closure" >&2
      exit 1
    fi
    printf '%s\n' "$ghalint_identity" >"$ghalint_staging_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$ghalint_staging_root" ghalint-workflow
    mv "$ghalint_staging_root" "$ghalint_root"
    rm -rf -- "$ghalint_build_dir"
    ghalint_build_dir=
  fi
  if [[ ! -x $ghalint_binary ]]; then
    echo "error: controlled ghalint installation is incomplete: $ghalint_root" >&2
    exit 1
  fi
  if [[ ! -f $ghalint_root/.velvet-glove-artifacts.json ]] || \
    [[ $(<"$ghalint_root/.velvet-glove-artifacts.json") != "$ghalint_identity" ]]; then
    echo "error: controlled ghalint installation does not match the declared source build" >&2
    exit 1
  fi
  read -r observed_ghalint_sha256 _ < <(/usr/bin/shasum -a 256 "$ghalint_binary")
  if [[ $observed_ghalint_sha256 != "$ghalint_expected_sha256" ]]; then
    echo "error: controlled ghalint artifact checksum mismatch" >&2
    exit 1
  fi
  if [[ $("$ghalint_binary" --version) != "ghalint version 1.5.6+velvet-glove.1" ]]; then
    echo "error: controlled ghalint failed its exact patched-version probe" >&2
    exit 1
  fi
  verify_macho_closure "$ghalint_root" ghalint-workflow
fi

if [[ $errcheck_selected == true ]]; then
  errcheck_identity=$(recipe_integrity_json errcheck-macos-arm64)
  errcheck_root=$(pinned_component_cache_root \
    "$state_dir" errcheck-1.20.0 "$errcheck_identity")
  errcheck_binary="$errcheck_root/bin/errcheck"
  errcheck_expected_sha256=$(printf '%s\n' "$errcheck_identity" | \
    "$jq_bin" -r '.integrity.builtArtifactSha256')
  errcheck_manifest_path=$(printf '%s\n' "$errcheck_identity" | \
    "$jq_bin" -r '.integrity.moduleManifestPath')
  errcheck_lock_path=$(printf '%s\n' "$errcheck_identity" | \
    "$jq_bin" -r '.integrity.moduleLockPath')
  errcheck_manifest_sha256=$(printf '%s\n' "$errcheck_identity" | \
    "$jq_bin" -r '.integrity.moduleManifestSha256')
  errcheck_lock_sha256=$(printf '%s\n' "$errcheck_identity" | \
    "$jq_bin" -r '.integrity.moduleLockSha256')
  errcheck_proxy_sha256=$(printf '%s\n' "$errcheck_identity" | \
    "$jq_bin" -r '.integrity.sha256')
  if [[ $errcheck_expected_sha256 != \
      4f369aeb1bd8454d6ebb6789fedd948ef216fe04c6be629d5016aca78908aa0c || \
    $errcheck_proxy_sha256 != \
      50dbdc1e07128552bda3dad27dfaad9dca100d16869bf58485fe05ed4a45f0b6 || \
    $(printf '%s\n' "$errcheck_identity" | \
      "$jq_bin" -r '.integrity.buildToolchainComponentId') != errcheck-go ]]; then
    echo "error: errcheck recipe does not cross-link the reviewed proxy, artifact, and Go identity" >&2
    exit 1
  fi
  errcheck_manifest="$repository_root/$errcheck_manifest_path"
  errcheck_lock="$repository_root/$errcheck_lock_path"
  if [[ ! -f $errcheck_manifest || -L $errcheck_manifest || \
    ! -f $errcheck_lock || -L $errcheck_lock ]]; then
    echo "error: errcheck exact module closure is missing or linked" >&2
    exit 1
  fi
  read -r observed_errcheck_manifest_sha256 _ < <(
    /usr/bin/shasum -a 256 "$errcheck_manifest"
  )
  read -r observed_errcheck_lock_sha256 _ < <(
    /usr/bin/shasum -a 256 "$errcheck_lock"
  )
  if [[ $observed_errcheck_manifest_sha256 != "$errcheck_manifest_sha256" || \
    $observed_errcheck_lock_sha256 != "$errcheck_lock_sha256" ]]; then
    echo "error: errcheck module manifest or sum checksum mismatch" >&2
    exit 1
  fi

  errcheck_go_root=$(env -i "${provisioning_env[@]}" "$mise_bin" where go@1.26.5)
  errcheck_go_bin="$errcheck_go_root/bin/go"
  case $errcheck_go_bin in
    "$state_dir"/*) ;;
    *)
      echo "error: pinned errcheck Go resolved outside the controlled state: $errcheck_go_bin" >&2
      exit 1
      ;;
  esac
  if [[ ! -f $errcheck_go_bin || -L $errcheck_go_bin || ! -x $errcheck_go_bin ]]; then
    echo "error: pinned errcheck Go executable is unavailable: $errcheck_go_bin" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" \
    "PATH=$errcheck_go_root/bin:/usr/bin:/bin" \
    "GOTOOLCHAIN=local" \
    "$errcheck_go_bin" version) != "go version go1.26.5 darwin/arm64" ]]; then
    echo "error: pinned errcheck build toolchain is not exact Go 1.26.5 Darwin arm64" >&2
    exit 1
  fi

  if [[ -e $errcheck_root && ( ! -d $errcheck_root || -L $errcheck_root ) ]]; then
    echo "error: controlled errcheck root is not a directory: $errcheck_root" >&2
    exit 1
  fi
  if [[ ! -d $errcheck_root ]]; then
    echo "==> Building the exact errcheck v1.20.0 module closure with locked Go 1.26.5"
    errcheck_build_dir="$state_dir/errcheck-build-1.20.0"
    if [[ -e $errcheck_build_dir ]]; then
      echo "warning: removing stale transactional errcheck build directory" >&2
      rm -rf -- "$errcheck_build_dir"
    fi
    errcheck_staging_root="$errcheck_build_dir/install"
    errcheck_staging_binary="$errcheck_staging_root/bin/errcheck"
    errcheck_mod_cache="$state_dir/errcheck-go1.26.5-mod-cache"
    errcheck_bootstrap_cache="$state_dir/errcheck-bootstrap-go1.26.5-build-cache"
    mkdir -p \
      "$errcheck_staging_root/bin" \
      "$errcheck_build_dir/go-build-cache" \
      "$errcheck_mod_cache" \
      "$errcheck_bootstrap_cache"
    errcheck_go_env=(
      "${provisioning_env[@]}"
      "PATH=$errcheck_go_root/bin:/usr/bin:/bin"
      "GOTOOLCHAIN=local"
      "CGO_ENABLED=0"
      "GOOS=darwin"
      "GOARCH=arm64"
      "GOMODCACHE=$errcheck_mod_cache"
    )
    env -i "${errcheck_go_env[@]}" \
      "GOCACHE=$errcheck_bootstrap_cache" \
      "GOPROXY=https://proxy.golang.org" \
      "GOSUMDB=sum.golang.org" \
      "$errcheck_go_bin" -C "${errcheck_manifest%/go.mod}" mod download \
        github.com/kisielk/errcheck@v1.20.0 \
        golang.org/x/mod@v0.35.0 \
        golang.org/x/sync@v0.20.0 \
        golang.org/x/tools@v0.44.0
    read -r observed_errcheck_manifest_sha256 _ < <(
      /usr/bin/shasum -a 256 "$errcheck_manifest"
    )
    read -r observed_errcheck_lock_sha256 _ < <(
      /usr/bin/shasum -a 256 "$errcheck_lock"
    )
    if [[ $observed_errcheck_manifest_sha256 != "$errcheck_manifest_sha256" || \
      $observed_errcheck_lock_sha256 != "$errcheck_lock_sha256" ]]; then
      echo "error: errcheck network bootstrap changed the exact module inputs" >&2
      exit 1
    fi
    errcheck_proxy_zip="$errcheck_mod_cache/cache/download/github.com/kisielk/errcheck/@v/v1.20.0.zip"
    if [[ ! -f $errcheck_proxy_zip || -L $errcheck_proxy_zip ]]; then
      echo "error: errcheck proxy bootstrap did not produce the declared module archive" >&2
      exit 1
    fi
    read -r observed_errcheck_proxy_sha256 _ < <(
      /usr/bin/shasum -a 256 "$errcheck_proxy_zip"
    )
    if [[ $observed_errcheck_proxy_sha256 != "$errcheck_proxy_sha256" ]]; then
      echo "error: errcheck Go proxy archive checksum mismatch" >&2
      exit 1
    fi
    env -i "${errcheck_go_env[@]}" \
      "GOCACHE=$errcheck_bootstrap_cache" \
      "GOPROXY=off" \
      "GOSUMDB=off" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      "$errcheck_go_bin" -C "${errcheck_manifest%/go.mod}" mod verify
    errcheck_observed_modules=$(env -i "${errcheck_go_env[@]}" \
      "GOCACHE=$errcheck_bootstrap_cache" \
      "GOPROXY=off" \
      "GOSUMDB=off" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      "$errcheck_go_bin" -C "${errcheck_manifest%/go.mod}" list \
        -mod=readonly \
        -deps \
        -f '{{with .Module}}{{.Path}} {{.Version}} {{.Sum}}{{end}}' \
        github.com/kisielk/errcheck | LC_ALL=C /usr/bin/sort -u)
    errcheck_expected_modules=$'github.com/kisielk/errcheck v1.20.0 h1:9rwHBNKzd4wkDWcROy3DvFGNqEPlkxBg305rvk7HabI=\ngolang.org/x/mod v0.35.0 h1:Ww1D637e6Pg+Zb2KrWfHQUnH2dQRLBQyAtpr/haaJeM=\ngolang.org/x/sync v0.20.0 h1:e0PTpb7pjO8GAtTs2dQ6jYa5BWYlMuX047Dco/pItO4=\ngolang.org/x/tools v0.44.0 h1:UP4ajHPIcuMjT1GqzDWRlalUEoY+uzoZKnhOjbIPD2c='
    if [[ $errcheck_observed_modules != "$errcheck_expected_modules" ]]; then
      echo "error: errcheck denied-network package dependency closure drifted" >&2
      exit 1
    fi
    (
      cd /
      env -i "${errcheck_go_env[@]}" \
        "GOCACHE=$errcheck_build_dir/go-build-cache" \
        "GOPROXY=file://$errcheck_mod_cache/cache/download" \
        "GOSUMDB=off" \
        "GOBIN=$errcheck_staging_root/bin" \
        "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
        "$errcheck_go_bin" install \
          -trimpath \
          -ldflags '-s -w -buildid=' \
          github.com/kisielk/errcheck@v1.20.0
    )
    validate_errcheck_binary \
      "$errcheck_staging_binary" "$errcheck_go_bin" "$errcheck_expected_sha256"
    printf '%s\n' "$errcheck_identity" \
      >"$errcheck_staging_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$errcheck_staging_root" errcheck-macos-arm64
    mv "$errcheck_staging_root" "$errcheck_root"
    rm -rf -- "$errcheck_build_dir"
    errcheck_build_dir=
  fi
  if ! pinned_component_cache_valid \
    "$errcheck_root" "$errcheck_identity" bin/errcheck; then
    echo "error: controlled errcheck installation does not match its exact recipe identity" >&2
    exit 1
  fi
  if [[ -n $(/usr/bin/find "$errcheck_root" -type l -print -quit) || \
    $(/usr/bin/find "$errcheck_root" -type f | /usr/bin/wc -l | /usr/bin/tr -d ' ') != 2 || \
    $(/usr/bin/find "$errcheck_root" -type d | /usr/bin/wc -l | /usr/bin/tr -d ' ') != 2 || \
    -n $(/usr/bin/find "$errcheck_root" -mindepth 1 ! -type d ! -type f -print -quit) ]]; then
    echo "error: controlled errcheck installation has an unexpected or linked closure" >&2
    exit 1
  fi
  validate_errcheck_binary \
    "$errcheck_binary" "$errcheck_go_bin" "$errcheck_expected_sha256"
  verify_macho_closure "$errcheck_root" errcheck-macos-arm64
fi

if [[ $goimports_selected == true ]]; then
  goimports_identity=$(recipe_integrity_json goimports-macos-arm64)
  goimports_root=$(pinned_component_cache_root \
    "$state_dir" goimports-0.48.0 "$goimports_identity")
  goimports_binary="$goimports_root/bin/goimports"
  goimports_expected_sha256=$(printf '%s\n' "$goimports_identity" | \
    "$jq_bin" -r '.integrity.builtArtifactSha256')
  goimports_manifest_path=$(printf '%s\n' "$goimports_identity" | \
    "$jq_bin" -r '.integrity.moduleManifestPath')
  goimports_lock_path=$(printf '%s\n' "$goimports_identity" | \
    "$jq_bin" -r '.integrity.moduleLockPath')
  goimports_manifest_sha256=$(printf '%s\n' "$goimports_identity" | \
    "$jq_bin" -r '.integrity.moduleManifestSha256')
  goimports_lock_sha256=$(printf '%s\n' "$goimports_identity" | \
    "$jq_bin" -r '.integrity.moduleLockSha256')
  goimports_proxy_sha256=$(printf '%s\n' "$goimports_identity" | \
    "$jq_bin" -r '.integrity.sha256')
  if [[ $goimports_expected_sha256 != \
      2d7d2892651e4452091f0fe8e280c7b6e14f3b6964854516fd7372442d57fd27 || \
    $goimports_proxy_sha256 != \
      8529e7bd696890fd79d3e1c37c7d1a3e2e26fb4b392b5beebfa7134ad2f65755 || \
    $(printf '%s\n' "$goimports_identity" | \
      "$jq_bin" -r '.integrity.buildToolchainComponentId') != goimports-go ]]; then
    echo "error: goimports recipe does not cross-link the reviewed proxy, artifact, and Go identity" >&2
    exit 1
  fi
  goimports_manifest="$repository_root/$goimports_manifest_path"
  goimports_lock="$repository_root/$goimports_lock_path"
  if [[ ! -f $goimports_manifest || -L $goimports_manifest || \
    ! -f $goimports_lock || -L $goimports_lock ]]; then
    echo "error: goimports exact module closure is missing or linked" >&2
    exit 1
  fi
  read -r observed_goimports_manifest_sha256 _ < <(
    /usr/bin/shasum -a 256 "$goimports_manifest"
  )
  read -r observed_goimports_lock_sha256 _ < <(
    /usr/bin/shasum -a 256 "$goimports_lock"
  )
  if [[ $observed_goimports_manifest_sha256 != "$goimports_manifest_sha256" || \
    $observed_goimports_lock_sha256 != "$goimports_lock_sha256" ]]; then
    echo "error: goimports module manifest or sum checksum mismatch" >&2
    exit 1
  fi

  goimports_go_root=$(env -i "${provisioning_env[@]}" "$mise_bin" where go@1.26.5)
  goimports_go_bin="$goimports_go_root/bin/go"
  case $goimports_go_bin in
    "$state_dir"/*) ;;
    *)
      echo "error: pinned goimports Go resolved outside the controlled state: $goimports_go_bin" >&2
      exit 1
      ;;
  esac
  if [[ ! -f $goimports_go_bin || -L $goimports_go_bin || ! -x $goimports_go_bin ]]; then
    echo "error: pinned goimports Go executable is unavailable: $goimports_go_bin" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" \
    "PATH=$goimports_go_root/bin:/usr/bin:/bin" \
    "GOTOOLCHAIN=local" \
    "$goimports_go_bin" version) != "go version go1.26.5 darwin/arm64" ]]; then
    echo "error: pinned goimports build toolchain is not exact Go 1.26.5 Darwin arm64" >&2
    exit 1
  fi

  if [[ -e $goimports_root && ( ! -d $goimports_root || -L $goimports_root ) ]]; then
    echo "error: controlled goimports root is not a directory: $goimports_root" >&2
    exit 1
  fi
  if [[ ! -d $goimports_root ]]; then
    echo "==> Building exact goimports v0.48.0 with locked Go 1.26.5"
    goimports_build_dir="$state_dir/goimports-build-0.48.0"
    if [[ -e $goimports_build_dir ]]; then
      echo "warning: removing stale transactional goimports build directory" >&2
      rm -rf -- "$goimports_build_dir"
    fi
    goimports_staging_root="$goimports_build_dir/install"
    goimports_staging_binary="$goimports_staging_root/bin/goimports"
    goimports_mod_cache="$state_dir/goimports-go1.26.5-mod-cache"
    goimports_bootstrap_cache="$state_dir/goimports-bootstrap-go1.26.5-build-cache"
    mkdir -p \
      "$goimports_staging_root/bin" \
      "$goimports_build_dir/go-build-cache" \
      "$goimports_mod_cache" \
      "$goimports_bootstrap_cache"
    goimports_go_env=(
      "${provisioning_env[@]}"
      "PATH=$goimports_go_root/bin:/usr/bin:/bin"
      "GOENV=off"
      "GOTOOLCHAIN=local"
      "CGO_ENABLED=0"
      "GOOS=darwin"
      "GOARCH=arm64"
      "GOARM64=v8.0"
      "GOMODCACHE=$goimports_mod_cache"
    )
    env -i "${goimports_go_env[@]}" \
      "GOCACHE=$goimports_bootstrap_cache" \
      "GOPROXY=https://proxy.golang.org" \
      "GOSUMDB=sum.golang.org" \
      "$goimports_go_bin" -C "${goimports_manifest%/go.mod}" mod download \
        golang.org/x/tools@v0.48.0 \
        golang.org/x/mod@v0.38.0 \
        golang.org/x/sync@v0.22.0 \
        golang.org/x/telemetry@v0.0.0-20260708182218-49f421fb7959
    read -r observed_goimports_manifest_sha256 _ < <(
      /usr/bin/shasum -a 256 "$goimports_manifest"
    )
    read -r observed_goimports_lock_sha256 _ < <(
      /usr/bin/shasum -a 256 "$goimports_lock"
    )
    if [[ $observed_goimports_manifest_sha256 != "$goimports_manifest_sha256" || \
      $observed_goimports_lock_sha256 != "$goimports_lock_sha256" ]]; then
      echo "error: goimports network bootstrap changed the exact module inputs" >&2
      exit 1
    fi
    goimports_archives=(
      "golang.org/x/tools/@v/v0.48.0.zip:8529e7bd696890fd79d3e1c37c7d1a3e2e26fb4b392b5beebfa7134ad2f65755"
      "golang.org/x/tools/@v/v0.48.0.mod:e6b55566a172ecfd21e5f4a8750f2d25665287288b24ff8d4e6cea5d5078c608"
      "golang.org/x/mod/@v/v0.38.0.zip:b19d1a19527f75bf148198b44be37784f7d7d22597b46e260bf22d1b320fe12c"
      "golang.org/x/mod/@v/v0.38.0.mod:c584b29967a2cf46a7b8eacd85cb34d5bb0ab2d61a7a46ebc0a1ee362516e410"
      "golang.org/x/sync/@v/v0.22.0.zip:4bc67d258ce7867cfc1a43765c43d98b4c49b90c46dd0b86f896a5a4909fade0"
      "golang.org/x/sync/@v/v0.22.0.mod:a3e29e76060bd561060454b1fa2bdcd66674f60c9ca93833b8106355e34c603c"
      "golang.org/x/telemetry/@v/v0.0.0-20260708182218-49f421fb7959.zip:5487d5d99925cc2ad6884e66d70906ac13aa0180d88387bc66f0c706276c2f22"
      "golang.org/x/telemetry/@v/v0.0.0-20260708182218-49f421fb7959.mod:f2675940de8f52f9d92c692ba4c2c793bb0d485ef582a0a6d99dd8ec3b97547f"
    )
    for archive_binding in "${goimports_archives[@]}"; do
      archive_relative=${archive_binding%%:*}
      archive_expected=${archive_binding#*:}
      archive_path="$goimports_mod_cache/cache/download/$archive_relative"
      if [[ ! -f $archive_path || -L $archive_path ]]; then
        echo "error: goimports proxy bootstrap omitted $archive_relative" >&2
        exit 1
      fi
      read -r archive_observed _ < <(/usr/bin/shasum -a 256 "$archive_path")
      if [[ $archive_observed != "$archive_expected" ]]; then
        echo "error: goimports proxy object checksum mismatch for $archive_relative" >&2
        exit 1
      fi
    done
    env -i "${goimports_go_env[@]}" \
      "GOCACHE=$goimports_bootstrap_cache" \
      "GOPROXY=off" \
      "GOSUMDB=off" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      "$goimports_go_bin" -C "${goimports_manifest%/go.mod}" mod verify
    (
      cd /
      env -i "${goimports_go_env[@]}" \
        "GOCACHE=$goimports_build_dir/go-build-cache" \
        "GOPROXY=file://$goimports_mod_cache/cache/download" \
        "GOSUMDB=off" \
        "GOBIN=$goimports_staging_root/bin" \
        "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
        "$goimports_go_bin" install \
          -trimpath \
          -ldflags '-s -w -buildid=' \
          golang.org/x/tools/cmd/goimports@v0.48.0
    )
    validate_goimports_binary \
      "$goimports_staging_binary" "$goimports_go_bin" "$goimports_expected_sha256"
    printf '%s\n' "$goimports_identity" \
      >"$goimports_staging_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$goimports_staging_root" goimports-macos-arm64
    mv "$goimports_staging_root" "$goimports_root"
    rm -rf -- "$goimports_build_dir"
    goimports_build_dir=
  fi
  if ! pinned_component_cache_valid \
    "$goimports_root" "$goimports_identity" bin/goimports; then
    echo "error: controlled goimports installation does not match its exact recipe identity" >&2
    exit 1
  fi
  if [[ -n $(/usr/bin/find "$goimports_root" -type l -print -quit) || \
    $(/usr/bin/find "$goimports_root" -type f | /usr/bin/wc -l | /usr/bin/tr -d ' ') != 2 || \
    $(/usr/bin/find "$goimports_root" -type d | /usr/bin/wc -l | /usr/bin/tr -d ' ') != 2 || \
    -n $(/usr/bin/find "$goimports_root" -mindepth 1 ! -type d ! -type f -print -quit) ]]; then
    echo "error: controlled goimports installation has an unexpected or linked closure" >&2
    exit 1
  fi
  validate_goimports_binary \
    "$goimports_binary" "$goimports_go_bin" "$goimports_expected_sha256"
  verify_macho_closure "$goimports_root" goimports-macos-arm64
fi

if [[ $golines_selected == true ]]; then
  golines_identity=$(recipe_integrity_json golines-macos-arm64)
  golines_root=$(pinned_component_cache_root \
    "$state_dir" golines-0.13.0-vg1 "$golines_identity")
  golines_binary="$golines_root/bin/golines"
  golines_expected_sha256=$(printf '%s\n' "$golines_identity" | \
    "$jq_bin" -r '.integrity.builtArtifactSha256')
  golines_expected_version=$(printf '%s\n' "$golines_identity" | \
    "$jq_bin" -r '.version')
  golines_expected_version_output=$("$jq_bin" -r \
    '.recipes[] | select(.id == "golines-macos-arm64") | .probe.expected' "$registry")
  golines_patch_path=$(printf '%s\n' "$golines_identity" | \
    "$jq_bin" -r '.integrity.path')
  golines_manifest_path=$(printf '%s\n' "$golines_identity" | \
    "$jq_bin" -r '.integrity.moduleManifestPath')
  golines_lock_path=$(printf '%s\n' "$golines_identity" | \
    "$jq_bin" -r '.integrity.moduleLockPath')
  golines_patch_sha256=$(printf '%s\n' "$golines_identity" | \
    "$jq_bin" -r '.integrity.patchSha256')
  golines_manifest_sha256=$(printf '%s\n' "$golines_identity" | \
    "$jq_bin" -r '.integrity.moduleManifestSha256')
  golines_lock_sha256=$(printf '%s\n' "$golines_identity" | \
    "$jq_bin" -r '.integrity.moduleLockSha256')
  golines_source_sha256=$(printf '%s\n' "$golines_identity" | \
    "$jq_bin" -r '.integrity.sha256')
  if [[ $golines_expected_sha256 != \
      4d7bf2a59b9b48bfc234078498b3ddf6a412cf9bd0ce525945bb19d558f6ab75 || \
    $golines_source_sha256 != \
      ec1933e0fb73cf0517fd007d325603007aa65ce430267a70fc78cfea43d9716e || \
    $golines_patch_sha256 != \
      c4a7fcf96b2f1a83440e824340e6d51e15ed34630415e044781a780fc7a2a4d3 || \
    $golines_manifest_sha256 != \
      8754d400db1f04a71e5e3eb13343bb051afaba153ea9cb9219fb217250adfa4b || \
    $golines_lock_sha256 != \
      21eaf4b83c0df55ae2e7b94ee43fd72a01171bf4ed2729a578b1fc1e54c219fe || \
    $golines_expected_version != 0.13.0+velvet-glove.1 || \
    $(printf '%s\n' "$golines_identity" | \
      "$jq_bin" -r '.integrity.buildToolchainComponentId') != golines-go ]]; then
    echo "error: golines recipe does not cross-link the reviewed source, patch, artifact, and Go identity" >&2
    exit 1
  fi
  golines_patch="$repository_root/$golines_patch_path"
  golines_manifest="$repository_root/$golines_manifest_path"
  golines_lock="$repository_root/$golines_lock_path"
  golines_provenance="$provisioning_dir/golines/source-build.json"
  for input in "$golines_patch" "$golines_manifest" "$golines_lock" "$golines_provenance"; do
    if [[ ! -f $input || -L $input ]]; then
      echo "error: golines exact source-build input is missing or linked: $input" >&2
      exit 1
    fi
  done
  read -r observed_golines_patch_sha256 _ < <(/usr/bin/shasum -a 256 "$golines_patch")
  read -r observed_golines_manifest_sha256 _ < <(/usr/bin/shasum -a 256 "$golines_manifest")
  read -r observed_golines_lock_sha256 _ < <(/usr/bin/shasum -a 256 "$golines_lock")
  if [[ $observed_golines_patch_sha256 != "$golines_patch_sha256" || \
    $observed_golines_manifest_sha256 != "$golines_manifest_sha256" || \
    $observed_golines_lock_sha256 != "$golines_lock_sha256" ]]; then
    echo "error: golines patch, module manifest, or sum checksum mismatch" >&2
    exit 1
  fi
  if ! "$jq_bin" -e \
    --arg patch "$golines_patch_sha256" \
    --arg manifest "$golines_manifest_sha256" \
    --arg lock "$golines_lock_sha256" \
    --arg artifact "$golines_expected_sha256" '
      .schemaVersion == 1
      and .status == "integrated"
      and .component.id == "golines"
      and .component.productVersion == "0.13.0+velvet-glove.1"
      and .closure.patchSha256 == $patch
      and .closure.moduleManifestSha256 == $manifest
      and .closure.moduleLockSha256 == $lock
      and (.closure.runtimeModuleObjects | length) == 12
      and .closure.runtimeModulePolicy == "Bootstrap exactly the 12 modules embedded by the pinned binary. Do not use `go mod download all`; the committed go.sum also records test-only modules that are not runtime build inputs."
      and .toolchain.componentId == "golines-go"
      and .toolchain.version == "1.26.5"
      and .build.environment.GOENV == "off"
      and .build.environment.GOWORK == "off"
      and .build.environment.GOPROXY == "off"
      and .artifact.sha256 == $artifact
      and .artifact.size == 7341970
      and .artifact.embeddedBuildFacts.dependencyCount == 12' \
    "$golines_provenance" >/dev/null; then
    echo "error: golines source-build provenance does not match the reviewed closure" >&2
    exit 1
  fi

  golines_go_root=$(env -i "${provisioning_env[@]}" "$mise_bin" where go@1.26.5)
  golines_go_bin="$golines_go_root/bin/go"
  case $golines_go_bin in
    "$state_dir"/*) ;;
    *)
      echo "error: pinned golines Go resolved outside the controlled state: $golines_go_bin" >&2
      exit 1
      ;;
  esac
  if [[ ! -f $golines_go_bin || -L $golines_go_bin || ! -x $golines_go_bin ]]; then
    echo "error: pinned golines Go executable is unavailable: $golines_go_bin" >&2
    exit 1
  fi
  if [[ $(env -i "${provisioning_env[@]}" \
    "PATH=$golines_go_root/bin:/usr/bin:/bin" \
    "GOENV=off" \
    "GOWORK=off" \
    "GOTOOLCHAIN=local" \
    "$golines_go_bin" version) != "go version go1.26.5 darwin/arm64" ]]; then
    echo "error: pinned golines build toolchain is not exact Go 1.26.5 Darwin arm64" >&2
    exit 1
  fi

  if [[ -e $golines_root && ( ! -d $golines_root || -L $golines_root ) ]]; then
    echo "error: controlled golines root is not a directory: $golines_root" >&2
    exit 1
  fi
  if [[ ! -d $golines_root ]]; then
    echo "==> Building exact patched golines 0.13.0+velvet-glove.1 with locked Go 1.26.5"
    golines_archive=$(fetch_component_archive golines)
    golines_build_dir="$state_dir/golines-build-0.13.0-vg1"
    if [[ -e $golines_build_dir ]]; then
      echo "warning: removing stale transactional golines build directory" >&2
      rm -rf -- "$golines_build_dir"
    fi
    mkdir -p \
      "$golines_build_dir/install/bin" \
      "$golines_build_dir/go-mod-cache" \
      "$golines_build_dir/bootstrap-go-build-cache" \
      "$golines_build_dir/go-build-cache"
    /usr/bin/tar -xf "$golines_archive" -C "$golines_build_dir"
    if [[ ! -d $golines_build_dir/golines-0.13.0 || \
      -L $golines_build_dir/golines-0.13.0 ]]; then
      echo "error: golines source archive omitted its exact root" >&2
      exit 1
    fi
    mv "$golines_build_dir/golines-0.13.0" "$golines_build_dir/source"
    golines_source="$golines_build_dir/source"
    golines_staging_root="$golines_build_dir/install"
    golines_staging_binary="$golines_staging_root/bin/golines"
    golines_mod_cache="$golines_build_dir/go-mod-cache"
    golines_bootstrap_cache="$golines_build_dir/bootstrap-go-build-cache"
    golines_build_cache="$golines_build_dir/go-build-cache"
    if [[ $(/usr/bin/shasum -a 256 "$golines_source/main.go" | /usr/bin/awk '{print $1}') != \
        f4b5292ae055fd299e5ea8d2b42af8b907bb9bf1002e7c5bb3796f8e1069949f || \
      $(/usr/bin/shasum -a 256 "$golines_source/go.mod" | /usr/bin/awk '{print $1}') != \
        1981e8cea70c114c08916c9fc46adb810e458d8c7af057d2d437a533a77ec660 || \
      $(/usr/bin/shasum -a 256 "$golines_source/go.sum" | /usr/bin/awk '{print $1}') != \
        5a29e3cb78df02fee0483a45e7ce92b83a6c3b1ebac46ca6971df9c2dc1081fe ]]; then
      echo "error: golines source archive does not contain the reviewed unpatched inputs" >&2
      exit 1
    fi
    (
      cd "$golines_source"
      env -i "${provisioning_env[@]}" \
        "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
        /usr/bin/patch -p1 -i "$golines_patch"
    )
    if ! /usr/bin/cmp -s "$golines_source/go.mod" "$golines_manifest" || \
      ! /usr/bin/cmp -s "$golines_source/go.sum" "$golines_lock" || \
      [[ $(/usr/bin/shasum -a 256 "$golines_source/main.go" | /usr/bin/awk '{print $1}') != \
        a600f1ece4dde5b86707b52bc157378f50889f69f780442abe18577e0cf895c0 ]]; then
      echo "error: golines closure patch did not produce the reviewed source inputs" >&2
      exit 1
    fi
    golines_go_env=(
      "${provisioning_env[@]}"
      "PATH=$golines_go_root/bin:/usr/bin:/bin"
      "GOENV=off"
      "GOWORK=off"
      "GOTOOLCHAIN=local"
      "GOFLAGS=-mod=readonly"
      "CGO_ENABLED=0"
      "GOOS=darwin"
      "GOARCH=arm64"
      "GOARM64=v8.0"
      "SOURCE_DATE_EPOCH=1755811321"
      "GOMODCACHE=$golines_mod_cache"
    )
    golines_modules=()
    while IFS=$'\t' read -r module version; do
      golines_modules+=("$module@$version")
    done < <("$jq_bin" -r '.closure.runtimeModuleObjects[] | [.module, .version] | @tsv' \
      "$golines_provenance")
    if [[ ${#golines_modules[@]} -ne 12 ]]; then
      echo "error: golines source-build provenance did not declare exactly 12 runtime modules" >&2
      exit 1
    fi
    env -i "${golines_go_env[@]}" \
      "GOCACHE=$golines_bootstrap_cache" \
      "GOPROXY=https://proxy.golang.org" \
      "GOSUMDB=sum.golang.org" \
      "$golines_go_bin" -C "$golines_source" mod download "${golines_modules[@]}"
    if ! /usr/bin/cmp -s "$golines_source/go.mod" "$golines_manifest" || \
      ! /usr/bin/cmp -s "$golines_source/go.sum" "$golines_lock"; then
      echo "error: golines network bootstrap changed the exact module inputs" >&2
      exit 1
    fi
    while IFS=$'\t' read -r module version zip_sha zip_size mod_sha mod_size; do
      archive_base="$golines_mod_cache/cache/download/$module/@v/$version"
      for object in zip mod; do
        if [[ $object == zip ]]; then
          object_sha=$zip_sha
          object_size=$zip_size
        else
          object_sha=$mod_sha
          object_size=$mod_size
        fi
        object_path="$archive_base.$object"
        if [[ ! -f $object_path || -L $object_path || \
          $(/usr/bin/stat -f '%z' "$object_path") != "$object_size" || \
          $(/usr/bin/shasum -a 256 "$object_path" | /usr/bin/awk '{print $1}') != "$object_sha" ]]; then
          echo "error: golines proxy object differs from the reviewed closure: $module@$version.$object" >&2
          exit 1
        fi
      done
    done < <("$jq_bin" -r \
      '.closure.runtimeModuleObjects[] | [.module, .version, .zip.sha256, .zip.size, .mod.sha256, .mod.size] | @tsv' \
      "$golines_provenance")
    env -i "${golines_go_env[@]}" \
      "GOCACHE=$golines_bootstrap_cache" \
      "GOPROXY=off" \
      "GOSUMDB=off" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      "$golines_go_bin" -C "$golines_source" mod verify
    env -i "${golines_go_env[@]}" \
      "GOCACHE=$golines_build_cache" \
      "GOPROXY=off" \
      "GOSUMDB=off" \
      "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
      "$golines_go_bin" -C "$golines_source" build \
        -trimpath \
        -buildvcs=false \
        -ldflags '-s -w -buildid= -X=main.version=0.13.0+velvet-glove.1 -X=main.commit=8f32f0f7e89c30f572c7f2cd3b2a48016b9d8bbf -X=main.date=2025-08-21T21:22:01Z' \
        -o "$golines_staging_binary" \
        .
    validate_golines_binary \
      "$golines_staging_binary" "$golines_go_bin" \
      "$golines_expected_sha256" "$golines_expected_version_output"
    printf '%s\n' "$golines_identity" \
      >"$golines_staging_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$golines_staging_root" golines-macos-arm64
    mv "$golines_staging_root" "$golines_root"
    rm -rf -- "$golines_build_dir"
    golines_build_dir=
  fi
  if ! pinned_component_cache_valid \
    "$golines_root" "$golines_identity" bin/golines; then
    echo "error: controlled golines installation does not match its exact recipe identity" >&2
    exit 1
  fi
  if [[ -n $(/usr/bin/find "$golines_root" -type l -print -quit) || \
    $(/usr/bin/find "$golines_root" -type f | /usr/bin/wc -l | /usr/bin/tr -d ' ') != 2 || \
    $(/usr/bin/find "$golines_root" -type d | /usr/bin/wc -l | /usr/bin/tr -d ' ') != 2 || \
    -n $(/usr/bin/find "$golines_root" -mindepth 1 ! -type d ! -type f -print -quit) ]]; then
    echo "error: controlled golines installation has an unexpected or linked closure" >&2
    exit 1
  fi
  validate_golines_binary \
    "$golines_binary" "$golines_go_bin" \
    "$golines_expected_sha256" "$golines_expected_version_output"
  verify_macho_closure "$golines_root" golines-macos-arm64
fi

if needs_group ruby; then
  ruby_archive=$(fetch_component_archive ruby)
  ruby_root="$state_dir/ruby-runtime-3.4.10-asciidoctor-2.0.26-rubocop-1.30.1"
  ruby_identity=$(component_integrity_json ruby)
  if [[ -e $ruby_root && ! -d $ruby_root ]]; then
    echo "error: controlled Ruby root is not a directory: $ruby_root" >&2
    exit 1
  fi
  if [[ ! -d $ruby_root ]]; then
    echo "==> Installing the checksum-verified relocatable Ruby archive"
    ruby_extract_dir=$(mktemp -d "$state_dir/ruby-extract.XXXXXX")
    /usr/bin/tar -xf "$ruby_archive" -C "$ruby_extract_dir"
    ruby_archive_root=$("$jq_bin" -r '.environments[].components[] | select(.id == "ruby") | .integrity.archiveRoot' "$registry")
    if [[ ! -x $ruby_extract_dir/$ruby_archive_root/bin/ruby ]]; then
      echo "error: Ruby archive is missing its declared root" >&2
      exit 1
    fi
    printf '%s\n' "$ruby_identity" >"$ruby_extract_dir/$ruby_archive_root/.velvet-glove-artifacts.json"
    verify_macho_closure "$ruby_extract_dir/$ruby_archive_root" ruby
    mv "$ruby_extract_dir/$ruby_archive_root" "$ruby_root"
    rm -rf -- "$ruby_extract_dir"
    ruby_extract_dir=
  fi
  if [[ ! -x $ruby_root/bin/ruby || ! -x $ruby_root/bin/bundle ]]; then
    echo "error: controlled Ruby installation is incomplete: $ruby_root" >&2
    exit 1
  fi
  if [[ ! -f $ruby_root/.velvet-glove-artifacts.json ]] || \
    [[ $(<"$ruby_root/.velvet-glove-artifacts.json") != "$ruby_identity" ]]; then
    echo "error: controlled Ruby installation does not match the declared archive: $ruby_root" >&2
    exit 1
  fi
  verify_macho_closure "$ruby_root" ruby

  echo "==> Installing the checksum-locked pure-Ruby Bundler graph"
  ruby_contract_root="$state_dir/ruby-contract-asciidoctor-2.0.26-rubocop-1.30.1"
  mkdir -p \
    "$ruby_contract_root/bin" \
    "$ruby_contract_root/cache" \
    "$ruby_contract_root/config" \
    "$ruby_contract_root/user"
  gemfile_lock_sha256=$(shasum -a 256 "$provisioning_dir/ruby/Gemfile.lock" | awk '{print $1}')
  ruby_contract_identity=$("$jq_bin" -cn \
    --argjson ruby "$ruby_identity" \
    --arg gemfileLockSha256 "$gemfile_lock_sha256" \
    '{ruby: $ruby, gemfileLockSha256: $gemfileLockSha256}')
  if [[ -f $ruby_root/.velvet-glove-contract.json ]] && \
    [[ $(<"$ruby_root/.velvet-glove-contract.json") != "$ruby_contract_identity" ]]; then
    echo "error: controlled Ruby contract does not match Gemfile.lock: $ruby_root" >&2
    exit 1
  fi
  ruby_gem_count=0
  while IFS=$'\t' read -r gem_name gem_version gem_sha256; do
    if [[ ! $gem_name =~ ^[A-Za-z0-9_.-]+$ || \
      ! $gem_version =~ ^[A-Za-z0-9_.-]+$ || \
      ! $gem_sha256 =~ ^[0-9a-f]{64}$ ]]; then
      echo "error: unsafe checksum entry in Gemfile.lock" >&2
      exit 1
    fi
    gem_archive="$ruby_contract_root/cache/$gem_name-$gem_version.gem"
    if [[ -f $gem_archive ]]; then
      read -r observed_gem_sha256 _ < <(shasum -a 256 "$gem_archive")
      if [[ $observed_gem_sha256 != "$gem_sha256" ]]; then
        rm -f -- "$gem_archive"
      fi
    fi
    if [[ ! -f $gem_archive ]]; then
      gem_partial=$(mktemp "$ruby_contract_root/cache/$gem_name-$gem_version.partial.XXXXXX")
      gem_url="https://rubygems.org/downloads/$gem_name-$gem_version.gem"
      echo "==> Downloading checksum-locked Ruby gem $gem_name $gem_version"
      if ! env -i "${provisioning_env[@]}" \
        /usr/bin/curl --fail --location --show-error --output "$gem_partial" "$gem_url"; then
        rm -f -- "$gem_partial"
        exit 1
      fi
      read -r observed_gem_sha256 _ < <(shasum -a 256 "$gem_partial")
      if [[ $observed_gem_sha256 != "$gem_sha256" ]]; then
        echo "error: Ruby gem checksum mismatch for $gem_name $gem_version" >&2
        rm -f -- "$gem_partial"
        exit 1
      fi
      mv "$gem_partial" "$gem_archive"
    fi
    ruby_gem_count=$((ruby_gem_count + 1))
  done < <(env -i "${provisioning_env[@]}" \
    "PATH=$ruby_root/bin:/usr/bin:/bin" \
    "$ruby_root/bin/ruby" -e '
      in_checksums = false
      File.foreach(ARGV.fetch(0)) do |line|
        stripped = line.chomp
        if stripped == "CHECKSUMS"
          in_checksums = true
          next
        end
        break if in_checksums && stripped == "BUNDLED WITH"
        next unless in_checksums
        match = stripped.match(/\A  ([A-Za-z0-9_.-]+) \(([^)]+)\) sha256=([0-9a-f]{64})\z/)
        puts match.captures.join("\t") if match
      end
    ' "$provisioning_dir/ruby/Gemfile.lock")
  if [[ $ruby_gem_count -ne 13 ]]; then
    echo "error: expected 13 checksum-locked Ruby gem packages, found $ruby_gem_count" >&2
    exit 1
  fi
  ruby_env=(
    "BUNDLE_APP_CONFIG=$ruby_contract_root/config"
    "BUNDLE_CACHE_PATH=$ruby_contract_root/cache"
    "BUNDLE_DEPLOYMENT=1"
    "BUNDLE_FROZEN=1"
    "BUNDLE_GEMFILE=$provisioning_dir/ruby/Gemfile"
    "BUNDLE_PATH__SYSTEM=true"
    "BUNDLE_USER_HOME=$ruby_contract_root/user"
  )
  native_bundles_before=$(find "$ruby_root" -type f -name '*.bundle' -print | LC_ALL=C sort)
  env -i "${provisioning_env[@]}" \
    "PATH=$ruby_root/bin:/usr/bin:/bin" \
    "${ruby_env[@]}" \
    "$ruby_root/bin/bundle" install --local --jobs 4
  ruby_lock_check_dir="$run_home/tmp/ruby-lock-check"
  mkdir -p "$ruby_lock_check_dir"
  cp "$provisioning_dir/ruby/Gemfile" "$ruby_lock_check_dir/Gemfile"
  cp "$provisioning_dir/ruby/Gemfile.lock" "$ruby_lock_check_dir/Gemfile.lock"
  env -i "${provisioning_env[@]}" \
    "PATH=$ruby_root/bin:/usr/bin:/bin" \
    "BUNDLE_APP_CONFIG=$ruby_lock_check_dir/config" \
    "BUNDLE_CACHE_PATH=$ruby_contract_root/cache" \
    "BUNDLE_GEMFILE=$ruby_lock_check_dir/Gemfile" \
    "BUNDLE_PATH__SYSTEM=true" \
    "BUNDLE_USER_HOME=$ruby_lock_check_dir/user" \
    "$ruby_root/bin/bundle" lock --local
  observed_gemfile_lock_sha256=$(shasum -a 256 "$ruby_lock_check_dir/Gemfile.lock" | awk '{print $1}')
  if [[ $observed_gemfile_lock_sha256 != "$gemfile_lock_sha256" ]]; then
    echo "error: Bundler would change the frozen Gemfile.lock" >&2
    exit 1
  fi
  env -i "${provisioning_env[@]}" \
    "PATH=$ruby_root/bin:/usr/bin:/bin" \
    "${ruby_env[@]}" \
    "$ruby_root/bin/bundle" binstubs asciidoctor rubocop \
      --path "$ruby_contract_root/bin" --force
  native_bundles_after=$(find "$ruby_root" -type f -name '*.bundle' -print | LC_ALL=C sort)
  if [[ $native_bundles_after != "$native_bundles_before" ]]; then
    echo "error: Ruby bootstrap unexpectedly changed the native-extension closure" >&2
    exit 1
  fi
  printf '%s\n' "$ruby_contract_identity" >"$ruby_root/.velvet-glove-contract.json"
  verify_macho_closure "$ruby_root" ruby
fi

echo "==> Running $selection with a cleared environment and denied network"
env -i "${case_env[@]}" \
  "$mise_bin" -C "$provisioning_dir" exec --locked --fresh-env --deny-net -- \
  "$inner_runner" "$state_dir" "$artifact_dir" "$selection" "$mise_version"

echo "Pinned contract evidence: $artifact_dir/pinned-environment.json"
