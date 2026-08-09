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
ruby_extract_dir=
betterleaks_build_dir=
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
  case $ruby_extract_dir in
    "$state_dir"/ruby-extract.*) rm -rf -- "$ruby_extract_dir" ;;
  esac
  case $betterleaks_build_dir in
    "$state_dir"/betterleaks-build-1.7.3-vg1) rm -rf -- "$betterleaks_build_dir" ;;
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
    first((.sharedComponents + [.environments[].components[]])[] | select(.id == $id))
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
   | [.recipes[] | select(.toolId as $tool | $tools | index($tool)) | .environmentId] as $environmentIds
   | [.environments[] | select(.id as $id | $environmentIds | index($id)) | .provisioningGroup]
   | unique | join(",")' "$registry")

tool_specs=()
while IFS= read -r tool_spec; do
  tool_specs+=("$tool_spec")
done < <("$jq_bin" -r --arg selection "$selection" '
  ($selection | split(",") | map(split("/")[0])) as $tools
  | ([.recipes[] | select(.toolId as $tool | $tools | index($tool)) | .environmentId]
     | unique) as $environmentIds
  | (.sharedComponents
     + [.environments[]
        | select(.id as $id | $environmentIds | index($id))
        | .components[]])
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
