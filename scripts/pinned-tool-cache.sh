#!/usr/bin/env bash
# shellcheck disable=SC2016 # Dollar-prefixed names in single-quoted jq programs.

# Shared cache identity helpers for the outer provisioner and denied-network
# runner. Callers remain responsible for validating the installed closure.

pinned_component_install_identity() {
  if [[ $# -ne 3 ]]; then
    echo "error: pinned_component_install_identity requires JQ REGISTRY COMPONENT_ID" >&2
    return 2
  fi
  local jq_bin=$1
  local registry=$2
  local component_id=$3

  "$jq_bin" -ce --arg id "$component_id" '
    first((.sharedComponents + [.environments[].components[]])[] | select(.id == $id))
    | select(.installComponents | type == "array" and length > 0)
    | {
        integrity: {id, version, integrity},
        installedComponents: .installComponents
      }' "$registry"
}

pinned_component_cache_root() {
  if [[ $# -ne 3 ]]; then
    echo "error: pinned_component_cache_root requires STATE_DIR LABEL IDENTITY" >&2
    return 2
  fi
  local state_dir=$1
  local label=$2
  local identity=$3
  local digest

  case $state_dir in
    /*) ;;
    *)
      echo "error: pinned component state directory must be absolute: $state_dir" >&2
      return 2
      ;;
  esac
  case $label in
    *[!a-z0-9.-]* | '')
      echo "error: unsafe pinned component cache label: $label" >&2
      return 2
      ;;
  esac
  if [[ -z $identity || $identity == *$'\n'* ]]; then
    echo "error: pinned component cache identity must be one nonempty line" >&2
    return 2
  fi
  read -r digest _ < <(printf '%s' "$identity" | /usr/bin/shasum -a 256)
  if [[ ! $digest =~ ^[0-9a-f]{64}$ ]]; then
    echo "error: cannot hash pinned component cache identity" >&2
    return 2
  fi
  printf '%s/%s-%s\n' "$state_dir" "$label" "$digest"
}

pinned_component_cache_valid() {
  if [[ $# -lt 3 ]]; then
    echo "error: pinned_component_cache_valid requires ROOT IDENTITY EXECUTABLE..." >&2
    return 2
  fi
  local root=$1
  local identity=$2
  shift 2
  local executable

  if [[ ! -d $root || -L $root ]]; then
    return 1
  fi
  if [[ ! -f $root/.velvet-glove-artifacts.json || \
    -L $root/.velvet-glove-artifacts.json || \
    $(<"$root/.velvet-glove-artifacts.json") != "$identity" ]]; then
    return 1
  fi
  for executable in "$@"; do
    case $executable in
      bin/*) ;;
      *)
        echo "error: unsafe pinned component executable path: $executable" >&2
        return 2
        ;;
    esac
    if [[ $executable == *..* || ! -f $root/$executable || \
      -L $root/$executable || ! -x $root/$executable ]]; then
      return 1
    fi
  done
}
