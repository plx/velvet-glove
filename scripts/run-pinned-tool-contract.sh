#!/usr/bin/env bash
# shellcheck disable=SC2016 # Dollar-prefixed names in single quotes are jq variables.
set -euo pipefail

repository_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
provisioning_dir="$repository_root/crates/hookkit-pkl-config/validation/provisioning"
registry="$provisioning_dir/recipes.json"
inner_runner="$repository_root/scripts/run-pinned-tool-contract-inner.sh"
state_dir=${VELVET_GLOVE_PINNED_TOOL_STATE_DIR:-"$repository_root/target/pinned-tool-environments"}
artifact_dir=${VELVET_GLOVE_PINNED_TOOL_ARTIFACT_DIR:-"$state_dir/artifacts"}

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
run_home=$(mktemp -d "/private/tmp/velvet-glove-pinned.XXXXXX")
rust_extract_dir=
ruby_extract_dir=
cleanup() {
  case $run_home in
    /private/tmp/velvet-glove-pinned.*) rm -rf -- "$run_home" ;;
  esac
  case $rust_extract_dir in
    "$state_dir"/rust-extract.*) rm -rf -- "$rust_extract_dir" ;;
  esac
  case $ruby_extract_dir in
    "$state_dir"/ruby-extract.*) rm -rf -- "$ruby_extract_dir" ;;
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
install_tools jq@1.8.1
jq_root=$(env -i "${provisioning_env[@]}" "$mise_bin" where jq@1.8.1)
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
    | select(.integrity.kind == "sha256-archive")' "$registry")
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

if needs_group python; then
  echo "==> Installing the hash-locked Python wheel graph"
  provision_exec python -m venv --clear "$state_dir/python-venv"
  provision_exec "$state_dir/python-venv/bin/python" -m pip install \
    --require-hashes \
    --only-binary=:all: \
    --requirement "$provisioning_dir/python/requirements-macos-arm64.txt"
fi

if needs_group ruby; then
  ruby_archive=$(fetch_component_archive ruby)
  ruby_root="$state_dir/ruby-runtime-3.4.10-rubocop-1.30.1"
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
  ruby_contract_root="$state_dir/ruby-contract-1.30.1"
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
  if [[ $ruby_gem_count -ne 12 ]]; then
    echo "error: expected 12 checksum-locked Ruby gem packages, found $ruby_gem_count" >&2
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
    "$ruby_root/bin/bundle" binstubs rubocop --path "$ruby_contract_root/bin" --force
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
