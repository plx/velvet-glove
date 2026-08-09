#!/bin/sh
set -eu

: "${VELVET_GLOVE_TOOL_TRACE_DIR:?missing tool trace directory}"
: "${VELVET_GLOVE_TOOL_TRACE_SENTINEL:?missing tool trace sentinel}"

logical_program=${0##*/}
real_program_file="$0.real-program"
if [ ! -f "$real_program_file" ]; then
  printf '%s\n' "missing real tool program binding: $real_program_file" >&2
  exit 2
fi
IFS= read -r real_program <"$real_program_file"
case $real_program in
  /*) ;;
  *)
    printf '%s\n' "invalid real tool program binding: $real_program" >&2
    exit 2
    ;;
esac

invocations_dir="$VELVET_GLOVE_TOOL_TRACE_DIR/invocations"
/bin/mkdir -p "$invocations_dir"

index=1
while ! /bin/mkdir "$invocations_dir/$(printf '%04d' "$index")" 2>/dev/null; do
  index=$((index + 1))
done
record="$invocations_dir/$(printf '%04d' "$index")"

printf '%s\n' "$0" >"$record/program"
printf '%s\n' "$logical_program" >"$record/logical-program"
printf '%s\n' "$real_program" >"$record/real-program"
printf '%s\n' 'pass-through' >"$record/execution"
pwd -P >"$record/cwd"
printf '%s\n' "$#" >"$record/argc"
printf '%s\n' "${LANG-}" >"$record/env-LANG"
printf '%s\n' "${LC_ALL-}" >"$record/env-LC_ALL"
printf '%s\n' "${TZ-}" >"$record/env-TZ"
printf '%s\n' "${NO_COLOR-}" >"$record/env-NO_COLOR"
printf '%s\n' "${CLICOLOR-}" >"$record/env-CLICOLOR"
printf '%s\n' "${FORCE_COLOR-}" >"$record/env-FORCE_COLOR"
printf '%s\n' "${TERM-}" >"$record/env-TERM"
printf '%s\n' "${NODE_PATH-}" >"$record/env-NODE_PATH"
printf '%s\n' "${ASTRO_TELEMETRY_DISABLED-}" >"$record/env-ASTRO_TELEMETRY_DISABLED"
printf '%s\n' "${CI-}" >"$record/env-CI"
printf '%s\n' "${DEBUG-}" >"$record/env-DEBUG"
if [ "$logical_program" = node ]; then
  printf '%s\n' "${PATH-}" >"$record/env-PATH"
  printf '%s\n' "${HOME-}" >"$record/env-HOME"
  printf '%s\n' "${TMPDIR-}" >"$record/env-TMPDIR"
  printf '%s\n' "${XDG_CACHE_HOME-}" >"$record/env-XDG_CACHE_HOME"
  printf '%s\n' "${NODE_DISABLE_COLORS-}" >"$record/env-NODE_DISABLE_COLORS"
  printf '%s\n' "${NODE_DEBUG-}" >"$record/env-NODE_DEBUG"
  printf '%s\n' "${NODE_EXTRA_CA_CERTS-}" >"$record/env-NODE_EXTRA_CA_CERTS"
  printf '%s\n' "${NODE_NO_WARNINGS-}" >"$record/env-NODE_NO_WARNINGS"
  printf '%s\n' "${NODE_PENDING_DEPRECATION-}" >"$record/env-NODE_PENDING_DEPRECATION"
  printf '%s\n' "${NODE_REPL_HISTORY-}" >"$record/env-NODE_REPL_HISTORY"
  printf '%s\n' "${NODE_V8_COVERAGE-}" >"$record/env-NODE_V8_COVERAGE"
  printf '%s\n' "${NODE_VELVET_GLOVE_POISON-}" >"$record/env-NODE_VELVET_GLOVE_POISON"
  printf '%s\n' "${UV_THREADPOOL_SIZE-}" >"$record/env-UV_THREADPOOL_SIZE"
  printf '%s\n' "${NPM_CONFIG_USERCONFIG-}" >"$record/env-NPM_CONFIG_USERCONFIG"
  printf '%s\n' "${npm_config_userconfig-}" >"$record/env-npm_config_userconfig"
  printf '%s\n' "${SSL_CERT_FILE-}" >"$record/env-SSL_CERT_FILE"
  printf '%s\n' "${PRETTIER_DEBUG-}" >"$record/env-PRETTIER_DEBUG"
  printf '%s\n' "${PRETTIER_EXPERIMENTAL_CLI-}" >"$record/env-PRETTIER_EXPERIMENTAL_CLI"
  printf '%s\n' "${PRETTIER_PERF_REPEAT-}" >"$record/env-PRETTIER_PERF_REPEAT"
  printf '%s\n' "${PRETTIER_VELVET_GLOVE_POISON-}" >"$record/env-PRETTIER_VELVET_GLOVE_POISON"
  printf '%s\n' "${DYLD_FALLBACK_LIBRARY_PATH-}" >"$record/env-DYLD_FALLBACK_LIBRARY_PATH"
  printf '%s\n' "${DYLD_FALLBACK_FRAMEWORK_PATH-}" >"$record/env-DYLD_FALLBACK_FRAMEWORK_PATH"
  printf '%s\n' "${DYLD_FRAMEWORK_PATH-}" >"$record/env-DYLD_FRAMEWORK_PATH"
  printf '%s\n' "${DYLD_INSERT_LIBRARIES-}" >"$record/env-DYLD_INSERT_LIBRARIES"
  printf '%s\n' "${DYLD_LIBRARY_PATH-}" >"$record/env-DYLD_LIBRARY_PATH"
  printf '%s\n' "${DYLD_PRINT_LIBRARIES-}" >"$record/env-DYLD_PRINT_LIBRARIES"
  printf '%s\n' "${LD_LIBRARY_PATH-}" >"$record/env-LD_LIBRARY_PATH"
  printf '%s\n' "${LD_PRELOAD-}" >"$record/env-LD_PRELOAD"
  printf '%s\n' "${LD_VELVET_GLOVE_POISON-}" >"$record/env-LD_VELVET_GLOVE_POISON"
  printf '%s\n' "${CONTEXTLINT_VELVET_GLOVE_POISON-}" >"$record/env-CONTEXTLINT_VELVET_GLOVE_POISON"
fi
if [ "$logical_program" = buf ]; then
  printf '%s\n' "${PATH-}" >"$record/env-PATH"
  printf '%s\n' "${HOME-}" >"$record/env-HOME"
  printf '%s\n' "${TMPDIR-}" >"$record/env-TMPDIR"
  printf '%s\n' "${XDG_CACHE_HOME-}" >"$record/env-XDG_CACHE_HOME"
  printf '%s\n' "${DIFF_OPTIONS-}" >"$record/env-DIFF_OPTIONS"
  printf '%s\n' "${BUF_ALPHA_SUPPRESS_WARNINGS-}" >"$record/env-BUF_ALPHA_SUPPRESS_WARNINGS"
  printf '%s\n' "${BUF_BETA_COPY_FILES_TO_MEMORY-}" >"$record/env-BUF_BETA_COPY_FILES_TO_MEMORY"
  printf '%s\n' "${BUF_BETA_SUPPRESS_WARNINGS-}" >"$record/env-BUF_BETA_SUPPRESS_WARNINGS"
  printf '%s\n' "${BUF_BUFIMAGEUTIL_SHOULD_UPDATE_EXPECTATIONS-}" >"$record/env-BUF_BUFIMAGEUTIL_SHOULD_UPDATE_EXPECTATIONS"
  printf '%s\n' "${BUF_CACHE_DIR-}" >"$record/env-BUF_CACHE_DIR"
  printf '%s\n' "${BUF_INPUT_HTTPS_PASSWORD-}" >"$record/env-BUF_INPUT_HTTPS_PASSWORD"
  printf '%s\n' "${BUF_INPUT_HTTPS_USERNAME-}" >"$record/env-BUF_INPUT_HTTPS_USERNAME"
  printf '%s\n' "${BUF_INPUT_SSH_KEY_FILE-}" >"$record/env-BUF_INPUT_SSH_KEY_FILE"
  printf '%s\n' "${BUF_INPUT_SSH_KNOWN_HOSTS_FILES-}" >"$record/env-BUF_INPUT_SSH_KNOWN_HOSTS_FILES"
  printf '%s\n' "${BUF_TESTING_LEGACY_FEDERATION_REGISTRY-}" >"$record/env-BUF_TESTING_LEGACY_FEDERATION_REGISTRY"
  printf '%s\n' "${BUF_TESTING_PUBLIC_REGISTRY-}" >"$record/env-BUF_TESTING_PUBLIC_REGISTRY"
  printf '%s\n' "${BUF_TOKEN-}" >"$record/env-BUF_TOKEN"
  printf '%s\n' "${BUF_VELVET_GLOVE_POISON-}" >"$record/env-BUF_VELVET_GLOVE_POISON"
fi
if [ "$logical_program" = gofmt ]; then
  printf '%s\n' "${PATH-}" >"$record/env-PATH"
  printf '%s\n' "${GODEBUG-}" >"$record/env-GODEBUG"
  printf '%s\n' "${GOENV-}" >"$record/env-GOENV"
  printf '%s\n' "${GOMAXPROCS-}" >"$record/env-GOMAXPROCS"
  printf '%s\n' "${GOTELEMETRY-}" >"$record/env-GOTELEMETRY"
  printf '%s\n' "${GOTOOLCHAIN-}" >"$record/env-GOTOOLCHAIN"
  printf '%s\n' "${GOCACHE-}" >"$record/env-GOCACHE"
  printf '%s\n' "${GOFLAGS-}" >"$record/env-GOFLAGS"
  printf '%s\n' "${GONOSUMDB-}" >"$record/env-GONOSUMDB"
  printf '%s\n' "${GOPATH-}" >"$record/env-GOPATH"
  printf '%s\n' "${GOPROXY-}" >"$record/env-GOPROXY"
  printf '%s\n' "${GOSUMDB-}" >"$record/env-GOSUMDB"
  printf '%s\n' "${GOTMPDIR-}" >"$record/env-GOTMPDIR"
  printf '%s\n' "${GOWORK-}" >"$record/env-GOWORK"
  printf '%s\n' "${GO_VELVET_GLOVE_POISON-}" >"$record/env-GO_VELVET_GLOVE_POISON"
  printf '%s\n' "${DYLD_INSERT_LIBRARIES-}" >"$record/env-DYLD_INSERT_LIBRARIES"
  printf '%s\n' "${DYLD_PRINT_LIBRARIES-}" >"$record/env-DYLD_PRINT_LIBRARIES"
  printf '%s\n' "${LD_LIBRARY_PATH-}" >"$record/env-LD_LIBRARY_PATH"
  printf '%s\n' "${LD_PRELOAD-}" >"$record/env-LD_PRELOAD"
fi
if [ "$logical_program" = dclint ]; then
  printf '%s\n' "${PATH-}" >"$record/env-PATH"
  printf '%s\n' "${TMPDIR-}" >"$record/env-TMPDIR"
  printf '%s\n' "${NODE_NO_WARNINGS-}" >"$record/env-NODE_NO_WARNINGS"
  printf '%s\n' "${NODE_VELVET_GLOVE_POISON-}" >"$record/env-NODE_VELVET_GLOVE_POISON"
  printf '%s\n' "${DCLINT_CONFIG-}" >"$record/env-DCLINT_CONFIG"
  printf '%s\n' "${DYLD_INSERT_LIBRARIES-}" >"$record/env-DYLD_INSERT_LIBRARIES"
  printf '%s\n' "${DYLD_PRINT_LIBRARIES-}" >"$record/env-DYLD_PRINT_LIBRARIES"
  printf '%s\n' "${LD_LIBRARY_PATH-}" >"$record/env-LD_LIBRARY_PATH"
  printf '%s\n' "${LD_PRELOAD-}" >"$record/env-LD_PRELOAD"
fi
if [ "$logical_program" = cargo ] || [ "$logical_program" = cargo-clippy ] || [ "$logical_program" = cargo-fmt ] || [ "$logical_program" = rustfmt ]; then
  printf '%s\n' "${PATH-}" >"$record/env-PATH"
  printf '%s\n' "${TMPDIR-}" >"$record/env-TMPDIR"
  printf '%s\n' "${DYLD_LIBRARY_PATH-}" >"$record/env-DYLD_LIBRARY_PATH"
  printf '%s\n' "${DYLD_FALLBACK_LIBRARY_PATH-}" >"$record/env-DYLD_FALLBACK_LIBRARY_PATH"
  printf '%s\n' "${DYLD_FALLBACK_FRAMEWORK_PATH-}" >"$record/env-DYLD_FALLBACK_FRAMEWORK_PATH"
  printf '%s\n' "${DYLD_FRAMEWORK_PATH-}" >"$record/env-DYLD_FRAMEWORK_PATH"
  printf '%s\n' "${DYLD_INSERT_LIBRARIES-}" >"$record/env-DYLD_INSERT_LIBRARIES"
  printf '%s\n' "${DYLD_PRINT_LIBRARIES-}" >"$record/env-DYLD_PRINT_LIBRARIES"
  printf '%s\n' "${LD_LIBRARY_PATH-}" >"$record/env-LD_LIBRARY_PATH"
  printf '%s\n' "${LD_PRELOAD-}" >"$record/env-LD_PRELOAD"
  printf '%s\n' "${CARGO-}" >"$record/env-CARGO"
  printf '%s\n' "${RUSTC-}" >"$record/env-RUSTC"
  printf '%s\n' "${RUSTDOC-}" >"$record/env-RUSTDOC"
  printf '%s\n' "${RUSTFMT-}" >"$record/env-RUSTFMT"
  printf '%s\n' "${CARGO_HOME-}" >"$record/env-CARGO_HOME"
  printf '%s\n' "${CARGO_TARGET_DIR-}" >"$record/env-CARGO_TARGET_DIR"
  if [ -L "${CARGO_TARGET_DIR-}" ]; then
    printf '%s\n' symlink >"$record/env-CARGO_TARGET_DIR-kind"
  elif [ -d "${CARGO_TARGET_DIR-}" ]; then
    printf '%s\n' directory >"$record/env-CARGO_TARGET_DIR-kind"
  else
    printf '%s\n' missing >"$record/env-CARGO_TARGET_DIR-kind"
  fi
  printf '%s\n' "${CARGO_NET_OFFLINE-}" >"$record/env-CARGO_NET_OFFLINE"
  printf '%s\n' "${CARGO_BUILD_JOBS-}" >"$record/env-CARGO_BUILD_JOBS"
  printf '%s\n' "${CARGO_TERM_COLOR-}" >"$record/env-CARGO_TERM_COLOR"
  printf '%s\n' "${CARGO_ENCODED_RUSTFLAGS-}" >"$record/env-CARGO_ENCODED_RUSTFLAGS"
  printf '%s\n' "${RUSTFLAGS-}" >"$record/env-RUSTFLAGS"
  printf '%s\n' "${RUSTC_WRAPPER-}" >"$record/env-RUSTC_WRAPPER"
  printf '%s\n' "${RUSTC_WORKSPACE_WRAPPER-}" >"$record/env-RUSTC_WORKSPACE_WRAPPER"
  printf '%s\n' "${CLIPPY_CONF_DIR-}" >"$record/env-CLIPPY_CONF_DIR"
  if [ -L "${CLIPPY_CONF_DIR-}" ]; then
    printf '%s\n' symlink >"$record/env-CLIPPY_CONF_DIR-kind"
  elif [ -d "${CLIPPY_CONF_DIR-}" ]; then
    printf '%s\n' directory >"$record/env-CLIPPY_CONF_DIR-kind"
  else
    printf '%s\n' missing >"$record/env-CLIPPY_CONF_DIR-kind"
  fi
  if [ -L "${CLIPPY_CONF_DIR-}/clippy.toml" ]; then
    printf '%s\n' symlink >"$record/env-CLIPPY_CONF_DIR-clippy-toml-kind"
  elif [ -f "${CLIPPY_CONF_DIR-}/clippy.toml" ]; then
    if [ -s "${CLIPPY_CONF_DIR-}/clippy.toml" ]; then
      printf '%s\n' nonempty-file >"$record/env-CLIPPY_CONF_DIR-clippy-toml-kind"
    else
      printf '%s\n' empty-file >"$record/env-CLIPPY_CONF_DIR-clippy-toml-kind"
    fi
  else
    printf '%s\n' missing >"$record/env-CLIPPY_CONF_DIR-clippy-toml-kind"
  fi
  if [ -L "${CLIPPY_CONF_DIR-}/.clippy.toml" ]; then
    printf '%s\n' symlink >"$record/env-CLIPPY_CONF_DIR-dot-clippy-toml-kind"
  elif [ -f "${CLIPPY_CONF_DIR-}/.clippy.toml" ]; then
    printf '%s\n' file >"$record/env-CLIPPY_CONF_DIR-dot-clippy-toml-kind"
  else
    printf '%s\n' missing >"$record/env-CLIPPY_CONF_DIR-dot-clippy-toml-kind"
  fi
  printf '%s\n' "${CARGO_BUILD_RUSTC-}" >"$record/env-CARGO_BUILD_RUSTC"
  printf '%s\n' "${CARGO_BUILD_RUSTC_WRAPPER-}" >"$record/env-CARGO_BUILD_RUSTC_WRAPPER"
  printf '%s\n' "${CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER-}" >"$record/env-CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
  printf '%s\n' "${CARGO_BUILD_RUSTDOC-}" >"$record/env-CARGO_BUILD_RUSTDOC"
  printf '%s\n' "${CARGO_BUILD_TARGET-}" >"$record/env-CARGO_BUILD_TARGET"
  printf '%s\n' "${CARGO_ENCODED_RUSTDOCFLAGS-}" >"$record/env-CARGO_ENCODED_RUSTDOCFLAGS"
  printf '%s\n' "${CARGO_INCREMENTAL-}" >"$record/env-CARGO_INCREMENTAL"
  printf '%s\n' "${CARGO_PROFILE_DEV_DEBUG-}" >"$record/env-CARGO_PROFILE_DEV_DEBUG"
  printf '%s\n' "${CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS-}" >"$record/env-CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS"
  printf '%s\n' "${CLIPPY_ARGS-}" >"$record/env-CLIPPY_ARGS"
  printf '%s\n' "${CLIPPY_DISABLE_DOCS_LINKS-}" >"$record/env-CLIPPY_DISABLE_DOCS_LINKS"
  printf '%s\n' "${CLIPPY_DRIVER_DISABLE_DOCS_LINKS-}" >"$record/env-CLIPPY_DRIVER_DISABLE_DOCS_LINKS"
  printf '%s\n' "${CLIPPY_TERMINAL_WIDTH-}" >"$record/env-CLIPPY_TERMINAL_WIDTH"
  printf '%s\n' "${RUSTC_BOOTSTRAP-}" >"$record/env-RUSTC_BOOTSTRAP"
  printf '%s\n' "${RUSTDOCFLAGS-}" >"$record/env-RUSTDOCFLAGS"
  printf '%s\n' "${RUSTUP_TOOLCHAIN-}" >"$record/env-RUSTUP_TOOLCHAIN"
  printf '%s\n' "${SCCACHE_CACHE_SIZE-}" >"$record/env-SCCACHE_CACHE_SIZE"
  printf '%s\n' "${SCCACHE_DIR-}" >"$record/env-SCCACHE_DIR"
  printf '%s\n' "${SCCACHE_ENDPOINT-}" >"$record/env-SCCACHE_ENDPOINT"
  printf '%s\n' "${SCCACHE_ERROR_LOG-}" >"$record/env-SCCACHE_ERROR_LOG"
  printf '%s\n' "${SCCACHE_LOG-}" >"$record/env-SCCACHE_LOG"
  printf '%s\n' "${CCACHE_CONFIGPATH-}" >"$record/env-CCACHE_CONFIGPATH"
  printf '%s\n' "${CCACHE_DIR-}" >"$record/env-CCACHE_DIR"
  printf '%s\n' "${CCACHE_PREFIX-}" >"$record/env-CCACHE_PREFIX"
  printf '%s\n' "${CARGO_VELVET_GLOVE_POISON-}" >"$record/env-CARGO_VELVET_GLOVE_POISON"
  printf '%s\n' "${RUST_VELVET_GLOVE_POISON-}" >"$record/env-RUST_VELVET_GLOVE_POISON"
  printf '%s\n' "${CLIPPY_VELVET_GLOVE_POISON-}" >"$record/env-CLIPPY_VELVET_GLOVE_POISON"
  printf '%s\n' "${SCCACHE_VELVET_GLOVE_POISON-}" >"$record/env-SCCACHE_VELVET_GLOVE_POISON"
  printf '%s\n' "${CCACHE_VELVET_GLOVE_POISON-}" >"$record/env-CCACHE_VELVET_GLOVE_POISON"
  printf '%s\n' "${SYSROOT-}" >"$record/env-SYSROOT"
fi
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
dclint_config=
for argument in "$@"; do
  printf '%s\n' "$argument" >"$record/argv-$argument_index"
  if [ "$logical_program" = dclint ]; then
    case $argument in
      --config=*)
        if [ -n "$dclint_config" ]; then
          printf '%s\n' multiple >"$record/dclint-config-kind"
          exit 2
        fi
        dclint_config=${argument#--config=}
        ;;
    esac
  fi
  argument_index=$((argument_index + 1))
done

if [ "$logical_program" = dclint ]; then
  printf '%s\n' "$dclint_config" >"$record/dclint-config-path"
  if [ -z "$dclint_config" ]; then
    printf '%s\n' missing >"$record/dclint-config-kind"
  elif [ -L "$dclint_config" ]; then
    printf '%s\n' symlink >"$record/dclint-config-kind"
  elif [ -f "$dclint_config" ]; then
    printf '%s\n' file >"$record/dclint-config-kind"
    /usr/bin/stat -f '%Lp' "$dclint_config" >"$record/dclint-config-mode"
    /usr/bin/stat -f '%l' "$dclint_config" >"$record/dclint-config-links"
    /usr/bin/wc -c <"$dclint_config" | /usr/bin/tr -d ' ' >"$record/dclint-config-bytes"
    /usr/bin/shasum -a 256 "$dclint_config" >"$record/dclint-config-shasum"
    IFS=' ' read -r dclint_config_sha256 _ <"$record/dclint-config-shasum"
    printf '%s\n' "$dclint_config_sha256" >"$record/dclint-config-sha256"
    /bin/rm "$record/dclint-config-shasum"
  else
    printf '%s\n' other >"$record/dclint-config-kind"
  fi
  dclint_config_parent=${dclint_config%/*}
  printf '%s\n' "$dclint_config_parent" >"$record/dclint-config-parent"
  if [ -L "$dclint_config_parent" ]; then
    printf '%s\n' symlink >"$record/dclint-config-parent-kind"
  elif [ -d "$dclint_config_parent" ]; then
    printf '%s\n' directory >"$record/dclint-config-parent-kind"
    /usr/bin/stat -f '%Lp' "$dclint_config_parent" >"$record/dclint-config-parent-mode"
  else
    printf '%s\n' missing >"$record/dclint-config-parent-kind"
  fi
fi

set +e
"$real_program" "$@"
status=$?
set -e
printf '%s\n' "$status" >"$record/status"
exit "$status"
