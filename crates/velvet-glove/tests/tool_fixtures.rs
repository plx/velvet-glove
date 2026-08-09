//! Fixture-driven validation for Velvet Glove's built-in immediate workflows.
//!
//! The non-ignored tests fail closed on fixture discovery and prove, with a
//! hermetic executable, that every native protocol reaches a subprocess through
//! the real `velvet-glove` binary. The opt-in test additionally executes the
//! host's real tools against the checked-in golden corpus.

#[path = "support/process.rs"]
mod bounded_process;
mod support;

use bounded_process::{BoundedCommandError, BoundedOutput, run_with_timeout};
use hookkit_pkl_config::{
    ArgToken, ArgvElement, CheckScope, ExitCodes, InvocationGranularity, Phase, PhaseMode,
    ToolSpec, UnexpectedExitPolicy, WorkflowCommand, WriteBehavior,
};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use support::native_events::{PostToolUseBuilder, ProtocolSurface, canonical_project};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::process::Stdio;

const REAL_TOOL_SURFACES: &[ProtocolSurface] = &[ProtocolSurface::Claude, ProtocolSurface::Codex];
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const TIMEOUT_ENV: &str = "VELVET_GLOVE_FIXTURE_TIMEOUT_SECS";
const ARTIFACT_ENV: &str = "VELVET_GLOVE_FIXTURE_ARTIFACT_DIR";
const REQUIRED_TOOLS_ENV: &str = "VELVET_GLOVE_FIXTURE_REQUIRED_TOOLS";
const SELECTION_ENV: &str = "VELVET_GLOVE_FIXTURE_SELECTION";
const REPORT_PREFIX: &str = "VELVET_GLOVE_FIXTURE_JSON=";
const PROBE_SENTINEL_ENV: &str = "VELVET_GLOVE_FIXTURE_PROBE_SENTINEL";
const PROBE_DIR_ENV: &str = "VELVET_GLOVE_FIXTURE_PROBE_DIR";
const TOOL_TRACE_DIR_ENV: &str = "VELVET_GLOVE_TOOL_TRACE_DIR";
const TOOL_TRACE_SENTINEL_ENV: &str = "VELVET_GLOVE_TOOL_TRACE_SENTINEL";
const TOOL_TRACE_SENTINEL: &str = "real-tool-fixture";
const PATH_ENV: &str = "PATH";
const HOME_ENV: &str = "HOME";
const TMPDIR_ENV: &str = "TMPDIR";
const PWD_ENV: &str = "PWD";
const XDG_CONFIG_HOME_ENV: &str = "XDG_CONFIG_HOME";
const XDG_CACHE_HOME_ENV: &str = "XDG_CACHE_HOME";
const DIFF_OPTIONS_ENV: &str = "DIFF_OPTIONS";
const NODE_PATH_ENV: &str = "NODE_PATH";
const ASTRO_TELEMETRY_DISABLED_ENV: &str = "ASTRO_TELEMETRY_DISABLED";
const CI_ENV: &str = "CI";
const DEBUG_ENV: &str = "DEBUG";
const BETTERLEAKS_CONFIG_ENV: &str = "BETTERLEAKS_CONFIG";
const BETTERLEAKS_CONFIG_TOML_ENV: &str = "BETTERLEAKS_CONFIG_TOML";
const GITLEAKS_CONFIG_ENV: &str = "GITLEAKS_CONFIG";
const GITLEAKS_CONFIG_TOML_ENV: &str = "GITLEAKS_CONFIG_TOML";
const BETTERLEAKS_POISON_ENV_VALUE: &str = "velvet-glove-adapter-must-clear-this";
const VACUUM_POISON_ENV_VALUE: &str = "velvet-glove-vacuum-adapter-must-clear-this";
const VACUUM_CHILD_PATH: &str = "/usr/bin:/bin";
const VACUUM_PREFIX_SCRUBBED_ENV: &[&str] = &[
    "VACUUM_CONFIG",
    "VACUUM_FIX",
    "VACUUM_FUNCTIONS",
    "VACUUM_HARD_MODE",
    "VACUUM_REMOTE",
    "VACUUM_RULESET",
    "VACUUM_VELVET_GLOVE_POISON",
];
const VACUUM_SCRUBBED_ENV: &[&str] = &[
    "ALL_PROXY",
    DEBUG_ENV,
    "GOGC",
    "GOMEMLIMIT",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "SSL_CERT_DIR",
    "SSL_CERT_FILE",
];
const VACUUM_LOADER_SCRUBBED_ENV: &[&str] = &[
    "DYLD_INSERT_LIBRARIES",
    "DYLD_PRINT_LIBRARIES",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
];
const BIOME_POISON_ENV_VALUE: &str = "velvet-glove-biome-adapter-must-clear-this";
const PRETTIER_POISON_ENV_VALUE: &str = "velvet-glove-prettier-adapter-must-clear-this";
const PRETTIER_ROOT_ENV: &str = "VELVET_GLOVE_FIXTURE_PRETTIER_ROOT";
const ESLINT_POISON_ENV_VALUE: &str = "velvet-glove-eslint-adapter-must-clear-this";
const ESLINT_ROOT_ENV: &str = "VELVET_GLOVE_FIXTURE_ESLINT_ROOT";
const CONTEXTLINT_ROOT_ENV: &str = "VELVET_GLOVE_FIXTURE_CONTEXTLINT_ROOT";
const DCLINT_ROOT_ENV: &str = "VELVET_GLOVE_FIXTURE_DCLINT_ROOT";
const PRETTIER_CHILD_PATH: &str = "/usr/bin:/bin";
const PRETTIER_SCRUBBED_ENV: &[&str] = &[
    DEBUG_ENV,
    "NODE_DEBUG",
    "NODE_EXTRA_CA_CERTS",
    "NODE_NO_WARNINGS",
    "NODE_OPTIONS",
    NODE_PATH_ENV,
    "NODE_PENDING_DEPRECATION",
    "NODE_REPL_HISTORY",
    "NODE_V8_COVERAGE",
    "NODE_VELVET_GLOVE_POISON",
    "PRETTIER_DEBUG",
    "PRETTIER_EXPERIMENTAL_CLI",
    "PRETTIER_PERF_REPEAT",
    "PRETTIER_VELVET_GLOVE_POISON",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "DYLD_FALLBACK_FRAMEWORK_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_PRINT_LIBRARIES",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "LD_VELVET_GLOVE_POISON",
];
const ESLINT_CHILD_PATH: &str = "/usr/bin:/bin";
const ESLINT_SCRUBBED_ENV: &[&str] = &[
    DEBUG_ENV,
    "ESLINT_USE_FLAT_CONFIG",
    "ESLINT_CODE_PATH",
    "ESLINT_VELVET_GLOVE_POISON",
    "NODE_DEBUG",
    "NODE_EXTRA_CA_CERTS",
    "NODE_NO_WARNINGS",
    "NODE_OPTIONS",
    NODE_PATH_ENV,
    "NODE_PENDING_DEPRECATION",
    "NODE_REPL_HISTORY",
    "NODE_V8_COVERAGE",
    "NODE_VELVET_GLOVE_POISON",
    "NPM_CONFIG_USERCONFIG",
    "npm_config_userconfig",
    "SSL_CERT_FILE",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "DYLD_FALLBACK_FRAMEWORK_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_PRINT_LIBRARIES",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "LD_VELVET_GLOVE_POISON",
];
const BUF_POISON_ENV_VALUE: &str = "velvet-glove-buf-adapter-must-clear-this";
const GOFMT_POISON_ENV_VALUE: &str = "velvet-glove-gofmt-adapter-must-clear-this";
const GOFMT_CHILD_PATH: &str = "/usr/bin:/bin";
const GOFMT_CONTROLLED_ENV: &[(&str, &str)] = &[
    ("GODEBUG", ""),
    ("GOENV", "off"),
    ("GOMAXPROCS", "1"),
    ("GOTELEMETRY", "off"),
    ("GOTOOLCHAIN", "local"),
];
const GOFMT_SCRUBBED_ENV: &[&str] = &[
    "GOCACHE",
    "GOFLAGS",
    "GONOSUMDB",
    "GOPATH",
    "GOPROXY",
    "GOSUMDB",
    "GOTMPDIR",
    "GOWORK",
    "GO_VELVET_GLOVE_POISON",
    DEBUG_ENV,
];
const GOFMT_LOADER_SCRUBBED_ENV: &[&str] = &[
    "DYLD_INSERT_LIBRARIES",
    "DYLD_PRINT_LIBRARIES",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
];
const BUF_CACHE_DIR_ENV: &str = "BUF_CACHE_DIR";
const BUF_CHILD_PATH: &str = "/usr/bin:/bin";
const BUF_DIFF_PROGRAM: &str = "/usr/bin/diff";
const BUF_SCRUBBED_ENV: &[&str] = &[
    "BUF_ALPHA_SUPPRESS_WARNINGS",
    "BUF_BETA_COPY_FILES_TO_MEMORY",
    "BUF_BETA_SUPPRESS_WARNINGS",
    "BUF_BUFIMAGEUTIL_SHOULD_UPDATE_EXPECTATIONS",
    "BUF_INPUT_HTTPS_PASSWORD",
    "BUF_INPUT_HTTPS_USERNAME",
    "BUF_INPUT_SSH_KEY_FILE",
    "BUF_INPUT_SSH_KNOWN_HOSTS_FILES",
    "BUF_TESTING_LEGACY_FEDERATION_REGISTRY",
    "BUF_TESTING_PUBLIC_REGISTRY",
    "BUF_TOKEN",
    "BUF_VELVET_GLOVE_POISON",
    DEBUG_ENV,
];
const CARGO_CLIPPY_TOOLCHAIN_ROOT_ENV: &str = "VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT";
const CARGO_CLIPPY_POISON_ENV_VALUE: &str = "velvet-glove-cargo-clippy-adapter-must-clear-this";
const CARGO_FMT_POISON_ENV_VALUE: &str = "velvet-glove-cargo-fmt-adapter-must-clear-this";
const CARGO_PROGRAM_ENV: &str = "CARGO";
const RUSTC_ENV: &str = "RUSTC";
const RUSTDOC_ENV: &str = "RUSTDOC";
const RUSTFMT_ENV: &str = "RUSTFMT";
const DYLD_LIBRARY_PATH_ENV: &str = "DYLD_LIBRARY_PATH";
const CARGO_HOME_ENV: &str = "CARGO_HOME";
const CARGO_TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";
const CARGO_NET_OFFLINE_ENV: &str = "CARGO_NET_OFFLINE";
const CARGO_BUILD_JOBS_ENV: &str = "CARGO_BUILD_JOBS";
const CARGO_TERM_COLOR_ENV: &str = "CARGO_TERM_COLOR";
const CLIPPY_CONF_DIR_ENV: &str = "CLIPPY_CONF_DIR";
const CARGO_CLIPPY_EMPTY_ENV: &[&str] = &[
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTFLAGS",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
];
const CARGO_CLIPPY_SCRUBBED_ENV: &[&str] = &[
    "CARGO_BUILD_RUSTC",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTDOC",
    "CARGO_BUILD_TARGET",
    "CARGO_ENCODED_RUSTDOCFLAGS",
    "CARGO_PROFILE_DEV_DEBUG",
    "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS",
    "CLIPPY_ARGS",
    "CLIPPY_DRIVER_DISABLE_DOCS_LINKS",
    "CLIPPY_TERMINAL_WIDTH",
    "RUSTC_BOOTSTRAP",
    "RUST_LOG",
    "RUSTDOCFLAGS",
    "RUSTUP_TOOLCHAIN",
    "SCCACHE_CACHE_SIZE",
    "SCCACHE_DIR",
    "SCCACHE_ENDPOINT",
    "SCCACHE_ERROR_LOG",
    "SCCACHE_LOG",
    "CCACHE_CONFIGPATH",
    "CCACHE_DIR",
    "CCACHE_PREFIX",
    DEBUG_ENV,
    "SYSROOT",
];
const CARGO_CLIPPY_PREFIX_POISON_ENV: &[&str] = &[
    "CARGO_VELVET_GLOVE_POISON",
    "RUST_VELVET_GLOVE_POISON",
    "CLIPPY_VELVET_GLOVE_POISON",
    "SCCACHE_VELVET_GLOVE_POISON",
    "CCACHE_VELVET_GLOVE_POISON",
];
const CARGO_FMT_EMPTY_ENV: &[&str] = &[
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTFLAGS",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
];
const CARGO_FMT_SCRUBBED_ENV: &[&str] = &[
    "CARGO_BUILD_RUSTC",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTDOC",
    "CARGO_BUILD_TARGET",
    "CARGO_ENCODED_RUSTDOCFLAGS",
    "CARGO_PROFILE_DEV_DEBUG",
    "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS",
    "RUSTC_BOOTSTRAP",
    "RUST_LOG",
    "RUSTDOCFLAGS",
    "RUSTUP_TOOLCHAIN",
    "SCCACHE_CACHE_SIZE",
    "SCCACHE_DIR",
    "SCCACHE_ENDPOINT",
    "SCCACHE_ERROR_LOG",
    "SCCACHE_LOG",
    "CCACHE_CONFIGPATH",
    "CCACHE_DIR",
    "CCACHE_PREFIX",
    DEBUG_ENV,
    "SYSROOT",
];
const CARGO_FMT_PREFIX_POISON_ENV: &[&str] = &[
    "CARGO_VELVET_GLOVE_POISON",
    "RUST_VELVET_GLOVE_POISON",
    "SCCACHE_VELVET_GLOVE_POISON",
    "CCACHE_VELVET_GLOVE_POISON",
];
const CARGO_CLIPPY_LOADER_SCRUBBED_ENV: &[&str] = &[
    "DYLD_FALLBACK_LIBRARY_PATH",
    "DYLD_FALLBACK_FRAMEWORK_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_PRINT_LIBRARIES",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
];
const RAYON_NUM_THREADS_ENV: &str = "RAYON_NUM_THREADS";
const BIOME_SCRUBBED_ENV: &[&str] = &[
    "BIOME_BINARY",
    "BIOME_THREADS",
    "NODE_OPTIONS",
    NODE_PATH_ENV,
    "BIOME_CONFIG_PATH",
    "BIOME_LOG_FILE",
    "BIOME_LOG_PREFIX_NAME",
    "BIOME_LOG_PATH",
    "BIOME_LOG_LEVEL",
    "BIOME_LOG_KIND",
    "RUST_LOG",
    "RUST_BACKTRACE",
    "RUST_LIB_BACKTRACE",
    DEBUG_ENV,
];
const CONTEXTLINT_POISON_ENV_VALUE: &str = "velvet-glove-contextlint-adapter-must-clear-this";
const CONTEXTLINT_CHILD_PATH: &str = "/usr/bin:/bin";
const CONTEXTLINT_SCRUBBED_ENV: &[&str] = &[
    "NODE_OPTIONS",
    NODE_PATH_ENV,
    "NPM_CONFIG_USERCONFIG",
    "npm_config_userconfig",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    DEBUG_ENV,
    "CONTEXTLINT_VELVET_GLOVE_POISON",
];
const CONTEXTLINT_POISONED_ENV: &[&str] = &[
    "NODE_OPTIONS",
    NODE_PATH_ENV,
    "NPM_CONFIG_USERCONFIG",
    "npm_config_userconfig",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    DEBUG_ENV,
    "CONTEXTLINT_VELVET_GLOVE_POISON",
];
const DCLINT_FILES_MARKER: &str = "__VELVET_GLOVE_DCLINT_FILES__";
const DCLINT_PRIVATE_CONFIG_ARGUMENT: &str = "--config=<private-config>";
const DCLINT_PRIVATE_CONFIG_SHA256: &str =
    "d18839f8d0202f6139bdc65206a222a792df2a57b7f1e308636f6e652e548fbf";
const DCLINT_POISON_ENV_VALUE: &str = "velvet-glove-dclint-adapter-must-clear-this";
const DCLINT_SCRUBBED_ENV: &[&str] = &[
    "DCLINT_CONFIG",
    "DEBUG",
    "NODE_OPTIONS",
    NODE_PATH_ENV,
    "NODE_VELVET_GLOVE_POISON",
];
const DCLINT_LOADER_SCRUBBED_ENV: &[&str] = &[
    "DYLD_INSERT_LIBRARIES",
    "DYLD_PRINT_LIBRARIES",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
];
const DCLINT_TRACE_PLAN: TracePlan = TracePlan::DclintFixSubsetFilesMarker {
    fixable_targets: &[],
};
const DCLINT_AUTOFIX_TRACE_PLAN: TracePlan = TracePlan::DclintFixSubsetFilesMarker {
    fixable_targets: &["docker-compose.yml"],
};
static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedOutcome {
    Clean,
    Issues,
    OperationalFailure,
}

impl ExpectedOutcome {
    fn summary_status(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Issues => "issues",
            Self::OperationalFailure => "operational-failure",
        }
    }

    fn artifact_classification(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Issues => "issues",
            Self::OperationalFailure => "failure",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TracePlan {
    Direct,
    PrivateProbeThenContextlintWorkspace,
    PrivateConfigThenGhalintWorkspace,
    SingleNestedTrailingOptions {
        trailing: &'static [&'static str],
    },
    SingleNestedFilesMarker {
        nested_program_index: usize,
        adapter_prefix: &'static [&'static str],
        marker: &'static str,
        leading: &'static [&'static str],
        before_files: &'static [&'static str],
    },
    VacuumPrivateFilesMarker {
        nested_program_index: usize,
        adapter_prefix: &'static [&'static str],
        marker: &'static str,
        leading: &'static [&'static str],
    },
    SingleNestedModeFilesMarker {
        nested_program_index: usize,
        adapter_prefix: &'static [&'static str],
        marker: &'static str,
        leading: &'static [&'static str],
        mode_arguments: &'static [(&'static str, &'static [&'static str])],
        before_files: &'static [&'static str],
    },
    PreflightThenNestedModeFilesMarker {
        nested_program_index: usize,
        adapter_prefix: &'static [&'static str],
        marker: &'static str,
        mode_arguments: &'static [(&'static str, &'static [&'static [&'static str]])],
    },
    DclintFixSubsetFilesMarker {
        fixable_targets: &'static [&'static str],
    },
    PairedNodeModeFilesMarker {
        node_program_index: usize,
        tool_program_index: usize,
        adapter_prefix: &'static [&'static str],
        marker: &'static str,
        leading: &'static [&'static str],
        format_preflight_arguments: &'static [&'static str],
        mode_arguments: &'static [(&'static str, &'static [&'static str])],
        before_files: &'static [&'static str],
    },
    EslintPrivateModeFilesMarker,
    PreflightThenNestedModeWorkspaceMarker {
        nested_program_index: usize,
        adapter_prefix: &'static [&'static str],
        marker: &'static str,
        preflight: &'static [&'static str],
        leading: &'static [&'static str],
        mode_arguments: &'static [(&'static str, &'static [&'static str])],
        before_workspace: &'static [&'static str],
    },
    PreflightThenNestedModeWorkspaceIndicatorMarker {
        preflight_program_index: usize,
        command_program_index: usize,
        adapter_prefix: &'static [&'static str],
        marker: &'static str,
        modes: &'static [&'static str],
        preflight_before_indicator: &'static [&'static str],
        preflight_after_indicator: &'static [&'static str],
        command_before_indicator: &'static [&'static str],
        command_after_indicators: &'static [&'static [&'static str]],
    },
    CargoFmtWorkspaceIndicatorMarker {
        adapter_prefix: &'static [&'static str],
        marker: &'static str,
        target_roots: &'static [&'static str],
        edition: &'static str,
    },
    TrailingOptionsAdapter {
        preflight: &'static [&'static str],
        validation: &'static [&'static str],
    },
}

const ASCIIDOCTOR_TRACE_PLAN: TracePlan = TracePlan::TrailingOptionsAdapter {
    preflight: &[
        "--safe-mode=safe",
        "--failure-level=FATAL",
        "--out-file=/dev/null",
    ],
    validation: &[
        "--safe-mode=safe",
        "--failure-level=WARNING",
        "--out-file=/dev/null",
    ],
};

const ASTRO_TRACE_PLAN: TracePlan = TracePlan::SingleNestedTrailingOptions {
    trailing: &[
        "--silent",
        "--noSync",
        "--no-watch",
        "--root",
        ".",
        "--minimumSeverity=error",
        "--minimumFailingSeverity=error",
    ],
};

const CONTEXTLINT_FILES_MARKER: &str = "__VELVET_GLOVE_CONTEXTLINT_FILES__";
const CONTEXTLINT_TRACE_PLAN: TracePlan = TracePlan::PrivateProbeThenContextlintWorkspace;
const CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER: &str = "<contextlint-private>";
const CONTEXTLINT_PRIVATE_ROOT_PREFIX: &str = "velvet-glove-contextlint-probe-";

const GHALINT_WORKFLOW_FILES_MARKER: &str = "__VELVET_GLOVE_GHALINT_WORKFLOW_FILES__";
const GHALINT_PRIVATE_ROOT_PLACEHOLDER: &str = "<ghalint-private>";
const GHALINT_PRIVATE_ROOT_PREFIX: &str = "velvet-glove-ghalint-workflow-";
const GHALINT_TRACE_PLAN: TracePlan = TracePlan::PrivateConfigThenGhalintWorkspace;

const BETTERLEAKS_FILES_MARKER: &str = "__VELVET_GLOVE_BETTERLEAKS_FILES__";
const BETTERLEAKS_FIXED_ARGUMENTS: &[&str] = &[
    "--redact=100",
    "--verbose=true",
    "--no-color=true",
    "--no-banner=true",
    "--exit-code=10",
    "--log-level=fatal",
    "--legacy-print=true",
];
const BETTERLEAKS_TRACE_PLAN: TracePlan = TracePlan::SingleNestedFilesMarker {
    nested_program_index: 3,
    adapter_prefix: &["-I", "-c"],
    marker: BETTERLEAKS_FILES_MARKER,
    leading: &["dir"],
    before_files: BETTERLEAKS_FIXED_ARGUMENTS,
};
const BETTERLEAKS_FIXTURE_SECRET: &str = "VG_SECRET_AbCdEf0123456789";

const VACUUM_FILES_MARKER: &str = "__VELVET_GLOVE_VACUUM_FILES__";
const VACUUM_FIXED_ARGUMENTS: &[&str] = &[
    "lint",
    "--config=vacuum.conf.yaml",
    "--base=.",
    "--no-update-check",
    "--remote=false",
    "--no-style",
    "--no-banner",
    "--details",
    "--errors",
    "--silent",
    "--all-results",
    "--no-clip",
    "--fail-severity=error",
    "--fix=false",
    "--timeout=5",
    "--lookup-timeout=500",
    "--turbo=false",
    "--hard-mode=false",
    "--skip-check=false",
    "--ext-refs=false",
    "--resolve-all-refs=false",
    "--nested-refs-doc-context=false",
    "--allow-private-networks=false",
    "--allow-http=false",
    "--fetch-timeout=5",
];
const VACUUM_TRACE_PLAN: TracePlan = TracePlan::VacuumPrivateFilesMarker {
    nested_program_index: 3,
    adapter_prefix: &["-I", "-c"],
    marker: VACUUM_FILES_MARKER,
    leading: VACUUM_FIXED_ARGUMENTS,
};

const BIOME_FILES_MARKER: &str = "__VELVET_GLOVE_BIOME_FILES__";
const BIOME_MODE_ARGUMENTS: &[(&str, &[&str])] = &[("fix", &["--write"]), ("verify", &[])];
const BIOME_ARGUMENTS_BEFORE_FILES: &[&str] = &[
    "--colors=off",
    "--reporter=json",
    "--max-diagnostics=none",
    "--error-on-warnings",
    "--no-errors-on-unmatched",
    "--",
];
const BIOME_TRACE_PLAN: TracePlan = TracePlan::SingleNestedModeFilesMarker {
    nested_program_index: 3,
    adapter_prefix: &["-I", "-c"],
    marker: BIOME_FILES_MARKER,
    leading: &["check"],
    mode_arguments: BIOME_MODE_ARGUMENTS,
    before_files: BIOME_ARGUMENTS_BEFORE_FILES,
};

const GOFMT_FILES_MARKER: &str = "__VELVET_GLOVE_GOFMT_FILES__";
const GOFMT_VERIFY_COMMANDS: &[&[&str]] = &[&["-l"]];
const GOFMT_WRITE_COMMANDS: &[&[&str]] = &[&["-l"], &["-w"]];
const GOFMT_MODE_ARGUMENTS: &[(&str, &[&[&str]])] = &[
    ("verify", GOFMT_VERIFY_COMMANDS),
    ("write", GOFMT_WRITE_COMMANDS),
];
const GOFMT_TRACE_PLAN: TracePlan = TracePlan::PreflightThenNestedModeFilesMarker {
    nested_program_index: 3,
    adapter_prefix: &["-I", "-c"],
    marker: GOFMT_FILES_MARKER,
    mode_arguments: GOFMT_MODE_ARGUMENTS,
};

const PRETTIER_FILES_MARKER: &str = "__VELVET_GLOVE_PRETTIER_FILES__";
const PRETTIER_MODE_ARGUMENTS: &[(&str, &[&str])] = &[
    ("format", &["--write", "--log-level=error"]),
    ("verify", &["--list-different", "--log-level=log"]),
];
const PRETTIER_FORMAT_PREFLIGHT_ARGUMENTS: &[&str] = &["--list-different", "--log-level=log"];
const PRETTIER_ARGUMENTS_BEFORE_FILES: &[&str] = &[
    "--no-editorconfig",
    "--ignore-path=/dev/null",
    "--with-node-modules",
    "--no-color",
    "--",
];
const PRETTIER_TRACE_PLAN: TracePlan = TracePlan::PairedNodeModeFilesMarker {
    node_program_index: 3,
    tool_program_index: 4,
    adapter_prefix: &["-I", "-c"],
    marker: PRETTIER_FILES_MARKER,
    leading: &["--config=/dev/null"],
    format_preflight_arguments: PRETTIER_FORMAT_PREFLIGHT_ARGUMENTS,
    mode_arguments: PRETTIER_MODE_ARGUMENTS,
    before_files: PRETTIER_ARGUMENTS_BEFORE_FILES,
};

const ESLINT_FILES_MARKER: &str = "__VELVET_GLOVE_ESLINT_FILES__";
const ESLINT_TRACE_PLAN: TracePlan = TracePlan::EslintPrivateModeFilesMarker;
const ESLINT_PRIVATE_ROOT_PLACEHOLDER: &str = "<eslint-private>";
const ESLINT_PRIVATE_ROOT_PREFIX: &str = "velvet-glove-eslint-";

const BUF_WORKSPACE_MARKER: &str = "__VELVET_GLOVE_BUF_WORKSPACE__";
const BUF_MODE_ARGUMENTS: &[(&str, &[&str])] = &[
    ("write", &["--write"]),
    ("verify", &["--diff", "--exit-code"]),
];
const BUF_TRACE_PLAN: TracePlan = TracePlan::PreflightThenNestedModeWorkspaceMarker {
    nested_program_index: 3,
    adapter_prefix: &["-I", "-c"],
    marker: BUF_WORKSPACE_MARKER,
    preflight: &["config", "ls-modules", "--log-format=text", "--format=json"],
    leading: &[
        "format",
        "--disable-symlinks",
        "--error-format=text",
        "--log-format=text",
    ],
    mode_arguments: BUF_MODE_ARGUMENTS,
    before_workspace: &[],
};

const CARGO_CLIPPY_WORKSPACE_MARKER: &str = "__VELVET_GLOVE_CARGO_CLIPPY_WORKSPACE__";
const CARGO_CLIPPY_TRACE_PLAN: TracePlan =
    TracePlan::PreflightThenNestedModeWorkspaceIndicatorMarker {
        preflight_program_index: 3,
        command_program_index: 4,
        adapter_prefix: &["-I", "-c"],
        marker: CARGO_CLIPPY_WORKSPACE_MARKER,
        modes: &["fix", "verify"],
        preflight_before_indicator: &[
            "metadata",
            "--format-version=1",
            "--no-deps",
            "--manifest-path",
        ],
        preflight_after_indicator: &["--frozen", "--quiet", "--color=never"],
        command_before_indicator: &["clippy", "--manifest-path"],
        command_after_indicators: &[
            &[
                "--workspace",
                "--all-targets",
                "--all-features",
                "--no-deps",
                "--frozen",
                "--quiet",
                "--jobs=1",
                "--keep-going",
                "--color=never",
                "--message-format=json",
                "--",
                "--cap-lints=allow",
            ],
            &[
                "--workspace",
                "--all-targets",
                "--all-features",
                "--no-deps",
                "--frozen",
                "--quiet",
                "--jobs=1",
                "--keep-going",
                "--color=never",
                "--message-format=json",
                "--",
                "-Dwarnings",
            ],
        ],
    };

const CARGO_FMT_WORKSPACE_MARKER: &str = "__VELVET_GLOVE_CARGO_FMT_WORKSPACE__";
const CARGO_FMT_PRIVATE_ROOT_PLACEHOLDER: &str = "<cargo-fmt-private>";
const CARGO_FMT_TRACE_PLAN: TracePlan = TracePlan::CargoFmtWorkspaceIndicatorMarker {
    adapter_prefix: &["-I", "-c"],
    marker: CARGO_FMT_WORKSPACE_MARKER,
    target_roots: &["src/example.rs"],
    edition: "2024",
};
const CARGO_FMT_MULTI_TRACE_PLAN: TracePlan = TracePlan::CargoFmtWorkspaceIndicatorMarker {
    adapter_prefix: &["-I", "-c"],
    marker: CARGO_FMT_WORKSPACE_MARKER,
    target_roots: &[
        "alpha/src/example.rs",
        "alpha/src/selected_clean.rs",
        "beta/src/workspace_only.rs",
    ],
    edition: "2024",
};
const CARGO_FMT_COVERAGE_FAILURE_TRACE_PLAN: TracePlan =
    TracePlan::CargoFmtWorkspaceIndicatorMarker {
        adapter_prefix: &["-I", "-c"],
        marker: CARGO_FMT_WORKSPACE_MARKER,
        target_roots: &["src/example.rs"],
        edition: "2024",
    };

#[derive(Debug)]
struct ExpectedInvocation {
    targets: &'static [&'static str],
    exit_code: i32,
    trace_exit_codes: &'static [i32],
}

#[derive(Debug)]
struct RealToolContractCase {
    phase_id: &'static str,
    invocations: &'static [ExpectedInvocation],
    extra_args: &'static [&'static str],
    outcome: ExpectedOutcome,
    diagnostic_contains: &'static [&'static str],
    diagnostic_excludes: &'static [&'static str],
    trace_plan: TracePlan,
}

#[derive(Debug)]
struct MutatingToolContractCase {
    remedy_phase_id: &'static str,
    remedy_mode: PhaseMode,
    remedy_writes: WriteBehavior,
    remedy_invocations: &'static [ExpectedInvocation],
    repeat_remedy_invocations: Option<&'static [ExpectedInvocation]>,
    final_invocations: &'static [ExpectedInvocation],
    immediate_outcome: ExpectedOutcome,
    changed_targets: &'static [&'static str],
}

impl RealToolContractCase {
    fn targets(&self) -> Vec<&'static str> {
        self.invocations
            .iter()
            .flat_map(|invocation| invocation.targets.iter().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

fn real_tool_contract_case(case: &FixtureCase) -> Result<Option<RealToolContractCase>, String> {
    let contract = match (case.tool.as_str(), case.case.as_str()) {
        ("contextlint", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["docs/example.Md"],
                exit_code: 0,
                trace_exit_codes: &[1, 0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &["127.0.0.1"],
            trace_plan: CONTEXTLINT_TRACE_PLAN,
        },
        ("contextlint", "source-issue") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["docs/example.md"],
                exit_code: 1,
                trace_exit_codes: &[1, 0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "CTX-001",
                "Section \\\"Overview\\\" contains only placeholder \\\"TODO\\\"",
            ],
            diagnostic_excludes: &["127.0.0.1", "/etc/passwd"],
            trace_plan: CONTEXTLINT_TRACE_PLAN,
        },
        ("contextlint", "multi-file-project") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["docs/example.md", "docs/selected-clean.md"],
                exit_code: 1,
                trace_exit_codes: &[1, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "(project)",
                "STR-001",
                "docs/unselected.md",
                "SEC-001",
                "docs/missing-architecture.md",
            ],
            diagnostic_excludes: &["127.0.0.1"],
            trace_plan: CONTEXTLINT_TRACE_PLAN,
        },
        ("contextlint", "operational-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["docs/example.md"],
                exit_code: 2,
                trace_exit_codes: &[1, 2],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &[
                "Access to this API has been restricted",
                "contextlint exited with operational status 2",
            ],
            diagnostic_excludes: &["127.0.0.1"],
            trace_plan: CONTEXTLINT_TRACE_PLAN,
        },
        ("contextlint", other) => {
            return Err(format!(
                "contextlint fixture {other:?} has no real-tool contract declaration"
            ));
        }
        ("ghalint-workflow", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &[".github/workflows/build.yml"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: GHALINT_TRACE_PLAN,
        },
        ("ghalint-workflow", "source-issue") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &[".github/workflows/example.yml"],
                exit_code: 1,
                trace_exit_codes: &[0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"policy\":\"job_permissions\"",
                "\"policy\":\"job_timeout_minutes_is_required\"",
                "\"file\":\".github/workflows/example.yml\"",
            ],
            diagnostic_excludes: &["Aug ", "reference="],
            trace_plan: GHALINT_TRACE_PLAN,
        },
        ("ghalint-workflow", "multi-workflow") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &[
                    ".github/workflows/example.yml",
                    ".github/workflows/selected.yaml",
                ],
                exit_code: 1,
                trace_exit_codes: &[0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"policy\":\"job_permissions\"",
                "\"file\":\".github/workflows/example.yml\"",
            ],
            diagnostic_excludes: &[
                "nested/ignored.yml",
                "\"file\":\".github/workflows/selected.yaml\"",
            ],
            trace_plan: GHALINT_TRACE_PLAN,
        },
        ("ghalint-workflow", "policy-grammar") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &[".github/workflows/example.yml"],
                exit_code: 1,
                trace_exit_codes: &[0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"message\":\"secret should not be set to workflow's env\"",
                "\"message\":\"github.token should not be set to workflow's env\"",
                "\"policy\":\"action_ref_should_be_full_length_commit_sha\"",
                "\"policy\":\"github_app_should_limit_permissions\"",
                "\"file\":\".github/workflows/example.yml\"",
            ],
            diagnostic_excludes: &["Aug ", "reference=", "env_name=", "action="],
            trace_plan: GHALINT_TRACE_PLAN,
        },
        ("ghalint-workflow", "config-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &[".github/workflows/example.yml"],
                exit_code: 2,
                trace_exit_codes: &[0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &[
                "velvet-glove-ghalint-workflow: ghalint configuration is invalid",
            ],
            diagnostic_excludes: &["\"status\":\"issues\"", "Aug "],
            trace_plan: GHALINT_TRACE_PLAN,
        },
        ("ghalint-workflow", "malformed") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &[".github/workflows/example.yml"],
                exit_code: 1,
                trace_exit_codes: &[0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"kind\":\"parse\"",
                "parse a workflow file as YAML:",
                "\"file\":\".github/workflows/example.yml\"",
            ],
            diagnostic_excludes: &["\"kind\":\"policy\"", "Aug "],
            trace_plan: GHALINT_TRACE_PLAN,
        },
        ("ghalint-workflow", other) => {
            return Err(format!(
                "ghalint-workflow fixture {other:?} has no real-tool contract declaration"
            ));
        }
        ("jq", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.json"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: TracePlan::Direct,
        },
        ("jq", "invalid") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.json"],
                exit_code: 5,
                trace_exit_codes: &[5],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["jq: parse error:"],
            diagnostic_excludes: &[],
            trace_plan: TracePlan::Direct,
        },
        ("jq", "operational-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.json"],
                exit_code: 2,
                trace_exit_codes: &[2],
            }],
            extra_args: &["--indent", "9"],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &["jq: --indent takes a number between -1 and 7"],
            diagnostic_excludes: &[],
            trace_plan: TracePlan::Direct,
        },
        ("jq", "multi-file-fragments") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[
                ExpectedInvocation {
                    targets: &["example.1-open.json"],
                    exit_code: 5,
                    trace_exit_codes: &[5],
                },
                ExpectedInvocation {
                    targets: &["example.2-close.json"],
                    exit_code: 5,
                    trace_exit_codes: &[5],
                },
            ],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["jq: parse error:"],
            diagnostic_excludes: &[],
            trace_plan: TracePlan::Direct,
        },
        ("asciidoctor", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.adoc"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: ASCIIDOCTOR_TRACE_PLAN,
        },
        ("asciidoctor", "missing-include") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.adoc"],
                exit_code: 1,
                trace_exit_codes: &[0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["include file not found:", "does-not-exist.adoc"],
            diagnostic_excludes: &[],
            trace_plan: ASCIIDOCTOR_TRACE_PLAN,
        },
        ("asciidoctor", "multi-file") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.1-clean.adoc", "example.2-missing-include.adoc"],
                exit_code: 1,
                trace_exit_codes: &[0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["include file not found:", "does-not-exist.adoc"],
            diagnostic_excludes: &[],
            trace_plan: ASCIIDOCTOR_TRACE_PLAN,
        },
        ("asciidoctor", "operational-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.adoc"],
                exit_code: 2,
                trace_exit_codes: &[1],
            }],
            extra_args: &["--backend=definitely-not-a-backend"],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &["missing converter for backend 'definitely-not-a-backend'"],
            diagnostic_excludes: &[],
            trace_plan: ASCIIDOCTOR_TRACE_PLAN,
        },
        ("astro", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/pages/example.astro"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: ASTRO_TRACE_PLAN,
        },
        ("astro", "type-error") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/pages/example.astro"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "ts(2322):",
                "Type 'string' is not assignable to type 'number'.",
            ],
            diagnostic_excludes: &[],
            trace_plan: ASTRO_TRACE_PLAN,
        },
        ("astro", "multi-file-project") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &[
                    "src/components/selected-clean.astro",
                    "src/pages/example.astro",
                ],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "src/components/broken.astro",
                "Type 'string' is not assignable to type 'number'.",
                "Result (3 files):",
            ],
            diagnostic_excludes: &[],
            trace_plan: ASTRO_TRACE_PLAN,
        },
        ("astro", "operational-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/pages/example.astro"],
                exit_code: 2,
                trace_exit_codes: &[1],
            }],
            extra_args: &["--tsconfig", "does-not-exist.json"],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &["does-not-exist.json"],
            diagnostic_excludes: &[],
            trace_plan: ASTRO_TRACE_PLAN,
        },
        ("betterleaks", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.txt"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &["--config=.betterleaks.toml"],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: BETTERLEAKS_TRACE_PLAN,
        },
        ("betterleaks", "finding") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.txt"],
                exit_code: 10,
                trace_exit_codes: &[10],
            }],
            extra_args: &["--config=.betterleaks.toml"],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "Secret:      REDACTED",
                "RuleID:      fixture-secret",
                "example.txt:fixture-secret:1",
            ],
            diagnostic_excludes: &[BETTERLEAKS_FIXTURE_SECRET],
            trace_plan: BETTERLEAKS_TRACE_PLAN,
        },
        ("betterleaks", "multi-file") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.secret.txt", "src/selected-clean.txt"],
                exit_code: 10,
                trace_exit_codes: &[10],
            }],
            extra_args: &["--config=.betterleaks.toml"],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "Secret:      REDACTED",
                "RuleID:      fixture-secret",
                "example.secret.txt:fixture-secret:1",
            ],
            diagnostic_excludes: &[BETTERLEAKS_FIXTURE_SECRET],
            trace_plan: BETTERLEAKS_TRACE_PLAN,
        },
        ("betterleaks", "operational-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.txt"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &["--config=does-not-exist.toml"],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &[
                "FTL unable to load config",
                "does-not-exist.toml",
                "no such file or directory",
            ],
            diagnostic_excludes: &[],
            trace_plan: BETTERLEAKS_TRACE_PLAN,
        },
        ("biome", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: BIOME_TRACE_PLAN,
        },
        ("biome", "autofix") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"category\":\"format\"",
                "\"path\":\"src/example.js\"",
                "Formatter would have printed the following content:",
            ],
            diagnostic_excludes: &[],
            trace_plan: BIOME_TRACE_PLAN,
        },
        ("biome", "source-issue") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"category\":\"parse\"",
                "\"path\":\"src/example.js\"",
                "Expected an expression",
            ],
            diagnostic_excludes: &[],
            trace_plan: BIOME_TRACE_PLAN,
        },
        ("biome", "multi-file") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.js", "src/selected-two.js"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["\"category\":\"format\"", "\"path\":\"src/example.js\""],
            diagnostic_excludes: &["src/unselected-sentinel.js"],
            trace_plan: BIOME_TRACE_PLAN,
        },
        ("biome", "operational-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 2,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &["biome.json", "configuration resulted in errors"],
            diagnostic_excludes: &["::error title=format"],
            trace_plan: BIOME_TRACE_PLAN,
        },
        ("eslint", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: ESLINT_TRACE_PLAN,
        },
        ("eslint", "source-issue") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["\"fatalErrorCount\":1", "\"filePath\":\"example.js\""],
            diagnostic_excludes: &[],
            trace_plan: ESLINT_TRACE_PLAN,
        },
        ("eslint", "autofix") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["\"ruleId\":\"semi\"", "\"filePath\":\"example.js\""],
            diagnostic_excludes: &[],
            trace_plan: ESLINT_TRACE_PLAN,
        },
        ("eslint", "multi-file") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &[
                    "example.js",
                    "src/selected-clean.mjs",
                    "src/selected-common.cjs",
                ],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["\"ruleId\":\"semi\"", "\"filePath\":\"example.js\""],
            diagnostic_excludes: &["unselected-sentinel.cjs"],
            trace_plan: ESLINT_TRACE_PLAN,
        },
        ("eslint", "config-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 2,
                trace_exit_codes: &[],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &["contains unsupported keys", "plugins"],
            diagnostic_excludes: &[],
            trace_plan: ESLINT_TRACE_PLAN,
        },
        ("prettier", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: PRETTIER_TRACE_PLAN,
        },
        ("prettier", "unformatted") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["prettier: formatting differs:", "example.js"],
            diagnostic_excludes: &[],
            trace_plan: PRETTIER_TRACE_PLAN,
        },
        ("prettier", "multi-file") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.js", "src/selected-clean.js"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["prettier: formatting differs:", "example.js"],
            diagnostic_excludes: &["unselected-sentinel.js"],
            trace_plan: PRETTIER_TRACE_PLAN,
        },
        ("prettier", "config-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 2,
                trace_exit_codes: &[1],
            }],
            extra_args: &["--print-width=wide"],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &[
                "Invalid --print-width value",
                "without valid list-different evidence",
            ],
            diagnostic_excludes: &[],
            trace_plan: PRETTIER_TRACE_PLAN,
        },
        ("buf-format", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.proto"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: BUF_TRACE_PLAN,
        },
        ("buf-format", "unformatted") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.proto"],
                exit_code: 100,
                trace_exit_codes: &[0, 100],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["example.proto.orig ", "example.proto\t<mtime>", "@@ -"],
            diagnostic_excludes: &[],
            trace_plan: BUF_TRACE_PLAN,
        },
        ("buf-format", "multi-file") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.proto", "proto/selected-clean.proto"],
                exit_code: 100,
                trace_exit_codes: &[0, 100],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "example.proto.orig ",
                "workspace-only.proto.orig ",
                "\t<mtime>",
            ],
            diagnostic_excludes: &["selected-clean.proto.orig"],
            trace_plan: BUF_TRACE_PLAN,
        },
        ("buf-format", "operational-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example.proto"],
                exit_code: 2,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &[
                "velvet-glove-buf-format: buf.yaml module scope omits workspace proto files: excluded/unformatted.proto",
            ],
            diagnostic_excludes: &[],
            trace_plan: BUF_TRACE_PLAN,
        },
        ("cargo-fmt", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 1, 0, 1, 0, 0, 0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: CARGO_FMT_TRACE_PLAN,
        },
        ("cargo-fmt", "source-issue") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 1,
                trace_exit_codes: &[0, 0, 1, 0, 1, 1, 0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"kind\":\"cargo-fmt\"",
                "\"status\":\"issues\"",
                "\"unformatted\":[\"src/example.rs\"]",
            ],
            diagnostic_excludes: &["\"status\":\"clean\""],
            trace_plan: CARGO_FMT_TRACE_PLAN,
        },
        ("cargo-fmt", "workspace-multi") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["alpha/src/example.rs", "alpha/src/selected_clean.rs"],
                exit_code: 1,
                trace_exit_codes: &[0, 0, 1, 0, 1, 1, 0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"packages\":2",
                "\"unformatted\":[\"alpha/src/example.rs\",\"beta/src/workspace_only.rs\"]",
            ],
            diagnostic_excludes: &["alpha/src/selected_clean.rs\"]"],
            trace_plan: CARGO_FMT_MULTI_TRACE_PLAN,
        },
        ("cargo-fmt", "operational-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 2,
                trace_exit_codes: &[0, 0, 1, 0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &[
                "rustfmt.toml",
                "max_width",
                "velvet-glove-cargo-fmt: cargo-fmt coverage check exited 1 without a clean issue report",
            ],
            diagnostic_excludes: &["\"status\":\"issues\""],
            trace_plan: CARGO_FMT_TRACE_PLAN,
        },
        ("cargo-fmt", "coverage-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 2,
                trace_exit_codes: &[0, 0, 1, 0, 1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &[
                "cargo-fmt does not cover the complete physical Rust source set",
                "missing=['src/main.rs']",
            ],
            diagnostic_excludes: &["\"status\":\"issues\""],
            trace_plan: CARGO_FMT_COVERAGE_FAILURE_TRACE_PLAN,
        },
        ("cargo-clippy", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: CARGO_CLIPPY_TRACE_PLAN,
        },
        ("cargo-clippy", "source-issue") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 1,
                trace_exit_codes: &[0, 0, 101],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"code\":\"clippy::ptr_arg\"",
                "\"file\":\"src/example.rs\"",
                "\"fixable\":false",
                "writing `&Vec` instead of `&[_]`",
            ],
            diagnostic_excludes: &["\"status\":\"fixed\""],
            trace_plan: CARGO_CLIPPY_TRACE_PLAN,
        },
        ("cargo-clippy", "workspace-autofix") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.rs", "src/selected_clean.rs"],
                exit_code: 1,
                trace_exit_codes: &[0, 0, 101],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"code\":\"clippy::useless_vec\"",
                "\"file\":\"src/example.rs\"",
                "\"file\":\"src/workspace_only.rs\"",
                "\"fixable\":true",
            ],
            diagnostic_excludes: &["\"file\":\"src/selected_clean.rs\""],
            trace_plan: CARGO_CLIPPY_TRACE_PLAN,
        },
        ("cargo-clippy", "operational-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 2,
                trace_exit_codes: &[0, 101],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &[
                "error reading Clippy's configuration file",
                "expected u64",
                "velvet-glove-cargo-clippy: Cargo reported a non-source or dependency compilation error",
            ],
            diagnostic_excludes: &["\"status\":\"issues\""],
            trace_plan: CARGO_CLIPPY_TRACE_PLAN,
        },
        ("dclint", "clean") => RealToolContractCase {
            phase_id: "fix",
            invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: DCLINT_TRACE_PLAN,
        },
        ("dclint", "source-issue") => RealToolContractCase {
            phase_id: "fix",
            invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"fixable\":false",
                "\"rule\":\"service-image-require-explicit-tag\"",
            ],
            diagnostic_excludes: &["<PRIVATE_CONFIG>"],
            trace_plan: DCLINT_TRACE_PLAN,
        },
        ("dclint", "validation-issue") => RealToolContractCase {
            phase_id: "fix",
            invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["\"fixable\":false", "\"rule\":\"invalid-yaml\""],
            diagnostic_excludes: &["<PRIVATE_CONFIG>"],
            trace_plan: DCLINT_TRACE_PLAN,
        },
        ("dclint", "autofix-multi-file") => RealToolContractCase {
            phase_id: "fix",
            invocations: &[ExpectedInvocation {
                targets: &["compose.selected-clean.yaml", "docker-compose.yml"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "\"fixable\":true",
                "\"rule\":\"services-alphabetical-order\"",
                "docker-compose.yml",
            ],
            diagnostic_excludes: &["compose.unselected-sentinel.yml"],
            trace_plan: DCLINT_AUTOFIX_TRACE_PLAN,
        },
        ("dclint", "operational-failure") => RealToolContractCase {
            phase_id: "fix",
            invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 2,
                trace_exit_codes: &[],
            }],
            extra_args: &["--config=config/unsafe.json"],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &[
                "velvet-glove-dclint: dclint config has unknown or executable-loading keys: ['extends']",
            ],
            diagnostic_excludes: &["Traceback", "node_modules"],
            trace_plan: DCLINT_TRACE_PLAN,
        },
        ("go-fmt", "clean") => RealToolContractCase {
            phase_id: "format",
            invocations: &[ExpectedInvocation {
                targets: &["example.go"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &[],
            trace_plan: GOFMT_TRACE_PLAN,
        },
        ("go-fmt", "unformatted") => RealToolContractCase {
            phase_id: "format",
            invocations: &[ExpectedInvocation {
                targets: &["example.go"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["example.go"],
            diagnostic_excludes: &[],
            trace_plan: GOFMT_TRACE_PLAN,
        },
        ("go-fmt", "multi-file") => RealToolContractCase {
            phase_id: "format",
            invocations: &[ExpectedInvocation {
                targets: &["example.go", "selected-clean.go"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &["example.go"],
            diagnostic_excludes: &["unselected-sentinel.go"],
            trace_plan: GOFMT_TRACE_PLAN,
        },
        ("go-fmt", "operational-failure") => RealToolContractCase {
            phase_id: "format",
            invocations: &[ExpectedInvocation {
                targets: &["example.go", "invalid.go"],
                exit_code: 2,
                trace_exit_codes: &[2],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &["invalid.go:3:15: expected ')', found '{'"],
            diagnostic_excludes: &["gofmt: changed example.go"],
            trace_plan: GOFMT_TRACE_PLAN,
        },
        ("vacuum", "clean") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example-openapi.yaml"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Clean,
            diagnostic_contains: &[],
            diagnostic_excludes: &["this project-local configuration"],
            trace_plan: VACUUM_TRACE_PLAN,
        },
        ("vacuum", "source-issue") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example-openapi.yaml"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "example-openapi.yaml:2:1",
                "`info` section must have a `description`",
                "$.info",
                "rule: info-description  category: Contract Information",
            ],
            diagnostic_excludes: &["this project-local configuration"],
            trace_plan: VACUUM_TRACE_PLAN,
        },
        ("vacuum", "multi-file") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example-openapi.yaml", "selected-clean-openapi.yaml"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::Issues,
            diagnostic_contains: &[
                "example-openapi.yaml:2:1",
                "`info` section must have a `description`",
                "$.info",
                "rule: info-description  category: Contract Information",
            ],
            diagnostic_excludes: &[
                "unselected-sentinel.yaml",
                "this project-local configuration",
            ],
            trace_plan: VACUUM_TRACE_PLAN,
        },
        ("vacuum", "operational-failure") => RealToolContractCase {
            phase_id: "verify",
            invocations: &[ExpectedInvocation {
                targets: &["example-openapi.yaml"],
                exit_code: 2,
                trace_exit_codes: &[2],
            }],
            extra_args: &[],
            outcome: ExpectedOutcome::OperationalFailure,
            diagnostic_contains: &[
                "Unable to process spec 'example-openapi.yaml'",
                "unable to parse specification",
                "did not find expected ',' or ']'",
            ],
            diagnostic_excludes: &["rule: info-description", "this project-local configuration"],
            trace_plan: VACUUM_TRACE_PLAN,
        },
        (
            "jq" | "asciidoctor" | "astro" | "betterleaks" | "biome" | "buf-format" | "cargo-fmt"
            | "cargo-clippy" | "dclint" | "go-fmt" | "prettier" | "vacuum",
            other,
        ) => {
            return Err(format!(
                "{} fixture {other:?} has no real-tool contract declaration",
                case.tool
            ));
        }
        _ => return Ok(None),
    };
    Ok(Some(contract))
}

fn mutating_tool_contract_case(
    case: &FixtureCase,
) -> Result<Option<MutatingToolContractCase>, String> {
    let contract = match (case.tool.as_str(), case.case.as_str()) {
        ("biome", "clean") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &[],
        },
        ("biome", "autofix") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["src/example.js"],
        },
        ("biome", "source-issue") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            immediate_outcome: ExpectedOutcome::Issues,
            changed_targets: &[],
        },
        ("biome", "multi-file") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.js", "src/selected-two.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["src/example.js", "src/selected-two.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["src/example.js"],
        },
        ("biome", "operational-failure") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.js"],
                exit_code: 2,
                trace_exit_codes: &[1],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[],
            immediate_outcome: ExpectedOutcome::OperationalFailure,
            changed_targets: &[],
        },
        ("eslint", "clean") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &[],
        },
        ("eslint", "source-issue") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 1,
                trace_exit_codes: &[1, 1],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            immediate_outcome: ExpectedOutcome::Issues,
            changed_targets: &[],
        },
        ("eslint", "autofix") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[1, 0, 0, 0],
            }],
            repeat_remedy_invocations: Some(&[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }]),
            final_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["example.js"],
        },
        ("eslint", "multi-file") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &[
                    "example.js",
                    "src/selected-clean.mjs",
                    "src/selected-common.cjs",
                ],
                exit_code: 0,
                trace_exit_codes: &[1, 0, 0, 0],
            }],
            repeat_remedy_invocations: Some(&[ExpectedInvocation {
                targets: &[
                    "example.js",
                    "src/selected-clean.mjs",
                    "src/selected-common.cjs",
                ],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }]),
            final_invocations: &[ExpectedInvocation {
                targets: &[
                    "example.js",
                    "src/selected-clean.mjs",
                    "src/selected-common.cjs",
                ],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["example.js"],
        },
        ("eslint", "config-failure") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 2,
                trace_exit_codes: &[],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[],
            immediate_outcome: ExpectedOutcome::OperationalFailure,
            changed_targets: &[],
        },
        ("prettier", "clean") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &[],
        },
        ("prettier", "unformatted") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[1, 0],
            }],
            repeat_remedy_invocations: Some(&[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }]),
            final_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["example.js"],
        },
        ("prettier", "multi-file") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.js", "src/selected-clean.js"],
                exit_code: 0,
                trace_exit_codes: &[1, 0],
            }],
            repeat_remedy_invocations: Some(&[ExpectedInvocation {
                targets: &["example.js", "src/selected-clean.js"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }]),
            final_invocations: &[ExpectedInvocation {
                targets: &["example.js", "src/selected-clean.js"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["example.js"],
        },
        ("prettier", "config-failure") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.js"],
                exit_code: 2,
                trace_exit_codes: &[1],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[],
            immediate_outcome: ExpectedOutcome::OperationalFailure,
            changed_targets: &[],
        },
        ("buf-format", "clean") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.proto"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["example.proto"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &[],
        },
        ("buf-format", "unformatted") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.proto"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["example.proto"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["example.proto"],
        },
        ("buf-format", "multi-file") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.proto", "proto/selected-clean.proto"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["example.proto", "proto/selected-clean.proto"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["example.proto", "proto/workspace-only.proto"],
        },
        ("buf-format", "operational-failure") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.proto"],
                exit_code: 2,
                trace_exit_codes: &[0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[],
            immediate_outcome: ExpectedOutcome::OperationalFailure,
            changed_targets: &[],
        },
        ("cargo-fmt", "clean") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 1, 0, 1, 0, 0, 0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 1, 0, 1, 0, 0, 0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &[],
        },
        ("cargo-fmt", "source-issue") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 1, 0, 1, 0, 0, 0],
            }],
            repeat_remedy_invocations: Some(&[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 1, 0, 1, 0, 0, 0],
            }]),
            final_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 1, 0, 1, 0, 0, 0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["src/example.rs"],
        },
        ("cargo-fmt", "workspace-multi") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["alpha/src/example.rs", "alpha/src/selected_clean.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 1, 0, 1, 0, 0, 0],
            }],
            repeat_remedy_invocations: Some(&[ExpectedInvocation {
                targets: &["alpha/src/example.rs", "alpha/src/selected_clean.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 1, 0, 1, 0, 0, 0],
            }]),
            final_invocations: &[ExpectedInvocation {
                targets: &["alpha/src/example.rs", "alpha/src/selected_clean.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 1, 0, 1, 0, 0, 0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["alpha/src/example.rs", "beta/src/workspace_only.rs"],
        },
        ("cargo-fmt", "operational-failure") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 2,
                trace_exit_codes: &[0, 0, 1, 0, 1],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[],
            immediate_outcome: ExpectedOutcome::OperationalFailure,
            changed_targets: &[],
        },
        ("cargo-fmt", "coverage-failure") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 2,
                trace_exit_codes: &[0, 0, 1, 0, 1],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[],
            immediate_outcome: ExpectedOutcome::OperationalFailure,
            changed_targets: &[],
        },
        ("cargo-clippy", "clean") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &[],
        },
        ("cargo-clippy", "source-issue") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 1,
                trace_exit_codes: &[0, 0, 101],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 1,
                trace_exit_codes: &[0, 0, 101],
            }],
            immediate_outcome: ExpectedOutcome::Issues,
            changed_targets: &[],
        },
        ("cargo-clippy", "workspace-autofix") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs", "src/selected_clean.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 101],
            }],
            repeat_remedy_invocations: Some(&[ExpectedInvocation {
                targets: &["src/example.rs", "src/selected_clean.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 0],
            }]),
            final_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs", "src/selected_clean.rs"],
                exit_code: 0,
                trace_exit_codes: &[0, 0, 0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["src/example.rs", "src/workspace_only.rs"],
        },
        ("cargo-clippy", "operational-failure") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::Workspace,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["src/example.rs"],
                exit_code: 2,
                trace_exit_codes: &[0, 101],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[],
            immediate_outcome: ExpectedOutcome::OperationalFailure,
            changed_targets: &[],
        },
        ("dclint", "clean") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &[],
        },
        ("dclint", "source-issue") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            immediate_outcome: ExpectedOutcome::Issues,
            changed_targets: &[],
        },
        ("dclint", "validation-issue") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 1,
                trace_exit_codes: &[1],
            }],
            immediate_outcome: ExpectedOutcome::Issues,
            changed_targets: &[],
        },
        ("dclint", "autofix-multi-file") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["compose.selected-clean.yaml", "docker-compose.yml"],
                exit_code: 0,
                trace_exit_codes: &[1, 0, 0],
            }],
            repeat_remedy_invocations: Some(&[ExpectedInvocation {
                targets: &["compose.selected-clean.yaml", "docker-compose.yml"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }]),
            final_invocations: &[ExpectedInvocation {
                targets: &["compose.selected-clean.yaml", "docker-compose.yml"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["docker-compose.yml"],
        },
        ("dclint", "operational-failure") => MutatingToolContractCase {
            remedy_phase_id: "fix",
            remedy_mode: PhaseMode::Fix,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["docker-compose.yml"],
                exit_code: 2,
                trace_exit_codes: &[],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[],
            immediate_outcome: ExpectedOutcome::OperationalFailure,
            changed_targets: &[],
        },
        ("go-fmt", "clean") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.go"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["example.go"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &[],
        },
        ("go-fmt", "unformatted") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.go"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["example.go"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["example.go"],
        },
        ("go-fmt", "multi-file") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.go", "selected-clean.go"],
                exit_code: 0,
                trace_exit_codes: &[0, 0],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[ExpectedInvocation {
                targets: &["example.go", "selected-clean.go"],
                exit_code: 0,
                trace_exit_codes: &[0],
            }],
            immediate_outcome: ExpectedOutcome::Clean,
            changed_targets: &["example.go"],
        },
        ("go-fmt", "operational-failure") => MutatingToolContractCase {
            remedy_phase_id: "format",
            remedy_mode: PhaseMode::Format,
            remedy_writes: WriteBehavior::TargetFiles,
            remedy_invocations: &[ExpectedInvocation {
                targets: &["example.go", "invalid.go"],
                exit_code: 2,
                trace_exit_codes: &[2],
            }],
            repeat_remedy_invocations: None,
            final_invocations: &[],
            immediate_outcome: ExpectedOutcome::OperationalFailure,
            changed_targets: &[],
        },
        (
            "biome" | "prettier" | "buf-format" | "cargo-fmt" | "cargo-clippy" | "dclint"
            | "go-fmt",
            other,
        ) => {
            return Err(format!(
                "{} fixture {other:?} has no mutating-tool contract declaration",
                case.tool
            ));
        }
        _ => return Ok(None),
    };
    Ok(Some(contract))
}

#[test]
fn fixture_inventory_is_non_empty_and_has_no_orphans() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let catalog = discover_fixture_catalog(&fixtures_root(), &specs)
        .unwrap_or_else(|error| panic!("fixture discovery failed: {error}"));

    let report = serde_json::json!({
        "formatVersion": 1,
        "kind": "inventory",
        "totals": {
            "tools": catalog.tool_count,
            "cases": catalog.cases.len(),
            "fixtureSurfaces": REAL_TOOL_SURFACES.len(),
            "protocolProbeSurfaces": ProtocolSurface::ALL.len(),
            "plannedSurfaceCases": catalog.cases.len() * REAL_TOOL_SURFACES.len(),
        }
    });
    println!("{REPORT_PREFIX}{report}");
}

#[test]
fn probe_reaches_external_command_on_every_surface() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    let artifact_dir = configured_artifact_dir().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let commands = run_probe_matrix(timeout, artifact_dir.as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(commands, ProtocolSurface::ALL.len());
}

#[test]
fn subprocess_timeout_retains_partial_output() {
    let root = unique_temp_dir("velvet-glove-timeout-test");
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "printf started; printf diagnostic >&2; sleep 5"]);
    let result = run_with_timeout(
        &mut command,
        &[],
        Duration::from_millis(500),
        &root.join("capture"),
    );
    match result {
        Err(BoundedCommandError::Timeout { stdout, stderr, .. }) => {
            assert_eq!(stdout, b"started");
            assert_eq!(stderr, b"diagnostic");
        }
        other => panic!("expected a bounded timeout, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[cfg(unix)]
fn subprocess_timeout_terminates_descendants() {
    let root = unique_temp_dir("velvet-glove-timeout-descendant-test");
    let marker = root.join("descendant-survived");
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("(sleep 1; printf leaked > \"$1\") & printf started; wait")
        .arg("fixture-timeout")
        .arg(&marker);

    let result = run_with_timeout(
        &mut command,
        &[],
        Duration::from_millis(300),
        &root.join("capture"),
    );
    assert!(
        matches!(result, Err(BoundedCommandError::Timeout { .. })),
        "expected process-tree timeout, got {result:?}"
    );
    std::thread::sleep(Duration::from_millis(1_100));
    assert!(
        !marker.exists(),
        "a timed-out descendant continued mutating files"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn discovery_rejects_zero_cases_and_orphan_tools() {
    let empty_root = unique_temp_dir("velvet-glove-empty-fixtures");
    std::fs::create_dir_all(&empty_root).expect("empty fixture root");
    let empty_error = discover_fixture_catalog(&empty_root, &BTreeMap::new())
        .expect_err("empty fixture inventory must fail");
    assert!(empty_error.contains("zero tool directories"));

    let orphan_root = unique_temp_dir("velvet-glove-orphan-fixtures");
    let orphan_case = orphan_root.join("orphan-tool/example");
    std::fs::create_dir_all(&orphan_case).expect("orphan fixture root");
    std::fs::write(orphan_case.join("example.txt"), "fixture").expect("orphan input");
    let orphan_error = discover_fixture_catalog(&orphan_root, &BTreeMap::new())
        .expect_err("orphan fixture must fail");
    assert!(orphan_error.contains("orphan fixture tool directory"));

    let _ = std::fs::remove_dir_all(empty_root);
    let _ = std::fs::remove_dir_all(orphan_root);
}

#[test]
fn discovery_rejects_goldens_for_unexecuted_surfaces() {
    let case = unique_temp_dir("velvet-glove-unsupported-golden");
    std::fs::write(case.join("example.txt"), "fixture").expect("fixture input");
    std::fs::write(case.join("antigravity.json"), "{}").expect("unsupported golden");

    let error = validate_supported_goldens(&case)
        .expect_err("goldens outside the real fixture matrix must fail closed");
    assert!(error.contains("antigravity"));
    assert!(error.contains("not executed"));

    let _ = std::fs::remove_dir_all(case);
}

#[test]
fn entry_discovery_finds_one_nested_project_source() {
    let case = unique_temp_dir("velvet-glove-nested-entry");
    let pages = case.join("src/pages");
    std::fs::create_dir_all(&pages).expect("nested fixture source directory");
    std::fs::write(case.join("package.json"), "{}\n").expect("project manifest");
    std::fs::write(pages.join("example.astro"), "<h1>fixture</h1>\n")
        .expect("nested fixture source");

    assert_eq!(
        find_entry_file(&case).expect("discover nested entry"),
        PathBuf::from("src/pages/example.astro")
    );

    let _ = std::fs::remove_dir_all(case);
}

#[test]
fn entry_discovery_rejects_ambiguous_nested_project_sources() {
    let case = unique_temp_dir("velvet-glove-ambiguous-nested-entry");
    for relative in ["src/pages/example.astro", "src/components/example.astro"] {
        let path = case.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).expect("nested fixture directory");
        std::fs::write(path, "<p>fixture</p>\n").expect("nested fixture source");
    }

    let error = find_entry_file(&case).expect_err("ambiguous nested entries must fail");
    assert!(error.contains("multiple nested `example.*` entry files"));
    assert!(error.contains("src/pages"));
    assert!(error.contains("src/components"));

    let _ = std::fs::remove_dir_all(case);
}

#[test]
fn requested_failure_artifacts_copy_actionable_evidence() {
    let source = unique_temp_dir("velvet-glove-artifact-source");
    let artifact_root = unique_temp_dir("velvet-glove-artifact-root");
    std::fs::create_dir_all(source.join("workspace/.velvet-glove")).expect("fixture workspace");
    std::fs::create_dir_all(source.join("evidence")).expect("fixture evidence");
    std::fs::write(
        source.join("workspace/.velvet-glove/post-tool-use.pkl"),
        "config",
    )
    .expect("fixture config");
    std::fs::write(source.join("evidence/input.json"), "{}").expect("fixture input evidence");
    std::fs::write(source.join("evidence/stderr"), "failure").expect("fixture stderr evidence");
    let case = FixtureCase {
        tool: "probe-tool".to_owned(),
        case: "failure-case".to_owned(),
        directory: PathBuf::new(),
        entry: PathBuf::new(),
        pkl_property: String::new(),
        spec: ToolSpec::default(),
    };

    let retained = retain_failure(&source, &artifact_root, &case, ProtocolSurface::Claude)
        .expect("retain requested artifacts");
    assert_eq!(
        std::fs::read_to_string(retained.join("evidence/input.json")).unwrap(),
        "{}"
    );
    assert_eq!(
        std::fs::read_to_string(retained.join("evidence/stderr")).unwrap(),
        "failure"
    );
    assert!(
        retained
            .join("workspace/.velvet-glove/post-tool-use.pkl")
            .is_file()
    );

    let _ = std::fs::remove_dir_all(source);
    let _ = std::fs::remove_dir_all(artifact_root);
}

#[test]
#[cfg(unix)]
fn setup_failures_are_retained_when_requested() {
    let fixture_root = unique_temp_dir("velvet-glove-setup-failure-fixture");
    let artifact_root = unique_temp_dir("velvet-glove-setup-failure-artifacts");
    std::os::unix::fs::symlink("missing-target", fixture_root.join("example.txt"))
        .expect("fixture symlink");
    let case = FixtureCase {
        tool: "fixture-tool".to_owned(),
        case: "setup-failure".to_owned(),
        directory: fixture_root.clone(),
        entry: PathBuf::from("example.txt"),
        pkl_property: "fixtureTool".to_owned(),
        spec: ToolSpec::default(),
    };
    let options = HarnessOptions {
        timeout: Duration::from_secs(1),
        artifact_dir: Some(artifact_root.clone()),
        required_tools: RequiredTools::default(),
        selection: FixtureSelection::default(),
    };

    let outcome = run_fixture_case(&case, ProtocolSurface::Claude, &options);
    assert!(matches!(outcome.status, FixtureStatus::Fail(_)));
    let retained = outcome.artifacts.expect("retained setup failure artifacts");
    assert!(retained.join("evidence/outcome.json").is_file());
    assert!(!retained.join("workspace/example.txt").exists());

    let _ = std::fs::remove_dir_all(fixture_root);
    let _ = std::fs::remove_dir_all(artifact_root);
}

#[test]
fn temporary_directories_are_unique_across_parallel_callers() {
    let paths = (0..16)
        .map(|_| {
            std::thread::spawn(|| {
                (0..16)
                    .map(|_| unique_temp_dir("velvet-glove-parallel-temp-test"))
                    .collect::<Vec<_>>()
            })
        })
        .flat_map(|thread| thread.join().expect("temporary directory worker"))
        .collect::<Vec<_>>();
    let unique = paths.iter().collect::<BTreeSet<_>>();

    assert_eq!(unique.len(), paths.len());
    assert!(paths.iter().all(|path| path.is_dir()));
    for path in paths {
        let _ = std::fs::remove_dir(path);
    }
}

#[test]
fn required_tools_reject_unknown_fixture_ids() {
    let required = RequiredTools {
        all: false,
        names: BTreeSet::from(["known-tool".to_owned(), "typo-tool".to_owned()]),
    };
    let available = BTreeSet::from(["known-tool".to_owned()]);

    let error = required
        .validate(&available)
        .expect_err("unknown required tools must fail closed");
    assert!(error.contains("typo-tool"));
    assert!(error.contains(REQUIRED_TOOLS_ENV));
}

#[test]
fn fixture_selection_filters_exact_cases_and_recounts_tools() {
    let catalog = FixtureCatalog {
        tool_count: 2,
        cases: vec![
            named_fixture_case("tool-a", "clean"),
            named_fixture_case("tool-a", "issues"),
            named_fixture_case("tool-b", "clean"),
        ],
    };
    let selection = FixtureSelection {
        tools: BTreeSet::new(),
        cases: BTreeSet::from([
            ("tool-a".to_owned(), "issues".to_owned()),
            ("tool-b".to_owned(), "clean".to_owned()),
        ]),
    };

    let selected = selection.apply(catalog).expect("valid selection");
    assert_eq!(selected.tool_count, 2);
    assert_eq!(selected.cases.len(), 2);
    assert_eq!(selected.cases[0].tool, "tool-a");
    assert_eq!(selected.cases[0].case, "issues");
    assert_eq!(selected.cases[1].tool, "tool-b");
    assert_eq!(selected.cases[1].case, "clean");
}

#[test]
fn fixture_selection_rejects_unknown_tools_and_cases() {
    let catalog = || FixtureCatalog {
        tool_count: 1,
        cases: vec![named_fixture_case("tool-a", "clean")],
    };
    let unknown_tool = FixtureSelection {
        tools: BTreeSet::from(["tool-b".to_owned()]),
        cases: BTreeSet::new(),
    }
    .apply(catalog())
    .expect_err("unknown tool must fail");
    assert!(unknown_tool.contains("tool-b"));

    let unknown_case = FixtureSelection {
        tools: BTreeSet::new(),
        cases: BTreeSet::from([("tool-a".to_owned(), "issues".to_owned())]),
    }
    .apply(catalog())
    .expect_err("unknown case must fail");
    assert!(unknown_case.contains("tool-a/issues"));
}

#[test]
fn real_tool_contract_registry_preserves_direct_and_adapter_shapes() {
    let jq_multi = real_tool_contract_case(&named_fixture_case("jq", "multi-file-fragments"))
        .expect("jq contract lookup")
        .expect("jq contract");
    assert_eq!(jq_multi.invocations.len(), 2);
    assert!(
        jq_multi
            .invocations
            .iter()
            .all(|invocation| invocation.targets.len() == 1)
    );
    assert_eq!(jq_multi.targets().len(), 2);
    assert_eq!(jq_multi.trace_plan, TracePlan::Direct);

    let asciidoctor_multi =
        real_tool_contract_case(&named_fixture_case("asciidoctor", "multi-file"))
            .expect("Asciidoctor contract lookup")
            .expect("Asciidoctor contract");
    assert_eq!(asciidoctor_multi.invocations.len(), 1);
    assert_eq!(asciidoctor_multi.invocations[0].targets.len(), 2);
    assert_eq!(asciidoctor_multi.invocations[0].trace_exit_codes, &[0, 1]);
    assert_eq!(asciidoctor_multi.trace_plan, ASCIIDOCTOR_TRACE_PLAN);

    let asciidoctor_failure =
        real_tool_contract_case(&named_fixture_case("asciidoctor", "operational-failure"))
            .expect("Asciidoctor failure contract lookup")
            .expect("Asciidoctor failure contract");
    assert_eq!(
        asciidoctor_failure.extra_args,
        &["--backend=definitely-not-a-backend"]
    );
    assert_eq!(asciidoctor_failure.invocations[0].exit_code, 2);
    assert_eq!(asciidoctor_failure.invocations[0].trace_exit_codes, &[1]);
    assert_eq!(
        asciidoctor_failure.outcome,
        ExpectedOutcome::OperationalFailure
    );

    let astro_multi = real_tool_contract_case(&named_fixture_case("astro", "multi-file-project"))
        .expect("Astro contract lookup")
        .expect("Astro contract");
    assert_eq!(astro_multi.invocations.len(), 1);
    assert_eq!(astro_multi.invocations[0].targets.len(), 2);
    assert_eq!(
        astro_multi.invocations[0].targets,
        &[
            "src/components/selected-clean.astro",
            "src/pages/example.astro"
        ]
    );
    assert!(
        !astro_multi.invocations[0]
            .targets
            .contains(&"src/components/broken.astro")
    );
    assert_eq!(astro_multi.invocations[0].trace_exit_codes, &[1]);
    assert_eq!(astro_multi.trace_plan, ASTRO_TRACE_PLAN);

    let astro_failure =
        real_tool_contract_case(&named_fixture_case("astro", "operational-failure"))
            .expect("Astro failure contract lookup")
            .expect("Astro failure contract");
    assert_eq!(
        astro_failure.extra_args,
        &["--tsconfig", "does-not-exist.json"]
    );
    assert_eq!(astro_failure.invocations[0].exit_code, 2);
    assert_eq!(astro_failure.invocations[0].trace_exit_codes, &[1]);
    assert_eq!(astro_failure.outcome, ExpectedOutcome::OperationalFailure);

    let betterleaks_multi =
        real_tool_contract_case(&named_fixture_case("betterleaks", "multi-file"))
            .expect("Betterleaks contract lookup")
            .expect("Betterleaks contract");
    assert_eq!(betterleaks_multi.invocations.len(), 1);
    assert_eq!(betterleaks_multi.invocations[0].targets.len(), 2);
    assert_eq!(betterleaks_multi.invocations[0].trace_exit_codes, &[10]);
    assert_eq!(
        betterleaks_multi.extra_args,
        &["--config=.betterleaks.toml"]
    );
    assert_eq!(betterleaks_multi.trace_plan, BETTERLEAKS_TRACE_PLAN);

    let betterleaks_failure =
        real_tool_contract_case(&named_fixture_case("betterleaks", "operational-failure"))
            .expect("Betterleaks failure contract lookup")
            .expect("Betterleaks failure contract");
    assert_eq!(
        betterleaks_failure.extra_args,
        &["--config=does-not-exist.toml"]
    );
    assert_eq!(betterleaks_failure.invocations[0].exit_code, 1);
    assert_eq!(betterleaks_failure.invocations[0].trace_exit_codes, &[1]);
    assert_eq!(
        betterleaks_failure.outcome,
        ExpectedOutcome::OperationalFailure
    );

    let vacuum_multi = real_tool_contract_case(&named_fixture_case("vacuum", "multi-file"))
        .expect("Vacuum contract lookup")
        .expect("Vacuum contract");
    assert_eq!(vacuum_multi.invocations.len(), 1);
    assert_eq!(
        vacuum_multi.invocations[0].targets,
        &["example-openapi.yaml", "selected-clean-openapi.yaml"]
    );
    assert_eq!(vacuum_multi.invocations[0].exit_code, 1);
    assert_eq!(vacuum_multi.invocations[0].trace_exit_codes, &[1]);
    assert_eq!(vacuum_multi.trace_plan, VACUUM_TRACE_PLAN);

    let vacuum_failure =
        real_tool_contract_case(&named_fixture_case("vacuum", "operational-failure"))
            .expect("Vacuum failure contract lookup")
            .expect("Vacuum failure contract");
    assert_eq!(vacuum_failure.invocations[0].exit_code, 2);
    assert_eq!(vacuum_failure.invocations[0].trace_exit_codes, &[2]);
    assert_eq!(vacuum_failure.outcome, ExpectedOutcome::OperationalFailure);

    let ghalint_multi =
        real_tool_contract_case(&named_fixture_case("ghalint-workflow", "multi-workflow"))
            .expect("ghalint-workflow contract lookup")
            .expect("ghalint-workflow contract");
    assert_eq!(ghalint_multi.invocations.len(), 1);
    assert_eq!(
        ghalint_multi.invocations[0].targets,
        &[
            ".github/workflows/example.yml",
            ".github/workflows/selected.yaml",
        ]
    );
    assert_eq!(ghalint_multi.invocations[0].trace_exit_codes, &[0, 1]);
    assert_eq!(ghalint_multi.trace_plan, GHALINT_TRACE_PLAN);

    let ghalint_config =
        real_tool_contract_case(&named_fixture_case("ghalint-workflow", "config-failure"))
            .expect("ghalint-workflow config contract lookup")
            .expect("ghalint-workflow config contract");
    assert_eq!(ghalint_config.invocations[0].exit_code, 2);
    assert_eq!(ghalint_config.invocations[0].trace_exit_codes, &[0, 1]);
    assert_eq!(ghalint_config.outcome, ExpectedOutcome::OperationalFailure);
}

#[test]
fn mutating_contract_registry_preserves_biome_target_file_writes() {
    for case_name in [
        "clean",
        "autofix",
        "source-issue",
        "multi-file",
        "operational-failure",
    ] {
        let contract = mutating_tool_contract_case(&named_fixture_case("biome", case_name))
            .expect("Biome mutating contract lookup")
            .expect("Biome mutating contract");
        assert_eq!(contract.remedy_mode, PhaseMode::Fix);
        assert_eq!(contract.remedy_writes, WriteBehavior::TargetFiles);
    }
}

#[test]
fn explicit_workflow_trace_membership_keeps_gofmt_and_dclint_distinct() {
    let specs = builtin_index().expect("builtin specs");

    let (_, gofmt) = specs.get("go-fmt").expect("gofmt spec");
    let gofmt_check = gofmt
        .workflows
        .get("format")
        .and_then(|workflow| workflow.check.as_ref())
        .expect("gofmt workflow check");
    assert_eq!(gofmt.phase_order, ["format"]);
    assert!(
        gofmt
            .phase_order
            .iter()
            .filter_map(|phase_id| gofmt.phases.get(phase_id))
            .all(|phase| !phase_matches_workflow_check(phase, gofmt_check)),
        "gofmt's deferred checker must not be appended to its immediate format phase"
    );
    let gofmt_mutation = mutating_tool_contract_case(&named_fixture_case("go-fmt", "multi-file"))
        .expect("gofmt mutation lookup")
        .expect("gofmt mutation contract");
    assert_eq!(
        gofmt_mutation.remedy_invocations[0].trace_exit_codes.len(),
        2
    );
    assert_eq!(
        gofmt_mutation.final_invocations[0].trace_exit_codes.len(),
        1
    );
    assert_eq!(
        1 + usize::from(!gofmt_mutation.remedy_invocations.is_empty())
            + usize::from(!gofmt_mutation.final_invocations.is_empty()),
        3,
        "gofmt deferred execution remains check, remedy, final-check"
    );

    let (_, dclint) = specs.get("dclint").expect("dclint spec");
    let dclint_check = dclint
        .workflows
        .get("fix")
        .and_then(|workflow| workflow.check.as_ref())
        .expect("dclint workflow check");
    let matching_phases = dclint
        .phase_order
        .iter()
        .filter(|phase_id| {
            dclint
                .phases
                .get(*phase_id)
                .is_some_and(|phase| phase_matches_workflow_check(phase, dclint_check))
        })
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(matching_phases, ["verify"]);
    let dclint_clean = mutating_tool_contract_case(&named_fixture_case("dclint", "clean"))
        .expect("dclint clean mutation lookup")
        .expect("dclint clean mutation contract");
    assert_eq!(
        dclint_clean.remedy_invocations[0].trace_exit_codes.len()
            + dclint_clean.final_invocations[0].trace_exit_codes.len(),
        2,
        "dclint immediate execution remains fix plus authoritative verify"
    );
    let dclint_source = mutating_tool_contract_case(&named_fixture_case("dclint", "source-issue"))
        .expect("dclint source mutation lookup")
        .expect("dclint source mutation contract");
    assert_eq!(
        dclint_source.immediate_outcome,
        ExpectedOutcome::Issues,
        "persistent dclint findings remain issues after the final check"
    );
}

#[test]
fn prettier_contract_registry_binds_read_only_format_preflight() {
    for (case_name, expected) in [
        ("clean", &[0, 0][..]),
        ("unformatted", &[1, 0][..]),
        ("multi-file", &[1, 0][..]),
        ("config-failure", &[1][..]),
    ] {
        let contract = mutating_tool_contract_case(&named_fixture_case("prettier", case_name))
            .expect("Prettier mutating contract lookup")
            .expect("Prettier mutating contract");
        assert_eq!(contract.remedy_mode, PhaseMode::Format);
        assert_eq!(contract.remedy_writes, WriteBehavior::TargetFiles);
        assert_eq!(
            contract.remedy_invocations[0].trace_exit_codes, expected,
            "{case_name} preflight/write trace sequence"
        );
    }
}

#[test]
fn buf_contract_registry_binds_workspace_format_lifecycle() {
    let multi = named_fixture_case("buf-format", "multi-file");
    let check = real_tool_contract_case(&multi)
        .expect("Buf check contract lookup")
        .expect("Buf check contract");
    let mutation = mutating_tool_contract_case(&multi)
        .expect("Buf mutation contract lookup")
        .expect("Buf mutation contract");

    assert_eq!(check.phase_id, "verify");
    assert_eq!(check.invocations.len(), 1);
    assert_eq!(
        check.invocations[0].targets,
        &["example.proto", "proto/selected-clean.proto"]
    );
    assert_eq!(check.invocations[0].exit_code, 100);
    assert_eq!(check.invocations[0].trace_exit_codes, &[0, 100]);
    assert_eq!(check.trace_plan, BUF_TRACE_PLAN);
    assert_eq!(mutation.remedy_phase_id, "format");
    assert_eq!(mutation.remedy_mode, PhaseMode::Format);
    assert_eq!(mutation.remedy_writes, WriteBehavior::Workspace);
    assert_eq!(
        mutation.changed_targets,
        &["example.proto", "proto/workspace-only.proto"]
    );
    assert!(
        !check.targets().contains(&"proto/workspace-only.proto"),
        "workspace-only mutation must remain outside the event candidates"
    );

    let failure = named_fixture_case("buf-format", "operational-failure");
    let failure_check = real_tool_contract_case(&failure)
        .expect("Buf failure check lookup")
        .expect("Buf failure check contract");
    let failure_mutation = mutating_tool_contract_case(&failure)
        .expect("Buf failure mutation lookup")
        .expect("Buf failure mutation contract");
    assert_eq!(failure_check.invocations[0].exit_code, 2);
    assert_eq!(failure_check.invocations[0].trace_exit_codes, &[0]);
    assert_eq!(failure_mutation.remedy_invocations[0].exit_code, 2);
    assert_eq!(
        failure_mutation.remedy_invocations[0].trace_exit_codes,
        &[0]
    );
    assert!(failure_mutation.final_invocations.is_empty());
}

#[test]
fn cargo_clippy_contract_registry_binds_workspace_fix_lifecycle() {
    let multi = named_fixture_case("cargo-clippy", "workspace-autofix");
    let check = real_tool_contract_case(&multi)
        .expect("cargo-clippy check contract lookup")
        .expect("cargo-clippy check contract");
    let mutation = mutating_tool_contract_case(&multi)
        .expect("cargo-clippy mutation contract lookup")
        .expect("cargo-clippy mutation contract");

    assert_eq!(check.phase_id, "verify");
    assert_eq!(check.invocations.len(), 1);
    assert_eq!(
        check.invocations[0].targets,
        &["src/example.rs", "src/selected_clean.rs"]
    );
    assert_eq!(check.invocations[0].exit_code, 1);
    assert_eq!(check.invocations[0].trace_exit_codes, &[0, 0, 101]);
    assert_eq!(check.trace_plan, CARGO_CLIPPY_TRACE_PLAN);
    assert_eq!(mutation.remedy_phase_id, "fix");
    assert_eq!(mutation.remedy_mode, PhaseMode::Fix);
    assert_eq!(mutation.remedy_writes, WriteBehavior::Workspace);
    let repeat_remedy = mutation
        .repeat_remedy_invocations
        .expect("workspace autofix binds a fixed-state repeat remedy");
    assert_eq!(repeat_remedy.len(), 1);
    assert_eq!(repeat_remedy[0].trace_exit_codes, &[0, 0, 0]);
    assert_eq!(
        mutation.changed_targets,
        &["src/example.rs", "src/workspace_only.rs"]
    );
    assert!(
        !check.targets().contains(&"src/workspace_only.rs"),
        "workspace-only Clippy mutation must remain outside the event candidates"
    );

    let operational = named_fixture_case("cargo-clippy", "operational-failure");
    let operational_check = real_tool_contract_case(&operational)
        .expect("cargo-clippy operational check lookup")
        .expect("cargo-clippy operational check contract");
    assert_eq!(operational_check.invocations[0].exit_code, 2);
    assert_eq!(operational_check.invocations[0].trace_exit_codes, &[0, 101]);
}

#[test]
fn cargo_fmt_contract_registry_binds_workspace_format_and_coverage_failure() {
    let multi = named_fixture_case("cargo-fmt", "workspace-multi");
    let check = real_tool_contract_case(&multi)
        .expect("cargo-fmt check contract lookup")
        .expect("cargo-fmt check contract");
    let mutation = mutating_tool_contract_case(&multi)
        .expect("cargo-fmt mutation contract lookup")
        .expect("cargo-fmt mutation contract");

    assert_eq!(check.phase_id, "verify");
    assert_eq!(check.invocations.len(), 1);
    assert_eq!(check.invocations[0].exit_code, 1);
    assert_eq!(
        check.invocations[0].trace_exit_codes,
        &[0, 0, 1, 0, 1, 1, 0, 1]
    );
    assert_eq!(check.trace_plan, CARGO_FMT_MULTI_TRACE_PLAN);
    assert_eq!(mutation.remedy_phase_id, "format");
    assert_eq!(mutation.remedy_mode, PhaseMode::Format);
    assert_eq!(mutation.remedy_writes, WriteBehavior::Workspace);
    assert_eq!(
        mutation.changed_targets,
        &["alpha/src/example.rs", "beta/src/workspace_only.rs"]
    );
    assert!(
        !check.targets().contains(&"beta/src/workspace_only.rs"),
        "workspace-only formatting mutation must remain outside event candidates"
    );

    let coverage = named_fixture_case("cargo-fmt", "coverage-failure");
    let coverage_check = real_tool_contract_case(&coverage)
        .expect("cargo-fmt coverage check lookup")
        .expect("cargo-fmt coverage check contract");
    let coverage_mutation = mutating_tool_contract_case(&coverage)
        .expect("cargo-fmt coverage mutation lookup")
        .expect("cargo-fmt coverage mutation contract");
    assert_eq!(coverage_check.outcome, ExpectedOutcome::OperationalFailure);
    assert_eq!(coverage_check.invocations[0].exit_code, 2);
    assert_eq!(
        coverage_check.invocations[0].trace_exit_codes,
        &[0, 0, 1, 0, 1]
    );
    assert_eq!(
        coverage_check.trace_plan,
        CARGO_FMT_COVERAGE_FAILURE_TRACE_PLAN
    );
    assert!(coverage_mutation.final_invocations.is_empty());
}

#[test]
fn changed_path_attribution_distinguishes_target_and_workspace_scopes() {
    let selected = PathBuf::from("/workspace/src/example.proto");
    let workspace_only = PathBuf::from("/workspace/src/workspace-only.proto");
    let changed = BTreeSet::from([selected.clone(), workspace_only.clone()]);

    assert_eq!(
        expected_invocation_changed_paths(
            WriteBehavior::TargetFiles,
            &changed,
            std::slice::from_ref(&selected),
        ),
        vec![selected]
    );
    for writes in [WriteBehavior::MatchingGlobs, WriteBehavior::Workspace] {
        assert_eq!(
            expected_invocation_changed_paths(writes, &changed, &[]),
            vec![
                PathBuf::from("/workspace/src/example.proto"),
                workspace_only.clone(),
            ]
        );
    }
    assert!(expected_invocation_changed_paths(WriteBehavior::None, &changed, &[]).is_empty());
}

#[test]
fn expected_arguments_render_the_nearest_workspace_job() {
    let root = unique_temp_dir("velvet-glove-workspace-argument-test");
    let project = root.join("project");
    let workspace = project.join("member");
    let source = workspace.join("src");
    std::fs::create_dir_all(&source).expect("workspace source directory");
    std::fs::write(project.join("Cargo.toml"), "[workspace]\n").expect("outer workspace marker");
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"member\"\n",
    )
    .expect("nearest workspace marker");
    let first = source.join("example.rs");
    let second = source.join("selected.rs");
    std::fs::write(&first, "fn example() {}\n").expect("first selected source");
    std::fs::write(&second, "fn selected() {}\n").expect("second selected source");

    let spec = ToolSpec {
        id: "workspace-tool".to_owned(),
        executable: "workspace-tool".to_owned(),
        workspace_indicator: Some("Cargo.toml".to_owned()),
        ..ToolSpec::default()
    };
    let phase = Phase {
        argv: vec![
            ArgvElement::Token(ArgToken::WorkspaceIndicator),
            ArgvElement::Token(ArgToken::Workspace),
            ArgvElement::Token(ArgToken::WorkspaceFiles),
            ArgvElement::Token(ArgToken::ProjectRoot),
        ],
        ..Phase::default()
    };
    let project = canonical_project(&project);
    let workspace = canonical_project(&workspace);
    let targets = [canonical_project(&first), canonical_project(&second)];

    let arguments = render_expected_arguments(&spec, &phase, &project, &targets)
        .expect("render nearest workspace arguments");

    assert_eq!(
        arguments,
        vec![
            workspace.join("Cargo.toml").to_string_lossy().into_owned(),
            workspace.to_string_lossy().into_owned(),
            "src/example.rs".to_owned(),
            "src/selected.rs".to_owned(),
            project.to_string_lossy().into_owned(),
        ]
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn expected_workspace_job_rejects_split_or_missing_markers() {
    let root = unique_temp_dir("velvet-glove-workspace-partition-test");
    let project = root.join("project");
    let first = project.join("first/src/example.rs");
    let second = project.join("second/src/example.rs");
    let missing = project.join("missing/src/example.rs");
    for target in [&first, &second, &missing] {
        std::fs::create_dir_all(target.parent().unwrap()).expect("workspace source directory");
        std::fs::write(target, "fn example() {}\n").expect("selected source");
    }
    for member in ["first", "second"] {
        std::fs::write(
            project.join(member).join("Cargo.toml"),
            format!("[package]\nname = \"{member}\"\n"),
        )
        .expect("workspace marker");
    }
    let spec = ToolSpec {
        id: "workspace-tool".to_owned(),
        workspace_indicator: Some("Cargo.toml".to_owned()),
        ..ToolSpec::default()
    };
    let project = canonical_project(&project);

    let split = resolve_expected_workspace_job(
        &spec,
        &project,
        &[canonical_project(&first), canonical_project(&second)],
    )
    .expect_err("one invocation must not span workspace partitions");
    assert!(split.contains("spans multiple"));

    let absent = resolve_expected_workspace_job(&spec, &project, &[canonical_project(&missing)])
        .expect_err("workspace marker is required");
    assert!(absent.contains("found no"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ghalint_trace_plan_binds_version_private_config_and_top_level_inventory() {
    let root = unique_temp_dir("velvet-glove-ghalint-trace-plan");
    let project = root.join("project");
    let workflows = project.join(".github/workflows");
    std::fs::create_dir_all(&workflows).expect("workflow directory");
    let first = workflows.join("example.yml");
    let second = workflows.join("selected.yaml");
    std::fs::write(&first, "name: first\n").expect("first workflow");
    std::fs::write(&second, "name: second\n").expect("second workflow");
    let project = canonical_project(&project);
    let targets = [canonical_project(&first), canonical_project(&second)];
    let outer_arguments = vec![
        "-I".to_owned(),
        "-c".to_owned(),
        "adapter".to_owned(),
        "ghalint".to_owned(),
        project.to_string_lossy().into_owned(),
        GHALINT_WORKFLOW_FILES_MARKER.to_owned(),
        targets[0].to_string_lossy().into_owned(),
        targets[1].to_string_lossy().into_owned(),
    ];

    let (program, invocations) = resolve_trace_invocations(
        GHALINT_TRACE_PLAN,
        "python",
        &outer_arguments,
        &targets,
        &[0, 1],
    )
    .expect("resolve ghalint-workflow trace");

    assert_eq!(program, "ghalint");
    assert_eq!(invocations.len(), 2);
    assert_eq!(invocations[0].arguments, ["--version"]);
    assert_eq!(invocations[0].exit_code, 0);
    assert_eq!(
        invocations[1].arguments,
        [
            "--log-color=never",
            "--log-level=error",
            "--config=<ghalint-private>/ghalint.yaml",
            "run",
        ]
    );
    assert_eq!(invocations[1].exit_code, 1);
    assert_eq!(invocations[1].targets, targets);

    let nested = workflows.join("nested/ignored.yml");
    std::fs::create_dir_all(nested.parent().expect("nested parent"))
        .expect("nested workflow directory");
    std::fs::write(&nested, "name: ignored\n").expect("nested workflow");
    let nested_target = canonical_project(&nested);
    let mut nested_arguments = outer_arguments;
    *nested_arguments.last_mut().expect("second rendered target") =
        nested_target.to_string_lossy().into_owned();
    let error = resolve_trace_invocations(
        GHALINT_TRACE_PLAN,
        "python",
        &nested_arguments,
        &[targets[0].clone(), nested_target],
        &[0, 1],
    )
    .expect_err("nested workflow target must be rejected");
    assert!(error.contains("outside its top-level workflow inventory"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn single_child_trace_plan_appends_controlled_trailing_options() {
    let outer_arguments = [
        "--eval",
        "adapter",
        "--",
        "astro",
        "check",
        "--root",
        "/workspace",
    ]
    .map(str::to_owned);
    let targets = [PathBuf::from("/workspace/src/pages/example.astro")];

    let (program, invocations) = resolve_trace_invocations(
        TracePlan::SingleNestedTrailingOptions {
            trailing: &["--noSync", "--minimumSeverity=error"],
        },
        "node",
        &outer_arguments,
        &targets,
        &[1],
    )
    .expect("resolve single-child trace");

    assert_eq!(program, "astro");
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].targets, targets);
    assert_eq!(
        invocations[0].arguments,
        [
            "check",
            "--root",
            "/workspace",
            "--noSync",
            "--minimumSeverity=error",
        ]
    );
    assert_eq!(invocations[0].exit_code, 1);
}

#[test]
fn single_child_trace_plan_rejects_ambiguous_shapes() {
    let no_separator = ["--eval", "adapter", "astro"].map(str::to_owned);
    let error = resolve_trace_invocations(
        TracePlan::SingleNestedTrailingOptions { trailing: &[] },
        "node",
        &no_separator,
        &[],
        &[0],
    )
    .expect_err("nested trace without separator must fail");
    assert!(error.contains("no `--` separator"));

    let too_many_statuses = ["--eval", "adapter", "--", "astro"].map(str::to_owned);
    let error = resolve_trace_invocations(
        TracePlan::SingleNestedTrailingOptions { trailing: &[] },
        "node",
        &too_many_statuses,
        &[],
        &[0, 1],
    )
    .expect_err("single-child trace with two statuses must fail");
    assert!(error.contains("exactly one exit code"));
}

#[test]
fn marker_delimited_trace_plan_inserts_controls_before_files() {
    let outer_arguments = [
        "-I",
        "-c",
        "adapter",
        "betterleaks",
        "--config=.betterleaks.toml",
        BETTERLEAKS_FILES_MARKER,
        "/workspace/src/example.secret.txt",
        "/workspace/src/selected-clean.txt",
    ]
    .map(str::to_owned);
    let targets = [
        PathBuf::from("/workspace/src/example.secret.txt"),
        PathBuf::from("/workspace/src/selected-clean.txt"),
    ];

    let (program, invocations) = resolve_trace_invocations(
        BETTERLEAKS_TRACE_PLAN,
        "python",
        &outer_arguments,
        &targets,
        &[10],
    )
    .expect("resolve marker-delimited trace");

    assert_eq!(program, "betterleaks");
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].targets, targets);
    assert_eq!(
        invocations[0].arguments,
        [
            "dir",
            "--config=.betterleaks.toml",
            "--redact=100",
            "--verbose=true",
            "--no-color=true",
            "--no-banner=true",
            "--exit-code=10",
            "--log-level=fatal",
            "--legacy-print=true",
            "/workspace/src/example.secret.txt",
            "/workspace/src/selected-clean.txt",
        ]
    );
    assert_eq!(invocations[0].exit_code, 10);
}

#[test]
fn marker_delimited_trace_plan_rejects_ambiguous_shapes() {
    let targets = [PathBuf::from("/workspace/src/example.txt")];
    let missing_marker = [
        "-I",
        "-c",
        "adapter",
        "betterleaks",
        "/workspace/src/example.txt",
    ]
    .map(str::to_owned);
    let error = resolve_trace_invocations(
        BETTERLEAKS_TRACE_PLAN,
        "python",
        &missing_marker,
        &targets,
        &[0],
    )
    .expect_err("marker-delimited trace without a marker must fail");
    assert!(error.contains("exactly one"));

    let wrong_files = [
        "-I",
        "-c",
        "adapter",
        "betterleaks",
        BETTERLEAKS_FILES_MARKER,
        "/workspace/src/not-selected.txt",
    ]
    .map(str::to_owned);
    let error = resolve_trace_invocations(
        BETTERLEAKS_TRACE_PLAN,
        "python",
        &wrong_files,
        &targets,
        &[0],
    )
    .expect_err("marker-delimited trace with a different file suffix must fail");
    assert!(error.contains("file suffix mismatch"));

    let unisolated = [
        "-c",
        "adapter",
        "betterleaks",
        BETTERLEAKS_FILES_MARKER,
        "/workspace/src/example.txt",
    ]
    .map(str::to_owned);
    let error = resolve_trace_invocations(
        BETTERLEAKS_TRACE_PLAN,
        "python",
        &unisolated,
        &targets,
        &[0],
    )
    .expect_err("marker-delimited Python adapter without isolation must fail");
    assert!(error.contains("adapter prefix"));
}

#[test]
fn buf_workspace_trace_plan_binds_preflight_format_argv_and_workspace() {
    let outer_arguments = [
        "-I",
        "-c",
        "adapter",
        "buf",
        "write",
        BUF_WORKSPACE_MARKER,
        "/workspace",
    ]
    .map(str::to_owned);
    let targets = [
        PathBuf::from("/workspace/example.proto"),
        PathBuf::from("/workspace/proto/selected.proto"),
    ];

    let (program, invocations) = resolve_trace_invocations(
        BUF_TRACE_PLAN,
        "python",
        &outer_arguments,
        &targets,
        &[0, 0],
    )
    .expect("resolve mode-and-workspace trace");

    assert_eq!(program, "buf");
    assert_eq!(invocations.len(), 2);
    assert_eq!(invocations[0].targets, targets);
    assert_eq!(
        invocations[0].arguments,
        ["config", "ls-modules", "--log-format=text", "--format=json"]
    );
    assert_eq!(invocations[0].exit_code, 0);
    assert_eq!(invocations[1].targets, targets);
    assert_eq!(
        invocations[1].arguments,
        [
            "format",
            "--disable-symlinks",
            "--error-format=text",
            "--log-format=text",
            "--write",
            "/workspace",
        ]
    );
    assert_eq!(invocations[1].exit_code, 0);
}

#[test]
fn buf_workspace_trace_plan_can_stop_after_preflight() {
    let outer_arguments = [
        "-I",
        "-c",
        "adapter",
        "buf",
        "verify",
        BUF_WORKSPACE_MARKER,
        "/workspace",
    ]
    .map(str::to_owned);
    let targets = [PathBuf::from("/workspace/example.proto")];

    let (_, invocations) =
        resolve_trace_invocations(BUF_TRACE_PLAN, "python", &outer_arguments, &targets, &[0])
            .expect("resolve preflight-only workspace trace");

    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].targets, targets);
    assert_eq!(
        invocations[0].arguments,
        ["config", "ls-modules", "--log-format=text", "--format=json"]
    );
    assert_eq!(invocations[0].exit_code, 0);
}

#[test]
fn mode_and_workspace_trace_plan_rejects_ambiguous_or_escaping_workspaces() {
    let plan = BUF_TRACE_PLAN;
    let target = [PathBuf::from("/workspace/example.proto")];

    let multiple_workspaces = [
        "-I",
        "-c",
        "adapter",
        "buf",
        "verify",
        BUF_WORKSPACE_MARKER,
        "/workspace",
        "/other",
    ]
    .map(str::to_owned);
    let error = resolve_trace_invocations(plan, "python", &multiple_workspaces, &target, &[0])
        .expect_err("multiple rendered workspaces must fail");
    assert!(error.contains("exactly one workspace"));

    let relative_workspace = [
        "-I",
        "-c",
        "adapter",
        "buf",
        "verify",
        BUF_WORKSPACE_MARKER,
        "workspace",
    ]
    .map(str::to_owned);
    let error = resolve_trace_invocations(plan, "python", &relative_workspace, &target, &[0])
        .expect_err("relative rendered workspace must fail");
    assert!(error.contains("non-absolute workspace"));

    let escaped_target = [PathBuf::from("/other/example.proto")];
    let workspace = [
        "-I",
        "-c",
        "adapter",
        "buf",
        "verify",
        BUF_WORKSPACE_MARKER,
        "/workspace",
    ]
    .map(str::to_owned);
    let error = resolve_trace_invocations(plan, "python", &workspace, &escaped_target, &[0])
        .expect_err("target outside rendered workspace must fail");
    assert!(error.contains("targets escape workspace"));
}

#[test]
fn cargo_fmt_trace_plan_binds_coverage_and_real_workspace_children() {
    let root = unique_temp_dir("velvet-glove-cargo-fmt-trace-test");
    let indicator = root.join("Cargo.lock");
    let manifest = root.join("Cargo.toml");
    let config = root.join("rustfmt.toml");
    let target = root.join("src/example.rs");
    std::fs::create_dir_all(target.parent().unwrap()).expect("workspace source directory");
    std::fs::write(&indicator, "# lock\n").expect("workspace indicator");
    std::fs::write(&manifest, "[package]\nname = \"fixture\"\n").expect("workspace manifest");
    std::fs::write(&config, "style_edition = \"2024\"\n").expect("rustfmt config");
    std::fs::write(&target, "fn example() {}\n").expect("selected source");
    let indicator = canonical_project(&indicator).to_string_lossy().into_owned();
    let manifest = canonical_project(&manifest).to_string_lossy().into_owned();
    let target = canonical_project(&target);
    let outer_arguments = [
        "-I".to_owned(),
        "-c".to_owned(),
        "adapter".to_owned(),
        "cargo".to_owned(),
        "cargo-fmt".to_owned(),
        "rustfmt".to_owned(),
        "verify".to_owned(),
        CARGO_FMT_WORKSPACE_MARKER.to_owned(),
        indicator.clone(),
    ];

    let (program, invocations) = resolve_trace_invocations(
        CARGO_FMT_TRACE_PLAN,
        "python",
        &outer_arguments,
        std::slice::from_ref(&target),
        &[0, 0, 1, 0, 1, 1, 0, 1],
    )
    .expect("resolve Cargo Fmt trace");

    assert_eq!(program, "cargo");
    assert_eq!(invocations.len(), 8);
    assert_eq!(
        invocations
            .iter()
            .map(|invocation| invocation.program.as_str())
            .collect::<Vec<_>>(),
        [
            "cargo",
            "cargo",
            "cargo-fmt",
            "cargo",
            "rustfmt",
            "cargo-fmt",
            "cargo",
            "rustfmt",
        ]
    );
    assert_eq!(
        invocations[0].arguments,
        [
            "metadata",
            "--format-version=1",
            "--no-deps",
            "--manifest-path",
            &manifest,
            "--locked",
            "--offline",
            "--quiet",
        ]
    );
    assert_eq!(
        invocations[3].arguments,
        [
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            "<cargo-fmt-private>/coverage-workspace/Cargo.toml",
            "--offline",
        ]
    );
    assert_eq!(
        invocations[4].arguments,
        [
            "<cargo-fmt-private>/coverage-workspace/src/example.rs",
            "--edition",
            "2024",
            "--config-path",
            "<cargo-fmt-private>/coverage-workspace",
            "--color",
            "never",
            "--files-with-diff",
            "--check",
        ]
    );
    assert_eq!(
        invocations[7].arguments,
        [
            target.to_string_lossy().as_ref(),
            "--edition",
            "2024",
            "--config-path",
            canonical_project(&root).to_string_lossy().as_ref(),
            "--color",
            "never",
            "--files-with-diff",
            "--check",
        ]
    );
    assert_eq!(
        invocations
            .iter()
            .map(|invocation| invocation.exit_code)
            .collect::<Vec<_>>(),
        [0, 0, 1, 0, 1, 1, 0, 1]
    );

    let (_, coverage_only) = resolve_trace_invocations(
        CARGO_FMT_TRACE_PLAN,
        "python",
        &outer_arguments,
        &[target],
        &[0, 0, 1, 0, 1],
    )
    .expect("resolve coverage rejection trace");
    assert_eq!(coverage_only.len(), 5);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn dclint_trace_plan_binds_fixable_subset_and_full_authoritative_recheck() {
    let outer_arguments = [
        "-I",
        "-c",
        "adapter",
        "dclint",
        "fix",
        "/workspace",
        DCLINT_FILES_MARKER,
        "/workspace/compose.selected-clean.yaml",
        "/workspace/docker-compose.yml",
    ]
    .map(str::to_owned);
    let targets = [
        PathBuf::from("/workspace/compose.selected-clean.yaml"),
        PathBuf::from("/workspace/docker-compose.yml"),
    ];

    let (program, invocations) = resolve_trace_invocations(
        DCLINT_AUTOFIX_TRACE_PLAN,
        "python",
        &outer_arguments,
        &targets,
        &[1, 0, 0],
    )
    .expect("resolve dclint fix trace");

    assert_eq!(program, "dclint");
    assert_eq!(invocations.len(), 3);
    assert_eq!(invocations[0].targets, targets);
    assert_eq!(
        invocations[1].targets,
        [PathBuf::from("/workspace/docker-compose.yml")]
    );
    assert_eq!(invocations[2].targets, targets);
    assert_eq!(
        invocations[1].arguments,
        [
            "--formatter=json",
            "--color=false",
            "--max-warnings=0",
            DCLINT_PRIVATE_CONFIG_ARGUMENT,
            "--fix",
            "/workspace/docker-compose.yml",
        ]
    );
    assert!(
        !invocations[0]
            .arguments
            .iter()
            .any(|argument| argument == "--fix")
    );
    assert!(
        !invocations[2]
            .arguments
            .iter()
            .any(|argument| argument == "--fix")
    );

    let (_, fixed) = resolve_trace_invocations(
        DCLINT_AUTOFIX_TRACE_PLAN,
        "python",
        &outer_arguments,
        &targets,
        &[0],
    )
    .expect("resolve fixed-state dclint trace");
    assert_eq!(fixed.len(), 1);
}

#[test]
fn dclint_trace_plan_allows_fail_closed_rejection_before_native_spawn() {
    let outer_arguments = [
        "-I",
        "-c",
        "adapter",
        "dclint",
        "verify",
        "/workspace",
        "--config=config/unsafe.json",
        DCLINT_FILES_MARKER,
        "/workspace/docker-compose.yml",
    ]
    .map(str::to_owned);
    let (program, invocations) = resolve_trace_invocations(
        DCLINT_TRACE_PLAN,
        "python",
        &outer_arguments,
        &[PathBuf::from("/workspace/docker-compose.yml")],
        &[],
    )
    .expect("resolve dclint pre-spawn rejection trace");
    assert_eq!(program, "dclint");
    assert!(invocations.is_empty());
}

#[test]
fn workspace_indicator_trace_plan_binds_preflight_and_read_only_remedy_probes() {
    let root = unique_temp_dir("velvet-glove-workspace-indicator-trace-test");
    let indicator = root.join("Cargo.toml");
    let target = root.join("src/example.rs");
    std::fs::create_dir_all(target.parent().unwrap()).expect("workspace source directory");
    std::fs::write(&indicator, "[package]\nname = \"fixture\"\n").expect("workspace indicator");
    std::fs::write(&target, "fn example() {}\n").expect("selected source");
    let indicator = canonical_project(&indicator).to_string_lossy().into_owned();
    let outer_arguments = [
        "-I".to_owned(),
        "-c".to_owned(),
        "adapter".to_owned(),
        "cargo".to_owned(),
        "cargo-clippy".to_owned(),
        "fix".to_owned(),
        "__WORKSPACE_INDICATOR__".to_owned(),
        indicator.clone(),
    ];
    let targets = [canonical_project(&target)];
    let plan = TracePlan::PreflightThenNestedModeWorkspaceIndicatorMarker {
        preflight_program_index: 3,
        command_program_index: 4,
        adapter_prefix: &["-I", "-c"],
        marker: "__WORKSPACE_INDICATOR__",
        modes: &["fix", "verify"],
        preflight_before_indicator: &["metadata", "--metadata-control", "--manifest-path"],
        preflight_after_indicator: &["--metadata-after-indicator"],
        command_before_indicator: &["clippy", "--manifest-path"],
        command_after_indicators: &[
            &[
                "--checker-control",
                "--message-format=json",
                "--",
                "--cap-lints=allow",
            ],
            &[
                "--checker-control",
                "--message-format=json",
                "--",
                "-D",
                "warnings",
            ],
        ],
    };

    let (program, invocations) =
        resolve_trace_invocations(plan, "python", &outer_arguments, &targets, &[0, 0, 101])
            .expect("resolve workspace-indicator trace");

    assert_eq!(program, "cargo");
    assert_eq!(invocations.len(), 3);
    assert_eq!(invocations[0].program, "cargo");
    assert_eq!(
        invocations[0].arguments,
        [
            "metadata",
            "--metadata-control",
            "--manifest-path",
            &indicator,
            "--metadata-after-indicator",
        ]
    );
    assert_eq!(invocations[0].exit_code, 0);
    assert_eq!(invocations[1].program, "cargo-clippy");
    assert_eq!(
        invocations[1].arguments,
        [
            "clippy",
            "--manifest-path",
            &indicator,
            "--checker-control",
            "--message-format=json",
            "--",
            "--cap-lints=allow",
        ]
    );
    assert_eq!(invocations[1].exit_code, 0);
    assert_eq!(invocations[2].program, "cargo-clippy");
    assert_eq!(
        invocations[2].arguments,
        [
            "clippy",
            "--manifest-path",
            &indicator,
            "--checker-control",
            "--message-format=json",
            "--",
            "-D",
            "warnings",
        ]
    );
    assert!(
        invocations[1..]
            .iter()
            .flat_map(|invocation| &invocation.arguments)
            .all(|argument| argument != "--fix"),
        "adapter fix mode must remain a read-only native Clippy probe"
    );
    assert_eq!(invocations[2].exit_code, 101);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workspace_indicator_trace_plan_can_stop_after_preflight_or_coverage_and_rejects_forwarding() {
    let root = unique_temp_dir("velvet-glove-workspace-indicator-trace-failure-test");
    let indicator = root.join("Cargo.toml");
    let target = root.join("src/example.rs");
    std::fs::create_dir_all(target.parent().unwrap()).expect("workspace source directory");
    std::fs::write(&indicator, "not a valid manifest\n").expect("workspace indicator");
    std::fs::write(&target, "fn example() {}\n").expect("selected source");
    let indicator = canonical_project(&indicator).to_string_lossy().into_owned();
    let plan = TracePlan::PreflightThenNestedModeWorkspaceIndicatorMarker {
        preflight_program_index: 3,
        command_program_index: 4,
        adapter_prefix: &["-I", "-c"],
        marker: "__WORKSPACE_INDICATOR__",
        modes: &["fix", "verify"],
        preflight_before_indicator: &["metadata", "--manifest-path"],
        preflight_after_indicator: &[],
        command_before_indicator: &["clippy", "--manifest-path"],
        command_after_indicators: &[&["--", "--cap-lints=allow"], &["--", "-Dwarnings"]],
    };
    let targets = [canonical_project(&target)];
    let preflight_failure = [
        "-I".to_owned(),
        "-c".to_owned(),
        "adapter".to_owned(),
        "cargo".to_owned(),
        "cargo-clippy".to_owned(),
        "verify".to_owned(),
        "__WORKSPACE_INDICATOR__".to_owned(),
        indicator.clone(),
    ];

    let (_, invocations) =
        resolve_trace_invocations(plan, "python", &preflight_failure, &targets, &[101])
            .expect("resolve preflight-only failure");
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].program, "cargo");
    assert_eq!(
        invocations[0].arguments,
        ["metadata", "--manifest-path", &indicator]
    );
    assert_eq!(invocations[0].exit_code, 101);

    let (_, invocations) =
        resolve_trace_invocations(plan, "python", &preflight_failure, &targets, &[0, 101])
            .expect("resolve coverage failure");
    assert_eq!(invocations.len(), 2);
    assert_eq!(invocations[1].program, "cargo-clippy");
    assert_eq!(
        invocations[1].arguments,
        [
            "clippy",
            "--manifest-path",
            &indicator,
            "--",
            "--cap-lints=allow",
        ]
    );
    assert_eq!(invocations[1].exit_code, 101);

    let forwarded = [
        "-I".to_owned(),
        "-c".to_owned(),
        "adapter".to_owned(),
        "cargo".to_owned(),
        "cargo-clippy".to_owned(),
        "verify".to_owned(),
        "--package=escape".to_owned(),
        "__WORKSPACE_INDICATOR__".to_owned(),
        indicator,
    ];
    let error = resolve_trace_invocations(plan, "python", &forwarded, &targets, &[0, 0, 101])
        .expect_err("forwarded extra arguments must fail closed");
    assert!(error.contains("does not permit forwarded extra arguments"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn tool_trace_shim_dispatches_distinct_program_bindings() {
    let root = unique_temp_dir("velvet-glove-multi-program-trace-test");
    let shim_dir = root.join("shims");
    let real_dir = root.join("real");
    let trace_dir = root.join("trace");
    for directory in [&shim_dir, &real_dir, &trace_dir] {
        std::fs::create_dir_all(directory).expect("trace test directory");
    }

    for (program, exit_code) in [("cargo", 0), ("cargo-clippy", 7)] {
        let shim = shim_dir.join(program);
        let real = real_dir.join(program);
        std::fs::write(&shim, include_bytes!("support/tool-trace.sh")).expect("trace shim");
        std::fs::write(&real, format!("#!/bin/sh\nexit {exit_code}\n"))
            .expect("real program fixture");
        std::fs::write(
            shim_dir.join(format!("{program}.real-program")),
            format!("{}\n", real.display()),
        )
        .expect("real program binding");
        #[cfg(unix)]
        for executable in [&shim, &real] {
            let mut permissions = std::fs::metadata(executable)
                .expect("executable metadata")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(executable, permissions).expect("executable permissions");
        }

        let status = Command::new(&shim)
            .args(["probe", program])
            .env(TOOL_TRACE_DIR_ENV, &trace_dir)
            .env(TOOL_TRACE_SENTINEL_ENV, TOOL_TRACE_SENTINEL)
            .status()
            .expect("run trace shim");
        assert_eq!(status.code(), Some(exit_code));
    }

    let invocations = sorted_entries(&trace_dir.join("invocations")).expect("trace records");
    assert_eq!(invocations.len(), 2);
    for (invocation, program) in invocations.iter().zip(["cargo", "cargo-clippy"]) {
        let record = invocation.path();
        assert_record(&record, "logical-program", program).expect("logical program record");
        assert_record(
            &record,
            "real-program",
            real_dir.join(program).to_string_lossy().as_ref(),
        )
        .expect("real program record");
        assert_record(&record, "argv-0", "probe").expect("first argument record");
        assert_record(&record, "argv-1", program).expect("second argument record");
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn tool_trace_shim_does_not_treat_prettier_config_as_eslint_private_state() {
    let root = unique_temp_dir("velvet-glove-prettier-config-trace-test");
    let shim_dir = root.join("shims");
    let trace_dir = root.join("trace");
    std::fs::create_dir_all(&shim_dir).expect("trace shim directory");
    std::fs::create_dir_all(&trace_dir).expect("trace record directory");

    let shim = shim_dir.join("node");
    let real = root.join("real-node");
    std::fs::write(&shim, include_bytes!("support/tool-trace.sh")).expect("trace shim");
    std::fs::write(&real, "#!/bin/sh\nexit 0\n").expect("real Node fixture");
    std::fs::write(
        shim_dir.join("node.real-program"),
        format!("{}\n", real.display()),
    )
    .expect("real Node binding");
    #[cfg(unix)]
    for executable in [&shim, &real] {
        let mut permissions = std::fs::metadata(executable)
            .expect("executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(executable, permissions).expect("executable permissions");
    }

    let status = Command::new(&shim)
        .args([
            "--config=/dev/null",
            "--no-editorconfig",
            "--ignore-path=/dev/null",
            "--with-node-modules",
            "--no-color",
            "--",
            "example.js",
        ])
        .env(TOOL_TRACE_DIR_ENV, &trace_dir)
        .env(TOOL_TRACE_SENTINEL_ENV, TOOL_TRACE_SENTINEL)
        .status()
        .expect("trace Prettier-shaped Node invocation");
    assert_eq!(status.code(), Some(0));

    let invocations = sorted_entries(&trace_dir.join("invocations")).expect("trace records");
    assert_eq!(invocations.len(), 1);
    assert!(!invocations[0].path().join("eslint-private-root").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn astro_trace_environment_is_bound_to_the_executable_package_graph() {
    let root = unique_temp_dir("velvet-glove-astro-trace-environment");
    let node_modules = root.join("node_modules");
    let real_program = node_modules.join("astro/astro.js");
    for path in [
        &real_program,
        &node_modules.join("astro/package.json"),
        &node_modules.join("@astrojs/check/package.json"),
        &node_modules.join("typescript/package.json"),
    ] {
        std::fs::create_dir_all(path.parent().unwrap()).expect("package directory");
        std::fs::write(path, "{}\n").expect("package fixture");
    }
    let record = root.join("record");
    std::fs::create_dir_all(&record).expect("trace record");
    std::fs::write(
        record.join(format!("env-{NODE_PATH_ENV}")),
        format!("{}\n", node_modules.display()),
    )
    .expect("NODE_PATH record");
    std::fs::write(
        record.join(format!("env-{ASTRO_TELEMETRY_DISABLED_ENV}")),
        "1\n",
    )
    .expect("telemetry record");
    std::fs::write(record.join(format!("env-{CI_ENV}")), "1\n").expect("CI record");
    std::fs::write(record.join(format!("env-{DEBUG_ENV}")), "\n").expect("DEBUG record");
    let harness = ToolTraceHarness {
        shim_dir: root.join("shim"),
        trace_root: root.join("trace"),
        programs: BTreeMap::from([("astro".to_owned(), real_program)]),
        cargo_clippy_toolchain: None,
        cargo_fmt_toolchain: None,
        prettier_toolchain: None,
        eslint_toolchain: None,
        contextlint_toolchain: None,
        contextlint: false,
        dclint_toolchain: None,
        eslint: false,
    };

    let (observed_root, telemetry, ci, debug) =
        verify_astro_trace_environment(&record, &harness).expect("valid Astro trace environment");
    assert_eq!(
        PathBuf::from(observed_root),
        node_modules.canonicalize().unwrap()
    );
    assert_eq!(telemetry, "1");
    assert_eq!(ci, "1");
    assert!(debug.is_empty());

    std::fs::write(record.join(format!("env-{CI_ENV}")), "0\n").expect("invalid CI record");
    let error = verify_astro_trace_environment(&record, &harness)
        .expect_err("interactive Astro trace environment must fail closed");
    assert!(error.contains("CI=1"));

    std::fs::write(record.join(format!("env-{CI_ENV}")), "1\n").expect("restore CI record");
    std::fs::write(record.join(format!("env-{DEBUG_ENV}")), "astro:*\n")
        .expect("invalid DEBUG record");
    let error = verify_astro_trace_environment(&record, &harness)
        .expect_err("debug Astro trace environment must fail closed");
    assert!(error.contains("clear DEBUG"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn astro_trace_environment_rejects_a_different_module_graph() {
    let root = unique_temp_dir("velvet-glove-astro-trace-escape");
    let controlled = root.join("controlled/node_modules");
    let escaped = root.join("escaped/node_modules");
    let real_program = controlled.join("astro/astro.js");
    for path in [
        &real_program,
        &escaped.join("astro/package.json"),
        &escaped.join("@astrojs/check/package.json"),
        &escaped.join("typescript/package.json"),
    ] {
        std::fs::create_dir_all(path.parent().unwrap()).expect("package directory");
        std::fs::write(path, "{}\n").expect("package fixture");
    }
    let record = root.join("record");
    std::fs::create_dir_all(&record).expect("trace record");
    std::fs::write(
        record.join(format!("env-{NODE_PATH_ENV}")),
        format!("{}\n", escaped.display()),
    )
    .expect("NODE_PATH record");
    std::fs::write(
        record.join(format!("env-{ASTRO_TELEMETRY_DISABLED_ENV}")),
        "1\n",
    )
    .expect("telemetry record");
    let harness = ToolTraceHarness {
        shim_dir: root.join("shim"),
        trace_root: root.join("trace"),
        programs: BTreeMap::from([("astro".to_owned(), real_program)]),
        cargo_clippy_toolchain: None,
        cargo_fmt_toolchain: None,
        prettier_toolchain: None,
        eslint_toolchain: None,
        contextlint_toolchain: None,
        contextlint: false,
        dclint_toolchain: None,
        eslint: false,
    };

    let error = verify_astro_trace_environment(&record, &harness)
        .expect_err("escaped NODE_PATH must fail closed");
    assert!(error.contains("escaped its pinned executable graph"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn betterleaks_trace_environment_rejects_inherited_config() {
    let root = unique_temp_dir("velvet-glove-betterleaks-trace-environment");
    let record = root.join("record");
    std::fs::create_dir_all(&record).expect("trace record");
    for name in [
        BETTERLEAKS_CONFIG_ENV,
        BETTERLEAKS_CONFIG_TOML_ENV,
        GITLEAKS_CONFIG_ENV,
        GITLEAKS_CONFIG_TOML_ENV,
    ] {
        std::fs::write(record.join(format!("env-{name}")), "\n")
            .expect("empty Betterleaks config environment record");
    }
    let environment = verify_betterleaks_trace_environment(&record)
        .expect("scrubbed Betterleaks trace environment");
    assert!(environment.values().all(String::is_empty));

    std::fs::write(
        record.join(format!("env-{GITLEAKS_CONFIG_TOML_ENV}")),
        format!("{BETTERLEAKS_POISON_ENV_VALUE}\n"),
    )
    .expect("poisoned Betterleaks config environment record");
    let error = verify_betterleaks_trace_environment(&record)
        .expect_err("inherited Betterleaks-compatible config must fail closed");
    assert!(error.contains(GITLEAKS_CONFIG_TOML_ENV));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn buf_trace_environment_is_isolated_and_bound_to_the_managed_tool() {
    let root = unique_temp_dir("velvet-glove-buf-trace-environment");
    let shim_dir = root.join("tool-shim");
    let trace_root = root.join("tool-traces");
    let record = root.join("record");
    let cache = root.join("tmp/velvet-glove-buf-cache");
    std::fs::create_dir_all(&shim_dir).expect("Buf shim directory");
    std::fs::create_dir_all(&trace_root).expect("Buf trace directory");
    std::fs::create_dir_all(&record).expect("Buf trace record");
    std::fs::create_dir_all(&cache).expect("controlled Buf cache");
    let shim = shim_dir.join("buf");
    std::fs::write(&shim, "fixture shim\n").expect("Buf shim");

    for (name, value) in [
        (PATH_ENV, BUF_CHILD_PATH.to_owned()),
        (HOME_ENV, root.join("home").to_string_lossy().into_owned()),
        (TMPDIR_ENV, root.join("tmp").to_string_lossy().into_owned()),
        (
            XDG_CACHE_HOME_ENV,
            root.join("xdg-cache").to_string_lossy().into_owned(),
        ),
        (DIFF_OPTIONS_ENV, String::new()),
        (BUF_CACHE_DIR_ENV, cache.to_string_lossy().into_owned()),
    ] {
        std::fs::write(record.join(format!("env-{name}")), format!("{value}\n"))
            .expect("controlled Buf environment record");
    }
    for name in BUF_SCRUBBED_ENV {
        std::fs::write(record.join(format!("env-{name}")), "\n")
            .expect("scrubbed Buf environment record");
    }
    std::fs::write(record.join("program"), format!("{}\n", shim.display()))
        .expect("absolute Buf shim record");
    let harness = ToolTraceHarness {
        shim_dir,
        trace_root,
        programs: BTreeMap::from([("buf".to_owned(), root.join("managed/bin/buf"))]),
        cargo_clippy_toolchain: None,
        cargo_fmt_toolchain: None,
        prettier_toolchain: None,
        eslint_toolchain: None,
        contextlint_toolchain: None,
        contextlint: false,
        dclint_toolchain: None,
        eslint: false,
    };

    let environment = verify_buf_trace_environment(&record, &harness)
        .expect("isolated managed Buf trace environment");
    assert_eq!(
        environment.get(PATH_ENV).map(String::as_str),
        Some(BUF_CHILD_PATH)
    );
    assert_eq!(
        environment.get(BUF_CACHE_DIR_ENV).map(String::as_str),
        Some(cache.to_string_lossy().as_ref())
    );
    assert!(
        BUF_SCRUBBED_ENV
            .iter()
            .all(|name| environment.get(*name).is_some_and(String::is_empty))
    );

    let dynamic = BUF_SCRUBBED_ENV
        .last()
        .expect("dynamic Buf poison environment name");
    std::fs::write(
        record.join(format!("env-{dynamic}")),
        format!("{BUF_POISON_ENV_VALUE}\n"),
    )
    .expect("poisoned dynamic Buf environment record");
    let error = verify_buf_trace_environment(&record, &harness)
        .expect_err("an inherited future Buf variable must fail closed");
    assert!(error.contains(dynamic));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn gofmt_trace_environment_is_isolated_and_bound_to_the_managed_tool() {
    let root = unique_temp_dir("velvet-glove-gofmt-trace-environment");
    let shim_dir = root.join("tool-shim");
    let trace_root = root.join("tool-traces");
    let record = root.join("record");
    std::fs::create_dir_all(&shim_dir).expect("gofmt shim directory");
    std::fs::create_dir_all(&trace_root).expect("gofmt trace directory");
    std::fs::create_dir_all(&record).expect("gofmt trace record");
    let shim = shim_dir.join("gofmt");
    std::fs::write(&shim, "fixture shim\n").expect("gofmt shim");

    for (name, value) in std::iter::once((PATH_ENV, GOFMT_CHILD_PATH))
        .chain(std::iter::once(("TERM", "dumb")))
        .chain(GOFMT_CONTROLLED_ENV.iter().copied())
    {
        std::fs::write(record.join(format!("env-{name}")), format!("{value}\n"))
            .expect("controlled gofmt environment record");
    }
    for name in GOFMT_SCRUBBED_ENV.iter().chain(GOFMT_LOADER_SCRUBBED_ENV) {
        std::fs::write(record.join(format!("env-{name}")), "\n")
            .expect("scrubbed gofmt environment record");
    }
    std::fs::write(record.join("program"), format!("{}\n", shim.display()))
        .expect("absolute gofmt shim record");
    let harness = ToolTraceHarness {
        shim_dir,
        trace_root,
        programs: BTreeMap::from([("gofmt".to_owned(), root.join("managed/bin/gofmt"))]),
        cargo_clippy_toolchain: None,
        cargo_fmt_toolchain: None,
        prettier_toolchain: None,
        eslint_toolchain: None,
        contextlint_toolchain: None,
        contextlint: false,
        dclint_toolchain: None,
        eslint: false,
    };

    let environment = verify_gofmt_trace_environment(&record, &harness)
        .expect("isolated managed gofmt trace environment");
    assert_eq!(
        environment.get(PATH_ENV).map(String::as_str),
        Some(GOFMT_CHILD_PATH)
    );
    assert_eq!(
        environment.get("GOTELEMETRY").map(String::as_str),
        Some("off")
    );
    assert!(
        GOFMT_SCRUBBED_ENV
            .iter()
            .chain(GOFMT_LOADER_SCRUBBED_ENV)
            .all(|name| environment.get(*name).is_some_and(String::is_empty))
    );

    std::fs::write(
        record.join("env-GO_VELVET_GLOVE_POISON"),
        format!("{GOFMT_POISON_ENV_VALUE}\n"),
    )
    .expect("poisoned dynamic Go environment record");
    let error = verify_gofmt_trace_environment(&record, &harness)
        .expect_err("an inherited future Go variable must fail closed");
    assert!(error.contains("GO_VELVET_GLOVE_POISON"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn betterleaks_output_requires_adapter_canonicalization() {
    let suffix = "unable to load config, err: open does-not-exist.toml: no such file or directory";
    for clock in ["8:10AM", "12:59PM"] {
        let diagnostic = format!("{clock} FTL {suffix}");
        let error = verify_tool_output_is_canonical("betterleaks", "synthetic stderr", &diagnostic)
            .expect_err("raw Betterleaks console clocks must fail closed");
        assert!(error.contains(clock));
    }
    verify_tool_output_is_canonical(
        "betterleaks",
        "synthetic stderr",
        &format!("<time> FTL {suffix}"),
    )
    .expect("the adapter's canonical Betterleaks diagnostic must pass");
    verify_tool_output_is_canonical(
        "betterleaks",
        "synthetic stderr",
        "8:10AM INF completed scan",
    )
    .expect("unrelated Betterleaks levels must remain untouched");
    verify_tool_output_is_canonical("jq", "synthetic stderr", &format!("8:10AM FTL {suffix}"))
        .expect("the Betterleaks assertion must not rewrite or reject other tools");
}

#[test]
fn buf_output_requires_adapter_canonicalization() {
    let raw =
        "--- example.proto.orig\t2026-08-09 12:34:56\n+++ example.proto\t2026-08-09 12:34:56\n";
    let error = verify_tool_output_is_canonical("buf-format", "synthetic stdout", raw)
        .expect_err("raw Buf diff mtimes must fail closed");
    assert!(error.contains("dynamic diff mtime"));

    let canonical = "--- example.proto.orig\t<mtime>\n+++ example.proto\t<mtime>\n";
    verify_tool_output_is_canonical("buf-format", "synthetic stdout", canonical)
        .expect("adapter-canonicalized Buf diff headers must pass");
    verify_tool_output_is_canonical("jq", "synthetic stdout", raw)
        .expect("the Buf assertion must not reject other tools");
}

#[test]
fn requested_probe_failure_artifacts_are_retained() {
    let artifact_root = unique_temp_dir("velvet-glove-probe-artifact-root");
    let error = run_probe_attempt(ProtocolSurface::Claude, Some(&artifact_root), |root| {
        std::fs::create_dir_all(root.join("evidence"))
            .map_err(|error| format!("create probe evidence: {error}"))?;
        std::fs::write(root.join("evidence/input.json"), "{\"probe\":true}")
            .map_err(|error| format!("write probe evidence: {error}"))?;
        Err("intentional probe failure".to_owned())
    })
    .expect_err("failing probe must return an error");

    assert!(error.contains("intentional probe failure"));
    assert!(error.contains("retained probe artifacts"));
    let retained =
        sorted_entries(&artifact_root.join("probe/claude")).expect("retained probe directories");
    assert_eq!(retained.len(), 1);
    assert_eq!(
        std::fs::read_to_string(retained[0].path().join("evidence/input.json")).unwrap(),
        "{\"probe\":true}"
    );
    assert!(
        retained[0]
            .path()
            .join("evidence/probe-outcome.json")
            .is_file()
    );

    let _ = std::fs::remove_dir_all(artifact_root);
}

#[test]
fn machine_report_reconciles_totals_and_structured_skips() {
    let catalog = FixtureCatalog {
        tool_count: 1,
        cases: vec![fixture_case("case-a"), fixture_case("case-b")],
    };
    let outcomes = vec![
        FixtureOutcome::pass(&catalog.cases[0], ProtocolSurface::Claude),
        FixtureOutcome::skipped(
            &catalog.cases[0],
            ProtocolSurface::Codex,
            SkipReason {
                code: "executable-unavailable",
                detail: "missing fixture-tool".to_owned(),
            },
        ),
        FixtureOutcome::failed(
            &catalog.cases[1],
            ProtocolSurface::Claude,
            "golden mismatch",
        ),
        FixtureOutcome::pass(&catalog.cases[1], ProtocolSurface::Codex),
    ];

    let report = build_report(&catalog, &outcomes, ProtocolSurface::ALL.len());
    let totals = &report["totals"];
    assert_eq!(totals["plannedSurfaceCases"], 4);
    assert_eq!(totals["attemptedSurfaceCases"], 3);
    assert_eq!(totals["passed"], 2);
    assert_eq!(totals["skipped"], 1);
    assert_eq!(totals["failed"], 1);
    assert_eq!(report["skipReasons"]["executable-unavailable"], 1);
    assert_eq!(
        report["outcomes"][1]["reason"]["code"],
        "executable-unavailable"
    );
    assert_eq!(
        totals["plannedSurfaceCases"].as_u64(),
        Some(
            totals["passed"].as_u64().unwrap()
                + totals["skipped"].as_u64().unwrap()
                + totals["failed"].as_u64().unwrap()
        )
    );
}

#[test]
fn machine_report_writes_a_stable_index_and_historical_copy() {
    let root = unique_temp_dir("velvet-glove-report-index");
    let report = serde_json::json!({"formatVersion": 1, "kind": "example"});

    let stable = write_report(&root, &report).expect("write machine report");

    assert_eq!(stable, root.join("report.json"));
    assert_eq!(
        serde_json::from_slice::<JsonValue>(&std::fs::read(&stable).unwrap()).unwrap(),
        report
    );
    let historical = sorted_entries(&root)
        .unwrap()
        .into_iter()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("report-") && name.ends_with(".json"))
        })
        .collect::<Vec<_>>();
    assert_eq!(historical.len(), 1);

    let _ = std::fs::remove_dir_all(root);
}

fn fixture_case(name: &str) -> FixtureCase {
    named_fixture_case("fixture-tool", name)
}

fn named_fixture_case(tool: &str, name: &str) -> FixtureCase {
    FixtureCase {
        tool: tool.to_owned(),
        case: name.to_owned(),
        directory: PathBuf::new(),
        entry: PathBuf::from("example.txt"),
        pkl_property: "fixtureTool".to_owned(),
        spec: ToolSpec::default(),
    }
}

#[test]
#[ignore = "evaluated Biome adapter lifecycle; requires controlled Python"]
fn biome_evaluated_adapter_lifecycle() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("biome")
        .unwrap_or_else(|| panic!("builtin catalog has no Biome spec"));
    verify_biome_adapter_lifecycle(spec, timeout).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "evaluated Contextlint adapter adversarial probe; requires controlled Python"]
fn contextlint_evaluated_adapter_noop_resistance() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("contextlint")
        .unwrap_or_else(|| panic!("builtin catalog has no Contextlint spec"));
    verify_contextlint_adapter_noop_resistance(spec, timeout)
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "evaluated Contextlint adapter lifecycle; requires controlled Python"]
fn contextlint_evaluated_adapter_lifecycle() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("contextlint")
        .unwrap_or_else(|| panic!("builtin catalog has no Contextlint spec"));
    verify_contextlint_adapter_lifecycle(spec, timeout).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "evaluated ghalint-workflow adapter lifecycle; requires controlled Python"]
fn ghalint_workflow_evaluated_adapter_lifecycle() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("ghalint-workflow")
        .unwrap_or_else(|| panic!("builtin catalog has no ghalint-workflow spec"));
    verify_ghalint_workflow_adapter_lifecycle(spec, timeout)
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "evaluated gofmt adapter lifecycle; requires controlled Python"]
fn gofmt_evaluated_adapter_lifecycle() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("go-fmt")
        .unwrap_or_else(|| panic!("builtin catalog has no gofmt spec"));
    verify_gofmt_adapter_lifecycle(spec, timeout).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "evaluated Vacuum adapter lifecycle; requires controlled Python"]
fn vacuum_evaluated_adapter_lifecycle() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("vacuum")
        .unwrap_or_else(|| panic!("builtin catalog has no Vacuum spec"));
    verify_vacuum_adapter_lifecycle(spec, timeout).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "evaluated Cargo Fmt adapter lifecycle; requires controlled Python"]
fn cargo_fmt_evaluated_adapter_lifecycle() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("cargo-fmt")
        .unwrap_or_else(|| panic!("builtin catalog has no Cargo Fmt spec"));
    verify_cargo_fmt_adapter_lifecycle(spec, timeout).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn cargo_fmt_failure_goldens_embed_the_evaluated_adapter() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("cargo-fmt")
        .unwrap_or_else(|| panic!("builtin catalog has no Cargo Fmt spec"));
    let phase = spec.phases.get("format").expect("Cargo Fmt format phase");
    let ArgvElement::Literal(adapter) = &phase.argv[2] else {
        panic!("Cargo Fmt format phase must embed one literal adapter")
    };
    let command_prefix = "[format] command: python -I -c ";
    let command_suffix = " cargo cargo-fmt rustfmt format __VELVET_GLOVE_CARGO_FMT_WORKSPACE__ <workspace>/Cargo.lock";
    for case in ["coverage-failure", "operational-failure"] {
        for surface in ["claude", "codex"] {
            let path = fixtures_root()
                .join("cargo-fmt")
                .join(case)
                .join(format!("{surface}.stderr.txt"));
            let golden = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            assert_eq!(
                golden.matches(command_prefix).count(),
                1,
                "{}",
                path.display()
            );
            assert_eq!(
                golden.matches(command_suffix).count(),
                1,
                "{}",
                path.display()
            );
            let embedded = golden
                .split_once(command_prefix)
                .expect("checked Cargo Fmt command prefix")
                .1
                .split_once(command_suffix)
                .expect("checked Cargo Fmt command suffix")
                .0;
            assert_eq!(
                embedded.as_bytes(),
                adapter.as_bytes(),
                "{}",
                path.display()
            );
        }
    }
}

#[test]
#[ignore = "evaluated Prettier adapter adversarial contract; requires controlled Python"]
fn prettier_evaluated_adapter_adversarial_contract() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("prettier")
        .unwrap_or_else(|| panic!("builtin catalog has no Prettier spec"));
    verify_prettier_adapter_adversarial_contract(spec, timeout)
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "evaluated dclint adapter lifecycle; requires controlled Python and Node"]
fn dclint_evaluated_adapter_lifecycle() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("dclint")
        .unwrap_or_else(|| panic!("builtin catalog has no dclint spec"));
    verify_dclint_adapter_lifecycle(spec, timeout).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "evaluated ESLint adapter adversarial lifecycle; requires controlled Python"]
fn eslint_evaluated_adapter_adversarial_lifecycle() {
    let timeout = configured_timeout().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let (_, spec) = specs
        .get("eslint")
        .unwrap_or_else(|| panic!("builtin catalog has no ESLint spec"));
    verify_eslint_adapter_adversarial_lifecycle(spec, timeout)
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "real-tool compatibility lane; requires controlled PATH versions"]
fn run_all_tool_fixtures() {
    let options = HarnessOptions::from_environment().unwrap_or_else(|error| panic!("{error}"));
    require_pkl(options.timeout).unwrap_or_else(|error| panic!("{error}"));
    let specs = builtin_index().unwrap_or_else(|error| panic!("{error}"));
    let catalog = discover_fixture_catalog(&fixtures_root(), &specs)
        .unwrap_or_else(|error| panic!("fixture discovery failed: {error}"));
    let catalog = options
        .selection
        .apply(catalog)
        .unwrap_or_else(|error| panic!("{error}"));
    options
        .required_tools
        .validate(&catalog.tool_ids())
        .unwrap_or_else(|error| panic!("{error}"));
    if let Some(case) = catalog.cases.iter().find(|case| case.tool == "betterleaks") {
        verify_betterleaks_adapter_lifecycle(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("betterleaks adapter lifecycle probe: pass");
    }
    if let Some(case) = catalog.cases.iter().find(|case| case.tool == "biome") {
        verify_biome_adapter_lifecycle(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("biome adapter lifecycle probe: pass");
    }
    if let Some(case) = catalog.cases.iter().find(|case| case.tool == "contextlint") {
        verify_contextlint_adapter_noop_resistance(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("contextlint adapter no-op resistance probe: pass");
        verify_contextlint_adapter_lifecycle(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("contextlint adapter lifecycle probe: pass");
    }
    if let Some(case) = catalog
        .cases
        .iter()
        .find(|case| case.tool == "ghalint-workflow")
    {
        verify_ghalint_workflow_adapter_lifecycle(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("ghalint-workflow adapter lifecycle probe: pass");
    }
    if let Some(case) = catalog.cases.iter().find(|case| case.tool == "go-fmt") {
        verify_gofmt_adapter_lifecycle(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("gofmt adapter lifecycle probe: pass");
    }
    if let Some(case) = catalog.cases.iter().find(|case| case.tool == "vacuum") {
        verify_vacuum_adapter_lifecycle(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("vacuum adapter lifecycle probe: pass");
    }
    if let Some(case) = catalog.cases.iter().find(|case| case.tool == "cargo-fmt") {
        verify_cargo_fmt_adapter_lifecycle(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("cargo-fmt adapter lifecycle probe: pass");
    }
    if let Some(case) = catalog.cases.iter().find(|case| case.tool == "prettier") {
        verify_prettier_adapter_adversarial_contract(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("prettier adapter adversarial contract probe: pass");
    }
    if let Some(case) = catalog.cases.iter().find(|case| case.tool == "dclint") {
        verify_dclint_adapter_lifecycle(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("dclint adapter lifecycle probe: pass");
    }
    if let Some(case) = catalog.cases.iter().find(|case| case.tool == "eslint") {
        verify_eslint_adapter_adversarial_lifecycle(&case.spec, options.timeout)
            .unwrap_or_else(|error| panic!("{error}"));
        println!("eslint adapter adversarial lifecycle probe: pass");
    }
    let probe_commands = run_probe_matrix(options.timeout, options.artifact_dir.as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(probe_commands > 0, "probe executed zero external commands");

    let mut availability = BTreeMap::<String, Result<(), Vec<String>>>::new();
    let mut outcomes = Vec::with_capacity(catalog.cases.len() * REAL_TOOL_SURFACES.len());
    for case in &catalog.cases {
        let available = availability
            .entry(case.tool.clone())
            .or_insert_with(|| check_tool_programs(&case.spec));
        for surface in REAL_TOOL_SURFACES {
            match available {
                Ok(()) => outcomes.push(run_fixture_case(case, *surface, &options)),
                Err(programs)
                    if options.selection.is_active()
                        || options.required_tools.requires(&case.tool) =>
                {
                    outcomes.push(FixtureOutcome::failed(
                        case,
                        *surface,
                        format!("required prerequisite unavailable: {}", programs.join(", ")),
                    ));
                }
                Err(programs) => outcomes.push(FixtureOutcome::skipped(
                    case,
                    *surface,
                    SkipReason {
                        code: "executable-unavailable",
                        detail: format!("programs not found on PATH: {}", programs.join(", ")),
                    },
                )),
            }
        }
    }

    let report = build_report(&catalog, &outcomes, probe_commands);
    print_outcomes(&outcomes);
    println!("{REPORT_PREFIX}{report}");
    if let Some(root) = &options.artifact_dir {
        let path = write_report(root, &report).unwrap_or_else(|error| panic!("{error}"));
        println!("machine-readable report: {}", path.display());
    }

    let planned = catalog.cases.len() * REAL_TOOL_SURFACES.len();
    assert_eq!(
        outcomes.len(),
        planned,
        "surface-case totals must reconcile"
    );
    let attempted = outcomes
        .iter()
        .filter(|outcome| !matches!(outcome.status, FixtureStatus::Skip(_)))
        .count();
    assert!(attempted > 0, "real-tool lane attempted zero surface cases");

    let failures = outcomes
        .iter()
        .filter_map(|outcome| match &outcome.status {
            FixtureStatus::Fail(reason) => Some(format!(
                "{}/{} ({}): {reason}",
                outcome.tool, outcome.case, outcome.surface
            )),
            FixtureStatus::Pass | FixtureStatus::Skip(_) => None,
        })
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "{} fixture surface(s) failed:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[derive(Debug)]
struct HarnessOptions {
    timeout: Duration,
    artifact_dir: Option<PathBuf>,
    required_tools: RequiredTools,
    selection: FixtureSelection,
}

impl HarnessOptions {
    fn from_environment() -> Result<Self, String> {
        Ok(Self {
            timeout: configured_timeout()?,
            artifact_dir: configured_artifact_dir()?,
            required_tools: RequiredTools::from_environment()?,
            selection: FixtureSelection::from_environment()?,
        })
    }
}

#[derive(Debug, Default)]
struct FixtureSelection {
    tools: BTreeSet<String>,
    cases: BTreeSet<(String, String)>,
}

impl FixtureSelection {
    fn from_environment() -> Result<Self, String> {
        let Some(value) = std::env::var_os(SELECTION_ENV) else {
            return Ok(Self::default());
        };
        let value = value
            .into_string()
            .map_err(|_| format!("{SELECTION_ENV} must be UTF-8"))?;
        let mut selection = Self::default();
        for selector in value
            .split(',')
            .map(str::trim)
            .filter(|selector| !selector.is_empty())
        {
            let mut parts = selector.split('/');
            let tool = parts.next().unwrap_or_default();
            let case = parts.next();
            if tool.is_empty() || parts.next().is_some() || case.is_some_and(str::is_empty) {
                return Err(format!(
                    "{SELECTION_ENV} entries must be `tool-id` or `tool-id/case-id`; invalid entry {selector:?}"
                ));
            }
            match case {
                Some(case) => {
                    selection.cases.insert((tool.to_owned(), case.to_owned()));
                }
                None => {
                    selection.tools.insert(tool.to_owned());
                }
            }
        }
        if selection.tools.is_empty() && selection.cases.is_empty() {
            return Err(format!(
                "{SELECTION_ENV} must contain at least one `tool-id` or `tool-id/case-id`"
            ));
        }
        let redundant = selection
            .cases
            .iter()
            .filter(|(tool, _)| selection.tools.contains(tool))
            .map(|(tool, case)| format!("{tool}/{case}"))
            .collect::<Vec<_>>();
        if !redundant.is_empty() {
            return Err(format!(
                "{SELECTION_ENV} contains case selectors already covered by a tool selector: {}",
                redundant.join(", ")
            ));
        }
        Ok(selection)
    }

    fn is_active(&self) -> bool {
        !self.tools.is_empty() || !self.cases.is_empty()
    }

    fn apply(&self, catalog: FixtureCatalog) -> Result<FixtureCatalog, String> {
        if !self.is_active() {
            return Ok(catalog);
        }
        let available_tools = catalog.tool_ids();
        let requested_tools = self
            .tools
            .iter()
            .cloned()
            .chain(self.cases.iter().map(|(tool, _)| tool.clone()))
            .collect::<BTreeSet<_>>();
        let unknown_tools = requested_tools
            .difference(&available_tools)
            .cloned()
            .collect::<Vec<_>>();
        if !unknown_tools.is_empty() {
            return Err(format!(
                "{SELECTION_ENV} names tools without fixture cases: {}",
                unknown_tools.join(", ")
            ));
        }
        let available_cases = catalog
            .cases
            .iter()
            .map(|case| (case.tool.clone(), case.case.clone()))
            .collect::<BTreeSet<_>>();
        let unknown_cases = self
            .cases
            .difference(&available_cases)
            .map(|(tool, case)| format!("{tool}/{case}"))
            .collect::<Vec<_>>();
        if !unknown_cases.is_empty() {
            return Err(format!(
                "{SELECTION_ENV} names unknown fixture cases: {}",
                unknown_cases.join(", ")
            ));
        }
        let cases = catalog
            .cases
            .into_iter()
            .filter(|case| {
                self.tools.contains(&case.tool)
                    || self.cases.contains(&(case.tool.clone(), case.case.clone()))
            })
            .collect::<Vec<_>>();
        let tool_count = cases
            .iter()
            .map(|case| case.tool.as_str())
            .collect::<BTreeSet<_>>()
            .len();
        if cases.is_empty() {
            return Err(format!("{SELECTION_ENV} selected zero fixture cases"));
        }
        Ok(FixtureCatalog { tool_count, cases })
    }
}

#[derive(Debug, Default)]
struct RequiredTools {
    all: bool,
    names: BTreeSet<String>,
}

impl RequiredTools {
    fn from_environment() -> Result<Self, String> {
        let Some(value) = std::env::var_os(REQUIRED_TOOLS_ENV) else {
            return Ok(Self::default());
        };
        let value = value
            .into_string()
            .map_err(|_| format!("{REQUIRED_TOOLS_ENV} must be UTF-8"))?;
        let mut required = Self::default();
        for name in value
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if name == "all" {
                required.all = true;
            } else {
                required.names.insert(name.to_owned());
            }
        }
        if !required.all && required.names.is_empty() && !value.trim().is_empty() {
            return Err(format!(
                "{REQUIRED_TOOLS_ENV} must be `all` or a comma-separated tool-id list"
            ));
        }
        Ok(required)
    }

    fn requires(&self, tool: &str) -> bool {
        self.all || self.names.contains(tool)
    }

    fn validate(&self, available: &BTreeSet<String>) -> Result<(), String> {
        let unknown = self
            .names
            .difference(available)
            .cloned()
            .collect::<Vec<_>>();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{REQUIRED_TOOLS_ENV} names tools without fixture cases: {}",
                unknown.join(", ")
            ))
        }
    }
}

#[derive(Debug)]
struct FixtureCatalog {
    tool_count: usize,
    cases: Vec<FixtureCase>,
}

impl FixtureCatalog {
    fn tool_ids(&self) -> BTreeSet<String> {
        self.cases.iter().map(|case| case.tool.clone()).collect()
    }
}

#[derive(Debug)]
struct FixtureCase {
    tool: String,
    case: String,
    directory: PathBuf,
    entry: PathBuf,
    pkl_property: String,
    spec: ToolSpec,
}

fn builtin_index() -> Result<BTreeMap<String, (String, ToolSpec)>, String> {
    let specs = hookkit_pkl_config::builtin_specs()
        .map_err(|error| format!("load builtin tool specs: {error}"))?;
    let mut by_id = BTreeMap::new();
    for (property, spec) in specs {
        let id = spec.id.clone();
        if by_id.insert(id.clone(), (property, spec)).is_some() {
            return Err(format!("duplicate builtin tool id {id}"));
        }
    }
    Ok(by_id)
}

fn discover_fixture_catalog(
    root: &Path,
    specs: &BTreeMap<String, (String, ToolSpec)>,
) -> Result<FixtureCatalog, String> {
    if !root.is_dir() {
        return Err(format!(
            "required fixture root is not a directory: {root:?}"
        ));
    }
    let mut cases = Vec::new();
    let mut tool_count = 0;
    for tool_entry in sorted_entries(root)? {
        let name = tool_entry.file_name();
        if name == OsStr::new("README.md") && tool_entry.path().is_file() {
            continue;
        }
        let file_type = tool_entry
            .file_type()
            .map_err(|error| format!("file type for {:?}: {error}", tool_entry.path()))?;
        if !file_type.is_dir() {
            return Err(format!(
                "orphan fixture-root entry is not a tool directory: {:?}",
                tool_entry.path()
            ));
        }
        let tool = name
            .into_string()
            .map_err(|name| format!("tool directory name is not UTF-8: {name:?}"))?;
        let Some((property, spec)) = specs.get(&tool) else {
            return Err(format!(
                "orphan fixture tool directory has no builtin spec: {tool}"
            ));
        };
        if !spec.enabled {
            return Err(format!(
                "orphan fixture tool directory targets disabled spec: {tool}"
            ));
        }
        tool_count += 1;

        let before = cases.len();
        for case_entry in sorted_entries(&tool_entry.path())? {
            let case_name = case_entry.file_name();
            if case_name == OsStr::new("README.md") && case_entry.path().is_file() {
                continue;
            }
            let file_type = case_entry
                .file_type()
                .map_err(|error| format!("file type for {:?}: {error}", case_entry.path()))?;
            if !file_type.is_dir() {
                return Err(format!(
                    "orphan entry in tool fixture directory {tool}: {:?}",
                    case_entry.path()
                ));
            }
            let case = case_name
                .into_string()
                .map_err(|name| format!("case directory name is not UTF-8: {name:?}"))?;
            let directory = case_entry.path();
            validate_supported_goldens(&directory)
                .map_err(|error| format!("{tool}/{case}: {error}"))?;
            let entry =
                find_entry_file(&directory).map_err(|error| format!("{tool}/{case}: {error}"))?;
            cases.push(FixtureCase {
                tool: tool.clone(),
                case,
                directory,
                entry,
                pkl_property: property.clone(),
                spec: spec.clone(),
            });
        }
        if cases.len() == before {
            return Err(format!(
                "fixture tool directory contains zero cases: {tool}"
            ));
        }
    }
    if tool_count == 0 {
        return Err("fixture discovery found zero tool directories".to_owned());
    }
    if cases.is_empty() {
        return Err("fixture discovery found zero cases".to_owned());
    }
    Ok(FixtureCatalog { tool_count, cases })
}

fn sorted_entries(path: &Path) -> Result<Vec<std::fs::DirEntry>, String> {
    let entries = std::fs::read_dir(path)
        .map_err(|error| format!("read fixture directory {path:?}: {error}"))?;
    let mut entries = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read entry in fixture directory {path:?}: {error}"))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    Ok(entries)
}

#[derive(Debug)]
struct FixtureOutcome {
    tool: String,
    case: String,
    surface: ProtocolSurface,
    status: FixtureStatus,
    artifacts: Option<PathBuf>,
}

#[derive(Debug)]
enum FixtureStatus {
    Pass,
    Skip(SkipReason),
    Fail(String),
}

#[derive(Debug)]
struct SkipReason {
    code: &'static str,
    detail: String,
}

impl FixtureOutcome {
    fn pass(case: &FixtureCase, surface: ProtocolSurface) -> Self {
        Self {
            tool: case.tool.clone(),
            case: case.case.clone(),
            surface,
            status: FixtureStatus::Pass,
            artifacts: None,
        }
    }

    fn skipped(case: &FixtureCase, surface: ProtocolSurface, reason: SkipReason) -> Self {
        Self {
            tool: case.tool.clone(),
            case: case.case.clone(),
            surface,
            status: FixtureStatus::Skip(reason),
            artifacts: None,
        }
    }

    fn failed(case: &FixtureCase, surface: ProtocolSurface, reason: impl Into<String>) -> Self {
        Self {
            tool: case.tool.clone(),
            case: case.case.clone(),
            surface,
            status: FixtureStatus::Fail(reason.into()),
            artifacts: None,
        }
    }

    fn as_json(&self) -> JsonValue {
        let (status, detail) = match &self.status {
            FixtureStatus::Pass => ("pass", JsonValue::Null),
            FixtureStatus::Skip(reason) => (
                "skip",
                serde_json::json!({"code": reason.code, "detail": reason.detail}),
            ),
            FixtureStatus::Fail(reason) => ("fail", serde_json::json!({"detail": reason})),
        };
        serde_json::json!({
            "tool": self.tool,
            "case": self.case,
            "surface": self.surface.cli_name(),
            "status": status,
            "reason": detail,
            "artifacts": self.artifacts.as_ref().map(|path| path.to_string_lossy()),
        })
    }
}

struct FixtureWorkspace {
    root: PathBuf,
    project: PathBuf,
    evidence: PathBuf,
}

struct FixtureSetupFailure {
    root: PathBuf,
    detail: String,
}

impl FixtureWorkspace {
    fn prepare(case: &FixtureCase, surface: ProtocolSurface) -> Result<Self, FixtureSetupFailure> {
        let root = unique_temp_dir(&format!(
            "velvet-glove-fixture-{}-{}-{surface}",
            case.tool, case.case
        ));
        let project = root.join("workspace");
        let evidence = root.join("evidence");
        if let Err(error) = std::fs::create_dir_all(&project) {
            return Err(FixtureSetupFailure {
                root,
                detail: format!("create fixture workspace {project:?}: {error}"),
            });
        }
        if let Err(error) = std::fs::create_dir_all(&evidence) {
            return Err(FixtureSetupFailure {
                root,
                detail: format!("create fixture evidence {evidence:?}: {error}"),
            });
        }
        if let Err(error) = copy_fixture_inputs(&case.directory, &case.directory, &project) {
            return Err(FixtureSetupFailure {
                root,
                detail: error,
            });
        }
        Ok(Self {
            root,
            project,
            evidence,
        })
    }
}

fn run_fixture_case(
    case: &FixtureCase,
    surface: ProtocolSurface,
    options: &HarnessOptions,
) -> FixtureOutcome {
    let workspace = match FixtureWorkspace::prepare(case, surface) {
        Ok(workspace) => workspace,
        Err(failure) => {
            return finalize_fixture_outcome(
                &failure.root,
                case,
                surface,
                options,
                FixtureOutcome::failed(case, surface, failure.detail),
            );
        }
    };
    let result = run_fixture_case_inner(case, surface, options.timeout, &workspace);
    let outcome = match result {
        Ok(()) => FixtureOutcome::pass(case, surface),
        Err(error) => FixtureOutcome::failed(case, surface, error),
    };
    finalize_fixture_outcome(&workspace.root, case, surface, options, outcome)
}

fn finalize_fixture_outcome(
    root: &Path,
    case: &FixtureCase,
    surface: ProtocolSurface,
    options: &HarnessOptions,
    mut outcome: FixtureOutcome,
) -> FixtureOutcome {
    let mut preserve_temporary_evidence = false;
    let retain_success = matches!(outcome.status, FixtureStatus::Pass)
        && options.artifact_dir.is_some()
        && matches!(real_tool_contract_case(case), Ok(Some(_)));

    if matches!(outcome.status, FixtureStatus::Fail(_)) || retain_success {
        let evidence = root.join("evidence");
        if let Err(error) = std::fs::create_dir_all(&evidence)
            .map_err(|error| format!("create fixture evidence directory: {error}"))
            .and_then(|()| write_json(&evidence.join("outcome.json"), &outcome.as_json()))
        {
            append_failure(&mut outcome, format!("write fixture evidence: {error}"));
        }
        if let Some(artifact_root) = &options.artifact_dir {
            match retain_failure(root, artifact_root, case, surface) {
                Ok(path) => {
                    outcome.artifacts = Some(path.clone());
                    if let Err(error) =
                        write_json(&path.join("evidence/outcome.json"), &outcome.as_json())
                    {
                        append_failure(&mut outcome, format!("update retained outcome: {error}"));
                    }
                }
                Err(error) => {
                    preserve_temporary_evidence = true;
                    append_failure(
                        &mut outcome,
                        format!(
                            "{error}; preserved temporary fixture evidence at {}",
                            root.display()
                        ),
                    );
                }
            }
        }
    }
    if !preserve_temporary_evidence {
        let _ = std::fs::remove_dir_all(root);
    }
    outcome
}

fn run_fixture_case_inner(
    case: &FixtureCase,
    surface: ProtocolSurface,
    timeout: Duration,
    workspace: &FixtureWorkspace,
) -> Result<(), String> {
    let contract = real_tool_contract_case(case)?;
    let mutation = mutating_tool_contract_case(case)?;
    if mutation.is_some() && contract.is_none() {
        return Err(format!(
            "{} mutating fixture has no authoritative check contract",
            case.tool
        ));
    }
    let config = write_pkl_config(
        &workspace.project,
        &case.tool,
        &case.pkl_property,
        contract.as_ref(),
    )?;
    let resolved_contract = contract
        .as_ref()
        .map(|contract| resolve_real_tool_contract(case, contract, &config, &workspace.project))
        .transpose()?;
    let resolved_mutation = match (contract.as_ref(), mutation.as_ref()) {
        (Some(contract), Some(mutation)) => Some(resolve_mutating_tool_contract(
            case,
            contract,
            mutation,
            &config,
            &workspace.project,
        )?),
        _ => None,
    };
    let input = build_fixture_input(case, surface, &workspace.project, contract.as_ref())?;
    std::fs::write(workspace.evidence.join("input.json"), input.bytes())
        .map_err(|error| format!("write input evidence: {error}"))?;

    let tool_trace = resolved_contract
        .as_ref()
        .map(|contract| {
            let mut programs = contract
                .trace_invocations
                .iter()
                .map(|invocation| invocation.program.clone())
                .collect::<BTreeSet<_>>();
            // A fail-closed adapter may reject before its expected child runs;
            // keep the shim armed so zero invocations is positive evidence.
            programs.insert(contract.trace_program.clone());
            ToolTraceHarness::prepare(case, workspace, &programs)
        })
        .transpose()?;
    let before = contract
        .as_ref()
        .map(|_| TreeSnapshot::read(&workspace.project))
        .transpose()?;
    if let Some(before) = &before {
        write_json(
            &workspace.evidence.join("workspace-before.json"),
            &before.as_json(),
        )?;
    }

    if let (Some(contract), Some(mutation), Some(resolved), Some(resolved_mutation)) = (
        contract.as_ref(),
        mutation.as_ref(),
        resolved_contract.as_ref(),
        resolved_mutation.as_ref(),
    ) {
        return run_mutating_fixture_case_inner(
            case,
            surface,
            timeout,
            workspace,
            &config,
            &input,
            tool_trace
                .as_ref()
                .expect("mutating real-tool contract has trace harness"),
            contract,
            mutation,
            resolved,
            resolved_mutation,
            before
                .as_ref()
                .expect("mutating real-tool contract has before snapshot"),
        );
    }

    let binary = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary);
    command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(&config)
        .arg("post-tool-immediate");
    input.configure_command(&mut command);
    if let Some(trace) = &tool_trace {
        trace.configure(&mut command, "immediate-1")?;
    }
    let output = run_with_timeout(&mut command, input.bytes(), timeout, &workspace.evidence)
        .map_err(|error| format!("run {binary} for {surface}: {error}"))?;
    std::fs::write(
        workspace.evidence.join("exit.txt"),
        format!("{}\n", output.status.code().unwrap_or(-1)),
    )
    .map_err(|error| format!("write exit evidence: {error}"))?;
    verify_outputs(case, surface, &workspace.project, &output)?;

    let Some(contract) = contract.as_ref() else {
        return Ok(());
    };
    let resolved = resolved_contract
        .as_ref()
        .expect("real-tool contract was resolved");
    let trace = tool_trace
        .as_ref()
        .expect("real-tool contract has trace harness");
    verify_tool_trace(
        trace,
        "immediate-1",
        resolved,
        &workspace.project,
        &workspace.evidence.join("immediate-1-trace.json"),
    )?;
    let after_first = TreeSnapshot::read(&workspace.project)?;
    let first_diff = before
        .as_ref()
        .expect("real-tool contract has before snapshot")
        .diff(&after_first);
    verify_first_workspace_diff(case, contract, &first_diff)?;
    write_json(
        &workspace.evidence.join("workspace-after-immediate-1.json"),
        &after_first.as_json(),
    )?;
    write_json(
        &workspace.evidence.join("workspace-immediate-1-diff.json"),
        &first_diff.as_json(),
    )?;
    verify_immediate_artifact(case, contract, &workspace.project)?;

    let repeat_dir = workspace.evidence.join("immediate-repeat");
    let mut repeat_command = Command::new(binary);
    repeat_command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(&config)
        .arg("post-tool-immediate");
    input.configure_command(&mut repeat_command);
    trace.configure(&mut repeat_command, "immediate-2")?;
    let repeated = run_with_timeout(&mut repeat_command, input.bytes(), timeout, &repeat_dir)
        .map_err(|error| format!("repeat {binary} for {surface}: {error}"))?;
    std::fs::write(
        repeat_dir.join("exit.txt"),
        format!("{}\n", repeated.status.code().unwrap_or(-1)),
    )
    .map_err(|error| format!("write repeated exit evidence: {error}"))?;
    verify_repeated_output(&case.tool, &output, &repeated, &workspace.project)?;
    verify_tool_trace(
        trace,
        "immediate-2",
        resolved,
        &workspace.project,
        &workspace.evidence.join("immediate-2-trace.json"),
    )?;
    let after_second = TreeSnapshot::read(&workspace.project)?;
    let repeat_diff = after_first.diff(&after_second);
    if !repeat_diff.is_empty() {
        return Err(format!(
            "{} immediate repeat was not idempotent: {}",
            case.tool,
            repeat_diff.describe()
        ));
    }
    write_json(
        &workspace.evidence.join("workspace-immediate-2-diff.json"),
        &repeat_diff.as_json(),
    )?;

    let mut deferred_contract = None;
    for attempt in 1..=2 {
        let observed = run_deferred_attempt(
            case,
            surface,
            timeout,
            workspace,
            &config,
            &input,
            trace,
            contract,
            resolved,
            attempt,
            &after_second,
        )?;
        if let Some(expected) = &deferred_contract {
            if expected != &observed {
                return Err(format!(
                    "{} deferred repeat changed its semantic evidence\nfirst:\n{}\nsecond:\n{}",
                    case.tool,
                    serde_json::to_string_pretty(expected)
                        .unwrap_or_else(|_| format!("{expected:?}")),
                    serde_json::to_string_pretty(&observed)
                        .unwrap_or_else(|_| format!("{observed:?}")),
                ));
            }
        }
        deferred_contract = Some(observed);
    }
    write_json(
        &workspace.evidence.join("deferred-idempotence.json"),
        &serde_json::json!({
            "formatVersion": 1,
            "attempts": 2,
            "equal": true,
            "contract": deferred_contract,
        }),
    )?;
    Ok(())
}

#[derive(Debug)]
struct DeferredPhaseExpectation<'a> {
    phase: &'static str,
    resolved: &'a ResolvedContract,
    assert_diagnostics: bool,
}

#[derive(Debug)]
struct DeferredAttemptExpectation<'a> {
    phases: Vec<DeferredPhaseExpectation<'a>>,
    outcome: ExpectedOutcome,
    initial_outcome: ExpectedOutcome,
    final_outcome: Option<ExpectedOutcome>,
    fix_attempted: bool,
    changed_targets: &'a [&'static str],
}

#[allow(clippy::too_many_arguments)]
fn run_mutating_fixture_case_inner(
    case: &FixtureCase,
    surface: ProtocolSurface,
    timeout: Duration,
    workspace: &FixtureWorkspace,
    config: &Path,
    input: &support::native_events::NativePostToolInput,
    trace: &ToolTraceHarness,
    contract: &RealToolContractCase,
    mutation: &MutatingToolContractCase,
    resolved: &ResolvedContract,
    resolved_mutation: &ResolvedMutatingContract,
    pristine: &TreeSnapshot,
) -> Result<(), String> {
    validate_mutation_expected_tree(case, mutation)?;
    let phase_trace = |immediate: &ResolvedContract| {
        let final_check = (!resolved_mutation.explicit_workflow
            || resolved_mutation.immediate_includes_final_check)
            .then_some(&resolved_mutation.final_check)
            .into_iter()
            .flat_map(|phase| phase.iter())
            .flat_map(|phase| phase.trace_invocations.iter());
        immediate
            .trace_invocations
            .iter()
            .chain(final_check)
            .cloned()
            .collect::<Vec<_>>()
    };
    let immediate_trace = phase_trace(&resolved_mutation.immediate);
    let repeat_trace = phase_trace(
        resolved_mutation
            .repeat_immediate
            .as_ref()
            .unwrap_or(&resolved_mutation.immediate),
    );
    let binary = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary);
    command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(config)
        .arg("post-tool-immediate");
    input.configure_command(&mut command);
    trace.configure(&mut command, "immediate-1")?;
    let output = run_with_timeout(&mut command, input.bytes(), timeout, &workspace.evidence)
        .map_err(|error| format!("run {binary} for {surface}: {error}"))?;
    std::fs::write(
        workspace.evidence.join("exit.txt"),
        format!("{}\n", output.status.code().unwrap_or(-1)),
    )
    .map_err(|error| format!("write exit evidence: {error}"))?;
    verify_outputs(case, surface, &workspace.project, &output)?;
    verify_tool_trace_invocations(
        trace,
        "immediate-1",
        &immediate_trace,
        &workspace.project,
        &workspace.evidence.join("immediate-1-trace.json"),
    )?;
    let after_first = TreeSnapshot::read(&workspace.project)?;
    let first_diff = pristine.diff(&after_first);
    verify_mutating_workspace_diff(case, mutation, mutation.immediate_outcome, &first_diff)?;
    write_json(
        &workspace.evidence.join("workspace-after-immediate-1.json"),
        &after_first.as_json(),
    )?;
    write_json(
        &workspace.evidence.join("workspace-immediate-1-diff.json"),
        &first_diff.as_json(),
    )?;
    verify_mutating_immediate_artifact(case, contract, mutation, &workspace.project)?;

    let repeat_dir = workspace.evidence.join("immediate-repeat");
    let mut repeat_command = Command::new(binary);
    repeat_command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(config)
        .arg("post-tool-immediate");
    input.configure_command(&mut repeat_command);
    trace.configure(&mut repeat_command, "immediate-2")?;
    let repeated = run_with_timeout(&mut repeat_command, input.bytes(), timeout, &repeat_dir)
        .map_err(|error| format!("repeat {binary} for {surface}: {error}"))?;
    std::fs::write(
        repeat_dir.join("exit.txt"),
        format!("{}\n", repeated.status.code().unwrap_or(-1)),
    )
    .map_err(|error| format!("write repeated exit evidence: {error}"))?;
    if mutation.changed_targets.is_empty() {
        verify_repeated_output(&case.tool, &output, &repeated, &workspace.project)?;
    } else {
        verify_idempotent_immediate_output(case, surface, &repeated, &workspace.project)?;
    }
    verify_tool_trace_invocations(
        trace,
        "immediate-2",
        &repeat_trace,
        &workspace.project,
        &workspace.evidence.join("immediate-2-trace.json"),
    )?;
    let after_second = TreeSnapshot::read(&workspace.project)?;
    let repeat_diff = after_first.diff(&after_second);
    if !repeat_diff.is_empty() {
        return Err(format!(
            "{} immediate repeat was not idempotent: {}",
            case.tool,
            repeat_diff.describe()
        ));
    }
    write_json(
        &workspace.evidence.join("workspace-immediate-2-diff.json"),
        &repeat_diff.as_json(),
    )?;

    pristine.restore(&workspace.project)?;
    let restored = TreeSnapshot::read(&workspace.project)?;
    if &restored != pristine {
        return Err(format!(
            "{} could not restore a pristine deferred baseline",
            case.tool
        ));
    }
    write_json(
        &workspace.evidence.join("workspace-deferred-pristine.json"),
        &restored.as_json(),
    )?;

    let initial_outcome = aggregate_resolved_outcome(&resolved.invocations);
    let mut first_phases = vec![DeferredPhaseExpectation {
        phase: "initial-check",
        resolved,
        assert_diagnostics: initial_outcome != ExpectedOutcome::Clean,
    }];
    let first_fix_attempted = initial_outcome == ExpectedOutcome::Issues;
    let first_final_outcome = if first_fix_attempted {
        first_phases.push(DeferredPhaseExpectation {
            phase: "remedy",
            resolved: &resolved_mutation.remedy,
            assert_diagnostics: false,
        });
        let final_check = resolved_mutation.final_check.as_ref().ok_or_else(|| {
            format!(
                "{} fixable contract omitted its authoritative final check",
                case.tool
            )
        })?;
        first_phases.push(DeferredPhaseExpectation {
            phase: "final-check",
            resolved: final_check,
            assert_diagnostics: false,
        });
        Some(aggregate_resolved_outcome(&final_check.invocations))
    } else if initial_outcome == ExpectedOutcome::Clean {
        Some(ExpectedOutcome::Clean)
    } else {
        None
    };
    let first_outcome = first_final_outcome.unwrap_or(initial_outcome);
    let first_expectation = DeferredAttemptExpectation {
        phases: first_phases,
        outcome: first_outcome,
        initial_outcome,
        final_outcome: first_final_outcome,
        fix_attempted: first_fix_attempted,
        changed_targets: if first_fix_attempted {
            mutation.changed_targets
        } else {
            &[]
        },
    };
    let first_semantic = run_mutating_deferred_attempt(
        case,
        surface,
        timeout,
        workspace,
        config,
        input,
        trace,
        contract,
        mutation,
        1,
        &restored,
        &first_expectation,
    )?;
    let after_deferred_first = TreeSnapshot::read(&workspace.project)?;

    let repaired = first_fix_attempted && first_final_outcome == Some(ExpectedOutcome::Clean);
    let second_expectation = if repaired {
        let second_resolved = resolved_mutation
            .final_check
            .as_ref()
            .ok_or_else(|| format!("{} fixed-state contract omitted checker", case.tool))?;
        let second_initial_outcome = aggregate_resolved_outcome(&second_resolved.invocations);
        DeferredAttemptExpectation {
            phases: vec![DeferredPhaseExpectation {
                phase: "initial-check",
                resolved: second_resolved,
                assert_diagnostics: false,
            }],
            outcome: second_initial_outcome,
            initial_outcome: second_initial_outcome,
            final_outcome: Some(second_initial_outcome),
            fix_attempted: false,
            changed_targets: &[],
        }
    } else if first_fix_attempted {
        let final_check = resolved_mutation
            .final_check
            .as_ref()
            .ok_or_else(|| format!("{} repeated manual-fix contract omitted checker", case.tool))?;
        let final_outcome = aggregate_resolved_outcome(&final_check.invocations);
        DeferredAttemptExpectation {
            phases: vec![
                DeferredPhaseExpectation {
                    phase: "initial-check",
                    resolved,
                    assert_diagnostics: true,
                },
                DeferredPhaseExpectation {
                    phase: "remedy",
                    resolved: &resolved_mutation.remedy,
                    assert_diagnostics: false,
                },
                DeferredPhaseExpectation {
                    phase: "final-check",
                    resolved: final_check,
                    assert_diagnostics: false,
                },
            ],
            outcome: final_outcome,
            initial_outcome,
            final_outcome: Some(final_outcome),
            fix_attempted: true,
            changed_targets: mutation.changed_targets,
        }
    } else {
        DeferredAttemptExpectation {
            phases: vec![DeferredPhaseExpectation {
                phase: "initial-check",
                resolved,
                assert_diagnostics: initial_outcome != ExpectedOutcome::Clean,
            }],
            outcome: initial_outcome,
            initial_outcome,
            final_outcome: (initial_outcome == ExpectedOutcome::Clean)
                .then_some(ExpectedOutcome::Clean),
            fix_attempted: false,
            changed_targets: &[],
        }
    };
    let second_semantic = run_mutating_deferred_attempt(
        case,
        surface,
        timeout,
        workspace,
        config,
        input,
        trace,
        contract,
        mutation,
        2,
        &after_deferred_first,
        &second_expectation,
    )?;
    let expect_equal_semantics = mutation.changed_targets.is_empty();
    if expect_equal_semantics && first_semantic != second_semantic {
        return Err(format!(
            "{} unchanged deferred repeat changed semantic evidence\nfirst:\n{}\nsecond:\n{}",
            case.tool,
            serde_json::to_string_pretty(&first_semantic)
                .unwrap_or_else(|_| format!("{first_semantic:?}")),
            serde_json::to_string_pretty(&second_semantic)
                .unwrap_or_else(|_| format!("{second_semantic:?}")),
        ));
    }
    write_json(
        &workspace.evidence.join("deferred-idempotence.json"),
        &serde_json::json!({
            "formatVersion": 1,
            "attempts": 2,
            "first": first_semantic,
            "second": second_semantic,
            "equal": expect_equal_semantics,
            "secondChangedFiles": [],
            "secondFixAttempted": second_expectation.fix_attempted,
        }),
    )?;
    Ok(())
}

fn build_fixture_input(
    case: &FixtureCase,
    surface: ProtocolSurface,
    project: &Path,
    contract: Option<&RealToolContractCase>,
) -> Result<support::native_events::NativePostToolInput, String> {
    let event_entry = contract
        .filter(|_| case.tool == "ghalint-workflow")
        .and_then(|contract| {
            contract
                .invocations
                .iter()
                .flat_map(|invocation| invocation.targets.iter())
                .next()
        })
        .map(PathBuf::from)
        .unwrap_or_else(|| case.entry.clone());
    let mut builder = PostToolUseBuilder::new(surface, project, event_entry).identity(
        "test-session",
        "test-turn",
        format!("{}-tool", case.tool),
    );
    if let Some(contract) = contract {
        let targets = contract.targets();
        for relative in &targets {
            let target = project.join(relative);
            if !target.is_file() {
                return Err(format!(
                    "{} contract target is not a fixture file: {target:?}",
                    case.tool
                ));
            }
        }
        if targets.len() > 1 {
            let mut patch = String::from("*** Begin Patch\n");
            for relative in targets {
                patch.push_str(&format!("*** Update File: {relative}\n@@\n"));
            }
            patch.push_str("*** End Patch\n");
            builder = builder.tool(
                "apply_patch",
                serde_json::json!({"patch": patch}),
                serde_json::json!({"exit_code": 0}),
            );
        }
    }
    builder.build()
}

#[derive(Debug, Clone)]
struct ResolvedContract {
    outer_program: String,
    trace_program: String,
    invocations: Vec<ResolvedInvocation>,
    trace_invocations: Vec<ResolvedTraceInvocation>,
}

#[derive(Debug, Clone)]
struct ResolvedInvocation {
    targets: Vec<PathBuf>,
    arguments: Vec<String>,
    exit_code: i32,
    outcome: ExpectedOutcome,
}

#[derive(Debug, Clone)]
struct ResolvedTraceInvocation {
    program: String,
    targets: Vec<PathBuf>,
    arguments: Vec<String>,
    exit_code: i32,
}

#[derive(Debug)]
struct ResolvedMutatingContract {
    immediate: ResolvedContract,
    repeat_immediate: Option<ResolvedContract>,
    remedy: ResolvedContract,
    final_check: Option<ResolvedContract>,
    explicit_workflow: bool,
    immediate_includes_final_check: bool,
}

fn resolve_real_tool_contract(
    case: &FixtureCase,
    contract: &RealToolContractCase,
    config: &Path,
    project: &Path,
) -> Result<ResolvedContract, String> {
    let loaded = hookkit_pkl_config::load_explicit(config, project)
        .map_err(|error| format!("reload evaluated fixture config {config:?}: {error}"))?;
    if loaded.config.run != vec![case.tool.clone()] {
        return Err(format!(
            "{} contract config run list drifted: {:?}",
            case.tool, loaded.config.run
        ));
    }
    let spec = loaded
        .config
        .tools
        .get(&case.tool)
        .ok_or_else(|| format!("evaluated fixture config omitted tool {}", case.tool))?;
    if !spec.workflows.is_empty() {
        if spec.workflow_order != vec![contract.phase_id.to_owned()] {
            return Err(format!(
                "{} explicit workflow order mismatch: expected {:?}, got {:?}",
                case.tool,
                [contract.phase_id],
                spec.workflow_order
            ));
        }
        let workflow = spec.workflows.get(contract.phase_id).ok_or_else(|| {
            format!(
                "{} explicit workflow {:?} is absent from evaluated config",
                case.tool, contract.phase_id
            )
        })?;
        let expected_shape = if case.tool == "ghalint-workflow" {
            workflow.check_scope == CheckScope::Workspace
                && workflow.invocation == InvocationGranularity::Workspace
        } else {
            workflow.check_scope == CheckScope::TargetFiles
                && workflow.invocation == InvocationGranularity::Batch
        };
        if !workflow.enabled || !expected_shape {
            return Err(format!(
                "{} explicit workflow {:?} has the wrong enabled/scope/invocation contract; got enabled={} scope={:?} invocation={:?}",
                case.tool,
                contract.phase_id,
                workflow.enabled,
                workflow.check_scope,
                workflow.invocation
            ));
        }
        let check = workflow.check.as_ref().ok_or_else(|| {
            format!(
                "{} explicit workflow {:?} omitted its checker",
                case.tool, contract.phase_id
            )
        })?;
        if check.writes != WriteBehavior::None || !check.issues_on_stdout {
            return Err(format!(
                "{} explicit workflow {:?} checker must be read-only and stdout-signaled; got writes={:?} issuesOnStdout={}",
                case.tool, contract.phase_id, check.writes, check.issues_on_stdout
            ));
        }
        let expected_extra_args = contract
            .extra_args
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect::<Vec<_>>();
        if check.extra_args != expected_extra_args {
            return Err(format!(
                "{} explicit checker extra args mismatch: expected {:?}, evaluated {:?}",
                case.tool, expected_extra_args, check.extra_args
            ));
        }
        return resolve_workflow_invocations(
            case,
            spec,
            check,
            workflow.invocation,
            contract.invocations,
            contract.outcome,
            contract.trace_plan,
            project,
            "explicit checker",
        );
    }
    let phase = spec.phases.get(contract.phase_id).ok_or_else(|| {
        format!(
            "{} contract phase {:?} is absent from evaluated config",
            case.tool, contract.phase_id
        )
    })?;
    if !phase.enabled || !matches!(phase.mode, PhaseMode::Verify | PhaseMode::CheckOnly) {
        return Err(format!(
            "{} contract phase {:?} is not an enabled checker",
            case.tool, contract.phase_id
        ));
    }
    if phase.writes != WriteBehavior::None {
        return Err(format!(
            "{} contract phase {:?} declares writes {:?}",
            case.tool, contract.phase_id, phase.writes
        ));
    }
    let expected_extra_args = contract
        .extra_args
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect::<Vec<_>>();
    if phase.extra_args != expected_extra_args {
        return Err(format!(
            "{} contract extra args mismatch: expected {:?}, evaluated {:?}",
            case.tool, expected_extra_args, phase.extra_args
        ));
    }
    match spec.phase_invocation {
        InvocationGranularity::PerFile
            if contract
                .invocations
                .iter()
                .all(|invocation| invocation.targets.len() == 1) => {}
        InvocationGranularity::Batch if contract.invocations.len() == 1 => {}
        InvocationGranularity::Workspace if contract.invocations.len() == 1 => {}
        actual => {
            return Err(format!(
                "{} contract invocation groups do not match evaluated granularity {actual:?}",
                case.tool
            ));
        }
    }

    let outer_program = phase
        .program
        .clone()
        .unwrap_or_else(|| spec.executable.clone());
    let mut invocations = Vec::with_capacity(contract.invocations.len());
    let mut trace_program = None;
    let mut trace_invocations = Vec::new();
    for invocation in contract.invocations {
        let targets = invocation
            .targets
            .iter()
            .map(|relative| canonical_project(&project.join(relative)))
            .collect::<Vec<_>>();
        let arguments = render_expected_arguments(spec, phase, project, &targets)?;
        let outcome = classify_expected_exit(phase, invocation.exit_code);
        let (invocation_trace_program, mut invocation_traces) = resolve_trace_invocations(
            contract.trace_plan,
            &outer_program,
            &arguments,
            &targets,
            invocation.trace_exit_codes,
        )?;
        if let Some(expected) = &trace_program {
            if expected != &invocation_trace_program {
                return Err(format!(
                    "{} contract trace program changed between invocation groups",
                    case.tool
                ));
            }
        } else {
            trace_program = Some(invocation_trace_program);
        }
        trace_invocations.append(&mut invocation_traces);
        invocations.push(ResolvedInvocation {
            targets,
            arguments,
            exit_code: invocation.exit_code,
            outcome,
        });
    }
    let aggregate = if invocations
        .iter()
        .any(|invocation| invocation.outcome == ExpectedOutcome::OperationalFailure)
    {
        ExpectedOutcome::OperationalFailure
    } else if invocations
        .iter()
        .any(|invocation| invocation.outcome == ExpectedOutcome::Issues)
    {
        ExpectedOutcome::Issues
    } else {
        ExpectedOutcome::Clean
    };
    if aggregate != contract.outcome {
        return Err(format!(
            "{} contract expected {:?}, but evaluated exit policy classifies its invocations as {aggregate:?}",
            case.tool, contract.outcome
        ));
    }
    Ok(ResolvedContract {
        outer_program,
        trace_program: trace_program
            .ok_or_else(|| format!("{} contract has no trace program", case.tool))?,
        invocations,
        trace_invocations,
    })
}

fn resolve_mutating_tool_contract(
    case: &FixtureCase,
    contract: &RealToolContractCase,
    mutation: &MutatingToolContractCase,
    config: &Path,
    project: &Path,
) -> Result<ResolvedMutatingContract, String> {
    let loaded = hookkit_pkl_config::load_explicit(config, project)
        .map_err(|error| format!("reload evaluated fixture config {config:?}: {error}"))?;
    let spec = loaded
        .config
        .tools
        .get(&case.tool)
        .ok_or_else(|| format!("evaluated fixture config omitted tool {}", case.tool))?;
    let immediate_phase = spec.phases.get(mutation.remedy_phase_id).ok_or_else(|| {
        format!(
            "{} mutating contract immediate phase {:?} is absent from evaluated config",
            case.tool, mutation.remedy_phase_id
        )
    })?;
    if !immediate_phase.enabled || immediate_phase.mode != mutation.remedy_mode {
        return Err(format!(
            "{} mutating contract immediate phase {:?} expected enabled {:?} mode, got enabled={} mode={:?}",
            case.tool,
            mutation.remedy_phase_id,
            mutation.remedy_mode,
            immediate_phase.enabled,
            immediate_phase.mode
        ));
    }
    if immediate_phase.writes != mutation.remedy_writes {
        return Err(format!(
            "{} mutating contract immediate phase {:?} expected {:?} writes, got {:?}",
            case.tool, mutation.remedy_phase_id, mutation.remedy_writes, immediate_phase.writes
        ));
    }
    if mutation.remedy_writes == WriteBehavior::None {
        return Err(format!(
            "{} mutating contract remedy {:?} must declare a non-none write scope",
            case.tool, mutation.remedy_phase_id
        ));
    }
    if matches!(
        mutation.remedy_writes,
        WriteBehavior::MatchingGlobs | WriteBehavior::Workspace
    ) && mutation.remedy_invocations.len() != 1
    {
        return Err(format!(
            "{} mutating contract remedy {:?} uses {:?} writes across {} invocation groups; declare per-invocation changed paths before extending this harness shape",
            case.tool,
            mutation.remedy_phase_id,
            mutation.remedy_writes,
            mutation.remedy_invocations.len()
        ));
    }
    let expected_extra_args = contract
        .extra_args
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect::<Vec<_>>();
    if immediate_phase.extra_args != expected_extra_args {
        return Err(format!(
            "{} immediate phase extra args mismatch: expected {:?}, evaluated {:?}",
            case.tool, expected_extra_args, immediate_phase.extra_args
        ));
    }
    let immediate = resolve_phase_invocations(
        case,
        spec,
        immediate_phase,
        mutation.remedy_invocations,
        contract.trace_plan,
        project,
    )?;
    let repeat_immediate = mutation
        .repeat_remedy_invocations
        .map(|invocations| {
            resolve_phase_invocations(
                case,
                spec,
                immediate_phase,
                invocations,
                contract.trace_plan,
                project,
            )
        })
        .transpose()?;
    if repeat_immediate.is_some() && mutation.changed_targets.is_empty() {
        return Err(format!(
            "{} declares a fixed-state repeat remedy without any expected mutations",
            case.tool
        ));
    }

    let explicit_workflow = !spec.workflows.is_empty();
    let mut immediate_includes_final_check = false;
    let (remedy, final_check) = if explicit_workflow {
        let workflow = spec.workflows.get(contract.phase_id).ok_or_else(|| {
            format!(
                "{} mutating contract explicit workflow {:?} is absent from evaluated config",
                case.tool, contract.phase_id
            )
        })?;
        if !workflow.enabled
            || workflow.check_scope != CheckScope::TargetFiles
            || workflow.invocation != InvocationGranularity::Batch
        {
            return Err(format!(
                "{} mutating explicit workflow {:?} must be enabled, target-files scoped, and batch invoked",
                case.tool, contract.phase_id
            ));
        }
        let remedy_command = workflow.remedy.as_ref().ok_or_else(|| {
            format!(
                "{} mutating explicit workflow {:?} omitted its remedy",
                case.tool, contract.phase_id
            )
        })?;
        if remedy_command.writes != mutation.remedy_writes
            || remedy_command.issues_on_stdout
            || remedy_command.extra_args != expected_extra_args
        {
            return Err(format!(
                "{} mutating explicit remedy {:?} drifted: writes={:?} issuesOnStdout={} extraArgs={:?}",
                case.tool,
                contract.phase_id,
                remedy_command.writes,
                remedy_command.issues_on_stdout,
                remedy_command.extra_args
            ));
        }
        let remedy = resolve_workflow_invocations(
            case,
            spec,
            remedy_command,
            workflow.invocation,
            mutation.remedy_invocations,
            mutation.immediate_outcome,
            contract.trace_plan,
            project,
            "explicit remedy",
        )?;
        let final_check = if mutation.final_invocations.is_empty() {
            None
        } else {
            let check = workflow.check.as_ref().ok_or_else(|| {
                format!(
                    "{} mutating explicit workflow {:?} omitted its final checker",
                    case.tool, contract.phase_id
                )
            })?;
            if check.writes != WriteBehavior::None
                || !check.issues_on_stdout
                || check.extra_args != expected_extra_args
            {
                return Err(format!(
                    "{} mutating explicit checker {:?} drifted: writes={:?} issuesOnStdout={} extraArgs={:?}",
                    case.tool,
                    contract.phase_id,
                    check.writes,
                    check.issues_on_stdout,
                    check.extra_args
                ));
            }
            let matching_immediate_checks = spec
                .phase_order
                .iter()
                .filter_map(|phase_id| {
                    spec.phases
                        .get(phase_id)
                        .filter(|phase| phase_matches_workflow_check(phase, check))
                        .map(|_| phase_id)
                })
                .collect::<Vec<_>>();
            if matching_immediate_checks.len() > 1 {
                return Err(format!(
                    "{} explicit workflow checker is duplicated in immediate phase order: {:?}",
                    case.tool, matching_immediate_checks
                ));
            }
            immediate_includes_final_check = !matching_immediate_checks.is_empty();
            Some(resolve_workflow_invocations(
                case,
                spec,
                check,
                workflow.invocation,
                mutation.final_invocations,
                mutation.immediate_outcome,
                contract.trace_plan,
                project,
                "explicit final checker",
            )?)
        };
        (remedy, final_check)
    } else {
        let final_check = if mutation.final_invocations.is_empty() {
            None
        } else {
            let phase = spec.phases.get(contract.phase_id).ok_or_else(|| {
                format!(
                    "{} mutating contract final phase {:?} is absent from evaluated config",
                    case.tool, contract.phase_id
                )
            })?;
            Some(resolve_phase_invocations(
                case,
                spec,
                phase,
                mutation.final_invocations,
                contract.trace_plan,
                project,
            )?)
        };
        (immediate.clone(), final_check)
    };

    for resolved in std::iter::once(&immediate)
        .chain(repeat_immediate.iter())
        .chain(std::iter::once(&remedy))
        .chain(final_check.iter())
    {
        if resolved.trace_program
            != final_check
                .as_ref()
                .map_or(remedy.trace_program.as_str(), |final_check| {
                    final_check.trace_program.as_str()
                })
        {
            return Err(format!(
                "{} mutating contract changes its traced child program between phases",
                case.tool
            ));
        }
    }
    let aggregate = if explicit_workflow {
        aggregate_resolved_outcome(&immediate.invocations)
    } else {
        aggregate_resolved_outcome(
            final_check
                .as_ref()
                .map_or(immediate.invocations.as_slice(), |final_check| {
                    final_check.invocations.as_slice()
                }),
        )
    };
    if aggregate != mutation.immediate_outcome {
        return Err(format!(
            "{} mutating contract expected immediate {:?}, but final executed phase classifies as {aggregate:?}",
            case.tool, mutation.immediate_outcome
        ));
    }
    let targets = contract.targets().into_iter().collect::<BTreeSet<_>>();
    let changed = mutation
        .changed_targets
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if mutation.remedy_writes == WriteBehavior::TargetFiles && !changed.is_subset(&targets) {
        return Err(format!(
            "{} mutating contract changed targets {changed:?} outside candidates {targets:?}",
            case.tool
        ));
    }
    Ok(ResolvedMutatingContract {
        immediate,
        repeat_immediate,
        remedy,
        final_check,
        explicit_workflow,
        immediate_includes_final_check,
    })
}

fn phase_matches_workflow_check(phase: &Phase, check: &WorkflowCommand) -> bool {
    phase.enabled
        && matches!(phase.mode, PhaseMode::Verify | PhaseMode::CheckOnly)
        && phase.program == check.program
        && phase.argv == check.argv
        && phase.exit_codes == check.exit_codes
        && phase.writes == check.writes
        && phase.extra_args == check.extra_args
}

fn resolve_phase_invocations(
    case: &FixtureCase,
    spec: &ToolSpec,
    phase: &Phase,
    expected_invocations: &[ExpectedInvocation],
    trace_plan: TracePlan,
    project: &Path,
) -> Result<ResolvedContract, String> {
    match spec.phase_invocation {
        InvocationGranularity::PerFile
            if expected_invocations
                .iter()
                .all(|invocation| invocation.targets.len() == 1) => {}
        InvocationGranularity::Batch if expected_invocations.len() == 1 => {}
        InvocationGranularity::Workspace if expected_invocations.len() == 1 => {}
        actual => {
            return Err(format!(
                "{} phase contract invocation groups do not match evaluated granularity {actual:?}",
                case.tool
            ));
        }
    }
    let outer_program = phase
        .program
        .clone()
        .unwrap_or_else(|| spec.executable.clone());
    let mut invocations = Vec::with_capacity(expected_invocations.len());
    let mut trace_program = None;
    let mut trace_invocations = Vec::new();
    for invocation in expected_invocations {
        let targets = invocation
            .targets
            .iter()
            .map(|relative| canonical_project(&project.join(relative)))
            .collect::<Vec<_>>();
        let arguments = render_expected_arguments(spec, phase, project, &targets)?;
        let outcome = classify_expected_exit(phase, invocation.exit_code);
        let (invocation_trace_program, mut invocation_traces) = resolve_trace_invocations(
            trace_plan,
            &outer_program,
            &arguments,
            &targets,
            invocation.trace_exit_codes,
        )?;
        if let Some(expected) = &trace_program {
            if expected != &invocation_trace_program {
                return Err(format!(
                    "{} phase contract trace program changed between invocation groups",
                    case.tool
                ));
            }
        } else {
            trace_program = Some(invocation_trace_program);
        }
        trace_invocations.append(&mut invocation_traces);
        invocations.push(ResolvedInvocation {
            targets,
            arguments,
            exit_code: invocation.exit_code,
            outcome,
        });
    }
    Ok(ResolvedContract {
        outer_program,
        trace_program: trace_program
            .ok_or_else(|| format!("{} phase contract has no trace program", case.tool))?,
        invocations,
        trace_invocations,
    })
}

#[allow(clippy::too_many_arguments)]
fn resolve_workflow_invocations(
    case: &FixtureCase,
    spec: &ToolSpec,
    command: &WorkflowCommand,
    invocation_granularity: InvocationGranularity,
    expected_invocations: &[ExpectedInvocation],
    expected_outcome: ExpectedOutcome,
    trace_plan: TracePlan,
    project: &Path,
    context: &str,
) -> Result<ResolvedContract, String> {
    match invocation_granularity {
        InvocationGranularity::PerFile
            if expected_invocations
                .iter()
                .all(|invocation| invocation.targets.len() == 1) => {}
        InvocationGranularity::Batch if expected_invocations.len() == 1 => {}
        InvocationGranularity::Workspace if expected_invocations.len() == 1 => {}
        actual => {
            return Err(format!(
                "{} {context} invocation groups do not match evaluated granularity {actual:?}",
                case.tool
            ));
        }
    }
    if command.issues_on_stdout
        && expected_outcome == ExpectedOutcome::Issues
        && expected_invocations.len() != 1
    {
        return Err(format!(
            "{} {context} stdout-signaled issue expectation is ambiguous across {} invocation groups",
            case.tool,
            expected_invocations.len()
        ));
    }
    let outer_program = command
        .program
        .clone()
        .unwrap_or_else(|| spec.executable.clone());
    let mut invocations = Vec::with_capacity(expected_invocations.len());
    let mut trace_program = None;
    let mut trace_invocations = Vec::new();
    for invocation in expected_invocations {
        let targets = invocation
            .targets
            .iter()
            .map(|relative| canonical_project(&project.join(relative)))
            .collect::<Vec<_>>();
        let arguments = render_expected_workflow_arguments(spec, command, project, &targets)?;
        let mut outcome = classify_expected_exit_codes(&command.exit_codes, invocation.exit_code);
        if command.issues_on_stdout
            && expected_outcome == ExpectedOutcome::Issues
            && outcome == ExpectedOutcome::Clean
        {
            outcome = ExpectedOutcome::Issues;
        }
        let (invocation_trace_program, mut invocation_traces) = resolve_trace_invocations(
            trace_plan,
            &outer_program,
            &arguments,
            &targets,
            invocation.trace_exit_codes,
        )?;
        if let Some(expected) = &trace_program {
            if expected != &invocation_trace_program {
                return Err(format!(
                    "{} {context} trace program changed between invocation groups",
                    case.tool
                ));
            }
        } else {
            trace_program = Some(invocation_trace_program);
        }
        trace_invocations.append(&mut invocation_traces);
        invocations.push(ResolvedInvocation {
            targets,
            arguments,
            exit_code: invocation.exit_code,
            outcome,
        });
    }
    let aggregate = aggregate_resolved_outcome(&invocations);
    if aggregate != expected_outcome {
        return Err(format!(
            "{} {context} expected {expected_outcome:?}, but evaluated exit/stdout policy classifies its invocations as {aggregate:?}",
            case.tool
        ));
    }
    Ok(ResolvedContract {
        outer_program,
        trace_program: trace_program
            .ok_or_else(|| format!("{} {context} has no trace program", case.tool))?,
        invocations,
        trace_invocations,
    })
}

fn aggregate_resolved_outcome(invocations: &[ResolvedInvocation]) -> ExpectedOutcome {
    if invocations
        .iter()
        .any(|invocation| invocation.outcome == ExpectedOutcome::OperationalFailure)
    {
        ExpectedOutcome::OperationalFailure
    } else if invocations
        .iter()
        .any(|invocation| invocation.outcome == ExpectedOutcome::Issues)
    {
        ExpectedOutcome::Issues
    } else {
        ExpectedOutcome::Clean
    }
}

fn resolve_trace_invocations(
    plan: TracePlan,
    outer_program: &str,
    outer_arguments: &[String],
    targets: &[PathBuf],
    expected_exit_codes: &[i32],
) -> Result<(String, Vec<ResolvedTraceInvocation>), String> {
    match plan {
        TracePlan::Direct => {
            if expected_exit_codes.len() != 1 {
                return Err(format!(
                    "direct trace for {outer_program} must declare exactly one exit code"
                ));
            }
            Ok((
                outer_program.to_owned(),
                vec![ResolvedTraceInvocation {
                    program: outer_program.to_owned(),
                    targets: targets.to_vec(),
                    arguments: outer_arguments.to_vec(),
                    exit_code: expected_exit_codes[0],
                }],
            ))
        }
        TracePlan::PrivateProbeThenContextlintWorkspace => {
            if expected_exit_codes.len() != 2 {
                return Err(format!(
                    "Contextlint adapter trace for {outer_program} must declare private-probe and project exit codes, got {expected_exit_codes:?}"
                ));
            }
            let [
                isolated,
                command,
                adapter,
                node_program,
                tool_program,
                indicator,
                marker_and_files @ ..,
            ] = outer_arguments
            else {
                return Err(format!(
                    "Contextlint adapter trace has an incomplete outer command: {outer_arguments:?}"
                ));
            };
            if isolated != "-I" || command != "-c" || adapter.is_empty() || node_program != "node" {
                return Err(format!(
                    "Contextlint adapter trace expected isolated Python plus the logical node program, got {outer_arguments:?}"
                ));
            }
            let rendered_cli = PathBuf::from(tool_program);
            if !rendered_cli.is_absolute() {
                return Err(format!(
                    "Contextlint adapter trace expected an absolute managed CLI path, got {rendered_cli:?}"
                ));
            }
            let rendered_cli = rendered_cli.canonicalize().map_err(|error| {
                format!("canonicalize rendered Contextlint CLI path {rendered_cli:?}: {error}")
            })?;
            let expected_cli = resolve_contextlint_fixture_cli()?;
            if rendered_cli != expected_cli {
                return Err(format!(
                    "Contextlint adapter trace rendered CLI {rendered_cli:?}, expected managed CLI {expected_cli:?}"
                ));
            }
            let [marker, rendered_files @ ..] = marker_and_files else {
                return Err("Contextlint adapter trace has no file marker".to_owned());
            };
            if marker != CONTEXTLINT_FILES_MARKER {
                return Err(format!(
                    "Contextlint adapter trace rejects forwarded arguments before its file marker: {marker_and_files:?}"
                ));
            }
            let expected_files = targets
                .iter()
                .map(|target| target.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            if rendered_files != expected_files {
                return Err(format!(
                    "Contextlint adapter trace candidate suffix mismatch: expected {expected_files:?}, got {rendered_files:?}"
                ));
            }
            let indicator_path = PathBuf::from(indicator);
            if !indicator_path.is_absolute()
                || indicator_path.file_name() != Some(OsStr::new("contextlint.config.json"))
                || !indicator_path.is_file()
            {
                return Err(format!(
                    "Contextlint adapter trace rendered an invalid workspace indicator {indicator_path:?}"
                ));
            }
            let indicator_path = indicator_path.canonicalize().map_err(|error| {
                format!("canonicalize Contextlint workspace indicator {indicator_path:?}: {error}")
            })?;
            let workspace = indicator_path.parent().ok_or_else(|| {
                format!("Contextlint indicator has no workspace parent: {indicator_path:?}")
            })?;
            let outside_workspace = targets
                .iter()
                .filter(|target| !target.starts_with(workspace))
                .collect::<Vec<_>>();
            if !outside_workspace.is_empty() {
                return Err(format!(
                    "Contextlint adapter trace targets escape workspace {workspace:?}: {outside_workspace:?}"
                ));
            }
            let inventory = contextlint_markdown_inventory(workspace)?;
            let cli_entry = rendered_cli;
            let graph = cli_entry
                .ancestors()
                .find(|path| path.file_name() == Some(OsStr::new("node_modules")))
                .ok_or_else(|| {
                    format!("pinned Contextlint executable is outside node_modules: {cli_entry:?}")
                })?
                .to_path_buf();
            let expected_cli_entry = graph.join("@contextlint/cli/dist/index.js");
            let expected_cli_entry = expected_cli_entry.canonicalize().map_err(|error| {
                format!("canonicalize pinned Contextlint CLI entry {expected_cli_entry:?}: {error}")
            })?;
            if cli_entry != expected_cli_entry {
                return Err(format!(
                    "pinned Contextlint executable did not resolve to its CLI JS: {cli_entry:?}"
                ));
            }
            let run_root = PathBuf::from(CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER);
            let probe_config = run_root.join("contextlint.config.json");
            let project_config = run_root.join("project.config.json");
            let probe_document = run_root.join("probe.md");
            let render = |readable: &[&Path], config: &Path, cwd: &Path, files: &[PathBuf]| {
                let mut arguments = vec![
                    "--disable-proto=throw".to_owned(),
                    "--permission".to_owned(),
                ];
                arguments.extend(
                    readable
                        .iter()
                        .map(|root| format!("--allow-fs-read={}", root.display())),
                );
                arguments.extend([
                    cli_entry.to_string_lossy().into_owned(),
                    "lint".to_owned(),
                    "--config".to_owned(),
                    config.to_string_lossy().into_owned(),
                    "--cwd".to_owned(),
                    cwd.to_string_lossy().into_owned(),
                    "--format".to_owned(),
                    "json".to_owned(),
                    "--".to_owned(),
                ]);
                arguments.extend(files.iter().map(|file| file.to_string_lossy().into_owned()));
                arguments
            };
            let probe_files = vec![probe_document];
            let probe_arguments =
                render(&[&graph, &run_root], &probe_config, &run_root, &probe_files);
            let command_arguments = render(
                &[&graph, workspace, &run_root],
                &project_config,
                workspace,
                &inventory,
            );
            Ok((
                node_program.clone(),
                vec![
                    ResolvedTraceInvocation {
                        program: node_program.clone(),
                        targets: targets.to_vec(),
                        arguments: probe_arguments,
                        exit_code: expected_exit_codes[0],
                    },
                    ResolvedTraceInvocation {
                        program: node_program.clone(),
                        targets: targets.to_vec(),
                        arguments: command_arguments,
                        exit_code: expected_exit_codes[1],
                    },
                ],
            ))
        }
        TracePlan::PrivateConfigThenGhalintWorkspace => {
            if expected_exit_codes.len() != 2 {
                return Err(format!(
                    "ghalint-workflow adapter trace for {outer_program} must declare version and workspace-run exit codes, got {expected_exit_codes:?}"
                ));
            }
            let [
                isolated,
                command,
                adapter,
                tool_program,
                project,
                marker_and_files @ ..,
            ] = outer_arguments
            else {
                return Err(format!(
                    "ghalint-workflow adapter trace has an incomplete outer command: {outer_arguments:?}"
                ));
            };
            if isolated != "-I"
                || command != "-c"
                || adapter.is_empty()
                || tool_program != "ghalint"
            {
                return Err(format!(
                    "ghalint-workflow adapter trace expected isolated Python plus logical ghalint, got {outer_arguments:?}"
                ));
            }
            let project = PathBuf::from(project);
            if !project.is_absolute() || !project.is_dir() {
                return Err(format!(
                    "ghalint-workflow adapter trace rendered an invalid project root {project:?}"
                ));
            }
            let project = project.canonicalize().map_err(|error| {
                format!("canonicalize ghalint-workflow project {project:?}: {error}")
            })?;
            let [marker, rendered_files @ ..] = marker_and_files else {
                return Err("ghalint-workflow adapter trace has no file marker".to_owned());
            };
            if marker != GHALINT_WORKFLOW_FILES_MARKER {
                return Err(format!(
                    "ghalint-workflow adapter trace rejects forwarded arguments before its file marker: {marker_and_files:?}"
                ));
            }
            let expected_files = targets
                .iter()
                .map(|target| target.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            if rendered_files != expected_files {
                return Err(format!(
                    "ghalint-workflow adapter trace candidate suffix mismatch: expected {expected_files:?}, got {rendered_files:?}"
                ));
            }
            let workflows = project.join(".github/workflows");
            if targets.iter().any(|target| {
                target.parent() != Some(workflows.as_path())
                    || !matches!(
                        target.extension().and_then(OsStr::to_str),
                        Some("yml" | "yaml")
                    )
            }) {
                return Err(format!(
                    "ghalint-workflow adapter trace targets are outside its top-level workflow inventory: {targets:?}"
                ));
            }
            Ok((
                tool_program.clone(),
                vec![
                    ResolvedTraceInvocation {
                        program: tool_program.clone(),
                        targets: targets.to_vec(),
                        arguments: vec!["--version".to_owned()],
                        exit_code: expected_exit_codes[0],
                    },
                    ResolvedTraceInvocation {
                        program: tool_program.clone(),
                        targets: targets.to_vec(),
                        arguments: vec![
                            "--log-color=never".to_owned(),
                            "--log-level=error".to_owned(),
                            format!("--config={GHALINT_PRIVATE_ROOT_PLACEHOLDER}/ghalint.yaml"),
                            "run".to_owned(),
                        ],
                        exit_code: expected_exit_codes[1],
                    },
                ],
            ))
        }
        TracePlan::SingleNestedTrailingOptions { trailing } => {
            if expected_exit_codes.len() != 1 {
                return Err(format!(
                    "single-child adapter trace for {outer_program} must declare exactly one exit code, got {expected_exit_codes:?}"
                ));
            }
            let (trace_program, base_arguments) =
                nested_trace_command(outer_program, outer_arguments, "single-child adapter")?;
            let arguments = base_arguments
                .into_iter()
                .chain(trailing.iter().map(|argument| (*argument).to_owned()))
                .collect();
            Ok((
                trace_program.clone(),
                vec![ResolvedTraceInvocation {
                    program: trace_program.clone(),
                    targets: targets.to_vec(),
                    arguments,
                    exit_code: expected_exit_codes[0],
                }],
            ))
        }
        TracePlan::SingleNestedFilesMarker {
            nested_program_index,
            adapter_prefix,
            marker,
            leading,
            before_files,
        } => {
            if expected_exit_codes.len() != 1 {
                return Err(format!(
                    "marker-delimited single-child adapter trace for {outer_program} must declare exactly one exit code, got {expected_exit_codes:?}"
                ));
            }
            if nested_program_index != adapter_prefix.len() + 1 {
                return Err(format!(
                    "marker-delimited single-child adapter trace plan for {outer_program} must place exactly one script between its adapter prefix and nested tool"
                ));
            }
            let rendered_prefix = outer_arguments
                .get(..adapter_prefix.len())
                .unwrap_or(outer_arguments);
            if rendered_prefix != adapter_prefix {
                return Err(format!(
                    "marker-delimited single-child adapter {outer_program} adapter prefix mismatch: expected {adapter_prefix:?}, got {rendered_prefix:?}"
                ));
            }
            outer_arguments
                .get(adapter_prefix.len())
                .filter(|script| !script.is_empty())
                .ok_or_else(|| {
                    format!(
                        "marker-delimited single-child adapter {outer_program} has no adapter script after {adapter_prefix:?}: {outer_arguments:?}"
                    )
                })?;
            let trace_program = outer_arguments
                .get(nested_program_index)
                .filter(|program| !program.is_empty())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "marker-delimited single-child adapter {outer_program} has no nested tool at argument {nested_program_index}: {outer_arguments:?}"
                    )
                })?;
            let marker_indices = outer_arguments
                .iter()
                .enumerate()
                .filter_map(|(index, argument)| (argument == marker).then_some(index))
                .collect::<Vec<_>>();
            let [marker_index] = marker_indices.as_slice() else {
                return Err(format!(
                    "marker-delimited single-child adapter {outer_program} requires exactly one {marker:?} marker, found {marker_indices:?}: {outer_arguments:?}"
                ));
            };
            if *marker_index <= nested_program_index {
                return Err(format!(
                    "marker-delimited single-child adapter {outer_program} places {marker:?} before its nested tool: {outer_arguments:?}"
                ));
            }
            let expected_files = targets
                .iter()
                .map(|target| target.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let rendered_files = &outer_arguments[(*marker_index + 1)..];
            if rendered_files != expected_files {
                return Err(format!(
                    "marker-delimited single-child adapter {outer_program} file suffix mismatch: expected {expected_files:?}, got {rendered_files:?}"
                ));
            }
            let arguments = leading
                .iter()
                .map(|argument| (*argument).to_owned())
                .chain(
                    outer_arguments[(nested_program_index + 1)..*marker_index]
                        .iter()
                        .cloned(),
                )
                .chain(before_files.iter().map(|argument| (*argument).to_owned()))
                .chain(expected_files)
                .collect();
            Ok((
                trace_program.clone(),
                vec![ResolvedTraceInvocation {
                    program: trace_program.clone(),
                    targets: targets.to_vec(),
                    arguments,
                    exit_code: expected_exit_codes[0],
                }],
            ))
        }
        TracePlan::VacuumPrivateFilesMarker {
            nested_program_index,
            adapter_prefix,
            marker,
            leading,
        } => {
            if expected_exit_codes.len() != 1 {
                return Err(format!(
                    "Vacuum private-input adapter trace for {outer_program} must declare exactly one exit code, got {expected_exit_codes:?}"
                ));
            }
            if nested_program_index != adapter_prefix.len() + 1 {
                return Err(format!(
                    "Vacuum private-input adapter trace plan for {outer_program} must place exactly one script between its adapter prefix and nested tool"
                ));
            }
            let rendered_prefix = outer_arguments
                .get(..adapter_prefix.len())
                .unwrap_or(outer_arguments);
            if rendered_prefix != adapter_prefix {
                return Err(format!(
                    "Vacuum private-input adapter {outer_program} prefix mismatch: expected {adapter_prefix:?}, got {rendered_prefix:?}"
                ));
            }
            outer_arguments
                .get(adapter_prefix.len())
                .filter(|script| !script.is_empty())
                .ok_or_else(|| {
                    format!(
                        "Vacuum private-input adapter {outer_program} has no script after {adapter_prefix:?}: {outer_arguments:?}"
                    )
                })?;
            let trace_program = outer_arguments
                .get(nested_program_index)
                .filter(|program| !program.is_empty())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "Vacuum private-input adapter {outer_program} has no nested tool at argument {nested_program_index}: {outer_arguments:?}"
                    )
                })?;
            let marker_indices = outer_arguments
                .iter()
                .enumerate()
                .filter_map(|(index, argument)| (argument == marker).then_some(index))
                .collect::<Vec<_>>();
            let [marker_index] = marker_indices.as_slice() else {
                return Err(format!(
                    "Vacuum private-input adapter {outer_program} requires exactly one {marker:?} marker, found {marker_indices:?}: {outer_arguments:?}"
                ));
            };
            if *marker_index != nested_program_index + 1 {
                return Err(format!(
                    "Vacuum private-input adapter {outer_program} must reject every extra argument before {marker:?}: {outer_arguments:?}"
                ));
            }
            let expected_files = targets
                .iter()
                .map(|target| target.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let rendered_files = &outer_arguments[(*marker_index + 1)..];
            if rendered_files != expected_files {
                return Err(format!(
                    "Vacuum private-input adapter {outer_program} selected-file suffix mismatch: expected {expected_files:?}, got {rendered_files:?}"
                ));
            }
            let private_files = targets
                .iter()
                .enumerate()
                .map(|(index, target)| {
                    let extension = target
                        .extension()
                        .and_then(OsStr::to_str)
                        .map(str::to_ascii_lowercase)
                        .ok_or_else(|| {
                            format!(
                                "Vacuum private-input trace target lacks a UTF-8 extension: {target:?}"
                            )
                        })?;
                    if !matches!(extension.as_str(), "yaml" | "yml" | "json") {
                        return Err(format!(
                            "Vacuum private-input trace target has unsupported extension {extension:?}: {target:?}"
                        ));
                    }
                    Ok(format!("inputs/{index:04}.{extension}"))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let arguments = leading
                .iter()
                .map(|argument| (*argument).to_owned())
                .chain(private_files)
                .collect();
            Ok((
                trace_program.clone(),
                vec![ResolvedTraceInvocation {
                    program: trace_program,
                    targets: targets.to_vec(),
                    arguments,
                    exit_code: expected_exit_codes[0],
                }],
            ))
        }
        TracePlan::SingleNestedModeFilesMarker {
            nested_program_index,
            adapter_prefix,
            marker,
            leading,
            mode_arguments,
            before_files,
        } => {
            if expected_exit_codes.len() != 1 {
                return Err(format!(
                    "mode-and-files single-child adapter trace for {outer_program} must declare exactly one exit code, got {expected_exit_codes:?}"
                ));
            }
            if nested_program_index != adapter_prefix.len() + 1 {
                return Err(format!(
                    "mode-and-files single-child adapter trace plan for {outer_program} must place exactly one script between its adapter prefix and nested tool"
                ));
            }
            let rendered_prefix = outer_arguments
                .get(..adapter_prefix.len())
                .unwrap_or(outer_arguments);
            if rendered_prefix != adapter_prefix {
                return Err(format!(
                    "mode-and-files single-child adapter {outer_program} prefix mismatch: expected {adapter_prefix:?}, got {rendered_prefix:?}"
                ));
            }
            outer_arguments
                .get(adapter_prefix.len())
                .filter(|script| !script.is_empty())
                .ok_or_else(|| {
                    format!(
                        "mode-and-files single-child adapter {outer_program} has no script after {adapter_prefix:?}: {outer_arguments:?}"
                    )
                })?;
            let trace_program = outer_arguments
                .get(nested_program_index)
                .filter(|program| !program.is_empty())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "mode-and-files single-child adapter {outer_program} has no nested tool at argument {nested_program_index}: {outer_arguments:?}"
                    )
                })?;
            let mode_index = nested_program_index + 1;
            let mode = outer_arguments.get(mode_index).ok_or_else(|| {
                format!(
                    "mode-and-files single-child adapter {outer_program} has no phase mode: {outer_arguments:?}"
                )
            })?;
            let mode_arguments = mode_arguments
                .iter()
                .find_map(|(name, arguments)| (*name == mode).then_some(*arguments))
                .ok_or_else(|| {
                    format!(
                        "mode-and-files single-child adapter {outer_program} has unsupported phase mode {mode:?}"
                    )
                })?;
            let marker_indices = outer_arguments
                .iter()
                .enumerate()
                .filter_map(|(index, argument)| (argument == marker).then_some(index))
                .collect::<Vec<_>>();
            let [marker_index] = marker_indices.as_slice() else {
                return Err(format!(
                    "mode-and-files single-child adapter {outer_program} requires exactly one {marker:?} marker, found {marker_indices:?}: {outer_arguments:?}"
                ));
            };
            if *marker_index <= mode_index {
                return Err(format!(
                    "mode-and-files single-child adapter {outer_program} places {marker:?} before phase mode: {outer_arguments:?}"
                ));
            }
            let expected_files = targets
                .iter()
                .map(|target| target.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let rendered_files = &outer_arguments[(*marker_index + 1)..];
            if rendered_files != expected_files {
                return Err(format!(
                    "mode-and-files single-child adapter {outer_program} file suffix mismatch: expected {expected_files:?}, got {rendered_files:?}"
                ));
            }
            let arguments = leading
                .iter()
                .map(|argument| (*argument).to_owned())
                .chain(mode_arguments.iter().map(|argument| (*argument).to_owned()))
                .chain(
                    outer_arguments[(mode_index + 1)..*marker_index]
                        .iter()
                        .cloned(),
                )
                .chain(before_files.iter().map(|argument| (*argument).to_owned()))
                .chain(expected_files)
                .collect();
            Ok((
                trace_program.clone(),
                vec![ResolvedTraceInvocation {
                    program: trace_program.clone(),
                    targets: targets.to_vec(),
                    arguments,
                    exit_code: expected_exit_codes[0],
                }],
            ))
        }
        TracePlan::PreflightThenNestedModeFilesMarker {
            nested_program_index,
            adapter_prefix,
            marker,
            mode_arguments,
        } => {
            if nested_program_index != adapter_prefix.len() + 1 {
                return Err(format!(
                    "preflight-and-mode files adapter trace plan for {outer_program} must place exactly one script between its adapter prefix and nested tool"
                ));
            }
            let rendered_prefix = outer_arguments
                .get(..adapter_prefix.len())
                .unwrap_or(outer_arguments);
            if rendered_prefix != adapter_prefix {
                return Err(format!(
                    "preflight-and-mode files adapter {outer_program} prefix mismatch: expected {adapter_prefix:?}, got {rendered_prefix:?}"
                ));
            }
            outer_arguments
                .get(adapter_prefix.len())
                .filter(|script| !script.is_empty())
                .ok_or_else(|| {
                    format!(
                        "preflight-and-mode files adapter {outer_program} has no script after {adapter_prefix:?}: {outer_arguments:?}"
                    )
                })?;
            let trace_program = outer_arguments
                .get(nested_program_index)
                .filter(|program| !program.is_empty())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "preflight-and-mode files adapter {outer_program} has no nested tool at argument {nested_program_index}: {outer_arguments:?}"
                    )
                })?;
            let mode_index = nested_program_index + 1;
            let mode = outer_arguments.get(mode_index).ok_or_else(|| {
                format!(
                    "preflight-and-mode files adapter {outer_program} has no phase mode: {outer_arguments:?}"
                )
            })?;
            let commands = mode_arguments
                .iter()
                .find_map(|(name, arguments)| (*name == mode).then_some(*arguments))
                .ok_or_else(|| {
                    format!(
                        "preflight-and-mode files adapter {outer_program} has unsupported phase mode {mode:?}"
                    )
                })?;
            if expected_exit_codes.is_empty() || expected_exit_codes.len() > commands.len() {
                return Err(format!(
                    "preflight-and-mode files adapter trace for {outer_program} mode {mode:?} expected one through {} exit codes, got {expected_exit_codes:?}",
                    commands.len()
                ));
            }
            let marker_indices = outer_arguments
                .iter()
                .enumerate()
                .filter_map(|(index, argument)| (argument == marker).then_some(index))
                .collect::<Vec<_>>();
            let [marker_index] = marker_indices.as_slice() else {
                return Err(format!(
                    "preflight-and-mode files adapter {outer_program} requires exactly one {marker:?} marker, found {marker_indices:?}: {outer_arguments:?}"
                ));
            };
            if *marker_index <= mode_index {
                return Err(format!(
                    "preflight-and-mode files adapter {outer_program} places {marker:?} before phase mode: {outer_arguments:?}"
                ));
            }
            let expected_files = targets
                .iter()
                .map(|target| target.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let rendered_files = &outer_arguments[(*marker_index + 1)..];
            if rendered_files != expected_files {
                return Err(format!(
                    "preflight-and-mode files adapter {outer_program} file suffix mismatch: expected {expected_files:?}, got {rendered_files:?}"
                ));
            }
            let extra_arguments = &outer_arguments[(mode_index + 1)..*marker_index];
            let traces = commands
                .iter()
                .zip(expected_exit_codes)
                .map(|(command, exit_code)| ResolvedTraceInvocation {
                    program: trace_program.clone(),
                    targets: targets.to_vec(),
                    arguments: command
                        .iter()
                        .map(|argument| (*argument).to_owned())
                        .chain(extra_arguments.iter().cloned())
                        .chain(expected_files.iter().cloned())
                        .collect(),
                    exit_code: *exit_code,
                })
                .collect();
            Ok((trace_program, traces))
        }
        TracePlan::PairedNodeModeFilesMarker {
            node_program_index,
            tool_program_index,
            adapter_prefix,
            marker,
            leading,
            format_preflight_arguments,
            mode_arguments,
            before_files,
        } => {
            if node_program_index != adapter_prefix.len() + 1
                || tool_program_index != node_program_index + 1
            {
                return Err(format!(
                    "paired-Node mode-and-files adapter trace plan for {outer_program} must place exactly one script before consecutive Node and tool arguments"
                ));
            }
            let rendered_prefix = outer_arguments
                .get(..adapter_prefix.len())
                .unwrap_or(outer_arguments);
            if rendered_prefix != adapter_prefix {
                return Err(format!(
                    "paired-Node mode-and-files adapter {outer_program} prefix mismatch: expected {adapter_prefix:?}, got {rendered_prefix:?}"
                ));
            }
            outer_arguments
                .get(adapter_prefix.len())
                .filter(|script| !script.is_empty())
                .ok_or_else(|| {
                    format!(
                        "paired-Node mode-and-files adapter {outer_program} has no script after {adapter_prefix:?}: {outer_arguments:?}"
                    )
                })?;
            let trace_program = outer_arguments
                .get(node_program_index)
                .filter(|program| !program.is_empty())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "paired-Node mode-and-files adapter {outer_program} has no Node executable at argument {node_program_index}: {outer_arguments:?}"
                    )
                })?;
            let tool_program = outer_arguments
                .get(tool_program_index)
                .filter(|program| Path::new(program).is_absolute())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "paired-Node mode-and-files adapter {outer_program} requires an absolute managed tool CLI at argument {tool_program_index}: {outer_arguments:?}"
                    )
                })?;
            let mode_index = tool_program_index + 1;
            let mode = outer_arguments.get(mode_index).ok_or_else(|| {
                format!(
                    "paired-Node mode-and-files adapter {outer_program} has no phase mode: {outer_arguments:?}"
                )
            })?;
            let mode_arguments = mode_arguments
                .iter()
                .find_map(|(name, arguments)| (*name == mode).then_some(*arguments))
                .ok_or_else(|| {
                    format!(
                        "paired-Node mode-and-files adapter {outer_program} has unsupported phase mode {mode:?}"
                    )
                })?;
            if (*mode == "format" && !matches!(expected_exit_codes.len(), 1 | 2))
                || (*mode != "format" && expected_exit_codes.len() != 1)
            {
                return Err(format!(
                    "paired-Node mode-and-files adapter trace for {outer_program} mode {mode:?} has invalid exit-code sequence {expected_exit_codes:?}"
                ));
            }
            let marker_indices = outer_arguments
                .iter()
                .enumerate()
                .filter_map(|(index, argument)| (argument == marker).then_some(index))
                .collect::<Vec<_>>();
            let [marker_index] = marker_indices.as_slice() else {
                return Err(format!(
                    "paired-Node mode-and-files adapter {outer_program} requires exactly one {marker:?} marker, found {marker_indices:?}: {outer_arguments:?}"
                ));
            };
            if *marker_index <= mode_index {
                return Err(format!(
                    "paired-Node mode-and-files adapter {outer_program} places {marker:?} before phase mode: {outer_arguments:?}"
                ));
            }
            let expected_files = targets
                .iter()
                .map(|target| target.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let rendered_files = &outer_arguments[(*marker_index + 1)..];
            if rendered_files != expected_files {
                return Err(format!(
                    "paired-Node mode-and-files adapter {outer_program} file suffix mismatch: expected {expected_files:?}, got {rendered_files:?}"
                ));
            }
            let arguments_for = |native_arguments: &[&str]| {
                std::iter::once(tool_program.clone())
                    .chain(leading.iter().map(|argument| (*argument).to_owned()))
                    .chain(
                        native_arguments
                            .iter()
                            .map(|argument| (*argument).to_owned()),
                    )
                    .chain(
                        outer_arguments[(mode_index + 1)..*marker_index]
                            .iter()
                            .cloned(),
                    )
                    .chain(before_files.iter().map(|argument| (*argument).to_owned()))
                    .chain(expected_files.iter().cloned())
                    .collect::<Vec<_>>()
            };
            let first_arguments = if *mode == "format" {
                arguments_for(format_preflight_arguments)
            } else {
                arguments_for(mode_arguments)
            };
            let mut traces = vec![ResolvedTraceInvocation {
                program: trace_program.clone(),
                targets: targets.to_vec(),
                arguments: first_arguments,
                exit_code: expected_exit_codes[0],
            }];
            if let Some(exit_code) = expected_exit_codes.get(1) {
                traces.push(ResolvedTraceInvocation {
                    program: trace_program.clone(),
                    targets: targets.to_vec(),
                    arguments: arguments_for(mode_arguments),
                    exit_code: *exit_code,
                });
            }
            Ok((trace_program, traces))
        }
        TracePlan::EslintPrivateModeFilesMarker => {
            let [
                isolated,
                command,
                adapter,
                node_program,
                tool_program,
                mode,
                suffix @ ..,
            ] = outer_arguments
            else {
                return Err(format!(
                    "ESLint adapter trace has an incomplete outer command: {outer_arguments:?}"
                ));
            };
            if isolated != "-I"
                || command != "-c"
                || adapter.is_empty()
                || node_program != "node"
                || !Path::new(tool_program).is_absolute()
            {
                return Err(format!(
                    "ESLint adapter trace expected isolated Python, logical Node, and an absolute managed CLI: {outer_arguments:?}"
                ));
            }
            let [marker, rendered_files @ ..] = suffix else {
                return Err("ESLint adapter trace has no file marker".to_owned());
            };
            if marker != ESLINT_FILES_MARKER {
                return Err(format!(
                    "ESLint adapter trace rejects every forwarded argument before its file marker: {suffix:?}"
                ));
            }
            let expected_files = targets
                .iter()
                .map(|target| target.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            if rendered_files != expected_files {
                return Err(format!(
                    "ESLint adapter trace file suffix mismatch: expected {expected_files:?}, got {rendered_files:?}"
                ));
            }
            let native_modes = match (mode.as_str(), expected_exit_codes.len()) {
                ("verify" | "fix", 0) => Vec::new(),
                ("verify", 1) => vec!["verify"],
                ("fix", 2) => vec!["verify", "dry-run"],
                ("fix", 3) => vec!["verify", "dry-run", "fix"],
                ("fix", 4) => vec!["verify", "dry-run", "fix", "verify"],
                _ => {
                    return Err(format!(
                        "ESLint adapter trace mode {mode:?} has invalid exit-code sequence {expected_exit_codes:?}"
                    ));
                }
            };
            let private_root = PathBuf::from(ESLINT_PRIVATE_ROOT_PLACEHOLDER);
            let config = private_root.join("eslint.config.cjs");
            let suppressions = private_root.join("suppressions.json");
            let mut traces = Vec::with_capacity(native_modes.len());
            for (index, (native_mode, exit_code)) in native_modes
                .into_iter()
                .zip(expected_exit_codes)
                .enumerate()
            {
                let native_files = if native_mode == "fix" {
                    expected_files
                        .first()
                        .cloned()
                        .into_iter()
                        .collect::<Vec<_>>()
                } else {
                    expected_files.clone()
                };
                let mut arguments = vec![
                    tool_program.clone(),
                    "--format=json".to_owned(),
                    "--no-color".to_owned(),
                    "--no-config-lookup".to_owned(),
                    format!("--config={}", config.display()),
                    "--no-ignore".to_owned(),
                    "--no-warn-ignored".to_owned(),
                    "--no-inline-config".to_owned(),
                    "--max-warnings=0".to_owned(),
                    "--concurrency=off".to_owned(),
                    "--cache".to_owned(),
                    "--cache-strategy=content".to_owned(),
                    format!(
                        "--cache-location={}",
                        private_root
                            .join(format!("cache-{index}/.eslintcache"))
                            .display()
                    ),
                    format!("--suppressions-location={}", suppressions.display()),
                    "--pass-on-unpruned-suppressions".to_owned(),
                ];
                match native_mode {
                    "dry-run" => arguments.push("--fix-dry-run".to_owned()),
                    "fix" => arguments.push("--fix".to_owned()),
                    "verify" => {}
                    _ => unreachable!("ESLint native mode was constructed above"),
                }
                arguments.push("--".to_owned());
                arguments.extend(native_files.iter().cloned());
                traces.push(ResolvedTraceInvocation {
                    program: node_program.clone(),
                    targets: native_files.into_iter().map(PathBuf::from).collect(),
                    arguments,
                    exit_code: *exit_code,
                });
            }
            Ok((node_program.clone(), traces))
        }
        TracePlan::PreflightThenNestedModeWorkspaceMarker {
            nested_program_index,
            adapter_prefix,
            marker,
            preflight,
            leading,
            mode_arguments,
            before_workspace,
        } => {
            if !matches!(expected_exit_codes.len(), 1 | 2) {
                return Err(format!(
                    "preflight-and-mode workspace adapter trace for {outer_program} must declare one or two exit codes, got {expected_exit_codes:?}"
                ));
            }
            if nested_program_index != adapter_prefix.len() + 1 {
                return Err(format!(
                    "preflight-and-mode workspace adapter trace plan for {outer_program} must place exactly one script between its adapter prefix and nested tool"
                ));
            }
            let rendered_prefix = outer_arguments
                .get(..adapter_prefix.len())
                .unwrap_or(outer_arguments);
            if rendered_prefix != adapter_prefix {
                return Err(format!(
                    "preflight-and-mode workspace adapter {outer_program} prefix mismatch: expected {adapter_prefix:?}, got {rendered_prefix:?}"
                ));
            }
            outer_arguments
                .get(adapter_prefix.len())
                .filter(|script| !script.is_empty())
                .ok_or_else(|| {
                    format!(
                        "preflight-and-mode workspace adapter {outer_program} has no script after {adapter_prefix:?}: {outer_arguments:?}"
                    )
                })?;
            let trace_program = outer_arguments
                .get(nested_program_index)
                .filter(|program| !program.is_empty())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "preflight-and-mode workspace adapter {outer_program} has no nested tool at argument {nested_program_index}: {outer_arguments:?}"
                    )
                })?;
            let mode_index = nested_program_index + 1;
            let mode = outer_arguments.get(mode_index).ok_or_else(|| {
                format!(
                    "preflight-and-mode workspace adapter {outer_program} has no phase mode: {outer_arguments:?}"
                )
            })?;
            let mode_arguments = mode_arguments
                .iter()
                .find_map(|(name, arguments)| (*name == mode).then_some(*arguments))
                .ok_or_else(|| {
                    format!(
                        "preflight-and-mode workspace adapter {outer_program} has unsupported phase mode {mode:?}"
                    )
                })?;
            let marker_indices = outer_arguments
                .iter()
                .enumerate()
                .filter_map(|(index, argument)| (argument == marker).then_some(index))
                .collect::<Vec<_>>();
            let [marker_index] = marker_indices.as_slice() else {
                return Err(format!(
                    "preflight-and-mode workspace adapter {outer_program} requires exactly one {marker:?} marker, found {marker_indices:?}: {outer_arguments:?}"
                ));
            };
            if *marker_index <= mode_index {
                return Err(format!(
                    "preflight-and-mode workspace adapter {outer_program} places {marker:?} before phase mode: {outer_arguments:?}"
                ));
            }
            let rendered_workspace = &outer_arguments[(*marker_index + 1)..];
            let [workspace] = rendered_workspace else {
                return Err(format!(
                    "preflight-and-mode workspace adapter {outer_program} requires exactly one workspace after {marker:?}, got {rendered_workspace:?}"
                ));
            };
            let workspace_path = Path::new(workspace);
            if !workspace_path.is_absolute() {
                return Err(format!(
                    "preflight-and-mode workspace adapter {outer_program} rendered a non-absolute workspace {workspace:?}"
                ));
            }
            let outside_workspace = targets
                .iter()
                .filter(|target| !target.starts_with(workspace_path))
                .collect::<Vec<_>>();
            if !outside_workspace.is_empty() {
                return Err(format!(
                    "preflight-and-mode workspace adapter {outer_program} targets escape workspace {workspace:?}: {outside_workspace:?}"
                ));
            }
            let arguments = leading
                .iter()
                .map(|argument| (*argument).to_owned())
                .chain(mode_arguments.iter().map(|argument| (*argument).to_owned()))
                .chain(
                    outer_arguments[(mode_index + 1)..*marker_index]
                        .iter()
                        .cloned(),
                )
                .chain(
                    before_workspace
                        .iter()
                        .map(|argument| (*argument).to_owned()),
                )
                .chain(std::iter::once(workspace.clone()))
                .collect();
            let mut traces = vec![ResolvedTraceInvocation {
                program: trace_program.clone(),
                targets: targets.to_vec(),
                arguments: preflight
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect(),
                exit_code: expected_exit_codes[0],
            }];
            if let Some(exit_code) = expected_exit_codes.get(1) {
                traces.push(ResolvedTraceInvocation {
                    program: trace_program.clone(),
                    targets: targets.to_vec(),
                    arguments,
                    exit_code: *exit_code,
                });
            }
            Ok((trace_program, traces))
        }
        TracePlan::PreflightThenNestedModeWorkspaceIndicatorMarker {
            preflight_program_index,
            command_program_index,
            adapter_prefix,
            marker,
            modes,
            preflight_before_indicator,
            preflight_after_indicator,
            command_before_indicator,
            command_after_indicators,
        } => {
            if expected_exit_codes.is_empty()
                || expected_exit_codes.len() > command_after_indicators.len() + 1
            {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter trace for {outer_program} must declare one preflight exit code and at most {} command exit codes, got {expected_exit_codes:?}",
                    command_after_indicators.len()
                ));
            }
            if command_after_indicators.is_empty() {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter trace plan for {outer_program} must declare at least one command argument suffix"
                ));
            }
            if preflight_program_index != adapter_prefix.len() + 1
                || command_program_index != preflight_program_index + 1
            {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter trace plan for {outer_program} must place exactly one script before consecutive preflight and command tools"
                ));
            }
            let rendered_prefix = outer_arguments
                .get(..adapter_prefix.len())
                .unwrap_or(outer_arguments);
            if rendered_prefix != adapter_prefix {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} prefix mismatch: expected {adapter_prefix:?}, got {rendered_prefix:?}"
                ));
            }
            outer_arguments
                .get(adapter_prefix.len())
                .filter(|script| !script.is_empty())
                .ok_or_else(|| {
                    format!(
                        "preflight-and-mode workspace-indicator adapter {outer_program} has no script after {adapter_prefix:?}: {outer_arguments:?}"
                    )
                })?;
            let preflight_program = outer_arguments
                .get(preflight_program_index)
                .filter(|program| !program.is_empty())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "preflight-and-mode workspace-indicator adapter {outer_program} has no preflight tool at argument {preflight_program_index}: {outer_arguments:?}"
                    )
                })?;
            let command_program = outer_arguments
                .get(command_program_index)
                .filter(|program| !program.is_empty())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "preflight-and-mode workspace-indicator adapter {outer_program} has no command tool at argument {command_program_index}: {outer_arguments:?}"
                    )
                })?;
            if command_program == preflight_program {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} must trace distinct preflight and command tools, got {preflight_program:?} twice"
                ));
            }
            let mode_index = command_program_index + 1;
            let mode = outer_arguments.get(mode_index).ok_or_else(|| {
                format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} has no phase mode: {outer_arguments:?}"
                )
            })?;
            if !modes.iter().any(|expected| *expected == mode) {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} has unsupported phase mode {mode:?}"
                ));
            }
            let marker_indices = outer_arguments
                .iter()
                .enumerate()
                .filter_map(|(index, argument)| (argument == marker).then_some(index))
                .collect::<Vec<_>>();
            let [marker_index] = marker_indices.as_slice() else {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} requires exactly one {marker:?} marker, found {marker_indices:?}: {outer_arguments:?}"
                ));
            };
            if *marker_index <= mode_index {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} places {marker:?} before phase mode: {outer_arguments:?}"
                ));
            }
            let forwarded = &outer_arguments[(mode_index + 1)..*marker_index];
            if !forwarded.is_empty() {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} fixture trace does not permit forwarded extra arguments: {forwarded:?}"
                ));
            }
            let rendered_indicators = &outer_arguments[(*marker_index + 1)..];
            let [indicator] = rendered_indicators else {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} requires exactly one indicator after {marker:?}, got {rendered_indicators:?}"
                ));
            };
            let indicator_path = Path::new(indicator);
            if !indicator_path.is_absolute() || !indicator_path.is_file() {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} rendered an invalid workspace indicator {indicator:?}"
                ));
            }
            let workspace = indicator_path.parent().ok_or_else(|| {
                format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} indicator has no parent: {indicator:?}"
                )
            })?;
            let outside_workspace = targets
                .iter()
                .filter(|target| !target.starts_with(workspace))
                .collect::<Vec<_>>();
            if !outside_workspace.is_empty() {
                return Err(format!(
                    "preflight-and-mode workspace-indicator adapter {outer_program} targets escape indicator workspace {workspace:?}: {outside_workspace:?}"
                ));
            }
            let render = |before: &[&str], after: &[&str]| {
                before
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .chain(std::iter::once(indicator.clone()))
                    .chain(after.iter().map(|argument| (*argument).to_owned()))
                    .collect::<Vec<_>>()
            };
            let mut traces = vec![ResolvedTraceInvocation {
                program: preflight_program.clone(),
                targets: targets.to_vec(),
                arguments: render(preflight_before_indicator, preflight_after_indicator),
                exit_code: expected_exit_codes[0],
            }];
            for (exit_code, after_indicator) in expected_exit_codes
                .iter()
                .skip(1)
                .zip(command_after_indicators.iter())
            {
                traces.push(ResolvedTraceInvocation {
                    program: command_program.clone(),
                    targets: targets.to_vec(),
                    arguments: render(command_before_indicator, after_indicator),
                    exit_code: *exit_code,
                });
            }
            Ok((preflight_program, traces))
        }
        TracePlan::CargoFmtWorkspaceIndicatorMarker {
            adapter_prefix,
            marker,
            target_roots,
            edition,
        } => {
            if !matches!(expected_exit_codes.len(), 5 | 8) {
                return Err(format!(
                    "Cargo Fmt adapter trace for {outer_program} must declare the five coverage children and optionally the three real-workspace children, got {expected_exit_codes:?}"
                ));
            }
            let rendered_prefix = outer_arguments
                .get(..adapter_prefix.len())
                .unwrap_or(outer_arguments);
            if rendered_prefix != adapter_prefix {
                return Err(format!(
                    "Cargo Fmt adapter {outer_program} prefix mismatch: expected {adapter_prefix:?}, got {rendered_prefix:?}"
                ));
            }
            outer_arguments
                .get(adapter_prefix.len())
                .filter(|script| !script.is_empty())
                .ok_or_else(|| {
                    format!(
                        "Cargo Fmt adapter {outer_program} has no script after {adapter_prefix:?}: {outer_arguments:?}"
                    )
                })?;
            let cargo = outer_arguments.get(3).filter(|value| *value == "cargo").cloned().ok_or_else(|| {
                format!("Cargo Fmt adapter has no fixed cargo launcher at argument 3: {outer_arguments:?}")
            })?;
            let cargo_fmt = outer_arguments
                .get(4)
                .filter(|value| *value == "cargo-fmt")
                .cloned()
                .ok_or_else(|| {
                    format!("Cargo Fmt adapter has no fixed cargo-fmt companion at argument 4: {outer_arguments:?}")
                })?;
            let rustfmt = outer_arguments
                .get(5)
                .filter(|value| *value == "rustfmt")
                .cloned()
                .ok_or_else(|| {
                    format!("Cargo Fmt adapter has no fixed rustfmt companion at argument 5: {outer_arguments:?}")
                })?;
            let phase = outer_arguments.get(6).ok_or_else(|| {
                format!("Cargo Fmt adapter has no phase mode: {outer_arguments:?}")
            })?;
            if !matches!(phase.as_str(), "format" | "verify") {
                return Err(format!(
                    "Cargo Fmt adapter has unsupported phase mode {phase:?}"
                ));
            }
            let marker_indices = outer_arguments
                .iter()
                .enumerate()
                .filter_map(|(index, argument)| (argument == marker).then_some(index))
                .collect::<Vec<_>>();
            let [marker_index] = marker_indices.as_slice() else {
                return Err(format!(
                    "Cargo Fmt adapter requires exactly one {marker:?} marker, found {marker_indices:?}: {outer_arguments:?}"
                ));
            };
            if *marker_index != 7 || !outer_arguments[7..*marker_index].is_empty() {
                return Err(format!(
                    "Cargo Fmt fixture trace rejects forwarded extra arguments: {outer_arguments:?}"
                ));
            }
            let [indicator] = &outer_arguments[(*marker_index + 1)..] else {
                return Err(format!(
                    "Cargo Fmt adapter requires exactly one workspace indicator after {marker:?}: {outer_arguments:?}"
                ));
            };
            let indicator_path = Path::new(indicator);
            if !indicator_path.is_absolute()
                || indicator_path.file_name() != Some(OsStr::new("Cargo.lock"))
                || !indicator_path.is_file()
            {
                return Err(format!(
                    "Cargo Fmt adapter rendered an invalid workspace indicator {indicator:?}"
                ));
            }
            let workspace = indicator_path.parent().ok_or_else(|| {
                format!("Cargo Fmt workspace indicator has no parent: {indicator:?}")
            })?;
            let manifest_path = canonical_project(&workspace.join("Cargo.toml"));
            if !manifest_path.is_file() {
                return Err(format!(
                    "Cargo Fmt fixture trace requires a root Cargo.toml beside {indicator:?}"
                ));
            }
            let manifest = manifest_path.to_string_lossy().into_owned();
            if targets.iter().any(|target| !target.starts_with(workspace)) {
                return Err(format!(
                    "Cargo Fmt adapter targets escape indicator workspace {workspace:?}: {targets:?}"
                ));
            }
            if !workspace.join("rustfmt.toml").is_file()
                && !workspace.join(".rustfmt.toml").is_file()
            {
                return Err(format!(
                    "Cargo Fmt fixture trace requires one root rustfmt configuration in {workspace:?}"
                ));
            }

            let actual_targets = target_roots
                .iter()
                .map(|relative| canonical_project(&workspace.join(relative)))
                .collect::<Vec<_>>();
            for target in &actual_targets {
                if !target.is_file() {
                    return Err(format!(
                        "Cargo Fmt trace target root is unavailable: {target:?}"
                    ));
                }
            }
            let private = |suffix: &str| {
                format!(
                    "{CARGO_FMT_PRIVATE_ROOT_PLACEHOLDER}/{}",
                    suffix.trim_start_matches('/')
                )
            };
            let coverage_indicator = private("coverage-workspace/Cargo.toml");
            let coverage_config = private("coverage-workspace");
            let coverage_targets = target_roots
                .iter()
                .map(|relative| private(&format!("coverage-workspace/{relative}")))
                .collect::<Vec<_>>();
            let config = workspace.to_string_lossy().into_owned();
            let metadata = |selected_indicator: String, locked: bool| {
                let mut arguments = vec![
                    "metadata".to_owned(),
                    if locked {
                        "--format-version=1".to_owned()
                    } else {
                        "--format-version".to_owned()
                    },
                ];
                if !locked {
                    arguments.push("1".to_owned());
                }
                arguments.extend([
                    "--no-deps".to_owned(),
                    "--manifest-path".to_owned(),
                    selected_indicator,
                ]);
                if locked {
                    arguments.extend([
                        "--locked".to_owned(),
                        "--offline".to_owned(),
                        "--quiet".to_owned(),
                    ]);
                } else {
                    arguments.push("--offline".to_owned());
                }
                arguments
            };
            let cargo_fmt_arguments =
                |selected_indicator: String, selected_config: String, check: bool| {
                    let mut arguments = vec![
                        "fmt".to_owned(),
                        "--all".to_owned(),
                        "--manifest-path".to_owned(),
                        selected_indicator,
                    ];
                    if check {
                        arguments.push("--check".to_owned());
                    }
                    arguments.extend([
                        "--".to_owned(),
                        "--config-path".to_owned(),
                        selected_config,
                        "--color".to_owned(),
                        "never".to_owned(),
                        "--files-with-diff".to_owned(),
                    ]);
                    arguments
                };
            let rustfmt_arguments =
                |selected_targets: Vec<String>, selected_config: String, check: bool| {
                    let mut arguments = selected_targets;
                    arguments.extend([
                        "--edition".to_owned(),
                        edition.to_owned(),
                        "--config-path".to_owned(),
                        selected_config,
                        "--color".to_owned(),
                        "never".to_owned(),
                        "--files-with-diff".to_owned(),
                    ]);
                    if check {
                        arguments.push("--check".to_owned());
                    }
                    arguments
                };
            let original_targets = actual_targets
                .iter()
                .map(|target| target.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let coverage_candidate_targets = coverage_targets
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>();
            let mut traces = vec![
                ResolvedTraceInvocation {
                    program: cargo.clone(),
                    targets: targets.to_vec(),
                    arguments: metadata(manifest.clone(), true),
                    exit_code: expected_exit_codes[0],
                },
                ResolvedTraceInvocation {
                    program: cargo.clone(),
                    targets: coverage_candidate_targets.clone(),
                    arguments: metadata(coverage_indicator.clone(), true),
                    exit_code: expected_exit_codes[1],
                },
                ResolvedTraceInvocation {
                    program: cargo_fmt.clone(),
                    targets: coverage_candidate_targets.clone(),
                    arguments: cargo_fmt_arguments(
                        coverage_indicator.clone(),
                        coverage_config.clone(),
                        true,
                    ),
                    exit_code: expected_exit_codes[2],
                },
                ResolvedTraceInvocation {
                    program: cargo.clone(),
                    targets: coverage_candidate_targets.clone(),
                    arguments: metadata(coverage_indicator.clone(), false),
                    exit_code: expected_exit_codes[3],
                },
                ResolvedTraceInvocation {
                    program: rustfmt.clone(),
                    targets: coverage_candidate_targets,
                    arguments: rustfmt_arguments(coverage_targets, coverage_config, true),
                    exit_code: expected_exit_codes[4],
                },
            ];
            if expected_exit_codes.len() == 8 {
                traces.extend([
                    ResolvedTraceInvocation {
                        program: cargo_fmt,
                        targets: actual_targets.clone(),
                        arguments: cargo_fmt_arguments(
                            manifest.clone(),
                            config.clone(),
                            phase == "verify",
                        ),
                        exit_code: expected_exit_codes[5],
                    },
                    ResolvedTraceInvocation {
                        program: cargo.clone(),
                        targets: actual_targets.clone(),
                        arguments: metadata(manifest, false),
                        exit_code: expected_exit_codes[6],
                    },
                    ResolvedTraceInvocation {
                        program: rustfmt,
                        targets: actual_targets,
                        arguments: rustfmt_arguments(original_targets, config, phase == "verify"),
                        exit_code: expected_exit_codes[7],
                    },
                ]);
            }
            Ok((cargo, traces))
        }
        TracePlan::DclintFixSubsetFilesMarker { fixable_targets } => {
            resolve_dclint_trace_invocations(
                outer_program,
                outer_arguments,
                targets,
                expected_exit_codes,
                fixable_targets,
            )
        }
        TracePlan::TrailingOptionsAdapter {
            preflight,
            validation,
        } => {
            if !matches!(expected_exit_codes.len(), 1 | 2) {
                return Err(format!(
                    "failure-level adapter trace must declare one or two exit codes, got {expected_exit_codes:?}"
                ));
            }
            let (trace_program, base_arguments) =
                nested_trace_command(outer_program, outer_arguments, "failure-level adapter")?;
            let render_nested = |trailing_options: &[&str]| {
                base_arguments
                    .iter()
                    .cloned()
                    .chain(
                        trailing_options
                            .iter()
                            .map(|argument| (*argument).to_owned()),
                    )
                    .collect::<Vec<_>>()
            };
            let mut traces = vec![ResolvedTraceInvocation {
                program: trace_program.clone(),
                targets: targets.to_vec(),
                arguments: render_nested(preflight),
                exit_code: expected_exit_codes[0],
            }];
            if let Some(exit_code) = expected_exit_codes.get(1) {
                traces.push(ResolvedTraceInvocation {
                    program: trace_program.clone(),
                    targets: targets.to_vec(),
                    arguments: render_nested(validation),
                    exit_code: *exit_code,
                });
            }
            Ok((trace_program, traces))
        }
    }
}

fn resolve_dclint_trace_invocations(
    outer_program: &str,
    outer_arguments: &[String],
    targets: &[PathBuf],
    expected_exit_codes: &[i32],
    fixable_targets: &[&str],
) -> Result<(String, Vec<ResolvedTraceInvocation>), String> {
    const NESTED_PROGRAM_INDEX: usize = 3;
    const ADAPTER_PREFIX: &[&str] = &["-I", "-c"];
    if !matches!(expected_exit_codes.len(), 0 | 1 | 3) {
        return Err(format!(
            "dclint adapter trace must declare zero, one, or three native statuses, got {expected_exit_codes:?}"
        ));
    }
    let rendered_prefix = outer_arguments
        .get(..ADAPTER_PREFIX.len())
        .unwrap_or(outer_arguments);
    if rendered_prefix != ADAPTER_PREFIX {
        return Err(format!(
            "dclint adapter {outer_program} prefix mismatch: {outer_arguments:?}"
        ));
    }
    outer_arguments
        .get(ADAPTER_PREFIX.len())
        .filter(|script| !script.is_empty())
        .ok_or_else(|| format!("dclint adapter {outer_program} has no evaluated script"))?;
    let trace_program = outer_arguments
        .get(NESTED_PROGRAM_INDEX)
        .filter(|program| !program.is_empty())
        .cloned()
        .ok_or_else(|| format!("dclint adapter {outer_program} has no nested tool"))?;
    let mode = outer_arguments
        .get(NESTED_PROGRAM_INDEX + 1)
        .map(String::as_str)
        .ok_or_else(|| format!("dclint adapter {outer_program} has no mode"))?;
    if !matches!(mode, "verify" | "fix") {
        return Err(format!(
            "dclint adapter {outer_program} has unsupported mode {mode:?}"
        ));
    }
    let project = outer_arguments
        .get(NESTED_PROGRAM_INDEX + 2)
        .map(PathBuf::from)
        .ok_or_else(|| format!("dclint adapter {outer_program} has no project root"))?;
    if !project.is_absolute() {
        return Err(format!(
            "dclint adapter {outer_program} rendered a non-absolute project root {project:?}"
        ));
    }
    let marker_indices = outer_arguments
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| (argument == DCLINT_FILES_MARKER).then_some(index))
        .collect::<Vec<_>>();
    let [marker_index] = marker_indices.as_slice() else {
        return Err(format!(
            "dclint adapter {outer_program} requires exactly one {DCLINT_FILES_MARKER:?} marker, found {marker_indices:?}"
        ));
    };
    if *marker_index < NESTED_PROGRAM_INDEX + 3 {
        return Err(format!(
            "dclint adapter {outer_program} places its files marker before the project root"
        ));
    }
    let expected_files = targets
        .iter()
        .map(|target| target.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if outer_arguments[(*marker_index + 1)..] != expected_files {
        return Err(format!(
            "dclint adapter {outer_program} file suffix mismatch: expected {expected_files:?}, got {:?}",
            &outer_arguments[(*marker_index + 1)..]
        ));
    }
    if targets.iter().any(|target| !target.starts_with(&project)) {
        return Err(format!(
            "dclint adapter {outer_program} has selected files outside {project:?}: {targets:?}"
        ));
    }
    let extra_arguments = &outer_arguments[(NESTED_PROGRAM_INDEX + 3)..*marker_index];
    let config_argument = match extra_arguments {
        [] => DCLINT_PRIVATE_CONFIG_ARGUMENT.to_owned(),
        [argument] if argument.starts_with("--config=") => {
            let relative = &argument["--config=".len()..];
            let path = project.join(relative);
            format!("--config={}", path.to_string_lossy())
        }
        _ => {
            if expected_exit_codes.is_empty() {
                return Ok((trace_program, Vec::new()));
            }
            return Err(format!(
                "dclint adapter {outer_program} trace cannot render extra arguments {extra_arguments:?}"
            ));
        }
    };
    if expected_exit_codes.is_empty() {
        return Ok((trace_program, Vec::new()));
    }
    let native_prefix = [
        "--formatter=json".to_owned(),
        "--color=false".to_owned(),
        "--max-warnings=0".to_owned(),
        config_argument,
    ];
    let render = |files: &[String], fix: bool| {
        native_prefix
            .iter()
            .cloned()
            .chain(fix.then_some("--fix".to_owned()))
            .chain(files.iter().cloned())
            .collect::<Vec<_>>()
    };
    let mut traces = vec![ResolvedTraceInvocation {
        program: trace_program.clone(),
        targets: targets.to_vec(),
        arguments: render(&expected_files, false),
        exit_code: expected_exit_codes[0],
    }];
    if expected_exit_codes.len() == 3 {
        if mode != "fix" || fixable_targets.is_empty() {
            return Err(
                "dclint three-child trace requires fix mode and a nonempty proven-fixable subset"
                    .to_owned(),
            );
        }
        let mut fixable_files = Vec::new();
        let mut fixable_paths = Vec::new();
        for relative in fixable_targets {
            let candidate = project.join(relative);
            if !targets.contains(&candidate) {
                return Err(format!(
                    "dclint proven-fixable trace target is not selected: {relative:?}"
                ));
            }
            fixable_files.push(candidate.to_string_lossy().into_owned());
            fixable_paths.push(candidate);
        }
        traces.push(ResolvedTraceInvocation {
            program: trace_program.clone(),
            targets: fixable_paths,
            arguments: render(&fixable_files, true),
            exit_code: expected_exit_codes[1],
        });
        traces.push(ResolvedTraceInvocation {
            program: trace_program.clone(),
            targets: targets.to_vec(),
            arguments: render(&expected_files, false),
            exit_code: expected_exit_codes[2],
        });
    }
    Ok((trace_program, traces))
}

fn nested_trace_command(
    outer_program: &str,
    outer_arguments: &[String],
    adapter: &str,
) -> Result<(String, Vec<String>), String> {
    let separator = outer_arguments
        .iter()
        .position(|argument| argument == "--")
        .ok_or_else(|| {
            format!("{adapter} {outer_program} command has no `--` separator: {outer_arguments:?}")
        })?;
    let trace_program = outer_arguments
        .get(separator + 1)
        .filter(|program| !program.is_empty())
        .cloned()
        .ok_or_else(|| {
            format!("{adapter} {outer_program} command has no nested tool executable")
        })?;
    let base_arguments = outer_arguments[(separator + 2)..].to_vec();
    if base_arguments.iter().any(|argument| argument == "--") {
        return Err(format!(
            "{adapter} nested command rejects inner `--`: {base_arguments:?}"
        ));
    }
    Ok((trace_program, base_arguments))
}

fn contextlint_markdown_inventory(workspace: &Path) -> Result<Vec<PathBuf>, String> {
    const EXCLUDED: &[&str] = &[".git", "node_modules", ".velvet-glove"];
    const MAX_ENTRIES: usize = 100_000;
    const MAX_FILES: usize = 4_096;
    const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
    const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

    fn visit(
        directory: &Path,
        inventory: &mut Vec<PathBuf>,
        seen: &mut BTreeSet<(u64, u64)>,
        entry_count: &mut usize,
        total_bytes: &mut u64,
    ) -> Result<(), String> {
        use std::os::unix::fs::MetadataExt;

        let metadata = std::fs::symlink_metadata(directory).map_err(|error| {
            format!("inspect Contextlint inventory directory {directory:?}: {error}")
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(format!(
                "Contextlint inventory encountered a non-physical directory {directory:?}"
            ));
        }
        for entry in sorted_entries(directory)? {
            *entry_count += 1;
            if *entry_count > MAX_ENTRIES {
                return Err(format!(
                    "Contextlint inventory exceeds {MAX_ENTRIES} entries"
                ));
            }
            let path = entry.path();
            let name = entry.file_name();
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
                format!("inspect Contextlint workspace entry {path:?}: {error}")
            })?;
            if metadata.file_type().is_symlink() {
                if name.to_str().is_some_and(|name| EXCLUDED.contains(&name)) {
                    continue;
                }
                return Err(format!(
                    "Contextlint inventory rejects symbolic link {path:?}"
                ));
            }
            if metadata.is_dir() {
                if !name.to_str().is_some_and(|name| EXCLUDED.contains(&name)) {
                    visit(&path, inventory, seen, entry_count, total_bytes)?;
                }
                continue;
            }
            let is_markdown = name
                .to_str()
                .map(str::to_ascii_lowercase)
                .is_some_and(|name| name.ends_with(".md") || name.ends_with(".markdown"));
            if !is_markdown {
                continue;
            }
            if !metadata.is_file() || metadata.nlink() != 1 {
                return Err(format!(
                    "Contextlint inventory requires one-link regular Markdown files, got {path:?}"
                ));
            }
            if metadata.len() > MAX_FILE_BYTES {
                return Err(format!(
                    "Contextlint Markdown file exceeds {MAX_FILE_BYTES} bytes: {path:?}"
                ));
            }
            *total_bytes = total_bytes.saturating_add(metadata.len());
            if *total_bytes > MAX_TOTAL_BYTES {
                return Err(format!(
                    "Contextlint inventory exceeds {MAX_TOTAL_BYTES} Markdown bytes"
                ));
            }
            if !seen.insert((metadata.dev(), metadata.ino())) {
                return Err(format!(
                    "Contextlint inventory contains a duplicate inode at {path:?}"
                ));
            }
            if inventory.len() >= MAX_FILES {
                return Err(format!(
                    "Contextlint inventory exceeds {MAX_FILES} Markdown files"
                ));
            }
            inventory.push(path.canonicalize().map_err(|error| {
                format!("canonicalize Contextlint Markdown file {path:?}: {error}")
            })?);
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = workspace;
        return Err("Contextlint real-tool trace requires Unix file identities".to_owned());
    }
    #[cfg(unix)]
    {
        let mut inventory = Vec::new();
        let mut seen = BTreeSet::new();
        let mut entry_count = 0;
        let mut total_bytes = 0;
        visit(
            workspace,
            &mut inventory,
            &mut seen,
            &mut entry_count,
            &mut total_bytes,
        )?;
        inventory.sort();
        if inventory.is_empty() {
            return Err("Contextlint inventory found zero Markdown files".to_owned());
        }
        Ok(inventory)
    }
}

fn render_expected_arguments(
    spec: &ToolSpec,
    phase: &Phase,
    project: &Path,
    targets: &[PathBuf],
) -> Result<Vec<String>, String> {
    render_expected_argv(spec, &phase.argv, &phase.extra_args, project, targets)
}

fn render_expected_workflow_arguments(
    spec: &ToolSpec,
    command: &WorkflowCommand,
    project: &Path,
    targets: &[PathBuf],
) -> Result<Vec<String>, String> {
    render_expected_argv(spec, &command.argv, &command.extra_args, project, targets)
}

fn render_expected_argv(
    spec: &ToolSpec,
    argv: &[ArgvElement],
    extra_args: &[String],
    project: &Path,
    targets: &[PathBuf],
) -> Result<Vec<String>, String> {
    let project = canonical_project(project);
    let (workspace, workspace_indicator) = resolve_expected_workspace_job(spec, &project, targets)?;
    let mut arguments = Vec::new();
    for element in argv {
        match element {
            ArgvElement::Literal(value) => arguments.push(value.clone()),
            ArgvElement::Token(ArgToken::Files) => arguments.extend(
                targets
                    .iter()
                    .map(|target| target.to_string_lossy().into_owned()),
            ),
            ArgvElement::Token(ArgToken::ExtraArgs) => {
                arguments.extend(extra_args.iter().cloned());
            }
            ArgvElement::Token(ArgToken::ProjectRoot) => {
                arguments.push(project.to_string_lossy().into_owned());
            }
            ArgvElement::Token(ArgToken::Workspace) => {
                arguments.push(workspace.to_string_lossy().into_owned());
            }
            ArgvElement::Token(ArgToken::WorkspaceFiles) => {
                arguments.extend(targets.iter().map(|target| {
                    target
                        .strip_prefix(&workspace)
                        .unwrap_or(target)
                        .to_string_lossy()
                        .into_owned()
                }));
            }
            ArgvElement::Token(ArgToken::ToolExecutable) => {
                arguments.push(spec.executable.clone());
            }
            ArgvElement::Token(ArgToken::WorkspaceIndicator) => arguments.push(
                workspace_indicator
                    .as_ref()
                    .ok_or_else(|| {
                        format!(
                            "{} fixture contract rendered WorkspaceIndicator without a configured marker",
                            spec.id
                        )
                    })?
                    .to_string_lossy()
                    .into_owned(),
            ),
        }
    }
    Ok(arguments)
}

fn resolve_expected_workspace_job(
    spec: &ToolSpec,
    project: &Path,
    targets: &[PathBuf],
) -> Result<(PathBuf, Option<PathBuf>), String> {
    let Some(indicator_name) = spec.workspace_indicator.as_deref() else {
        return Ok((project.to_path_buf(), None));
    };
    if targets.is_empty() {
        return Err(format!(
            "{} fixture contract cannot resolve workspace marker {indicator_name:?} for an empty invocation",
            spec.id
        ));
    }

    let mut indicators = BTreeSet::new();
    for target in targets {
        if !target.starts_with(project) {
            return Err(format!(
                "{} fixture contract target escapes project {project:?}: {target:?}",
                spec.id
            ));
        }
        let indicator = nearest_fixture_workspace_indicator(target, project, indicator_name)
            .ok_or_else(|| {
                format!(
                    "{} fixture contract found no {indicator_name:?} from target {target:?} through project {project:?}",
                    spec.id
                )
            })?;
        indicators.insert(indicator);
    }
    if indicators.len() != 1 {
        return Err(format!(
            "{} fixture invocation spans multiple {indicator_name:?} workspaces: {indicators:?}",
            spec.id
        ));
    }
    let indicator = indicators
        .into_iter()
        .next()
        .expect("one workspace indicator was required");
    let workspace = indicator.parent().ok_or_else(|| {
        format!(
            "{} fixture workspace marker has no parent directory: {indicator:?}",
            spec.id
        )
    })?;
    Ok((workspace.to_path_buf(), Some(indicator)))
}

fn nearest_fixture_workspace_indicator(
    target: &Path,
    project: &Path,
    indicator_name: &str,
) -> Option<PathBuf> {
    let mut current = target.parent();
    while let Some(directory) = current {
        let candidate = directory.join(indicator_name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if directory == project {
            break;
        }
        current = directory.parent();
    }
    None
}

fn classify_expected_exit(phase: &Phase, code: i32) -> ExpectedOutcome {
    classify_expected_exit_codes(&phase.exit_codes, code)
}

fn classify_expected_exit_codes(exit_codes: &ExitCodes, code: i32) -> ExpectedOutcome {
    if exit_codes.clean.contains(&code) {
        ExpectedOutcome::Clean
    } else if exit_codes.issues.contains(&code) {
        ExpectedOutcome::Issues
    } else if exit_codes.failure.contains(&code) {
        ExpectedOutcome::OperationalFailure
    } else {
        match exit_codes.unexpected {
            UnexpectedExitPolicy::Failure => ExpectedOutcome::OperationalFailure,
            UnexpectedExitPolicy::Issues => ExpectedOutcome::Issues,
        }
    }
}

#[derive(Clone)]
struct PrettierToolchain {
    root: PathBuf,
    node: PathBuf,
    cli: PathBuf,
}

impl PrettierToolchain {
    fn resolve_if_configured() -> Result<Option<Self>, String> {
        let Some(requested_root) = std::env::var_os(PRETTIER_ROOT_ENV) else {
            return Ok(None);
        };
        Self::resolve(PathBuf::from(requested_root)).map(Some)
    }

    fn resolve(requested_root: PathBuf) -> Result<Self, String> {
        if !requested_root.is_absolute() {
            return Err(format!(
                "{PRETTIER_ROOT_ENV} must be an absolute directory, got {requested_root:?}"
            ));
        }
        let requested_metadata = std::fs::symlink_metadata(&requested_root).map_err(|error| {
            format!("inspect {PRETTIER_ROOT_ENV} root {requested_root:?}: {error}")
        })?;
        if !requested_metadata.is_dir() || requested_metadata.file_type().is_symlink() {
            return Err(format!(
                "{PRETTIER_ROOT_ENV} must name a real directory, got {requested_root:?}"
            ));
        }
        let root = requested_root.canonicalize().map_err(|error| {
            format!("canonicalize {PRETTIER_ROOT_ENV} root {requested_root:?}: {error}")
        })?;
        let node = require_executable(&root.join("node/bin/node"), "Prettier Node runtime")?;
        let cli = require_readable_file(
            &root.join("package/node_modules/prettier/bin/prettier.cjs"),
            "Prettier JavaScript CLI",
        )?;
        for (label, path) in [("Node runtime", &node), ("Prettier CLI", &cli)] {
            if !path.starts_with(&root) {
                return Err(format!(
                    "managed Prettier {label} escapes {PRETTIER_ROOT_ENV} {root:?}: {path:?}"
                ));
            }
        }

        let package_path = root.join("package/package.json");
        let lock_path = root.join("package/package-lock.json");
        let package: JsonValue = serde_json::from_slice(
            &std::fs::read(&package_path)
                .map_err(|error| format!("read managed Prettier package manifest: {error}"))?,
        )
        .map_err(|error| format!("parse managed Prettier package manifest: {error}"))?;
        if package["engines"]["node"] != "24.19.0" || package["dependencies"]["prettier"] != "3.9.6"
        {
            return Err(format!(
                "managed Prettier package manifest does not pin Node 24.19.0 and Prettier 3.9.6: {package_path:?}"
            ));
        }
        let lock: JsonValue = serde_json::from_slice(
            &std::fs::read(&lock_path)
                .map_err(|error| format!("read managed Prettier npm lock: {error}"))?,
        )
        .map_err(|error| format!("parse managed Prettier npm lock: {error}"))?;
        let locked = &lock["packages"]["node_modules/prettier"];
        if lock["lockfileVersion"] != 3
            || lock["packages"][""]["dependencies"]["prettier"] != "3.9.6"
            || locked["version"] != "3.9.6"
            || locked["resolved"] != "https://registry.npmjs.org/prettier/-/prettier-3.9.6.tgz"
            || locked["integrity"]
                != "sha512-OpN0zzVdiaiAhxpuuj5efpIS4sY9j7bY6uR5mnj5yPzGkdkjNKSJeUThPb60Jw29QuAZgA4o+/iB49kFiaBX6g=="
        {
            return Err(format!(
                "managed Prettier npm lock does not contain the exact 3.9.6 registry artifact: {lock_path:?}"
            ));
        }

        let identity_path = root.join(".velvet-glove-artifacts.json");
        let identity: JsonValue = serde_json::from_slice(
            &std::fs::read(&identity_path)
                .map_err(|error| format!("read managed Prettier identity: {error}"))?,
        )
        .map_err(|error| format!("parse managed Prettier identity: {error}"))?;
        if identity["node"]["id"] != "prettier-node"
            || identity["node"]["version"] != "24.19.0"
            || identity["node"]["integrity"]["sha256"]
                != "8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d"
            || identity["npm"]["id"] != "prettier-npm"
            || identity["npm"]["version"] != "11.17.0"
            || identity["prettier"]["version"] != "3.9.6"
        {
            return Err(format!(
                "managed Prettier identity does not bind the declared Node/npm/Prettier closure: {identity_path:?}"
            ));
        }
        Ok(Self { root, node, cli })
    }
}

#[derive(Clone)]
struct EslintToolchain {
    root: PathBuf,
    node: PathBuf,
    cli: PathBuf,
}

impl EslintToolchain {
    fn resolve_if_configured() -> Result<Option<Self>, String> {
        let Some(requested_root) = std::env::var_os(ESLINT_ROOT_ENV) else {
            return Ok(None);
        };
        Self::resolve(PathBuf::from(requested_root)).map(Some)
    }

    fn resolve(requested_root: PathBuf) -> Result<Self, String> {
        if !requested_root.is_absolute() {
            return Err(format!(
                "{ESLINT_ROOT_ENV} must be an absolute directory, got {requested_root:?}"
            ));
        }
        let requested_metadata = std::fs::symlink_metadata(&requested_root).map_err(|error| {
            format!("inspect {ESLINT_ROOT_ENV} root {requested_root:?}: {error}")
        })?;
        if !requested_metadata.is_dir() || requested_metadata.file_type().is_symlink() {
            return Err(format!(
                "{ESLINT_ROOT_ENV} must name a real directory, got {requested_root:?}"
            ));
        }
        let root = requested_root.canonicalize().map_err(|error| {
            format!("canonicalize {ESLINT_ROOT_ENV} root {requested_root:?}: {error}")
        })?;
        let node = require_executable(&root.join("node/bin/node"), "ESLint Node runtime")?;
        let cli = require_readable_file(
            &root.join("package/node_modules/eslint/bin/eslint.js"),
            "ESLint JavaScript CLI",
        )?;
        for (label, path) in [("Node runtime", &node), ("ESLint CLI", &cli)] {
            if !path.starts_with(&root) {
                return Err(format!(
                    "managed ESLint {label} escapes {ESLINT_ROOT_ENV} {root:?}: {path:?}"
                ));
            }
        }

        let package_path = root.join("package/package.json");
        let lock_path = root.join("package/package-lock.json");
        let package: JsonValue = serde_json::from_slice(
            &std::fs::read(&package_path)
                .map_err(|error| format!("read managed ESLint package manifest: {error}"))?,
        )
        .map_err(|error| format!("parse managed ESLint package manifest: {error}"))?;
        if package["engines"]["node"] != "24.19.0"
            || package["engines"]["npm"] != "11.17.0"
            || package["dependencies"]["eslint"] != "10.8.1"
        {
            return Err(format!(
                "managed ESLint package manifest does not pin Node 24.19.0, npm 11.17.0, and ESLint 10.8.1: {package_path:?}"
            ));
        }
        let lock: JsonValue = serde_json::from_slice(
            &std::fs::read(&lock_path)
                .map_err(|error| format!("read managed ESLint npm lock: {error}"))?,
        )
        .map_err(|error| format!("parse managed ESLint npm lock: {error}"))?;
        let locked = &lock["packages"]["node_modules/eslint"];
        if lock["lockfileVersion"] != 3
            || lock["packages"][""]["engines"]["node"] != "24.19.0"
            || lock["packages"][""]["engines"]["npm"] != "11.17.0"
            || lock["packages"][""]["dependencies"]["eslint"] != "10.8.1"
            || locked["version"] != "10.8.1"
            || locked["resolved"] != "https://registry.npmjs.org/eslint/-/eslint-10.8.1.tgz"
            || locked["integrity"]
                != "sha512-wqA7W2jbsC/BnV9Iv1UZpKVFkO1AdNoSmYW8NWG4HNOBbkAMvIqDZ27pI2f07dqn583NcIC44ckjAcOXDL1QbQ=="
        {
            return Err(format!(
                "managed ESLint npm lock does not contain the exact 10.8.1 registry artifact: {lock_path:?}"
            ));
        }

        let identity_path = root.join(".velvet-glove-artifacts.json");
        let identity: JsonValue = serde_json::from_slice(
            &std::fs::read(&identity_path)
                .map_err(|error| format!("read managed ESLint identity: {error}"))?,
        )
        .map_err(|error| format!("parse managed ESLint identity: {error}"))?;
        if identity["node"]["id"] != "eslint-node"
            || identity["node"]["version"] != "24.19.0"
            || identity["node"]["integrity"]["sha256"]
                != "8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d"
            || identity["npm"]["id"] != "eslint-npm"
            || identity["npm"]["version"] != "11.17.0"
            || identity["eslint"]["version"] != "10.8.1"
        {
            return Err(format!(
                "managed ESLint identity does not bind the declared Node/npm/ESLint closure: {identity_path:?}"
            ));
        }
        Ok(Self { root, node, cli })
    }
}

#[derive(Clone)]
struct ContextlintToolchain {
    root: PathBuf,
    node: PathBuf,
    cli: PathBuf,
}

impl ContextlintToolchain {
    fn resolve_if_configured() -> Result<Option<Self>, String> {
        let Some(requested_root) = std::env::var_os(CONTEXTLINT_ROOT_ENV) else {
            return Ok(None);
        };
        Self::resolve(PathBuf::from(requested_root)).map(Some)
    }

    fn resolve(requested_root: PathBuf) -> Result<Self, String> {
        if !requested_root.is_absolute() {
            return Err(format!(
                "{CONTEXTLINT_ROOT_ENV} must be an absolute directory, got {requested_root:?}"
            ));
        }
        let requested_metadata = std::fs::symlink_metadata(&requested_root).map_err(|error| {
            format!("inspect {CONTEXTLINT_ROOT_ENV} root {requested_root:?}: {error}")
        })?;
        if !requested_metadata.is_dir() || requested_metadata.file_type().is_symlink() {
            return Err(format!(
                "{CONTEXTLINT_ROOT_ENV} must name a real directory, got {requested_root:?}"
            ));
        }
        let root = requested_root.canonicalize().map_err(|error| {
            format!("canonicalize {CONTEXTLINT_ROOT_ENV} root {requested_root:?}: {error}")
        })?;
        let node = require_executable(&root.join("node/bin/node"), "Contextlint Node runtime")?;
        let cli = require_readable_file(
            &root.join("package/node_modules/@contextlint/cli/dist/index.js"),
            "Contextlint JavaScript CLI",
        )?;
        for (label, path) in [("Node runtime", &node), ("Contextlint CLI", &cli)] {
            if !path.starts_with(&root) {
                return Err(format!(
                    "managed Contextlint {label} escapes {CONTEXTLINT_ROOT_ENV} {root:?}: {path:?}"
                ));
            }
        }

        let package_path = root.join("package/package.json");
        let lock_path = root.join("package/package-lock.json");
        let package: JsonValue = serde_json::from_slice(
            &std::fs::read(&package_path)
                .map_err(|error| format!("read managed Contextlint package manifest: {error}"))?,
        )
        .map_err(|error| format!("parse managed Contextlint package manifest: {error}"))?;
        if package["engines"]["node"] != "24.19.0"
            || package["dependencies"]["@contextlint/cli"] != "1.1.1"
            || package["dependencies"]["@contextlint/core"] != "1.1.1"
        {
            return Err(format!(
                "managed Contextlint package manifest does not pin Node 24.19.0 and the CLI/core 1.1.1 pair: {package_path:?}"
            ));
        }
        let lock: JsonValue = serde_json::from_slice(
            &std::fs::read(&lock_path)
                .map_err(|error| format!("read managed Contextlint npm lock: {error}"))?,
        )
        .map_err(|error| format!("parse managed Contextlint npm lock: {error}"))?;
        let cli_lock = &lock["packages"]["node_modules/@contextlint/cli"];
        let core_lock = &lock["packages"]["node_modules/@contextlint/core"];
        if lock["lockfileVersion"] != 3
            || lock["packages"][""]["engines"]["node"] != "24.19.0"
            || cli_lock["version"] != "1.1.1"
            || cli_lock["resolved"] != "https://registry.npmjs.org/@contextlint/cli/-/cli-1.1.1.tgz"
            || cli_lock["integrity"]
                != "sha512-QCyjqmdaoanH9L8AduX2jH7vRm2yryHpxroLai0PHHP2lijBTG96UEICCuSIHbkoQ4FXulrokQst5+eTf34v9g=="
            || core_lock["version"] != "1.1.1"
            || core_lock["resolved"]
                != "https://registry.npmjs.org/@contextlint/core/-/core-1.1.1.tgz"
            || core_lock["integrity"]
                != "sha512-ui2ymL90ZlV260NZD8pgki6fwCUM1bX2wj1LbDy5H4u7w8JyTvxIBORxzhWlklDUmsXf1wVxIZXdbvuRYRsqfQ=="
        {
            return Err(format!(
                "managed Contextlint npm lock does not contain the exact CLI/core 1.1.1 registry artifacts: {lock_path:?}"
            ));
        }

        let identity_path = root.join(".velvet-glove-artifacts.json");
        let identity: JsonValue = serde_json::from_slice(
            &std::fs::read(&identity_path)
                .map_err(|error| format!("read managed Contextlint identity: {error}"))?,
        )
        .map_err(|error| format!("parse managed Contextlint identity: {error}"))?;
        if identity["node"]["id"] != "contextlint-node"
            || identity["node"]["version"] != "24.19.0"
            || identity["node"]["integrity"]["sha256"]
                != "8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d"
            || identity["npm"]["id"] != "contextlint-npm"
            || identity["npm"]["version"] != "11.17.0"
            || identity["contextlint"]["version"] != "1.1.1"
            || identity["contextlint"]["cliVersion"] != "1.1.1"
            || identity["contextlint"]["coreVersion"] != "1.1.1"
        {
            return Err(format!(
                "managed Contextlint identity does not bind the declared Node/npm/CLI/core closure: {identity_path:?}"
            ));
        }
        Ok(Self { root, node, cli })
    }
}

#[derive(Clone)]
struct DclintToolchain {
    root: PathBuf,
    node_bin: PathBuf,
    package_bin: PathBuf,
    node: PathBuf,
    cli: PathBuf,
}

impl DclintToolchain {
    fn resolve_if_configured() -> Result<Option<Self>, String> {
        let Some(requested_root) = std::env::var_os(DCLINT_ROOT_ENV) else {
            return Ok(None);
        };
        Self::resolve(PathBuf::from(requested_root)).map(Some)
    }

    fn resolve(requested_root: PathBuf) -> Result<Self, String> {
        if !requested_root.is_absolute() {
            return Err(format!(
                "{DCLINT_ROOT_ENV} must be an absolute directory, got {requested_root:?}"
            ));
        }
        let requested_metadata = std::fs::symlink_metadata(&requested_root).map_err(|error| {
            format!("inspect {DCLINT_ROOT_ENV} root {requested_root:?}: {error}")
        })?;
        if !requested_metadata.is_dir() || requested_metadata.file_type().is_symlink() {
            return Err(format!(
                "{DCLINT_ROOT_ENV} must name a real directory, got {requested_root:?}"
            ));
        }
        let root = requested_root.canonicalize().map_err(|error| {
            format!("canonicalize {DCLINT_ROOT_ENV} root {requested_root:?}: {error}")
        })?;
        let node_bin = root.join("node/bin");
        let package_bin = root.join("package/node_modules/.bin");
        let node = require_executable(&node_bin.join("node"), "dclint Node runtime")?;
        let requested_cli = package_bin.join("dclint");
        let cli_metadata = std::fs::symlink_metadata(&requested_cli)
            .map_err(|error| format!("inspect managed dclint bin link: {error}"))?;
        if !cli_metadata.file_type().is_symlink()
            || std::fs::read_link(&requested_cli)
                .map_err(|error| format!("read managed dclint bin link: {error}"))?
                != Path::new("../dclint/bin/dclint.cjs")
        {
            return Err(format!(
                "managed dclint bin link does not bind the declared CLI: {requested_cli:?}"
            ));
        }
        let cli = require_executable(&requested_cli, "dclint JavaScript CLI")?;
        for (label, path) in [("Node runtime", &node), ("dclint CLI", &cli)] {
            if !path.starts_with(&root) {
                return Err(format!(
                    "managed dclint {label} escapes {DCLINT_ROOT_ENV} {root:?}: {path:?}"
                ));
            }
        }

        let package_path = root.join("package/package.json");
        let lock_path = root.join("package/package-lock.json");
        let package_bytes = std::fs::read(&package_path)
            .map_err(|error| format!("read managed dclint package manifest: {error}"))?;
        let lock_bytes = std::fs::read(&lock_path)
            .map_err(|error| format!("read managed dclint npm lock: {error}"))?;
        let checked_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hookkit-pkl-config/validation/provisioning/dclint");
        if package_bytes
            != std::fs::read(checked_root.join("package.json"))
                .map_err(|error| format!("read checked dclint package manifest: {error}"))?
            || lock_bytes
                != std::fs::read(checked_root.join("package-lock.json"))
                    .map_err(|error| format!("read checked dclint npm lock: {error}"))?
        {
            return Err(
                "managed dclint package manifest or lock differs from checked bytes".to_owned(),
            );
        }
        let package: JsonValue = serde_json::from_slice(&package_bytes)
            .map_err(|error| format!("parse managed dclint package manifest: {error}"))?;
        if package["engines"]["node"] != "24.19.0" || package["dependencies"]["dclint"] != "3.1.0" {
            return Err(format!(
                "managed dclint package manifest does not pin Node 24.19.0 and dclint 3.1.0: {package_path:?}"
            ));
        }
        let lock: JsonValue = serde_json::from_slice(&lock_bytes)
            .map_err(|error| format!("parse managed dclint npm lock: {error}"))?;
        let locked = &lock["packages"]["node_modules/dclint"];
        if lock["lockfileVersion"] != 3
            || lock["packages"][""]["dependencies"]["dclint"] != "3.1.0"
            || locked["version"] != "3.1.0"
            || locked["resolved"] != "https://registry.npmjs.org/dclint/-/dclint-3.1.0.tgz"
            || locked["integrity"]
                != "sha512-afTGdzRFUXK4yCpIiEW/LOR+9TOMEDhNldDp56VCWzn7JDmD451PcUi640GGlMHgbHKJ10rDBm4PtpcBbjqlXw=="
        {
            return Err(format!(
                "managed dclint npm lock does not contain the exact 3.1.0 registry artifact: {lock_path:?}"
            ));
        }

        let identity_path = root.join(".velvet-glove-artifacts.json");
        let identity: JsonValue = serde_json::from_slice(
            &std::fs::read(&identity_path)
                .map_err(|error| format!("read managed dclint identity: {error}"))?,
        )
        .map_err(|error| format!("parse managed dclint identity: {error}"))?;
        if identity["node"]["id"] != "dclint-node"
            || identity["node"]["version"] != "24.19.0"
            || identity["node"]["integrity"]["sha256"]
                != "8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d"
            || identity["npm"]["id"] != "dclint-npm"
            || identity["npm"]["version"] != "11.17.0"
            || identity["dclint"]["version"] != "3.1.0"
            || identity["dclint"]["packageSha256"]
                != "8d1f39c4caea61e2593b426ce230008a1374c42d10130306d026c14cd904c1a9"
            || identity["dclint"]["packageLockSha256"]
                != "58dba76cffb45147c65cfdd791543e443097a88c4e69ed8911c2ee159f79f9a8"
        {
            return Err(format!(
                "managed dclint identity does not bind the declared Node/npm/dclint closure: {identity_path:?}"
            ));
        }
        Ok(Self {
            root,
            node_bin,
            package_bin,
            node,
            cli,
        })
    }
}

fn require_readable_file(path: &Path, description: &str) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("inspect {description} {path:?}: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("{description} is not a real file: {path:?}"));
    }
    std::fs::File::open(path).map_err(|error| format!("open {description} {path:?}: {error}"))?;
    path.canonicalize()
        .map_err(|error| format!("resolve {description} {path:?}: {error}"))
}

struct CargoClippyToolchain {
    root: PathBuf,
    bin: PathBuf,
    library: PathBuf,
    cargo: PathBuf,
    rustc: PathBuf,
    rustdoc: PathBuf,
    cargo_clippy: PathBuf,
    clippy_driver: PathBuf,
    cargo_home: PathBuf,
    temporary: PathBuf,
}

impl CargoClippyToolchain {
    fn resolve() -> Result<Self, String> {
        let requested_root =
            std::env::var_os(CARGO_CLIPPY_TOOLCHAIN_ROOT_ENV).ok_or_else(|| {
                format!("cargo-clippy real-tool fixtures require {CARGO_CLIPPY_TOOLCHAIN_ROOT_ENV}")
            })?;
        let requested_root = PathBuf::from(requested_root);
        if !requested_root.is_absolute() {
            return Err(format!(
                "{CARGO_CLIPPY_TOOLCHAIN_ROOT_ENV} must be an absolute directory, got {requested_root:?}"
            ));
        }
        let root = requested_root.canonicalize().map_err(|error| {
            format!(
                "canonicalize {CARGO_CLIPPY_TOOLCHAIN_ROOT_ENV} root {requested_root:?}: {error}"
            )
        })?;
        let metadata = std::fs::metadata(&root)
            .map_err(|error| format!("inspect cargo-clippy toolchain root {root:?}: {error}"))?;
        if !metadata.is_dir() {
            return Err(format!(
                "cargo-clippy toolchain root is not a directory: {root:?}"
            ));
        }
        let bin = root.join("bin");
        let library = root.join("lib");
        if !library.is_dir() {
            return Err(format!(
                "cargo-clippy toolchain root lacks its library directory: {library:?}"
            ));
        }
        let cargo = require_executable(&bin.join("cargo"), "cargo-clippy cargo")?;
        let rustc = require_executable(&bin.join("rustc"), "cargo-clippy rustc")?;
        let rustdoc = require_executable(&bin.join("rustdoc"), "cargo-clippy rustdoc")?;
        let cargo_clippy = require_executable(&bin.join("cargo-clippy"), "cargo-clippy driver")?;
        let clippy_driver =
            require_executable(&bin.join("clippy-driver"), "Clippy compiler driver")?;
        let cargo_home = PathBuf::from(std::env::var_os(CARGO_HOME_ENV).ok_or_else(|| {
            format!("cargo-clippy real-tool fixtures require controlled {CARGO_HOME_ENV}")
        })?);
        if !cargo_home.is_absolute() {
            return Err(format!(
                "cargo-clippy real-tool fixtures require an absolute {CARGO_HOME_ENV}, got {cargo_home:?}"
            ));
        }
        let cargo_home = cargo_home.canonicalize().map_err(|error| {
            format!("canonicalize controlled {CARGO_HOME_ENV} {cargo_home:?}: {error}")
        })?;
        for config_name in ["config", "config.toml"] {
            let config = cargo_home.join(config_name);
            match std::fs::symlink_metadata(&config) {
                Ok(_) => {
                    return Err(format!(
                        "cargo-clippy real-tool fixtures reject ambient Cargo configuration at {config:?}"
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "inspect controlled Cargo configuration path {config:?}: {error}"
                    ));
                }
            }
        }
        let temporary = PathBuf::from(std::env::var_os(TMPDIR_ENV).ok_or_else(|| {
            format!("cargo-clippy real-tool fixtures require controlled {TMPDIR_ENV}")
        })?);
        if !temporary.is_absolute() {
            return Err(format!(
                "cargo-clippy real-tool fixtures require an absolute {TMPDIR_ENV}, got {temporary:?}"
            ));
        }
        let temporary = temporary.canonicalize().map_err(|error| {
            format!("canonicalize controlled {TMPDIR_ENV} {temporary:?}: {error}")
        })?;
        if !temporary.is_dir() {
            return Err(format!(
                "cargo-clippy real-tool fixture {TMPDIR_ENV} is not a directory: {temporary:?}"
            ));
        }
        Ok(Self {
            root,
            bin,
            library,
            cargo,
            rustc,
            rustdoc,
            cargo_clippy,
            clippy_driver,
            cargo_home,
            temporary,
        })
    }
}

struct CargoFmtToolchain {
    root: PathBuf,
    bin: PathBuf,
    library: PathBuf,
    cargo: PathBuf,
    cargo_fmt: PathBuf,
    rustfmt: PathBuf,
    rustc: PathBuf,
    cargo_home: PathBuf,
    temporary: PathBuf,
}

impl CargoFmtToolchain {
    fn resolve() -> Result<Self, String> {
        let requested_root =
            std::env::var_os(CARGO_CLIPPY_TOOLCHAIN_ROOT_ENV).ok_or_else(|| {
                format!("cargo-fmt real-tool fixtures require {CARGO_CLIPPY_TOOLCHAIN_ROOT_ENV}")
            })?;
        let requested_root = PathBuf::from(requested_root);
        if !requested_root.is_absolute() {
            return Err(format!(
                "{CARGO_CLIPPY_TOOLCHAIN_ROOT_ENV} must be an absolute directory, got {requested_root:?}"
            ));
        }
        let root = requested_root.canonicalize().map_err(|error| {
            format!(
                "canonicalize {CARGO_CLIPPY_TOOLCHAIN_ROOT_ENV} root {requested_root:?}: {error}"
            )
        })?;
        if !root.is_dir() {
            return Err(format!(
                "cargo-fmt toolchain root is not a directory: {root:?}"
            ));
        }
        let bin = root.join("bin");
        let library = root.join("lib");
        if !library.is_dir() {
            return Err(format!(
                "cargo-fmt toolchain root lacks its library directory: {library:?}"
            ));
        }
        let cargo = require_executable(&bin.join("cargo"), "cargo-fmt cargo")?;
        let cargo_fmt = require_executable(&bin.join("cargo-fmt"), "cargo-fmt driver")?;
        let rustfmt = require_executable(&bin.join("rustfmt"), "rustfmt driver")?;
        let rustc = require_executable(&bin.join("rustc"), "cargo-fmt rustc")?;
        let cargo_home = PathBuf::from(std::env::var_os(CARGO_HOME_ENV).ok_or_else(|| {
            format!("cargo-fmt real-tool fixtures require controlled {CARGO_HOME_ENV}")
        })?);
        if !cargo_home.is_absolute() {
            return Err(format!(
                "cargo-fmt real-tool fixtures require an absolute {CARGO_HOME_ENV}, got {cargo_home:?}"
            ));
        }
        let cargo_home = cargo_home.canonicalize().map_err(|error| {
            format!("canonicalize controlled {CARGO_HOME_ENV} {cargo_home:?}: {error}")
        })?;
        for config_name in ["config", "config.toml"] {
            let config = cargo_home.join(config_name);
            match std::fs::symlink_metadata(&config) {
                Ok(_) => {
                    return Err(format!(
                        "cargo-fmt real-tool fixtures reject ambient Cargo configuration at {config:?}"
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "inspect controlled Cargo configuration path {config:?}: {error}"
                    ));
                }
            }
        }
        let temporary = PathBuf::from(std::env::var_os(TMPDIR_ENV).ok_or_else(|| {
            format!("cargo-fmt real-tool fixtures require controlled {TMPDIR_ENV}")
        })?);
        if !temporary.is_absolute() {
            return Err(format!(
                "cargo-fmt real-tool fixtures require an absolute {TMPDIR_ENV}, got {temporary:?}"
            ));
        }
        let temporary = temporary.canonicalize().map_err(|error| {
            format!("canonicalize controlled {TMPDIR_ENV} {temporary:?}: {error}")
        })?;
        if !temporary.is_dir() {
            return Err(format!(
                "cargo-fmt real-tool fixture {TMPDIR_ENV} is not a directory: {temporary:?}"
            ));
        }
        Ok(Self {
            root,
            bin,
            library,
            cargo,
            cargo_fmt,
            rustfmt,
            rustc,
            cargo_home,
            temporary,
        })
    }
}

fn require_executable(path: &Path, description: &str) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("resolve {description} executable {path:?}: {error}"))?;
    let metadata = std::fs::metadata(&canonical)
        .map_err(|error| format!("inspect {description} executable {canonical:?}: {error}"))?;
    if !metadata.is_file() {
        return Err(format!("{description} is not a file: {canonical:?}"));
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!("{description} is not executable: {canonical:?}"));
    }
    Ok(canonical)
}

struct ToolTraceHarness {
    shim_dir: PathBuf,
    trace_root: PathBuf,
    programs: BTreeMap<String, PathBuf>,
    cargo_clippy_toolchain: Option<CargoClippyToolchain>,
    cargo_fmt_toolchain: Option<CargoFmtToolchain>,
    prettier_toolchain: Option<PrettierToolchain>,
    eslint_toolchain: Option<EslintToolchain>,
    contextlint_toolchain: Option<ContextlintToolchain>,
    contextlint: bool,
    dclint_toolchain: Option<DclintToolchain>,
    eslint: bool,
}

impl ToolTraceHarness {
    fn prepare(
        case: &FixtureCase,
        workspace: &FixtureWorkspace,
        logical_programs: &BTreeSet<String>,
    ) -> Result<Self, String> {
        if logical_programs.is_empty() {
            return Err(format!("{} real-tool trace has no programs", case.tool));
        }
        for logical_program in logical_programs {
            if Path::new(logical_program).components().count() != 1 || logical_program.is_empty() {
                return Err(format!(
                    "real-tool trace requires bare logical program names, got {logical_program:?}"
                ));
            }
        }
        let cargo_clippy_toolchain =
            (case.tool == "cargo-clippy").then(CargoClippyToolchain::resolve);
        let cargo_clippy_toolchain = cargo_clippy_toolchain.transpose()?;
        let cargo_fmt_toolchain = (case.tool == "cargo-fmt").then(CargoFmtToolchain::resolve);
        let cargo_fmt_toolchain = cargo_fmt_toolchain.transpose()?;
        let prettier_toolchain = if case.tool == "prettier" {
            PrettierToolchain::resolve_if_configured()?
        } else {
            None
        };
        let eslint_toolchain = if case.tool == "eslint" {
            EslintToolchain::resolve_if_configured()?
        } else {
            None
        };
        let contextlint_toolchain = if case.tool == "contextlint" {
            ContextlintToolchain::resolve_if_configured()?
        } else {
            None
        };
        let dclint_toolchain = if case.tool == "dclint" {
            DclintToolchain::resolve_if_configured()?
        } else {
            None
        };
        let mut programs = BTreeMap::new();
        for logical_program in logical_programs {
            let real_program = match logical_program.as_str() {
                "cargo" if cargo_fmt_toolchain.is_some() => cargo_fmt_toolchain
                    .as_ref()
                    .expect("checked cargo-fmt toolchain")
                    .cargo
                    .clone(),
                "cargo-fmt" if cargo_fmt_toolchain.is_some() => cargo_fmt_toolchain
                    .as_ref()
                    .expect("checked cargo-fmt toolchain")
                    .cargo_fmt
                    .clone(),
                "rustfmt" if cargo_fmt_toolchain.is_some() => cargo_fmt_toolchain
                    .as_ref()
                    .expect("checked cargo-fmt toolchain")
                    .rustfmt
                    .clone(),
                "cargo" if cargo_clippy_toolchain.is_some() => cargo_clippy_toolchain
                    .as_ref()
                    .expect("checked cargo-clippy toolchain")
                    .cargo
                    .clone(),
                "cargo-clippy" if cargo_clippy_toolchain.is_some() => cargo_clippy_toolchain
                    .as_ref()
                    .expect("checked cargo-clippy toolchain")
                    .cargo_clippy
                    .clone(),
                "node" if prettier_toolchain.is_some() => prettier_toolchain
                    .as_ref()
                    .expect("checked Prettier toolchain")
                    .node
                    .clone(),
                "node" if eslint_toolchain.is_some() => eslint_toolchain
                    .as_ref()
                    .expect("checked ESLint toolchain")
                    .node
                    .clone(),
                "node" if contextlint_toolchain.is_some() => contextlint_toolchain
                    .as_ref()
                    .expect("checked Contextlint toolchain")
                    .node
                    .clone(),
                "node" if dclint_toolchain.is_some() => dclint_toolchain
                    .as_ref()
                    .expect("checked dclint toolchain")
                    .node
                    .clone(),
                "dclint" if dclint_toolchain.is_some() => dclint_toolchain
                    .as_ref()
                    .expect("checked dclint toolchain")
                    .cli
                    .clone(),
                _ => resolve_program(logical_program).ok_or_else(|| {
                    format!("contract could not resolve pinned {logical_program} before tracing")
                })?,
            };
            let real_program = real_program.canonicalize().map_err(|error| {
                format!("canonicalize contract executable {real_program:?}: {error}")
            })?;
            programs.insert(logical_program.clone(), real_program);
        }
        let shim_dir = workspace.root.join("tool-shim");
        let trace_root = workspace.root.join("tool-traces");
        std::fs::create_dir_all(&shim_dir)
            .map_err(|error| format!("create tool shim directory {shim_dir:?}: {error}"))?;
        std::fs::create_dir_all(&trace_root)
            .map_err(|error| format!("create tool trace directory {trace_root:?}: {error}"))?;
        if logical_programs.contains("buf") {
            let diff = Path::new(BUF_DIFF_PROGRAM);
            let metadata = std::fs::metadata(diff).map_err(|error| {
                format!("inspect Buf adapter diff prerequisite {diff:?}: {error}")
            })?;
            if !metadata.is_file() {
                return Err(format!(
                    "Buf trace requires the adapter's fixed diff executable at {BUF_DIFF_PROGRAM}"
                ));
            }
            #[cfg(unix)]
            if metadata.permissions().mode() & 0o111 == 0 {
                return Err(format!(
                    "Buf trace requires an executable adapter diff prerequisite at {BUF_DIFF_PROGRAM}"
                ));
            }
            for directory in ["home", "tmp/velvet-glove-buf-cache", "xdg-cache"] {
                let path = workspace.root.join(directory);
                std::fs::create_dir_all(&path).map_err(|error| {
                    format!("create controlled Buf environment directory {path:?}: {error}")
                })?;
            }
        }
        let contextlint = case.tool == "contextlint";
        let eslint = case.tool == "eslint";
        if eslint {
            for directory in [
                "eslint-tmp",
                "eslint-outer-poison-home",
                "eslint-outer-poison-cache",
            ] {
                let path = workspace.root.join(directory);
                std::fs::create_dir_all(&path).map_err(|error| {
                    format!("create controlled ESLint environment directory {path:?}: {error}")
                })?;
            }
        }
        if contextlint {
            for directory in [
                "contextlint-tmp",
                "contextlint-outer-poison-home",
                "contextlint-outer-poison-cache",
            ] {
                let path = workspace.root.join(directory);
                std::fs::create_dir_all(&path).map_err(|error| {
                    format!("create controlled Contextlint environment directory {path:?}: {error}")
                })?;
            }
        }
        if logical_programs.contains("dclint") {
            let temporary = workspace.root.join("tmp");
            std::fs::create_dir_all(&temporary).map_err(|error| {
                format!("create controlled dclint temporary directory {temporary:?}: {error}")
            })?;
        }
        for (logical_program, real_program) in &programs {
            let shim = shim_dir.join(logical_program);
            std::fs::write(&shim, include_bytes!("support/tool-trace.sh"))
                .map_err(|error| format!("write tool trace shim {shim:?}: {error}"))?;
            std::fs::write(
                shim_dir.join(format!("{logical_program}.real-program")),
                format!("{}\n", real_program.display()),
            )
            .map_err(|error| format!("write {logical_program} trace program binding: {error}"))?;
            #[cfg(unix)]
            {
                let mut permissions = std::fs::metadata(&shim)
                    .map_err(|error| format!("tool trace shim metadata {shim:?}: {error}"))?
                    .permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(&shim, permissions).map_err(|error| {
                    format!("make tool trace shim executable {shim:?}: {error}")
                })?;
            }
        }
        Ok(Self {
            shim_dir,
            trace_root,
            programs,
            cargo_clippy_toolchain,
            cargo_fmt_toolchain,
            prettier_toolchain,
            eslint_toolchain,
            contextlint_toolchain,
            contextlint,
            dclint_toolchain,
            eslint,
        })
    }

    fn configure(&self, command: &mut Command, label: &str) -> Result<(), String> {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let mut path_entries = vec![self.shim_dir.clone()];
        if let Some(toolchain) = &self.cargo_clippy_toolchain {
            path_entries.push(toolchain.bin.clone());
        }
        if let Some(toolchain) = &self.cargo_fmt_toolchain {
            path_entries.push(toolchain.bin.clone());
        }
        if let Some(toolchain) = &self.dclint_toolchain {
            path_entries.push(toolchain.package_bin.clone());
            path_entries.push(toolchain.node_bin.clone());
        }
        path_entries.extend(std::env::split_paths(&inherited));
        let path = std::env::join_paths(path_entries)
            .map_err(|error| format!("construct tool trace PATH: {error}"))?;
        let trace_dir = self.trace_root.join(label);
        std::fs::create_dir_all(&trace_dir)
            .map_err(|error| format!("create tool trace attempt {trace_dir:?}: {error}"))?;
        command
            .env("PATH", path)
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .env("NO_COLOR", "1")
            .env("CLICOLOR", "0")
            .env("FORCE_COLOR", "0")
            .env(TOOL_TRACE_DIR_ENV, trace_dir)
            .env(TOOL_TRACE_SENTINEL_ENV, TOOL_TRACE_SENTINEL);
        if self.programs.contains_key("betterleaks") {
            for name in [
                BETTERLEAKS_CONFIG_ENV,
                BETTERLEAKS_CONFIG_TOML_ENV,
                GITLEAKS_CONFIG_ENV,
                GITLEAKS_CONFIG_TOML_ENV,
            ] {
                command.env(name, BETTERLEAKS_POISON_ENV_VALUE);
            }
        }
        if self.programs.contains_key("vacuum") {
            for name in VACUUM_PREFIX_SCRUBBED_ENV.iter().chain(VACUUM_SCRUBBED_ENV) {
                command.env(name, VACUUM_POISON_ENV_VALUE);
            }
            for name in VACUUM_LOADER_SCRUBBED_ENV {
                command.env_remove(name);
            }
        }
        if self.programs.contains_key("biome") {
            command.env(CI_ENV, BIOME_POISON_ENV_VALUE);
            command.env(RAYON_NUM_THREADS_ENV, BIOME_POISON_ENV_VALUE);
            for name in BIOME_SCRUBBED_ENV {
                command.env(name, BIOME_POISON_ENV_VALUE);
            }
        }
        if self.programs.contains_key("node") {
            for name in [
                "LANG",
                "LC_ALL",
                "TZ",
                "TERM",
                CI_ENV,
                "NO_COLOR",
                "CLICOLOR",
                "FORCE_COLOR",
            ] {
                command.env(name, PRETTIER_POISON_ENV_VALUE);
            }
            for name in PRETTIER_SCRUBBED_ENV {
                if name.starts_with("DYLD_") || name.starts_with("LD_") {
                    command.env_remove(name);
                } else {
                    command.env(name, PRETTIER_POISON_ENV_VALUE);
                }
            }
        }
        if self.eslint {
            let root = self.trace_root.parent().ok_or_else(|| {
                format!(
                    "ESLint trace root has no controlled environment parent: {:?}",
                    self.trace_root
                )
            })?;
            command
                .env(HOME_ENV, root.join("eslint-outer-poison-home"))
                .env(TMPDIR_ENV, root.join("eslint-tmp"))
                .env(XDG_CACHE_HOME_ENV, root.join("eslint-outer-poison-cache"))
                .env("TERM", ESLINT_POISON_ENV_VALUE)
                .env(CI_ENV, ESLINT_POISON_ENV_VALUE)
                .env("NODE_DISABLE_COLORS", ESLINT_POISON_ENV_VALUE)
                .env("UV_THREADPOOL_SIZE", ESLINT_POISON_ENV_VALUE);
            for name in ESLINT_SCRUBBED_ENV {
                if name.starts_with("DYLD_") || name.starts_with("LD_") {
                    command.env_remove(name);
                } else {
                    command.env(name, ESLINT_POISON_ENV_VALUE);
                }
            }
        }
        if self.contextlint {
            let root = self.trace_root.parent().ok_or_else(|| {
                format!(
                    "Contextlint trace root has no controlled environment parent: {:?}",
                    self.trace_root
                )
            })?;
            command
                .env(HOME_ENV, root.join("contextlint-outer-poison-home"))
                .env(TMPDIR_ENV, root.join("contextlint-tmp"))
                .env(
                    XDG_CACHE_HOME_ENV,
                    root.join("contextlint-outer-poison-cache"),
                )
                .env("TERM", CONTEXTLINT_POISON_ENV_VALUE)
                .env(CI_ENV, CONTEXTLINT_POISON_ENV_VALUE)
                .env("NODE_DISABLE_COLORS", CONTEXTLINT_POISON_ENV_VALUE)
                .env("UV_THREADPOOL_SIZE", CONTEXTLINT_POISON_ENV_VALUE);
            for name in CONTEXTLINT_POISONED_ENV {
                command.env(name, CONTEXTLINT_POISON_ENV_VALUE);
            }
        }
        if self.programs.contains_key("buf") {
            let root = self.trace_root.parent().ok_or_else(|| {
                format!(
                    "Buf trace root has no controlled environment parent: {:?}",
                    self.trace_root
                )
            })?;
            command
                .env(HOME_ENV, root.join("home"))
                .env(TMPDIR_ENV, root.join("tmp"))
                .env(XDG_CACHE_HOME_ENV, root.join("xdg-cache"))
                .env(DIFF_OPTIONS_ENV, BUF_POISON_ENV_VALUE)
                .env(BUF_CACHE_DIR_ENV, BUF_POISON_ENV_VALUE);
            for name in BUF_SCRUBBED_ENV {
                command.env(name, BUF_POISON_ENV_VALUE);
            }
        }
        if self.programs.contains_key("gofmt") {
            for (name, _) in GOFMT_CONTROLLED_ENV {
                command.env(name, GOFMT_POISON_ENV_VALUE);
            }
            for name in GOFMT_SCRUBBED_ENV {
                command.env(name, GOFMT_POISON_ENV_VALUE);
            }
            for name in GOFMT_LOADER_SCRUBBED_ENV {
                command.env_remove(name);
            }
        }
        if self.programs.contains_key("dclint") {
            let root = self.trace_root.parent().ok_or_else(|| {
                format!(
                    "dclint trace root has no controlled environment parent: {:?}",
                    self.trace_root
                )
            })?;
            let temporary = root.join("tmp").canonicalize().map_err(|error| {
                format!("canonicalize controlled dclint temporary directory: {error}")
            })?;
            command.env(TMPDIR_ENV, temporary);
            for name in DCLINT_SCRUBBED_ENV {
                command.env(name, DCLINT_POISON_ENV_VALUE);
            }
            for name in DCLINT_LOADER_SCRUBBED_ENV {
                command.env_remove(name);
            }
        }
        if let Some(toolchain) = &self.cargo_clippy_toolchain {
            command
                .env(DYLD_LIBRARY_PATH_ENV, CARGO_CLIPPY_POISON_ENV_VALUE)
                .env(CARGO_HOME_ENV, &toolchain.cargo_home)
                .env(CARGO_TARGET_DIR_ENV, CARGO_CLIPPY_POISON_ENV_VALUE)
                .env(CARGO_NET_OFFLINE_ENV, CARGO_CLIPPY_POISON_ENV_VALUE)
                .env(CARGO_BUILD_JOBS_ENV, CARGO_CLIPPY_POISON_ENV_VALUE)
                .env(CARGO_TERM_COLOR_ENV, CARGO_CLIPPY_POISON_ENV_VALUE)
                .env(CARGO_PROGRAM_ENV, CARGO_CLIPPY_POISON_ENV_VALUE)
                .env(RUSTC_ENV, CARGO_CLIPPY_POISON_ENV_VALUE)
                .env(RUSTDOC_ENV, CARGO_CLIPPY_POISON_ENV_VALUE);
            for name in CARGO_CLIPPY_EMPTY_ENV
                .iter()
                .chain(CARGO_CLIPPY_SCRUBBED_ENV)
                .chain(CARGO_CLIPPY_PREFIX_POISON_ENV)
            {
                command.env(name, CARGO_CLIPPY_POISON_ENV_VALUE);
            }
            for name in CARGO_CLIPPY_LOADER_SCRUBBED_ENV {
                command.env_remove(name);
            }
        }
        if let Some(toolchain) = &self.cargo_fmt_toolchain {
            command
                .env(DYLD_LIBRARY_PATH_ENV, CARGO_FMT_POISON_ENV_VALUE)
                .env(CARGO_HOME_ENV, &toolchain.cargo_home)
                .env(CARGO_TARGET_DIR_ENV, CARGO_FMT_POISON_ENV_VALUE)
                .env(CARGO_NET_OFFLINE_ENV, CARGO_FMT_POISON_ENV_VALUE)
                .env(CARGO_BUILD_JOBS_ENV, CARGO_FMT_POISON_ENV_VALUE)
                .env(CARGO_TERM_COLOR_ENV, CARGO_FMT_POISON_ENV_VALUE)
                .env(CARGO_PROGRAM_ENV, CARGO_FMT_POISON_ENV_VALUE)
                .env(RUSTC_ENV, CARGO_FMT_POISON_ENV_VALUE)
                .env(RUSTDOC_ENV, CARGO_FMT_POISON_ENV_VALUE)
                .env(RUSTFMT_ENV, CARGO_FMT_POISON_ENV_VALUE);
            for name in CARGO_FMT_EMPTY_ENV
                .iter()
                .chain(CARGO_FMT_SCRUBBED_ENV)
                .chain(CARGO_FMT_PREFIX_POISON_ENV)
            {
                command.env(name, CARGO_FMT_POISON_ENV_VALUE);
            }
            for name in CARGO_CLIPPY_LOADER_SCRUBBED_ENV {
                command.env_remove(name);
            }
        }
        Ok(())
    }
}

fn verify_tool_trace(
    harness: &ToolTraceHarness,
    label: &str,
    contract: &ResolvedContract,
    project: &Path,
    evidence_path: &Path,
) -> Result<(), String> {
    verify_tool_trace_invocations(
        harness,
        label,
        &contract.trace_invocations,
        project,
        evidence_path,
    )
}

fn verify_tool_trace_invocations(
    harness: &ToolTraceHarness,
    label: &str,
    expected_invocations: &[ResolvedTraceInvocation],
    project: &Path,
    evidence_path: &Path,
) -> Result<(), String> {
    let trace_dir = harness.trace_root.join(label).join("invocations");
    let invocations = if trace_dir.is_dir() {
        sorted_entries(&trace_dir)?
            .into_iter()
            .filter(|entry| entry.path().is_dir())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if invocations.len() != expected_invocations.len() {
        return Err(format!(
            "{label} expected {} tool invocation(s), observed {} at {trace_dir:?}",
            expected_invocations.len(),
            invocations.len()
        ));
    }

    let cwd = canonical_project(project);
    let mut records = Vec::new();
    let mut cargo_target_dir = None;
    let mut clippy_conf_dir = None;
    let mut dclint_private_configs = Vec::new();
    for (invocation, expected) in invocations.iter().zip(expected_invocations) {
        let trace_program = expected.program.as_str();
        let record = invocation.path();
        assert_record(&record, "logical-program", trace_program)?;
        for (name, expected) in [
            ("LANG", "C"),
            ("LC_ALL", "C"),
            ("TZ", "UTC"),
            ("NO_COLOR", "1"),
            ("CLICOLOR", "0"),
            ("FORCE_COLOR", "0"),
            (TOOL_TRACE_SENTINEL_ENV, TOOL_TRACE_SENTINEL),
        ] {
            assert_record(&record, &format!("env-{name}"), expected)?;
        }
        let real_program = harness.programs.get(trace_program).ok_or_else(|| {
            format!("{label} has no real executable binding for {trace_program:?}")
        })?;
        assert_record(
            &record,
            "real-program",
            real_program.to_string_lossy().as_ref(),
        )?;
        let recorded_cwd = read_record(&record, "cwd")?;
        let contextlint_private_root = if trace_program == "node" && harness.contextlint {
            resolve_contextlint_private_trace_root(&record, &expected.arguments, &recorded_cwd)?
        } else {
            None
        };
        let eslint_private_root = if trace_program == "node" && harness.eslint {
            resolve_eslint_private_trace_root(&record, &expected.arguments)?
        } else {
            None
        };
        let private_state = if let Some(private_root) = eslint_private_root.as_deref() {
            verify_eslint_private_trace_state(&record, harness, project, private_root)?
        } else {
            serde_json::json!({})
        };
        let ghalint_private_root = if trace_program == "ghalint" {
            resolve_ghalint_private_trace_root(&record, &expected.arguments)?
        } else {
            None
        };
        let expected_cwd = if trace_program == "node" && harness.contextlint {
            let indices = expected
                .arguments
                .iter()
                .enumerate()
                .filter_map(|(index, argument)| (argument == "--cwd").then_some(index))
                .collect::<Vec<_>>();
            let [index] = indices.as_slice() else {
                return Err(format!(
                    "Contextlint {label} trace requires exactly one --cwd argument, got {indices:?}"
                ));
            };
            let expected = expected
                .arguments
                .get(index + 1)
                .ok_or_else(|| format!("Contextlint {label} trace has no value after --cwd"))?
                .clone();
            resolve_dynamic_trace_argument(
                &expected,
                &recorded_cwd,
                contextlint_private_root.as_deref(),
                eslint_private_root.as_deref(),
                ghalint_private_root.as_deref(),
            )?
        } else {
            cwd.to_string_lossy().into_owned()
        };
        if !matches!(
            trace_program,
            "cargo" | "cargo-clippy" | "cargo-fmt" | "rustfmt" | "vacuum"
        ) && recorded_cwd.trim_end_matches(std::path::MAIN_SEPARATOR)
            != expected_cwd.trim_end_matches(std::path::MAIN_SEPARATOR)
        {
            return Err(format!(
                "{trace_program} {label} trace expected cwd {expected_cwd:?}, got {recorded_cwd:?}"
            ));
        }
        assert_record(&record, "argc", &expected.arguments.len().to_string())?;
        let mut recorded_arguments = Vec::with_capacity(expected.arguments.len());
        let mut observed_dclint_config = None;
        for (index, argument) in expected.arguments.iter().enumerate() {
            let argument = resolve_dynamic_trace_argument(
                argument,
                &recorded_cwd,
                contextlint_private_root.as_deref(),
                eslint_private_root.as_deref(),
                ghalint_private_root.as_deref(),
            )?;
            if trace_program == "dclint" && argument == DCLINT_PRIVATE_CONFIG_ARGUMENT {
                let actual = read_record(&record, &format!("argv-{index}"))?;
                observed_dclint_config = Some(verify_dclint_private_config_argument(
                    &record, &actual, project,
                )?);
                recorded_arguments.push(DCLINT_PRIVATE_CONFIG_ARGUMENT.to_owned());
            } else {
                assert_record(&record, &format!("argv-{index}"), &argument)?;
                recorded_arguments.push(argument);
            }
        }
        dclint_private_configs.push(observed_dclint_config);
        assert_record(&record, "status", &expected.exit_code.to_string())?;
        assert_record(&record, "execution", "pass-through")?;
        let program = read_record(&record, "program")?;
        if Path::new(&program).file_name() != Some(OsStr::new(trace_program)) {
            return Err(format!(
                "{} {label} trace recorded unexpected shim program {program:?}",
                trace_program
            ));
        }
        let mut environment = serde_json::json!({
            "LANG": "C",
            "LC_ALL": "C",
            "TZ": "UTC",
            "NO_COLOR": "1",
            "CLICOLOR": "0",
            "FORCE_COLOR": "0",
            TOOL_TRACE_SENTINEL_ENV: TOOL_TRACE_SENTINEL,
        });
        if trace_program == "astro" {
            let (node_path, telemetry_disabled, ci, debug) =
                verify_astro_trace_environment(&record, harness)?;
            let environment = environment
                .as_object_mut()
                .expect("trace environment is a JSON object");
            environment.insert(NODE_PATH_ENV.to_owned(), JsonValue::String(node_path));
            environment.insert(
                ASTRO_TELEMETRY_DISABLED_ENV.to_owned(),
                JsonValue::String(telemetry_disabled),
            );
            environment.insert(CI_ENV.to_owned(), JsonValue::String(ci));
            environment.insert(DEBUG_ENV.to_owned(), JsonValue::String(debug));
        }
        if trace_program == "betterleaks" {
            let scrubbed = verify_betterleaks_trace_environment(&record)?;
            let environment = environment
                .as_object_mut()
                .expect("trace environment is a JSON object");
            for (name, value) in scrubbed {
                environment.insert(name, JsonValue::String(value));
            }
        }
        if trace_program == "biome" {
            let scrubbed = verify_biome_trace_environment(&record)?;
            let environment = environment
                .as_object_mut()
                .expect("trace environment is a JSON object");
            for (name, value) in scrubbed {
                environment.insert(name, JsonValue::String(value));
            }
        }
        if trace_program == "node" {
            let controlled = if harness.eslint {
                verify_eslint_trace_environment(&record)?
            } else if harness.contextlint {
                verify_contextlint_trace_environment(&record, harness)?
            } else {
                verify_prettier_trace_environment(&record)?
            };
            let environment = environment
                .as_object_mut()
                .expect("trace environment is a JSON object");
            for (name, value) in controlled {
                environment.insert(name, JsonValue::String(value));
            }
        }
        if trace_program == "buf" {
            let controlled = verify_buf_trace_environment(&record, harness)?;
            let environment = environment
                .as_object_mut()
                .expect("trace environment is a JSON object");
            for (name, value) in controlled {
                environment.insert(name, JsonValue::String(value));
            }
        }
        if trace_program == "gofmt" {
            let controlled = verify_gofmt_trace_environment(&record, harness)?;
            let environment = environment
                .as_object_mut()
                .expect("trace environment is a JSON object");
            for (name, value) in controlled {
                environment.insert(name, JsonValue::String(value));
            }
        }
        if trace_program == "dclint" {
            let controlled = verify_dclint_trace_environment(&record, harness)?;
            let environment = environment
                .as_object_mut()
                .expect("trace environment is a JSON object");
            for (name, value) in controlled {
                environment.insert(name, JsonValue::String(value));
            }
        }
        if trace_program == "vacuum" {
            let controlled = verify_vacuum_trace_environment(
                &record,
                harness,
                &recorded_cwd,
                expected.targets.len(),
            )?;
            let environment = environment
                .as_object_mut()
                .expect("trace environment is a JSON object");
            for (name, value) in controlled {
                environment.insert(name, JsonValue::String(value));
            }
        }
        if harness.cargo_fmt_toolchain.is_some()
            && matches!(trace_program, "cargo" | "cargo-fmt" | "rustfmt")
        {
            if trace_program == "cargo"
                && expected
                    .arguments
                    .iter()
                    .any(|argument| argument == "--format-version=1")
                && !expected
                    .arguments
                    .iter()
                    .any(|argument| argument.contains(CARGO_FMT_PRIVATE_ROOT_PLACEHOLDER))
            {
                cargo_target_dir = None;
            }
            let controlled = verify_cargo_fmt_trace_environment(
                &record,
                harness,
                project,
                &recorded_cwd,
                &mut cargo_target_dir,
            )?;
            let environment = environment
                .as_object_mut()
                .expect("trace environment is a JSON object");
            for (name, value) in controlled {
                environment.insert(name, JsonValue::String(value));
            }
        } else if matches!(trace_program, "cargo" | "cargo-clippy") {
            if trace_program == "cargo" {
                cargo_target_dir = None;
                clippy_conf_dir = None;
            }
            let controlled = verify_cargo_clippy_trace_environment(
                &record,
                harness,
                project,
                &recorded_cwd,
                &mut cargo_target_dir,
                &mut clippy_conf_dir,
            )?;
            let environment = environment
                .as_object_mut()
                .expect("trace environment is a JSON object");
            for (name, value) in controlled {
                environment.insert(name, JsonValue::String(value));
            }
        }
        let prerequisites = if trace_program == "vacuum" {
            serde_json::json!({
                "config": "owned-0600-empty-config",
                "inputs": "owned-0600-byte-copies",
                "network": "fixed-deny-flags",
                "privateRootRemoved": true,
            })
        } else if trace_program == "node" && harness.eslint {
            eslint_trace_prerequisites(harness, expected)?
        } else if trace_program == "node" && harness.contextlint {
            contextlint_trace_prerequisites(harness, expected)?
        } else if trace_program == "buf" {
            serde_json::json!({"diff": BUF_DIFF_PROGRAM})
        } else if harness.cargo_fmt_toolchain.is_some()
            && matches!(trace_program, "cargo" | "cargo-fmt" | "rustfmt")
        {
            cargo_fmt_trace_prerequisites(harness)?
        } else if trace_program == "node" {
            prettier_trace_prerequisites(harness, expected)?
        } else if trace_program == "dclint" {
            dclint_trace_prerequisites(harness)?
        } else if matches!(trace_program, "cargo" | "cargo-clippy") {
            cargo_clippy_trace_prerequisites(harness)?
        } else {
            serde_json::json!({})
        };
        let evidence_cwd = if trace_program == "node"
            && harness.contextlint
            && Path::new(&recorded_cwd)
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with(CONTEXTLINT_PRIVATE_ROOT_PREFIX))
        {
            CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER.to_owned()
        } else {
            recorded_cwd.clone()
        };
        let evidence_arguments =
            if trace_program == "node" && (harness.contextlint || harness.eslint) {
                expected.arguments.clone()
            } else {
                recorded_arguments
            };
        let mut trace_record = serde_json::json!({
            "logicalProgram": trace_program,
            "shimProgram": program,
            "realProgram": real_program,
            "cwd": evidence_cwd,
            "argv": evidence_arguments,
            "candidateFiles": expected.targets,
            "environment": environment,
            "prerequisites": prerequisites,
            "execution": "pass-through",
            "exitCode": expected.exit_code,
        });
        if harness.eslint {
            trace_record
                .as_object_mut()
                .expect("trace record is a JSON object")
                .insert("privateState".to_owned(), private_state);
        }
        records.push(trace_record);
    }
    for (index, expected) in expected_invocations.iter().enumerate() {
        if expected.program != "dclint"
            || !expected
                .arguments
                .iter()
                .any(|argument| argument == "--fix")
        {
            continue;
        }
        let Some(previous) = index.checked_sub(1) else {
            return Err("dclint write child has no preflight trace".to_owned());
        };
        let Some(next) = index
            .checked_add(1)
            .filter(|next| *next < expected_invocations.len())
        else {
            return Err("dclint write child has no authoritative verification trace".to_owned());
        };
        let expected_config = dclint_private_configs
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(|| "dclint write child has no observed private config".to_owned())?;
        if dclint_private_configs
            .get(previous)
            .and_then(Option::as_ref)
            != Some(expected_config)
            || dclint_private_configs.get(next).and_then(Option::as_ref) != Some(expected_config)
        {
            return Err(format!(
                "dclint preflight, write child, and authoritative verification did not share one private config around trace {index}"
            ));
        }
    }
    write_json(
        evidence_path,
        &serde_json::json!({
            "formatVersion": 1,
            "label": label,
            "invocations": records,
        }),
    )
}

fn resolve_contextlint_private_trace_root(
    record: &Path,
    expected_arguments: &[String],
    recorded_cwd: &str,
) -> Result<Option<String>, String> {
    if !expected_arguments
        .iter()
        .any(|argument| argument.contains(CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER))
    {
        return Ok(None);
    }
    let mut candidates = BTreeSet::new();
    let cwd = Path::new(recorded_cwd);
    if cwd
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with(CONTEXTLINT_PRIVATE_ROOT_PREFIX))
    {
        candidates.insert(recorded_cwd.to_owned());
    }
    for (index, expected) in expected_arguments.iter().enumerate() {
        if !expected.contains(CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER) {
            continue;
        }
        if expected
            .matches(CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER)
            .count()
            != 1
        {
            return Err(format!(
                "Contextlint trace argument contains its private-root placeholder more than once: {expected:?}"
            ));
        }
        let (prefix, suffix) = expected
            .split_once(CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER)
            .expect("placeholder presence was checked");
        let actual = read_record(record, &format!("argv-{index}"))?;
        let candidate = actual
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(suffix))
            .ok_or_else(|| {
                format!(
                    "Contextlint trace argument {index} cannot bind expected private path shape {expected:?} to {actual:?}"
                )
            })?;
        let candidate_path = Path::new(candidate);
        if !candidate_path.is_absolute()
            || !candidate_path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with(CONTEXTLINT_PRIVATE_ROOT_PREFIX))
        {
            return Err(format!(
                "Contextlint trace argument {index} bound an invalid private root {candidate_path:?}"
            ));
        }
        candidates.insert(candidate.to_owned());
    }
    if candidates.len() != 1 {
        return Err(format!(
            "Contextlint trace did not bind one consistent private root: {candidates:?}"
        ));
    }
    Ok(candidates.into_iter().next())
}

fn resolve_eslint_private_trace_root(
    record: &Path,
    expected_arguments: &[String],
) -> Result<Option<String>, String> {
    if !expected_arguments
        .iter()
        .any(|argument| argument.contains(ESLINT_PRIVATE_ROOT_PLACEHOLDER))
    {
        return Ok(None);
    }
    let mut candidates = BTreeSet::new();
    for (index, expected) in expected_arguments.iter().enumerate() {
        if !expected.contains(ESLINT_PRIVATE_ROOT_PLACEHOLDER) {
            continue;
        }
        if expected.matches(ESLINT_PRIVATE_ROOT_PLACEHOLDER).count() != 1 {
            return Err(format!(
                "ESLint trace argument contains its private-root placeholder more than once: {expected:?}"
            ));
        }
        let (prefix, suffix) = expected
            .split_once(ESLINT_PRIVATE_ROOT_PLACEHOLDER)
            .expect("placeholder presence was checked");
        let actual = read_record(record, &format!("argv-{index}"))?;
        let candidate = actual
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(suffix))
            .ok_or_else(|| {
                format!(
                    "ESLint trace argument {index} cannot bind expected private path shape {expected:?} to {actual:?}"
                )
            })?;
        let candidate_path = Path::new(candidate);
        if !candidate_path.is_absolute()
            || !candidate_path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with(ESLINT_PRIVATE_ROOT_PREFIX))
        {
            return Err(format!(
                "ESLint trace argument {index} bound an invalid private root {candidate_path:?}"
            ));
        }
        candidates.insert(candidate.to_owned());
    }
    if candidates.len() != 1 {
        return Err(format!(
            "ESLint trace did not bind one consistent private root: {candidates:?}"
        ));
    }
    Ok(candidates.into_iter().next())
}

fn resolve_ghalint_private_trace_root(
    record: &Path,
    expected_arguments: &[String],
) -> Result<Option<String>, String> {
    let mut candidates = BTreeSet::new();
    for (index, expected) in expected_arguments.iter().enumerate() {
        if !expected.contains(GHALINT_PRIVATE_ROOT_PLACEHOLDER) {
            continue;
        }
        if expected.matches(GHALINT_PRIVATE_ROOT_PLACEHOLDER).count() != 1 {
            return Err(format!(
                "ghalint trace argument contains its private-root placeholder more than once: {expected:?}"
            ));
        }
        let (prefix, suffix) = expected
            .split_once(GHALINT_PRIVATE_ROOT_PLACEHOLDER)
            .expect("placeholder presence was checked");
        let actual = read_record(record, &format!("argv-{index}"))?;
        let candidate = actual
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(suffix))
            .ok_or_else(|| {
                format!(
                    "ghalint trace argument {index} cannot bind expected private path shape {expected:?} to {actual:?}"
                )
            })?;
        let candidate_path = Path::new(candidate);
        if !candidate_path.is_absolute()
            || !candidate_path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with(GHALINT_PRIVATE_ROOT_PREFIX))
        {
            return Err(format!(
                "ghalint trace argument {index} bound an invalid private root {candidate_path:?}"
            ));
        }
        candidates.insert(candidate.to_owned());
    }
    match candidates.len() {
        0 => Ok(None),
        1 => Ok(candidates.into_iter().next()),
        _ => Err(format!(
            "ghalint trace did not bind one consistent private root: {candidates:?}"
        )),
    }
}

fn resolve_dynamic_trace_argument(
    argument: &str,
    recorded_cwd: &str,
    contextlint_private_root: Option<&str>,
    eslint_private_root: Option<&str>,
    ghalint_private_root: Option<&str>,
) -> Result<String, String> {
    let cwd = Path::new(recorded_cwd);
    if argument.contains(GHALINT_PRIVATE_ROOT_PLACEHOLDER) {
        let ghalint_private_root = ghalint_private_root.ok_or_else(|| {
            format!("ghalint dynamic trace path has no validated private root: {argument:?}")
        })?;
        if argument.matches(GHALINT_PRIVATE_ROOT_PLACEHOLDER).count() != 1 {
            return Err(format!(
                "ghalint dynamic trace argument contains its private-root placeholder more than once: {argument:?}"
            ));
        }
        return Ok(argument.replacen(GHALINT_PRIVATE_ROOT_PLACEHOLDER, ghalint_private_root, 1));
    }
    if argument.contains(CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER) {
        let contextlint_private_root = contextlint_private_root.ok_or_else(|| {
            format!("Contextlint dynamic trace path has no validated private root: {argument:?}")
        })?;
        if argument
            .matches(CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER)
            .count()
            != 1
        {
            return Err(format!(
                "Contextlint dynamic trace argument contains its private-root placeholder more than once: {argument:?}"
            ));
        }
        return Ok(argument.replacen(
            CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER,
            contextlint_private_root,
            1,
        ));
    }
    if argument.contains(ESLINT_PRIVATE_ROOT_PLACEHOLDER) {
        let eslint_private_root = eslint_private_root.ok_or_else(|| {
            format!("ESLint dynamic trace path has no validated private root: {argument:?}")
        })?;
        if argument.matches(ESLINT_PRIVATE_ROOT_PLACEHOLDER).count() != 1 {
            return Err(format!(
                "ESLint dynamic trace argument contains its private-root placeholder more than once: {argument:?}"
            ));
        }
        return Ok(argument.replacen(ESLINT_PRIVATE_ROOT_PLACEHOLDER, eslint_private_root, 1));
    }
    let Some(suffix) = argument.strip_prefix(CARGO_FMT_PRIVATE_ROOT_PLACEHOLDER) else {
        return Ok(argument.to_owned());
    };
    if cwd.file_name() != Some(OsStr::new("invocation")) {
        return Err(format!(
            "Cargo Fmt dynamic trace path requires an invocation cwd, got {cwd:?}"
        ));
    }
    let private_root = cwd
        .parent()
        .ok_or_else(|| format!("Cargo Fmt invocation cwd has no private parent: {cwd:?}"))?;
    if !private_root
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with("velvet-glove-cargo-fmt-"))
    {
        return Err(format!(
            "Cargo Fmt dynamic trace path has an unexpected private root: {private_root:?}"
        ));
    }
    Ok(private_root
        .join(suffix.trim_start_matches('/'))
        .to_string_lossy()
        .into_owned())
}

fn verify_eslint_private_trace_state(
    record: &Path,
    harness: &ToolTraceHarness,
    project: &Path,
    private_root: &str,
) -> Result<JsonValue, String> {
    let recorded_root = read_record(record, "eslint-private-root")?;
    if recorded_root != private_root {
        return Err(format!(
            "ESLint trace private root mismatch: arguments bound {private_root:?}, metadata recorded {recorded_root:?}"
        ));
    }
    let root = Path::new(private_root);
    let expected_parent = harness
        .trace_root
        .parent()
        .ok_or_else(|| "ESLint trace root has no controlled parent".to_owned())?
        .join("eslint-tmp")
        .canonicalize()
        .map_err(|error| format!("canonicalize controlled ESLint temporary root: {error}"))?;
    if root.parent() != Some(expected_parent.as_path())
        || root.starts_with(project)
        || !root
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(ESLINT_PRIVATE_ROOT_PREFIX))
    {
        return Err(format!(
            "ESLint private state escaped its controlled temporary root: {root:?}"
        ));
    }
    for (name, expected) in [
        ("eslint-private-root-mode", "700"),
        ("eslint-config-mode", "600"),
        ("eslint-suppressions-mode", "600"),
        ("eslint-cache-directory-mode", "700"),
        ("eslint-cache-kind", "file"),
        ("eslint-cache-mode", "600"),
    ] {
        let actual = read_record(record, name)?;
        if actual != expected {
            return Err(format!(
                "ESLint private trace requires {name}={expected:?}, got {actual:?}"
            ));
        }
    }
    let suppressions = std::fs::read(record.join("eslint-suppressions.json"))
        .map_err(|error| format!("read traced ESLint suppressions: {error}"))?;
    if suppressions != b"{}\n" {
        return Err(format!(
            "ESLint private suppressions drifted from the exact empty object: {suppressions:?}"
        ));
    }
    let config_bytes = std::fs::read(record.join("eslint-config.cjs"))
        .map_err(|error| format!("read traced ESLint private config: {error}"))?;
    let config_text = std::str::from_utf8(&config_bytes)
        .map_err(|error| format!("traced ESLint config is not UTF-8: {error}"))?;
    let config_json = config_text
        .strip_prefix("'use strict';\nmodule.exports = ")
        .and_then(|value| value.strip_suffix(";\n"))
        .ok_or_else(|| "traced ESLint config is not the exact data-only CJS envelope".to_owned())?;
    let config: JsonValue = serde_json::from_str(config_json)
        .map_err(|error| format!("parse traced ESLint data-only config: {error}"))?;
    let rules = serde_json::json!({
        "eqeqeq": 2,
        "no-debugger": 2,
        "no-undef": 2,
        "no-unused-vars": 2,
        "no-var": 2,
        "prefer-const": 2,
        "semi": [2, "always"],
    });
    let expected_config = serde_json::json!([
        {
            "files": ["**/*.js", "**/*.mjs"],
            "languageOptions": {"ecmaVersion": "latest", "sourceType": "module"},
            "linterOptions": {
                "noInlineConfig": true,
                "reportUnusedDisableDirectives": "error",
            },
            "rules": rules.clone(),
        },
        {
            "files": ["**/*.cjs"],
            "languageOptions": {"ecmaVersion": "latest", "sourceType": "commonjs"},
            "linterOptions": {
                "noInlineConfig": true,
                "reportUnusedDisableDirectives": "error",
            },
            "rules": rules,
        },
    ]);
    if config != expected_config {
        return Err(format!(
            "ESLint private config escaped the built-in data-only rule contract: {config}"
        ));
    }
    let cache_location = PathBuf::from(read_record(record, "eslint-cache-location")?);
    if !cache_location.starts_with(root)
        || cache_location.file_name() != Some(OsStr::new(".eslintcache"))
        || !cache_location
            .parent()
            .and_then(Path::file_name)
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("cache-"))
    {
        return Err(format!(
            "ESLint cache escaped its private root: {cache_location:?}"
        ));
    }
    Ok(serde_json::json!({
        "root": ESLINT_PRIVATE_ROOT_PLACEHOLDER,
        "rootMode": "0700",
        "config": config,
        "configMode": "0600",
        "suppressions": {},
        "suppressionsMode": "0600",
        "cache": cache_location
            .to_string_lossy()
            .replace(private_root, ESLINT_PRIVATE_ROOT_PLACEHOLDER),
        "cacheMode": "0600",
    }))
}

fn verify_astro_trace_environment(
    record: &Path,
    harness: &ToolTraceHarness,
) -> Result<(String, String, String, String), String> {
    let node_path = read_record(record, &format!("env-{NODE_PATH_ENV}"))?;
    if node_path.is_empty() {
        return Err("Astro trace inherited an empty NODE_PATH".to_owned());
    }
    let roots = std::env::split_paths(OsStr::new(&node_path)).collect::<Vec<_>>();
    if roots.len() != 1 {
        return Err(format!(
            "Astro trace requires exactly one controlled NODE_PATH root, got {roots:?}"
        ));
    }
    let root = roots[0]
        .canonicalize()
        .map_err(|error| format!("canonicalize Astro NODE_PATH {:?}: {error}", roots[0]))?;
    let real_program = harness
        .programs
        .get("astro")
        .ok_or_else(|| "Astro trace harness has no executable binding".to_owned())?;
    let controlled_root = real_program
        .ancestors()
        .find(|path| path.file_name() == Some(OsStr::new("node_modules")))
        .ok_or_else(|| {
            format!(
                "pinned Astro executable is not inside node_modules: {:?}",
                real_program
            )
        })?
        .canonicalize()
        .map_err(|error| {
            format!(
                "canonicalize pinned Astro node_modules root for {:?}: {error}",
                real_program
            )
        })?;
    if root != controlled_root {
        return Err(format!(
            "Astro NODE_PATH escaped its pinned executable graph: expected {controlled_root:?}, got {root:?}"
        ));
    }
    for manifest in [
        "astro/package.json",
        "@astrojs/check/package.json",
        "typescript/package.json",
    ] {
        let path = root.join(manifest);
        if !path.is_file() {
            return Err(format!(
                "Astro NODE_PATH lacks required package manifest {path:?}"
            ));
        }
    }
    let telemetry_disabled = read_record(record, &format!("env-{ASTRO_TELEMETRY_DISABLED_ENV}"))?;
    if telemetry_disabled != "1" {
        return Err(format!(
            "Astro trace must disable telemetry, got {ASTRO_TELEMETRY_DISABLED_ENV}={telemetry_disabled:?}"
        ));
    }
    let ci = read_record(record, &format!("env-{CI_ENV}"))?;
    if ci != "1" {
        return Err(format!(
            "Astro trace must set CI=1 to disable interactive installs, got {CI_ENV}={ci:?}"
        ));
    }
    let debug = read_record(record, &format!("env-{DEBUG_ENV}"))?;
    if !debug.is_empty() {
        return Err(format!(
            "Astro trace must clear DEBUG to keep diagnostics deterministic, got {DEBUG_ENV}={debug:?}"
        ));
    }
    Ok((
        root.to_string_lossy().into_owned(),
        telemetry_disabled,
        ci,
        debug,
    ))
}

fn verify_betterleaks_trace_environment(record: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut environment = BTreeMap::new();
    for name in [
        BETTERLEAKS_CONFIG_ENV,
        BETTERLEAKS_CONFIG_TOML_ENV,
        GITLEAKS_CONFIG_ENV,
        GITLEAKS_CONFIG_TOML_ENV,
    ] {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "Betterleaks trace must clear inherited configuration, got {name}={value:?}"
            ));
        }
        environment.insert((*name).to_owned(), value);
    }
    Ok(environment)
}

fn verify_prettier_trace_environment(record: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut environment = BTreeMap::new();
    for (name, expected) in [
        (PATH_ENV, PRETTIER_CHILD_PATH),
        (CI_ENV, "1"),
        ("TERM", "dumb"),
    ] {
        let value = read_record(record, &format!("env-{name}"))?;
        if value != expected {
            return Err(format!(
                "Prettier trace requires {name}={expected:?}, got {value:?}"
            ));
        }
        environment.insert(name.to_owned(), value);
    }
    for name in PRETTIER_SCRUBBED_ENV {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "Prettier trace must clear inherited Node, Prettier, and loader state; got {name}={value:?}"
            ));
        }
        environment.insert((*name).to_owned(), value);
    }
    Ok(environment)
}

fn verify_eslint_trace_environment(record: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut environment = BTreeMap::new();
    for (name, expected) in [
        (PATH_ENV, ESLINT_CHILD_PATH),
        (CI_ENV, "1"),
        ("TERM", "dumb"),
        ("NODE_DISABLE_COLORS", "1"),
        ("UV_THREADPOOL_SIZE", "1"),
    ] {
        let value = read_record(record, &format!("env-{name}"))?;
        if value != expected {
            return Err(format!(
                "ESLint trace requires {name}={expected:?}, got {value:?}"
            ));
        }
        environment.insert(name.to_owned(), value);
    }
    for name in [HOME_ENV, TMPDIR_ENV, XDG_CACHE_HOME_ENV]
        .into_iter()
        .chain(ESLINT_SCRUBBED_ENV.iter().copied())
    {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "ESLint trace must clear inherited project, Node, npm, ESLint, and loader state; got {name}={value:?}"
            ));
        }
        environment.insert(name.to_owned(), value);
    }
    Ok(environment)
}

fn prettier_trace_prerequisites(
    harness: &ToolTraceHarness,
    expected: &ResolvedTraceInvocation,
) -> Result<JsonValue, String> {
    let Some(toolchain) = &harness.prettier_toolchain else {
        return Ok(serde_json::json!({}));
    };
    let real_node = harness
        .programs
        .get("node")
        .ok_or_else(|| "Prettier trace has no managed Node binding".to_owned())?;
    if real_node != &toolchain.node {
        return Err(format!(
            "Prettier trace bound Node {:?}, expected dedicated runtime {:?}",
            real_node, toolchain.node
        ));
    }
    if expected.arguments.first().map(PathBuf::from).as_ref() != Some(&toolchain.cli) {
        return Err(format!(
            "Prettier trace did not pass the dedicated managed CLI as Node argv[0]: expected {:?}, got {:?}",
            toolchain.cli,
            expected.arguments.first()
        ));
    }
    Ok(serde_json::json!({
        "root": toolchain.root,
        "node": toolchain.node,
        "prettierCli": toolchain.cli,
    }))
}

fn eslint_trace_prerequisites(
    harness: &ToolTraceHarness,
    expected: &ResolvedTraceInvocation,
) -> Result<JsonValue, String> {
    let Some(toolchain) = &harness.eslint_toolchain else {
        return Ok(serde_json::json!({
            "eslint": "10.8.1",
            "node": "24.19.0",
            "npm": "11.17.0",
            "configuration": "private data-derived CJS; built-in rules only",
        }));
    };
    let real_node = harness
        .programs
        .get("node")
        .ok_or_else(|| "ESLint trace has no managed Node binding".to_owned())?;
    if real_node != &toolchain.node {
        return Err(format!(
            "ESLint trace bound Node {:?}, expected dedicated runtime {:?}",
            real_node, toolchain.node
        ));
    }
    if expected.arguments.first().map(PathBuf::from).as_ref() != Some(&toolchain.cli) {
        return Err(format!(
            "ESLint trace did not pass the dedicated managed CLI as Node argv[0]: expected {:?}, got {:?}",
            toolchain.cli,
            expected.arguments.first()
        ));
    }
    Ok(serde_json::json!({
        "root": toolchain.root,
        "node": toolchain.node,
        "eslintCli": toolchain.cli,
        "eslint": "10.8.1",
        "npm": "11.17.0",
        "configuration": "private data-derived CJS; built-in rules only",
    }))
}

fn contextlint_trace_prerequisites(
    harness: &ToolTraceHarness,
    expected: &ResolvedTraceInvocation,
) -> Result<JsonValue, String> {
    let Some(toolchain) = &harness.contextlint_toolchain else {
        return Ok(serde_json::json!({
            "cli": "@contextlint/cli@1.1.1",
            "core": "@contextlint/core@1.1.1",
            "node": "24.19.0",
            "permissions": {
                "filesystemReads": "package graph, workspace, and authoritative private config/probe",
                "filesystemWrites": false,
                "childProcesses": false,
                "workers": false,
                "nativeAddons": false,
            },
        }));
    };
    let real_node = harness
        .programs
        .get("node")
        .ok_or_else(|| "Contextlint trace has no managed Node binding".to_owned())?;
    if real_node != &toolchain.node {
        return Err(format!(
            "Contextlint trace bound Node {:?}, expected dedicated runtime {:?}",
            real_node, toolchain.node
        ));
    }
    let cli = toolchain.cli.to_string_lossy();
    if expected
        .arguments
        .iter()
        .filter(|argument| *argument == cli.as_ref())
        .count()
        != 1
    {
        return Err(format!(
            "Contextlint trace did not pass the dedicated managed CLI exactly once: expected {:?}, got {:?}",
            toolchain.cli, expected.arguments
        ));
    }
    Ok(serde_json::json!({
        "root": toolchain.root,
        "node": toolchain.node,
        "contextlintCli": toolchain.cli,
        "cli": "@contextlint/cli@1.1.1",
        "core": "@contextlint/core@1.1.1",
        "permissions": {
            "filesystemReads": "package graph, workspace, and authoritative private config/probe",
            "filesystemWrites": false,
            "childProcesses": false,
            "workers": false,
            "nativeAddons": false,
        },
    }))
}

fn verify_biome_trace_environment(record: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut environment = BTreeMap::new();
    let ci = read_record(record, &format!("env-{CI_ENV}"))?;
    if ci != "1" {
        return Err(format!(
            "Biome trace must force non-interactive CI mode, got {CI_ENV}={ci:?}"
        ));
    }
    environment.insert(CI_ENV.to_owned(), ci);
    let rayon_threads = read_record(record, &format!("env-{RAYON_NUM_THREADS_ENV}"))?;
    if rayon_threads != "1" {
        return Err(format!(
            "Biome trace must force deterministic worker count, got {RAYON_NUM_THREADS_ENV}={rayon_threads:?}"
        ));
    }
    environment.insert(RAYON_NUM_THREADS_ENV.to_owned(), rayon_threads);
    for name in BIOME_SCRUBBED_ENV {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "Biome trace must clear inherited {name}, got {name}={value:?}"
            ));
        }
        environment.insert((*name).to_owned(), value);
    }
    Ok(environment)
}

fn verify_contextlint_trace_environment(
    record: &Path,
    harness: &ToolTraceHarness,
) -> Result<BTreeMap<String, String>, String> {
    let controlled_root = harness.trace_root.parent().ok_or_else(|| {
        format!(
            "Contextlint trace root has no controlled environment parent: {:?}",
            harness.trace_root
        )
    })?;
    let controlled_root = controlled_root.canonicalize().map_err(|error| {
        format!("canonicalize Contextlint controlled environment root {controlled_root:?}: {error}")
    })?;
    let home_value = read_record(record, &format!("env-{HOME_ENV}"))?;
    let home = PathBuf::from(&home_value);
    let run_root = home
        .parent()
        .ok_or_else(|| format!("Contextlint trace HOME has no private parent: {home_value:?}"))?;
    if home.file_name() != Some(OsStr::new("home"))
        || !run_root
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(CONTEXTLINT_PRIVATE_ROOT_PREFIX))
        || run_root.parent() != Some(controlled_root.join("contextlint-tmp").as_path())
    {
        return Err(format!(
            "Contextlint trace HOME is outside its unique controlled private root: {home_value:?}"
        ));
    }
    let expected = BTreeMap::from([
        (PATH_ENV, CONTEXTLINT_CHILD_PATH.to_owned()),
        (
            HOME_ENV,
            run_root.join("home").to_string_lossy().into_owned(),
        ),
        (
            TMPDIR_ENV,
            run_root.join("tmp").to_string_lossy().into_owned(),
        ),
        (
            XDG_CACHE_HOME_ENV,
            run_root.join("xdg-cache").to_string_lossy().into_owned(),
        ),
        ("TERM", "dumb".to_owned()),
        (CI_ENV, "1".to_owned()),
        ("NODE_DISABLE_COLORS", "1".to_owned()),
        ("UV_THREADPOOL_SIZE", "1".to_owned()),
    ]);
    let mut environment = BTreeMap::new();
    for (name, expected_value) in expected {
        let value = read_record(record, &format!("env-{name}"))?;
        if value != expected_value {
            return Err(format!(
                "Contextlint trace expected {name}={expected_value:?}, got {value:?}"
            ));
        }
        environment.insert(
            name.to_owned(),
            value.replace(
                run_root.to_string_lossy().as_ref(),
                CONTEXTLINT_PRIVATE_ROOT_PLACEHOLDER,
            ),
        );
    }
    for name in CONTEXTLINT_SCRUBBED_ENV {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "Contextlint trace must scrub inherited {name}, got {name}={value:?}"
            ));
        }
        environment.insert((*name).to_owned(), value);
    }
    Ok(environment)
}

fn verify_buf_trace_environment(
    record: &Path,
    harness: &ToolTraceHarness,
) -> Result<BTreeMap<String, String>, String> {
    let root = harness.trace_root.parent().ok_or_else(|| {
        format!(
            "Buf trace root has no controlled environment parent: {:?}",
            harness.trace_root
        )
    })?;
    let home = root.join("home");
    let tmpdir = root.join("tmp");
    let xdg_cache = root.join("xdg-cache");
    let cache = tmpdir.join("velvet-glove-buf-cache");
    let mut environment = BTreeMap::new();
    for (name, expected) in [
        (PATH_ENV, BUF_CHILD_PATH.to_owned()),
        (HOME_ENV, home.to_string_lossy().into_owned()),
        (TMPDIR_ENV, tmpdir.to_string_lossy().into_owned()),
        (XDG_CACHE_HOME_ENV, xdg_cache.to_string_lossy().into_owned()),
        (DIFF_OPTIONS_ENV, String::new()),
        (BUF_CACHE_DIR_ENV, cache.to_string_lossy().into_owned()),
    ] {
        let value = read_record(record, &format!("env-{name}"))?;
        if value != expected {
            return Err(format!(
                "Buf trace expected controlled {name}={expected:?}, got {value:?}"
            ));
        }
        environment.insert(name.to_owned(), value);
    }
    for name in BUF_SCRUBBED_ENV {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "Buf trace must clear inherited {name}, got {name}={value:?}"
            ));
        }
        environment.insert((*name).to_owned(), value);
    }
    let cache_metadata = std::fs::symlink_metadata(&cache)
        .map_err(|error| format!("inspect controlled Buf cache {cache:?}: {error}"))?;
    if !cache_metadata.is_dir() || cache_metadata.file_type().is_symlink() {
        return Err(format!(
            "controlled Buf cache must be a real directory: {cache:?}"
        ));
    }
    let program = PathBuf::from(read_record(record, "program")?);
    if !program.is_absolute() {
        return Err(format!(
            "Buf adapter must execute an absolute managed tool path, got {program:?}"
        ));
    }
    let observed_program = program
        .canonicalize()
        .map_err(|error| format!("canonicalize traced Buf program {program:?}: {error}"))?;
    let expected_program = harness
        .shim_dir
        .join("buf")
        .canonicalize()
        .map_err(|error| format!("canonicalize managed Buf trace shim: {error}"))?;
    if observed_program != expected_program {
        return Err(format!(
            "Buf adapter escaped the managed executable: expected {expected_program:?}, got {observed_program:?}"
        ));
    }
    Ok(environment)
}

fn vacuum_private_root_name(value: &str, suffix: Option<&str>) -> Result<String, String> {
    let relative = value
        .strip_prefix("/private/tmp/")
        .or_else(|| value.strip_prefix("/tmp/"))
        .ok_or_else(|| format!("Vacuum private path escaped /tmp: {value:?}"))?;
    let components = relative.split('/').collect::<Vec<_>>();
    let expected_len = usize::from(suffix.is_some()) + 1;
    if components.len() != expected_len
        || !components[0].starts_with("velvet-glove-vacuum-")
        || components[0].len() == "velvet-glove-vacuum-".len()
        || suffix.is_some_and(|expected| components.get(1).copied() != Some(expected))
    {
        return Err(format!(
            "Vacuum private path has an unexpected shape for suffix {suffix:?}: {value:?}"
        ));
    }
    Ok(components[0].to_owned())
}

fn verify_vacuum_trace_environment(
    record: &Path,
    harness: &ToolTraceHarness,
    recorded_cwd: &str,
    expected_input_count: usize,
) -> Result<BTreeMap<String, String>, String> {
    let private_root_name = vacuum_private_root_name(recorded_cwd, None)?;
    if Path::new(recorded_cwd).exists() {
        return Err(format!(
            "Vacuum adapter left its private root after child exit: {recorded_cwd:?}"
        ));
    }

    let mut environment = BTreeMap::new();
    for (name, expected) in [
        (PATH_ENV, VACUUM_CHILD_PATH),
        ("TERM", "dumb"),
        ("COLUMNS", "120"),
        ("LINES", "40"),
        ("GODEBUG", ""),
        ("GOMAXPROCS", "1"),
        ("GOTRACEBACK", "none"),
    ] {
        let value = read_record(record, &format!("env-{name}"))?;
        if value != expected {
            return Err(format!(
                "Vacuum trace expected controlled {name}={expected:?}, got {value:?}"
            ));
        }
        environment.insert(name.to_owned(), value);
    }

    for (name, suffix) in [
        (PWD_ENV, None),
        (HOME_ENV, Some("home")),
        (TMPDIR_ENV, Some("tmp")),
        (XDG_CONFIG_HOME_ENV, Some("config")),
        (XDG_CACHE_HOME_ENV, Some("cache")),
    ] {
        let value = read_record(record, &format!("env-{name}"))?;
        let observed_root = vacuum_private_root_name(&value, suffix)?;
        if observed_root != private_root_name {
            return Err(format!(
                "Vacuum trace {name} escaped its invocation root: expected {private_root_name:?}, got {observed_root:?} from {value:?}"
            ));
        }
        if Path::new(&value).exists() {
            return Err(format!(
                "Vacuum adapter left private {name} state after child exit: {value:?}"
            ));
        }
        environment.insert(name.to_owned(), value);
    }

    for name in VACUUM_PREFIX_SCRUBBED_ENV
        .iter()
        .chain(VACUUM_SCRUBBED_ENV)
        .chain(VACUUM_LOADER_SCRUBBED_ENV)
    {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "Vacuum trace must clear inherited {name}, got {name}={value:?}"
            ));
        }
        environment.insert((*name).to_owned(), value);
    }

    for (name, expected) in [
        ("vacuum-config-kind", "file"),
        ("vacuum-config-size", "3"),
        ("vacuum-config-content", "controlled"),
        ("vacuum-config-mode", "600"),
    ] {
        assert_record(record, name, expected)?;
    }
    assert_record(
        record,
        "vacuum-input-count",
        &expected_input_count.to_string(),
    )?;
    for index in 0..expected_input_count {
        assert_record(record, &format!("vacuum-input-{index}-kind"), "file")?;
        assert_record(record, &format!("vacuum-input-{index}-mode"), "600")?;
    }

    let program = PathBuf::from(read_record(record, "program")?);
    if !program.is_absolute() {
        return Err(format!(
            "Vacuum adapter must execute an absolute managed tool path, got {program:?}"
        ));
    }
    let observed_program = program
        .canonicalize()
        .map_err(|error| format!("canonicalize traced Vacuum program {program:?}: {error}"))?;
    let expected_program = harness
        .shim_dir
        .join("vacuum")
        .canonicalize()
        .map_err(|error| format!("canonicalize managed Vacuum trace shim: {error}"))?;
    if observed_program != expected_program {
        return Err(format!(
            "Vacuum adapter escaped the managed executable: expected {expected_program:?}, got {observed_program:?}"
        ));
    }
    Ok(environment)
}

fn verify_gofmt_trace_environment(
    record: &Path,
    harness: &ToolTraceHarness,
) -> Result<BTreeMap<String, String>, String> {
    let mut environment = BTreeMap::new();
    for (name, expected) in std::iter::once((PATH_ENV, GOFMT_CHILD_PATH))
        .chain(std::iter::once(("TERM", "dumb")))
        .chain(GOFMT_CONTROLLED_ENV.iter().copied())
    {
        let value = read_record(record, &format!("env-{name}"))?;
        if value != expected {
            return Err(format!(
                "gofmt trace expected controlled {name}={expected:?}, got {value:?}"
            ));
        }
        environment.insert(name.to_owned(), value);
    }
    for name in GOFMT_SCRUBBED_ENV.iter().chain(GOFMT_LOADER_SCRUBBED_ENV) {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "gofmt trace must clear inherited {name}, got {name}={value:?}"
            ));
        }
        environment.insert((*name).to_owned(), value);
    }
    let program = PathBuf::from(read_record(record, "program")?);
    if !program.is_absolute() {
        return Err(format!(
            "gofmt adapter must execute an absolute managed tool path, got {program:?}"
        ));
    }
    let observed_program = program
        .canonicalize()
        .map_err(|error| format!("canonicalize traced gofmt program {program:?}: {error}"))?;
    let expected_program = harness
        .shim_dir
        .join("gofmt")
        .canonicalize()
        .map_err(|error| format!("canonicalize managed gofmt trace shim: {error}"))?;
    if observed_program != expected_program {
        return Err(format!(
            "gofmt adapter escaped the managed executable: expected {expected_program:?}, got {observed_program:?}"
        ));
    }
    Ok(environment)
}

fn verify_dclint_private_config_argument(
    record: &Path,
    argument: &str,
    project: &Path,
) -> Result<PathBuf, String> {
    let value = argument.strip_prefix("--config=").ok_or_else(|| {
        format!("dclint trace expected a private --config argument, got {argument:?}")
    })?;
    let path = PathBuf::from(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || path.starts_with(project)
        || path.file_name() != Some(OsStr::new("config.json"))
    {
        return Err(format!(
            "dclint trace private config is not a normalized out-of-project config.json: {path:?}"
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("dclint trace config has no parent: {path:?}"))?;
    if !parent
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with("velvet-glove-dclint-"))
    {
        return Err(format!(
            "dclint trace config is not in its dedicated private directory: {path:?}"
        ));
    }
    let temporary = PathBuf::from(read_record(record, &format!("env-{TMPDIR_ENV}"))?);
    if parent.parent() != Some(temporary.as_path()) {
        return Err(format!(
            "dclint trace config directory is not a direct child of controlled TMPDIR {temporary:?}: {parent:?}"
        ));
    }
    assert_record(record, "dclint-config-path", value)?;
    assert_record(record, "dclint-config-kind", "file")?;
    assert_record(record, "dclint-config-mode", "600")?;
    assert_record(record, "dclint-config-links", "1")?;
    assert_record(record, "dclint-config-bytes", "187")?;
    assert_record(record, "dclint-config-sha256", DCLINT_PRIVATE_CONFIG_SHA256)?;
    assert_record(
        record,
        "dclint-config-parent",
        parent.to_string_lossy().as_ref(),
    )?;
    assert_record(record, "dclint-config-parent-kind", "directory")?;
    assert_record(record, "dclint-config-parent-mode", "700")?;
    Ok(path)
}

fn verify_dclint_trace_environment(
    record: &Path,
    harness: &ToolTraceHarness,
) -> Result<BTreeMap<String, String>, String> {
    let mut environment = BTreeMap::new();
    let node = if let Some(toolchain) = &harness.dclint_toolchain {
        toolchain.node.clone()
    } else {
        resolve_program("node")
            .ok_or_else(|| "dclint trace cannot resolve its controlled Node runtime".to_owned())?
            .canonicalize()
            .map_err(|error| format!("canonicalize dclint trace Node runtime: {error}"))?
    };
    let node_bin = node
        .parent()
        .ok_or_else(|| format!("dclint trace Node runtime has no bin directory: {node:?}"))?;
    let expected_path = std::env::join_paths([node_bin, Path::new("/usr/bin"), Path::new("/bin")])
        .map_err(|error| format!("construct dclint child PATH: {error}"))?;
    let temporary = harness
        .trace_root
        .parent()
        .ok_or_else(|| "dclint trace root has no parent".to_owned())?
        .join("tmp")
        .canonicalize()
        .map_err(|error| format!("canonicalize controlled dclint TMPDIR: {error}"))?;
    for (name, expected) in [
        (PATH_ENV, expected_path.to_string_lossy().into_owned()),
        (TMPDIR_ENV, temporary.to_string_lossy().into_owned()),
        ("TERM", "dumb".to_owned()),
        (CI_ENV, "1".to_owned()),
        ("NODE_NO_WARNINGS", "1".to_owned()),
    ] {
        let value = read_record(record, &format!("env-{name}"))?;
        if value != expected {
            return Err(format!(
                "dclint trace expected controlled {name}={expected:?}, got {value:?}"
            ));
        }
        environment.insert(name.to_owned(), value);
    }
    for name in DCLINT_SCRUBBED_ENV.iter().chain(DCLINT_LOADER_SCRUBBED_ENV) {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "dclint trace must clear inherited {name}, got {name}={value:?}"
            ));
        }
        environment.insert((*name).to_owned(), value);
    }
    let program = PathBuf::from(read_record(record, "program")?);
    if !program.is_absolute() {
        return Err(format!(
            "dclint adapter must execute an absolute managed tool path, got {program:?}"
        ));
    }
    let observed_program = program
        .canonicalize()
        .map_err(|error| format!("canonicalize traced dclint program {program:?}: {error}"))?;
    let expected_program = harness
        .shim_dir
        .join("dclint")
        .canonicalize()
        .map_err(|error| format!("canonicalize managed dclint trace shim: {error}"))?;
    if observed_program != expected_program {
        return Err(format!(
            "dclint adapter escaped the managed executable: expected {expected_program:?}, got {observed_program:?}"
        ));
    }
    Ok(environment)
}

fn dclint_trace_prerequisites(harness: &ToolTraceHarness) -> Result<JsonValue, String> {
    let Some(toolchain) = &harness.dclint_toolchain else {
        return Ok(serde_json::json!({}));
    };
    let real_cli = harness
        .programs
        .get("dclint")
        .ok_or_else(|| "dclint trace has no managed CLI binding".to_owned())?;
    if real_cli != &toolchain.cli {
        return Err(format!(
            "dclint trace bound CLI {:?}, expected dedicated CLI {:?}",
            real_cli, toolchain.cli
        ));
    }
    Ok(serde_json::json!({
        "root": toolchain.root,
        "node": toolchain.node,
        "dclintCli": toolchain.cli,
    }))
}

fn verify_cargo_clippy_trace_environment(
    record: &Path,
    harness: &ToolTraceHarness,
    project: &Path,
    recorded_cwd: &str,
    expected_target_dir: &mut Option<PathBuf>,
    expected_conf_dir: &mut Option<PathBuf>,
) -> Result<BTreeMap<String, String>, String> {
    let toolchain = harness
        .cargo_clippy_toolchain
        .as_ref()
        .ok_or_else(|| "cargo-clippy trace has no managed toolchain binding".to_owned())?;
    let mut environment = BTreeMap::new();
    let logical_program = read_record(record, "logical-program")?;
    if !matches!(logical_program.as_str(), "cargo" | "cargo-clippy") {
        return Err(format!(
            "cargo-clippy trace recorded an unexpected logical program {logical_program:?}"
        ));
    }
    let observed_shim = PathBuf::from(read_record(record, "program")?)
        .canonicalize()
        .map_err(|error| format!("canonicalize traced {logical_program} shim: {error}"))?;
    let expected_shim = harness
        .shim_dir
        .join(&logical_program)
        .canonicalize()
        .map_err(|error| format!("canonicalize expected {logical_program} shim: {error}"))?;
    if observed_shim != expected_shim {
        return Err(format!(
            "cargo-clippy adapter escaped its {logical_program} trace shim: expected {expected_shim:?}, got {observed_shim:?}"
        ));
    }

    for (name, expected) in [
        (
            DYLD_LIBRARY_PATH_ENV,
            // macOS strips DYLD_* while resolving the trace shim's /bin/sh
            // interpreter. The paired library remains bound as a prerequisite
            // and in the evaluated adapter source, but is intentionally absent
            // by the time the shim can record its environment.
            String::new(),
        ),
        (
            CARGO_PROGRAM_ENV,
            toolchain.cargo.to_string_lossy().into_owned(),
        ),
        (RUSTC_ENV, toolchain.rustc.to_string_lossy().into_owned()),
        (
            RUSTDOC_ENV,
            toolchain.rustdoc.to_string_lossy().into_owned(),
        ),
        (
            CARGO_HOME_ENV,
            toolchain.cargo_home.to_string_lossy().into_owned(),
        ),
        (
            TMPDIR_ENV,
            toolchain.temporary.to_string_lossy().into_owned(),
        ),
        (CARGO_NET_OFFLINE_ENV, "true".to_owned()),
        (CARGO_BUILD_JOBS_ENV, "1".to_owned()),
        ("CARGO_INCREMENTAL", "0".to_owned()),
        (CARGO_TERM_COLOR_ENV, "never".to_owned()),
        ("CLIPPY_DISABLE_DOCS_LINKS", "1".to_owned()),
        ("RUST_BACKTRACE", "0".to_owned()),
        ("RUST_LIB_BACKTRACE", "0".to_owned()),
        ("TERM", "dumb".to_owned()),
    ] {
        let value = read_record(record, &format!("env-{name}"))?;
        if value != expected {
            return Err(format!(
                "cargo-clippy trace expected controlled {name}={expected:?}, got {value:?}"
            ));
        }
        environment.insert(name.to_owned(), value);
    }

    let expected_path = std::env::join_paths([
        harness.shim_dir.as_path(),
        toolchain.bin.as_path(),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .map_err(|error| format!("construct cargo-clippy child PATH: {error}"))?;
    let path = read_record(record, &format!("env-{PATH_ENV}"))?;
    if OsStr::new(&path) != expected_path {
        return Err(format!(
            "cargo-clippy trace expected controlled PATH {expected_path:?}, got {path:?}"
        ));
    }
    environment.insert(PATH_ENV.to_owned(), path);

    let project = canonical_project(project);
    let target_dir = controlled_private_trace_directory(
        record,
        CARGO_TARGET_DIR_ENV,
        &project,
        expected_target_dir,
    )?;
    environment.insert(
        CARGO_TARGET_DIR_ENV.to_owned(),
        target_dir.to_string_lossy().into_owned(),
    );
    if target_dir.parent() != Some(toolchain.temporary.as_path())
        || !target_dir
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("velvet-glove-cargo-clippy-"))
    {
        return Err(format!(
            "cargo-clippy trace target is not a direct private child of {:?}: {target_dir:?}",
            toolchain.temporary
        ));
    }
    let invocation_dir = target_dir.join("invocation");
    if Path::new(recorded_cwd) != invocation_dir {
        return Err(format!(
            "cargo-clippy trace must run from its private invocation directory {invocation_dir:?}, got {recorded_cwd:?}"
        ));
    }

    let conf_dir = controlled_private_trace_directory(
        record,
        CLIPPY_CONF_DIR_ENV,
        &project,
        expected_conf_dir,
    )?;
    let root_config = [project.join(".clippy.toml"), project.join("clippy.toml")]
        .into_iter()
        .find(|path| path.is_file());
    if root_config.is_some() {
        if conf_dir != project {
            return Err(format!(
                "cargo-clippy trace must bind a validated root Clippy config to {project:?}, got {conf_dir:?}"
            ));
        }
    } else {
        if conf_dir != invocation_dir {
            return Err(format!(
                "cargo-clippy trace must bind its empty fallback Clippy config to {invocation_dir:?}, got {conf_dir:?}"
            ));
        }
        assert_record(record, "env-CLIPPY_CONF_DIR-clippy-toml-kind", "empty-file")?;
        assert_record(
            record,
            "env-CLIPPY_CONF_DIR-dot-clippy-toml-kind",
            "missing",
        )?;
    }
    environment.insert(
        CLIPPY_CONF_DIR_ENV.to_owned(),
        conf_dir.to_string_lossy().into_owned(),
    );

    for name in CARGO_CLIPPY_EMPTY_ENV
        .iter()
        .chain(CARGO_CLIPPY_SCRUBBED_ENV)
        .chain(CARGO_CLIPPY_PREFIX_POISON_ENV)
        .chain(CARGO_CLIPPY_LOADER_SCRUBBED_ENV)
    {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "cargo-clippy trace must clear inherited {name}, got {name}={value:?}"
            ));
        }
        environment.insert((*name).to_owned(), value);
    }
    Ok(environment)
}

fn verify_cargo_fmt_trace_environment(
    record: &Path,
    harness: &ToolTraceHarness,
    project: &Path,
    recorded_cwd: &str,
    expected_target_dir: &mut Option<PathBuf>,
) -> Result<BTreeMap<String, String>, String> {
    let toolchain = harness
        .cargo_fmt_toolchain
        .as_ref()
        .ok_or_else(|| "cargo-fmt trace has no managed toolchain binding".to_owned())?;
    let mut environment = BTreeMap::new();
    let logical_program = read_record(record, "logical-program")?;
    if !matches!(logical_program.as_str(), "cargo" | "cargo-fmt" | "rustfmt") {
        return Err(format!(
            "cargo-fmt trace recorded an unexpected logical program {logical_program:?}"
        ));
    }
    let observed_shim = PathBuf::from(read_record(record, "program")?)
        .canonicalize()
        .map_err(|error| format!("canonicalize traced {logical_program} shim: {error}"))?;
    let expected_shim = harness
        .shim_dir
        .join(&logical_program)
        .canonicalize()
        .map_err(|error| format!("canonicalize expected {logical_program} shim: {error}"))?;
    if observed_shim != expected_shim {
        return Err(format!(
            "cargo-fmt adapter escaped its {logical_program} trace shim: expected {expected_shim:?}, got {observed_shim:?}"
        ));
    }

    for (name, expected) in [
        (
            DYLD_LIBRARY_PATH_ENV,
            // macOS strips DYLD_* while resolving the trace shim's /bin/sh
            // interpreter. The paired library is asserted as a prerequisite.
            String::new(),
        ),
        (
            CARGO_PROGRAM_ENV,
            harness
                .shim_dir
                .join("cargo")
                .to_string_lossy()
                .into_owned(),
        ),
        (RUSTC_ENV, toolchain.rustc.to_string_lossy().into_owned()),
        (RUSTDOC_ENV, String::new()),
        (
            RUSTFMT_ENV,
            harness
                .shim_dir
                .join("rustfmt")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            CARGO_HOME_ENV,
            toolchain.cargo_home.to_string_lossy().into_owned(),
        ),
        (
            TMPDIR_ENV,
            toolchain.temporary.to_string_lossy().into_owned(),
        ),
        (CARGO_NET_OFFLINE_ENV, "true".to_owned()),
        (CARGO_BUILD_JOBS_ENV, "1".to_owned()),
        ("CARGO_INCREMENTAL", "0".to_owned()),
        (CARGO_TERM_COLOR_ENV, "never".to_owned()),
        ("RUST_BACKTRACE", "0".to_owned()),
        ("RUST_LIB_BACKTRACE", "0".to_owned()),
        ("TERM", "dumb".to_owned()),
    ] {
        let value = read_record(record, &format!("env-{name}"))?;
        if value != expected {
            return Err(format!(
                "cargo-fmt trace expected controlled {name}={expected:?}, got {value:?}"
            ));
        }
        environment.insert(name.to_owned(), value);
    }

    let expected_path = std::env::join_paths([
        harness.shim_dir.as_path(),
        toolchain.bin.as_path(),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .map_err(|error| format!("construct cargo-fmt child PATH: {error}"))?;
    let path = read_record(record, &format!("env-{PATH_ENV}"))?;
    if OsStr::new(&path) != expected_path {
        return Err(format!(
            "cargo-fmt trace expected controlled PATH {expected_path:?}, got {path:?}"
        ));
    }
    environment.insert(PATH_ENV.to_owned(), path);

    let project = canonical_project(project);
    let target_dir = controlled_private_trace_directory(
        record,
        CARGO_TARGET_DIR_ENV,
        &project,
        expected_target_dir,
    )?;
    environment.insert(
        CARGO_TARGET_DIR_ENV.to_owned(),
        target_dir.to_string_lossy().into_owned(),
    );
    if target_dir.parent() != Some(toolchain.temporary.as_path())
        || !target_dir
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("velvet-glove-cargo-fmt-"))
    {
        return Err(format!(
            "cargo-fmt trace target is not a direct private child of {:?}: {target_dir:?}",
            toolchain.temporary
        ));
    }
    let invocation_dir = target_dir.join("invocation");
    if Path::new(recorded_cwd) != invocation_dir {
        return Err(format!(
            "cargo-fmt trace must run from its private invocation directory {invocation_dir:?}, got {recorded_cwd:?}"
        ));
    }

    for name in CARGO_FMT_EMPTY_ENV
        .iter()
        .chain(CARGO_FMT_SCRUBBED_ENV)
        .chain(CARGO_FMT_PREFIX_POISON_ENV)
        .chain(CARGO_CLIPPY_LOADER_SCRUBBED_ENV)
    {
        let value = read_record(record, &format!("env-{name}"))?;
        if !value.is_empty() {
            return Err(format!(
                "cargo-fmt trace must clear inherited {name}, got {name}={value:?}"
            ));
        }
        environment.insert((*name).to_owned(), value);
    }
    Ok(environment)
}

fn controlled_private_trace_directory(
    record: &Path,
    name: &str,
    project: &Path,
    expected: &mut Option<PathBuf>,
) -> Result<PathBuf, String> {
    let value = read_record(record, &format!("env-{name}"))?;
    let path = PathBuf::from(&value);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!(
            "Cargo trace requires an absolute normalized {name}, got {value:?}"
        ));
    }
    assert_record(record, &format!("env-{name}-kind"), "directory")?;
    if name == CARGO_TARGET_DIR_ENV && path.starts_with(project) {
        return Err(format!(
            "Cargo trace must keep its build target outside the project, got {path:?}"
        ));
    }
    if let Some(expected) = expected {
        if path != *expected {
            return Err(format!(
                "Cargo trace changed {name} between child processes: expected {expected:?}, got {path:?}"
            ));
        }
    } else {
        *expected = Some(path.clone());
    }
    Ok(path)
}

fn cargo_clippy_trace_prerequisites(harness: &ToolTraceHarness) -> Result<JsonValue, String> {
    let toolchain = harness
        .cargo_clippy_toolchain
        .as_ref()
        .ok_or_else(|| "cargo-clippy trace has no managed toolchain prerequisites".to_owned())?;
    Ok(serde_json::json!({
        "toolchainRoot": toolchain.root,
        "bin": toolchain.bin,
        "library": toolchain.library,
        "cargo": toolchain.cargo,
        "rustc": toolchain.rustc,
        "rustdoc": toolchain.rustdoc,
        "cargoClippy": toolchain.cargo_clippy,
        "clippyDriver": toolchain.clippy_driver,
        "cargoHome": toolchain.cargo_home,
        "temporaryRoot": toolchain.temporary,
    }))
}

fn cargo_fmt_trace_prerequisites(harness: &ToolTraceHarness) -> Result<JsonValue, String> {
    let toolchain = harness
        .cargo_fmt_toolchain
        .as_ref()
        .ok_or_else(|| "cargo-fmt trace has no managed toolchain prerequisites".to_owned())?;
    Ok(serde_json::json!({
        "toolchainRoot": toolchain.root,
        "bin": toolchain.bin,
        "library": toolchain.library,
        "cargo": toolchain.cargo,
        "cargoFmt": toolchain.cargo_fmt,
        "rustfmt": toolchain.rustfmt,
        "rustc": toolchain.rustc,
        "cargoHome": toolchain.cargo_home,
        "temporaryRoot": toolchain.temporary,
    }))
}

fn read_record(record: &Path, name: &str) -> Result<String, String> {
    let path = record.join(name);
    std::fs::read_to_string(&path)
        .map(|value| value.trim_end().to_owned())
        .map_err(|error| format!("read tool trace record {path:?}: {error}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeSnapshot {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl TreeSnapshot {
    fn read(root: &Path) -> Result<Self, String> {
        let mut files = BTreeMap::new();
        collect_snapshot_files(root, root, &mut files)?;
        Ok(Self { files })
    }

    fn diff(&self, after: &Self) -> TreeDiff {
        let paths = self
            .files
            .keys()
            .chain(after.files.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();
        for path in paths {
            match (self.files.get(&path), after.files.get(&path)) {
                (None, Some(_)) => added.push(path),
                (Some(_), None) => removed.push(path),
                (Some(before), Some(after)) if before != after => changed.push(path),
                (Some(_), Some(_)) => {}
                (None, None) => unreachable!(),
            }
        }
        TreeDiff {
            added,
            removed,
            changed,
        }
    }

    fn restore(&self, root: &Path) -> Result<(), String> {
        let current = Self::read(root)?;
        for relative in current.files.keys() {
            if self.files.contains_key(relative) {
                continue;
            }
            let path = root.join(relative);
            std::fs::remove_file(&path)
                .map_err(|error| format!("remove restored-baseline addition {path:?}: {error}"))?;
        }
        for (relative, bytes) in &self.files {
            let path = root.join(relative);
            let parent = path.parent().ok_or_else(|| {
                format!("restored-baseline file has no parent directory: {path:?}")
            })?;
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("create restored-baseline directory {parent:?}: {error}")
            })?;
            std::fs::write(&path, bytes)
                .map_err(|error| format!("restore baseline file {path:?}: {error}"))?;
        }
        Ok(())
    }

    fn as_json(&self) -> JsonValue {
        JsonValue::Object(
            self.files
                .iter()
                .map(|(path, bytes)| {
                    (
                        slash_path(path),
                        serde_json::json!({
                            "bytes": bytes.len(),
                            "hex": hex_bytes(bytes),
                        }),
                    )
                })
                .collect(),
        )
    }
}

#[derive(Debug)]
struct TreeDiff {
    added: Vec<PathBuf>,
    removed: Vec<PathBuf>,
    changed: Vec<PathBuf>,
}

impl TreeDiff {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    fn describe(&self) -> String {
        format!(
            "added={:?}, removed={:?}, changed={:?}",
            self.added, self.removed, self.changed
        )
    }

    fn as_json(&self) -> JsonValue {
        serde_json::json!({
            "added": self.added.iter().map(|path| slash_path(path)).collect::<Vec<_>>(),
            "removed": self.removed.iter().map(|path| slash_path(path)).collect::<Vec<_>>(),
            "changed": self.changed.iter().map(|path| slash_path(path)).collect::<Vec<_>>(),
        })
    }
}

fn collect_snapshot_files(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Result<(), String> {
    for entry in sorted_entries(current)? {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("snapshot file type for {path:?}: {error}"))?;
        if file_type.is_dir() {
            collect_snapshot_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| format!("snapshot relative path for {path:?}: {error}"))?
                .to_path_buf();
            let bytes =
                std::fs::read(&path).map_err(|error| format!("snapshot file {path:?}: {error}"))?;
            files.insert(relative, bytes);
        } else {
            return Err(format!("snapshot does not support {path:?}"));
        }
    }
    Ok(())
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn verify_first_workspace_diff(
    case: &FixtureCase,
    contract: &RealToolContractCase,
    diff: &TreeDiff,
) -> Result<(), String> {
    if !diff.removed.is_empty() || !diff.changed.is_empty() {
        return Err(format!(
            "{} changed or removed existing workspace files: {}",
            case.tool,
            diff.describe()
        ));
    }
    let expected_artifacts = usize::from(contract.outcome != ExpectedOutcome::Clean);
    let diagnostics = format!(".velvet-glove/{}-agent-hook", case.tool);
    if diff.added.len() != expected_artifacts
        || diff.added.iter().any(|path| {
            !path.starts_with(Path::new(&diagnostics))
                || path.extension() != Some(OsStr::new("txt"))
        })
    {
        return Err(format!(
            "{} workspace diff contained unexpected additions: {}",
            case.tool,
            diff.describe()
        ));
    }
    Ok(())
}

fn validate_mutation_expected_tree(
    case: &FixtureCase,
    mutation: &MutatingToolContractCase,
) -> Result<(), String> {
    let expected_root = case.directory.join("expected");
    let mut expected_paths = BTreeSet::new();
    if expected_root.is_dir() {
        collect_relative_file_paths(&expected_root, &expected_root, &mut expected_paths)?;
    }
    let changed_paths = mutation
        .changed_targets
        .iter()
        .map(PathBuf::from)
        .collect::<BTreeSet<_>>();
    if expected_paths != changed_paths {
        return Err(format!(
            "{} mutating fixture expected/ mirror does not exactly bind changed targets: expected files {expected_paths:?}, contract {changed_paths:?}",
            case.tool
        ));
    }
    Ok(())
}

fn collect_relative_file_paths(
    root: &Path,
    current: &Path,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    for entry in sorted_entries(current)? {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("expected-tree file type for {path:?}: {error}"))?;
        if file_type.is_dir() {
            collect_relative_file_paths(root, &path, paths)?;
        } else if file_type.is_file() {
            paths.insert(
                path.strip_prefix(root)
                    .map_err(|error| format!("expected-tree relative path {path:?}: {error}"))?
                    .to_path_buf(),
            );
        } else {
            return Err(format!("expected-tree does not support {path:?}"));
        }
    }
    Ok(())
}

fn verify_mutating_workspace_diff(
    case: &FixtureCase,
    mutation: &MutatingToolContractCase,
    outcome: ExpectedOutcome,
    diff: &TreeDiff,
) -> Result<(), String> {
    if !diff.removed.is_empty() {
        return Err(format!(
            "{} mutating run removed workspace files: {}",
            case.tool,
            diff.describe()
        ));
    }
    let expected_changed = mutation
        .changed_targets
        .iter()
        .map(PathBuf::from)
        .collect::<BTreeSet<_>>();
    let actual_changed = diff.changed.iter().cloned().collect::<BTreeSet<_>>();
    if actual_changed != expected_changed {
        return Err(format!(
            "{} mutating run changed the wrong workspace files: expected {expected_changed:?}, got {}",
            case.tool,
            diff.describe()
        ));
    }
    let expected_artifacts = usize::from(outcome != ExpectedOutcome::Clean);
    let diagnostics = format!(".velvet-glove/{}-agent-hook", case.tool);
    if diff.added.len() != expected_artifacts
        || diff.added.iter().any(|path| {
            !path.starts_with(Path::new(&diagnostics))
                || path.extension() != Some(OsStr::new("txt"))
        })
    {
        return Err(format!(
            "{} mutating run added unexpected workspace files: {}",
            case.tool,
            diff.describe()
        ));
    }
    Ok(())
}

fn verify_mutating_immediate_artifact(
    case: &FixtureCase,
    contract: &RealToolContractCase,
    mutation: &MutatingToolContractCase,
    project: &Path,
) -> Result<(), String> {
    let directory = project.join(format!(".velvet-glove/{}-agent-hook", case.tool));
    let artifacts = if directory.is_dir() {
        sorted_entries(&directory)?
            .into_iter()
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.path())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let expected = usize::from(mutation.immediate_outcome != ExpectedOutcome::Clean);
    if artifacts.len() != expected {
        return Err(format!(
            "{} mutating immediate expected {expected} diagnostic artifact(s), found {artifacts:?}",
            case.tool
        ));
    }
    let Some(path) = artifacts.first() else {
        return Ok(());
    };
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("read {} immediate artifact {path:?}: {error}", case.tool))?;
    let classification = match mutation.immediate_outcome {
        ExpectedOutcome::Clean => unreachable!(),
        ExpectedOutcome::Issues => "classification: Some(Issues)",
        ExpectedOutcome::OperationalFailure => "classification: Some(Failure)",
    };
    if !contents.contains(classification) {
        return Err(format!(
            "{} mutating immediate artifact lacks {classification:?}:\n{contents}",
            case.tool
        ));
    }
    verify_stable_diagnostics(case, contract, &contents, "mutating immediate artifact")
}

fn verify_idempotent_immediate_output(
    case: &FixtureCase,
    surface: ProtocolSurface,
    output: &BoundedOutput,
    project: &Path,
) -> Result<(), String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    verify_tool_output_is_canonical(&case.tool, "idempotent immediate stdout", &stdout)?;
    verify_tool_output_is_canonical(&case.tool, "idempotent immediate stderr", &stderr)?;
    let aliases = workspace_path_aliases(project);
    let normalized_stdout = normalize(&stdout, &aliases);
    let normalized_stderr = normalize(&stderr, &aliases);
    let native = serde_json::from_str::<JsonValue>(&normalized_stdout).map_err(|error| {
        format!(
            "{} idempotent immediate {surface} output was not JSON: {error}: {normalized_stdout:?}",
            case.tool
        )
    })?;
    if output.status.code() != Some(0) || native != serde_json::json!({}) {
        return Err(format!(
            "{} idempotent immediate {surface} was not a successful native no-op: status={:?}, stdout={normalized_stdout:?}",
            case.tool,
            output.status.code()
        ));
    }
    if !normalized_stderr.trim().is_empty() {
        return Err(format!(
            "{} idempotent immediate {surface} emitted stderr:\n{normalized_stderr}",
            case.tool
        ));
    }
    Ok(())
}

fn verify_immediate_artifact(
    case: &FixtureCase,
    contract: &RealToolContractCase,
    project: &Path,
) -> Result<(), String> {
    let directory = project.join(format!(".velvet-glove/{}-agent-hook", case.tool));
    let artifacts = if directory.is_dir() {
        sorted_entries(&directory)?
            .into_iter()
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.path())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let expected = usize::from(contract.outcome != ExpectedOutcome::Clean);
    if artifacts.len() != expected {
        return Err(format!(
            "{} immediate expected {expected} diagnostic artifact(s), found {artifacts:?}",
            case.tool
        ));
    }
    let Some(path) = artifacts.first() else {
        return Ok(());
    };
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("read {} immediate artifact {path:?}: {error}", case.tool))?;
    let classification = match contract.outcome {
        ExpectedOutcome::Clean => unreachable!(),
        ExpectedOutcome::Issues => "classification: Some(Issues)",
        ExpectedOutcome::OperationalFailure => "classification: Some(Failure)",
    };
    if !contents.contains(classification) {
        return Err(format!(
            "{} immediate artifact lacks {classification:?}:\n{contents}",
            case.tool
        ));
    }
    verify_stable_diagnostics(case, contract, &contents, "immediate artifact")?;
    Ok(())
}

fn verify_stable_diagnostics(
    case: &FixtureCase,
    contract: &RealToolContractCase,
    contents: &str,
    context: &str,
) -> Result<(), String> {
    for needle in contract.diagnostic_contains {
        if !contents.contains(needle) {
            return Err(format!(
                "{} {context} lacks stable diagnostic {needle:?}:\n{contents}",
                case.tool
            ));
        }
    }
    for forbidden in contract.diagnostic_excludes {
        if contents.contains(forbidden) {
            return Err(format!(
                "{} {context} exposed forbidden diagnostic content {forbidden:?}:\n{contents}",
                case.tool
            ));
        }
    }
    if case.tool == "asciidoctor" {
        if let Some(primary) = contract.diagnostic_contains.first() {
            let occurrences = contents.matches(primary).count();
            if occurrences != 1 {
                return Err(format!(
                    "{} {context} emitted its primary diagnostic {occurrences} times, expected exactly once: {primary:?}\n{contents}",
                    case.tool
                ));
            }
        }
    }
    Ok(())
}

fn verify_repeated_output(
    tool: &str,
    first: &BoundedOutput,
    second: &BoundedOutput,
    project: &Path,
) -> Result<(), String> {
    let first_stdout_raw = String::from_utf8_lossy(&first.stdout);
    let second_stdout_raw = String::from_utf8_lossy(&second.stdout);
    let first_stderr_raw = String::from_utf8_lossy(&first.stderr);
    let second_stderr_raw = String::from_utf8_lossy(&second.stderr);
    verify_tool_output_is_canonical(tool, "first immediate stdout", &first_stdout_raw)?;
    verify_tool_output_is_canonical(tool, "second immediate stdout", &second_stdout_raw)?;
    verify_tool_output_is_canonical(tool, "first immediate stderr", &first_stderr_raw)?;
    verify_tool_output_is_canonical(tool, "second immediate stderr", &second_stderr_raw)?;
    let aliases = workspace_path_aliases(project);
    let first_stdout = normalize(&first_stdout_raw, &aliases);
    let second_stdout = normalize(&second_stdout_raw, &aliases);
    let first_stderr = normalize(&first_stderr_raw, &aliases);
    let second_stderr = normalize(&second_stderr_raw, &aliases);
    if first.status.code() != second.status.code()
        || first_stdout != second_stdout
        || first_stderr != second_stderr
    {
        return Err(format!(
            "{tool} immediate repeat changed its observable result\nfirst stdout:\n{first_stdout}\nsecond stdout:\n{second_stdout}\nfirst stderr:\n{first_stderr}\nsecond stderr:\n{second_stderr}"
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_deferred_attempt(
    case: &FixtureCase,
    surface: ProtocolSurface,
    timeout: Duration,
    workspace: &FixtureWorkspace,
    config: &Path,
    immediate_input: &support::native_events::NativePostToolInput,
    trace: &ToolTraceHarness,
    contract: &RealToolContractCase,
    resolved: &ResolvedContract,
    attempt: usize,
    project_baseline: &TreeSnapshot,
) -> Result<JsonValue, String> {
    let state_dir = workspace.root.join(format!("deferred-state-{attempt}"));
    seed_pending_targets(case, &state_dir, surface, &workspace.project, contract)?;
    let turn_input = turn_completion_input(case, surface, &workspace.project)?;
    let evidence = workspace.evidence.join(format!("deferred-{attempt}"));
    std::fs::create_dir_all(&evidence).map_err(|error| {
        format!(
            "create {} deferred evidence {evidence:?}: {error}",
            case.tool
        )
    })?;
    std::fs::write(evidence.join("input.json"), &turn_input)
        .map_err(|error| format!("write {} deferred input evidence: {error}", case.tool))?;

    let binary = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary);
    command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(config)
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("turn-completion");
    immediate_input.configure_command(&mut command);
    let trace_label = format!("deferred-{attempt}");
    trace.configure(&mut command, &trace_label)?;
    let output = run_with_timeout(&mut command, &turn_input, timeout, &evidence)
        .map_err(|error| format!("run deferred {binary} for {surface}: {error}"))?;
    std::fs::write(
        evidence.join("exit.txt"),
        format!("{}\n", output.status.code().unwrap_or(-1)),
    )
    .map_err(|error| format!("write {} deferred exit evidence: {error}", case.tool))?;
    if !output.status.success() {
        return Err(format!(
            "{} deferred {surface} attempt {attempt} exited {:?}\nstdout:\n{}\nstderr:\n{}",
            case.tool,
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    let native: JsonValue = serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "parse {} deferred {surface} attempt {attempt} native output: {error}\n{}",
            case.tool,
            String::from_utf8_lossy(&output.stdout)
        )
    })?;
    verify_deferred_native(case, contract, &native, surface)?;
    verify_tool_trace(
        trace,
        &trace_label,
        resolved,
        &workspace.project,
        &workspace
            .evidence
            .join(format!("deferred-{attempt}-trace.json")),
    )?;

    let summaries = files_named(&state_dir, "summary.json");
    if summaries.len() != 1 {
        return Err(format!(
            "{} deferred {surface} attempt {attempt} expected one summary, found {summaries:?}",
            case.tool
        ));
    }
    let summary_path = &summaries[0];
    let summary: JsonValue =
        serde_json::from_slice(&std::fs::read(summary_path).map_err(|error| {
            format!(
                "read {} deferred summary {summary_path:?}: {error}",
                case.tool
            )
        })?)
        .map_err(|error| {
            format!(
                "parse {} deferred summary {summary_path:?}: {error}",
                case.tool
            )
        })?;
    let semantic = verify_deferred_summary(case, contract, resolved, &workspace.project, &summary)?;
    write_json(
        &workspace
            .evidence
            .join(format!("deferred-{attempt}-summary.json")),
        &summary,
    )?;
    write_json(
        &workspace
            .evidence
            .join(format!("deferred-{attempt}-semantic.json")),
        &semantic,
    )?;

    let after = TreeSnapshot::read(&workspace.project)?;
    let diff = project_baseline.diff(&after);
    if !diff.is_empty() {
        return Err(format!(
            "{} deferred {surface} attempt {attempt} mutated the fixture workspace: {}",
            case.tool,
            diff.describe()
        ));
    }
    write_json(
        &workspace
            .evidence
            .join(format!("workspace-deferred-{attempt}-diff.json")),
        &diff.as_json(),
    )?;
    Ok(semantic)
}

#[allow(clippy::too_many_arguments)]
fn run_mutating_deferred_attempt(
    case: &FixtureCase,
    surface: ProtocolSurface,
    timeout: Duration,
    workspace: &FixtureWorkspace,
    config: &Path,
    immediate_input: &support::native_events::NativePostToolInput,
    trace: &ToolTraceHarness,
    contract: &RealToolContractCase,
    mutation: &MutatingToolContractCase,
    attempt: usize,
    project_baseline: &TreeSnapshot,
    expectation: &DeferredAttemptExpectation<'_>,
) -> Result<JsonValue, String> {
    let state_dir = workspace.root.join(format!("deferred-state-{attempt}"));
    seed_pending_targets(case, &state_dir, surface, &workspace.project, contract)?;
    let turn_input = turn_completion_input(case, surface, &workspace.project)?;
    let evidence = workspace.evidence.join(format!("deferred-{attempt}"));
    std::fs::create_dir_all(&evidence).map_err(|error| {
        format!(
            "create {} mutating deferred evidence {evidence:?}: {error}",
            case.tool
        )
    })?;
    std::fs::write(evidence.join("input.json"), &turn_input)
        .map_err(|error| format!("write {} deferred input evidence: {error}", case.tool))?;

    let binary = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary);
    command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(config)
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("turn-completion");
    immediate_input.configure_command(&mut command);
    let trace_label = format!("deferred-{attempt}");
    trace.configure(&mut command, &trace_label)?;
    let output = run_with_timeout(&mut command, &turn_input, timeout, &evidence)
        .map_err(|error| format!("run deferred {binary} for {surface}: {error}"))?;
    std::fs::write(
        evidence.join("exit.txt"),
        format!("{}\n", output.status.code().unwrap_or(-1)),
    )
    .map_err(|error| format!("write {} deferred exit evidence: {error}", case.tool))?;
    if !output.status.success() {
        return Err(format!(
            "{} deferred {surface} attempt {attempt} exited {:?}\nstdout:\n{}\nstderr:\n{}",
            case.tool,
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    let native: JsonValue = serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "parse {} deferred {surface} attempt {attempt} native output: {error}\n{}",
            case.tool,
            String::from_utf8_lossy(&output.stdout)
        )
    })?;
    verify_mutating_deferred_native(case, expectation, &native, surface)?;
    let expected_trace = expectation
        .phases
        .iter()
        .flat_map(|phase| phase.resolved.trace_invocations.iter())
        .cloned()
        .collect::<Vec<_>>();
    verify_tool_trace_invocations(
        trace,
        &trace_label,
        &expected_trace,
        &workspace.project,
        &workspace
            .evidence
            .join(format!("deferred-{attempt}-trace.json")),
    )?;

    let summaries = files_named(&state_dir, "summary.json");
    if summaries.len() != 1 {
        return Err(format!(
            "{} deferred {surface} attempt {attempt} expected one summary, found {summaries:?}",
            case.tool
        ));
    }
    let summary_path = &summaries[0];
    let summary: JsonValue =
        serde_json::from_slice(&std::fs::read(summary_path).map_err(|error| {
            format!(
                "read {} deferred summary {summary_path:?}: {error}",
                case.tool
            )
        })?)
        .map_err(|error| {
            format!(
                "parse {} deferred summary {summary_path:?}: {error}",
                case.tool
            )
        })?;
    let semantic = verify_mutating_deferred_summary(
        case,
        contract,
        mutation,
        expectation,
        &workspace.project,
        &summary,
    )?;
    write_json(
        &workspace
            .evidence
            .join(format!("deferred-{attempt}-summary.json")),
        &summary,
    )?;
    write_json(
        &workspace
            .evidence
            .join(format!("deferred-{attempt}-semantic.json")),
        &semantic,
    )?;

    let after = TreeSnapshot::read(&workspace.project)?;
    let diff = project_baseline.diff(&after);
    verify_mutating_deferred_diff(case, expectation, &diff)?;
    if !expectation.changed_targets.is_empty() {
        let expected_root = case.directory.join("expected");
        verify_expected_tree(&expected_root, &expected_root, &workspace.project)?;
    }
    write_json(
        &workspace
            .evidence
            .join(format!("workspace-deferred-{attempt}-diff.json")),
        &diff.as_json(),
    )?;
    Ok(semantic)
}

fn verify_mutating_deferred_native(
    case: &FixtureCase,
    expectation: &DeferredAttemptExpectation<'_>,
    native: &JsonValue,
    surface: ProtocolSurface,
) -> Result<(), String> {
    match expectation.outcome {
        ExpectedOutcome::Clean => {
            if native.get("decision").is_some() {
                return Err(format!(
                    "{} clean deferred {surface} unexpectedly blocked: {native}",
                    case.tool
                ));
            }
            let message = native
                .get("systemMessage")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "{} clean deferred {surface} lacked systemMessage: {native}",
                        case.tool
                    )
                })?;
            let needle = if expectation.fix_attempted {
                "Auto-fixed"
            } else {
                "clean"
            };
            if !message.contains(needle) {
                return Err(format!(
                    "{} clean deferred {surface} lacked {needle:?}: {message:?}",
                    case.tool
                ));
            }
        }
        ExpectedOutcome::Issues | ExpectedOutcome::OperationalFailure => {
            if native.get("decision").and_then(JsonValue::as_str) != Some("block") {
                return Err(format!(
                    "{} deferred {surface} expected a native block: {native}",
                    case.tool
                ));
            }
        }
    }
    Ok(())
}

fn verify_mutating_deferred_diff(
    case: &FixtureCase,
    expectation: &DeferredAttemptExpectation<'_>,
    diff: &TreeDiff,
) -> Result<(), String> {
    let expected_changed = expectation
        .changed_targets
        .iter()
        .map(PathBuf::from)
        .collect::<BTreeSet<_>>();
    let actual_changed = diff.changed.iter().cloned().collect::<BTreeSet<_>>();
    if !diff.added.is_empty() || !diff.removed.is_empty() || actual_changed != expected_changed {
        return Err(format!(
            "{} deferred workspace diff mismatch: expected only changed={expected_changed:?}, got {}",
            case.tool,
            diff.describe()
        ));
    }
    Ok(())
}

fn seed_pending_targets(
    case: &FixtureCase,
    state_dir: &Path,
    surface: ProtocolSurface,
    project: &Path,
    contract: &RealToolContractCase,
) -> Result<(), String> {
    let state = hookkit_session_state::SessionState::open(
        surface.harness_id(),
        hookkit_session_state::SessionIdentity::Session("test-session".into()),
        hookkit_session_state::StateRoot::new(state_dir),
    )
    .map_err(|error| format!("open {} deferred state for {surface}: {error}", case.tool))?;
    let store = hookkit_file_activity::FileActivityStore::from_state(state).map_err(|error| {
        format!(
            "open {} file-activity state for {surface}: {error}",
            case.tool
        )
    })?;
    let targets = contract
        .targets()
        .into_iter()
        .map(|relative| {
            let path = canonical_project(&project.join(relative));
            hookkit_core::Utf8PathBuf::from_path_buf(path.clone())
                .map(hookkit_file_activity::FileActivityTarget::exact)
                .map_err(|path| format!("{} deferred target is not UTF-8: {path:?}", case.tool))
        })
        .collect::<Result<Vec<_>, _>>()?;
    store
        .requeue_targets(&format!("{}-real-tool-fixture", case.tool), targets)
        .map(|_| ())
        .map_err(|error| format!("seed {} deferred targets for {surface}: {error}", case.tool))
}

fn turn_completion_input(
    case: &FixtureCase,
    surface: ProtocolSurface,
    project: &Path,
) -> Result<Vec<u8>, String> {
    let transcript = format!("/tmp/velvet-glove-{}-fixture.jsonl", case.tool);
    let value = match surface {
        ProtocolSurface::Claude => serde_json::json!({
            "session_id": "test-session",
            "transcript_path": transcript,
            "cwd": project,
            "hook_event_name": "Stop",
            "stop_hook_active": false,
            "last_assistant_message": "done",
        }),
        ProtocolSurface::Codex => serde_json::json!({
            "session_id": "test-session",
            "transcript_path": transcript,
            "cwd": project,
            "hook_event_name": "Stop",
            "model": "fixture-model",
            "turn_id": "test-turn",
            "permission_mode": "default",
            "stop_hook_active": false,
            "last_assistant_message": "done",
        }),
        ProtocolSurface::Antigravity => {
            return Err(format!(
                "{} real-tool lane does not execute Antigravity",
                case.tool
            ));
        }
    };
    serde_json::to_vec(&value)
        .map_err(|error| format!("serialize {} deferred {surface} input: {error}", case.tool))
}

fn verify_deferred_native(
    case: &FixtureCase,
    contract: &RealToolContractCase,
    native: &JsonValue,
    surface: ProtocolSurface,
) -> Result<(), String> {
    match contract.outcome {
        ExpectedOutcome::Clean => {
            if native.get("decision").is_some() {
                return Err(format!(
                    "{} clean deferred {surface} unexpectedly blocked: {native}",
                    case.tool
                ));
            }
            let message = native
                .get("systemMessage")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "{} clean deferred {surface} lacked systemMessage: {native}",
                        case.tool
                    )
                })?;
            if !message.contains("Checked") || !message.contains("clean") {
                return Err(format!(
                    "{} clean deferred {surface} had unexpected systemMessage: {message:?}",
                    case.tool
                ));
            }
        }
        ExpectedOutcome::Issues | ExpectedOutcome::OperationalFailure => {
            if native.get("decision").and_then(JsonValue::as_str) != Some("block") {
                return Err(format!(
                    "{} deferred {surface} expected a native block: {native}",
                    case.tool
                ));
            }
        }
    }
    Ok(())
}

fn expected_invocation_changed_paths(
    writes: WriteBehavior,
    expected_changed: &BTreeSet<PathBuf>,
    invocation_targets: &[PathBuf],
) -> Vec<PathBuf> {
    match writes {
        WriteBehavior::TargetFiles => invocation_targets
            .iter()
            .filter(|target| expected_changed.contains(*target))
            .cloned()
            .collect(),
        WriteBehavior::MatchingGlobs | WriteBehavior::Workspace => {
            expected_changed.iter().cloned().collect()
        }
        WriteBehavior::None => Vec::new(),
    }
}

fn verify_mutating_deferred_summary(
    case: &FixtureCase,
    contract: &RealToolContractCase,
    mutation: &MutatingToolContractCase,
    expectation: &DeferredAttemptExpectation<'_>,
    project: &Path,
    summary: &JsonValue,
) -> Result<JsonValue, String> {
    require_json_string(summary, "status", expectation.outcome.summary_status())?;
    let expected_targets = contract
        .targets()
        .into_iter()
        .map(|relative| canonical_project(&project.join(relative)))
        .collect::<Vec<_>>();
    require_path_array(summary, "candidateFiles", &expected_targets)?;
    let expected_changed = expectation
        .changed_targets
        .iter()
        .map(|relative| canonical_project(&project.join(relative)))
        .collect::<BTreeSet<_>>();
    let expected_result_paths = expected_targets
        .iter()
        .chain(expected_changed.iter())
        .cloned()
        .collect::<BTreeSet<_>>();

    let result = require_json_object(summary, "result")?;
    let artifacts = require_json_object_value(result, "artifacts")?;
    let expected_artifact_count = expectation
        .phases
        .iter()
        .map(|phase| phase.resolved.invocations.len())
        .sum::<usize>();
    if artifacts.len() != expected_artifact_count {
        return Err(format!(
            "{} mutating deferred expected {expected_artifact_count} artifacts, got {}: {artifacts:?}",
            case.tool,
            artifacts.len()
        ));
    }
    let initial = expectation
        .phases
        .first()
        .ok_or_else(|| format!("{} deferred expectation has no initial phase", case.tool))?;
    let reports = require_json_object_value(result, "reports")?;
    if reports.len() != initial.resolved.invocations.len() {
        return Err(format!(
            "{} mutating deferred expected {} reports, got {}: {reports:?}",
            case.tool,
            initial.resolved.invocations.len(),
            reports.len()
        ));
    }

    let cwd = canonical_project(project);
    let mut artifact_contracts = Vec::new();
    for phase in &expectation.phases {
        for invocation in &phase.resolved.invocations {
            let expected_classification = invocation.outcome.artifact_classification();
            let artifact = artifacts
                .values()
                .find(|artifact| {
                    artifact.get("phase").and_then(JsonValue::as_str) == Some(phase.phase)
                        && json_path_array_equals(artifact, "candidateFiles", &invocation.targets)
                })
                .ok_or_else(|| {
                    format!(
                        "{} deferred artifact missing for phase {} and {:?}: {artifacts:?}",
                        case.tool, phase.phase, invocation.targets
                    )
                })?;
            let changed = expected_invocation_changed_paths(
                mutation.remedy_writes,
                &expected_changed,
                &invocation.targets,
            );
            let files = invocation
                .targets
                .iter()
                .chain(changed.iter())
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            require_json_string(artifact, "toolId", &case.tool)?;
            require_json_string(artifact, "workflowId", mutation.remedy_phase_id)?;
            require_json_string(artifact, "phase", phase.phase)?;
            require_json_string(artifact, "classification", expected_classification)?;
            require_json_i64(artifact, "exitCode", i64::from(invocation.exit_code))?;
            require_json_string(artifact, "program", &phase.resolved.outer_program)?;
            require_string_array(artifact, "arguments", &invocation.arguments)?;
            require_json_path(artifact, "workingDirectory", &cwd)?;
            require_path_array(artifact, "files", &files)?;
            require_path_array(artifact, "candidateFiles", &invocation.targets)?;
            require_path_array(artifact, "changedFiles", &changed)?;
            let contents = artifact
                .get("contents")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "{} deferred artifact lacks text contents: {artifact}",
                        case.tool
                    )
                })?;
            verify_tool_output_is_canonical(
                &case.tool,
                "mutating deferred diagnostic artifact",
                contents,
            )?;
            if phase.assert_diagnostics {
                verify_stable_diagnostics(
                    case,
                    contract,
                    contents,
                    &format!(
                        "deferred {} artifact for {:?}",
                        phase.phase, invocation.targets
                    ),
                )?;
            }
            let absolute = artifact
                .get("absolutePath")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "{} deferred artifact lacks absolutePath: {artifact}",
                        case.tool
                    )
                })?;
            let on_disk = std::fs::read_to_string(absolute).map_err(|error| {
                format!("read {} deferred artifact {absolute:?}: {error}", case.tool)
            })?;
            if on_disk != contents {
                return Err(format!(
                    "{} deferred artifact contents differ from {absolute:?}",
                    case.tool
                ));
            }
            artifact_contracts.push(serde_json::json!({
                "targets": invocation.targets,
                "phase": phase.phase,
                "classification": expected_classification,
                "exitCode": invocation.exit_code,
                "program": phase.resolved.outer_program,
                "arguments": invocation.arguments,
                "workingDirectory": cwd,
                "changedFiles": changed,
                "contents": contents,
            }));
        }
    }

    let expected_initial_check = match expectation.initial_outcome {
        ExpectedOutcome::Clean => Some("clean"),
        ExpectedOutcome::Issues => Some("issues"),
        ExpectedOutcome::OperationalFailure => None,
    };
    let expected_final_check = match expectation.final_outcome {
        Some(ExpectedOutcome::Clean) => Some("clean"),
        Some(ExpectedOutcome::Issues) => Some("issues"),
        Some(ExpectedOutcome::OperationalFailure) | None => None,
    };
    for invocation in &initial.resolved.invocations {
        let report = reports
            .values()
            .find(|report| json_path_array_equals(report, "candidateFiles", &invocation.targets))
            .ok_or_else(|| {
                format!(
                    "{} deferred report missing for {:?}: {reports:?}",
                    case.tool, invocation.targets
                )
            })?;
        let changed = expected_invocation_changed_paths(
            mutation.remedy_writes,
            &expected_changed,
            &invocation.targets,
        );
        require_json_string(report, "toolId", &case.tool)?;
        require_json_string(report, "workflowId", mutation.remedy_phase_id)?;
        require_path_array(report, "candidateFiles", &invocation.targets)?;
        require_path_array(report, "changedFiles", &changed)?;
        require_json_bool(report, "fixAttempted", expectation.fix_attempted)?;
        require_json_bool(
            report,
            "conservativeAttribution",
            invocation.targets.len() > 1,
        )?;
        require_optional_json_string(report, "initialCheck", expected_initial_check)?;
        require_optional_json_string(report, "finalCheck", expected_final_check)?;
    }

    let files = require_json_object_value(result, "files")?;
    let operational = require_json_object_value(result, "operationalProblems")?;
    match expectation.outcome {
        ExpectedOutcome::Clean | ExpectedOutcome::Issues => {
            if files.len() != expected_result_paths.len() || !operational.is_empty() {
                return Err(format!(
                    "{} mutating deferred normal result shape mismatch: files={files:?}, operational={operational:?}",
                    case.tool
                ));
            }
            let status = match (expectation.outcome, expectation.fix_attempted) {
                (ExpectedOutcome::Clean, true) => "auto-fixed",
                (ExpectedOutcome::Clean, false) => "clean",
                (ExpectedOutcome::Issues, _) => "manual-fixes-needed",
                (ExpectedOutcome::OperationalFailure, _) => unreachable!(),
            };
            for target in &expected_result_paths {
                let file = files
                    .values()
                    .find(|file| json_path_equals(file, "path", target))
                    .ok_or_else(|| {
                        format!("{} deferred file result missing for {target:?}", case.tool)
                    })?;
                require_json_string(file, "status", status)?;
                require_json_bool(file, "changedByRunner", expected_changed.contains(target))?;
            }
        }
        ExpectedOutcome::OperationalFailure => {
            let failed_invocations = initial
                .resolved
                .invocations
                .iter()
                .filter(|invocation| invocation.outcome == ExpectedOutcome::OperationalFailure)
                .collect::<Vec<_>>();
            if !files.is_empty() || operational.len() != failed_invocations.len() {
                return Err(format!(
                    "{} mutating deferred operational shape mismatch: files={files:?}, operational={operational:?}",
                    case.tool
                ));
            }
            for invocation in failed_invocations {
                let problem = operational
                    .values()
                    .find(|problem| {
                        json_path_array_equals(problem, "affectedFiles", &invocation.targets)
                    })
                    .ok_or_else(|| {
                        format!(
                            "{} deferred operational problem missing for {:?}",
                            case.tool, invocation.targets
                        )
                    })?;
                require_json_string(problem, "toolId", &case.tool)?;
                require_json_string(problem, "phase", "initial-check")?;
            }
        }
    }

    Ok(serde_json::json!({
        "status": expectation.outcome.summary_status(),
        "targets": expected_targets,
        "artifacts": artifact_contracts,
        "fixAttempted": expectation.fix_attempted,
        "changedFiles": expected_changed,
        "fileStatuses": files.values().map(|file| serde_json::json!({
            "path": file.get("path"),
            "status": file.get("status"),
            "changedByRunner": file.get("changedByRunner"),
        })).collect::<Vec<_>>(),
        "operationalProblems": operational.values().map(|problem| serde_json::json!({
            "toolId": problem.get("toolId"),
            "phase": problem.get("phase"),
            "affectedFiles": problem.get("affectedFiles"),
            "message": problem.get("message"),
        })).collect::<Vec<_>>(),
    }))
}

fn verify_deferred_summary(
    case: &FixtureCase,
    contract: &RealToolContractCase,
    resolved: &ResolvedContract,
    project: &Path,
    summary: &JsonValue,
) -> Result<JsonValue, String> {
    let expected_status = contract.outcome.summary_status();
    require_json_string(summary, "status", expected_status)?;
    let expected_targets = contract
        .targets()
        .into_iter()
        .map(|relative| canonical_project(&project.join(relative)))
        .collect::<Vec<_>>();
    require_path_array(summary, "candidateFiles", &expected_targets)?;

    let result = require_json_object(summary, "result")?;
    let artifacts = require_json_object_value(result, "artifacts")?;
    if artifacts.len() != resolved.invocations.len() {
        return Err(format!(
            "{} deferred expected {} artifacts, got {}: {artifacts:?}",
            case.tool,
            resolved.invocations.len(),
            artifacts.len()
        ));
    }
    let reports = require_json_object_value(result, "reports")?;
    if reports.len() != resolved.invocations.len() {
        return Err(format!(
            "{} deferred expected {} reports, got {}: {reports:?}",
            case.tool,
            resolved.invocations.len(),
            reports.len()
        ));
    }

    let cwd = canonical_project(project);
    let mut artifact_contracts = Vec::new();
    for invocation in &resolved.invocations {
        let expected_classification = invocation.outcome.artifact_classification();
        let artifact = artifacts
            .values()
            .find(|artifact| {
                json_path_array_equals(artifact, "candidateFiles", &invocation.targets)
            })
            .ok_or_else(|| {
                format!(
                    "{} deferred artifact missing for {:?}: {artifacts:?}",
                    case.tool, invocation.targets
                )
            })?;
        require_json_string(artifact, "toolId", &case.tool)?;
        require_json_string(artifact, "workflowId", contract.phase_id)?;
        require_json_string(artifact, "phase", "initial-check")?;
        require_json_string(artifact, "classification", expected_classification)?;
        require_json_i64(artifact, "exitCode", i64::from(invocation.exit_code))?;
        require_json_string(artifact, "program", &resolved.outer_program)?;
        require_string_array(artifact, "arguments", &invocation.arguments)?;
        require_json_path(artifact, "workingDirectory", &cwd)?;
        require_path_array(artifact, "files", &invocation.targets)?;
        require_path_array(artifact, "candidateFiles", &invocation.targets)?;
        require_empty_array(artifact, "changedFiles")?;
        let contents = artifact
            .get("contents")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                format!(
                    "{} deferred artifact lacks text contents: {artifact}",
                    case.tool
                )
            })?;
        verify_tool_output_is_canonical(&case.tool, "deferred diagnostic artifact", contents)?;
        verify_stable_diagnostics(
            case,
            contract,
            contents,
            &format!("deferred artifact for {:?}", invocation.targets),
        )?;
        let absolute = artifact
            .get("absolutePath")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                format!(
                    "{} deferred artifact lacks absolutePath: {artifact}",
                    case.tool
                )
            })?;
        let on_disk = std::fs::read_to_string(absolute).map_err(|error| {
            format!("read {} deferred artifact {absolute:?}: {error}", case.tool)
        })?;
        if on_disk != contents {
            return Err(format!(
                "{} deferred artifact contents differ from {absolute:?}",
                case.tool
            ));
        }
        artifact_contracts.push(serde_json::json!({
            "target": (invocation.targets.len() == 1).then(|| &invocation.targets[0]),
            "targets": invocation.targets,
            "phase": "initial-check",
            "classification": expected_classification,
            "exitCode": invocation.exit_code,
            "program": resolved.outer_program,
            "arguments": invocation.arguments,
            "workingDirectory": cwd,
            "changedFiles": [],
            "contents": contents,
        }));

        let report = reports
            .values()
            .find(|report| json_path_array_equals(report, "candidateFiles", &invocation.targets))
            .ok_or_else(|| {
                format!(
                    "{} deferred report missing for {:?}: {reports:?}",
                    case.tool, invocation.targets
                )
            })?;
        require_json_string(report, "toolId", &case.tool)?;
        require_json_string(report, "workflowId", contract.phase_id)?;
        require_path_array(report, "candidateFiles", &invocation.targets)?;
        require_empty_array(report, "changedFiles")?;
        require_json_bool(report, "fixAttempted", false)?;
        require_json_bool(
            report,
            "conservativeAttribution",
            invocation.targets.len() > 1,
        )?;
        let expected_check = match invocation.outcome {
            ExpectedOutcome::Clean => Some("clean"),
            ExpectedOutcome::Issues => Some("issues"),
            ExpectedOutcome::OperationalFailure => None,
        };
        require_optional_json_string(report, "initialCheck", expected_check)?;
        require_optional_json_string(report, "finalCheck", expected_check)?;
    }
    let files = require_json_object_value(result, "files")?;
    let operational = require_json_object_value(result, "operationalProblems")?;
    match contract.outcome {
        ExpectedOutcome::Clean | ExpectedOutcome::Issues => {
            if files.len() != expected_targets.len() || !operational.is_empty() {
                return Err(format!(
                    "{} deferred normal result shape mismatch: files={files:?}, operational={operational:?}",
                    case.tool
                ));
            }
            let status = if contract.outcome == ExpectedOutcome::Clean {
                "clean"
            } else {
                "manual-fixes-needed"
            };
            for target in &expected_targets {
                let file = files
                    .values()
                    .find(|file| json_path_equals(file, "path", target))
                    .ok_or_else(|| {
                        format!("{} deferred file result missing for {target:?}", case.tool)
                    })?;
                require_json_string(file, "status", status)?;
                require_json_bool(file, "changedByRunner", false)?;
            }
        }
        ExpectedOutcome::OperationalFailure => {
            let failed_invocations = resolved
                .invocations
                .iter()
                .filter(|invocation| invocation.outcome == ExpectedOutcome::OperationalFailure)
                .collect::<Vec<_>>();
            if !files.is_empty() || operational.len() != failed_invocations.len() {
                return Err(format!(
                    "{} deferred operational shape mismatch: files={files:?}, operational={operational:?}",
                    case.tool
                ));
            }
            for invocation in failed_invocations {
                let problem = operational
                    .values()
                    .find(|problem| {
                        json_path_array_equals(problem, "affectedFiles", &invocation.targets)
                    })
                    .ok_or_else(|| {
                        format!(
                            "{} deferred operational problem missing for {:?}",
                            case.tool, invocation.targets
                        )
                    })?;
                require_json_string(problem, "toolId", &case.tool)?;
                require_json_string(problem, "phase", "initial-check")?;
                if let Some(message) = problem.get("message").and_then(JsonValue::as_str) {
                    verify_tool_output_is_canonical(
                        &case.tool,
                        "deferred operational problem",
                        message,
                    )?;
                }
            }
        }
    }

    Ok(serde_json::json!({
        "status": expected_status,
        "targets": expected_targets,
        "artifacts": artifact_contracts,
        "fileStatuses": files.values().map(|file| serde_json::json!({
            "path": file.get("path"),
            "status": file.get("status"),
            "changedByRunner": file.get("changedByRunner"),
        })).collect::<Vec<_>>(),
        "operationalProblems": operational.values().map(|problem| serde_json::json!({
            "toolId": problem.get("toolId"),
            "phase": problem.get("phase"),
            "affectedFiles": problem.get("affectedFiles"),
            "message": problem.get("message"),
        })).collect::<Vec<_>>(),
    }))
}

fn require_json_object<'a>(value: &'a JsonValue, field: &str) -> Result<&'a JsonValue, String> {
    let child = value
        .get(field)
        .ok_or_else(|| format!("missing JSON field {field:?}: {value}"))?;
    if !child.is_object() {
        return Err(format!("JSON field {field:?} is not an object: {child}"));
    }
    Ok(child)
}

fn require_json_object_value<'a>(
    value: &'a JsonValue,
    field: &str,
) -> Result<&'a serde_json::Map<String, JsonValue>, String> {
    value
        .get(field)
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("JSON field {field:?} is not an object: {value}"))
}

fn require_json_string(value: &JsonValue, field: &str, expected: &str) -> Result<(), String> {
    let actual = value.get(field).and_then(JsonValue::as_str);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} mismatch: expected {expected:?}, got {actual:?} in {value}"
        ))
    }
}

fn require_optional_json_string(
    value: &JsonValue,
    field: &str,
    expected: Option<&str>,
) -> Result<(), String> {
    let actual = value.get(field).and_then(JsonValue::as_str);
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} mismatch: expected {expected:?}, got {actual:?} in {value}"
        ))
    }
}

fn require_json_i64(value: &JsonValue, field: &str, expected: i64) -> Result<(), String> {
    let actual = value.get(field).and_then(JsonValue::as_i64);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} mismatch: expected {expected}, got {actual:?} in {value}"
        ))
    }
}

fn require_json_bool(value: &JsonValue, field: &str, expected: bool) -> Result<(), String> {
    let actual = value.get(field).and_then(JsonValue::as_bool);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} mismatch: expected {expected}, got {actual:?} in {value}"
        ))
    }
}

fn require_json_path(value: &JsonValue, field: &str, expected: &Path) -> Result<(), String> {
    if json_path_equals(value, field, expected) {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} path mismatch: expected {expected:?}, got {:?}",
            value.get(field)
        ))
    }
}

fn json_path_equals(value: &JsonValue, field: &str, expected: &Path) -> bool {
    value
        .get(field)
        .and_then(JsonValue::as_str)
        .is_some_and(|actual| Path::new(actual) == expected)
}

fn json_path_array_equals<'a>(
    value: &JsonValue,
    field: &str,
    expected: impl IntoIterator<Item = &'a PathBuf>,
) -> bool {
    let expected = expected
        .into_iter()
        .map(PathBuf::as_path)
        .collect::<Vec<_>>();
    value
        .get(field)
        .and_then(JsonValue::as_array)
        .is_some_and(|actual| {
            actual.len() == expected.len()
                && actual.iter().zip(expected).all(|(actual, expected)| {
                    actual
                        .as_str()
                        .is_some_and(|actual| Path::new(actual) == expected)
                })
        })
}

fn require_path_array<'a>(
    value: &JsonValue,
    field: &str,
    expected: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<(), String> {
    if json_path_array_equals(value, field, expected) {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} paths mismatch: got {:?} in {value}",
            value.get(field)
        ))
    }
}

fn require_string_array(value: &JsonValue, field: &str, expected: &[String]) -> Result<(), String> {
    let actual = value
        .get(field)
        .and_then(JsonValue::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .collect::<Vec<_>>()
        });
    if actual.as_deref()
        == Some(
            expected
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
        )
    {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} mismatch: expected {expected:?}, got {actual:?} in {value}"
        ))
    }
}

fn require_empty_array(value: &JsonValue, field: &str) -> Result<(), String> {
    if value
        .get(field)
        .and_then(JsonValue::as_array)
        .is_some_and(Vec::is_empty)
    {
        Ok(())
    } else {
        Err(format!(
            "JSON {field:?} expected an empty array, got {:?}",
            value.get(field)
        ))
    }
}

fn files_named(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_named(&path, name));
        } else if path.file_name().and_then(OsStr::to_str) == Some(name) {
            found.push(path);
        }
    }
    found.sort();
    found
}

fn verify_outputs(
    case: &FixtureCase,
    surface: ProtocolSurface,
    project: &Path,
    output: &BoundedOutput,
) -> Result<(), String> {
    let actual_stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let actual_stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let actual_exit = output.status.code().unwrap_or(-1);
    let project_paths = workspace_path_aliases(project);
    verify_tool_output_is_canonical(&case.tool, "protocol stdout", &actual_stdout)?;
    verify_tool_output_is_canonical(&case.tool, "protocol stderr", &actual_stderr)?;

    let stdout_golden_path = case.directory.join(format!("{}.json", surface.cli_name()));
    let golden_stdout = if stdout_golden_path.exists() {
        std::fs::read_to_string(&stdout_golden_path)
            .map_err(|error| format!("read {stdout_golden_path:?}: {error}"))?
    } else {
        "{}".to_owned()
    };
    let golden_stdout = normalize_fixture_output(case, &golden_stdout, &project_paths);
    let actual_stdout_normalized = normalize_fixture_output(case, &actual_stdout, &project_paths);
    match (
        serde_json::from_str::<JsonValue>(&golden_stdout),
        serde_json::from_str::<JsonValue>(&actual_stdout_normalized),
    ) {
        (Ok(expected), Ok(actual)) if expected == actual => {}
        (Ok(expected), Ok(actual)) => {
            return Err(format!(
                "stdout JSON mismatch:\n  expected: {expected}\n  actual:   {actual}"
            ));
        }
        _ if golden_stdout.trim() == actual_stdout_normalized.trim() => {}
        _ => {
            return Err(format!(
                "stdout mismatch:\n  expected: {golden_stdout}\n  actual:   {actual_stdout_normalized}"
            ));
        }
    }

    let stderr_golden_path = case
        .directory
        .join(format!("{}.stderr.txt", surface.cli_name()));
    if stderr_golden_path.exists() {
        let expected = std::fs::read_to_string(&stderr_golden_path)
            .map_err(|error| format!("read {stderr_golden_path:?}: {error}"))?;
        let expected = normalize_fixture_output(case, &expected, &project_paths);
        let actual = normalize_fixture_output(case, &actual_stderr, &project_paths);
        if expected.trim() != actual.trim() {
            return Err(format!(
                "stderr mismatch:\n  expected:\n{expected}\n  actual:\n{actual}"
            ));
        }
    } else {
        let actual = normalize_fixture_output(case, &actual_stderr, &project_paths);
        if !actual.trim().is_empty() {
            return Err(format!(
                "stderr expected empty but got:\n{actual}\n(write {stderr_golden_path:?} to assert content)"
            ));
        }
    }

    let exit_golden_path = case.directory.join(format!("{}.exit", surface.cli_name()));
    let expected_exit = if exit_golden_path.exists() {
        std::fs::read_to_string(&exit_golden_path)
            .map_err(|error| format!("read {exit_golden_path:?}: {error}"))?
            .trim()
            .parse::<i32>()
            .map_err(|error| format!("parse {exit_golden_path:?}: {error}"))?
    } else {
        0
    };
    if actual_exit != expected_exit {
        return Err(format!(
            "exit code mismatch: expected {expected_exit}, got {actual_exit}\nstdout:\n{actual_stdout}\nstderr:\n{actual_stderr}"
        ));
    }

    let expected_root = case.directory.join("expected");
    if expected_root.exists() {
        verify_expected_tree(&expected_root, &expected_root, project)?;
    }
    Ok(())
}

fn verify_expected_tree(root: &Path, current: &Path, project: &Path) -> Result<(), String> {
    for entry in sorted_entries(current)? {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {path:?}: {error}"))?;
        if file_type.is_dir() {
            verify_expected_tree(root, &path, project)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("strip expected prefix from {path:?}: {error}"))?;
        let actual_path = project.join(relative);
        let expected = std::fs::read_to_string(&path)
            .map_err(|error| format!("read expected {path:?}: {error}"))?;
        let actual = std::fs::read_to_string(&actual_path).map_err(|error| {
            format!("read post-run {actual_path:?} for expected/{relative:?}: {error}")
        })?;
        if expected != actual {
            return Err(format!(
                "post-run file mismatch for {relative:?}:\n  expected:\n{expected}\n  actual:\n{actual}"
            ));
        }
    }
    Ok(())
}

fn resolve_prettier_fixture_cli() -> Result<PathBuf, String> {
    if let Some(toolchain) = PrettierToolchain::resolve_if_configured()? {
        return Ok(toolchain.cli);
    }
    let requested = resolve_program("prettier")
        .ok_or_else(|| "Prettier fixture could not resolve its managed CLI".to_owned())?;
    requested
        .canonicalize()
        .map_err(|error| format!("canonicalize managed Prettier CLI {requested:?}: {error}"))
}

fn resolve_eslint_fixture_cli() -> Result<PathBuf, String> {
    if let Some(toolchain) = EslintToolchain::resolve_if_configured()? {
        return Ok(toolchain.cli);
    }
    let requested = resolve_program("eslint")
        .ok_or_else(|| "ESLint fixture could not resolve its managed CLI".to_owned())?;
    requested
        .canonicalize()
        .map_err(|error| format!("canonicalize managed ESLint CLI {requested:?}: {error}"))
}

fn resolve_contextlint_fixture_cli() -> Result<PathBuf, String> {
    if let Some(toolchain) = ContextlintToolchain::resolve_if_configured()? {
        return Ok(toolchain.cli);
    }
    let requested = resolve_program("contextlint")
        .ok_or_else(|| "Contextlint fixture could not resolve its managed CLI".to_owned())?;
    requested
        .canonicalize()
        .map_err(|error| format!("canonicalize managed Contextlint CLI {requested:?}: {error}"))
}

fn write_pkl_config(
    project: &Path,
    tool: &str,
    property: &str,
    contract: Option<&RealToolContractCase>,
) -> Result<PathBuf, String> {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir)
        .map_err(|error| format!("create config directory {config_dir:?}: {error}"))?;
    let tool_definition = if let Some(contract) = contract {
        let extra_args = contract
            .extra_args
            .iter()
            .map(|argument| format!("\"{}\"", pkl_string(argument)))
            .collect::<Vec<_>>()
            .join("; ");
        let phase_overrides = if tool == "prettier" {
            format!(
                r#"      ["format"] {{
        extraArgs = new Listing<String> {{ {extra_args} }}
      }}
      ["verify"] {{
        extraArgs = new Listing<String> {{ {extra_args} }}
      }}"#,
            )
        } else {
            format!(
                r#"      ["{phase_id}"] {{
        extraArgs = new Listing<String> {{ {extra_args} }}
      }}"#,
                phase_id = contract.phase_id,
            )
        };
        let workflow_extra_args = if tool == "dclint" {
            format!(
                r#"
    workflows {{
      ["{phase_id}"] {{
        check {{ extraArgs = new Listing<String> {{ {extra_args} }} }}
        remedy {{ extraArgs = new Listing<String> {{ {extra_args} }} }}
      }}
    }}"#,
                phase_id = contract.phase_id,
            )
        } else {
            String::new()
        };
        let executable_override = if matches!(tool, "prettier" | "eslint" | "contextlint") {
            let executable = if tool == "prettier" {
                resolve_prettier_fixture_cli()?
            } else if tool == "eslint" {
                resolve_eslint_fixture_cli()?
            } else {
                resolve_contextlint_fixture_cli()?
            };
            format!(
                "    executable = \"{}\"\n",
                pkl_string(&executable.to_string_lossy())
            )
        } else {
            String::new()
        };
        format!(
            r#"(Builtins.{property}) {{
{executable_override}
    phases {{
{phase_overrides}
    }}{workflow_extra_args}
  }}"#,
        )
    } else {
        format!("Builtins.{property}")
    };
    let contract_settings = if contract.is_some() {
        "  jobs = 1\n  fileActivity { filesystemMtime = false }\n"
    } else {
        ""
    };
    let body = format!(
        r#"amends "Config.pkl"
import "Builtins.pkl"

settings {{
{contract_settings}
  diagnosticsDirectory = ".velvet-glove/{tool}-agent-hook"
}}

tools {{
  ["{tool}"] = {tool_definition}
}}
run = new Listing<String> {{ "{tool}" }}
"#
    );
    let path = config_dir.join("post-tool-use.pkl");
    std::fs::write(&path, body).map_err(|error| format!("write post-tool-use.pkl: {error}"))?;
    Ok(path)
}

fn copy_fixture_inputs(root: &Path, current: &Path, target: &Path) -> Result<(), String> {
    for entry in sorted_entries(current)? {
        let path = entry.path();
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if current == root
            && (name == OsStr::new("expected")
                || name == OsStr::new("README.md")
                || is_golden_output(&name_text))
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {path:?}: {error}"))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("strip fixture prefix from {path:?}: {error}"))?;
        let destination = target.join(relative);
        if file_type.is_dir() {
            std::fs::create_dir_all(&destination)
                .map_err(|error| format!("create {destination:?}: {error}"))?;
            copy_fixture_inputs(root, &path, target)?;
        } else if file_type.is_file() {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("create {parent:?}: {error}"))?;
            }
            std::fs::copy(&path, &destination)
                .map_err(|error| format!("copy {path:?} to {destination:?}: {error}"))?;
        } else {
            return Err(format!("unsupported fixture entry type: {path:?}"));
        }
    }
    Ok(())
}

fn find_entry_file(directory: &Path) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    let mut nested_examples = Vec::new();
    for entry in sorted_entries(directory)? {
        let path = entry.path();
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if name == OsStr::new("expected")
            || name == OsStr::new("README.md")
            || is_golden_output(&name_text)
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {path:?}: {error}"))?;
        if file_type.is_dir() {
            collect_nested_example_files(directory, &path, &mut nested_examples)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if name_text.starts_with("example.") {
            return Ok(PathBuf::from(name));
        }
        candidates.push(PathBuf::from(name));
    }
    nested_examples.sort();
    match nested_examples.as_slice() {
        [entry] => return Ok(entry.clone()),
        [] => {}
        entries => {
            return Err(format!(
                "multiple nested `example.*` entry files in {directory:?}: {entries:?}; keep exactly one nested entry marker"
            ));
        }
    }
    candidates.sort();
    candidates.into_iter().next().ok_or_else(|| {
        format!("no entry file in {directory:?}; add an `example.<ext>` at the case root")
    })
}

fn collect_nested_example_files(
    root: &Path,
    current: &Path,
    examples: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for entry in sorted_entries(current)? {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {path:?}: {error}"))?;
        if file_type.is_dir() {
            collect_nested_example_files(root, &path, examples)?;
        } else if file_type.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("example."))
        {
            examples.push(
                path.strip_prefix(root)
                    .map_err(|error| format!("strip fixture prefix from {path:?}: {error}"))?
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}

fn is_golden_output(name: &str) -> bool {
    ProtocolSurface::ALL.iter().any(|surface| {
        name == format!("{}.json", surface.cli_name())
            || name == format!("{}.stderr.txt", surface.cli_name())
            || name == format!("{}.exit", surface.cli_name())
    })
}

fn validate_supported_goldens(directory: &Path) -> Result<(), String> {
    for entry in sorted_entries(directory)? {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {:?}: {error}", entry.path()))?;
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        for surface in ProtocolSurface::ALL {
            let is_surface_golden = name == format!("{}.json", surface.cli_name())
                || name == format!("{}.stderr.txt", surface.cli_name())
                || name == format!("{}.exit", surface.cli_name());
            if is_surface_golden && !REAL_TOOL_SURFACES.contains(&surface) {
                return Err(format!(
                    "{} golden is not executed by the real-tool fixture matrix",
                    surface.cli_name()
                ));
            }
        }
    }
    Ok(())
}

fn check_tool_programs(spec: &ToolSpec) -> Result<(), Vec<String>> {
    if spec.id == "prettier" && std::env::var_os(PRETTIER_ROOT_ENV).is_some() {
        let mut missing = Vec::new();
        if !matches!(PrettierToolchain::resolve_if_configured(), Ok(Some(_))) {
            missing.push(PRETTIER_ROOT_ENV.to_owned());
        }
        if resolve_program("python").is_none() {
            missing.push("python".to_owned());
        }
        return if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        };
    }
    if spec.id == "eslint" && std::env::var_os(ESLINT_ROOT_ENV).is_some() {
        let mut missing = Vec::new();
        if !matches!(EslintToolchain::resolve_if_configured(), Ok(Some(_))) {
            missing.push(ESLINT_ROOT_ENV.to_owned());
        }
        if resolve_program("python").is_none() {
            missing.push("python".to_owned());
        }
        return if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        };
    }
    if spec.id == "contextlint" && std::env::var_os(CONTEXTLINT_ROOT_ENV).is_some() {
        let mut missing = Vec::new();
        if !matches!(ContextlintToolchain::resolve_if_configured(), Ok(Some(_))) {
            missing.push(CONTEXTLINT_ROOT_ENV.to_owned());
        }
        if resolve_program("python").is_none() {
            missing.push("python".to_owned());
        }
        return if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        };
    }
    if spec.id == "dclint" && std::env::var_os(DCLINT_ROOT_ENV).is_some() {
        let mut missing = Vec::new();
        if !matches!(DclintToolchain::resolve_if_configured(), Ok(Some(_))) {
            missing.push(DCLINT_ROOT_ENV.to_owned());
        }
        if resolve_program("python").is_none() {
            missing.push("python".to_owned());
        }
        return if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        };
    }
    let mut programs = BTreeSet::from([spec.executable.as_str()]);
    programs.extend(
        spec.phases
            .values()
            .filter(|phase| phase.enabled)
            .filter_map(|phase| phase.program.as_deref()),
    );
    let missing = programs
        .into_iter()
        .filter(|program| resolve_program(program).is_none())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

fn verify_ghalint_workflow_adapter_lifecycle(
    spec: &ToolSpec,
    timeout: Duration,
) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (spec, timeout);
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let phase = spec
            .phases
            .get("verify")
            .ok_or_else(|| "ghalint-workflow lifecycle probe lacks a verify phase".to_owned())?;
        let [
            ArgvElement::Literal(isolated),
            ArgvElement::Literal(command),
            ArgvElement::Literal(adapter),
            ArgvElement::Token(ArgToken::ToolExecutable),
            ArgvElement::Token(ArgToken::ProjectRoot),
            ArgvElement::Token(ArgToken::ExtraArgs),
            ArgvElement::Literal(marker),
            ArgvElement::Token(ArgToken::Files),
        ] = phase.argv.as_slice()
        else {
            return Err(
                "ghalint-workflow lifecycle probe could not extract the evaluated adapter"
                    .to_owned(),
            );
        };
        if isolated != "-I" || command != "-c" || marker != GHALINT_WORKFLOW_FILES_MARKER {
            return Err(format!(
                "ghalint-workflow lifecycle probe expected exact isolated workspace shape, got {isolated:?} {command:?} marker={marker:?}"
            ));
        }
        let python_program = phase
            .program
            .as_deref()
            .ok_or_else(|| "ghalint-workflow lifecycle probe lacks Python".to_owned())?;
        let python = resolve_program(python_program)
            .or_else(|| {
                (python_program == "python")
                    .then(|| resolve_program("python3"))
                    .flatten()
            })
            .ok_or_else(|| format!("cannot resolve lifecycle Python {python_program:?}"))?
            .canonicalize()
            .map_err(|error| format!("canonicalize lifecycle Python: {error}"))?;

        let temporary = unique_temp_dir("velvet-glove-ghalint-lifecycle");
        let root = temporary
            .canonicalize()
            .map_err(|error| format!("canonicalize ghalint lifecycle root: {error}"))?;
        let result = (|| {
            let bin = root.join("bin");
            let adapter_tmp = root.join("adapter-tmp");
            let outer_home = root.join("outer-home");
            for directory in [&bin, &adapter_tmp, &outer_home] {
                std::fs::create_dir(directory).map_err(|error| {
                    format!("create ghalint lifecycle directory {directory:?}: {error}")
                })?;
            }
            let tool = bin.join("ghalint");
            let tool_source = r#"#!/bin/sh
set -eu
expected_path=${0%/*}:/usr/bin:/bin
[ "${PATH-}" = "$expected_path" ]
[ "${LANG-}" = C ]
[ "${LC_ALL-}" = C ]
[ "${TZ-}" = UTC ]
[ "${TERM-}" = dumb ]
[ "${CI-}" = 1 ]
[ "${NO_COLOR-}" = 1 ]
[ "${CLICOLOR-}" = 0 ]
[ "${FORCE_COLOR-}" = 0 ]
[ "${GHALINT_LOG_COLOR-}" = never ]
[ "${GHALINT_LOG_LEVEL-}" = error ]
[ -z "${GHALINT_VELVET_GLOVE_POISON-}" ]
[ "${HOME-}" = "${XDG_CACHE_HOME-}" ]
[ "${HOME-}" = "${XDG_CONFIG_HOME-}" ]
[ "${HOME-}" = "${XDG_DATA_HOME-}" ]
[ -d "${HOME-}" ]
[ -d "${TMPDIR-}" ]
if [ "${1-}" = --version ]; then
  if [ -f mode-bad-version ]; then
    printf '%s\n' 'ghalint version 1.5.6'
  else
    printf '%s\n' 'ghalint version 1.5.6+velvet-glove.1'
  fi
  exit 0
fi
if [ -f mode-source ]; then
  printf '%s\n' 'Jan  1 00:00:00.000 ERR the job violates policies program=ghalint version=1.5.6+velvet-glove.1 workflow_file_path=.github/workflows/example.yml policy_name=job_permissions reference=https://github.com/suzuki-shunsuke/ghalint/blob/main/docs/policies/001.md job_name=test error="job should have permissions"' >&2
  printf '%s\n' 'Jan  1 00:00:00.001 ERR the job violates policies program=ghalint version=1.5.6+velvet-glove.1 workflow_file_path=.github/workflows/example.yml policy_name=job_timeout_minutes_is_required reference=https://github.com/suzuki-shunsuke/ghalint/blob/main/docs/policies/012.md job_name=test error="job'\''s timeout-minutes is required"' >&2
  exit 1
fi
if [ -f mode-policy-grammar ]; then
  printf '%s\n' 'Jan  1 00:00:00.000 ERR secret should not be set to workflow'\''s env program=ghalint version=1.5.6+velvet-glove.1 workflow_file_path=.github/workflows/example.yml policy_name=workflow_secrets reference=https://github.com/suzuki-shunsuke/ghalint/blob/main/docs/policies/005.md env_name=GLOBAL_SECRET' >&2
  printf '%s\n' 'Jan  1 00:00:00.001 ERR github.token should not be set to workflow'\''s env program=ghalint version=1.5.6+velvet-glove.1 workflow_file_path=.github/workflows/example.yml policy_name=workflow_secrets reference=https://github.com/suzuki-shunsuke/ghalint/blob/main/docs/policies/005.md env_name=GLOBAL_TOKEN' >&2
  printf '%s\n' 'Jan  1 00:00:00.002 ERR the job violates policies program=ghalint version=1.5.6+velvet-glove.1 workflow_file_path=.github/workflows/example.yml policy_name=action_ref_should_be_full_length_commit_sha reference=https://github.com/suzuki-shunsuke/ghalint/blob/main/docs/policies/008.md job_name=reusable action=example/reusable/.github/workflows/check.yml error="action ref should be full length SHA"' >&2
  printf '%s\n' 'Jan  1 00:00:00.003 ERR the step violates policies program=ghalint version=1.5.6+velvet-glove.1 workflow_file_path=.github/workflows/example.yml policy_name=github_app_should_limit_permissions reference=https://github.com/suzuki-shunsuke/ghalint/blob/main/docs/policies/010.md job_name=test step_id=app-token step_name="under-scoped app token" action=actions/create-github-app-token error="an input `permission-*` is required"' >&2
  exit 1
fi
if [ -f mode-parse ]; then
  printf '%s\n' 'Jan  1 00:00:00.000 ERR read a workflow file program=ghalint version=1.5.6+velvet-glove.1 workflow_file_path=.github/workflows/example.yml reference=https://github.com/suzuki-shunsuke/ghalint/blob/main/docs/codes/001.md error="parse a workflow file as YAML: fixture parse failure"' >&2
  exit 1
fi
if [ -f mode-parse-fields ]; then
  printf '%s\n' 'Jan  1 00:00:00.000 ERR read a workflow file program=ghalint version=1.5.6+velvet-glove.1 workflow_file_path=.github/workflows/example.yml permission=execute error="parse a workflow file as YAML: unknown permissions"' >&2
  printf '%s\n' 'Jan  1 00:00:00.001 ERR read a workflow file program=ghalint version=1.5.6+velvet-glove.1 workflow_file_path=.github/workflows/example.yml secrets=everything error="parse a workflow file as YAML: job secrets must be a map or `inherit`"' >&2
  exit 1
fi
if [ -f mode-config ]; then
  config=
  for argument in "$@"; do
    case "$argument" in
      --config=*) config=${argument#--config=} ;;
    esac
  done
  [ -n "$config" ]
  printf 'Jan  1 00:00:00.000 ERR ghalint failed program=ghalint version=1.5.6+velvet-glove.1 config_file=%s error="read a configuration file: EOF"\n' "$config" >&2
  exit 1
fi
if [ -f mode-config-pattern ]; then
  config=
  for argument in "$@"; do
    case "$argument" in
      --config=*) config=${argument#--config=} ;;
    esac
  done
  [ -n "$config" ]
  printf 'Jan  1 00:00:00.000 ERR ghalint failed program=ghalint version=1.5.6+velvet-glove.1 pattern_reference=https://pkg.go.dev/path#Match config_file=%s error="validate a configuration file: syntax error in pattern"\n' "$config" >&2
  exit 1
fi
if [ -f mode-config-policy ]; then
  config=
  for argument in "$@"; do
    case "$argument" in
      --config=*) config=${argument#--config=} ;;
    esac
  done
  [ -n "$config" ]
  printf 'Jan  1 00:00:00.000 ERR ghalint failed program=ghalint version=1.5.6+velvet-glove.1 policy_name=unknown-policy config_file=%s error="validate a configuration file: the policy can'\''t be excluded"\n' "$config" >&2
  exit 1
fi
if [ -f mode-mutate ]; then
  printf '%s\n' '# changed' >> .github/workflows/example.yml
  exit 0
fi
if [ -f mode-descendant ]; then
  (trap '' HUP INT TERM; while :; do /bin/sleep 1; done) </dev/null >/dev/null 2>&1 &
  exit 0
fi
exit 0
"#;
            write_executable_fixture(&tool, tool_source, "ghalint lifecycle executable")?;

            let create_case = |name: &str, mode: &str, with_workflow: bool| {
                let project = root.join(name);
                let workflows = project.join(".github/workflows");
                std::fs::create_dir_all(&workflows).map_err(|error| {
                    format!("create ghalint lifecycle workflow directory: {error}")
                })?;
                std::fs::write(project.join(format!("mode-{mode}")), "\n")
                    .map_err(|error| format!("write ghalint lifecycle mode: {error}"))?;
                let workflow = workflows.join("example.yml");
                if with_workflow {
                    std::fs::write(
                        &workflow,
                        "name: fixture\non:\n  push:\npermissions: {}\njobs:\n  test:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n    steps:\n      - run: echo fixture\n",
                    )
                    .map_err(|error| format!("write ghalint lifecycle workflow: {error}"))?;
                }
                let project = project
                    .canonicalize()
                    .map_err(|error| format!("canonicalize ghalint lifecycle project: {error}"))?;
                Ok::<_, String>((project, workflow))
            };

            let run_case = |label: &str, project: &Path, selected: &Path| {
                let mut process = Command::new(&python);
                process
                    .arg("-I")
                    .arg("-c")
                    .arg(adapter)
                    .arg(&tool)
                    .arg(project)
                    .arg(GHALINT_WORKFLOW_FILES_MARKER)
                    .arg(selected)
                    .env(TMPDIR_ENV, &adapter_tmp)
                    .env(HOME_ENV, &outer_home)
                    .env("GHALINT_LOG_COLOR", "poison")
                    .env("GHALINT_LOG_LEVEL", "poison")
                    .env("GHALINT_VELVET_GLOVE_POISON", "poison");
                run_with_timeout(
                    &mut process,
                    &[],
                    timeout,
                    &root.join("captures").join(label),
                )
                .map_err(|error| format!("ghalint lifecycle {label}: {error}"))
            };
            let assert_status = |label: &str, output: &BoundedOutput, expected: i32| {
                let actual = output.status.code().unwrap_or(-1);
                if actual != expected {
                    return Err(format!(
                        "ghalint lifecycle {label} expected {expected}, got {actual}\nstdout:\n{}\nstderr:\n{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr),
                    ));
                }
                Ok(())
            };

            let (clean_project, clean_workflow) = create_case("clean", "clean", true)?;
            let clean = run_case("clean", &clean_project, &clean_workflow)?;
            assert_status("clean", &clean, 0)?;
            if !clean.stdout.is_empty() || !clean.stderr.is_empty() {
                return Err(format!(
                    "ghalint clean lifecycle emitted output\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&clean.stdout),
                    String::from_utf8_lossy(&clean.stderr),
                ));
            }

            let (source_project, source_workflow) = create_case("source", "source", true)?;
            let source = run_case("source", &source_project, &source_workflow)?;
            assert_status("source", &source, 1)?;
            let source_stdout = String::from_utf8_lossy(&source.stdout);
            if !source_stdout.contains("\"policy\":\"job_permissions\"")
                || !source_stdout.contains("\"policy\":\"job_timeout_minutes_is_required\"")
                || !source.stderr.is_empty()
            {
                return Err(format!(
                    "ghalint source lifecycle was not canonical: {source_stdout:?} {:?}",
                    String::from_utf8_lossy(&source.stderr)
                ));
            }

            let (policy_project, policy_workflow) =
                create_case("policy-grammar", "policy-grammar", true)?;
            let policy = run_case("policy-grammar", &policy_project, &policy_workflow)?;
            assert_status("policy-grammar", &policy, 1)?;
            let policy_stdout = String::from_utf8_lossy(&policy.stdout);
            if !policy_stdout.contains("\"message\":\"secret should not be set to workflow's env\"")
                || !policy_stdout
                    .contains("\"policy\":\"action_ref_should_be_full_length_commit_sha\"")
                || !policy_stdout.contains("\"policy\":\"github_app_should_limit_permissions\"")
                || !policy.stderr.is_empty()
            {
                return Err(format!(
                    "ghalint policy-grammar lifecycle was not canonical: {policy_stdout:?} {:?}",
                    String::from_utf8_lossy(&policy.stderr)
                ));
            }

            let (parse_project, parse_workflow) = create_case("parse", "parse", true)?;
            let parse = run_case("parse", &parse_project, &parse_workflow)?;
            assert_status("parse", &parse, 1)?;
            if !String::from_utf8_lossy(&parse.stdout).contains("\"kind\":\"parse\"")
                || !parse.stderr.is_empty()
            {
                return Err("ghalint parse lifecycle was not source-classified".to_owned());
            }

            let (parse_fields_project, parse_fields_workflow) =
                create_case("parse-fields", "parse-fields", true)?;
            let parse_fields = run_case(
                "parse-fields",
                &parse_fields_project,
                &parse_fields_workflow,
            )?;
            assert_status("parse-fields", &parse_fields, 1)?;
            let parse_fields_stdout = String::from_utf8_lossy(&parse_fields.stdout);
            if !parse_fields_stdout.contains("unknown permissions")
                || !parse_fields_stdout.contains("job secrets must be a map or `inherit`")
                || !parse_fields.stderr.is_empty()
            {
                return Err(
                    "ghalint structured parse lifecycle was not source-classified".to_owned(),
                );
            }

            let (config_project, config_workflow) = create_case("config", "config", true)?;
            std::fs::write(config_project.join("ghalint.yaml"), "excludes: [\n")
                .map_err(|error| format!("write invalid ghalint lifecycle config: {error}"))?;
            let config = run_case("config", &config_project, &config_workflow)?;
            assert_status("config", &config, 2)?;
            if !config.stdout.is_empty()
                || !String::from_utf8_lossy(&config.stderr)
                    .contains("ghalint configuration is invalid")
            {
                return Err("ghalint config lifecycle was not failure-classified".to_owned());
            }

            for (name, mode, config_body) in [
                (
                    "config-pattern",
                    "config-pattern",
                    "excludes:\n  - policy_name: action_ref_should_be_full_length_commit_sha\n    action_name: \"[\"\n",
                ),
                (
                    "config-policy",
                    "config-policy",
                    "excludes:\n  - policy_name: unknown-policy\n",
                ),
            ] {
                let (project, workflow) = create_case(name, mode, true)?;
                std::fs::write(project.join("ghalint.yaml"), config_body)
                    .map_err(|error| format!("write {name} lifecycle config: {error}"))?;
                let output = run_case(name, &project, &workflow)?;
                assert_status(name, &output, 2)?;
                if !output.stdout.is_empty()
                    || !String::from_utf8_lossy(&output.stderr)
                        .contains("ghalint configuration is invalid")
                {
                    return Err(format!(
                        "ghalint {name} lifecycle was not failure-classified"
                    ));
                }
            }

            let (version_project, version_workflow) =
                create_case("bad-version", "bad-version", true)?;
            let version = run_case("bad-version", &version_project, &version_workflow)?;
            assert_status("bad-version", &version, 2)?;
            if !String::from_utf8_lossy(&version.stderr).contains("exact pinned version probe") {
                return Err("ghalint lifecycle accepted an unpatched version".to_owned());
            }

            let (empty_project, empty_workflow) = create_case("empty", "clean", false)?;
            let empty = run_case("empty", &empty_project, &empty_workflow)?;
            assert_status("empty", &empty, 2)?;
            if !String::from_utf8_lossy(&empty.stderr)
                .contains("ghalint workflow inventory is empty")
            {
                return Err("ghalint lifecycle accepted an empty native inventory".to_owned());
            }

            let (symlink_project, symlink_workflow) = create_case("symlink", "clean", true)?;
            symlink(
                &symlink_workflow,
                symlink_project.join("workflow-alias.yml"),
            )
            .map_err(|error| format!("create ghalint lifecycle symlink: {error}"))?;
            let symlink_output = run_case("symlink", &symlink_project, &symlink_workflow)?;
            assert_status("symlink", &symlink_output, 2)?;
            if !String::from_utf8_lossy(&symlink_output.stderr).contains("symbolic link") {
                return Err("ghalint lifecycle accepted a retained-project symlink".to_owned());
            }

            let (mutate_project, mutate_workflow) = create_case("mutate", "mutate", true)?;
            let mutate = run_case("mutate", &mutate_project, &mutate_workflow)?;
            assert_status("mutate", &mutate, 2)?;
            if !String::from_utf8_lossy(&mutate.stderr)
                .contains("ghalint changed retained project files")
            {
                return Err("ghalint lifecycle missed a project mutation".to_owned());
            }

            let (descendant_project, descendant_workflow) =
                create_case("descendant", "descendant", true)?;
            let descendant = run_case("descendant", &descendant_project, &descendant_workflow)?;
            assert_status("descendant", &descendant, 2)?;
            if !String::from_utf8_lossy(&descendant.stderr).contains("same-group descendant") {
                return Err("ghalint lifecycle missed a same-group descendant".to_owned());
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(root);
        result
    }
}

fn verify_cargo_fmt_adapter_lifecycle(spec: &ToolSpec, timeout: Duration) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (spec, timeout);
        return Ok(());
    }
    #[cfg(unix)]
    {
        const MODE_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_MODE";
        const CHILD_PID_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_CHILD_PID";
        const DESCENDANT_PID_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_DESCENDANT_PID";
        const ORPHAN_PID_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_ORPHAN_PID";
        const READY_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_READY";
        const INVOKED_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_INVOKED";
        const CLEANUP_READY_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_CLEANUP_READY";
        const CUTOFF_READY_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_CUTOFF_READY";
        const CUTOFF_RELEASE_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_CUTOFF_RELEASE";
        const INIT_READY_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_INIT_READY";
        const INIT_RELEASE_ENV: &str = "VELVET_GLOVE_CARGO_FMT_LIFECYCLE_INIT_RELEASE";

        let phase = spec
            .phases
            .get("format")
            .ok_or_else(|| "Cargo Fmt lifecycle probe lacks a format phase".to_owned())?;
        let [
            ArgvElement::Literal(isolated),
            ArgvElement::Literal(command),
            ArgvElement::Literal(adapter),
            ArgvElement::Token(ArgToken::ToolExecutable),
            ArgvElement::Literal(cargo_fmt),
            ArgvElement::Literal(rustfmt),
            ArgvElement::Literal(mode),
            ArgvElement::Token(ArgToken::ExtraArgs),
            ArgvElement::Literal(marker),
            ArgvElement::Token(ArgToken::WorkspaceIndicator),
        ] = phase.argv.as_slice()
        else {
            return Err(
                "Cargo Fmt lifecycle probe could not extract the evaluated adapter".to_owned(),
            );
        };
        if isolated != "-I"
            || command != "-c"
            || cargo_fmt != "cargo-fmt"
            || rustfmt != "rustfmt"
            || mode != "format"
            || marker != CARGO_FMT_WORKSPACE_MARKER
        {
            return Err(format!(
                "Cargo Fmt lifecycle probe expected exact isolated format shape, got {isolated:?} {command:?} cargo-fmt={cargo_fmt:?} rustfmt={rustfmt:?} mode={mode:?} marker={marker:?}"
            ));
        }
        let python_program = phase
            .program
            .as_deref()
            .ok_or_else(|| "Cargo Fmt lifecycle probe lacks an adapter program".to_owned())?;
        let python = resolve_program(python_program)
            .ok_or_else(|| format!("Cargo Fmt lifecycle probe cannot resolve {python_program:?}"))?
            .canonicalize()
            .map_err(|error| format!("canonicalize Cargo Fmt lifecycle Python: {error}"))?;

        let temporary = unique_temp_dir("velvet-glove-cargo-fmt-lifecycle");
        let root = temporary
            .canonicalize()
            .map_err(|error| format!("canonicalize Cargo Fmt lifecycle root: {error}"))?;
        let result = (|| {
            let bin = root.join("bin");
            let cargo_home = root.join("cargo-home");
            let adapter_tmp = root.join("adapter-tmp");
            let home = root.join("home");
            for directory in [&bin, &cargo_home, &adapter_tmp, &home] {
                std::fs::create_dir(directory).map_err(|error| {
                    format!("create Cargo Fmt lifecycle directory {directory:?}: {error}")
                })?;
            }

            let cargo_source = r#"#!/bin/sh
set -eu
mode=${VELVET_GLOVE_CARGO_FMT_LIFECYCLE_MODE:-}
case "$mode" in
  signal)
    trap 'exit 0' HUP INT TERM
    (
      trap '' HUP INT TERM
      while :; do /bin/sleep 1; done
    ) &
    printf '%s\n' "$!" > "${VELVET_GLOVE_CARGO_FMT_LIFECYCLE_DESCENDANT_PID:?}"
    printf '%s\n' "$$" > "${VELVET_GLOVE_CARGO_FMT_LIFECYCLE_CHILD_PID:?}"
    : > "${VELVET_GLOVE_CARGO_FMT_LIFECYCLE_READY:?}"
    while :; do /bin/sleep 1; done
    ;;
  output-cap)
    /usr/bin/yes x | /usr/bin/head -c 16777217
    exit 0
    ;;
  rollback-success|rollback-failure|chmod-format|mtime-only|directory-add|directory-chmod|directory-remove|normal-exit-orphan|cleanup-signal|cutoff-format)
    manifest=
    take_manifest=false
    for argument in "$@"; do
      if [ "$take_manifest" = true ]; then
        manifest=$argument
        take_manifest=false
      elif [ "$argument" = --manifest-path ]; then
        take_manifest=true
      fi
    done
    [ -n "$manifest" ] || exit 65
    workspace=${manifest%/Cargo.toml}
    printf '{"version":1,"workspace_root":"%s","target_directory":"%s","build_directory":"%s","workspace_members":["member"],"packages":[{"id":"member","manifest_path":"%s","dependencies":[],"targets":[{"src_path":"%s/src/example.rs"}]}]}\n' "$workspace" "${CARGO_TARGET_DIR:?}" "${CARGO_TARGET_DIR:?}" "$manifest" "$workspace"
    ;;
  *)
    : > "${VELVET_GLOVE_CARGO_FMT_LIFECYCLE_INVOKED:?}"
    exit 64
    ;;
esac
"#;
            write_executable_fixture(&bin.join("cargo"), cargo_source, "Cargo lifecycle fake")?;

            let cargo_fmt_source = r#"#!/bin/sh
set -eu
mode=${VELVET_GLOVE_CARGO_FMT_LIFECYCLE_MODE:-}
manifest=
take_manifest=false
for argument in "$@"; do
  if [ "$take_manifest" = true ]; then
    manifest=$argument
    take_manifest=false
  elif [ "$argument" = --manifest-path ]; then
    take_manifest=true
  fi
done
[ -n "$manifest" ] || exit 65
workspace=${manifest%/Cargo.toml}
case "$workspace" in
  */coverage-workspace)
    printf '%s\n' "$workspace/src/example.rs" "$workspace/src/other.rs"
    exit 1
    ;;
esac
case "$mode" in
  rollback-success)
    printf 'pub fn example() { }\n' > "$workspace/src/example.rs"
    ;;
  rollback-failure)
    /bin/rm -f "$workspace/src/example.rs"
    /bin/mkdir -p "$workspace/src/example.rs/target"
    : > "$workspace/src/example.rs/target/blocker"
    ;;
  chmod-format)
    printf 'pub fn example() { }\n' > "$workspace/src/example.rs"
    /bin/chmod 755 "$workspace/src/example.rs"
    printf '%s\n' "$workspace/src/example.rs"
    exit 0
    ;;
  mtime-only)
    /usr/bin/touch -t 200001010000 "$workspace/src/example.rs"
    exit 0
    ;;
  directory-add)
    /bin/mkdir "$workspace/empty-leak"
    exit 0
    ;;
  directory-chmod)
    /bin/chmod 000 "$workspace/empty-baseline"
    exit 0
    ;;
  directory-remove)
    /bin/rmdir "$workspace/empty-baseline"
    exit 0
    ;;
  normal-exit-orphan)
    printf 'pub fn example() { }\n' > "$workspace/src/example.rs"
    (
      trap '' HUP INT TERM
      exec </dev/null >/dev/null 2>&1
      /bin/sleep 1
      /bin/mkdir "$workspace/late-leak"
    ) &
    printf '%s\n' "$!" > "${VELVET_GLOVE_CARGO_FMT_LIFECYCLE_ORPHAN_PID:?}"
    printf '%s\n' "$workspace/src/example.rs"
    exit 0
    ;;
  cleanup-signal)
    delay=${CARGO_TARGET_DIR:?}/cleanup-delay
    /bin/mkdir "$delay"
    index=0
    while [ "$index" -lt 4000 ]; do
      : > "$delay/$index"
      index=$((index + 1))
    done
    printf 'pub fn example() { }\n' > "$workspace/src/example.rs"
    printf '%s\n' "$workspace/src/example.rs"
    : > "${VELVET_GLOVE_CARGO_FMT_LIFECYCLE_CLEANUP_READY:?}"
    while :; do /bin/sleep 1; done
    ;;
  cutoff-format)
    printf 'pub fn example() { }\n' > "$workspace/src/example.rs"
    printf '%s\n' "$workspace/src/example.rs"
    exit 0
    ;;
  *)
    exit 66
    ;;
esac
printf '%s\n' "$workspace/src/example.rs"
exit 2
"#;
            write_executable_fixture(
                &bin.join("cargo-fmt"),
                cargo_fmt_source,
                "cargo-fmt lifecycle fake",
            )?;
            for companion in ["rustfmt", "rustc"] {
                write_executable_fixture(
                    &bin.join(companion),
                    "#!/bin/sh\nexit 64\n",
                    &format!("Cargo Fmt lifecycle {companion} fake"),
                )?;
            }

            let workspace = root.join("workspace");
            write_cargo_fmt_lifecycle_workspace(&workspace, true)?;
            let child_path = format!("{}:/usr/bin:/bin", bin.display());
            let adapter_command_with_script =
                |script: &str,
                 selected_workspace: &Path,
                 lifecycle_mode: &str,
                 extra_argument: Option<&str>| {
                    let mut command = Command::new(&python);
                    command
                        .env_clear()
                        .args(["-I", "-c", script])
                        .arg("cargo")
                        .arg("cargo-fmt")
                        .arg("rustfmt")
                        .arg("format");
                    if let Some(extra_argument) = extra_argument {
                        command.arg(extra_argument);
                    }
                    command
                        .arg(CARGO_FMT_WORKSPACE_MARKER)
                        .arg(selected_workspace.join("Cargo.lock"))
                        .current_dir(selected_workspace)
                        .env(HOME_ENV, &home)
                        .env(TMPDIR_ENV, &adapter_tmp)
                        .env(CARGO_HOME_ENV, &cargo_home)
                        .env(PATH_ENV, &child_path)
                        .env(MODE_ENV, lifecycle_mode)
                        .env("LANG", "C")
                        .env("LC_ALL", "C")
                        .env("TERM", "dumb");
                    command
                };
            let adapter_command =
                |selected_workspace: &Path, lifecycle_mode: &str, extra_argument: Option<&str>| {
                    adapter_command_with_script(
                        adapter,
                        selected_workspace,
                        lifecycle_mode,
                        extra_argument,
                    )
                };

            for (signal_name, signal_number) in [("HUP", 1), ("INT", 2), ("TERM", 15)] {
                let child_pid_path = root.join(format!("{signal_name}.child.pid"));
                let descendant_pid_path = root.join(format!("{signal_name}.descendant.pid"));
                let ready_path = root.join(format!("{signal_name}.ready"));
                let mut command = adapter_command(&workspace, "signal", None);
                command
                    .env(CHILD_PID_ENV, &child_pid_path)
                    .env(DESCENDANT_PID_ENV, &descendant_pid_path)
                    .env(READY_ENV, &ready_path)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut outer = command.spawn().map_err(|error| {
                    format!("spawn evaluated Cargo Fmt {signal_name} adapter: {error}")
                })?;
                let outer_pid = outer.id();
                let startup_timeout = timeout.min(Duration::from_secs(5));
                let startup_deadline = std::time::Instant::now() + startup_timeout;
                while !ready_path.is_file() {
                    if let Some(status) = outer
                        .try_wait()
                        .map_err(|error| format!("poll Cargo Fmt {signal_name} adapter: {error}"))?
                    {
                        return Err(format!(
                            "Cargo Fmt {signal_name} adapter exited {status:?} before its child became ready"
                        ));
                    }
                    if std::time::Instant::now() >= startup_deadline {
                        let _ = signal_process(outer_pid, "KILL");
                        let _ = outer.wait();
                        return Err(format!(
                            "Cargo Fmt {signal_name} child did not become ready within {startup_timeout:?}"
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let child_pid =
                    read_pid_file(&child_pid_path, &format!("Cargo Fmt {signal_name} child"))?;
                let descendant_pid = read_pid_file(
                    &descendant_pid_path,
                    &format!("Cargo Fmt {signal_name} descendant"),
                )?;
                if !signal_process_group(child_pid, "0")?.success() {
                    let _ = signal_process(descendant_pid, "KILL");
                    let _ = signal_process(outer_pid, "KILL");
                    let _ = outer.wait();
                    return Err(format!(
                        "Cargo Fmt {signal_name} child {child_pid} did not lead an isolated process group"
                    ));
                }
                if !signal_process(outer_pid, signal_name)?.success() {
                    let _ = signal_process_group(child_pid, "KILL");
                    let _ = signal_process(outer_pid, "KILL");
                    let _ = outer.wait();
                    return Err(format!(
                        "send SIG{signal_name} to Cargo Fmt lifecycle adapter"
                    ));
                }
                let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                std::thread::spawn(move || {
                    let _ = sender.send(outer.wait_with_output());
                });
                let completion_timeout = timeout.min(Duration::from_secs(5));
                let output = match receiver.recv_timeout(completion_timeout) {
                    Ok(Ok(output)) => output,
                    Ok(Err(error)) => {
                        let _ = signal_process_group(child_pid, "KILL");
                        return Err(format!(
                            "wait for terminated Cargo Fmt {signal_name} adapter: {error}"
                        ));
                    }
                    Err(error) => {
                        let _ = signal_process_group(child_pid, "KILL");
                        let _ = signal_process(outer_pid, "KILL");
                        return Err(format!(
                            "Cargo Fmt {signal_name} adapter or descendant pipe remained open for {completion_timeout:?}: {error}"
                        ));
                    }
                };
                let child_alive = process_survives(child_pid, Duration::from_secs(1))?;
                let descendant_alive = process_survives(descendant_pid, Duration::from_secs(1))?;
                let group_alive = process_group_survives(child_pid, Duration::from_secs(1))?;
                if child_alive || descendant_alive || group_alive {
                    let _ = signal_process_group(child_pid, "KILL");
                }
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let expected_stderr =
                    format!("velvet-glove-cargo-fmt: received signal {signal_number}\n");
                if output.status.code() != Some(2)
                    || child_alive
                    || descendant_alive
                    || group_alive
                    || !stdout.is_empty()
                    || stderr != expected_stderr
                {
                    return Err(format!(
                        "Cargo Fmt SIG{signal_name} lifecycle mismatch: status={:?}; child={child_pid}:{child_alive}; descendant={descendant_pid}:{descendant_alive}; group={group_alive}; stdout={stdout:?}; stderr={stderr:?}",
                        output.status.code()
                    ));
                }
                assert_cargo_fmt_private_roots_removed(&adapter_tmp, &format!("SIG{signal_name}"))?;
            }

            let mut output_cap = adapter_command(&workspace, "output-cap", None);
            let output = run_with_timeout(
                &mut output_cap,
                b"",
                timeout.min(Duration::from_secs(10)),
                &root.join("output-cap-evidence"),
            )
            .map_err(|error| format!("run Cargo Fmt output-cap probe: {error}"))?;
            let output_cap_stdout = String::from_utf8_lossy(&output.stdout);
            let output_cap_stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.code() != Some(2)
                || !output_cap_stdout.is_empty()
                || !output_cap_stderr.contains("combined output exceeded 16777216 bytes")
            {
                return Err(format!(
                    "Cargo Fmt output cap did not fail closed: status={:?}; stdout={output_cap_stdout:?}; stderr={output_cap_stderr:?}",
                    output.status.code()
                ));
            }
            assert_cargo_fmt_private_roots_removed(&adapter_tmp, "output-cap")?;

            for (label, alias_kind, extra_argument, diagnostic) in [
                (
                    "symlink",
                    Some("symlink"),
                    None,
                    "path is not a unique regular file",
                ),
                (
                    "hardlink",
                    Some("hardlink"),
                    None,
                    "path is not a unique regular file",
                ),
                (
                    "extra-argument",
                    None,
                    Some("--check"),
                    "extra arguments are unsupported",
                ),
            ] {
                let rejected_workspace = root.join(format!("reject-{label}"));
                write_cargo_fmt_lifecycle_workspace(&rejected_workspace, false)?;
                match alias_kind {
                    Some("symlink") => {
                        std::os::unix::fs::symlink(
                            rejected_workspace.join("src/example.rs"),
                            rejected_workspace.join("src/alias.rs"),
                        )
                        .map_err(|error| format!("create Cargo Fmt source symlink: {error}"))?;
                    }
                    Some("hardlink") => {
                        std::fs::hard_link(
                            rejected_workspace.join("src/example.rs"),
                            rejected_workspace.join("src/alias.rs"),
                        )
                        .map_err(|error| format!("create Cargo Fmt source hardlink: {error}"))?;
                    }
                    None => {}
                    Some(other) => return Err(format!("unknown Cargo Fmt alias probe {other}")),
                }
                let invoked = root.join(format!("{label}.invoked"));
                let mut rejection = adapter_command(&rejected_workspace, "reject", extra_argument);
                rejection.env(INVOKED_ENV, &invoked);
                let output = run_with_timeout(
                    &mut rejection,
                    b"",
                    timeout.min(Duration::from_secs(5)),
                    &root.join(format!("{label}-evidence")),
                )
                .map_err(|error| format!("run Cargo Fmt {label} rejection: {error}"))?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.code() != Some(2)
                    || !stdout.is_empty()
                    || !stderr.contains(diagnostic)
                    || invoked.exists()
                {
                    return Err(format!(
                        "Cargo Fmt {label} rejection failed closed incorrectly: status={:?}; stdout={stdout:?}; stderr={stderr:?}; invoked={}",
                        output.status.code(),
                        invoked.exists()
                    ));
                }
                assert_cargo_fmt_private_roots_removed(&adapter_tmp, label)?;
            }

            let set_lifecycle_directory_mode =
                |directory: &Path, mode: u32, context: &str| -> Result<(), String> {
                    let mut permissions = std::fs::metadata(directory)
                        .map_err(|error| format!("inspect {context}: {error}"))?
                        .permissions();
                    permissions.set_mode(mode);
                    std::fs::set_permissions(directory, permissions)
                        .map_err(|error| format!("chmod {context} to {mode:o}: {error}"))
                };

            let init_workspace = root.join("initialization-failure");
            write_cargo_fmt_lifecycle_workspace(&init_workspace, true)?;
            let init_tmp = root.join("initialization-unwritable-tmp");
            std::fs::create_dir(&init_tmp)
                .map_err(|error| format!("create Cargo Fmt unwritable TMPDIR: {error}"))?;
            set_lifecycle_directory_mode(&init_tmp, 0o500, "Cargo Fmt unwritable TMPDIR")?;
            let init_invoked = root.join("initialization-failure.invoked");
            let mut init_command = adapter_command(&init_workspace, "reject", None);
            init_command
                .env(TMPDIR_ENV, &init_tmp)
                .env(INVOKED_ENV, &init_invoked);
            let init_output_result = run_with_timeout(
                &mut init_command,
                b"",
                timeout.min(Duration::from_secs(5)),
                &root.join("initialization-failure-evidence"),
            );
            set_lifecycle_directory_mode(&init_tmp, 0o700, "Cargo Fmt unwritable TMPDIR")?;
            let init_output = init_output_result
                .map_err(|error| format!("run Cargo Fmt initialization failure probe: {error}"))?;
            let init_stdout = String::from_utf8_lossy(&init_output.stdout);
            let init_stderr = String::from_utf8_lossy(&init_output.stderr);
            let init_tmp_text = init_tmp.to_string_lossy();
            if init_output.status.code() != Some(2)
                || !init_stdout.is_empty()
                || !init_stderr.contains("cannot initialize controlled Cargo Fmt execution")
                || !init_stderr.contains("<cargo-fmt-private>")
                || init_stderr.contains(init_tmp_text.as_ref())
                || init_stderr.contains("velvet-glove-cargo-fmt-")
                || init_invoked.exists()
            {
                return Err(format!(
                    "Cargo Fmt unwritable TMPDIR was not normalized and rejected before child execution: status={:?}; stdout={init_stdout:?}; stderr={init_stderr:?}; invoked={}",
                    init_output.status.code(),
                    init_invoked.exists(),
                ));
            }
            assert_cargo_fmt_private_roots_removed(&init_tmp, "initialization failure")?;

            let initialization_anchor = "initialization_mask = signal.pthread_sigmask(\n                signal.SIG_BLOCK, handled_signals\n            )\n";
            if adapter.matches(initialization_anchor).count() != 1 {
                return Err(
                    "Cargo Fmt initialization cutoff probe requires one exact SIG_BLOCK anchor"
                        .to_owned(),
                );
            }
            let initialization_offset = adapter
                .find(initialization_anchor)
                .expect("checked Cargo Fmt initialization cutoff anchor");
            let initialization_line = adapter[..initialization_offset]
                .rfind('\n')
                .map_or(0, |offset| offset + 1);
            let initialization_indent = &adapter[initialization_line..initialization_offset];
            if !initialization_indent
                .chars()
                .all(|character| character == ' ')
            {
                return Err(format!(
                    "Cargo Fmt initialization cutoff anchor has unexpected indentation {initialization_indent:?}"
                ));
            }
            let initialization_hook = format!(
                "{initialization_anchor}{initialization_indent}ready_descriptor = os.open(os.environ[{INIT_READY_ENV:?}], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)\n{initialization_indent}os.close(ready_descriptor)\n{initialization_indent}release_descriptor = os.open(os.environ[{INIT_RELEASE_ENV:?}], os.O_RDONLY)\n{initialization_indent}os.close(release_descriptor)\n"
            );
            let initialization_adapter =
                adapter.replacen(initialization_anchor, &initialization_hook, 1);
            let initialization_tmp = root.join("initialization-cutoff-unwritable-tmp");
            std::fs::create_dir(&initialization_tmp).map_err(|error| {
                format!("create Cargo Fmt initialization cutoff TMPDIR: {error}")
            })?;
            set_lifecycle_directory_mode(
                &initialization_tmp,
                0o500,
                "Cargo Fmt initialization cutoff TMPDIR",
            )?;
            let initialization_ready = root.join("initialization-cutoff.ready");
            let initialization_release = root.join("initialization-cutoff.release");
            let initialization_invoked = root.join("initialization-cutoff.invoked");
            let mkfifo = Command::new("/usr/bin/mkfifo")
                .arg(&initialization_release)
                .status()
                .map_err(|error| {
                    let _ = set_lifecycle_directory_mode(
                        &initialization_tmp,
                        0o700,
                        "Cargo Fmt initialization cutoff TMPDIR",
                    );
                    format!("create Cargo Fmt initialization cutoff FIFO: {error}")
                })?;
            if !mkfifo.success() {
                set_lifecycle_directory_mode(
                    &initialization_tmp,
                    0o700,
                    "Cargo Fmt initialization cutoff TMPDIR",
                )?;
                return Err(format!(
                    "create Cargo Fmt initialization cutoff FIFO exited {mkfifo:?}"
                ));
            }
            let mut initialization_command = adapter_command_with_script(
                &initialization_adapter,
                &init_workspace,
                "reject",
                None,
            );
            initialization_command
                .env(TMPDIR_ENV, &initialization_tmp)
                .env(INVOKED_ENV, &initialization_invoked)
                .env(INIT_READY_ENV, &initialization_ready)
                .env(INIT_RELEASE_ENV, &initialization_release)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let initialization_spawn = initialization_command.spawn();
            let mut initialization_outer = match initialization_spawn {
                Ok(child) => child,
                Err(error) => {
                    set_lifecycle_directory_mode(
                        &initialization_tmp,
                        0o700,
                        "Cargo Fmt initialization cutoff TMPDIR",
                    )?;
                    return Err(format!(
                        "spawn Cargo Fmt initialization cutoff adapter: {error}"
                    ));
                }
            };
            let initialization_outer_pid = initialization_outer.id();
            let initialization_startup_timeout = timeout.min(Duration::from_secs(5));
            let initialization_startup_deadline =
                std::time::Instant::now() + initialization_startup_timeout;
            while !initialization_ready.is_file() {
                if let Some(status) = initialization_outer.try_wait().map_err(|error| {
                    let _ = set_lifecycle_directory_mode(
                        &initialization_tmp,
                        0o700,
                        "Cargo Fmt initialization cutoff TMPDIR",
                    );
                    format!("poll Cargo Fmt initialization cutoff adapter: {error}")
                })? {
                    set_lifecycle_directory_mode(
                        &initialization_tmp,
                        0o700,
                        "Cargo Fmt initialization cutoff TMPDIR",
                    )?;
                    return Err(format!(
                        "Cargo Fmt initialization cutoff adapter exited {status:?} before the post-block hook"
                    ));
                }
                if std::time::Instant::now() >= initialization_startup_deadline {
                    let _ = signal_process(initialization_outer_pid, "KILL");
                    let _ = initialization_outer.wait();
                    set_lifecycle_directory_mode(
                        &initialization_tmp,
                        0o700,
                        "Cargo Fmt initialization cutoff TMPDIR",
                    )?;
                    return Err(format!(
                        "Cargo Fmt initialization cutoff adapter did not reach its post-block hook within {initialization_startup_timeout:?}"
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            set_lifecycle_directory_mode(
                &initialization_tmp,
                0o700,
                "Cargo Fmt initialization cutoff TMPDIR",
            )?;
            if !signal_process(initialization_outer_pid, "TERM")?.success() {
                let _ = signal_process(initialization_outer_pid, "KILL");
                let _ = initialization_outer.wait();
                return Err(
                    "send post-block initialization SIGTERM to Cargo Fmt adapter".to_owned(),
                );
            }
            std::fs::OpenOptions::new()
                .write(true)
                .open(&initialization_release)
                .map_err(|error| {
                    format!("release Cargo Fmt initialization cutoff hook: {error}")
                })?;
            let (initialization_sender, initialization_receiver) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = initialization_sender.send(initialization_outer.wait_with_output());
            });
            let initialization_output = initialization_receiver
                .recv_timeout(timeout.min(Duration::from_secs(5)))
                .map_err(|error| {
                    let _ = signal_process(initialization_outer_pid, "KILL");
                    format!("wait for Cargo Fmt initialization cutoff adapter: {error}")
                })?
                .map_err(|error| {
                    format!("collect Cargo Fmt initialization cutoff output: {error}")
                })?;
            let initialization_stdout = String::from_utf8_lossy(&initialization_output.stdout);
            let initialization_stderr = String::from_utf8_lossy(&initialization_output.stderr);
            let initialization_tmp_text = initialization_tmp.to_string_lossy();
            if initialization_output.status.code() != Some(2)
                || !initialization_stdout.is_empty()
                || !initialization_stderr
                    .contains("cannot initialize controlled Cargo Fmt execution")
                || !initialization_stderr.contains("<cargo-fmt-private>")
                || initialization_stderr.matches("received signal 15").count() != 1
                || initialization_stderr.contains(initialization_tmp_text.as_ref())
                || initialization_stderr.contains("velvet-glove-cargo-fmt-")
                || initialization_invoked.exists()
            {
                return Err(format!(
                    "Cargo Fmt initialization cutoff signal was not folded after normalized failure: status={:?}; stdout={initialization_stdout:?}; stderr={initialization_stderr:?}; invoked={}",
                    initialization_output.status.code(),
                    initialization_invoked.exists(),
                ));
            }
            assert_cargo_fmt_private_roots_removed(&initialization_tmp, "initialization cutoff")?;

            let chmod_workspace = root.join("chmod-format");
            write_cargo_fmt_lifecycle_workspace(&chmod_workspace, true)?;
            let chmod_source = chmod_workspace.join("src/example.rs");
            let chmod_original = std::fs::read(&chmod_source)
                .map_err(|error| format!("read Cargo Fmt chmod baseline: {error}"))?;
            let chmod_original_mode = std::fs::metadata(&chmod_source)
                .map_err(|error| format!("inspect Cargo Fmt chmod baseline: {error}"))?
                .permissions()
                .mode();
            let mut chmod = adapter_command(&chmod_workspace, "chmod-format", None);
            let output = run_with_timeout(
                &mut chmod,
                b"",
                timeout.min(Duration::from_secs(10)),
                &root.join("chmod-format-evidence"),
            )
            .map_err(|error| format!("run Cargo Fmt chmod probe: {error}"))?;
            let chmod_stderr = String::from_utf8_lossy(&output.stderr);
            let chmod_restored = std::fs::read(&chmod_source)
                .map_err(|error| format!("read restored Cargo Fmt chmod source: {error}"))?;
            let chmod_restored_mode = std::fs::metadata(&chmod_source)
                .map_err(|error| format!("inspect restored Cargo Fmt chmod source: {error}"))?
                .permissions()
                .mode();
            if output.status.code() != Some(2)
                || chmod_restored != chmod_original
                || chmod_restored_mode != chmod_original_mode
                || !chmod_stderr.contains("changed workspace structure or metadata")
                || !chmod_stderr.contains("src/example.rs")
            {
                return Err(format!(
                    "Cargo Fmt chmod+format was not rejected and rolled back: status={:?}; bytes_restored={}; mode_before={chmod_original_mode:o}; mode_after={chmod_restored_mode:o}; stderr={chmod_stderr:?}",
                    output.status.code(),
                    chmod_restored == chmod_original
                ));
            }
            assert_cargo_fmt_private_roots_removed(&adapter_tmp, "chmod-format")?;

            let mtime_workspace = root.join("mtime-only");
            write_cargo_fmt_lifecycle_workspace(&mtime_workspace, true)?;
            let mtime_source = mtime_workspace.join("src/example.rs");
            let mtime_original = std::fs::read(&mtime_source)
                .map_err(|error| format!("read Cargo Fmt mtime baseline: {error}"))?;
            let mtime_original_metadata = std::fs::metadata(&mtime_source)
                .map_err(|error| format!("inspect Cargo Fmt mtime baseline: {error}"))?;
            let mtime_original_mode = mtime_original_metadata.permissions().mode();
            let mtime_original_value = mtime_original_metadata
                .modified()
                .map_err(|error| format!("read Cargo Fmt baseline mtime: {error}"))?;
            let mut mtime = adapter_command(&mtime_workspace, "mtime-only", None);
            let output = run_with_timeout(
                &mut mtime,
                b"",
                timeout.min(Duration::from_secs(10)),
                &root.join("mtime-only-evidence"),
            )
            .map_err(|error| format!("run Cargo Fmt mtime-only probe: {error}"))?;
            let mtime_stderr = String::from_utf8_lossy(&output.stderr);
            let mtime_restored = std::fs::read(&mtime_source)
                .map_err(|error| format!("read restored Cargo Fmt mtime source: {error}"))?;
            let mtime_restored_metadata = std::fs::metadata(&mtime_source)
                .map_err(|error| format!("inspect restored Cargo Fmt mtime source: {error}"))?;
            let mtime_restored_mode = mtime_restored_metadata.permissions().mode();
            let mtime_restored_value = mtime_restored_metadata
                .modified()
                .map_err(|error| format!("read restored Cargo Fmt mtime: {error}"))?;
            if output.status.code() != Some(2)
                || mtime_restored != mtime_original
                || mtime_restored_mode != mtime_original_mode
                || mtime_restored_value != mtime_original_value
                || !mtime_stderr.contains("changed workspace structure or metadata")
                || !mtime_stderr.contains("src/example.rs")
            {
                return Err(format!(
                    "Cargo Fmt mtime-only mutation was not rejected and rolled back: status={:?}; bytes_restored={}; mode_before={mtime_original_mode:o}; mode_after={mtime_restored_mode:o}; mtime_before={mtime_original_value:?}; mtime_after={mtime_restored_value:?}; stderr={mtime_stderr:?}",
                    output.status.code(),
                    mtime_restored == mtime_original
                ));
            }
            assert_cargo_fmt_private_roots_removed(&adapter_tmp, "mtime-only")?;

            for (directory_mode, diagnostic, diagnostic_path) in [
                ("directory-add", "added_directories", "empty-leak"),
                ("directory-chmod", "cannot walk workspace", ""),
                ("directory-remove", "removed_directories", "empty-baseline"),
            ] {
                let directory_workspace = root.join(directory_mode);
                write_cargo_fmt_lifecycle_workspace(&directory_workspace, true)?;
                let baseline_directory = directory_workspace.join("empty-baseline");
                let baseline_directory_mode = std::fs::metadata(&baseline_directory)
                    .map_err(|error| {
                        format!("inspect Cargo Fmt {directory_mode} baseline directory: {error}")
                    })?
                    .permissions()
                    .mode();
                let directory_source = directory_workspace.join("src/example.rs");
                let directory_original = std::fs::read(&directory_source).map_err(|error| {
                    format!("read Cargo Fmt {directory_mode} baseline source: {error}")
                })?;
                let directory_source_metadata =
                    std::fs::metadata(&directory_source).map_err(|error| {
                        format!("inspect Cargo Fmt {directory_mode} baseline source: {error}")
                    })?;
                let directory_original_mode = directory_source_metadata.permissions().mode();
                let directory_original_mtime =
                    directory_source_metadata.modified().map_err(|error| {
                        format!("read Cargo Fmt {directory_mode} baseline mtime: {error}")
                    })?;
                let mut directory_command =
                    adapter_command(&directory_workspace, directory_mode, None);
                let output = run_with_timeout(
                    &mut directory_command,
                    b"",
                    timeout.min(Duration::from_secs(10)),
                    &root.join(format!("{directory_mode}-evidence")),
                )
                .map_err(|error| format!("run Cargo Fmt {directory_mode} probe: {error}"))?;
                let directory_stdout = String::from_utf8_lossy(&output.stdout);
                let directory_stderr = String::from_utf8_lossy(&output.stderr);
                let restored_directory_metadata =
                    std::fs::metadata(&baseline_directory).map_err(|error| {
                        format!("inspect restored Cargo Fmt {directory_mode} directory: {error}")
                    })?;
                let restored_directory_mode = restored_directory_metadata.permissions().mode();
                let restored_directory_empty = std::fs::read_dir(&baseline_directory)
                    .map_err(|error| {
                        format!("read restored Cargo Fmt {directory_mode} directory: {error}")
                    })?
                    .next()
                    .is_none();
                let directory_restored = std::fs::read(&directory_source).map_err(|error| {
                    format!("read restored Cargo Fmt {directory_mode} source: {error}")
                })?;
                let directory_restored_metadata =
                    std::fs::metadata(&directory_source).map_err(|error| {
                        format!("inspect restored Cargo Fmt {directory_mode} source: {error}")
                    })?;
                let directory_restored_mode = directory_restored_metadata.permissions().mode();
                let directory_restored_mtime =
                    directory_restored_metadata.modified().map_err(|error| {
                        format!("read restored Cargo Fmt {directory_mode} mtime: {error}")
                    })?;
                let leak_exists = directory_workspace.join("empty-leak").exists();
                if output.status.code() != Some(2)
                    || !directory_stdout.is_empty()
                    || directory_restored != directory_original
                    || directory_restored_mode != directory_original_mode
                    || directory_restored_mtime != directory_original_mtime
                    || restored_directory_mode != baseline_directory_mode
                    || !restored_directory_empty
                    || leak_exists
                    || !directory_stderr.contains(diagnostic)
                    || (!diagnostic_path.is_empty() && !directory_stderr.contains(diagnostic_path))
                    || directory_stderr.contains("rollback failed")
                {
                    return Err(format!(
                        "Cargo Fmt {directory_mode} mutation was not rejected and rolled back: status={:?}; stdout={directory_stdout:?}; bytes_restored={}; file_mode_before={directory_original_mode:o}; file_mode_after={directory_restored_mode:o}; file_mtime_before={directory_original_mtime:?}; file_mtime_after={directory_restored_mtime:?}; dir_mode_before={baseline_directory_mode:o}; dir_mode_after={restored_directory_mode:o}; dir_empty={restored_directory_empty}; leak_exists={leak_exists}; stderr={directory_stderr:?}",
                        output.status.code(),
                        directory_restored == directory_original,
                    ));
                }
                assert_cargo_fmt_private_roots_removed(&adapter_tmp, directory_mode)?;
            }

            let orphan_workspace = root.join("normal-exit-orphan");
            write_cargo_fmt_lifecycle_workspace(&orphan_workspace, true)?;
            let orphan_source = orphan_workspace.join("src/example.rs");
            let orphan_original = std::fs::read(&orphan_source)
                .map_err(|error| format!("read Cargo Fmt orphan baseline: {error}"))?;
            let orphan_original_metadata = std::fs::metadata(&orphan_source)
                .map_err(|error| format!("inspect Cargo Fmt orphan baseline: {error}"))?;
            let orphan_original_mode = orphan_original_metadata.permissions().mode();
            let orphan_original_mtime = orphan_original_metadata
                .modified()
                .map_err(|error| format!("read Cargo Fmt orphan baseline mtime: {error}"))?;
            let orphan_pid_path = root.join("normal-exit-orphan.pid");
            let mut orphan_command = adapter_command(&orphan_workspace, "normal-exit-orphan", None);
            orphan_command.env(ORPHAN_PID_ENV, &orphan_pid_path);
            let orphan_output = run_with_timeout(
                &mut orphan_command,
                b"",
                timeout.min(Duration::from_secs(10)),
                &root.join("normal-exit-orphan-evidence"),
            )
            .map_err(|error| format!("run Cargo Fmt normal-exit orphan probe: {error}"))?;
            let orphan_pid = read_pid_file(&orphan_pid_path, "Cargo Fmt normal-exit orphan")?;
            let orphan_alive = process_survives(orphan_pid, Duration::from_secs(1))?;
            if orphan_alive {
                let _ = signal_process(orphan_pid, "KILL");
            }
            std::thread::sleep(Duration::from_millis(1100));
            let orphan_stdout = String::from_utf8_lossy(&orphan_output.stdout);
            let orphan_stderr = String::from_utf8_lossy(&orphan_output.stderr);
            let orphan_restored = std::fs::read(&orphan_source)
                .map_err(|error| format!("read restored Cargo Fmt orphan source: {error}"))?;
            let orphan_restored_metadata = std::fs::metadata(&orphan_source)
                .map_err(|error| format!("inspect restored Cargo Fmt orphan source: {error}"))?;
            let orphan_restored_mode = orphan_restored_metadata.permissions().mode();
            let orphan_restored_mtime = orphan_restored_metadata
                .modified()
                .map_err(|error| format!("read restored Cargo Fmt orphan mtime: {error}"))?;
            let late_leak_exists = orphan_workspace.join("late-leak").exists();
            if orphan_output.status.code() != Some(2)
                || orphan_alive
                || !orphan_stdout.is_empty()
                || orphan_restored != orphan_original
                || orphan_restored_mode != orphan_original_mode
                || orphan_restored_mtime != orphan_original_mtime
                || late_leak_exists
                || !orphan_stderr.contains("child left same-group descendants after leader exit")
                || orphan_stderr.contains("rollback failed")
            {
                return Err(format!(
                    "Cargo Fmt normal-exit orphan was not swept and rolled back: status={:?}; orphan={orphan_pid}:{orphan_alive}; stdout={orphan_stdout:?}; bytes_restored={}; mode_before={orphan_original_mode:o}; mode_after={orphan_restored_mode:o}; mtime_before={orphan_original_mtime:?}; mtime_after={orphan_restored_mtime:?}; late_leak={late_leak_exists}; stderr={orphan_stderr:?}",
                    orphan_output.status.code(),
                    orphan_restored == orphan_original,
                ));
            }
            assert_cargo_fmt_private_roots_removed(&adapter_tmp, "normal-exit-orphan")?;

            let cleanup_workspace = root.join("cleanup-signal");
            write_cargo_fmt_lifecycle_workspace(&cleanup_workspace, true)?;
            let cleanup_source = cleanup_workspace.join("src/example.rs");
            let cleanup_original = std::fs::read(&cleanup_source)
                .map_err(|error| format!("read Cargo Fmt cleanup-signal baseline: {error}"))?;
            let cleanup_original_mode = std::fs::metadata(&cleanup_source)
                .map_err(|error| format!("inspect Cargo Fmt cleanup-signal baseline: {error}"))?
                .permissions()
                .mode();
            let cleanup_ready = root.join("cleanup-signal.ready");
            let mut cleanup_command = adapter_command(&cleanup_workspace, "cleanup-signal", None);
            cleanup_command
                .env(CLEANUP_READY_ENV, &cleanup_ready)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut cleanup_outer = cleanup_command
                .spawn()
                .map_err(|error| format!("spawn Cargo Fmt cleanup-signal adapter: {error}"))?;
            let cleanup_outer_pid = cleanup_outer.id();
            let cleanup_startup_timeout = timeout.min(Duration::from_secs(10));
            let cleanup_startup_deadline = std::time::Instant::now() + cleanup_startup_timeout;
            while !cleanup_ready.is_file() {
                if let Some(status) = cleanup_outer
                    .try_wait()
                    .map_err(|error| format!("poll Cargo Fmt cleanup-signal adapter: {error}"))?
                {
                    return Err(format!(
                        "Cargo Fmt cleanup-signal adapter exited {status:?} before its real formatter became ready"
                    ));
                }
                if std::time::Instant::now() >= cleanup_startup_deadline {
                    let _ = signal_process(cleanup_outer_pid, "KILL");
                    let _ = cleanup_outer.wait();
                    return Err(format!(
                        "Cargo Fmt cleanup-signal formatter did not become ready within {cleanup_startup_timeout:?}"
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let private_roots = sorted_entries(&adapter_tmp)?
                .into_iter()
                .filter_map(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with("velvet-glove-cargo-fmt-"))
                        .then(|| entry.path())
                })
                .collect::<Vec<_>>();
            let [cleanup_private_root] = private_roots.as_slice() else {
                let _ = signal_process(cleanup_outer_pid, "KILL");
                let _ = cleanup_outer.wait();
                return Err(format!(
                    "Cargo Fmt cleanup-signal expected one private root, got {private_roots:?}"
                ));
            };
            let cleanup_deadline = std::time::Instant::now() + timeout.min(Duration::from_secs(10));
            let mut cleanup_signal_count = 0usize;
            loop {
                let source_restored =
                    std::fs::read(&cleanup_source).is_ok_and(|content| content == cleanup_original);
                if !cleanup_private_root.exists() && source_restored {
                    break;
                }
                if std::time::Instant::now() >= cleanup_deadline {
                    let _ = signal_process(cleanup_outer_pid, "KILL");
                    let _ = cleanup_outer.wait();
                    return Err(format!(
                        "Cargo Fmt cleanup-signal adapter did not finish owned cleanup and rollback after {cleanup_signal_count} SIGTERMs"
                    ));
                }
                if !signal_process(cleanup_outer_pid, "TERM")?.success() {
                    break;
                }
                cleanup_signal_count += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
            let (cleanup_sender, cleanup_receiver) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = cleanup_sender.send(cleanup_outer.wait_with_output());
            });
            let cleanup_output = cleanup_receiver
                .recv_timeout(timeout.min(Duration::from_secs(10)))
                .map_err(|error| {
                    let _ = signal_process(cleanup_outer_pid, "KILL");
                    format!("wait for Cargo Fmt cleanup-signal adapter: {error}")
                })?
                .map_err(|error| format!("collect Cargo Fmt cleanup-signal output: {error}"))?;
            let cleanup_stdout = String::from_utf8_lossy(&cleanup_output.stdout);
            let cleanup_stderr = String::from_utf8_lossy(&cleanup_output.stderr);
            let cleanup_restored = std::fs::read(&cleanup_source).map_err(|error| {
                format!("read restored Cargo Fmt cleanup-signal source: {error}")
            })?;
            let cleanup_restored_mode = std::fs::metadata(&cleanup_source)
                .map_err(|error| {
                    format!("inspect restored Cargo Fmt cleanup-signal source: {error}")
                })?
                .permissions()
                .mode();
            if cleanup_output.status.code() != Some(2)
                || cleanup_signal_count < 2
                || cleanup_restored != cleanup_original
                || cleanup_restored_mode != cleanup_original_mode
                || cleanup_stderr != "velvet-glove-cargo-fmt: received signal 15\n"
                || !cleanup_stdout.is_empty()
            {
                return Err(format!(
                    "Cargo Fmt cleanup-window signal was not contained: status={:?}; signals={cleanup_signal_count}; bytes_restored={}; mode_before={cleanup_original_mode:o}; mode_after={cleanup_restored_mode:o}; stdout={cleanup_stdout:?}; stderr={cleanup_stderr:?}",
                    cleanup_output.status.code(),
                    cleanup_restored == cleanup_original
                ));
            }
            assert_cargo_fmt_private_roots_removed(&adapter_tmp, "cleanup-signal")?;

            let cutoff_anchor =
                "blocked_mask = signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)\n";
            if adapter.matches(cutoff_anchor).count() != 1 {
                return Err(
                    "Cargo Fmt cutoff probe requires one exact live SIG_BLOCK anchor".to_owned(),
                );
            }
            let cutoff_offset = adapter
                .find(cutoff_anchor)
                .expect("checked Cargo Fmt cutoff anchor");
            let cutoff_line = adapter[..cutoff_offset]
                .rfind('\n')
                .map_or(0, |offset| offset + 1);
            let cutoff_indent = &adapter[cutoff_line..cutoff_offset];
            if !cutoff_indent.chars().all(|character| character == ' ') {
                return Err(format!(
                    "Cargo Fmt cutoff anchor has unexpected indentation {cutoff_indent:?}"
                ));
            }
            let cutoff_hook = format!(
                "{cutoff_anchor}{cutoff_indent}ready_descriptor = os.open(os.environ[{CUTOFF_READY_ENV:?}], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)\n{cutoff_indent}os.close(ready_descriptor)\n{cutoff_indent}release_descriptor = os.open(os.environ[{CUTOFF_RELEASE_ENV:?}], os.O_RDONLY)\n{cutoff_indent}os.close(release_descriptor)\n"
            );
            let cutoff_adapter = adapter.replacen(cutoff_anchor, &cutoff_hook, 1);
            let cutoff_workspace = root.join("cutoff-format");
            write_cargo_fmt_lifecycle_workspace(&cutoff_workspace, true)?;
            let cutoff_source = cutoff_workspace.join("src/example.rs");
            let cutoff_original = std::fs::read(&cutoff_source)
                .map_err(|error| format!("read Cargo Fmt cutoff baseline: {error}"))?;
            let cutoff_original_mode = std::fs::metadata(&cutoff_source)
                .map_err(|error| format!("inspect Cargo Fmt cutoff baseline: {error}"))?
                .permissions()
                .mode();
            let cutoff_ready = root.join("cutoff.ready");
            let cutoff_release = root.join("cutoff.release");
            let mkfifo = Command::new("/usr/bin/mkfifo")
                .arg(&cutoff_release)
                .status()
                .map_err(|error| format!("create Cargo Fmt cutoff release FIFO: {error}"))?;
            if !mkfifo.success() {
                return Err(format!(
                    "create Cargo Fmt cutoff release FIFO exited {mkfifo:?}"
                ));
            }
            let mut cutoff_command = adapter_command_with_script(
                &cutoff_adapter,
                &cutoff_workspace,
                "cutoff-format",
                None,
            );
            cutoff_command
                .env(CUTOFF_READY_ENV, &cutoff_ready)
                .env(CUTOFF_RELEASE_ENV, &cutoff_release)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut cutoff_outer = cutoff_command
                .spawn()
                .map_err(|error| format!("spawn Cargo Fmt cutoff adapter: {error}"))?;
            let cutoff_outer_pid = cutoff_outer.id();
            let cutoff_startup_timeout = timeout.min(Duration::from_secs(10));
            let cutoff_startup_deadline = std::time::Instant::now() + cutoff_startup_timeout;
            while !cutoff_ready.is_file() {
                if let Some(status) = cutoff_outer
                    .try_wait()
                    .map_err(|error| format!("poll Cargo Fmt cutoff adapter: {error}"))?
                {
                    return Err(format!(
                        "Cargo Fmt cutoff adapter exited {status:?} before the post-block hook"
                    ));
                }
                if std::time::Instant::now() >= cutoff_startup_deadline {
                    let _ = signal_process(cutoff_outer_pid, "KILL");
                    let _ = cutoff_outer.wait();
                    return Err(format!(
                        "Cargo Fmt cutoff adapter did not reach its post-block hook within {cutoff_startup_timeout:?}"
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_cargo_fmt_private_roots_removed(&adapter_tmp, "post-block cutoff")?;
            if std::fs::read(&cutoff_source).is_ok_and(|content| content == cutoff_original) {
                let _ = signal_process(cutoff_outer_pid, "KILL");
                let _ = cutoff_outer.wait();
                return Err("Cargo Fmt cutoff hook ran before the real mutation".to_owned());
            }
            if !signal_process(cutoff_outer_pid, "TERM")?.success() {
                let _ = signal_process(cutoff_outer_pid, "KILL");
                let _ = cutoff_outer.wait();
                return Err("send post-block SIGTERM to Cargo Fmt cutoff adapter".to_owned());
            }
            std::fs::OpenOptions::new()
                .write(true)
                .open(&cutoff_release)
                .map_err(|error| format!("release Cargo Fmt cutoff hook: {error}"))?;
            let (cutoff_sender, cutoff_receiver) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = cutoff_sender.send(cutoff_outer.wait_with_output());
            });
            let cutoff_output = cutoff_receiver
                .recv_timeout(timeout.min(Duration::from_secs(10)))
                .map_err(|error| {
                    let _ = signal_process(cutoff_outer_pid, "KILL");
                    format!("wait for Cargo Fmt cutoff adapter: {error}")
                })?
                .map_err(|error| format!("collect Cargo Fmt cutoff output: {error}"))?;
            let cutoff_stdout = String::from_utf8_lossy(&cutoff_output.stdout);
            let cutoff_stderr = String::from_utf8_lossy(&cutoff_output.stderr);
            let cutoff_restored = std::fs::read(&cutoff_source)
                .map_err(|error| format!("read restored Cargo Fmt cutoff source: {error}"))?;
            let cutoff_restored_mode = std::fs::metadata(&cutoff_source)
                .map_err(|error| format!("inspect restored Cargo Fmt cutoff source: {error}"))?
                .permissions()
                .mode();
            if cutoff_output.status.code() != Some(2)
                || cutoff_restored != cutoff_original
                || cutoff_restored_mode != cutoff_original_mode
                || cutoff_stderr != "velvet-glove-cargo-fmt: received signal 15\n"
                || !cutoff_stdout.contains("src/example.rs")
            {
                return Err(format!(
                    "Cargo Fmt post-block signal was not folded into exact rollback: status={:?}; bytes_restored={}; mode_before={cutoff_original_mode:o}; mode_after={cutoff_restored_mode:o}; stdout={cutoff_stdout:?}; stderr={cutoff_stderr:?}",
                    cutoff_output.status.code(),
                    cutoff_restored == cutoff_original
                ));
            }
            assert_cargo_fmt_private_roots_removed(&adapter_tmp, "cutoff-format")?;

            for (lifecycle_mode, rollback_must_succeed) in
                [("rollback-success", true), ("rollback-failure", false)]
            {
                let rollback_workspace = root.join(lifecycle_mode);
                write_cargo_fmt_lifecycle_workspace(&rollback_workspace, true)?;
                let original = std::fs::read(rollback_workspace.join("src/example.rs"))
                    .map_err(|error| format!("read Cargo Fmt rollback baseline: {error}"))?;
                let mut rollback = adapter_command(&rollback_workspace, lifecycle_mode, None);
                let output = run_with_timeout(
                    &mut rollback,
                    b"",
                    timeout.min(Duration::from_secs(10)),
                    &root.join(format!("{lifecycle_mode}-evidence")),
                )
                .map_err(|error| format!("run Cargo Fmt {lifecycle_mode} probe: {error}"))?;
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.code() != Some(2) {
                    return Err(format!(
                        "Cargo Fmt {lifecycle_mode} exited {:?}, expected 2; stdout={:?}; stderr={stderr:?}",
                        output.status.code(),
                        String::from_utf8_lossy(&output.stdout)
                    ));
                }
                let source = rollback_workspace.join("src/example.rs");
                if rollback_must_succeed {
                    let restored = std::fs::read(&source).map_err(|error| {
                        format!("read restored Cargo Fmt lifecycle source: {error}")
                    })?;
                    if restored != original || !stderr.contains("cargo-fmt format exited 2") {
                        return Err(format!(
                            "Cargo Fmt rollback did not restore exact bytes: restored={}; stderr={stderr:?}",
                            restored == original
                        ));
                    }
                } else if !source.is_dir() || !stderr.contains("rollback failed") {
                    return Err(format!(
                        "Cargo Fmt deterministic rollback failure was not reported: source_is_dir={}; stderr={stderr:?}",
                        source.is_dir()
                    ));
                }
                assert_cargo_fmt_private_roots_removed(&adapter_tmp, lifecycle_mode)?;
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&root);
        result
    }
}

#[cfg(unix)]
fn write_cargo_fmt_lifecycle_workspace(root: &Path, include_other: bool) -> Result<(), String> {
    let sources = root.join("src");
    std::fs::create_dir_all(&sources)
        .map_err(|error| format!("create Cargo Fmt lifecycle workspace {sources:?}: {error}"))?;
    std::fs::create_dir(root.join("empty-baseline"))
        .map_err(|error| format!("create Cargo Fmt lifecycle empty directory: {error}"))?;
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"cargo-fmt-lifecycle\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[lib]\npath = \"src/example.rs\"\n",
    )
    .map_err(|error| format!("write Cargo Fmt lifecycle Cargo.toml: {error}"))?;
    std::fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"cargo-fmt-lifecycle\"\nversion = \"0.0.0\"\n",
    )
    .map_err(|error| format!("write Cargo Fmt lifecycle Cargo.lock: {error}"))?;
    std::fs::write(root.join("rustfmt.toml"), "edition = \"2024\"\n")
        .map_err(|error| format!("write Cargo Fmt lifecycle rustfmt.toml: {error}"))?;
    std::fs::write(
        sources.join("example.rs"),
        "pub fn example(){println!(\"example\");}\n",
    )
    .map_err(|error| format!("write Cargo Fmt lifecycle example.rs: {error}"))?;
    if include_other {
        std::fs::write(
            sources.join("other.rs"),
            "pub fn other(){println!(\"other\");}\n",
        )
        .map_err(|error| format!("write Cargo Fmt lifecycle other.rs: {error}"))?;
    }
    Ok(())
}

#[cfg(unix)]
fn assert_cargo_fmt_private_roots_removed(root: &Path, label: &str) -> Result<(), String> {
    let retained = sorted_entries(root)?
        .into_iter()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("velvet-glove-cargo-fmt-"))
                .then(|| entry.path())
        })
        .collect::<Vec<_>>();
    if retained.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Cargo Fmt {label} lifecycle retained private roots: {retained:?}"
        ))
    }
}

fn verify_dclint_adapter_lifecycle(spec: &ToolSpec, timeout: Duration) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (spec, timeout);
        return Ok(());
    }
    #[cfg(unix)]
    {
        const MODE_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_MODE";
        const INVOKED_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_INVOKED";
        const SELECTED_CLEAN_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_SELECTED_CLEAN";
        const CHILD_PID_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_CHILD_PID";
        const DESCENDANT_PID_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_DESCENDANT_PID";
        const READY_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_READY";
        const SOURCE_CONFIG_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_SOURCE_CONFIG";
        const ORPHAN_PID_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_ORPHAN_PID";
        const ORPHAN_LATE_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_ORPHAN_LATE";
        const UNSELECTED_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_UNSELECTED";
        const DIRECTORY_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_DIRECTORY";
        const CONFIG_CAPTURE_ENV: &str = "VELVET_GLOVE_DCLINT_LIFECYCLE_CONFIG_CAPTURE";
        const CUTOFF_READY_ENV: &str = "VELVET_GLOVE_DCLINT_CUTOFF_READY";
        const CUTOFF_RELEASE_ENV: &str = "VELVET_GLOVE_DCLINT_CUTOFF_RELEASE";

        let phase = spec
            .phases
            .get("fix")
            .ok_or_else(|| "dclint lifecycle probe lacks a fix phase".to_owned())?;
        let [
            ArgvElement::Literal(isolated),
            ArgvElement::Literal(command),
            ArgvElement::Literal(adapter),
            ArgvElement::Token(ArgToken::ToolExecutable),
            ArgvElement::Literal(adapter_mode),
            ArgvElement::Token(ArgToken::ProjectRoot),
            ArgvElement::Token(ArgToken::ExtraArgs),
            ArgvElement::Literal(marker),
            ArgvElement::Token(ArgToken::Files),
        ] = phase.argv.as_slice()
        else {
            return Err(
                "dclint lifecycle probe could not extract the evaluated adapter".to_owned(),
            );
        };
        if isolated != "-I"
            || command != "-c"
            || adapter_mode != "fix"
            || marker != DCLINT_FILES_MARKER
        {
            return Err(format!(
                "dclint lifecycle expected exact isolated fix shape, got {isolated:?} {command:?} mode={adapter_mode:?} marker={marker:?}"
            ));
        }
        let python_program = phase
            .program
            .as_deref()
            .ok_or_else(|| "dclint lifecycle probe lacks an adapter program".to_owned())?;
        let python = resolve_program(python_program)
            .ok_or_else(|| format!("dclint lifecycle cannot resolve {python_program:?}"))?
            .canonicalize()
            .map_err(|error| format!("canonicalize dclint lifecycle Python: {error}"))?;

        let temporary = unique_temp_dir("velvet-glove-dclint-lifecycle");
        let root = temporary
            .canonicalize()
            .map_err(|error| format!("canonicalize dclint lifecycle root: {error}"))?;
        let result = (|| {
            let project = root.join("project");
            let private_tmp = root.join("tmp");
            let evidence = root.join("evidence");
            for directory in [&project, &private_tmp, &evidence] {
                std::fs::create_dir_all(directory).map_err(|error| {
                    format!("create dclint lifecycle directory {directory:?}: {error}")
                })?;
            }
            let fake_tool = root.join("dclint-fake");
            let fake_source = format!(
                "#!{}\n{}",
                python.display(),
                include_str!("support/dclint-lifecycle.py")
            );
            write_executable_fixture(&fake_tool, &fake_source, "dclint lifecycle fake")?;
            let invoked = root.join("invoked.jsonl");
            let dirty = project.join("a-dirty.yml");
            let selected_clean = project.join("b-selected-clean.yml");
            let nested_version = project.join("c-nested-version.yml");
            let source_config_probe = project.join("source-config-probe.json");
            let orphan_pid_path = root.join("normal-exit-orphan.pid");
            let orphan_late_path = root.join("normal-exit-orphan.late");
            let unselected = project.join("unselected.yml");
            let retained_directory = project.join("retained-directory");
            let config_capture = root.join("normalized-config.json");
            std::fs::write(&dirty, b"dirty\n")
                .map_err(|error| format!("write dclint lifecycle dirty target: {error}"))?;
            std::fs::write(&selected_clean, b"clean\n")
                .map_err(|error| format!("write dclint lifecycle clean target: {error}"))?;
            let nested_version_input = b"x-meta:\n  # dclint disable-line no-version-field\n  version: keep-me\nservices:\n  zebra: {}\n  alpha: {}\n";
            let nested_version_expected = b"x-meta:\n  # dclint disable-line no-version-field\n  version: keep-me\nservices:\n  alpha: {}\n  zebra: {}\n";
            std::fs::write(&nested_version, nested_version_input).map_err(|error| {
                format!("write dclint lifecycle nested-version target: {error}")
            })?;
            std::fs::write(&unselected, b"unselected sentinel\n")
                .map_err(|error| format!("write dclint lifecycle unselected target: {error}"))?;
            std::fs::create_dir(&retained_directory)
                .map_err(|error| format!("create dclint lifecycle retained directory: {error}"))?;
            let mut retained_permissions = std::fs::metadata(&retained_directory)
                .map_err(|error| format!("inspect dclint retained directory: {error}"))?
                .permissions();
            retained_permissions.set_mode(0o751);
            std::fs::set_permissions(&retained_directory, retained_permissions)
                .map_err(|error| format!("chmod dclint retained directory: {error}"))?;

            let run = |label: &str,
                       lifecycle_mode: &str,
                       extra_arguments: &[String],
                       targets: &[&Path]|
             -> Result<BoundedOutput, String> {
                std::fs::write(&invoked, b"")
                    .map_err(|error| format!("clear dclint lifecycle invocation log: {error}"))?;
                let capture = evidence.join(label);
                std::fs::create_dir_all(&capture).map_err(|error| {
                    format!("create dclint lifecycle capture {capture:?}: {error}")
                })?;
                let mut process = Command::new(&python);
                process
                    .args(["-I", "-c", adapter])
                    .arg(&fake_tool)
                    .arg("fix")
                    .arg(&project)
                    .args(extra_arguments)
                    .arg(DCLINT_FILES_MARKER)
                    .args(targets)
                    .current_dir(&project)
                    .env(TMPDIR_ENV, &private_tmp)
                    .env(MODE_ENV, lifecycle_mode)
                    .env(INVOKED_ENV, &invoked)
                    .env(SELECTED_CLEAN_ENV, &selected_clean)
                    .env(SOURCE_CONFIG_ENV, &source_config_probe)
                    .env(ORPHAN_PID_ENV, &orphan_pid_path)
                    .env(ORPHAN_LATE_ENV, &orphan_late_path)
                    .env(UNSELECTED_ENV, &unselected)
                    .env(DIRECTORY_ENV, &retained_directory)
                    .env(CONFIG_CAPTURE_ENV, &config_capture);
                run_with_timeout(
                    &mut process,
                    b"",
                    timeout.min(Duration::from_secs(8)),
                    &capture,
                )
                .map_err(|error| format!("run dclint lifecycle {label}: {error}"))
            };
            let assert_outcome = |label: &str,
                                  output: &BoundedOutput,
                                  status: i32,
                                  diagnostic: &str|
             -> Result<(), String> {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.code() != Some(status)
                    || (status == 2 && !stdout.is_empty())
                    || !stderr.contains(diagnostic)
                    || stderr.contains("Traceback")
                {
                    return Err(format!(
                        "dclint lifecycle {label} mismatch: status={:?}; stdout={stdout:?}; stderr={stderr:?}",
                        output.status.code()
                    ));
                }
                Ok(())
            };
            let read_invocations = || -> Result<Vec<Vec<String>>, String> {
                let contents = std::fs::read_to_string(&invoked)
                    .map_err(|error| format!("read dclint lifecycle invocations: {error}"))?;
                contents
                    .lines()
                    .map(|line| {
                        serde_json::from_str::<Vec<String>>(line).map_err(|error| {
                            format!("parse dclint lifecycle invocation {line:?}: {error}")
                        })
                    })
                    .collect()
            };
            let fingerprint = |path: &Path| -> Result<(Vec<u8>, u32, SystemTime), String> {
                let metadata = std::fs::metadata(path).map_err(|error| {
                    format!("inspect dclint lifecycle target {path:?}: {error}")
                })?;
                Ok((
                    std::fs::read(path).map_err(|error| {
                        format!("read dclint lifecycle target {path:?}: {error}")
                    })?,
                    metadata.permissions().mode(),
                    metadata.modified().map_err(|error| {
                        format!("read dclint lifecycle target mtime {path:?}: {error}")
                    })?,
                ))
            };

            for lifecycle_mode in [
                "unfixable",
                "invalid-yaml",
                "invalid-schema",
                "unknown-error",
            ] {
                let output = run(lifecycle_mode, lifecycle_mode, &[], &[&dirty])?;
                assert_outcome(lifecycle_mode, &output, 1, "")?;
                let calls = read_invocations()?;
                if calls.len() != 1 || calls[0].iter().any(|argument| argument == "--fix") {
                    return Err(format!(
                        "dclint lifecycle {lifecycle_mode} did not stop after one read-only preflight: {calls:?}"
                    ));
                }
            }
            for (lifecycle_mode, diagnostic) in [
                (
                    "ambiguous-fixability",
                    "fixability disagrees with pinned dclint 3.1.0 rule semantics",
                ),
                ("unknown-rule", "reports unknown rule 'future-fixable-rule'"),
                ("malformed-json", "is not strict UTF-8 JSON"),
                ("non-finite-json", "contains a non-finite number"),
                (
                    "native-stderr",
                    "emitted stderr instead of a controlled JSON report",
                ),
            ] {
                let output = run(lifecycle_mode, lifecycle_mode, &[], &[&dirty])?;
                assert_outcome(lifecycle_mode, &output, 2, diagnostic)?;
                if read_invocations()?.len() != 1 {
                    return Err(format!(
                        "dclint lifecycle {lifecycle_mode} did not fail closed after one child"
                    ));
                }
            }

            std::fs::write(&dirty, b"dirty\n")
                .map_err(|error| format!("reset dclint lifecycle dirty target: {error}"))?;
            std::fs::write(&selected_clean, b"clean\n")
                .map_err(|error| format!("reset dclint lifecycle clean target: {error}"))?;
            let clean_fingerprint = fingerprint(&selected_clean)?;
            let output = run("fixable", "fixable", &[], &[&dirty, &selected_clean])?;
            assert_outcome("fixable", &output, 0, "")?;
            if !output.stdout.is_empty()
                || std::fs::read(&dirty).ok().as_deref() != Some(b"fixed\n")
                || fingerprint(&selected_clean)? != clean_fingerprint
            {
                return Err("dclint lifecycle fixable run changed the wrong post-state".to_owned());
            }
            let calls = read_invocations()?;
            if calls.len() != 3
                || calls[0].iter().any(|argument| argument == "--fix")
                || !calls[1].iter().any(|argument| argument == "--fix")
                || !calls[1]
                    .iter()
                    .any(|argument| argument == dirty.to_string_lossy().as_ref())
                || calls[1]
                    .iter()
                    .any(|argument| argument == selected_clean.to_string_lossy().as_ref())
                || calls[2].iter().any(|argument| argument == "--fix")
            {
                return Err(format!(
                    "dclint lifecycle did not narrow its write child and restore full verification: {calls:?}"
                ));
            }
            let fixed_fingerprint = fingerprint(&dirty)?;
            let output = run(
                "fixable-idempotent",
                "fixable",
                &[],
                &[&dirty, &selected_clean],
            )?;
            assert_outcome("fixable-idempotent", &output, 0, "")?;
            if read_invocations()?.len() != 1
                || fingerprint(&dirty)? != fixed_fingerprint
                || fingerprint(&selected_clean)? != clean_fingerprint
            {
                return Err("dclint lifecycle fixed-state repeat was not an exact no-op".to_owned());
            }

            let output = run(
                "nested-version-preservation",
                "nested-version-preservation",
                &[],
                &[&nested_version],
            )?;
            assert_outcome("nested-version-preservation", &output, 0, "")?;
            if read_invocations()?.len() != 3
                || std::fs::read(&nested_version).ok().as_deref() != Some(nested_version_expected)
            {
                return Err(
                    "dclint did not preserve nested extension version data while applying an unrelated fix"
                        .to_owned(),
                );
            }
            let nested_version_fingerprint = fingerprint(&nested_version)?;
            let output = run(
                "nested-version-preservation-idempotent",
                "nested-version-preservation",
                &[],
                &[&nested_version],
            )?;
            assert_outcome("nested-version-preservation-idempotent", &output, 0, "")?;
            if read_invocations()?.len() != 1
                || fingerprint(&nested_version)? != nested_version_fingerprint
            {
                return Err(
                    "dclint nested-version preservation repeat was not an exact no-op".to_owned(),
                );
            }

            for (lifecycle_mode, diagnostic) in [
                ("noop-fix", "native fix was a no-op"),
                (
                    "touch-only",
                    "changed retained workspace topology or metadata outside byte edits",
                ),
                ("mutate-clean", "outside the proven fixable subset"),
                (
                    "partial-failure",
                    "emitted stderr instead of a controlled JSON report",
                ),
            ] {
                std::fs::write(&dirty, b"dirty\n").map_err(|error| {
                    format!("reset dclint {lifecycle_mode} dirty target: {error}")
                })?;
                std::fs::write(&selected_clean, b"clean\n").map_err(|error| {
                    format!("reset dclint {lifecycle_mode} clean target: {error}")
                })?;
                let dirty_before = fingerprint(&dirty)?;
                let clean_before = fingerprint(&selected_clean)?;
                let targets: &[&Path] = if lifecycle_mode == "mutate-clean" {
                    &[&dirty, &selected_clean]
                } else {
                    &[&dirty]
                };
                let output = run(lifecycle_mode, lifecycle_mode, &[], targets)?;
                assert_outcome(lifecycle_mode, &output, 2, diagnostic)?;
                if fingerprint(&dirty)? != dirty_before
                    || fingerprint(&selected_clean)? != clean_before
                {
                    return Err(format!(
                        "dclint lifecycle {lifecycle_mode} did not restore the exact selected-file baseline"
                    ));
                }
            }

            for (lifecycle_mode, diagnostic) in [
                ("unselected-change", "outside the proven fixable subset"),
                (
                    "unselected-create",
                    "changed retained workspace topology or metadata",
                ),
                (
                    "unselected-delete",
                    "changed retained workspace topology or metadata",
                ),
                (
                    "directory-add",
                    "changed retained workspace topology or metadata",
                ),
                (
                    "directory-remove",
                    "changed retained workspace topology or metadata",
                ),
                ("directory-chmod", "retained workspace"),
            ] {
                std::fs::write(&dirty, b"dirty\n").map_err(|error| {
                    format!("reset dclint {lifecycle_mode} selected target: {error}")
                })?;
                let dirty_before = fingerprint(&dirty)?;
                let unselected_before = fingerprint(&unselected)?;
                let retained_mode_before = std::fs::metadata(&retained_directory)
                    .map_err(|error| {
                        format!("inspect dclint {lifecycle_mode} retained directory: {error}")
                    })?
                    .permissions()
                    .mode();
                let output = run(lifecycle_mode, lifecycle_mode, &[], &[&dirty])?;
                assert_outcome(lifecycle_mode, &output, 2, diagnostic)?;
                let retained_mode_after = std::fs::metadata(&retained_directory)
                    .map_err(|error| {
                        format!(
                            "inspect restored dclint {lifecycle_mode} retained directory: {error}"
                        )
                    })?
                    .permissions()
                    .mode();
                if fingerprint(&dirty)? != dirty_before
                    || fingerprint(&unselected)? != unselected_before
                    || retained_mode_after != retained_mode_before
                    || unselected.with_extension("yml.created").exists()
                    || retained_directory.with_extension("created").exists()
                {
                    return Err(format!(
                        "dclint lifecycle {lifecycle_mode} did not restore retained file bytes/mode/mtime and directory topology/mode"
                    ));
                }
            }

            let regular = project.join("regular.yml");
            let symlink = project.join("symlink.yml");
            let hardlink = project.join("hardlink.yml");
            let outside = root.join("outside.yml");
            std::fs::write(&regular, b"clean\n")
                .map_err(|error| format!("write dclint lifecycle regular file: {error}"))?;
            std::fs::write(&outside, b"clean\n")
                .map_err(|error| format!("write dclint lifecycle outside file: {error}"))?;
            std::os::unix::fs::symlink(&selected_clean, &symlink)
                .map_err(|error| format!("create dclint lifecycle symlink: {error}"))?;
            std::fs::hard_link(&regular, &hardlink)
                .map_err(|error| format!("create dclint lifecycle hardlink: {error}"))?;
            for (label, target, diagnostic) in [
                ("symlink", symlink.as_path(), "traverses a symlink"),
                ("hardlink", hardlink.as_path(), "not a unique regular file"),
                ("outside", outside.as_path(), "escapes the project root"),
            ] {
                let output = run(label, "clean", &[], &[target])?;
                assert_outcome(label, &output, 2, diagnostic)?;
                if !read_invocations()?.is_empty() {
                    return Err(format!(
                        "dclint lifecycle {label} rejection invoked the native tool"
                    ));
                }
            }
            std::fs::remove_file(&symlink)
                .map_err(|error| format!("remove dclint lifecycle symlink: {error}"))?;
            std::fs::remove_file(&hardlink)
                .map_err(|error| format!("remove dclint lifecycle hardlink: {error}"))?;

            let configs = project.join("configs");
            std::fs::create_dir(&configs)
                .map_err(|error| format!("create dclint lifecycle configs: {error}"))?;
            for (label, contents, diagnostic) in [
                (
                    "extends",
                    r#"{"extends":"attacker.json"}"#,
                    "executable-loading keys",
                ),
                (
                    "plugin",
                    r#"{"plugin":"attacker"}"#,
                    "executable-loading keys",
                ),
                (
                    "exclude",
                    r#"{"exclude":["**/*"]}"#,
                    "exclude must be empty",
                ),
                ("debug", r#"{"debug":true}"#, "debug must be false"),
                (
                    "duplicate",
                    r#"{"rules":{},"rules":{}}"#,
                    "duplicate JSON key",
                ),
                (
                    "incomplete-top-level-order",
                    r#"{"rules":{"top-level-properties-order":[2,{"customOrder":["services"]}]}}"#,
                    "customOrder must be an exact permutation",
                ),
                (
                    "unsafe-no-version-numeric",
                    r#"{"rules":{"no-version-field":1}}"#,
                    "3.1.0 fixer can delete nested extension data",
                ),
                (
                    "unsafe-no-version-array",
                    r#"{"rules":{"no-version-field":[2,{}]}}"#,
                    "3.1.0 fixer can delete nested extension data",
                ),
                (
                    "duplicate-service-key-groups",
                    r#"{"rules":{"service-keys-order":[2,{"groups":{"Core Definitions":["image"],"Other":["image"]}}]}}"#,
                    "service key 'image' is assigned to multiple groups",
                ),
                (
                    "duplicate-effective-service-key-groups",
                    r#"{"rules":{"service-keys-order":[2,{"groups":{"Other":["image"]}}]}}"#,
                    "service key 'image' is assigned to multiple groups",
                ),
            ] {
                let config = configs.join(format!("{label}.json"));
                std::fs::write(&config, format!("{contents}\n"))
                    .map_err(|error| format!("write dclint lifecycle {label} config: {error}"))?;
                let extra = vec![format!("--config=configs/{label}.json")];
                let output = run(
                    &format!("config-{label}"),
                    "clean",
                    &extra,
                    &[&selected_clean],
                )?;
                assert_outcome(&format!("config-{label}"), &output, 2, diagnostic)?;
                if !read_invocations()?.is_empty() {
                    return Err(format!(
                        "dclint lifecycle unsafe {label} config reached the native tool"
                    ));
                }
            }
            let safe_top_level_order = serde_json::json!([
                "x-properties",
                "version",
                "name",
                "include",
                "services",
                "models",
                "networks",
                "volumes",
                "secrets",
                "configs"
            ]);
            let custom_top_level_order = serde_json::json!([
                "models",
                "services",
                "name",
                "x-properties",
                "include",
                "version",
                "configs",
                "secrets",
                "volumes",
                "networks"
            ]);
            let custom_order_config = configs.join("complete-top-level-order.json");
            std::fs::write(
                &custom_order_config,
                serde_json::to_vec(&serde_json::json!({
                    "rules": {
                        "top-level-properties-order": [
                            2,
                            {"customOrder": custom_top_level_order.clone()}
                        ]
                    }
                }))
                .map_err(|error| format!("serialize dclint complete custom order: {error}"))?,
            )
            .map_err(|error| format!("write dclint complete custom order: {error}"))?;
            let custom_order_before = fingerprint(&custom_order_config)?;
            let custom_order_extra =
                vec!["--config=configs/complete-top-level-order.json".to_owned()];
            let output = run(
                "config-complete-top-level-order",
                "capture-config",
                &custom_order_extra,
                &[&selected_clean],
            )?;
            assert_outcome("config-complete-top-level-order", &output, 0, "")?;
            let captured: JsonValue = serde_json::from_slice(
                &std::fs::read(&config_capture)
                    .map_err(|error| format!("read normalized custom-order config: {error}"))?,
            )
            .map_err(|error| format!("parse normalized custom-order config: {error}"))?;
            if read_invocations()?.len() != 1
                || captured["rules"]["no-version-field"] != 0
                || captured["rules"]["top-level-properties-order"][0] != 2
                || captured["rules"]["top-level-properties-order"][1]["customOrder"]
                    != custom_top_level_order
                || fingerprint(&custom_order_config)? != custom_order_before
            {
                return Err(format!(
                    "dclint did not preserve the severity and exact complete user top-level order: {captured}"
                ));
            }

            let numeric_order_config = configs.join("numeric-top-level-order.json");
            std::fs::write(
                &numeric_order_config,
                br#"{"rules":{"top-level-properties-order":2}}"#,
            )
            .map_err(|error| format!("write dclint numeric top-level order: {error}"))?;
            let numeric_order_extra =
                vec!["--config=configs/numeric-top-level-order.json".to_owned()];
            let output = run(
                "config-numeric-top-level-order",
                "capture-config",
                &numeric_order_extra,
                &[&selected_clean],
            )?;
            assert_outcome("config-numeric-top-level-order", &output, 0, "")?;
            let captured: JsonValue = serde_json::from_slice(
                &std::fs::read(&config_capture)
                    .map_err(|error| format!("read normalized numeric-order config: {error}"))?,
            )
            .map_err(|error| format!("parse normalized numeric-order config: {error}"))?;
            if read_invocations()?.len() != 1
                || captured["rules"]["no-version-field"] != 0
                || captured["rules"]["top-level-properties-order"][0] != 2
                || captured["rules"]["top-level-properties-order"][1]["customOrder"]
                    != safe_top_level_order
            {
                return Err(format!(
                    "dclint did not replace its destructive numeric-enabled default order: {captured}"
                ));
            }

            let output = run(
                "config-default-top-level-order",
                "capture-config",
                &[],
                &[&selected_clean],
            )?;
            assert_outcome("config-default-top-level-order", &output, 0, "")?;
            let captured: JsonValue = serde_json::from_slice(
                &std::fs::read(&config_capture)
                    .map_err(|error| format!("read normalized default-order config: {error}"))?,
            )
            .map_err(|error| format!("parse normalized default-order config: {error}"))?;
            if read_invocations()?.len() != 1
                || captured["rules"]["no-version-field"] != 0
                || captured["rules"]["top-level-properties-order"][0] != 1
                || captured["rules"]["top-level-properties-order"][1]["customOrder"]
                    != safe_top_level_order
            {
                return Err(format!(
                    "dclint did not replace its destructive native default top-level order: {captured}"
                ));
            }

            let config_regular = configs.join("regular.json");
            let config_hardlink = configs.join("hardlink.json");
            let config_symlink = configs.join("symlink.json");
            std::fs::write(&config_regular, b"{}\n")
                .map_err(|error| format!("write dclint lifecycle regular config: {error}"))?;
            std::fs::hard_link(&config_regular, &config_hardlink)
                .map_err(|error| format!("hardlink dclint lifecycle config: {error}"))?;
            std::os::unix::fs::symlink(&selected_clean, &config_symlink)
                .map_err(|error| format!("symlink dclint lifecycle config: {error}"))?;
            for (label, diagnostic) in [
                ("hardlink", "not a unique regular file"),
                ("symlink", "traverses a symlink"),
            ] {
                let extra = vec![format!("--config=configs/{label}.json")];
                let output = run(
                    &format!("config-path-{label}"),
                    "clean",
                    &extra,
                    &[&selected_clean],
                )?;
                assert_outcome(&format!("config-path-{label}"), &output, 2, diagnostic)?;
                if !read_invocations()?.is_empty() {
                    return Err(format!(
                        "dclint lifecycle {label} config path reached the native tool"
                    ));
                }
            }
            std::fs::remove_file(&config_symlink)
                .map_err(|error| format!("remove dclint lifecycle config symlink: {error}"))?;
            std::fs::remove_file(&config_hardlink)
                .map_err(|error| format!("remove dclint lifecycle config hardlink: {error}"))?;

            std::fs::write(&source_config_probe, b"{}\n")
                .map_err(|error| format!("write dclint config-swap source: {error}"))?;
            let config_swap_target_before = fingerprint(&selected_clean)?;
            let config_swap_extra = vec!["--config=source-config-probe.json".to_owned()];
            let output = run(
                "config-swap",
                "config-swap",
                &config_swap_extra,
                &[&selected_clean],
            )?;
            assert_outcome("config-swap", &output, 2, "source config changed")?;
            let calls = read_invocations()?;
            let source_argument = format!("--config={}", source_config_probe.display());
            if calls.len() != 1
                || calls[0].iter().any(|argument| argument == &source_argument)
                || !calls[0].iter().any(|argument| {
                    argument.starts_with("--config=") && argument.contains("velvet-glove-dclint-")
                })
                || fingerprint(&selected_clean)? != config_swap_target_before
            {
                return Err(format!(
                    "dclint lifecycle config swap influenced the child or selected target: {calls:?}"
                ));
            }
            std::fs::remove_file(&source_config_probe)
                .map_err(|error| format!("remove dclint config-swap source: {error}"))?;

            let output = run(
                "private-config-destroy",
                "private-config-destroy",
                &[],
                &[&selected_clean],
            )?;
            assert_outcome(
                "private-config-destroy",
                &output,
                2,
                "controlled private config",
            )?;
            let private_destroy_stderr = String::from_utf8_lossy(&output.stderr);
            if !private_destroy_stderr.contains("cannot remove private dclint config directory")
                || !private_destroy_stderr.contains("<dclint-private>")
                || private_destroy_stderr.contains("velvet-glove-dclint-")
            {
                return Err(format!(
                    "dclint lifecycle did not compose and normalize private cleanup failure: {private_destroy_stderr:?}"
                ));
            }
            for entry in sorted_entries(&private_tmp)? {
                let path = entry.path();
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("velvet-glove-dclint-"))
                {
                    std::fs::remove_file(&path).map_err(|error| {
                        format!("remove dclint destroyed private-config sentinel {path:?}: {error}")
                    })?;
                }
            }

            std::fs::write(&dirty, b"dirty\n")
                .map_err(|error| format!("reset dclint rollback-failure target: {error}"))?;
            let rollback_anchor = "def restore_workspace(project_root, before, before_directories):\n    errors = []\n";
            if adapter.matches(rollback_anchor).count() != 1 {
                return Err(
                    "dclint rollback-failure probe requires one exact restore anchor".to_owned(),
                );
            }
            let rollback_adapter = adapter.replacen(
                rollback_anchor,
                "def restore_workspace(project_root, before, before_directories):\n    return \"injected deterministic rollback failure\"\n    errors = []\n",
                1,
            );
            std::fs::write(&invoked, b"")
                .map_err(|error| format!("clear dclint rollback-failure log: {error}"))?;
            let mut rollback_command = Command::new(&python);
            rollback_command
                .args(["-I", "-c", &rollback_adapter])
                .arg(&fake_tool)
                .arg("fix")
                .arg(&project)
                .arg(DCLINT_FILES_MARKER)
                .arg(&dirty)
                .current_dir(&project)
                .env(TMPDIR_ENV, &private_tmp)
                .env(MODE_ENV, "partial-failure")
                .env(INVOKED_ENV, &invoked);
            let output = run_with_timeout(
                &mut rollback_command,
                b"",
                timeout.min(Duration::from_secs(5)),
                &evidence.join("rollback-failure"),
            )
            .map_err(|error| format!("run dclint rollback-failure probe: {error}"))?;
            assert_outcome(
                "rollback-failure",
                &output,
                2,
                "emitted stderr instead of a controlled JSON report",
            )?;
            let rollback_failure_stderr = String::from_utf8_lossy(&output.stderr);
            if !rollback_failure_stderr
                .contains("rollback failed: injected deterministic rollback failure")
                || std::fs::read(&dirty).ok().as_deref() != Some(b"partial\n")
            {
                return Err(format!(
                    "dclint lifecycle did not preserve primary plus rollback failure: {rollback_failure_stderr:?}"
                ));
            }
            std::fs::write(&dirty, b"dirty\n")
                .map_err(|error| format!("repair dclint rollback-failure target: {error}"))?;

            for (label, control) in [("control-tab", '\t'), ("control-del", '\u{7f}')] {
                let target = project.join(format!("control-{control}.yml"));
                std::fs::write(&target, b"clean\n")
                    .map_err(|error| format!("write dclint control-path target: {error}"))?;
                let output = run(label, "clean", &[], &[&target])?;
                assert_outcome(label, &output, 2, "contains a control character")?;
                if !read_invocations()?.is_empty() {
                    return Err("dclint control-path rejection invoked the native tool".to_owned());
                }
                std::fs::remove_file(&target)
                    .map_err(|error| format!("remove dclint control-path target: {error}"))?;
            }

            let run_with_tmpdir =
                |label: &str, temporary_value: &OsStr| -> Result<BoundedOutput, String> {
                    std::fs::write(&invoked, b"").map_err(|error| {
                        format!("clear dclint {label} TMPDIR invocation log: {error}")
                    })?;
                    let mut command = Command::new(&python);
                    command
                        .args(["-I", "-c", adapter])
                        .arg(&fake_tool)
                        .arg("verify")
                        .arg(&project)
                        .arg(DCLINT_FILES_MARKER)
                        .arg(&selected_clean)
                        .current_dir(&project)
                        .env(TMPDIR_ENV, temporary_value)
                        .env(MODE_ENV, "clean")
                        .env(INVOKED_ENV, &invoked);
                    run_with_timeout(
                        &mut command,
                        b"",
                        timeout.min(Duration::from_secs(5)),
                        &evidence.join(label),
                    )
                    .map_err(|error| format!("run dclint {label} TMPDIR probe: {error}"))
                };
            let tmp_alias = root.join("tmp-alias");
            std::os::unix::fs::symlink(&private_tmp, &tmp_alias)
                .map_err(|error| format!("create dclint TMPDIR alias: {error}"))?;
            let tmp_alias_spelling = format!("{}/", tmp_alias.display());
            let alias_output = run_with_tmpdir("tmpdir-alias", tmp_alias_spelling.as_ref())?;
            assert_outcome("tmpdir-alias", &alias_output, 0, "")?;
            let alias_calls = read_invocations()?;
            let alias_config = alias_calls
                .first()
                .and_then(|call| {
                    call.iter()
                        .find_map(|argument| argument.strip_prefix("--config="))
                })
                .map(PathBuf::from);
            if alias_calls.len() != 1
                || alias_config
                    .as_deref()
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    != Some(private_tmp.as_path())
            {
                return Err(format!(
                    "dclint did not canonicalize its existing symlink/trailing-slash TMPDIR before child launch: {alias_calls:?}"
                ));
            }
            std::fs::remove_file(&tmp_alias)
                .map_err(|error| format!("remove dclint TMPDIR alias: {error}"))?;

            let missing_tmp = root.join("missing-tmp");
            for (label, temporary_value, expected_stderr) in [
                (
                    "tmpdir-relative",
                    OsStr::new("relative-tmp"),
                    "velvet-glove-dclint: TMPDIR must name an existing absolute directory\n",
                ),
                (
                    "tmpdir-missing",
                    missing_tmp.as_os_str(),
                    "velvet-glove-dclint: TMPDIR must resolve to an existing accessible directory\n",
                ),
            ] {
                let output = run_with_tmpdir(label, temporary_value)?;
                if output.status.code() != Some(2)
                    || !output.stdout.is_empty()
                    || output.stderr != expected_stderr.as_bytes()
                    || !read_invocations()?.is_empty()
                {
                    return Err(format!(
                        "dclint {label} TMPDIR rejection was not deterministic and pre-spawn: status={:?}; stdout={:?}; stderr={:?}",
                        output.status.code(),
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
            }

            let output_cap_before = fingerprint(&selected_clean)?;
            let output = run("output-cap", "output-cap", &[], &[&selected_clean])?;
            assert_outcome(
                "output-cap",
                &output,
                2,
                "combined dclint output exceeded 33554432 bytes",
            )?;
            if fingerprint(&selected_clean)? != output_cap_before {
                return Err("dclint output-cap failure changed its selected file".to_owned());
            }

            let unwritable_tmp = root.join("unwritable-tmp");
            std::fs::create_dir(&unwritable_tmp)
                .map_err(|error| format!("create dclint unwritable TMPDIR: {error}"))?;
            let mut unwritable_permissions = std::fs::metadata(&unwritable_tmp)
                .map_err(|error| format!("inspect dclint unwritable TMPDIR: {error}"))?
                .permissions();
            unwritable_permissions.set_mode(0o500);
            std::fs::set_permissions(&unwritable_tmp, unwritable_permissions)
                .map_err(|error| format!("chmod dclint unwritable TMPDIR: {error}"))?;
            let unwritable_invoked = root.join("unwritable-tmp.invoked");
            std::fs::write(&unwritable_invoked, b"")
                .map_err(|error| format!("create dclint unwritable child marker: {error}"))?;
            let mut unwritable_command = Command::new(&python);
            unwritable_command
                .args(["-I", "-c", adapter])
                .arg(&fake_tool)
                .arg("verify")
                .arg(&project)
                .arg(DCLINT_FILES_MARKER)
                .arg(&selected_clean)
                .current_dir(&project)
                .env(TMPDIR_ENV, &unwritable_tmp)
                .env(MODE_ENV, "clean")
                .env(INVOKED_ENV, &unwritable_invoked);
            let unwritable_result = run_with_timeout(
                &mut unwritable_command,
                b"",
                timeout.min(Duration::from_secs(5)),
                &evidence.join("unwritable-tmp"),
            );
            let mut writable_permissions = std::fs::metadata(&unwritable_tmp)
                .map_err(|error| format!("reinspect dclint unwritable TMPDIR: {error}"))?
                .permissions();
            writable_permissions.set_mode(0o700);
            std::fs::set_permissions(&unwritable_tmp, writable_permissions)
                .map_err(|error| format!("restore dclint unwritable TMPDIR mode: {error}"))?;
            let unwritable_output = unwritable_result
                .map_err(|error| format!("run dclint unwritable TMPDIR probe: {error}"))?;
            let unwritable_stdout = String::from_utf8_lossy(&unwritable_output.stdout);
            let unwritable_stderr = String::from_utf8_lossy(&unwritable_output.stderr);
            if unwritable_output.status.code() != Some(2)
                || !unwritable_stdout.is_empty()
                || !unwritable_stderr.contains("<dclint-private>")
                || unwritable_stderr.contains("velvet-glove-dclint-")
                || !std::fs::read(&unwritable_invoked).is_ok_and(|contents| contents.is_empty())
            {
                return Err(format!(
                    "dclint unwritable TMPDIR was not normalized before child launch: status={:?}; stdout={unwritable_stdout:?}; stderr={unwritable_stderr:?}",
                    unwritable_output.status.code()
                ));
            }

            for (label, lifecycle_mode) in [
                ("normal-exit-orphan-closed", "normal-exit-orphan-closed"),
                ("normal-exit-orphan-pipe", "normal-exit-orphan-pipe"),
            ] {
                let _ = std::fs::remove_file(&orphan_pid_path);
                let _ = std::fs::remove_file(&orphan_late_path);
                let target_before = fingerprint(&selected_clean)?;
                let output = run(label, lifecycle_mode, &[], &[&selected_clean])?;
                assert_outcome(
                    label,
                    &output,
                    2,
                    "native dclint left same-group descendants after child exit",
                )?;
                let orphan_pid = read_pid_file(&orphan_pid_path, label)?;
                let orphan_alive = process_survives(orphan_pid, Duration::from_secs(1))?;
                std::thread::sleep(Duration::from_millis(850));
                if orphan_alive
                    || orphan_late_path.exists()
                    || fingerprint(&selected_clean)? != target_before
                {
                    let _ = signal_process(orphan_pid, "KILL");
                    return Err(format!(
                        "dclint lifecycle {label} leaked a same-group descendant or late mutation: pid={orphan_pid}; alive={orphan_alive}; late={} ",
                        orphan_late_path.exists()
                    ));
                }
            }

            let guarded_anchor =
                "previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, HANDLED_SIGNALS)\n";
            if adapter.matches(guarded_anchor).count() != 1 {
                return Err(
                    "dclint guarded-spawn probe requires one exact SIG_BLOCK anchor".to_owned(),
                );
            }
            let guarded_offset = adapter
                .find(guarded_anchor)
                .expect("checked dclint guarded-spawn anchor");
            let guarded_line = adapter[..guarded_offset]
                .rfind('\n')
                .map_or(0, |offset| offset + 1);
            let guarded_indent = &adapter[guarded_line..guarded_offset];
            if !guarded_indent.chars().all(|character| character == ' ') {
                return Err(format!(
                    "dclint guarded-spawn anchor has unexpected indentation {guarded_indent:?}"
                ));
            }
            let guarded_ready = root.join("guarded-spawn.ready");
            let guarded_release = root.join("guarded-spawn.release");
            let guarded_hook = format!(
                "{guarded_anchor}{guarded_indent}ready_descriptor = os.open(os.environ[{CUTOFF_READY_ENV:?}], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)\n{guarded_indent}os.close(ready_descriptor)\n{guarded_indent}release_descriptor = os.open(os.environ[{CUTOFF_RELEASE_ENV:?}], os.O_RDONLY)\n{guarded_indent}os.close(release_descriptor)\n"
            );
            let guarded_adapter = adapter.replacen(guarded_anchor, &guarded_hook, 1);
            let mkfifo = Command::new("/usr/bin/mkfifo")
                .arg(&guarded_release)
                .status()
                .map_err(|error| format!("create dclint guarded-spawn FIFO: {error}"))?;
            if !mkfifo.success() {
                return Err(format!(
                    "create dclint guarded-spawn FIFO exited {mkfifo:?}"
                ));
            }
            std::fs::write(&invoked, b"")
                .map_err(|error| format!("clear dclint guarded-spawn marker: {error}"))?;
            let guarded_before = fingerprint(&selected_clean)?;
            let mut guarded_command = Command::new(&python);
            guarded_command
                .args(["-I", "-c", &guarded_adapter])
                .arg(&fake_tool)
                .arg("verify")
                .arg(&project)
                .arg(DCLINT_FILES_MARKER)
                .arg(&selected_clean)
                .current_dir(&project)
                .env(TMPDIR_ENV, &private_tmp)
                .env(MODE_ENV, "clean")
                .env(INVOKED_ENV, &invoked)
                .env(CUTOFF_READY_ENV, &guarded_ready)
                .env(CUTOFF_RELEASE_ENV, &guarded_release)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut guarded_outer = guarded_command
                .spawn()
                .map_err(|error| format!("spawn dclint guarded-spawn adapter: {error}"))?;
            let guarded_pid = guarded_outer.id();
            let guarded_deadline = std::time::Instant::now() + timeout.min(Duration::from_secs(5));
            while !guarded_ready.is_file() {
                if let Some(status) = guarded_outer
                    .try_wait()
                    .map_err(|error| format!("poll dclint guarded-spawn adapter: {error}"))?
                {
                    return Err(format!(
                        "dclint guarded-spawn adapter exited {status:?} before its hook"
                    ));
                }
                if std::time::Instant::now() >= guarded_deadline {
                    let _ = signal_process(guarded_pid, "KILL");
                    let _ = guarded_outer.wait();
                    return Err("dclint guarded-spawn adapter did not reach its hook".to_owned());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if !signal_process(guarded_pid, "TERM")?.success() {
                let _ = signal_process(guarded_pid, "KILL");
                let _ = guarded_outer.wait();
                return Err("send guarded-spawn SIGTERM to dclint adapter".to_owned());
            }
            std::fs::OpenOptions::new()
                .write(true)
                .open(&guarded_release)
                .map_err(|error| format!("release dclint guarded-spawn hook: {error}"))?;
            let guarded_output = guarded_outer
                .wait_with_output()
                .map_err(|error| format!("collect dclint guarded-spawn output: {error}"))?;
            if guarded_output.status.code() != Some(2)
                || !guarded_output.stdout.is_empty()
                || guarded_output.stderr != b"velvet-glove-dclint: received signal 15\n"
                || !read_invocations()?.is_empty()
                || fingerprint(&selected_clean)? != guarded_before
            {
                return Err(format!(
                    "dclint guarded-spawn signal launched a child or changed state: status={:?}; stdout={:?}; stderr={:?}",
                    guarded_output.status.code(),
                    String::from_utf8_lossy(&guarded_output.stdout),
                    String::from_utf8_lossy(&guarded_output.stderr)
                ));
            }

            let cutoff_anchor =
                "blocked_mask = signal.pthread_sigmask(signal.SIG_BLOCK, HANDLED_SIGNALS)\n";
            if adapter.matches(cutoff_anchor).count() != 1 {
                return Err(
                    "dclint cleanup-cutoff probe requires one exact SIG_BLOCK anchor".to_owned(),
                );
            }
            let cutoff_offset = adapter
                .find(cutoff_anchor)
                .expect("checked dclint cleanup-cutoff anchor");
            let cutoff_line = adapter[..cutoff_offset]
                .rfind('\n')
                .map_or(0, |offset| offset + 1);
            let cutoff_indent = &adapter[cutoff_line..cutoff_offset];
            if !cutoff_indent.chars().all(|character| character == ' ') {
                return Err(format!(
                    "dclint cleanup-cutoff anchor has unexpected indentation {cutoff_indent:?}"
                ));
            }
            let cutoff_ready = root.join("cleanup-cutoff.ready");
            let cutoff_release = root.join("cleanup-cutoff.release");
            let cutoff_hook = format!(
                "{cutoff_anchor}{cutoff_indent}ready_descriptor = os.open(os.environ[{CUTOFF_READY_ENV:?}], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)\n{cutoff_indent}os.close(ready_descriptor)\n{cutoff_indent}release_descriptor = os.open(os.environ[{CUTOFF_RELEASE_ENV:?}], os.O_RDONLY)\n{cutoff_indent}os.close(release_descriptor)\n"
            );
            let cutoff_adapter = adapter.replacen(cutoff_anchor, &cutoff_hook, 1);
            let mkfifo = Command::new("/usr/bin/mkfifo")
                .arg(&cutoff_release)
                .status()
                .map_err(|error| format!("create dclint cleanup-cutoff FIFO: {error}"))?;
            if !mkfifo.success() {
                return Err(format!(
                    "create dclint cleanup-cutoff FIFO exited {mkfifo:?}"
                ));
            }
            std::fs::write(&dirty, b"dirty\n")
                .map_err(|error| format!("reset dclint cleanup-cutoff target: {error}"))?;
            std::fs::write(&invoked, b"")
                .map_err(|error| format!("clear dclint cleanup-cutoff marker: {error}"))?;
            let cutoff_before = fingerprint(&dirty)?;
            let mut cutoff_command = Command::new(&python);
            cutoff_command
                .args(["-I", "-c", &cutoff_adapter])
                .arg(&fake_tool)
                .arg("fix")
                .arg(&project)
                .arg(DCLINT_FILES_MARKER)
                .arg(&dirty)
                .current_dir(&project)
                .env(TMPDIR_ENV, &private_tmp)
                .env(MODE_ENV, "fixable")
                .env(INVOKED_ENV, &invoked)
                .env(CUTOFF_READY_ENV, &cutoff_ready)
                .env(CUTOFF_RELEASE_ENV, &cutoff_release)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut cutoff_outer = cutoff_command
                .spawn()
                .map_err(|error| format!("spawn dclint cleanup-cutoff adapter: {error}"))?;
            let cutoff_pid = cutoff_outer.id();
            let cutoff_deadline = std::time::Instant::now() + timeout.min(Duration::from_secs(8));
            while !cutoff_ready.is_file() {
                if let Some(status) = cutoff_outer
                    .try_wait()
                    .map_err(|error| format!("poll dclint cleanup-cutoff adapter: {error}"))?
                {
                    return Err(format!(
                        "dclint cleanup-cutoff adapter exited {status:?} before its hook"
                    ));
                }
                if std::time::Instant::now() >= cutoff_deadline {
                    let _ = signal_process(cutoff_pid, "KILL");
                    let _ = cutoff_outer.wait();
                    return Err("dclint cleanup-cutoff adapter did not reach its hook".to_owned());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if std::fs::read(&dirty).ok().as_deref() != Some(b"fixed\n") {
                let _ = signal_process(cutoff_pid, "KILL");
                let _ = cutoff_outer.wait();
                return Err("dclint cleanup-cutoff hook ran before the real mutation".to_owned());
            }
            if !signal_process(cutoff_pid, "TERM")?.success() {
                let _ = signal_process(cutoff_pid, "KILL");
                let _ = cutoff_outer.wait();
                return Err("send cleanup-cutoff SIGTERM to dclint adapter".to_owned());
            }
            std::fs::OpenOptions::new()
                .write(true)
                .open(&cutoff_release)
                .map_err(|error| format!("release dclint cleanup-cutoff hook: {error}"))?;
            let cutoff_output = cutoff_outer
                .wait_with_output()
                .map_err(|error| format!("collect dclint cleanup-cutoff output: {error}"))?;
            if cutoff_output.status.code() != Some(2)
                || !cutoff_output.stdout.is_empty()
                || cutoff_output.stderr != b"velvet-glove-dclint: received signal 15\n"
                || fingerprint(&dirty)? != cutoff_before
            {
                return Err(format!(
                    "dclint cleanup-cutoff signal did not cause exact rollback: status={:?}; stdout={:?}; stderr={:?}",
                    cutoff_output.status.code(),
                    String::from_utf8_lossy(&cutoff_output.stdout),
                    String::from_utf8_lossy(&cutoff_output.stderr)
                ));
            }

            let child_pid_path = root.join("cancel-child.pid");
            let descendant_pid_path = root.join("cancel-descendant.pid");
            let ready_path = root.join("cancel-ready");
            std::fs::write(&invoked, b"")
                .map_err(|error| format!("clear dclint cancel invocation log: {error}"))?;
            let mut cancellation = Command::new(&python);
            cancellation
                .args(["-I", "-c", adapter])
                .arg(&fake_tool)
                .arg("verify")
                .arg(&project)
                .arg(DCLINT_FILES_MARKER)
                .arg(&selected_clean)
                .current_dir(&project)
                .env(TMPDIR_ENV, &private_tmp)
                .env(MODE_ENV, "cancel")
                .env(INVOKED_ENV, &invoked)
                .env(CHILD_PID_ENV, &child_pid_path)
                .env(DESCENDANT_PID_ENV, &descendant_pid_path)
                .env(READY_ENV, &ready_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut outer = cancellation
                .spawn()
                .map_err(|error| format!("spawn dclint cancellation lifecycle: {error}"))?;
            let outer_pid = outer.id();
            let startup_timeout = timeout.min(Duration::from_secs(5));
            let startup_deadline = std::time::Instant::now() + startup_timeout;
            while !ready_path.is_file() {
                if let Some(status) = outer
                    .try_wait()
                    .map_err(|error| format!("poll dclint cancellation lifecycle: {error}"))?
                {
                    return Err(format!(
                        "dclint cancellation adapter exited {status:?} before its child was ready"
                    ));
                }
                if std::time::Instant::now() >= startup_deadline {
                    let _ = signal_process(outer_pid, "KILL");
                    let _ = outer.wait();
                    return Err(format!(
                        "dclint cancellation child was not ready within {startup_timeout:?}"
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let child_pid = read_pid_file(&child_pid_path, "dclint cancellation child")?;
            let descendant_pid =
                read_pid_file(&descendant_pid_path, "dclint cancellation descendant")?;
            if !signal_process(outer_pid, "TERM")?.success() {
                let _ = signal_process_group(child_pid, "KILL");
                let _ = signal_process(outer_pid, "KILL");
                let _ = outer.wait();
                return Err("send SIGTERM to dclint cancellation adapter".to_owned());
            }
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = sender.send(outer.wait_with_output());
            });
            let completion_timeout = timeout.min(Duration::from_secs(5));
            let cancelled = receiver.recv_timeout(completion_timeout).map_err(|error| {
                let _ = signal_process_group(child_pid, "KILL");
                format!(
                    "dclint cancellation adapter retained output pipes for {completion_timeout:?}: {error}"
                )
            })?;
            let cancelled = cancelled
                .map_err(|error| format!("wait for dclint cancellation adapter: {error}"))?;
            let child_alive = process_survives(child_pid, Duration::from_secs(1))?;
            let descendant_alive = process_survives(descendant_pid, Duration::from_secs(1))?;
            let group_alive = process_group_survives(child_pid, Duration::from_secs(1))?;
            if child_alive || descendant_alive || group_alive {
                let _ = signal_process_group(child_pid, "KILL");
                return Err(format!(
                    "dclint cancellation left child={child_alive}, descendant={descendant_alive}, group={group_alive} alive"
                ));
            }
            if cancelled.status.code() != Some(2)
                || !cancelled.stdout.is_empty()
                || cancelled.stderr != b"velvet-glove-dclint: received signal 15\n"
            {
                return Err(format!(
                    "dclint cancellation output was unstable: status={:?}; stdout={:?}; stderr={:?}",
                    cancelled.status.code(),
                    String::from_utf8_lossy(&cancelled.stdout),
                    String::from_utf8_lossy(&cancelled.stderr)
                ));
            }

            let leftovers = sorted_entries(&private_tmp)?
                .into_iter()
                .map(|entry| entry.path())
                .collect::<Vec<_>>();
            if !leftovers.is_empty() {
                return Err(format!(
                    "dclint lifecycle left private configuration artifacts: {leftovers:?}"
                ));
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&root);
        result
    }
}

#[cfg(unix)]
fn vacuum_lifecycle_command(
    python: &Path,
    adapter: &str,
    tool: &Path,
    extra_args: &[&str],
    files: &[PathBuf],
    current_dir: &Path,
) -> Command {
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", adapter])
        .arg(tool)
        .args(extra_args)
        .arg(VACUUM_FILES_MARKER)
        .args(files)
        .current_dir(current_dir);
    command
}

#[cfg(unix)]
fn assert_vacuum_private_root_removed(cwd_record: &Path, label: &str) -> Result<(), String> {
    let cwd = std::fs::read_to_string(cwd_record)
        .map_err(|error| format!("read Vacuum {label} private cwd record: {error}"))?;
    let cwd = cwd.trim();
    if !cwd.contains("/velvet-glove-vacuum-") {
        return Err(format!(
            "Vacuum {label} child did not run in an owned private root: {cwd:?}"
        ));
    }
    if Path::new(cwd).exists() {
        return Err(format!(
            "Vacuum {label} retained private state after exit: {cwd:?}"
        ));
    }
    Ok(())
}

fn verify_vacuum_adapter_lifecycle(spec: &ToolSpec, timeout: Duration) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (spec, timeout);
        return Ok(());
    }
    #[cfg(unix)]
    {
        const INVOKED_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_INVOKED";
        const ARGV_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_ARGV";
        const CWD_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_CWD";
        const COPY_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_COPY";
        const CONFIG_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_CONFIG";
        const MODES_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_MODES";
        const ENVIRONMENT_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_ENVIRONMENT";
        const ORIGINAL_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_ORIGINAL";
        const REPLACEMENT_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_REPLACEMENT";
        const CHILD_PID_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_CHILD_PID";
        const DESCENDANT_PID_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_DESCENDANT_PID";
        const READY_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_READY";
        const CUTOFF_READY_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_CUTOFF_READY";
        const CUTOFF_RELEASE_ENV: &str = "VELVET_GLOVE_VACUUM_LIFECYCLE_CUTOFF_RELEASE";

        let phase = spec
            .phases
            .get("verify")
            .ok_or_else(|| "Vacuum lifecycle probe lacks a verify phase".to_owned())?;
        let [
            ArgvElement::Literal(isolated),
            ArgvElement::Literal(command),
            ArgvElement::Literal(adapter),
            ArgvElement::Token(ArgToken::ToolExecutable),
            ArgvElement::Token(ArgToken::ExtraArgs),
            ArgvElement::Literal(marker),
            ArgvElement::Token(ArgToken::Files),
        ] = phase.argv.as_slice()
        else {
            return Err(
                "Vacuum lifecycle probe could not extract the evaluated adapter".to_owned(),
            );
        };
        if isolated != "-I" || command != "-c" || marker != VACUUM_FILES_MARKER {
            return Err(format!(
                "Vacuum lifecycle probe expected exact isolated marker shape, got {isolated:?} {command:?} marker={marker:?}"
            ));
        }
        let python_program = phase
            .program
            .as_deref()
            .ok_or_else(|| "Vacuum lifecycle probe lacks an adapter program".to_owned())?;
        let python = resolve_program(python_program)
            .ok_or_else(|| format!("Vacuum lifecycle probe cannot resolve {python_program:?}"))?
            .canonicalize()
            .map_err(|error| format!("canonicalize Vacuum lifecycle Python: {error}"))?;

        let temporary = unique_temp_dir("velvet-glove-vacuum-lifecycle");
        let root = temporary
            .canonicalize()
            .map_err(|error| format!("canonicalize Vacuum lifecycle root: {error}"))?;
        let result = (|| {
            let target = root.join("selected-openapi.yaml");
            let baseline = b"openapi: 3.0.0\ninfo:\n  title: Lifecycle\n  description: Controlled\n  version: 1.0.0\npaths: {}\n";
            std::fs::write(&target, baseline)
                .map_err(|error| format!("write Vacuum lifecycle target: {error}"))?;
            std::fs::write(
                root.join("vacuum.conf.yaml"),
                "lint: [project configuration must not load\n",
            )
            .map_err(|error| format!("write hostile Vacuum lifecycle config: {error}"))?;

            let clean_tool = root.join("vacuum-clean-fake");
            write_executable_fixture(
                &clean_tool,
                r#"#!/bin/sh
set -eu
: > "$VELVET_GLOVE_VACUUM_LIFECYCLE_ARGV"
for argument in "$@"; do
  printf '%s\n' "$argument" >> "$VELVET_GLOVE_VACUUM_LIFECYCLE_ARGV"
done
/bin/pwd -P > "$VELVET_GLOVE_VACUUM_LIFECYCLE_CWD"
/bin/cp vacuum.conf.yaml "$VELVET_GLOVE_VACUUM_LIFECYCLE_CONFIG"
last=
for argument in "$@"; do last=$argument; done
/bin/cp "$last" "$VELVET_GLOVE_VACUUM_LIFECYCLE_COPY"
: > "$VELVET_GLOVE_VACUUM_LIFECYCLE_MODES"
for controlled in vacuum.conf.yaml "$last" . inputs "$HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$TMPDIR"; do
  if controlled_mode=$(/usr/bin/stat -f '%Lp' "$controlled" 2>/dev/null); then :; else controlled_mode=$(/usr/bin/stat -c '%a' "$controlled"); fi
  printf '%s\n' "$controlled_mode" >> "$VELVET_GLOVE_VACUUM_LIFECYCLE_MODES"
done
{
  printf 'PATH=%s\n' "${PATH-}"
  printf 'HOME=%s\n' "${HOME-}"
  printf 'PWD=%s\n' "${PWD-}"
  printf 'TMPDIR=%s\n' "${TMPDIR-}"
  printf 'XDG_CONFIG_HOME=%s\n' "${XDG_CONFIG_HOME-}"
  printf 'XDG_CACHE_HOME=%s\n' "${XDG_CACHE_HOME-}"
  printf 'VACUUM_RULESET=%s\n' "${VACUUM_RULESET-}"
  printf 'HTTP_PROXY=%s\n' "${HTTP_PROXY-}"
  printf 'GODEBUG=%s\n' "${GODEBUG-}"
  printf 'GOMAXPROCS=%s\n' "${GOMAXPROCS-}"
  printf 'GOTRACEBACK=%s\n' "${GOTRACEBACK-}"
} > "$VELVET_GLOVE_VACUUM_LIFECYCLE_ENVIRONMENT"
exit 0
"#,
                "Vacuum clean lifecycle fake",
            )?;
            let clean_argv = root.join("clean.argv");
            let clean_cwd = root.join("clean.cwd");
            let clean_copy = root.join("clean.copy");
            let clean_config = root.join("clean.config");
            let clean_modes = root.join("clean.modes");
            let clean_environment = root.join("clean.environment");
            let mut clean = vacuum_lifecycle_command(
                &python,
                adapter,
                &clean_tool,
                &[],
                std::slice::from_ref(&target),
                &root,
            );
            clean
                .env(ARGV_ENV, &clean_argv)
                .env(CWD_ENV, &clean_cwd)
                .env(COPY_ENV, &clean_copy)
                .env(CONFIG_ENV, &clean_config)
                .env(MODES_ENV, &clean_modes)
                .env(ENVIRONMENT_ENV, &clean_environment)
                .env("VACUUM_RULESET", "poison")
                .env("HTTP_PROXY", "poison")
                .env("GODEBUG", "poison")
                .env("GOMAXPROCS", "999")
                .env("GOTRACEBACK", "all");
            let clean_output = run_with_timeout(
                &mut clean,
                b"",
                timeout.min(Duration::from_secs(10)),
                &root.join("clean-evidence"),
            )
            .map_err(|error| format!("run Vacuum clean lifecycle probe: {error}"))?;
            if clean_output.status.code() != Some(0)
                || !clean_output.stdout.is_empty()
                || !clean_output.stderr.is_empty()
            {
                return Err(format!(
                    "Vacuum clean lifecycle failed: status={:?}; stdout={:?}; stderr={:?}",
                    clean_output.status.code(),
                    String::from_utf8_lossy(&clean_output.stdout),
                    String::from_utf8_lossy(&clean_output.stderr)
                ));
            }
            let expected_argv = VACUUM_FIXED_ARGUMENTS
                .iter()
                .copied()
                .chain(std::iter::once("inputs/0000.yaml"))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            let observed_argv = std::fs::read_to_string(&clean_argv)
                .map_err(|error| format!("read Vacuum clean lifecycle argv: {error}"))?;
            if observed_argv != expected_argv
                || observed_argv.contains(target.to_string_lossy().as_ref())
            {
                return Err(format!(
                    "Vacuum child did not receive only the fixed command and private copy: expected={expected_argv:?}; observed={observed_argv:?}"
                ));
            }
            if std::fs::read(&clean_copy)
                .map_err(|error| format!("read Vacuum private copy: {error}"))?
                != baseline
                || std::fs::read(&clean_config)
                    .map_err(|error| format!("read Vacuum private config: {error}"))?
                    != b"{}\n"
                || std::fs::read_to_string(&clean_modes)
                    .map_err(|error| format!("read Vacuum private modes: {error}"))?
                    != "600\n600\n700\n700\n700\n700\n700\n700\n"
            {
                return Err(
                    "Vacuum lifecycle did not create exact 0600 config/input copies".to_owned(),
                );
            }
            let controlled_environment = std::fs::read_to_string(&clean_environment)
                .map_err(|error| format!("read Vacuum clean child environment: {error}"))?;
            for required in [
                "PATH=/usr/bin:/bin\n",
                "VACUUM_RULESET=\n",
                "HTTP_PROXY=\n",
                "GODEBUG=\n",
                "GOMAXPROCS=1\n",
                "GOTRACEBACK=none\n",
            ] {
                if !controlled_environment.contains(required) {
                    return Err(format!(
                        "Vacuum lifecycle child environment omits {required:?}: {controlled_environment:?}"
                    ));
                }
            }
            assert_vacuum_private_root_removed(&clean_cwd, "clean")?;

            let umask_start_anchor = "    private_aliases = list(dict.fromkeys((private_root, os.path.realpath(private_root))))\n";
            let umask_restore_anchor = "    environment = os.environ.copy()\n";
            if adapter.matches(umask_start_anchor).count() != 1
                || adapter.matches(umask_restore_anchor).count() != 1
            {
                return Err(
                    "Vacuum hostile-umask probe requires exact creation-window anchors".to_owned(),
                );
            }
            let umask_adapter = adapter
                .replacen(
                    umask_start_anchor,
                    &format!("    original_umask = os.umask(0o777)\n{umask_start_anchor}"),
                    1,
                )
                .replacen(
                    umask_restore_anchor,
                    &format!("    os.umask(original_umask)\n{umask_restore_anchor}"),
                    1,
                );
            let umask_argv = root.join("umask.argv");
            let umask_cwd = root.join("umask.cwd");
            let umask_copy = root.join("umask.copy");
            let umask_config = root.join("umask.config");
            let umask_modes = root.join("umask.modes");
            let umask_environment = root.join("umask.environment");
            let mut umask = vacuum_lifecycle_command(
                &python,
                &umask_adapter,
                &clean_tool,
                &[],
                std::slice::from_ref(&target),
                &root,
            );
            umask
                .env(ARGV_ENV, &umask_argv)
                .env(CWD_ENV, &umask_cwd)
                .env(COPY_ENV, &umask_copy)
                .env(CONFIG_ENV, &umask_config)
                .env(MODES_ENV, &umask_modes)
                .env(ENVIRONMENT_ENV, &umask_environment);
            let umask_output = run_with_timeout(
                &mut umask,
                b"",
                timeout.min(Duration::from_secs(5)),
                &root.join("umask-evidence"),
            )
            .map_err(|error| format!("run Vacuum hostile-umask probe: {error}"))?;
            let umask_observed_modes = std::fs::read_to_string(&umask_modes)
                .map_err(|error| format!("read Vacuum hostile-umask modes: {error}"))?;
            if umask_output.status.code() != Some(0)
                || !umask_output.stdout.is_empty()
                || !umask_output.stderr.is_empty()
                || umask_observed_modes != "600\n600\n700\n700\n700\n700\n700\n700\n"
            {
                return Err(format!(
                    "Vacuum hostile umask changed controlled permissions or completion: status={:?}; modes={umask_observed_modes:?}; stdout={:?}; stderr={:?}",
                    umask_output.status.code(),
                    String::from_utf8_lossy(&umask_output.stdout),
                    String::from_utf8_lossy(&umask_output.stderr)
                ));
            }
            assert_vacuum_private_root_removed(&umask_cwd, "hostile-umask")?;

            let rejection_tool = root.join("vacuum-rejection-fake");
            write_executable_fixture(
                &rejection_tool,
                "#!/bin/sh\nset -eu\n: > \"$VELVET_GLOVE_VACUUM_LIFECYCLE_INVOKED\"\nexit 0\n",
                "Vacuum rejection lifecycle fake",
            )?;
            let invoked = root.join("rejection-invoked");
            let rejection_cases = vec![
                (
                    "raw-ref",
                    b"openapi: 3.0.0\npaths: {}\n$ref: sibling.yaml\n".to_vec(),
                    "unsupported external-reference syntax",
                ),
                (
                    "raw-dynamic-ref",
                    b"openapi: 3.1.0\npaths: {}\n$dynamicRef: sibling.yaml\n".to_vec(),
                    "unsupported external-reference syntax",
                ),
                (
                    "raw-recursive-ref",
                    b"openapi: 3.1.0\npaths: {}\n$recursiveRef: sibling.yaml\n".to_vec(),
                    "unsupported external-reference syntax",
                ),
                (
                    "hex-dollar",
                    br#"note: "\x24"
"#
                    .to_vec(),
                    "unsupported encoded dollar token",
                ),
                (
                    "unicode-dollar",
                    br#"note: "\u0024"
"#
                    .to_vec(),
                    "unsupported encoded dollar token",
                ),
                (
                    "long-unicode-dollar",
                    br#"note: "\U00000024"
"#
                    .to_vec(),
                    "unsupported encoded dollar token",
                ),
                (
                    "mixed-ref",
                    br#""$\u0072ef": "sibling.yaml"
"#
                    .to_vec(),
                    "unsupported escaped external-reference syntax",
                ),
                (
                    "mixed-dynamic-ref",
                    br#""$d\u0079namicRef": "sibling.yaml"
"#
                    .to_vec(),
                    "unsupported escaped external-reference syntax",
                ),
                (
                    "mixed-recursive-ref",
                    br#""$recursive\u0052ef": "sibling.yaml"
"#
                    .to_vec(),
                    "unsupported escaped external-reference syntax",
                ),
                (
                    "del-path",
                    baseline.to_vec(),
                    "selected path contains a control character",
                ),
                ("non-utf8", vec![0xff, 0xfe], "selected file is not UTF-8"),
            ];
            for (label, content, diagnostic) in rejection_cases {
                let selected = if label == "del-path" {
                    root.join("del\u{7f}-openapi.yaml")
                } else {
                    root.join(format!("{label}-openapi.yaml"))
                };
                std::fs::write(&selected, content)
                    .map_err(|error| format!("write Vacuum {label} rejection input: {error}"))?;
                let _ = std::fs::remove_file(&invoked);
                let mut command = vacuum_lifecycle_command(
                    &python,
                    adapter,
                    &rejection_tool,
                    &[],
                    std::slice::from_ref(&selected),
                    &root,
                );
                command.env(INVOKED_ENV, &invoked);
                let output = run_with_timeout(
                    &mut command,
                    b"",
                    timeout.min(Duration::from_secs(5)),
                    &root.join(format!("rejection-{label}-evidence")),
                )
                .map_err(|error| format!("run Vacuum {label} rejection: {error}"))?;
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.code() != Some(2)
                    || !output.stdout.is_empty()
                    || !stderr.contains(diagnostic)
                    || stderr.matches("velvet-glove-vacuum:").count() != 1
                    || invoked.exists()
                {
                    return Err(format!(
                        "Vacuum {label} rejection did not fail before child execution: status={:?}; stdout={:?}; stderr={stderr:?}; invoked={}",
                        output.status.code(),
                        String::from_utf8_lossy(&output.stdout),
                        invoked.exists()
                    ));
                }
            }

            let regular = root.join("regular-openapi.yaml");
            let hardlink = root.join("hardlink-openapi.yaml");
            let symlink = root.join("symlink-openapi.yaml");
            std::fs::write(&regular, baseline)
                .map_err(|error| format!("write Vacuum link rejection input: {error}"))?;
            std::fs::hard_link(&regular, &hardlink)
                .map_err(|error| format!("create Vacuum hardlink rejection input: {error}"))?;
            std::os::unix::fs::symlink(&regular, &symlink)
                .map_err(|error| format!("create Vacuum symlink rejection input: {error}"))?;
            for (label, selected, diagnostic) in [
                (
                    "hardlink",
                    hardlink.as_path(),
                    "selected path is not a unique regular file",
                ),
                (
                    "symlink",
                    symlink.as_path(),
                    "selected path traverses a symlink",
                ),
            ] {
                let _ = std::fs::remove_file(&invoked);
                let mut command = vacuum_lifecycle_command(
                    &python,
                    adapter,
                    &rejection_tool,
                    &[],
                    &[selected.to_path_buf()],
                    &root,
                );
                command.env(INVOKED_ENV, &invoked);
                let output = run_with_timeout(
                    &mut command,
                    b"",
                    timeout.min(Duration::from_secs(5)),
                    &root.join(format!("rejection-{label}-evidence")),
                )
                .map_err(|error| format!("run Vacuum {label} rejection: {error}"))?;
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.code() != Some(2)
                    || !stderr.contains(diagnostic)
                    || invoked.exists()
                {
                    return Err(format!(
                        "Vacuum {label} rejection escaped preflight: status={:?}; stderr={stderr:?}; invoked={}",
                        output.status.code(),
                        invoked.exists()
                    ));
                }
            }

            for (label, extra, files, diagnostic) in [
                (
                    "extra-argument",
                    vec!["--remote=true"],
                    vec![target.clone()],
                    "extra arguments are unsupported",
                ),
                (
                    "file-count",
                    Vec::new(),
                    vec![target.clone(); 257],
                    "selected batch exceeds the 256-file limit",
                ),
            ] {
                let _ = std::fs::remove_file(&invoked);
                let mut command = vacuum_lifecycle_command(
                    &python,
                    adapter,
                    &rejection_tool,
                    &extra,
                    &files,
                    &root,
                );
                command.env(INVOKED_ENV, &invoked);
                let output = run_with_timeout(
                    &mut command,
                    b"",
                    timeout.min(Duration::from_secs(5)),
                    &root.join(format!("rejection-{label}-evidence")),
                )
                .map_err(|error| format!("run Vacuum {label} rejection: {error}"))?;
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.code() != Some(2)
                    || !stderr.contains(diagnostic)
                    || invoked.exists()
                {
                    return Err(format!(
                        "Vacuum {label} rejection escaped preflight: status={:?}; stderr={stderr:?}; invoked={}",
                        output.status.code(),
                        invoked.exists()
                    ));
                }
            }

            for (label, source, diagnostic) in [
                (
                    "clean-output",
                    "#!/bin/sh\nprintf unexpected\nexit 0\n",
                    "clean Vacuum lint emitted output under the silent contract",
                ),
                (
                    "empty-issue",
                    "#!/bin/sh\nexit 1\n",
                    "Vacuum violation status lacked a stable rule diagnostic",
                ),
            ] {
                let tool = root.join(format!("vacuum-{label}-fake"));
                write_executable_fixture(&tool, source, &format!("Vacuum {label} fake"))?;
                let mut command = vacuum_lifecycle_command(
                    &python,
                    adapter,
                    &tool,
                    &[],
                    std::slice::from_ref(&target),
                    &root,
                );
                let output = run_with_timeout(
                    &mut command,
                    b"",
                    timeout.min(Duration::from_secs(5)),
                    &root.join(format!("no-op-{label}-evidence")),
                )
                .map_err(|error| format!("run Vacuum {label} no-op resistance probe: {error}"))?;
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.code() != Some(2)
                    || !output.stdout.is_empty()
                    || !stderr.contains(diagnostic)
                    || stderr.matches("velvet-glove-vacuum:").count() != 1
                {
                    return Err(format!(
                        "Vacuum {label} no-op resistance failed: status={:?}; stdout={:?}; stderr={stderr:?}",
                        output.status.code(),
                        String::from_utf8_lossy(&output.stdout)
                    ));
                }
            }

            let mutation_tool = root.join("vacuum-mutation-fake");
            write_executable_fixture(
                &mutation_tool,
                r#"#!/bin/sh
set -eu
last=
for argument in "$@"; do last=$argument; done
/bin/cp "$last" "$VELVET_GLOVE_VACUUM_LIFECYCLE_COPY"
/bin/pwd -P > "$VELVET_GLOVE_VACUUM_LIFECYCLE_CWD"
printf 'replacement\n' > "$VELVET_GLOVE_VACUUM_LIFECYCLE_REPLACEMENT"
/bin/mv "$VELVET_GLOVE_VACUUM_LIFECYCLE_REPLACEMENT" "$VELVET_GLOVE_VACUUM_LIFECYCLE_ORIGINAL"
exit 0
"#,
                "Vacuum mutation lifecycle fake",
            )?;
            let mutation_target = root.join("mutated-openapi.yaml");
            std::fs::write(&mutation_target, baseline)
                .map_err(|error| format!("write Vacuum mutation target: {error}"))?;
            let mutation_copy = root.join("mutation.copy");
            let mutation_cwd = root.join("mutation.cwd");
            let replacement = root.join("replacement.tmp");
            let mut mutation = vacuum_lifecycle_command(
                &python,
                adapter,
                &mutation_tool,
                &[],
                std::slice::from_ref(&mutation_target),
                &root,
            );
            mutation
                .env(COPY_ENV, &mutation_copy)
                .env(CWD_ENV, &mutation_cwd)
                .env(ORIGINAL_ENV, &mutation_target)
                .env(REPLACEMENT_ENV, &replacement);
            let mutation_output = run_with_timeout(
                &mut mutation,
                b"",
                timeout.min(Duration::from_secs(5)),
                &root.join("mutation-evidence"),
            )
            .map_err(|error| format!("run Vacuum mutation probe: {error}"))?;
            let mutation_stderr = String::from_utf8_lossy(&mutation_output.stderr);
            if mutation_output.status.code() != Some(2)
                || !mutation_output.stdout.is_empty()
                || mutation_stderr
                    != "velvet-glove-vacuum: Vacuum mutated selected file mutated-openapi.yaml: changed content\n"
                || std::fs::read(&mutation_copy)
                    .map_err(|error| format!("read Vacuum mutation private copy: {error}"))?
                    != baseline
            {
                return Err(format!(
                    "Vacuum post-snapshot replacement was not contained: status={:?}; stdout={:?}; stderr={mutation_stderr:?}",
                    mutation_output.status.code(),
                    String::from_utf8_lossy(&mutation_output.stdout)
                ));
            }
            assert_vacuum_private_root_removed(&mutation_cwd, "mutation")?;

            let mkdtemp_anchor = "        private_root = tempfile.mkdtemp(prefix=\"velvet-glove-vacuum-\", dir=\"/tmp\")\n";
            if adapter.matches(mkdtemp_anchor).count() != 1 {
                return Err("Vacuum mkdtemp probe requires one exact live anchor".to_owned());
            }
            let mkdtemp_hook = "        raise PermissionError(13, \"Permission denied\", \"/tmp/velvet-glove-vacuum-sensitive-random\")\n";
            let mkdtemp_adapter = adapter.replacen(mkdtemp_anchor, mkdtemp_hook, 1);
            let mut mkdtemp = vacuum_lifecycle_command(
                &python,
                &mkdtemp_adapter,
                &rejection_tool,
                &[],
                std::slice::from_ref(&target),
                &root,
            );
            mkdtemp.env(INVOKED_ENV, &invoked);
            let _ = std::fs::remove_file(&invoked);
            let mkdtemp_output = run_with_timeout(
                &mut mkdtemp,
                b"",
                timeout.min(Duration::from_secs(5)),
                &root.join("mkdtemp-failure-evidence"),
            )
            .map_err(|error| format!("run Vacuum mkdtemp failure probe: {error}"))?;
            let mkdtemp_stderr = String::from_utf8_lossy(&mkdtemp_output.stderr);
            if mkdtemp_output.status.code() != Some(2)
                || !mkdtemp_output.stdout.is_empty()
                || mkdtemp_stderr.contains("sensitive-random")
                || !mkdtemp_stderr.contains("<vacuum-private>")
                || mkdtemp_stderr.matches("velvet-glove-vacuum:").count() != 1
                || invoked.exists()
            {
                return Err(format!(
                    "Vacuum mkdtemp failure leaked private state or invoked child: status={:?}; stdout={:?}; stderr={mkdtemp_stderr:?}; invoked={}",
                    mkdtemp_output.status.code(),
                    String::from_utf8_lossy(&mkdtemp_output.stdout),
                    invoked.exists()
                ));
            }

            let private_diagnostic_tool = root.join("vacuum-private-diagnostic-fake");
            write_executable_fixture(
                &private_diagnostic_tool,
                "#!/bin/sh\nset -eu\nprintf '%s\\n%s\\n%s\\n' \"$PWD\" \"$HOME\" \"$TMPDIR\" >&2\nexit 2\n",
                "Vacuum private-diagnostic lifecycle fake",
            )?;
            let mut private_diagnostic = vacuum_lifecycle_command(
                &python,
                adapter,
                &private_diagnostic_tool,
                &[],
                std::slice::from_ref(&target),
                &root,
            );
            let private_diagnostic_output = run_with_timeout(
                &mut private_diagnostic,
                b"",
                timeout.min(Duration::from_secs(5)),
                &root.join("private-diagnostic-evidence"),
            )
            .map_err(|error| format!("run Vacuum private-diagnostic probe: {error}"))?;
            let private_diagnostic_stderr =
                String::from_utf8_lossy(&private_diagnostic_output.stderr);
            if private_diagnostic_output.status.code() != Some(2)
                || !private_diagnostic_output.stdout.is_empty()
                || private_diagnostic_stderr.contains("velvet-glove-vacuum-")
                || !private_diagnostic_stderr.contains("<vacuum-private>\n")
                || !private_diagnostic_stderr.contains("<vacuum-private>/home\n")
                || !private_diagnostic_stderr.contains("<vacuum-private>/tmp\n")
            {
                return Err(format!(
                    "Vacuum status-2 diagnostic exposed private state: status={:?}; stdout={:?}; stderr={private_diagnostic_stderr:?}",
                    private_diagnostic_output.status.code(),
                    String::from_utf8_lossy(&private_diagnostic_output.stdout)
                ));
            }

            let output_tool = root.join("vacuum-output-cap-fake");
            let output_cwd = root.join("output-cap.cwd");
            write_executable_fixture(
                &output_tool,
                "#!/bin/sh\nset -eu\n/bin/pwd -P > \"$VELVET_GLOVE_VACUUM_LIFECYCLE_CWD\"\nexec /usr/bin/yes vacuum-output\n",
                "Vacuum output-cap lifecycle fake",
            )?;
            let mut output_cap = vacuum_lifecycle_command(
                &python,
                adapter,
                &output_tool,
                &[],
                std::slice::from_ref(&target),
                &root,
            );
            output_cap.env(CWD_ENV, &output_cwd);
            let output_cap_output = run_with_timeout(
                &mut output_cap,
                b"",
                timeout.min(Duration::from_secs(10)),
                &root.join("output-cap-evidence"),
            )
            .map_err(|error| format!("run Vacuum output-cap probe: {error}"))?;
            let output_cap_stderr = String::from_utf8_lossy(&output_cap_output.stderr);
            if output_cap_output.status.code() != Some(2)
                || !output_cap_output.stdout.is_empty()
                || !output_cap_stderr.contains("combined output exceeded 16777216 bytes")
                || output_cap_stderr.matches("velvet-glove-vacuum:").count() != 1
            {
                return Err(format!(
                    "Vacuum output cap did not fail closed: status={:?}; stdout_bytes={}; stderr={output_cap_stderr:?}",
                    output_cap_output.status.code(),
                    output_cap_output.stdout.len()
                ));
            }
            assert_vacuum_private_root_removed(&output_cwd, "output-cap")?;

            let spawn_tool = root.join("vacuum-spawn-signal-fake");
            write_executable_fixture(
                &spawn_tool,
                "#!/bin/sh\ntrap '' HUP INT TERM\nwhile :; do :; done\n",
                "Vacuum spawn-signal lifecycle fake",
            )?;
            let spawn_anchor = "            process = child\n";
            if adapter.matches(spawn_anchor).count() != 1 {
                return Err(
                    "Vacuum spawn cutoff probe requires one exact process-assignment anchor"
                        .to_owned(),
                );
            }
            let spawn_hook = concat!(
                "            with open(os.environ[\"VELVET_GLOVE_VACUUM_LIFECYCLE_CHILD_PID\"], \"x\") as handle:\n",
                "                handle.write(str(child.pid))\n",
                "            with open(os.environ[\"VELVET_GLOVE_VACUUM_LIFECYCLE_CWD\"], \"x\") as handle:\n",
                "                handle.write(private_root)\n",
                "            os.kill(os.getpid(), signal.SIGTERM)\n",
                "            process = child\n",
            );
            let spawn_adapter = adapter.replacen(spawn_anchor, spawn_hook, 1);
            let spawn_child_pid_path = root.join("spawn-signal.child.pid");
            let spawn_cwd = root.join("spawn-signal.cwd");
            let mut spawn_signal = vacuum_lifecycle_command(
                &python,
                &spawn_adapter,
                &spawn_tool,
                &[],
                std::slice::from_ref(&target),
                &root,
            );
            spawn_signal
                .env(CHILD_PID_ENV, &spawn_child_pid_path)
                .env(CWD_ENV, &spawn_cwd);
            let spawn_output = run_with_timeout(
                &mut spawn_signal,
                b"",
                timeout.min(Duration::from_secs(7)),
                &root.join("spawn-signal-evidence"),
            )
            .map_err(|error| format!("run Vacuum spawn-signal probe: {error}"))?;
            let spawn_child_pid =
                read_pid_file(&spawn_child_pid_path, "Vacuum spawn-signal child")?;
            let spawn_child_alive = process_survives(spawn_child_pid, Duration::from_secs(1))?;
            let spawn_group_alive =
                process_group_survives(spawn_child_pid, Duration::from_secs(1))?;
            if spawn_child_alive || spawn_group_alive {
                let _ = signal_process_group(spawn_child_pid, "KILL");
            }
            let spawn_stdout = String::from_utf8_lossy(&spawn_output.stdout);
            let spawn_stderr = String::from_utf8_lossy(&spawn_output.stderr);
            if spawn_output.status.code() != Some(2)
                || !spawn_stdout.is_empty()
                || spawn_stderr != "velvet-glove-vacuum: received signal 15\n"
                || spawn_child_alive
                || spawn_group_alive
            {
                return Err(format!(
                    "Vacuum Popen assignment signal was not contained: status={:?}; child={spawn_child_pid}:{spawn_child_alive}; group={spawn_group_alive}; stdout={spawn_stdout:?}; stderr={spawn_stderr:?}",
                    spawn_output.status.code()
                ));
            }
            assert_vacuum_private_root_removed(&spawn_cwd, "spawn-signal")?;

            let signal_tool = root.join("vacuum-signal-fake");
            write_executable_fixture(
                &signal_tool,
                r#"#!/bin/sh
set -eu
/bin/pwd -P > "$VELVET_GLOVE_VACUUM_LIFECYCLE_CWD"
trap '' HUP INT TERM
(
  trap '' HUP INT TERM
  while :; do :; done
) &
printf '%s\n' "$!" > "$VELVET_GLOVE_VACUUM_LIFECYCLE_DESCENDANT_PID"
printf '%s\n' "$$" > "$VELVET_GLOVE_VACUUM_LIFECYCLE_CHILD_PID"
: > "$VELVET_GLOVE_VACUUM_LIFECYCLE_READY"
while :; do :; done
"#,
                "Vacuum signal lifecycle fake",
            )?;
            for (signal_name, signal_number, followup) in [
                ("HUP", 1, None),
                ("INT", 2, None),
                ("TERM", 15, Some(("HUP", 1))),
            ] {
                let child_pid_path = root.join(format!("signal-{signal_name}.child.pid"));
                let descendant_pid_path = root.join(format!("signal-{signal_name}.descendant.pid"));
                let ready_path = root.join(format!("signal-{signal_name}.ready"));
                let cwd_path = root.join(format!("signal-{signal_name}.cwd"));
                let mut command = vacuum_lifecycle_command(
                    &python,
                    adapter,
                    &signal_tool,
                    &[],
                    std::slice::from_ref(&target),
                    &root,
                );
                command
                    .env(CHILD_PID_ENV, &child_pid_path)
                    .env(DESCENDANT_PID_ENV, &descendant_pid_path)
                    .env(READY_ENV, &ready_path)
                    .env(CWD_ENV, &cwd_path)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut outer = command.spawn().map_err(|error| {
                    format!("spawn Vacuum {signal_name} lifecycle probe: {error}")
                })?;
                let outer_pid = outer.id();
                let startup_timeout = timeout.min(Duration::from_secs(5));
                let startup_deadline = std::time::Instant::now() + startup_timeout;
                while !ready_path.is_file() {
                    if let Some(status) = outer.try_wait().map_err(|error| {
                        format!("poll Vacuum {signal_name} lifecycle probe: {error}")
                    })? {
                        return Err(format!(
                            "Vacuum {signal_name} lifecycle adapter exited {status:?} before child readiness"
                        ));
                    }
                    if std::time::Instant::now() >= startup_deadline {
                        let _ = signal_process(outer_pid, "KILL");
                        let _ = outer.wait();
                        return Err(format!(
                            "Vacuum {signal_name} lifecycle child did not become ready within {startup_timeout:?}"
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let child_pid = read_pid_file(
                    &child_pid_path,
                    &format!("Vacuum {signal_name} lifecycle child"),
                )?;
                let descendant_pid = read_pid_file(
                    &descendant_pid_path,
                    &format!("Vacuum {signal_name} lifecycle descendant"),
                )?;
                if !signal_process(outer_pid, signal_name)?.success() {
                    let _ = signal_process_group(child_pid, "KILL");
                    let _ = signal_process(outer_pid, "KILL");
                    let _ = outer.wait();
                    return Err(format!(
                        "send {signal_name} to Vacuum lifecycle adapter {outer_pid}"
                    ));
                }
                if let Some((followup_name, _)) = followup {
                    std::thread::sleep(Duration::from_millis(50));
                    if !signal_process(outer_pid, followup_name)?.success() {
                        let _ = signal_process_group(child_pid, "KILL");
                        let _ = signal_process(outer_pid, "KILL");
                        let _ = outer.wait();
                        return Err(format!(
                            "send follow-up {followup_name} while stopping Vacuum lifecycle adapter {outer_pid}"
                        ));
                    }
                }
                let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                std::thread::spawn(move || {
                    let _ = sender.send(outer.wait_with_output());
                });
                let completion_timeout = timeout.min(Duration::from_secs(7));
                let output = match receiver.recv_timeout(completion_timeout) {
                    Ok(Ok(output)) => output,
                    Ok(Err(error)) => {
                        let _ = signal_process_group(child_pid, "KILL");
                        return Err(format!(
                            "wait for Vacuum {signal_name} lifecycle adapter: {error}"
                        ));
                    }
                    Err(error) => {
                        let _ = signal_process_group(child_pid, "KILL");
                        let _ = signal_process(outer_pid, "KILL");
                        return Err(format!(
                            "Vacuum {signal_name} lifecycle output pipe remained open for {completion_timeout:?}: {error}"
                        ));
                    }
                };
                let child_alive = process_survives(child_pid, Duration::from_secs(1))?;
                let descendant_alive = process_survives(descendant_pid, Duration::from_secs(1))?;
                let group_alive = process_group_survives(child_pid, Duration::from_secs(1))?;
                if child_alive || descendant_alive || group_alive {
                    let _ = signal_process_group(child_pid, "KILL");
                }
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let expected_stderr = if let Some((_, followup_number)) = followup {
                    format!(
                        "velvet-glove-vacuum: received signal {signal_number}; also: received signal {followup_number}\n"
                    )
                } else {
                    format!("velvet-glove-vacuum: received signal {signal_number}\n")
                };
                if output.status.code() != Some(2)
                    || !stdout.is_empty()
                    || stderr != expected_stderr
                    || child_alive
                    || descendant_alive
                    || group_alive
                {
                    return Err(format!(
                        "Vacuum {signal_name} lifecycle containment failed: status={:?}; child={child_pid}:{child_alive}; descendant={descendant_pid}:{descendant_alive}; group={group_alive}; stdout={stdout:?}; stderr={stderr:?}",
                        output.status.code()
                    ));
                }
                assert_vacuum_private_root_removed(&cwd_path, signal_name)?;
            }

            let orphan_tool = root.join("vacuum-normal-exit-orphan-fake");
            let orphan_cwd = root.join("normal-exit-orphan.cwd");
            let orphan_child_pid_path = root.join("normal-exit-orphan.child.pid");
            let orphan_pid_path = root.join("normal-exit-orphan.pid");
            write_executable_fixture(
                &orphan_tool,
                r#"#!/bin/sh
set -eu
/bin/pwd -P > "$VELVET_GLOVE_VACUUM_LIFECYCLE_CWD"
printf '%s\n' "$$" > "$VELVET_GLOVE_VACUUM_LIFECYCLE_CHILD_PID"
(
  trap '' HUP INT TERM
  while :; do :; done
) &
printf '%s\n' "$!" > "$VELVET_GLOVE_VACUUM_LIFECYCLE_DESCENDANT_PID"
exit 0
"#,
                "Vacuum normal-exit orphan lifecycle fake",
            )?;
            let mut orphan = vacuum_lifecycle_command(
                &python,
                adapter,
                &orphan_tool,
                &[],
                std::slice::from_ref(&target),
                &root,
            );
            orphan
                .env(CWD_ENV, &orphan_cwd)
                .env(CHILD_PID_ENV, &orphan_child_pid_path)
                .env(DESCENDANT_PID_ENV, &orphan_pid_path);
            let orphan_output = run_with_timeout(
                &mut orphan,
                b"",
                timeout.min(Duration::from_secs(10)),
                &root.join("normal-exit-orphan-evidence"),
            )
            .map_err(|error| format!("run Vacuum normal-exit orphan probe: {error}"))?;
            let orphan_pid =
                read_pid_file(&orphan_pid_path, "Vacuum normal-exit orphan descendant")?;
            let orphan_child_pid =
                read_pid_file(&orphan_child_pid_path, "Vacuum normal-exit orphan leader")?;
            let orphan_alive = process_survives(orphan_pid, Duration::from_secs(1))?;
            let orphan_group_alive =
                process_group_survives(orphan_child_pid, Duration::from_secs(1))?;
            if orphan_alive || orphan_group_alive {
                let _ = signal_process_group(orphan_child_pid, "KILL");
            }
            let orphan_stdout = String::from_utf8_lossy(&orphan_output.stdout);
            let orphan_stderr = String::from_utf8_lossy(&orphan_output.stderr);
            if orphan_output.status.code() != Some(2)
                || !orphan_stdout.is_empty()
                || orphan_stderr
                    != "velvet-glove-vacuum: child left same-group descendants after leader exit\n"
                || orphan_alive
                || orphan_group_alive
            {
                return Err(format!(
                    "Vacuum normal-exit orphan was not swept: status={:?}; orphan={orphan_pid}:{orphan_alive}; group={orphan_group_alive}; stdout={orphan_stdout:?}; stderr={orphan_stderr:?}",
                    orphan_output.status.code()
                ));
            }
            assert_vacuum_private_root_removed(&orphan_cwd, "normal-exit-orphan")?;

            let cutoff_anchor = "            blocked_mask = signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)\n";
            if adapter.matches(cutoff_anchor).count() != 1 {
                return Err(
                    "Vacuum cutoff probe requires one exact live SIG_BLOCK anchor".to_owned(),
                );
            }
            let cutoff_hook = concat!(
                "            blocked_mask = signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)\n",
                "            with open(os.environ[\"VELVET_GLOVE_VACUUM_LIFECYCLE_CUTOFF_READY\"], \"xb\"):\n",
                "                pass\n",
                "            while not os.path.exists(os.environ[\"VELVET_GLOVE_VACUUM_LIFECYCLE_CUTOFF_RELEASE\"]):\n",
                "                time.sleep(0.01)\n",
            );
            let cutoff_adapter = adapter.replacen(cutoff_anchor, cutoff_hook, 1);
            let cutoff_ready = root.join("cutoff.ready");
            let cutoff_release = root.join("cutoff.release");
            let cutoff_cwd = root.join("cutoff.cwd");
            let mut cutoff = vacuum_lifecycle_command(
                &python,
                &cutoff_adapter,
                &clean_tool,
                &[],
                std::slice::from_ref(&target),
                &root,
            );
            cutoff
                .env(ARGV_ENV, root.join("cutoff.argv"))
                .env(CWD_ENV, &cutoff_cwd)
                .env(COPY_ENV, root.join("cutoff.copy"))
                .env(CONFIG_ENV, root.join("cutoff.config"))
                .env(MODES_ENV, root.join("cutoff.modes"))
                .env(ENVIRONMENT_ENV, root.join("cutoff.environment"))
                .env(CUTOFF_READY_ENV, &cutoff_ready)
                .env(CUTOFF_RELEASE_ENV, &cutoff_release)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut cutoff_outer = cutoff
                .spawn()
                .map_err(|error| format!("spawn Vacuum cutoff lifecycle probe: {error}"))?;
            let cutoff_outer_pid = cutoff_outer.id();
            let cutoff_startup_timeout = timeout.min(Duration::from_secs(5));
            let cutoff_startup_deadline = std::time::Instant::now() + cutoff_startup_timeout;
            while !cutoff_ready.is_file() {
                if let Some(status) = cutoff_outer
                    .try_wait()
                    .map_err(|error| format!("poll Vacuum cutoff lifecycle probe: {error}"))?
                {
                    return Err(format!(
                        "Vacuum cutoff adapter exited {status:?} before reaching the blocked window"
                    ));
                }
                if std::time::Instant::now() >= cutoff_startup_deadline {
                    let _ = signal_process(cutoff_outer_pid, "KILL");
                    let _ = cutoff_outer.wait();
                    return Err(format!(
                        "Vacuum cutoff adapter did not reach its blocked window within {cutoff_startup_timeout:?}"
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_vacuum_private_root_removed(&cutoff_cwd, "cutoff-before-release")?;
            if !signal_process(cutoff_outer_pid, "TERM")?.success() {
                let _ = signal_process(cutoff_outer_pid, "KILL");
                let _ = cutoff_outer.wait();
                return Err("send blocked-window TERM to Vacuum cutoff adapter".to_owned());
            }
            std::fs::write(&cutoff_release, b"release\n")
                .map_err(|error| format!("release Vacuum cutoff lifecycle hook: {error}"))?;
            let (cutoff_sender, cutoff_receiver) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = cutoff_sender.send(cutoff_outer.wait_with_output());
            });
            let cutoff_output = cutoff_receiver
                .recv_timeout(timeout.min(Duration::from_secs(5)))
                .map_err(|error| {
                    let _ = signal_process(cutoff_outer_pid, "KILL");
                    format!("wait for Vacuum cutoff lifecycle adapter: {error}")
                })?
                .map_err(|error| format!("collect Vacuum cutoff lifecycle output: {error}"))?;
            let cutoff_stdout = String::from_utf8_lossy(&cutoff_output.stdout);
            let cutoff_stderr = String::from_utf8_lossy(&cutoff_output.stderr);
            if cutoff_output.status.code() != Some(2)
                || !cutoff_stdout.is_empty()
                || cutoff_stderr != "velvet-glove-vacuum: received signal 15\n"
            {
                return Err(format!(
                    "Vacuum blocked cleanup signal was not drained exactly: status={:?}; stdout={cutoff_stdout:?}; stderr={cutoff_stderr:?}",
                    cutoff_output.status.code()
                ));
            }
            assert_vacuum_private_root_removed(&cutoff_cwd, "cutoff-after-exit")?;

            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&root);
        result
    }
}

fn verify_gofmt_adapter_lifecycle(spec: &ToolSpec, timeout: Duration) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (spec, timeout);
        return Ok(());
    }
    #[cfg(unix)]
    {
        const CHILD_PID_ENV: &str = "VELVET_GLOVE_GOFMT_LIFECYCLE_CHILD_PID";
        const DESCENDANT_PID_ENV: &str = "VELVET_GLOVE_GOFMT_LIFECYCLE_DESCENDANT_PID";
        const READY_ENV: &str = "VELVET_GLOVE_GOFMT_LIFECYCLE_READY";
        const ARGV_ENV: &str = "VELVET_GLOVE_GOFMT_LIFECYCLE_ARGV";
        const INVOKED_ENV: &str = "VELVET_GLOVE_GOFMT_LIFECYCLE_INVOKED";

        let phase = spec
            .phases
            .get("format")
            .ok_or_else(|| "gofmt lifecycle probe lacks a format phase".to_owned())?;
        let [
            ArgvElement::Literal(isolated),
            ArgvElement::Literal(command),
            ArgvElement::Literal(adapter),
            ArgvElement::Token(ArgToken::ToolExecutable),
            ArgvElement::Literal(mode),
            ArgvElement::Token(ArgToken::ExtraArgs),
            ArgvElement::Literal(marker),
            ArgvElement::Token(ArgToken::Files),
        ] = phase.argv.as_slice()
        else {
            return Err("gofmt lifecycle probe could not extract the evaluated adapter".to_owned());
        };
        if isolated != "-I" || command != "-c" || mode != "write" || marker != GOFMT_FILES_MARKER {
            return Err(format!(
                "gofmt lifecycle probe expected exact isolated write shape, got {isolated:?} {command:?} mode={mode:?} marker={marker:?}"
            ));
        }
        let python_program = phase
            .program
            .as_deref()
            .ok_or_else(|| "gofmt lifecycle probe lacks an adapter program".to_owned())?;
        let python = resolve_program(python_program)
            .ok_or_else(|| format!("gofmt lifecycle probe cannot resolve {python_program:?}"))?
            .canonicalize()
            .map_err(|error| format!("canonicalize gofmt lifecycle Python: {error}"))?;

        let temporary = unique_temp_dir("velvet-glove-gofmt-lifecycle");
        let root = temporary
            .canonicalize()
            .map_err(|error| format!("canonicalize gofmt lifecycle root: {error}"))?;
        let result = (|| {
            let target = root.join("selected.go");
            std::fs::write(&target, "package main\n")
                .map_err(|error| format!("write gofmt lifecycle target {target:?}: {error}"))?;
            let fake_tool = root.join("gofmt-fake");
            let child_pid_path = root.join("child.pid");
            let descendant_pid_path = root.join("descendant.pid");
            let ready_path = root.join("ready");
            let argv_path = root.join("child.argv");
            let fake_source = format!(
                r#"#!/bin/sh
set -eu
: > "${{{ARGV_ENV}}}"
for argument in "$@"; do
  printf '%s\n' "$argument" >> "${{{ARGV_ENV}}}"
done
trap 'exit 0' HUP INT TERM
(
  trap '' HUP INT TERM
  while :; do
    :
  done
) &
printf '%s\n' "$!" > "${{{DESCENDANT_PID_ENV}}}"
printf '%s\n' "$$" > "${{{CHILD_PID_ENV}}}"
: > "${{{READY_ENV}}}"
while :; do
  :
done
"#
            );
            write_executable_fixture(&fake_tool, &fake_source, "gofmt lifecycle fake")?;

            let mut command = Command::new(&python);
            command
                .args(["-I", "-c", adapter])
                .arg(&fake_tool)
                .arg("write")
                .arg(GOFMT_FILES_MARKER)
                .arg(&target)
                .current_dir(&root)
                .env(CHILD_PID_ENV, &child_pid_path)
                .env(DESCENDANT_PID_ENV, &descendant_pid_path)
                .env(READY_ENV, &ready_path)
                .env(ARGV_ENV, &argv_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut outer = command
                .spawn()
                .map_err(|error| format!("spawn evaluated gofmt lifecycle adapter: {error}"))?;
            let outer_pid = outer.id();
            let startup_timeout = timeout.min(Duration::from_secs(5));
            let startup_deadline = std::time::Instant::now() + startup_timeout;
            while !ready_path.is_file() {
                if let Some(status) = outer
                    .try_wait()
                    .map_err(|error| format!("poll gofmt lifecycle adapter: {error}"))?
                {
                    return Err(format!(
                        "gofmt lifecycle adapter exited {status:?} before its child became ready"
                    ));
                }
                if std::time::Instant::now() >= startup_deadline {
                    let _ = signal_process(outer_pid, "KILL");
                    let _ = outer.wait();
                    return Err(format!(
                        "gofmt lifecycle child did not become ready within {startup_timeout:?}"
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let child_pid = read_pid_file(&child_pid_path, "gofmt lifecycle child")?;
            let descendant_pid = read_pid_file(&descendant_pid_path, "gofmt lifecycle descendant")?;
            let observed_argv = std::fs::read_to_string(&argv_path)
                .map_err(|error| format!("read gofmt lifecycle argv: {error}"))?;
            let expected_argv = format!("-l\n{}\n", target.display());
            if observed_argv != expected_argv {
                let _ = signal_process_group(child_pid, "KILL");
                let _ = signal_process(outer_pid, "KILL");
                let _ = outer.wait();
                return Err(format!(
                    "gofmt lifecycle preflight argv mismatch: expected {expected_argv:?}, got {observed_argv:?}"
                ));
            }
            if !signal_process(outer_pid, "TERM")?.success() {
                let _ = signal_process_group(child_pid, "KILL");
                let _ = signal_process(outer_pid, "KILL");
                let _ = outer.wait();
                return Err("send SIGTERM to gofmt lifecycle adapter".to_owned());
            }
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = sender.send(outer.wait_with_output());
            });
            let completion_timeout = timeout.min(Duration::from_secs(5));
            let output = match receiver.recv_timeout(completion_timeout) {
                Ok(Ok(output)) => output,
                Ok(Err(error)) => {
                    let _ = signal_process_group(child_pid, "KILL");
                    return Err(format!(
                        "wait for terminated gofmt lifecycle adapter: {error}"
                    ));
                }
                Err(error) => {
                    let _ = signal_process_group(child_pid, "KILL");
                    return Err(format!(
                        "gofmt lifecycle adapter or descendant pipe remained open for {completion_timeout:?}: {error}"
                    ));
                }
            };
            let child_alive = signal_process(child_pid, "0")?.success();
            let descendant_alive = signal_process(descendant_pid, "0")?.success();
            if child_alive || descendant_alive {
                let _ = signal_process_group(child_pid, "KILL");
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.code() != Some(2) {
                return Err(format!(
                    "SIGTERM gofmt lifecycle adapter exited {:?}, expected 2; stdout={stdout:?}; stderr={stderr:?}",
                    output.status.code()
                ));
            }
            if child_alive || descendant_alive {
                return Err(format!(
                    "SIGTERM gofmt lifecycle adapter left child={child_pid}:{child_alive} descendant={descendant_pid}:{descendant_alive} alive"
                ));
            }
            if !stdout.is_empty()
                || stderr != "velvet-glove-gofmt: received signal 15\n"
                || stderr.matches("received signal").count() != 1
            {
                return Err(format!(
                    "SIGTERM gofmt lifecycle output was not one stable diagnostic: stdout={stdout:?}; stderr={stderr:?}"
                ));
            }

            let rejection_tool = root.join("gofmt-rejection-fake");
            let invoked_path = root.join("rejection-invoked");
            let rejection_source =
                format!("#!/bin/sh\nset -eu\nprintf invoked > \"${{{INVOKED_ENV}}}\"\nexit 64\n");
            write_executable_fixture(&rejection_tool, &rejection_source, "gofmt rejection fake")?;
            let regular = root.join("regular.go");
            let hardlink = root.join("hardlink.go");
            let symlink = root.join("symlink.go");
            let extra_target = root.join("extra.go");
            std::fs::write(&regular, "package main\n")
                .map_err(|error| format!("write gofmt hardlink target: {error}"))?;
            std::fs::hard_link(&regular, &hardlink)
                .map_err(|error| format!("create gofmt hardlink target: {error}"))?;
            std::os::unix::fs::symlink(&regular, &symlink)
                .map_err(|error| format!("create gofmt symlink target: {error}"))?;
            std::fs::write(&extra_target, "package main\n")
                .map_err(|error| format!("write gofmt extra-argument target: {error}"))?;
            for (label, selected, extra, diagnostic) in [
                (
                    "symlink",
                    symlink.as_path(),
                    None,
                    "selected path traverses a symlink",
                ),
                (
                    "hardlink",
                    hardlink.as_path(),
                    None,
                    "selected path is not a unique regular file",
                ),
                (
                    "extra-argument",
                    extra_target.as_path(),
                    Some("-w"),
                    "extra arguments are unsupported",
                ),
            ] {
                let evidence = root.join(format!("rejection-{label}"));
                std::fs::create_dir(&evidence)
                    .map_err(|error| format!("create gofmt {label} rejection evidence: {error}"))?;
                let mut rejection = Command::new(&python);
                rejection
                    .args(["-I", "-c", adapter])
                    .arg(&rejection_tool)
                    .arg("write");
                if let Some(extra) = extra {
                    rejection.arg(extra);
                }
                rejection
                    .arg(GOFMT_FILES_MARKER)
                    .arg(selected)
                    .current_dir(&root)
                    .env(INVOKED_ENV, &invoked_path);
                let output = run_with_timeout(
                    &mut rejection,
                    b"",
                    timeout.min(Duration::from_secs(5)),
                    &evidence,
                )
                .map_err(|error| format!("run gofmt {label} rejection: {error}"))?;
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.code() != Some(2)
                    || !output.stdout.is_empty()
                    || !stderr.contains(diagnostic)
                    || invoked_path.exists()
                {
                    return Err(format!(
                        "gofmt {label} rejection failed closed incorrectly: status={:?}; stdout={:?}; stderr={stderr:?}; invoked={}",
                        output.status.code(),
                        String::from_utf8_lossy(&output.stdout),
                        invoked_path.exists()
                    ));
                }
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&root);
        result
    }
}

#[cfg(unix)]
fn write_executable_fixture(path: &Path, contents: &str, label: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|error| format!("write {label} {path:?}: {error}"))?;
    let mut permissions = std::fs::metadata(path)
        .map_err(|error| format!("inspect {label} {path:?}: {error}"))?
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)
        .map_err(|error| format!("make {label} executable: {error}"))
}

#[cfg(unix)]
fn read_pid_file(path: &Path, label: &str) -> Result<u32, String> {
    let value = std::fs::read_to_string(path)
        .map_err(|error| format!("read {label} PID {path:?}: {error}"))?;
    value
        .trim()
        .parse::<u32>()
        .map_err(|error| format!("parse {label} PID {value:?}: {error}"))
}

#[cfg(unix)]
fn verify_eslint_adapter_adversarial_lifecycle(
    spec: &ToolSpec,
    timeout: Duration,
) -> Result<(), String> {
    let phase = spec
        .phases
        .get("verify")
        .ok_or_else(|| "ESLint adversarial probe lacks a verify phase".to_owned())?;
    let [
        ArgvElement::Literal(isolated),
        ArgvElement::Literal(command),
        ArgvElement::Literal(adapter),
        ArgvElement::Literal(node_name),
        ArgvElement::Token(ArgToken::ToolExecutable),
        ArgvElement::Literal(mode),
        ArgvElement::Token(ArgToken::ExtraArgs),
        ArgvElement::Literal(marker),
        ArgvElement::Token(ArgToken::Files),
    ] = phase.argv.as_slice()
    else {
        return Err("ESLint adversarial probe could not extract the evaluated adapter".to_owned());
    };
    if isolated != "-I"
        || command != "-c"
        || node_name != "node"
        || mode != "verify"
        || marker != ESLINT_FILES_MARKER
    {
        return Err(format!(
            "ESLint adversarial probe found unexpected evaluated argv shape: {:?}",
            phase.argv
        ));
    }
    let python_program = phase
        .program
        .as_deref()
        .ok_or_else(|| "ESLint adversarial probe lacks an adapter program".to_owned())?;
    let python = resolve_program(python_program)
        .ok_or_else(|| format!("ESLint adversarial probe cannot resolve {python_program:?}"))?
        .canonicalize()
        .map_err(|error| format!("canonicalize ESLint probe Python: {error}"))?;

    let requested_root = unique_temp_dir("velvet-glove-eslint-adversarial");
    let project = requested_root.join("project");
    let temporary = requested_root.join("private-temp");
    std::fs::create_dir_all(&project)
        .map_err(|error| format!("create ESLint adversarial project: {error}"))?;
    std::fs::create_dir_all(&temporary)
        .map_err(|error| format!("create ESLint adversarial temporary root: {error}"))?;
    let project = project
        .canonicalize()
        .map_err(|error| format!("canonicalize ESLint adversarial project: {error}"))?;
    let temporary = temporary
        .canonicalize()
        .map_err(|error| format!("canonicalize ESLint adversarial temporary root: {error}"))?;
    let result = (|| {
        let target = project.join("example.js");
        std::fs::write(&target, "export const answer = 42;\n")
            .map_err(|error| format!("write ESLint adversarial target: {error}"))?;
        let fake_cli = project.join("eslint.js");
        std::fs::write(
            &fake_cli,
            "throw new Error('must run through paired Node');\n",
        )
        .map_err(|error| format!("write ESLint adversarial fake CLI: {error}"))?;
        let source_config = project.join(".velvet-glove-eslint.json");
        std::fs::write(&source_config, "{\"rules\":{\"semi\":\"off\"}}\n")
            .map_err(|error| format!("write ESLint safe data config: {error}"))?;

        let captured_config = requested_root.join("captured-config.cjs");
        let captured_suppressions = requested_root.join("captured-suppressions.json");
        let captured_root = requested_root.join("captured-private-root");
        let captured_modes = requested_root.join("captured-modes");
        let captured_environment = requested_root.join("captured-environment");
        let node_marker = requested_root.join("node-ran");
        let success_node = requested_root.join("paired-node-success");
        write_executable_probe(
            &success_node,
            &format!(
                r#"#!/bin/sh
set -eu
: > '{node_marker}'
config=''
suppressions=''
last=''
for argument in "$@"; do
  last=$argument
  case $argument in
    --config=*) config=${{argument#--config=}} ;;
    --suppressions-location=*) suppressions=${{argument#--suppressions-location=}} ;;
  esac
done
[ -n "$config" ] && [ -n "$suppressions" ] || exit 91
/bin/cp "$config" '{captured_config}'
/bin/cp "$suppressions" '{captured_suppressions}'
/usr/bin/dirname "$config" > '{captured_root}'
{{
  /usr/bin/stat -f '%Lp' "$(/usr/bin/dirname "$config")"
  /usr/bin/stat -f '%Lp' "$config"
  /usr/bin/stat -f '%Lp' "$suppressions"
}} > '{captured_modes}'
{{
  printf 'PATH=%s\n' "${{PATH-}}"
  printf 'LANG=%s\n' "${{LANG-}}"
  printf 'LC_ALL=%s\n' "${{LC_ALL-}}"
  printf 'TZ=%s\n' "${{TZ-}}"
  printf 'TERM=%s\n' "${{TERM-}}"
  printf 'CI=%s\n' "${{CI-}}"
  printf 'HOME=%s\n' "${{HOME-}}"
  printf 'TMPDIR=%s\n' "${{TMPDIR-}}"
  printf 'NODE_OPTIONS=%s\n' "${{NODE_OPTIONS-}}"
  printf 'ESLINT_USE_FLAT_CONFIG=%s\n' "${{ESLINT_USE_FLAT_CONFIG-}}"
}} > '{captured_environment}'
printf '{{"plugins":{{}}}}\n' > '{source_config}'
printf '[{{"filePath":"%s","messages":[],"suppressedMessages":[],"errorCount":0,"fatalErrorCount":0,"warningCount":0,"fixableErrorCount":0,"fixableWarningCount":0}}]\n' "$last"
"#,
                node_marker = shell_probe_path(&node_marker)?,
                captured_config = shell_probe_path(&captured_config)?,
                captured_suppressions = shell_probe_path(&captured_suppressions)?,
                captured_root = shell_probe_path(&captured_root)?,
                captured_modes = shell_probe_path(&captured_modes)?,
                captured_environment = shell_probe_path(&captured_environment)?,
                source_config = shell_probe_path(&source_config)?,
            ),
        )?;
        let output = run_eslint_adapter_probe(
            &python,
            adapter,
            &success_node,
            &fake_cli,
            "verify",
            &[],
            &[&target],
            &project,
            &temporary,
            timeout,
            &requested_root.join("capture-safe-config"),
        )?;
        if output.status.code() != Some(0) || !output.stdout.is_empty() || !output.stderr.is_empty()
        {
            return Err(format!(
                "ESLint data-config probe failed: status={:?} stdout={:?} stderr={:?}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let private_config = std::fs::read_to_string(&captured_config)
            .map_err(|error| format!("read captured ESLint private config: {error}"))?;
        if !private_config.starts_with("'use strict';\nmodule.exports = [")
            || !private_config.contains("\"semi\":[0,\"always\"]")
            || private_config.contains("plugins")
            || std::fs::read(&captured_suppressions)
                .map_err(|error| format!("read captured ESLint suppressions: {error}"))?
                != b"{}\n"
            || std::fs::read_to_string(&captured_modes)
                .map_err(|error| format!("read captured ESLint private modes: {error}"))?
                != "700\n600\n600\n"
        {
            return Err("ESLint child did not receive exact private data-only state".to_owned());
        }
        let private_root = PathBuf::from(
            std::fs::read_to_string(&captured_root)
                .map_err(|error| format!("read captured ESLint private root: {error}"))?
                .trim(),
        );
        if private_root.parent() != Some(temporary.as_path())
            || private_root.exists()
            || !private_root
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with(ESLINT_PRIVATE_ROOT_PREFIX))
        {
            return Err(format!(
                "ESLint private root escaped or survived cleanup: {private_root:?}"
            ));
        }
        let expected_environment = concat!(
            "PATH=/usr/bin:/bin\n",
            "LANG=C\n",
            "LC_ALL=C\n",
            "TZ=UTC\n",
            "TERM=dumb\n",
            "CI=1\n",
            "HOME=\n",
            "TMPDIR=\n",
            "NODE_OPTIONS=\n",
            "ESLINT_USE_FLAT_CONFIG=\n",
        );
        if std::fs::read_to_string(&captured_environment)
            .map_err(|error| format!("read captured ESLint environment: {error}"))?
            != expected_environment
        {
            return Err("ESLint child environment was not fully controlled".to_owned());
        }
        if std::fs::read_to_string(&source_config)
            .map_err(|error| format!("read swapped ESLint source config: {error}"))?
            != "{\"plugins\":{}}\n"
        {
            return Err("ESLint source-config swap probe did not execute".to_owned());
        }

        let reject_marker = requested_root.join("reject-node-ran");
        let reject_node = requested_root.join("paired-node-reject");
        write_executable_probe(
            &reject_node,
            &format!(
                "#!/bin/sh\nset -eu\n: > '{}'\nexit 0\n",
                shell_probe_path(&reject_marker)?
            ),
        )?;
        std::fs::write(&source_config, "{\"rules\":{}}\n")
            .map_err(|error| format!("restore safe ESLint data config: {error}"))?;
        let rejected = run_eslint_adapter_probe(
            &python,
            adapter,
            &reject_node,
            &fake_cli,
            "verify",
            &["--plugin=./executed.cjs"],
            &[&target],
            &project,
            &temporary,
            timeout,
            &requested_root.join("capture-extra-arg-reject"),
        )?;
        if rejected.status.code() != Some(2)
            || reject_marker.exists()
            || !String::from_utf8_lossy(&rejected.stderr)
                .contains("extra arguments are unsupported")
        {
            return Err("ESLint extra argument was not rejected before Node".to_owned());
        }
        std::fs::write(&source_config, "{\"plugins\":{}}\n")
            .map_err(|error| format!("write rejected ESLint plugin config: {error}"))?;
        let rejected = run_eslint_adapter_probe(
            &python,
            adapter,
            &reject_node,
            &fake_cli,
            "verify",
            &[],
            &[&target],
            &project,
            &temporary,
            timeout,
            &requested_root.join("capture-plugin-config-reject"),
        )?;
        if rejected.status.code() != Some(2)
            || reject_marker.exists()
            || !String::from_utf8_lossy(&rejected.stderr).contains("unsupported keys")
        {
            return Err("ESLint executable/plugin config was not rejected before Node".to_owned());
        }

        std::fs::write(&source_config, "{\"rules\":{}}\n")
            .map_err(|error| format!("restore ESLint signal config: {error}"))?;
        verify_eslint_adapter_initialization_failure(
            &python,
            adapter,
            &fake_cli,
            &target,
            &project,
            &requested_root,
            timeout,
        )?;
        verify_eslint_adapter_malformed_completion(
            &python,
            adapter,
            &fake_cli,
            &target,
            &project,
            &temporary,
            &requested_root,
            timeout,
        )?;
        verify_eslint_adapter_normal_exit_cleanup(
            &python,
            adapter,
            &fake_cli,
            &target,
            &project,
            &temporary,
            &requested_root,
            timeout,
        )?;
        verify_eslint_adapter_signal_cleanup(
            &python,
            adapter,
            &fake_cli,
            &target,
            &project,
            &temporary,
            &requested_root,
            timeout,
        )?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&requested_root);
    result
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn run_eslint_adapter_probe(
    python: &Path,
    adapter: &str,
    node: &Path,
    tool: &Path,
    phase: &str,
    extra_args: &[&str],
    targets: &[&Path],
    project: &Path,
    temporary: &Path,
    timeout: Duration,
    capture: &Path,
) -> Result<BoundedOutput, String> {
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", adapter])
        .arg(node)
        .arg(tool)
        .arg(phase)
        .args(extra_args)
        .arg(ESLINT_FILES_MARKER)
        .args(targets)
        .current_dir(project)
        .env(TMPDIR_ENV, temporary)
        .env("NODE_OPTIONS", ESLINT_POISON_ENV_VALUE)
        .env("NODE_PATH", ESLINT_POISON_ENV_VALUE)
        .env("ESLINT_USE_FLAT_CONFIG", ESLINT_POISON_ENV_VALUE)
        .env("ESLINT_VELVET_GLOVE_POISON", ESLINT_POISON_ENV_VALUE);
    run_with_timeout(&mut command, &[], timeout, capture)
        .map_err(|error| format!("run ESLint adapter probe: {error}"))
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn verify_eslint_adapter_initialization_failure(
    python: &Path,
    adapter: &str,
    fake_cli: &Path,
    target: &Path,
    project: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let unwritable = root.join("eslint-unwritable-temp");
    std::fs::create_dir(&unwritable)
        .map_err(|error| format!("create unwritable ESLint temporary root: {error}"))?;
    let mut permissions = std::fs::metadata(&unwritable)
        .map_err(|error| format!("inspect unwritable ESLint temporary root: {error}"))?
        .permissions();
    permissions.set_mode(0o500);
    std::fs::set_permissions(&unwritable, permissions)
        .map_err(|error| format!("make ESLint temporary root unwritable: {error}"))?;

    let invoked = root.join("eslint-mkdtemp-node-ran");
    let node = root.join("eslint-mkdtemp-node");
    write_executable_probe(
        &node,
        &format!(
            "#!/bin/sh\nset -eu\n: > '{}'\nexit 0\n",
            shell_probe_path(&invoked)?
        ),
    )?;
    let output_result = run_eslint_adapter_probe(
        python,
        adapter,
        &node,
        fake_cli,
        "verify",
        &[],
        &[target],
        project,
        &unwritable,
        timeout.min(Duration::from_secs(5)),
        &root.join("eslint-mkdtemp-failure-evidence"),
    );
    let mut restore = std::fs::metadata(&unwritable)
        .map_err(|error| format!("reinspect unwritable ESLint temporary root: {error}"))?
        .permissions();
    restore.set_mode(0o700);
    std::fs::set_permissions(&unwritable, restore)
        .map_err(|error| format!("restore ESLint temporary-root permissions: {error}"))?;
    let output = output_result?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || invoked.exists()
        || !stderr.contains("<eslint-private>")
        || stderr.contains(ESLINT_PRIVATE_ROOT_PREFIX)
        || stderr.matches("velvet-glove-eslint:").count() != 1
    {
        return Err(format!(
            "ESLint mkdtemp failure leaked private state or invoked Node: status={:?}; invoked={}; stdout={:?}; stderr={stderr:?}",
            output.status.code(),
            invoked.exists(),
            String::from_utf8_lossy(&output.stdout)
        ));
    }
    Ok(())
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn verify_eslint_adapter_malformed_completion(
    python: &Path,
    adapter: &str,
    fake_cli: &Path,
    target: &Path,
    project: &Path,
    temporary: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let baseline = std::fs::read(target)
        .map_err(|error| format!("read ESLint malformed-output baseline: {error}"))?;
    for (label, payload, diagnostic) in [
        (
            "fixable-count",
            r#"printf '[{"filePath":"%s","messages":[],"suppressedMessages":[],"errorCount":0,"fatalErrorCount":0,"warningCount":0,"fixableErrorCount":1,"fixableWarningCount":0}]\n' "$last"
exit 0"#,
            "invalid fixableErrorCount",
        ),
        (
            "fatal-marker",
            r#"printf '[{"filePath":"%s","messages":[{"severity":2,"message":"fatal","fatal":"yes"}],"suppressedMessages":[],"errorCount":1,"fatalErrorCount":0,"warningCount":0,"fixableErrorCount":0,"fixableWarningCount":0}]\n' "$last"
exit 1"#,
            "non-boolean fatal marker",
        ),
    ] {
        let invoked = root.join(format!("eslint-malformed-{label}-node-ran"));
        let node = root.join(format!("eslint-malformed-{label}-node"));
        write_executable_probe(
            &node,
            &format!(
                "#!/bin/sh\nset -eu\n: > '{}'\nlast=''\nfor argument in \"$@\"; do last=$argument; done\n{payload}\n",
                shell_probe_path(&invoked)?
            ),
        )?;
        let output = run_eslint_adapter_probe(
            python,
            adapter,
            &node,
            fake_cli,
            "verify",
            &[],
            &[target],
            project,
            temporary,
            timeout.min(Duration::from_secs(5)),
            &root.join(format!("eslint-malformed-{label}-evidence")),
        )?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let observed = std::fs::read(target)
            .map_err(|error| format!("read ESLint malformed-{label} target: {error}"))?;
        if output.status.code() != Some(2)
            || !invoked.exists()
            || !stderr.contains(diagnostic)
            || observed != baseline
        {
            return Err(format!(
                "ESLint malformed {label} completion was accepted or mutated input: status={:?}; invoked={}; unchanged={}; stdout={:?}; stderr={stderr:?}",
                output.status.code(),
                invoked.exists(),
                observed == baseline,
                String::from_utf8_lossy(&output.stdout)
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn assert_eslint_orphan_swept(
    output: &BoundedOutput,
    leader_record: &Path,
    descendant_record: &Path,
    private_root_record: &Path,
    label: &str,
) -> Result<(), String> {
    let leader = read_pid_file(leader_record, &format!("ESLint {label} leader"))?;
    let descendant = read_pid_file(descendant_record, &format!("ESLint {label} descendant"))?;
    let private_root = PathBuf::from(
        std::fs::read_to_string(private_root_record)
            .map_err(|error| format!("read ESLint {label} private root: {error}"))?
            .trim(),
    );
    let leader_alive = process_survives(leader, Duration::from_secs(1))?;
    let descendant_alive = process_survives(descendant, Duration::from_secs(1))?;
    let group_alive = process_group_survives(leader, Duration::from_secs(1))?;
    if leader_alive || descendant_alive || group_alive {
        let _ = signal_process_group(leader, "KILL");
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || leader_alive
        || descendant_alive
        || group_alive
        || private_root.exists()
        || !stderr.contains("native ESLint left same-group descendants after child exit")
        || stderr.matches("velvet-glove-eslint:").count() != 1
    {
        return Err(format!(
            "ESLint {label} orphan was not swept: status={:?}; leader={leader}:{leader_alive}; descendant={descendant}:{descendant_alive}; group={group_alive}; private_exists={}; stdout={:?}; stderr={stderr:?}",
            output.status.code(),
            private_root.exists(),
            String::from_utf8_lossy(&output.stdout)
        ));
    }
    Ok(())
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn verify_eslint_adapter_normal_exit_cleanup(
    python: &Path,
    adapter: &str,
    fake_cli: &Path,
    target: &Path,
    project: &Path,
    temporary: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let inherited_count = root.join("eslint-inherited-pipe-count");
    let inherited_leader = root.join("eslint-inherited-pipe-leader");
    let inherited_descendant = root.join("eslint-inherited-pipe-descendant");
    let inherited_private = root.join("eslint-inherited-pipe-private-root");
    let inherited_node = root.join("eslint-inherited-pipe-node");
    write_executable_probe(
        &inherited_node,
        &format!(
            r#"#!/bin/sh
set -eu
count=0
[ ! -f '{count}' ] || count=$(/bin/cat '{count}')
count=$((count + 1))
printf '%s\n' "$count" > '{count}'
last=''
for argument in "$@"; do
  last=$argument
  case $argument in --config=*) /usr/bin/dirname "${{argument#--config=}}" > '{private}' ;; esac
done
if [ "$count" -eq 1 ]; then
  printf '[{{"filePath":"%s","messages":[],"suppressedMessages":[],"errorCount":0,"fatalErrorCount":0,"warningCount":0,"fixableErrorCount":0,"fixableWarningCount":0}}]\n' "$last"
  exit 0
fi
(
  trap '' HUP INT TERM
  while :; do :; done
) &
printf '%s\n' "$!" > '{descendant}'
printf '%s\n' "$$" > '{leader}'
exit 0
"#,
            count = shell_probe_path(&inherited_count)?,
            private = shell_probe_path(&inherited_private)?,
            descendant = shell_probe_path(&inherited_descendant)?,
            leader = shell_probe_path(&inherited_leader)?,
        ),
    )?;
    let inherited_output = match run_eslint_adapter_probe(
        python,
        adapter,
        &inherited_node,
        fake_cli,
        "fix",
        &[],
        &[target],
        project,
        temporary,
        timeout.min(Duration::from_secs(7)),
        &root.join("eslint-inherited-pipe-evidence"),
    ) {
        Ok(output) => output,
        Err(error) => {
            if let Ok(leader) = read_pid_file(&inherited_leader, "ESLint inherited-pipe leader") {
                let _ = signal_process_group(leader, "KILL");
            }
            return Err(error);
        }
    };
    assert_eslint_orphan_swept(
        &inherited_output,
        &inherited_leader,
        &inherited_descendant,
        &inherited_private,
        "inherited-pipe",
    )?;

    let closed_leader = root.join("eslint-closed-stdio-leader");
    let closed_descendant = root.join("eslint-closed-stdio-descendant");
    let closed_private = root.join("eslint-closed-stdio-private-root");
    let closed_node = root.join("eslint-closed-stdio-node");
    write_executable_probe(
        &closed_node,
        &format!(
            r#"#!/bin/sh
set -eu
for argument in "$@"; do
  case $argument in --config=*) /usr/bin/dirname "${{argument#--config=}}" > '{private}' ;; esac
done
(
  exec </dev/null >/dev/null 2>/dev/null
  trap '' HUP INT TERM
  while :; do :; done
) &
printf '%s\n' "$!" > '{descendant}'
printf '%s\n' "$$" > '{leader}'
exec >/dev/null 2>/dev/null
exit 0
"#,
            private = shell_probe_path(&closed_private)?,
            descendant = shell_probe_path(&closed_descendant)?,
            leader = shell_probe_path(&closed_leader)?,
        ),
    )?;
    let closed_output = match run_eslint_adapter_probe(
        python,
        adapter,
        &closed_node,
        fake_cli,
        "verify",
        &[],
        &[target],
        project,
        temporary,
        timeout.min(Duration::from_secs(7)),
        &root.join("eslint-closed-stdio-evidence"),
    ) {
        Ok(output) => output,
        Err(error) => {
            if let Ok(leader) = read_pid_file(&closed_leader, "ESLint closed-stdio leader") {
                let _ = signal_process_group(leader, "KILL");
            }
            return Err(error);
        }
    };
    assert_eslint_orphan_swept(
        &closed_output,
        &closed_leader,
        &closed_descendant,
        &closed_private,
        "closed-stdio",
    )?;

    let output_anchor = "OUTPUT_LIMIT = 16 * 1024 * 1024\n";
    let cleanup_anchor = concat!(
        "            if process_group_exists():\n",
        "                failures.append(\"native child process group survived SIGKILL\")\n",
    );
    if adapter.matches(output_anchor).count() != 1 || adapter.matches(cleanup_anchor).count() != 1 {
        return Err("ESLint output/cleanup composition probe lost its exact anchors".to_owned());
    }
    let output_adapter = adapter
        .replacen(output_anchor, "OUTPUT_LIMIT = 1024\n", 1)
        .replacen(
            cleanup_anchor,
            concat!(
                "            if True:\n",
                "                failures.append(\"native child process group survived SIGKILL\")\n",
            ),
            1,
        );
    let output_count = root.join("eslint-output-cap-count");
    let output_leader = root.join("eslint-output-cap-leader");
    let output_node = root.join("eslint-output-cap-node");
    write_executable_probe(
        &output_node,
        &format!(
            r#"#!/bin/sh
set -eu
count=0
[ ! -f '{count}' ] || count=$(/bin/cat '{count}')
count=$((count + 1))
printf '%s\n' "$count" > '{count}'
last=''
for argument in "$@"; do last=$argument; done
if [ "$count" -eq 1 ]; then
  printf '[{{"filePath":"%s","messages":[],"suppressedMessages":[],"errorCount":0,"fatalErrorCount":0,"warningCount":0,"fixableErrorCount":0,"fixableWarningCount":0}}]\n' "$last"
  exit 0
fi
printf '%s\n' "$$" > '{leader}'
exec /usr/bin/yes eslint-current-output
"#,
            count = shell_probe_path(&output_count)?,
            leader = shell_probe_path(&output_leader)?,
        ),
    )?;
    let output = run_eslint_adapter_probe(
        python,
        &output_adapter,
        &output_node,
        fake_cli,
        "fix",
        &[],
        &[target],
        project,
        temporary,
        timeout.min(Duration::from_secs(7)),
        &root.join("eslint-output-cap-evidence"),
    )?;
    let output_child = read_pid_file(&output_leader, "ESLint output-cap leader")?;
    let output_child_alive = process_survives(output_child, Duration::from_secs(1))?;
    let output_group_alive = process_group_survives(output_child, Duration::from_secs(1))?;
    if output_child_alive || output_group_alive {
        let _ = signal_process_group(output_child, "KILL");
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || output_child_alive
        || output_group_alive
        || !stderr.contains("combined output exceeded 1024 bytes")
        || !stderr.contains("native child process group survived SIGKILL")
        || stderr.contains(&target.display().to_string())
        || stderr.matches("velvet-glove-eslint:").count() != 1
    {
        return Err(format!(
            "ESLint output-cap cleanup did not compose or emitted stale JSON: status={:?}; child={output_child}:{output_child_alive}; group={output_group_alive}; stdout_bytes={}; stderr={stderr:?}",
            output.status.code(),
            output.stdout.len()
        ));
    }
    Ok(())
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn verify_eslint_adapter_signal_cleanup(
    python: &Path,
    adapter: &str,
    fake_cli: &Path,
    target: &Path,
    project: &Path,
    temporary: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let private_root_record = root.join("signal-private-root");
    let child_pid_record = root.join("signal-child-pid");
    let descendant_pid_record = root.join("signal-descendant-pid");
    let ready = root.join("signal-ready");
    let signal_node = root.join("paired-node-signal");
    write_executable_probe(
        &signal_node,
        &format!(
            r#"#!/bin/sh
set -eu
trap 'exit 0' HUP INT TERM
for argument in "$@"; do
  case $argument in --config=*) /usr/bin/dirname "${{argument#--config=}}" > '{private_root_record}' ;; esac
done
(
  trap '' HUP INT TERM
  : > '{ready}'
  while :; do :; done
) &
printf '%s\n' "$!" > '{descendant_pid_record}'
printf '%s\n' "$$" > '{child_pid_record}'
while [ ! -f '{ready}' ]; do :; done
while :; do :; done
"#,
            private_root_record = shell_probe_path(&private_root_record)?,
            ready = shell_probe_path(&ready)?,
            descendant_pid_record = shell_probe_path(&descendant_pid_record)?,
            child_pid_record = shell_probe_path(&child_pid_record)?,
        ),
    )?;
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", adapter])
        .arg(&signal_node)
        .arg(fake_cli)
        .arg("verify")
        .arg(ESLINT_FILES_MARKER)
        .arg(target)
        .current_dir(project)
        .env(TMPDIR_ENV, temporary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut outer = command
        .spawn()
        .map_err(|error| format!("spawn ESLint signal-cleanup adapter: {error}"))?;
    let outer_pid = outer.id();
    let deadline = std::time::Instant::now() + timeout.min(Duration::from_secs(5));
    while !(private_root_record.is_file()
        && child_pid_record.is_file()
        && descendant_pid_record.is_file()
        && ready.is_file())
    {
        if let Some(status) = outer
            .try_wait()
            .map_err(|error| format!("poll ESLint signal-cleanup adapter: {error}"))?
        {
            return Err(format!(
                "ESLint signal-cleanup adapter exited {status:?} before becoming ready"
            ));
        }
        if std::time::Instant::now() >= deadline {
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            return Err("ESLint signal-cleanup child did not become ready".to_owned());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let child_pid = read_pid_file(&child_pid_record, "ESLint signal child")?;
    let descendant_pid = read_pid_file(&descendant_pid_record, "ESLint signal descendant")?;
    let private_root = PathBuf::from(
        std::fs::read_to_string(&private_root_record)
            .map_err(|error| format!("read ESLint signal private root: {error}"))?
            .trim(),
    );
    let term = signal_process(outer_pid, "TERM")?;
    if !term.success() {
        let _ = signal_process_group(child_pid, "KILL");
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!("send SIGTERM to ESLint adapter: {term:?}"));
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(outer.wait_with_output());
    });
    let output = receiver
        .recv_timeout(timeout.min(Duration::from_secs(5)))
        .map_err(|error| format!("signaled ESLint adapter did not finish: {error}"))?
        .map_err(|error| format!("wait for signaled ESLint adapter: {error}"))?;
    let child_alive = process_survives(child_pid, Duration::from_secs(1))?;
    let descendant_alive = process_survives(descendant_pid, Duration::from_secs(1))?;
    let group_alive = process_group_survives(child_pid, Duration::from_secs(1))?;
    if output.status.code() != Some(2)
        || child_alive
        || descendant_alive
        || group_alive
        || private_root.exists()
        || !String::from_utf8_lossy(&output.stderr).contains("received signal 15")
    {
        let _ = signal_process_group(child_pid, "KILL");
        return Err(format!(
            "ESLint signal cleanup mismatch: status={:?} child={child_alive} descendant={descendant_alive} group={group_alive} private_exists={} stdout={:?} stderr={:?}",
            output.status.code(),
            private_root.exists(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_eslint_adapter_adversarial_lifecycle(
    _spec: &ToolSpec,
    _timeout: Duration,
) -> Result<(), String> {
    Ok(())
}

fn verify_prettier_adapter_adversarial_contract(
    spec: &ToolSpec,
    timeout: Duration,
) -> Result<(), String> {
    let phase = spec
        .phases
        .get("verify")
        .ok_or_else(|| "Prettier adversarial probe lacks a verify phase".to_owned())?;
    let [
        ArgvElement::Literal(isolated),
        ArgvElement::Literal(command),
        ArgvElement::Literal(adapter),
        ArgvElement::Literal(node_name),
        ArgvElement::Token(ArgToken::ToolExecutable),
        ArgvElement::Literal(mode),
        ArgvElement::Token(ArgToken::ExtraArgs),
        ArgvElement::Literal(marker),
        ArgvElement::Token(ArgToken::Files),
    ] = phase.argv.as_slice()
    else {
        return Err(
            "Prettier adversarial probe could not extract the evaluated adapter".to_owned(),
        );
    };
    if isolated != "-I"
        || command != "-c"
        || node_name != "node"
        || mode != "verify"
        || marker != PRETTIER_FILES_MARKER
    {
        return Err(format!(
            "Prettier adversarial probe found unexpected evaluated argv shape: {:?}",
            phase.argv
        ));
    }
    let python_program = phase
        .program
        .as_deref()
        .ok_or_else(|| "Prettier adversarial probe lacks an adapter program".to_owned())?;
    let python = resolve_program(python_program)
        .ok_or_else(|| format!("Prettier adversarial probe cannot resolve {python_program:?}"))?
        .canonicalize()
        .map_err(|error| format!("canonicalize Prettier probe Python: {error}"))?;

    let requested_root = unique_temp_dir("velvet-glove-prettier-adversarial");
    std::fs::create_dir_all(&requested_root)
        .map_err(|error| format!("create Prettier adversarial root {requested_root:?}: {error}"))?;
    let root = requested_root
        .canonicalize()
        .map_err(|error| format!("canonicalize Prettier adversarial root: {error}"))?;
    let result = (|| {
        let target = root.join("example.js");
        std::fs::write(&target, "const value = {answer: 42};\n")
            .map_err(|error| format!("write Prettier adversarial target: {error}"))?;
        let fake_cli = root.join("prettier.cjs");
        std::fs::write(
            &fake_cli,
            "throw new Error('must be launched by the paired Node');\n",
        )
        .map_err(|error| format!("write fake Prettier CLI: {error}"))?;
        let mut cli_permissions = std::fs::metadata(&fake_cli)
            .map_err(|error| format!("inspect fake Prettier CLI: {error}"))?
            .permissions();
        cli_permissions.set_mode(0o600);
        std::fs::set_permissions(&fake_cli, cli_permissions)
            .map_err(|error| format!("make fake Prettier CLI non-executable: {error}"))?;
        let probe = PrettierAdapterProbe {
            python: &python,
            adapter,
            tool: &fake_cli,
            root: &root,
            timeout,
        };

        let source_config = root.join("safe-config.json");
        std::fs::write(&source_config, "{\"singleQuote\":true}\n")
            .map_err(|error| format!("write safe Prettier config: {error}"))?;
        let captured_argv = root.join("captured-argv");
        let captured_config = root.join("captured-config");
        let captured_config_path = root.join("captured-config-path");
        let captured_mode = root.join("captured-config-mode");
        let captured_environment = root.join("captured-environment");
        let node_marker = root.join("node-ran");
        let success_node = root.join("paired-node-success");
        let success_source = format!(
            r#"#!/bin/sh
set -eu
: > '{node_marker}'
: > '{captured_argv}'
for argument in "$@"; do
  printf '%s\n' "$argument" >> '{captured_argv}'
done
[ "$1" = '{fake_cli}' ] || exit 91
config=''
for argument in "$@"; do
  case "$argument" in --config=*) config=${{argument#--config=}} ;; esac
done
[ -n "$config" ] || exit 92
printf '{{"plugins":["./executed.cjs"]}}\n' > '{source_config}'
printf '%s\n' "$config" > '{captured_config_path}'
/bin/cp "$config" '{captured_config}'
/usr/bin/stat -f '%Lp' "$config" > '{captured_mode}'
{{
  printf 'PATH=%s\n' "${{PATH-}}"
  printf 'LANG=%s\n' "${{LANG-}}"
  printf 'LC_ALL=%s\n' "${{LC_ALL-}}"
  printf 'TZ=%s\n' "${{TZ-}}"
  printf 'TERM=%s\n' "${{TERM-}}"
  printf 'CI=%s\n' "${{CI-}}"
  printf 'NODE_OPTIONS=%s\n' "${{NODE_OPTIONS-}}"
  printf 'NODE_V8_COVERAGE=%s\n' "${{NODE_V8_COVERAGE-}}"
  printf 'PRETTIER_EXPERIMENTAL_CLI=%s\n' "${{PRETTIER_EXPERIMENTAL_CLI-}}"
  printf 'PRETTIER_VELVET_GLOVE_POISON=%s\n' "${{PRETTIER_VELVET_GLOVE_POISON-}}"
  printf 'DEBUG=%s\n' "${{DEBUG-}}"
}} > '{captured_environment}'
printf 'example.js\n'
exit 1
"#,
            node_marker = shell_probe_path(&node_marker)?,
            captured_argv = shell_probe_path(&captured_argv)?,
            fake_cli = shell_probe_path(&fake_cli)?,
            source_config = shell_probe_path(&source_config)?,
            captured_config_path = shell_probe_path(&captured_config_path)?,
            captured_config = shell_probe_path(&captured_config)?,
            captured_mode = shell_probe_path(&captured_mode)?,
            captured_environment = shell_probe_path(&captured_environment)?,
        );
        write_executable_probe(&success_node, &success_source)?;
        let output = probe.run(
            &success_node,
            "verify",
            &["--config=safe-config.json", "--tab-width=4"],
            &[&target],
            "safe-config-swap",
        )?;
        if output.status.code() != Some(1)
            || !output.stdout.is_empty()
            || String::from_utf8_lossy(&output.stderr)
                != format!("prettier: formatting differs: {}\n", target.display())
        {
            return Err(format!(
                "Prettier safe-config evidence mismatch: status={:?} stdout={:?} stderr={:?}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        if std::fs::read_to_string(&source_config)
            .map_err(|error| format!("read swapped source config: {error}"))?
            != "{\"plugins\":[\"./executed.cjs\"]}\n"
        {
            return Err("fake Node did not swap the original config after validation".to_owned());
        }
        if std::fs::read_to_string(&captured_config)
            .map_err(|error| format!("read private config capture: {error}"))?
            != "{\"singleQuote\":true}\n"
        {
            return Err(
                "Prettier child did not receive the validated data-only config copy".to_owned(),
            );
        }
        if std::fs::read_to_string(&captured_mode)
            .map_err(|error| format!("read private config mode: {error}"))?
            .trim()
            != "600"
        {
            return Err("Prettier private config copy was not mode 0600".to_owned());
        }
        let private_config = PathBuf::from(
            std::fs::read_to_string(&captured_config_path)
                .map_err(|error| format!("read private config path: {error}"))?
                .trim(),
        );
        if private_config.starts_with(&root)
            || private_config.exists()
            || private_config.parent().is_some_and(Path::exists)
        {
            return Err(format!(
                "Prettier private config was inside the project or not cleaned: {private_config:?}"
            ));
        }
        let expected_argv = vec![
            fake_cli
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            format!("--config={}", private_config.display()),
            "--list-different".to_owned(),
            "--log-level=log".to_owned(),
            "--tab-width=4".to_owned(),
            "--no-editorconfig".to_owned(),
            "--ignore-path=/dev/null".to_owned(),
            "--with-node-modules".to_owned(),
            "--no-color".to_owned(),
            "--".to_owned(),
            target
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ];
        let actual_argv = std::fs::read_to_string(&captured_argv)
            .map_err(|error| format!("read paired Node argv: {error}"))?
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if actual_argv != expected_argv {
            return Err(format!(
                "paired Node argv mismatch: expected {expected_argv:?}, got {actual_argv:?}"
            ));
        }
        let expected_environment = concat!(
            "PATH=/usr/bin:/bin\n",
            "LANG=C\n",
            "LC_ALL=C\n",
            "TZ=UTC\n",
            "TERM=dumb\n",
            "CI=1\n",
            "NODE_OPTIONS=\n",
            "NODE_V8_COVERAGE=\n",
            "PRETTIER_EXPERIMENTAL_CLI=\n",
            "PRETTIER_VELVET_GLOVE_POISON=\n",
            "DEBUG=\n",
        );
        let actual_environment = std::fs::read_to_string(&captured_environment)
            .map_err(|error| format!("read Prettier child environment: {error}"))?;
        if actual_environment != expected_environment {
            return Err(format!(
                "Prettier child environment mismatch: expected {expected_environment:?}, got {actual_environment:?}"
            ));
        }

        let reject_marker = root.join("reject-node-ran");
        let reject_node = root.join("paired-node-reject-marker");
        write_executable_probe(
            &reject_node,
            &format!(
                "#!/bin/sh\nset -eu\n: > '{}'\nexit 0\n",
                shell_probe_path(&reject_marker)?
            ),
        )?;
        for (label, arguments) in [
            ("write-false", vec!["--write=false"]),
            ("check-false", vec!["--check=false"]),
            ("cache", vec!["--cache=true"]),
            ("plugin", vec!["--plugin=./executed.cjs"]),
            ("version", vec!["--version=true"]),
            ("ignore-path", vec!["--ignore-path=.prettierignore"]),
            ("cursor-offset", vec!["--cursor-offset=0"]),
        ] {
            let _ = std::fs::remove_file(&reject_marker);
            let output = probe.run(&reject_node, "verify", &arguments, &[&target], label)?;
            if output.status.code() != Some(2)
                || reject_marker.exists()
                || !String::from_utf8_lossy(&output.stderr).contains("unsupported argument")
            {
                return Err(format!(
                    "Prettier bypass {label:?} was not rejected before Node: status={:?} stderr={:?}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }

        for (label, name, body, diagnostic) in [
            (
                "executable-config",
                "prettier.config.cjs",
                "module.exports = { plugins: ['./executed.cjs'] };\n",
                "explicit config must be JSON",
            ),
            (
                "plugin-config",
                "plugin-config.json",
                "{\"plugins\":[\"./executed.cjs\"]}\n",
                "unsupported option 'plugins'",
            ),
            (
                "override-config",
                "override-config.json",
                "{\"overrides\":[{\"files\":\"*.js\",\"options\":{}}]}\n",
                "overrides are unsupported",
            ),
        ] {
            let config = root.join(name);
            std::fs::write(&config, body)
                .map_err(|error| format!("write {label} probe config: {error}"))?;
            let _ = std::fs::remove_file(&reject_marker);
            let argument = format!("--config={name}");
            let output = probe.run(
                &reject_node,
                "verify",
                &[argument.as_str()],
                &[&target],
                label,
            )?;
            if output.status.code() != Some(2)
                || reject_marker.exists()
                || !String::from_utf8_lossy(&output.stderr).contains(diagnostic)
            {
                return Err(format!(
                    "Prettier {label} was not rejected before Node: status={:?} stderr={:?}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }

        let symlink_target = root.join("symlink.js");
        std::os::unix::fs::symlink(&target, &symlink_target)
            .map_err(|error| format!("create Prettier target symlink: {error}"))?;
        let _ = std::fs::remove_file(&reject_marker);
        let output = probe.run(
            &reject_node,
            "verify",
            &[],
            &[&symlink_target],
            "symlink-target",
        )?;
        if output.status.code() != Some(2)
            || reject_marker.exists()
            || !String::from_utf8_lossy(&output.stderr).contains("traverses a symlink")
        {
            return Err("Prettier selected-path symlink was not rejected before Node".to_owned());
        }
        let hardlink_target = root.join("hardlink.js");
        std::fs::hard_link(&target, &hardlink_target)
            .map_err(|error| format!("create Prettier target hardlink: {error}"))?;
        let _ = std::fs::remove_file(&reject_marker);
        let output = probe.run(&reject_node, "verify", &[], &[&target], "hardlink-target")?;
        if output.status.code() != Some(2)
            || reject_marker.exists()
            || !String::from_utf8_lossy(&output.stderr).contains("unique regular file")
        {
            return Err("Prettier selected-path hardlink was not rejected before Node".to_owned());
        }
        std::fs::remove_file(&hardlink_target)
            .map_err(|error| format!("remove Prettier target hardlink: {error}"))?;

        std::fs::write(&source_config, "{\"singleQuote\":true}\n")
            .map_err(|error| format!("restore safe Prettier config: {error}"))?;
        let error_config_path = root.join("error-config-path");
        let error_node = root.join("paired-node-error");
        write_executable_probe(
            &error_node,
            &format!(
                r#"#!/bin/sh
set -eu
for argument in "$@"; do
  case "$argument" in
    --config=*)
      config=${{argument#--config=}}
      printf '%s\n' "$config" > '{}'
      printf 'native failure at %s\n' "$config" >&2
      ;;
  esac
done
exit 2
"#,
                shell_probe_path(&error_config_path)?
            ),
        )?;
        let output = probe.run(
            &error_node,
            "verify",
            &["--config=safe-config.json"],
            &[&target],
            "private-config-error-cleanup",
        )?;
        let error_private_config = PathBuf::from(
            std::fs::read_to_string(&error_config_path)
                .map_err(|error| format!("read error private config path: {error}"))?
                .trim(),
        );
        if output.status.code() != Some(2)
            || error_private_config.exists()
            || error_private_config.parent().is_some_and(Path::exists)
            || !String::from_utf8_lossy(&output.stderr)
                .contains("native failure at <prettier-private>/config.json")
            || String::from_utf8_lossy(&output.stderr).contains("velvet-glove-prettier-config-")
        {
            return Err(format!(
                "Prettier error path did not clean private config: status={:?} path={error_private_config:?} stderr={:?}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        verify_prettier_unwritable_temp_root_diagnostic(
            &python,
            adapter,
            &fake_cli,
            &source_config,
            &target,
            &root,
            timeout,
        )?;

        verify_prettier_mixed_parse_preflight(&python, adapter, &root, timeout)?;

        verify_prettier_normal_exit_descendant_sweep(
            &python, adapter, &fake_cli, &target, &root, timeout,
        )?;

        verify_prettier_adapter_signal_cleanup(
            &python,
            adapter,
            &fake_cli,
            &source_config,
            &target,
            &root,
            timeout,
        )?;
        verify_prettier_adapter_cleanup_cutoff(
            &python,
            adapter,
            &fake_cli,
            &source_config,
            &target,
            &root,
            timeout,
        )?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&root);
    result
}

#[cfg(not(unix))]
fn verify_prettier_adapter_adversarial_contract(
    _spec: &ToolSpec,
    _timeout: Duration,
) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
struct PrettierAdapterProbe<'a> {
    python: &'a Path,
    adapter: &'a str,
    tool: &'a Path,
    root: &'a Path,
    timeout: Duration,
}

#[cfg(unix)]
impl PrettierAdapterProbe<'_> {
    fn run(
        &self,
        node: &Path,
        phase: &str,
        extra_args: &[&str],
        targets: &[&Path],
        label: &str,
    ) -> Result<BoundedOutput, String> {
        run_prettier_adapter_probe_with_capture(
            self.python,
            self.adapter,
            node,
            self.tool,
            phase,
            extra_args,
            targets,
            self.root,
            self.timeout,
            &self.root.join(format!("capture-{label}")),
        )
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn run_prettier_adapter_probe_with_capture(
    python: &Path,
    adapter: &str,
    node: &Path,
    tool: &Path,
    phase: &str,
    extra_args: &[&str],
    targets: &[&Path],
    root: &Path,
    timeout: Duration,
    capture: &Path,
) -> Result<BoundedOutput, String> {
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", adapter])
        .arg(node)
        .arg(tool)
        .arg(phase)
        .args(extra_args)
        .arg(PRETTIER_FILES_MARKER)
        .args(targets)
        .current_dir(root)
        .env("NODE_OPTIONS", PRETTIER_POISON_ENV_VALUE)
        .env("NODE_V8_COVERAGE", PRETTIER_POISON_ENV_VALUE)
        .env("PRETTIER_EXPERIMENTAL_CLI", PRETTIER_POISON_ENV_VALUE)
        .env("PRETTIER_VELVET_GLOVE_POISON", PRETTIER_POISON_ENV_VALUE)
        .env(DEBUG_ENV, PRETTIER_POISON_ENV_VALUE);
    run_with_timeout(
        &mut command,
        &[],
        timeout.min(Duration::from_secs(10)),
        capture,
    )
    .map_err(|error| format!("run Prettier adapter probe in {root:?}: {error}"))
}

#[cfg(unix)]
fn write_executable_probe(path: &Path, source: &str) -> Result<(), String> {
    std::fs::write(path, source).map_err(|error| format!("write probe {path:?}: {error}"))?;
    let mut permissions = std::fs::metadata(path)
        .map_err(|error| format!("inspect probe {path:?}: {error}"))?
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)
        .map_err(|error| format!("make probe {path:?} executable: {error}"))
}

#[cfg(unix)]
fn shell_probe_path(path: &Path) -> Result<String, String> {
    let value = path.to_string_lossy();
    if value.contains('\'') || value.contains('\n') || value.contains('\r') {
        return Err(format!(
            "probe path cannot be safely shell-quoted: {path:?}"
        ));
    }
    Ok(value.into_owned())
}

#[cfg(unix)]
fn verify_prettier_unwritable_temp_root_diagnostic(
    python: &Path,
    adapter: &str,
    fake_cli: &Path,
    source_config: &Path,
    target: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    const TEMP_ROOT_ENTRY: &str = "        temp_root = os.path.realpath(tempfile.gettempdir())\n        temp_info = os.stat(temp_root)\n";
    if adapter.matches(TEMP_ROOT_ENTRY).count() != 1 {
        return Err(
            "Prettier unwritable-temp probe could not locate one temporary-root entry".to_owned(),
        );
    }
    let instrumented = adapter.replacen(
        TEMP_ROOT_ENTRY,
        "        temp_root = os.path.realpath(tempfile.gettempdir())\n        os.chmod(temp_root, 0o500)\n        temp_info = os.stat(temp_root)\n",
        1,
    );
    let temp_root = unique_temp_dir("velvet-glove-prettier-unwritable-temp");
    std::fs::create_dir_all(&temp_root)
        .map_err(|error| format!("create Prettier unwritable temp root: {error}"))?;
    let node_marker = root.join("unwritable-temp-node-ran");
    let node = root.join("paired-node-unwritable-temp");
    write_executable_probe(
        &node,
        &format!(
            "#!/bin/sh\nset -eu\n: > '{}'\nexit 0\n",
            shell_probe_path(&node_marker)?
        ),
    )?;
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", &instrumented])
        .arg(&node)
        .arg(fake_cli)
        .arg("verify")
        .arg(format!(
            "--config={}",
            source_config.file_name().unwrap().to_string_lossy()
        ))
        .arg(PRETTIER_FILES_MARKER)
        .arg(target)
        .current_dir(root)
        .env(TMPDIR_ENV, &temp_root);
    let output = run_with_timeout(
        &mut command,
        &[],
        timeout.min(Duration::from_secs(10)),
        &root.join("capture-unwritable-temp-root"),
    );
    let mut permissions = std::fs::metadata(&temp_root)
        .map_err(|error| format!("inspect Prettier unwritable temp root: {error}"))?
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&temp_root, permissions)
        .map_err(|error| format!("restore Prettier temp-root permissions: {error}"))?;
    let _ = std::fs::remove_dir_all(&temp_root);
    let output = output.map_err(|error| format!("run Prettier unwritable-temp probe: {error}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || node_marker.exists()
        || stderr.contains("velvet-glove-prettier-config-")
        || stderr != "velvet-glove-prettier: [Errno 13] Permission denied: '<prettier-private>'\n"
    {
        return Err(format!(
            "Prettier unwritable temp-root diagnostic mismatch: status={:?} node_ran={} stdout={:?} stderr={stderr:?}",
            output.status.code(),
            node_marker.exists(),
            String::from_utf8_lossy(&output.stdout)
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn verify_prettier_mixed_parse_preflight(
    python: &Path,
    adapter: &str,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let managed = PrettierToolchain::resolve_if_configured()?;
    let real_node = if let Some(toolchain) = &managed {
        toolchain.node.clone()
    } else {
        resolve_program("node")
            .ok_or_else(|| "mixed-parse Prettier probe cannot resolve Node".to_owned())?
            .canonicalize()
            .map_err(|error| format!("canonicalize mixed-parse Node: {error}"))?
    };
    let real_cli = if let Some(toolchain) = &managed {
        toolchain.cli.clone()
    } else {
        resolve_prettier_fixture_cli()?
    };
    let mut version = Command::new(&real_node);
    version.arg(&real_cli).arg("--version").current_dir(root);
    let version_output = run_with_timeout(
        &mut version,
        &[],
        timeout.min(Duration::from_secs(10)),
        &root.join("capture-mixed-parse-version"),
    )
    .map_err(|error| format!("probe exact Prettier version: {error}"))?;
    if version_output.status.code() != Some(0)
        || version_output.stdout != b"3.9.6\n"
        || !version_output.stderr.is_empty()
    {
        return Err(format!(
            "mixed-parse regression requires exact Prettier 3.9.6: status={:?} stdout={:?} stderr={:?}",
            version_output.status.code(),
            String::from_utf8_lossy(&version_output.stdout),
            String::from_utf8_lossy(&version_output.stderr)
        ));
    }

    let project = root.join("mixed-parse-project");
    std::fs::create_dir_all(&project)
        .map_err(|error| format!("create mixed-parse project: {error}"))?;
    let dirty = project.join("a-dirty.js");
    let broken = project.join("z-broken.js");
    std::fs::write(&dirty, "const dirty={answer:42}\n")
        .map_err(|error| format!("write dirty valid Prettier input: {error}"))?;
    std::fs::write(&broken, "const broken = ;\n")
        .map_err(|error| format!("write parse-invalid Prettier input: {error}"))?;
    let before = TreeSnapshot::read(&project)?;

    let invocation_log = root.join("mixed-parse-invocations");
    let write_marker = root.join("mixed-parse-write-ran");
    let traced_node = root.join("mixed-parse-node");
    write_executable_probe(
        &traced_node,
        &format!(
            r#"#!/bin/sh
set -eu
{{
  printf 'BEGIN\n'
  for argument in "$@"; do
    printf '%s\n' "$argument"
    if [ "$argument" = "--write" ]; then
      : > '{write_marker}'
    fi
  done
}} >> '{invocation_log}'
exec '{real_node}' "$@"
"#,
            write_marker = shell_probe_path(&write_marker)?,
            invocation_log = shell_probe_path(&invocation_log)?,
            real_node = shell_probe_path(&real_node)?,
        ),
    )?;
    let output = run_prettier_adapter_probe_with_capture(
        python,
        adapter,
        &traced_node,
        &real_cli,
        "format",
        &[],
        &[&dirty, &broken],
        &project,
        timeout,
        &root.join("capture-mixed-parse-format"),
    )?;
    let after = TreeSnapshot::read(&project)?;
    let recorded = std::fs::read_to_string(&invocation_log)
        .map_err(|error| format!("read mixed-parse invocation trace: {error}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() != Some(2)
        || before != after
        || recorded.lines().filter(|line| *line == "BEGIN").count() != 1
        || !recorded.lines().any(|line| line == "--list-different")
        || recorded.lines().any(|line| line == "--write")
        || write_marker.exists()
        || !stderr.contains(
            "native Prettier format preflight exited 2 without valid list-different evidence",
        )
    {
        return Err(format!(
            "mixed dirty/parse-invalid preflight was not read-only and fail-closed: status={:?} diff={} trace={recorded:?} write={} stdout={:?} stderr={stderr:?}",
            output.status.code(),
            before.diff(&after).describe(),
            write_marker.exists(),
            String::from_utf8_lossy(&output.stdout),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn verify_prettier_normal_exit_descendant_sweep(
    python: &Path,
    adapter: &str,
    fake_cli: &Path,
    target: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let child_pid_record = root.join("normal-exit-child-pid");
    let descendant_pid_record = root.join("normal-exit-descendant-pid");
    let descendant_ready = root.join("normal-exit-descendant-ready");
    let descendant_node = root.join("paired-node-normal-exit-descendant");
    write_executable_probe(
        &descendant_node,
        &format!(
            r#"#!/bin/sh
set -eu
(
  exec >/dev/null 2>&1
  trap '' HUP INT TERM
  : > '{descendant_ready}'
  while :; do :; done
) &
descendant=$!
while [ ! -f '{descendant_ready}' ]; do :; done
printf '%s\n' "$$" > '{child_pid_record}'
printf '%s\n' "$descendant" > '{descendant_pid_record}'
exit 0
"#,
            descendant_ready = shell_probe_path(&descendant_ready)?,
            child_pid_record = shell_probe_path(&child_pid_record)?,
            descendant_pid_record = shell_probe_path(&descendant_pid_record)?,
        ),
    )?;
    let output = PrettierAdapterProbe {
        python,
        adapter,
        tool: fake_cli,
        root,
        timeout,
    }
    .run(
        &descendant_node,
        "verify",
        &[],
        &[target],
        "normal-exit-descendant",
    )?;
    let child_pid = read_pid_file(&child_pid_record, "Prettier normal-exit child")?;
    let descendant_pid = read_pid_file(&descendant_pid_record, "Prettier normal-exit descendant")?;
    let child_alive = process_survives(child_pid, Duration::from_secs(1))?;
    let descendant_alive = process_survives(descendant_pid, Duration::from_secs(1))?;
    let group_alive = process_group_survives(child_pid, Duration::from_secs(1))?;
    if child_alive || descendant_alive || group_alive {
        let _ = signal_process_group(child_pid, "KILL");
        return Err(format!(
            "Prettier normal-exit sweep leaked processes: child={child_alive} descendant={descendant_alive} group={group_alive}"
        ));
    }
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || String::from_utf8_lossy(&output.stderr)
            != "velvet-glove-prettier: native Prettier left same-group descendants after child exit\n"
    {
        return Err(format!(
            "Prettier normal-exit descendant sweep mismatch: status={:?} stdout={:?} stderr={:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn verify_prettier_adapter_signal_cleanup(
    python: &Path,
    adapter: &str,
    fake_cli: &Path,
    source_config: &Path,
    target: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let private_config_record = root.join("signal-private-config");
    let child_pid_record = root.join("signal-child-pid");
    let descendant_pid_record = root.join("signal-descendant-pid");
    let descendant_ready = root.join("signal-descendant-ready");
    let signal_node = root.join("paired-node-signal");
    let source = format!(
        r#"#!/bin/sh
set -eu
trap 'exit 0' HUP INT TERM
for argument in "$@"; do
  case "$argument" in --config=*) printf '%s\n' "${{argument#--config=}}" > '{private_config_record}' ;; esac
done
(
  trap '' HUP INT TERM
  : > '{descendant_ready}'
  while :; do :; done
) &
while [ ! -f '{descendant_ready}' ]; do :; done
printf '%s\n' "$!" > '{descendant_pid_record}'
printf '%s\n' "$$" > '{child_pid_record}'
while :; do :; done
"#,
        private_config_record = shell_probe_path(&private_config_record)?,
        descendant_ready = shell_probe_path(&descendant_ready)?,
        descendant_pid_record = shell_probe_path(&descendant_pid_record)?,
        child_pid_record = shell_probe_path(&child_pid_record)?,
    );
    write_executable_probe(&signal_node, &source)?;
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", adapter])
        .arg(&signal_node)
        .arg(fake_cli)
        .arg("verify")
        .arg(format!(
            "--config={}",
            source_config.file_name().unwrap().to_string_lossy()
        ))
        .arg(PRETTIER_FILES_MARKER)
        .arg(target)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut outer = command
        .spawn()
        .map_err(|error| format!("spawn Prettier signal-cleanup adapter: {error}"))?;
    let outer_pid = outer.id();
    let startup_timeout = timeout.min(Duration::from_secs(5));
    let deadline = std::time::Instant::now() + startup_timeout;
    while !(private_config_record.is_file()
        && child_pid_record.is_file()
        && descendant_pid_record.is_file())
    {
        if let Some(status) = outer
            .try_wait()
            .map_err(|error| format!("poll Prettier signal-cleanup adapter: {error}"))?
        {
            return Err(format!(
                "Prettier signal-cleanup adapter exited {status:?} before becoming ready"
            ));
        }
        if std::time::Instant::now() >= deadline {
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            return Err(format!(
                "Prettier signal-cleanup child did not become ready within {startup_timeout:?}"
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let parse_pid = |path: &Path, label: &str| -> Result<u32, String> {
        std::fs::read_to_string(path)
            .map_err(|error| format!("read Prettier {label} PID: {error}"))?
            .trim()
            .parse::<u32>()
            .map_err(|error| format!("parse Prettier {label} PID: {error}"))
    };
    let child_pid = parse_pid(&child_pid_record, "child")?;
    let descendant_pid = parse_pid(&descendant_pid_record, "descendant")?;
    let private_config = PathBuf::from(
        std::fs::read_to_string(&private_config_record)
            .map_err(|error| format!("read Prettier signal private config: {error}"))?
            .trim(),
    );
    let term = signal_process(outer_pid, "TERM")?;
    if !term.success() {
        let _ = signal_process_group(child_pid, "KILL");
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!(
            "send SIGTERM to Prettier adapter {outer_pid}: {term:?}"
        ));
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(outer.wait_with_output());
    });
    let output = match receiver.recv_timeout(timeout.min(Duration::from_secs(5))) {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            let _ = signal_process_group(child_pid, "KILL");
            return Err(format!("wait for signaled Prettier adapter: {error}"));
        }
        Err(error) => {
            let _ = signal_process_group(child_pid, "KILL");
            let _ = signal_process(outer_pid, "KILL");
            return Err(format!("signaled Prettier adapter did not finish: {error}"));
        }
    };
    let child_alive = process_survives(child_pid, Duration::from_secs(1))?;
    let descendant_alive = process_survives(descendant_pid, Duration::from_secs(1))?;
    let group_alive = process_group_survives(child_pid, Duration::from_secs(1))?;
    if child_alive || descendant_alive || group_alive {
        let _ = signal_process_group(child_pid, "KILL");
        return Err(format!(
            "Prettier signal cleanup leaked processes: child={child_alive} descendant={descendant_alive} group={group_alive}"
        ));
    }
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || String::from_utf8_lossy(&output.stderr) != "velvet-glove-prettier: received signal 15\n"
        || private_config.exists()
        || private_config.parent().is_some_and(Path::exists)
    {
        return Err(format!(
            "Prettier signal cleanup mismatch: status={:?} stdout={:?} stderr={:?} private={private_config:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn verify_prettier_adapter_cleanup_cutoff(
    python: &Path,
    adapter: &str,
    fake_cli: &Path,
    source_config: &Path,
    target: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    const CLEANUP_READY_ENV: &str = "VELVET_GLOVE_PRETTIER_CLEANUP_PROBE_READY";
    const CLEANUP_RELEASE_ENV: &str = "VELVET_GLOVE_PRETTIER_CLEANUP_PROBE_RELEASE";
    const CLEANUP_ENTRY: &str = "finally:\n    cleaning = True\n    blocked_mask = None\n";
    if adapter.matches(CLEANUP_ENTRY).count() != 1 {
        return Err("Prettier cleanup-cutoff probe could not locate one cleanup entry".to_owned());
    }
    let instrumented_entry = format!(
        "finally:\n    cleaning = True\n    cleanup_probe_ready = os.environ.get({CLEANUP_READY_ENV:?})\n    cleanup_probe_release = os.environ.get({CLEANUP_RELEASE_ENV:?})\n    if cleanup_probe_ready is not None and cleanup_probe_release is not None:\n        with open(cleanup_probe_ready, \"wb\"):\n            pass\n        while not os.path.exists(cleanup_probe_release):\n            time.sleep(0.01)\n    blocked_mask = None\n"
    );
    let instrumented = adapter.replacen(CLEANUP_ENTRY, &instrumented_entry, 1);

    let private_config_record = root.join("cleanup-cutoff-private-config");
    let cleanup_ready = root.join("cleanup-cutoff-ready");
    let cleanup_release = root.join("cleanup-cutoff-release");
    let cleanup_node = root.join("paired-node-cleanup-cutoff");
    write_executable_probe(
        &cleanup_node,
        &format!(
            r#"#!/bin/sh
set -eu
for argument in "$@"; do
  case "$argument" in --config=*) printf '%s\n' "${{argument#--config=}}" > '{}' ;; esac
done
exit 0
"#,
            shell_probe_path(&private_config_record)?
        ),
    )?;

    let mut command = Command::new(python);
    command
        .args(["-I", "-c", &instrumented])
        .arg(&cleanup_node)
        .arg(fake_cli)
        .arg("verify")
        .arg(format!(
            "--config={}",
            source_config.file_name().unwrap().to_string_lossy()
        ))
        .arg(PRETTIER_FILES_MARKER)
        .arg(target)
        .current_dir(root)
        .env(CLEANUP_READY_ENV, &cleanup_ready)
        .env(CLEANUP_RELEASE_ENV, &cleanup_release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut outer = command
        .spawn()
        .map_err(|error| format!("spawn Prettier cleanup-cutoff adapter: {error}"))?;
    let outer_pid = outer.id();
    let startup_timeout = timeout.min(Duration::from_secs(5));
    let deadline = std::time::Instant::now() + startup_timeout;
    while !(cleanup_ready.is_file() && private_config_record.is_file()) {
        if let Some(status) = outer
            .try_wait()
            .map_err(|error| format!("poll Prettier cleanup-cutoff adapter: {error}"))?
        {
            return Err(format!(
                "Prettier cleanup-cutoff adapter exited {status:?} before the cleanup barrier"
            ));
        }
        if std::time::Instant::now() >= deadline {
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            return Err(format!(
                "Prettier cleanup-cutoff adapter did not reach its barrier within {startup_timeout:?}"
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let private_config = PathBuf::from(
        std::fs::read_to_string(&private_config_record)
            .map_err(|error| format!("read cleanup-cutoff private config: {error}"))?
            .trim(),
    );
    let term = signal_process(outer_pid, "TERM")?;
    if !term.success() {
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!(
            "send cleanup-window SIGTERM to Prettier adapter {outer_pid}: {term:?}"
        ));
    }
    std::fs::write(&cleanup_release, b"release\n")
        .map_err(|error| format!("release Prettier cleanup-cutoff barrier: {error}"))?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(outer.wait_with_output());
    });
    let output = match receiver.recv_timeout(timeout.min(Duration::from_secs(5))) {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            return Err(format!("wait for Prettier cleanup-cutoff adapter: {error}"));
        }
        Err(error) => {
            let _ = signal_process(outer_pid, "KILL");
            return Err(format!(
                "Prettier cleanup-cutoff adapter did not finish: {error}"
            ));
        }
    };
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || String::from_utf8_lossy(&output.stderr) != "velvet-glove-prettier: received signal 15\n"
        || private_config.exists()
        || private_config.parent().is_some_and(Path::exists)
    {
        return Err(format!(
            "Prettier cleanup cutoff mismatch: status={:?} stdout={:?} stderr={:?} private={private_config:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

fn verify_betterleaks_adapter_lifecycle(spec: &ToolSpec, timeout: Duration) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (spec, timeout);
        return Ok(());
    }
    #[cfg(unix)]
    {
        const CHILD_PID_ENV: &str = "VELVET_GLOVE_BETTERLEAKS_LIFECYCLE_CHILD_PID";
        let phase = spec
            .phases
            .get("verify")
            .ok_or_else(|| "betterleaks lifecycle probe lacks a verify phase".to_owned())?;
        let [
            ArgvElement::Literal(isolated),
            ArgvElement::Literal(command),
            ArgvElement::Literal(adapter),
            ..,
        ] = phase.argv.as_slice()
        else {
            return Err(
                "betterleaks lifecycle probe could not extract the evaluated adapter".to_owned(),
            );
        };
        if isolated != "-I" || command != "-c" {
            return Err(format!(
                "betterleaks lifecycle probe expected isolated Python -I -c, got {isolated:?} {command:?}"
            ));
        }
        let python_program = phase
            .program
            .as_deref()
            .ok_or_else(|| "betterleaks lifecycle probe lacks an adapter program".to_owned())?;
        let python = resolve_program(python_program).ok_or_else(|| {
            format!("betterleaks lifecycle probe cannot resolve {python_program:?}")
        })?;
        let python = python
            .canonicalize()
            .map_err(|error| format!("canonicalize lifecycle Python {python:?}: {error}"))?;

        let root = unique_temp_dir("velvet-glove-betterleaks-lifecycle");
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("create Betterleaks lifecycle root {root:?}: {error}"))?;
        let spaced_program_dir = root.join("state root with spaces");
        std::fs::create_dir(&spaced_program_dir).map_err(|error| {
            format!("create spaced Betterleaks lifecycle program root: {error}")
        })?;
        let lifecycle_python = spaced_program_dir.join("python");
        std::os::unix::fs::symlink(&python, &lifecycle_python).map_err(|error| {
            format!("link spaced Betterleaks lifecycle Python {lifecycle_python:?}: {error}")
        })?;
        let fake_tool = root.join("betterleaks-fake");
        let child_pid_path = root.join("child.pid");
        let target = root.join("selected.txt");
        std::fs::write(&target, "lifecycle probe\n")
            .map_err(|error| format!("write Betterleaks lifecycle target {target:?}: {error}"))?;
        let fake_source = format!(
            r#"#!/bin/sh
set -eu
trap 'exit 0' HUP INT TERM
printf 'ready\n'
printf '%s\n' "$$" > "${CHILD_PID_ENV}"
while :; do
    :
done
"#
        );
        std::fs::write(&fake_tool, fake_source)
            .map_err(|error| format!("write Betterleaks lifecycle fake {fake_tool:?}: {error}"))?;
        let mut permissions = std::fs::metadata(&fake_tool)
            .map_err(|error| format!("stat Betterleaks lifecycle fake {fake_tool:?}: {error}"))?
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&fake_tool, permissions)
            .map_err(|error| format!("make Betterleaks lifecycle fake executable: {error}"))?;

        let mut command = Command::new(&lifecycle_python);
        command
            .args(["-I", "-c", adapter])
            .arg(&fake_tool)
            .arg(BETTERLEAKS_FILES_MARKER)
            .arg(&target)
            .current_dir(&root)
            .env(CHILD_PID_ENV, &child_pid_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut outer = command
            .spawn()
            .map_err(|error| format!("spawn evaluated Betterleaks lifecycle adapter: {error}"))?;
        let outer_pid = outer.id();
        let startup_timeout = timeout.min(Duration::from_secs(5));
        let startup_deadline = std::time::Instant::now() + startup_timeout;
        let child_pid = loop {
            if let Ok(value) = std::fs::read_to_string(&child_pid_path) {
                match value.trim().parse::<u32>() {
                    Ok(pid) => break pid,
                    Err(error) => {
                        let _ = signal_process(outer_pid, "KILL");
                        let _ = outer.wait();
                        let _ = std::fs::remove_dir_all(&root);
                        return Err(format!(
                            "parse Betterleaks lifecycle child PID {value:?}: {error}"
                        ));
                    }
                }
            }
            if let Some(status) = outer
                .try_wait()
                .map_err(|error| format!("poll Betterleaks lifecycle adapter: {error}"))?
            {
                let _ = std::fs::remove_dir_all(&root);
                return Err(format!(
                    "Betterleaks lifecycle adapter exited {status:?} before its child became ready"
                ));
            }
            if std::time::Instant::now() >= startup_deadline {
                let _ = signal_process(outer_pid, "KILL");
                let _ = outer.wait();
                let _ = std::fs::remove_dir_all(&root);
                return Err(format!(
                    "Betterleaks lifecycle child did not become ready within {startup_timeout:?}"
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        };

        let term = signal_process(outer_pid, "TERM")?;
        if !term.success() {
            let _ = signal_process(child_pid, "KILL");
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            let _ = std::fs::remove_dir_all(&root);
            return Err(format!(
                "send SIGTERM to Betterleaks lifecycle adapter {outer_pid}: {term:?}"
            ));
        }

        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(outer.wait_with_output());
        });
        let completion_timeout = timeout.min(Duration::from_secs(5));
        let output = match receiver.recv_timeout(completion_timeout) {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                let _ = signal_process(child_pid, "KILL");
                let _ = std::fs::remove_dir_all(&root);
                return Err(format!(
                    "wait for terminated Betterleaks lifecycle adapter: {error}"
                ));
            }
            Err(error) => {
                let _ = signal_process(child_pid, "KILL");
                let _ = signal_process(outer_pid, "KILL");
                let _ = receiver.recv_timeout(Duration::from_secs(2));
                let _ = std::fs::remove_dir_all(&root);
                return Err(format!(
                    "Betterleaks lifecycle adapter or inherited stdout pipe remained open for {completion_timeout:?}: {error}"
                ));
            }
        };
        let child_alive = signal_process(child_pid, "0")?.success();
        if child_alive {
            let _ = signal_process(child_pid, "KILL");
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let result = if output.status.code() != Some(2) {
            Err(format!(
                "SIGTERM Betterleaks lifecycle adapter exited {:?}, expected status 2; stdout={stdout:?}; stderr={stderr:?}",
                output.status.code()
            ))
        } else if child_alive {
            Err(format!(
                "SIGTERM Betterleaks lifecycle adapter left child {child_pid} alive"
            ))
        } else if stdout != "ready\n" {
            Err(format!(
                "Betterleaks lifecycle child stdout was not drained exactly: {stdout:?}"
            ))
        } else if !stderr.is_empty() {
            Err(format!(
                "Betterleaks lifecycle adapter emitted unexpected stderr: {stderr:?}"
            ))
        } else {
            Ok(())
        };
        let _ = std::fs::remove_dir_all(&root);
        result
    }
}

fn verify_contextlint_adapter_noop_resistance(
    spec: &ToolSpec,
    timeout: Duration,
) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (spec, timeout);
        return Ok(());
    }
    #[cfg(unix)]
    {
        let phase = spec
            .phases
            .get("verify")
            .ok_or_else(|| "Contextlint adversarial probe lacks a verify phase".to_owned())?;
        let [
            ArgvElement::Literal(isolated),
            ArgvElement::Literal(command),
            ArgvElement::Literal(adapter),
            ArgvElement::Literal(node),
            ArgvElement::Token(ArgToken::ToolExecutable),
            ArgvElement::Token(ArgToken::WorkspaceIndicator),
            ArgvElement::Token(ArgToken::ExtraArgs),
            ArgvElement::Literal(marker),
            ArgvElement::Token(ArgToken::Files),
        ] = phase.argv.as_slice()
        else {
            return Err(
                "Contextlint adversarial probe could not extract the evaluated adapter".to_owned(),
            );
        };
        if isolated != "-I"
            || command != "-c"
            || node != "node"
            || marker != CONTEXTLINT_FILES_MARKER
        {
            return Err(format!(
                "Contextlint adversarial probe observed a drifted adapter prefix/marker: {:?}",
                phase.argv
            ));
        }
        let python_program = phase
            .program
            .as_deref()
            .ok_or_else(|| "Contextlint adversarial probe lacks Python".to_owned())?;
        let python = resolve_program(python_program)
            .or_else(|| {
                (python_program == "python")
                    .then(|| resolve_program("python3"))
                    .flatten()
            })
            .ok_or_else(|| format!("cannot resolve {python_program:?} for Contextlint probe"))?
            .canonicalize()
            .map_err(|error| {
                format!("canonicalize Contextlint probe Python {python_program:?}: {error}")
            })?;

        let root = unique_temp_dir("velvet-glove-contextlint-noop-resistance");
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("create Contextlint probe root {root:?}: {error}"))?;
        let root = root
            .canonicalize()
            .map_err(|error| format!("canonicalize Contextlint probe root: {error}"))?;
        let result = (|| {
            let workspace = root.join("workspace");
            let temporary = root.join("tmp");
            let graph = root.join("node_modules");
            let cli_directory = graph.join("@contextlint/cli/dist");
            let core_directory = graph.join("@contextlint/core");
            std::fs::create_dir_all(&workspace)
                .map_err(|error| format!("create Contextlint probe workspace: {error}"))?;
            std::fs::create_dir_all(&temporary)
                .map_err(|error| format!("create Contextlint probe TMPDIR: {error}"))?;
            std::fs::create_dir_all(&cli_directory)
                .map_err(|error| format!("create Contextlint probe CLI graph: {error}"))?;
            std::fs::create_dir_all(&core_directory)
                .map_err(|error| format!("create Contextlint probe core graph: {error}"))?;

            let cli_entry = cli_directory.join("index.js");
            std::fs::write(&cli_entry, "#!/usr/bin/env node\n")
                .map_err(|error| format!("write Contextlint probe CLI entry: {error}"))?;
            let mut cli_permissions = std::fs::metadata(&cli_entry)
                .map_err(|error| format!("stat Contextlint probe CLI entry: {error}"))?
                .permissions();
            cli_permissions.set_mode(0o700);
            std::fs::set_permissions(&cli_entry, cli_permissions)
                .map_err(|error| format!("make Contextlint probe CLI executable: {error}"))?;
            std::fs::write(
                graph.join("@contextlint/cli/package.json"),
                r#"{"name":"@contextlint/cli","version":"1.1.1","type":"module","bin":{"contextlint":"dist/index.js"},"dependencies":{"@contextlint/core":"1.1.1"}}
"#,
            )
            .map_err(|error| format!("write Contextlint probe CLI manifest: {error}"))?;
            std::fs::write(
                core_directory.join("package.json"),
                r#"{"name":"@contextlint/core","version":"1.1.1","type":"module"}
"#,
            )
            .map_err(|error| format!("write Contextlint probe core manifest: {error}"))?;

            let fake_node = root.join("node");
            std::fs::write(
                &fake_node,
                "#!/bin/sh\nset -eu\n: > \"$0.invoked\"\nexit 70\n",
            )
            .map_err(|error| format!("write Contextlint probe fake Node: {error}"))?;
            let mut node_permissions = std::fs::metadata(&fake_node)
                .map_err(|error| format!("stat Contextlint probe fake Node: {error}"))?
                .permissions();
            node_permissions.set_mode(0o700);
            std::fs::set_permissions(&fake_node, node_permissions)
                .map_err(|error| format!("make Contextlint probe fake Node executable: {error}"))?;

            let config = workspace.join("contextlint.config.json");
            let document = workspace.join("example.md");
            let config_bytes = b"{\"include\":[\"does-not-exist/**/*.md\"],\"rules\":[]}\n";
            let document_bytes = b"# Physical Markdown sentinel\n";
            std::fs::write(&config, config_bytes)
                .map_err(|error| format!("write zero-rule Contextlint config: {error}"))?;
            std::fs::write(&document, document_bytes)
                .map_err(|error| format!("write Contextlint sentinel document: {error}"))?;

            let capture = root.join("capture");
            let mut invocation = Command::new(&python);
            invocation
                .args(["-I", "-c", adapter])
                .arg(&fake_node)
                .arg(&cli_entry)
                .arg(&config)
                .arg(CONTEXTLINT_FILES_MARKER)
                .arg(&document)
                .current_dir(&workspace)
                .env_clear()
                .env(PATH_ENV, CONTEXTLINT_CHILD_PATH)
                .env(TMPDIR_ENV, &temporary);
            let output = run_with_timeout(&mut invocation, &[], timeout, &capture)
                .map_err(|error| format!("run zero-rule Contextlint probe: {error}"))?;
            if output.status.code() != Some(2) {
                return Err(format!(
                    "zero-rule Contextlint config must fail with status 2, got {:?}\nstdout:\n{}\nstderr:\n{}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                ));
            }
            if !output.stdout.is_empty() {
                return Err(format!(
                    "zero-rule Contextlint probe emitted stdout: {:?}",
                    String::from_utf8_lossy(&output.stdout)
                ));
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("must declare at least one rule") {
                return Err(format!(
                    "zero-rule Contextlint probe lacked stable rejection: {stderr:?}"
                ));
            }
            if fake_node.with_file_name("node.invoked").exists() {
                return Err(
                    "zero-rule Contextlint config invoked Node before structural rejection"
                        .to_owned(),
                );
            }
            if sorted_entries(&temporary)?.into_iter().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(CONTEXTLINT_PRIVATE_ROOT_PREFIX))
            }) {
                return Err(
                    "zero-rule Contextlint rejection left a private probe directory".to_owned(),
                );
            }
            if std::fs::read(&config).ok().as_deref() != Some(config_bytes)
                || std::fs::read(&document).ok().as_deref() != Some(document_bytes)
            {
                return Err("zero-rule Contextlint probe mutated project inputs".to_owned());
            }

            let valid_config =
                b"{\"rules\":[{\"rule\":\"sec001\",\"options\":{\"sections\":[\"Required\"]}}]}\n";
            std::fs::write(&config, valid_config)
                .map_err(|error| format!("write glob-magic Contextlint config: {error}"))?;
            let ignored_non_markdown = workspace.join("ignored[glob-magic].txt");
            std::fs::write(&ignored_non_markdown, b"outside Markdown inventory\n").map_err(
                |error| format!("write Contextlint non-Markdown magic sentinel: {error}"),
            )?;
            let non_markdown_node_marker = root.join("contextlint-non-markdown-node-ran");
            write_executable_probe(
                &fake_node,
                &contextlint_fake_node_source(&format!(
                    ": > '{}'\nprintf '%s\\n' '[]'\nexit 0\n",
                    shell_probe_path(&non_markdown_node_marker)?,
                )),
            )?;
            let mut invocation = Command::new(&python);
            invocation
                .args(["-I", "-c", adapter])
                .arg(&fake_node)
                .arg(&cli_entry)
                .arg(&config)
                .arg(CONTEXTLINT_FILES_MARKER)
                .arg(&document)
                .current_dir(&workspace)
                .env_clear()
                .env(PATH_ENV, CONTEXTLINT_CHILD_PATH)
                .env(TMPDIR_ENV, &temporary);
            let output = run_with_timeout(
                &mut invocation,
                &[],
                timeout,
                &root.join("capture-contextlint-non-markdown-glob-magic"),
            )
            .map_err(|error| format!("run Contextlint non-Markdown magic probe: {error}"))?;
            if output.status.code() != Some(0)
                || output.stdout != b"[]\n"
                || !output.stderr.is_empty()
                || !non_markdown_node_marker.is_file()
            {
                return Err(format!(
                    "Contextlint non-Markdown glob-magic sentinel mismatch: status={:?} node_ran={} stdout={:?} stderr={:?}",
                    output.status.code(),
                    non_markdown_node_marker.is_file(),
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                ));
            }
            if sorted_entries(&temporary)?.into_iter().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(CONTEXTLINT_PRIVATE_ROOT_PREFIX))
            }) {
                return Err(
                    "Contextlint non-Markdown glob-magic success left a private probe directory"
                        .to_owned(),
                );
            }
            write_executable_probe(
                &fake_node,
                "#!/bin/sh\nset -eu\n: > \"$0.invoked\"\nexit 70\n",
            )?;
            for (label, file_name, expected_magic) in [
                ("star", "literal*.md", "'*'"),
                ("question", "literal?.md", "'?'"),
                ("class", "[x].md", "'[]'"),
                ("brace", "{x,y}.md", "'{}'"),
                ("extglob", "+(x).md", "'()'"),
                ("escape", r"literal\name.md", r"'\\'"),
            ] {
                let magic_document = workspace.join(file_name);
                std::fs::write(&magic_document, b"# Required\n\nPresent.\n").map_err(|error| {
                    format!("write Contextlint {label} glob-magic document: {error}")
                })?;
                let mut invocation = Command::new(&python);
                invocation
                    .args(["-I", "-c", adapter])
                    .arg(&fake_node)
                    .arg(&cli_entry)
                    .arg(&config)
                    .arg(CONTEXTLINT_FILES_MARKER)
                    .arg(&magic_document)
                    .current_dir(&workspace)
                    .env_clear()
                    .env(PATH_ENV, CONTEXTLINT_CHILD_PATH)
                    .env(TMPDIR_ENV, &temporary);
                let output = run_with_timeout(
                    &mut invocation,
                    &[],
                    timeout,
                    &root.join(format!("capture-contextlint-glob-magic-{label}")),
                )
                .map_err(|error| format!("run Contextlint {label} glob-magic probe: {error}"))?;
                let stderr = String::from_utf8_lossy(&output.stderr);
                let expected_suffix = format!(": <workspace>/{file_name}\n");
                if output.status.code() != Some(2)
                    || !output.stdout.is_empty()
                    || !stderr.starts_with(
                        "velvet-glove-contextlint: workspace inventory path contains Contextlint glob magic ",
                    )
                    || !stderr.contains(expected_magic)
                    || !stderr.ends_with(&expected_suffix)
                    || fake_node.with_file_name("node.invoked").exists()
                {
                    return Err(format!(
                        "Contextlint {label} glob-magic no-op resistance mismatch: status={:?} node_ran={} stdout={:?} stderr={stderr:?}",
                        output.status.code(),
                        fake_node.with_file_name("node.invoked").exists(),
                        String::from_utf8_lossy(&output.stdout),
                    ));
                }
                if sorted_entries(&temporary)?.into_iter().any(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with(CONTEXTLINT_PRIVATE_ROOT_PREFIX))
                }) {
                    return Err(format!(
                        "Contextlint {label} glob-magic rejection left a private probe directory"
                    ));
                }
                std::fs::remove_file(&magic_document).map_err(|error| {
                    format!("remove Contextlint {label} glob-magic document: {error}")
                })?;
            }
            let magic_workspace = root.join("ancestor[magic]");
            std::fs::create_dir(&magic_workspace)
                .map_err(|error| format!("create Contextlint magic ancestor: {error}"))?;
            let magic_config = magic_workspace.join("contextlint.config.json");
            let magic_document = magic_workspace.join("example.md");
            std::fs::write(&magic_config, valid_config)
                .map_err(|error| format!("write Contextlint magic-ancestor config: {error}"))?;
            std::fs::write(&magic_document, b"# Required\n\nPresent.\n")
                .map_err(|error| format!("write Contextlint magic-ancestor document: {error}"))?;
            let mut invocation = Command::new(&python);
            invocation
                .args(["-I", "-c", adapter])
                .arg(&fake_node)
                .arg(&cli_entry)
                .arg(&magic_config)
                .arg(CONTEXTLINT_FILES_MARKER)
                .arg(&magic_document)
                .current_dir(&magic_workspace)
                .env_clear()
                .env(PATH_ENV, CONTEXTLINT_CHILD_PATH)
                .env(TMPDIR_ENV, &temporary);
            let output = run_with_timeout(
                &mut invocation,
                &[],
                timeout,
                &root.join("capture-contextlint-glob-magic-ancestor"),
            )
            .map_err(|error| format!("run Contextlint magic-ancestor probe: {error}"))?;
            if output.status.code() != Some(2)
                || !output.stdout.is_empty()
                || String::from_utf8_lossy(&output.stderr)
                    != "velvet-glove-contextlint: workspace indicator contains Contextlint glob magic '[]': <workspace>/contextlint.config.json\n"
                || fake_node.with_file_name("node.invoked").exists()
            {
                return Err(format!(
                    "Contextlint magic-ancestor mismatch: status={:?} node_ran={} stdout={:?} stderr={:?}",
                    output.status.code(),
                    fake_node.with_file_name("node.invoked").exists(),
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                ));
            }
            let external = root.join("excluded-symlink-external");
            std::fs::create_dir(&external)
                .map_err(|error| format!("create Contextlint symlink target: {error}"))?;
            std::fs::write(external.join("external.md"), b"# External\n")
                .map_err(|error| format!("write Contextlint symlink target: {error}"))?;
            for excluded_name in [".git", "node_modules", ".velvet-glove"] {
                let excluded_link = workspace.join(excluded_name);
                std::os::unix::fs::symlink(&external, &excluded_link).map_err(|error| {
                    format!("create Contextlint excluded-name symlink {excluded_name}: {error}")
                })?;
                std::fs::write(
                    &document,
                    format!("# Required\n\n[external]({excluded_name}/external.md)\n"),
                )
                .map_err(|error| {
                    format!("write Contextlint excluded-name link document: {error}")
                })?;
                let mut invocation = Command::new(&python);
                invocation
                    .args(["-I", "-c", adapter])
                    .arg(&fake_node)
                    .arg(&cli_entry)
                    .arg(&config)
                    .arg(CONTEXTLINT_FILES_MARKER)
                    .arg(&document)
                    .current_dir(&workspace)
                    .env_clear()
                    .env(PATH_ENV, CONTEXTLINT_CHILD_PATH)
                    .env(TMPDIR_ENV, &temporary);
                let capture_name = excluded_name.trim_start_matches('.').replace('.', "-");
                let output = run_with_timeout(
                    &mut invocation,
                    &[],
                    timeout,
                    &root.join(format!(
                        "capture-contextlint-excluded-symlink-{capture_name}"
                    )),
                )
                .map_err(|error| {
                    format!("run Contextlint excluded-name symlink {excluded_name}: {error}")
                })?;
                let expected_stderr = format!(
                    "velvet-glove-contextlint: workspace inventory rejects symbolic links: <workspace>/{excluded_name}\n"
                );
                if output.status.code() != Some(2)
                    || !output.stdout.is_empty()
                    || String::from_utf8_lossy(&output.stderr) != expected_stderr
                    || fake_node.with_file_name("node.invoked").exists()
                {
                    return Err(format!(
                        "Contextlint excluded-name symlink {excluded_name} mismatch: status={:?} node_ran={} stdout={:?} stderr={:?}",
                        output.status.code(),
                        fake_node.with_file_name("node.invoked").exists(),
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr),
                    ));
                }
                if sorted_entries(&temporary)?.into_iter().any(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with(CONTEXTLINT_PRIVATE_ROOT_PREFIX))
                }) {
                    return Err(format!(
                        "Contextlint excluded-name symlink {excluded_name} left a private probe directory"
                    ));
                }
                std::fs::remove_file(&excluded_link).map_err(|error| {
                    format!("remove Contextlint excluded-name symlink {excluded_name}: {error}")
                })?;
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&root);
        result
    }
}

fn verify_contextlint_adapter_lifecycle(spec: &ToolSpec, timeout: Duration) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (spec, timeout);
        return Ok(());
    }
    #[cfg(unix)]
    {
        let phase = spec
            .phases
            .get("verify")
            .ok_or_else(|| "Contextlint lifecycle probe lacks a verify phase".to_owned())?;
        let [
            ArgvElement::Literal(isolated),
            ArgvElement::Literal(command),
            ArgvElement::Literal(adapter),
            ArgvElement::Literal(node),
            ArgvElement::Token(ArgToken::ToolExecutable),
            ArgvElement::Token(ArgToken::WorkspaceIndicator),
            ArgvElement::Token(ArgToken::ExtraArgs),
            ArgvElement::Literal(marker),
            ArgvElement::Token(ArgToken::Files),
        ] = phase.argv.as_slice()
        else {
            return Err(
                "Contextlint lifecycle probe could not extract the evaluated adapter".to_owned(),
            );
        };
        if isolated != "-I"
            || command != "-c"
            || node != "node"
            || marker != CONTEXTLINT_FILES_MARKER
        {
            return Err(format!(
                "Contextlint lifecycle probe observed a drifted adapter prefix/marker: {:?}",
                phase.argv
            ));
        }
        let python_program = phase
            .program
            .as_deref()
            .ok_or_else(|| "Contextlint lifecycle probe lacks Python".to_owned())?;
        let python = resolve_program(python_program)
            .or_else(|| {
                (python_program == "python")
                    .then(|| resolve_program("python3"))
                    .flatten()
            })
            .ok_or_else(|| format!("cannot resolve {python_program:?} for Contextlint lifecycle"))?
            .canonicalize()
            .map_err(|error| {
                format!("canonicalize Contextlint lifecycle Python {python_program:?}: {error}")
            })?;

        let root = unique_temp_dir("velvet-glove-contextlint-lifecycle");
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("create Contextlint lifecycle root {root:?}: {error}"))?;
        let root = root
            .canonicalize()
            .map_err(|error| format!("canonicalize Contextlint lifecycle root: {error}"))?;
        let result = (|| {
            let workspace = root.join("workspace");
            let temporary = root.join("tmp");
            let graph = root.join("node_modules");
            let cli_directory = graph.join("@contextlint/cli/dist");
            let core_directory = graph.join("@contextlint/core");
            std::fs::create_dir_all(&workspace)
                .map_err(|error| format!("create Contextlint lifecycle workspace: {error}"))?;
            std::fs::create_dir_all(&temporary)
                .map_err(|error| format!("create Contextlint lifecycle TMPDIR: {error}"))?;
            std::fs::create_dir_all(&cli_directory)
                .map_err(|error| format!("create Contextlint lifecycle CLI graph: {error}"))?;
            std::fs::create_dir_all(&core_directory)
                .map_err(|error| format!("create Contextlint lifecycle core graph: {error}"))?;

            let cli_entry = cli_directory.join("index.js");
            std::fs::write(&cli_entry, "#!/usr/bin/env node\n")
                .map_err(|error| format!("write Contextlint lifecycle CLI entry: {error}"))?;
            let mut cli_permissions = std::fs::metadata(&cli_entry)
                .map_err(|error| format!("stat Contextlint lifecycle CLI entry: {error}"))?
                .permissions();
            cli_permissions.set_mode(0o700);
            std::fs::set_permissions(&cli_entry, cli_permissions)
                .map_err(|error| format!("make Contextlint lifecycle CLI executable: {error}"))?;
            std::fs::write(
                graph.join("@contextlint/cli/package.json"),
                r#"{"name":"@contextlint/cli","version":"1.1.1","type":"module","bin":{"contextlint":"dist/index.js"},"dependencies":{"@contextlint/core":"1.1.1"}}
"#,
            )
            .map_err(|error| format!("write Contextlint lifecycle CLI manifest: {error}"))?;
            std::fs::write(
                core_directory.join("package.json"),
                r#"{"name":"@contextlint/core","version":"1.1.1","type":"module"}
"#,
            )
            .map_err(|error| format!("write Contextlint lifecycle core manifest: {error}"))?;

            let fake_node = root.join("node");
            std::fs::write(
                &fake_node,
                r#"#!/bin/sh
set -eu
config=
previous=
for argument in "$@"; do
  if [ "$previous" = "--config" ]; then
    config=$argument
    break
  fi
  previous=$argument
done
case "$config" in
  */velvet-glove-contextlint-probe-*/contextlint.config.json)
    printf '%s\n' '[{"file":"probe.md","line":0,"severity":"error","message":"Missing required section \"__VELVET_GLOVE_CONTEXTLINT_COMPLETION__\"","ruleId":"SEC-001"}]'
    exit 1
    ;;
esac
: > .contextlint-lifecycle.argv
for argument in "$@"; do
  printf '%s\n' "$argument" >> .contextlint-lifecycle.argv
done
trap '' HUP INT TERM
(
  trap '' HUP INT TERM
  while :; do
    sleep 1
  done
) &
printf '%s\n' "$!" > .contextlint-lifecycle-descendant.pid
printf '%s\n' "$$" > .contextlint-lifecycle-child.pid
: > .contextlint-lifecycle.ready
while :; do
  sleep 1
done
"#,
            )
            .map_err(|error| format!("write Contextlint lifecycle fake Node: {error}"))?;
            let mut node_permissions = std::fs::metadata(&fake_node)
                .map_err(|error| format!("stat Contextlint lifecycle fake Node: {error}"))?
                .permissions();
            node_permissions.set_mode(0o700);
            std::fs::set_permissions(&fake_node, node_permissions).map_err(|error| {
                format!("make Contextlint lifecycle fake Node executable: {error}")
            })?;

            let config = workspace.join("contextlint.config.json");
            let document = workspace.join("example.md");
            std::fs::write(
                &config,
                b"{\"include\":[\"does-not-exist/**/*.md\"],\"rules\":[{\"rule\":\"sec001\",\"options\":{\"sections\":[\"Overview\"]}}]}\n",
            )
            .map_err(|error| format!("write Contextlint lifecycle config: {error}"))?;
            std::fs::write(&document, b"# Overview\n\nComplete.\n")
                .map_err(|error| format!("write Contextlint lifecycle document: {error}"))?;

            run_contextlint_adapter_lifecycle_scenario(
                &python, adapter, &fake_node, &cli_entry, &graph, &workspace, &temporary, &config,
                &document, timeout,
            )?;
            verify_contextlint_adapter_adversarial_contract(
                &python, adapter, &cli_entry, &graph, &root, timeout,
            )
        })();
        let _ = std::fs::remove_dir_all(&root);
        result
    }
}

#[cfg(unix)]
fn contextlint_private_roots(temporary: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    for entry in sorted_entries(temporary)? {
        let name = entry.file_name();
        if !name
            .to_str()
            .is_some_and(|name| name.starts_with(CONTEXTLINT_PRIVATE_ROOT_PREFIX))
        {
            continue;
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("inspect Contextlint private root {path:?}: {error}"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(format!(
                "Contextlint private root is not a physical directory: {path:?}"
            ));
        }
        roots.push(path);
    }
    Ok(roots)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn run_contextlint_adapter_lifecycle_scenario(
    python: &Path,
    adapter: &str,
    fake_node: &Path,
    cli_entry: &Path,
    graph: &Path,
    workspace: &Path,
    temporary: &Path,
    config: &Path,
    document: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let child_pid_path = workspace.join(".contextlint-lifecycle-child.pid");
    let descendant_pid_path = workspace.join(".contextlint-lifecycle-descendant.pid");
    let ready_path = workspace.join(".contextlint-lifecycle.ready");
    let argv_path = workspace.join(".contextlint-lifecycle.argv");
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", adapter])
        .arg(fake_node)
        .arg(cli_entry)
        .arg(config)
        .arg(CONTEXTLINT_FILES_MARKER)
        .arg(document)
        .current_dir(workspace)
        .env_clear()
        .env(PATH_ENV, CONTEXTLINT_CHILD_PATH)
        .env(TMPDIR_ENV, temporary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut outer = command
        .spawn()
        .map_err(|error| format!("spawn evaluated Contextlint lifecycle adapter: {error}"))?;
    let outer_pid = outer.id();
    let startup_timeout = timeout.min(Duration::from_secs(5));
    let startup_deadline = std::time::Instant::now() + startup_timeout;
    while !ready_path.is_file() {
        if let Some(status) = outer
            .try_wait()
            .map_err(|error| format!("poll Contextlint lifecycle adapter: {error}"))?
        {
            let output = outer.wait_with_output().map_err(|error| {
                format!("collect early Contextlint lifecycle adapter output: {error}")
            })?;
            return Err(format!(
                "Contextlint lifecycle adapter exited {status:?} before its authoritative child became ready; stdout={:?}; stderr={:?}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ));
        }
        if std::time::Instant::now() >= startup_deadline {
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            return Err(format!(
                "Contextlint lifecycle child did not become ready within {startup_timeout:?}"
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let private_roots = contextlint_private_roots(temporary)?;
    let [private_root] = private_roots.as_slice() else {
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!(
            "Contextlint lifecycle expected one unique private root, got {private_roots:?}"
        ));
    };
    let private_root = private_root.clone();

    let parse_pid = |path: &Path, description: &str| -> Result<u32, String> {
        let value = std::fs::read_to_string(path)
            .map_err(|error| format!("read Contextlint lifecycle {description}: {error}"))?;
        value.trim().parse::<u32>().map_err(|error| {
            format!("parse Contextlint lifecycle {description} {value:?}: {error}")
        })
    };
    let child_pid = match parse_pid(&child_pid_path, "child PID") {
        Ok(pid) => pid,
        Err(error) => {
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            return Err(error);
        }
    };
    let descendant_pid = match parse_pid(&descendant_pid_path, "descendant PID") {
        Ok(pid) => pid,
        Err(error) => {
            let _ = signal_process_group(child_pid, "KILL");
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            return Err(error);
        }
    };
    if !signal_process_group(child_pid, "0")?.success() {
        let _ = signal_process(descendant_pid, "KILL");
        let _ = signal_process(child_pid, "KILL");
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!(
            "Contextlint lifecycle child {child_pid} did not lead an isolated process group"
        ));
    }

    let expected_arguments = [
        "--disable-proto=throw".to_owned(),
        "--permission".to_owned(),
        format!("--allow-fs-read={}", graph.display()),
        format!("--allow-fs-read={}", workspace.display()),
        format!("--allow-fs-read={}", private_root.display()),
        cli_entry.to_string_lossy().into_owned(),
        "lint".to_owned(),
        "--config".to_owned(),
        private_root
            .join("project.config.json")
            .to_string_lossy()
            .into_owned(),
        "--cwd".to_owned(),
        workspace.to_string_lossy().into_owned(),
        "--format".to_owned(),
        "json".to_owned(),
        "--".to_owned(),
        document.to_string_lossy().into_owned(),
    ]
    .join("\n")
        + "\n";
    let observed_arguments = std::fs::read_to_string(&argv_path).map_err(|error| {
        let _ = signal_process_group(child_pid, "KILL");
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        format!("read Contextlint lifecycle argv: {error}")
    })?;
    if observed_arguments != expected_arguments {
        let _ = signal_process_group(child_pid, "KILL");
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!(
            "Contextlint lifecycle authoritative argv mismatch: expected {expected_arguments:?}, got {observed_arguments:?}"
        ));
    }

    let private_config = private_root.join("contextlint.config.json");
    let private_project_config = private_root.join("project.config.json");
    let private_document = private_root.join("probe.md");
    let expected_private_config = b"{\"rules\":[{\"rule\":\"sec001\",\"options\":{\"sections\":[\"__VELVET_GLOVE_CONTEXTLINT_COMPLETION__\"]}}]}\n";
    let expected_private_document = b"# Probe\n";
    let private_check = (|| {
        if std::fs::read(&private_config).ok().as_deref() != Some(expected_private_config)
            || std::fs::read(&private_project_config).ok().as_deref()
                != std::fs::read(config).ok().as_deref()
            || std::fs::read(&private_document).ok().as_deref() != Some(expected_private_document)
        {
            return Err("Contextlint lifecycle private probe bytes drifted".to_owned());
        }
        for (path, expected_mode) in [
            (&private_root, 0o700),
            (&private_root.join("home"), 0o700),
            (&private_root.join("tmp"), 0o700),
            (&private_root.join("xdg-cache"), 0o700),
            (&private_config, 0o600),
            (&private_project_config, 0o600),
            (&private_document, 0o600),
        ] {
            let mode = std::fs::metadata(path)
                .map_err(|error| format!("stat Contextlint private probe {path:?}: {error}"))?
                .permissions()
                .mode()
                & 0o777;
            if mode != expected_mode {
                return Err(format!(
                    "Contextlint private probe {path:?} mode {mode:o}, expected {expected_mode:o}"
                ));
            }
        }
        Ok(())
    })();
    if let Err(error) = private_check {
        let _ = signal_process_group(child_pid, "KILL");
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(error);
    }

    let term = signal_process(outer_pid, "TERM")?;
    if !term.success() {
        let _ = signal_process_group(child_pid, "KILL");
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!(
            "send SIGTERM to Contextlint lifecycle adapter {outer_pid}: {term:?}"
        ));
    }

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(outer.wait_with_output());
    });
    let completion_timeout = timeout.min(Duration::from_secs(5));
    let output = match receiver.recv_timeout(completion_timeout) {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            let _ = signal_process_group(child_pid, "KILL");
            return Err(format!(
                "wait for terminated Contextlint lifecycle adapter: {error}"
            ));
        }
        Err(error) => {
            let _ = signal_process_group(child_pid, "KILL");
            let _ = signal_process(outer_pid, "KILL");
            let _ = receiver.recv_timeout(Duration::from_secs(2));
            return Err(format!(
                "Contextlint lifecycle adapter or inherited output pipe remained open for {completion_timeout:?}: {error}"
            ));
        }
    };
    let child_alive = process_survives(child_pid, Duration::from_secs(1))?;
    let descendant_alive = process_survives(descendant_pid, Duration::from_secs(1))?;
    let group_alive = process_group_survives(child_pid, Duration::from_secs(1))?;
    if child_alive {
        let _ = signal_process(child_pid, "KILL");
    }
    if descendant_alive {
        let _ = signal_process(descendant_pid, "KILL");
    }
    if group_alive {
        let _ = signal_process_group(child_pid, "KILL");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() != Some(2) {
        Err(format!(
            "SIGTERM Contextlint lifecycle adapter exited {:?}, expected status 2; stdout={stdout:?}; stderr={stderr:?}",
            output.status.code()
        ))
    } else if child_alive {
        Err(format!(
            "SIGTERM Contextlint lifecycle adapter left child {child_pid} alive"
        ))
    } else if descendant_alive {
        Err(format!(
            "SIGTERM Contextlint lifecycle adapter left same-group descendant {descendant_pid} alive"
        ))
    } else if group_alive {
        Err(format!(
            "SIGTERM Contextlint lifecycle adapter left process group {child_pid} alive"
        ))
    } else if !stdout.is_empty() {
        Err(format!(
            "Contextlint lifecycle adapter emitted unexpected stdout: {stdout:?}"
        ))
    } else if stderr != "velvet-glove-contextlint: received signal 15\n" {
        Err(format!(
            "Contextlint lifecycle adapter emitted unexpected stderr: {stderr:?}"
        ))
    } else if private_root.exists() {
        Err("Contextlint lifecycle adapter left its private probe root behind".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
struct ContextlintProbeProject {
    workspace: PathBuf,
    temporary: PathBuf,
    config: PathBuf,
    document: PathBuf,
}

#[cfg(unix)]
fn prepare_contextlint_probe_project(
    root: &Path,
    label: &str,
) -> Result<ContextlintProbeProject, String> {
    let scenario = root.join(label);
    let workspace = scenario.join("workspace");
    let temporary = scenario.join("tmp");
    std::fs::create_dir_all(&workspace)
        .map_err(|error| format!("create Contextlint {label} workspace: {error}"))?;
    std::fs::create_dir_all(&temporary)
        .map_err(|error| format!("create Contextlint {label} TMPDIR: {error}"))?;
    let config = workspace.join("contextlint.config.json");
    let document = workspace.join("example.md");
    std::fs::write(
        &config,
        b"{\"rules\":[{\"rule\":\"sec001\",\"options\":{\"sections\":[\"Overview\"]}}]}\n",
    )
    .map_err(|error| format!("write Contextlint {label} config: {error}"))?;
    std::fs::write(&document, b"# Overview\n\nComplete.\n")
        .map_err(|error| format!("write Contextlint {label} document: {error}"))?;
    Ok(ContextlintProbeProject {
        workspace,
        temporary,
        config,
        document,
    })
}

#[cfg(unix)]
fn contextlint_fake_node_source(project_body: &str) -> String {
    let mut source = String::from(
        r#"#!/bin/sh
set -eu
config=
previous=
for argument in "$@"; do
  if [ "$previous" = "--config" ]; then
    config=$argument
    break
  fi
  previous=$argument
done
case "$config" in
  */velvet-glove-contextlint-probe-*/contextlint.config.json)
    printf '%s\n' '[{"file":"probe.md","line":0,"severity":"error","message":"Missing required section \"__VELVET_GLOVE_CONTEXTLINT_COMPLETION__\"","ruleId":"SEC-001"}]'
    exit 1
    ;;
esac
"#,
    );
    source.push_str(project_body);
    source
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn run_contextlint_adapter_probe(
    python: &Path,
    adapter: &str,
    node: &Path,
    cli_entry: &Path,
    project: &ContextlintProbeProject,
    config: &Path,
    document: &Path,
    timeout: Duration,
    capture: &Path,
) -> Result<BoundedOutput, String> {
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", adapter])
        .arg(node)
        .arg(cli_entry)
        .arg(config)
        .arg(CONTEXTLINT_FILES_MARKER)
        .arg(document)
        .current_dir(&project.workspace)
        .env_clear()
        .env(PATH_ENV, CONTEXTLINT_CHILD_PATH)
        .env(TMPDIR_ENV, &project.temporary);
    run_with_timeout(
        &mut command,
        &[],
        timeout.min(Duration::from_secs(10)),
        capture,
    )
    .map_err(|error| format!("run Contextlint adapter probe: {error}"))
}

#[cfg(unix)]
fn assert_contextlint_private_roots_removed(temporary: &Path, label: &str) -> Result<(), String> {
    let roots = contextlint_private_roots(temporary)?;
    if roots.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Contextlint {label} left private roots behind: {roots:?}"
        ))
    }
}

#[cfg(unix)]
fn verify_contextlint_adapter_adversarial_contract(
    python: &Path,
    adapter: &str,
    cli_entry: &Path,
    graph: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    verify_contextlint_authoritative_config_copy(python, adapter, cli_entry, graph, root, timeout)?;
    verify_contextlint_normal_exit_descendants(python, adapter, cli_entry, root, timeout)?;
    verify_contextlint_output_cap(python, adapter, cli_entry, root, timeout)?;
    verify_contextlint_cleanup_signal_cutoff(python, adapter, cli_entry, root, timeout)?;
    verify_contextlint_guarded_spawn_signal(python, adapter, cli_entry, root, timeout)?;
    verify_contextlint_preflight_paths(python, adapter, cli_entry, root, timeout)?;
    Ok(())
}

#[cfg(unix)]
fn verify_contextlint_authoritative_config_copy(
    python: &Path,
    adapter: &str,
    cli_entry: &Path,
    graph: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let project = prepare_contextlint_probe_project(root, "authoritative-config")?;
    let node = root.join("contextlint-authoritative-config-node");
    let captured_path = root.join("contextlint-authoritative-config-path");
    let captured_bytes = root.join("contextlint-authoritative-config-bytes");
    let config_bytes = std::fs::read(&project.config)
        .map_err(|error| format!("read Contextlint authoritative config baseline: {error}"))?;
    let source = contextlint_fake_node_source(&format!(
        r#"printf '%s\n' "$config" > '{captured_path}'
printf '%s\n' '{{"rules":[{{"rule":"not-pinned"}}]}}' > '{source_config}'
/bin/cp "$config" '{captured_bytes}'
printf '%s' '{original_config}' > '{source_config}'
printf '%s\n' '[]'
exit 0
"#,
        captured_path = shell_probe_path(&captured_path)?,
        source_config = shell_probe_path(&project.config)?,
        captured_bytes = shell_probe_path(&captured_bytes)?,
        original_config = String::from_utf8(config_bytes.clone())
            .map_err(|error| format!("Contextlint test config is not UTF-8: {error}"))?
            .replace('\'', "'\\''"),
    ));
    write_executable_probe(&node, &source)?;
    let output = run_contextlint_adapter_probe(
        python,
        adapter,
        &node,
        cli_entry,
        &project,
        &project.config,
        &project.document,
        timeout,
        &root.join("capture-contextlint-authoritative-config"),
    )?;
    let private_config = PathBuf::from(
        std::fs::read_to_string(&captured_path)
            .map_err(|error| format!("read Contextlint authoritative config path: {error}"))?
            .trim(),
    );
    let observed = std::fs::read(&captured_bytes)
        .map_err(|error| format!("read Contextlint authoritative config capture: {error}"))?;
    let source_after = std::fs::read(&project.config)
        .map_err(|error| format!("read Contextlint restored source config: {error}"))?;
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || String::from_utf8_lossy(&output.stderr)
            != "velvet-glove-contextlint: contextlint.config.json changed during contextlint: <workspace>/contextlint.config.json\n"
        || observed != config_bytes
        || source_after != config_bytes
        || !private_config.starts_with(&project.temporary)
        || private_config.starts_with(&project.workspace)
        || private_config.file_name() != Some(OsStr::new("project.config.json"))
        || private_config.exists()
        || private_config.parent().is_some_and(Path::exists)
    {
        return Err(format!(
            "Contextlint authoritative private config mismatch: status={:?} stdout={:?} stderr={:?} private={private_config:?} bytes={} source={} graph={graph:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            observed == config_bytes,
            source_after == config_bytes,
        ));
    }
    assert_contextlint_private_roots_removed(&project.temporary, "authoritative config")
}

#[cfg(unix)]
fn verify_contextlint_normal_exit_descendants(
    python: &Path,
    adapter: &str,
    cli_entry: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    for (label, redirect) in [
        ("closed-stdio-descendant", " >/dev/null 2>&1"),
        ("inherited-pipe-descendant", ""),
    ] {
        let project = prepare_contextlint_probe_project(root, label)?;
        let node = root.join(format!("contextlint-{label}-node"));
        let pid_record = root.join(format!("contextlint-{label}.pid"));
        let ready = root.join(format!("contextlint-{label}.ready"));
        let body = format!(
            r#"(
  trap '' HUP INT TERM
  : > '{ready}'
  while :; do sleep 1; done
){redirect} &
descendant=$!
while [ ! -f '{ready}' ]; do sleep 0.01; done
printf '%s\n' "$descendant" > '{pid_record}'
printf '%s\n' '[]'
exit 0
"#,
            ready = shell_probe_path(&ready)?,
            pid_record = shell_probe_path(&pid_record)?,
        );
        write_executable_probe(&node, &contextlint_fake_node_source(&body))?;
        let output = run_contextlint_adapter_probe(
            python,
            adapter,
            &node,
            cli_entry,
            &project,
            &project.config,
            &project.document,
            timeout,
            &root.join(format!("capture-contextlint-{label}")),
        )?;
        let descendant = read_pid_file(&pid_record, &format!("Contextlint {label}"))?;
        let descendant_alive = process_survives(descendant, Duration::from_secs(1))?;
        if descendant_alive {
            let _ = signal_process(descendant, "KILL");
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.code() != Some(2)
            || !output.stdout.is_empty()
            || descendant_alive
            || stderr
                != "velvet-glove-contextlint: native Contextlint left same-group descendants after child exit\n"
        {
            return Err(format!(
                "Contextlint {label} sweep mismatch: status={:?} descendant={descendant}:{descendant_alive} stdout={:?} stderr={stderr:?}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
            ));
        }
        assert_contextlint_private_roots_removed(&project.temporary, label)?;
    }
    Ok(())
}

#[cfg(unix)]
fn verify_contextlint_output_cap(
    python: &Path,
    adapter: &str,
    cli_entry: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    const LIMIT_ENTRY: &str = "MAX_OUTPUT_BYTES = 16 * 1024 * 1024";
    if adapter.matches(LIMIT_ENTRY).count() != 1 {
        return Err("Contextlint output-cap probe could not locate one limit".to_owned());
    }
    let instrumented = adapter.replacen(LIMIT_ENTRY, "MAX_OUTPUT_BYTES = 1024", 1);
    let project = prepare_contextlint_probe_project(root, "output-cap")?;
    let node = root.join("contextlint-output-cap-node");
    let oversized = "x".repeat(2048);
    write_executable_probe(
        &node,
        &contextlint_fake_node_source(&format!("printf '%s' '{oversized}'\nexit 0\n")),
    )?;
    let output = run_contextlint_adapter_probe(
        python,
        &instrumented,
        &node,
        cli_entry,
        &project,
        &project.config,
        &project.document,
        timeout,
        &root.join("capture-contextlint-output-cap"),
    )?;
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || String::from_utf8_lossy(&output.stderr)
            != "velvet-glove-contextlint: adapter failure: combined contextlint output exceeded 1024 bytes\n"
    {
        return Err(format!(
            "Contextlint output-cap mismatch: status={:?} stdout={:?} stderr={:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    assert_contextlint_private_roots_removed(&project.temporary, "output cap")
}

#[cfg(unix)]
fn wait_for_contextlint_barrier(
    outer: &mut std::process::Child,
    outer_pid: u32,
    ready: &Path,
    label: &str,
    timeout: Duration,
) -> Result<(), String> {
    let startup_timeout = timeout.min(Duration::from_secs(5));
    let deadline = std::time::Instant::now() + startup_timeout;
    while !ready.is_file() {
        if let Some(status) = outer
            .try_wait()
            .map_err(|error| format!("poll Contextlint {label} adapter: {error}"))?
        {
            return Err(format!(
                "Contextlint {label} adapter exited {status:?} before its barrier"
            ));
        }
        if std::time::Instant::now() >= deadline {
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            return Err(format!(
                "Contextlint {label} adapter did not reach its barrier within {startup_timeout:?}"
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[cfg(unix)]
fn collect_contextlint_outer(
    outer: std::process::Child,
    outer_pid: u32,
    label: &str,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(outer.wait_with_output());
    });
    match receiver.recv_timeout(timeout.min(Duration::from_secs(5))) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(format!("wait for Contextlint {label} adapter: {error}")),
        Err(error) => {
            let _ = signal_process(outer_pid, "KILL");
            Err(format!(
                "Contextlint {label} adapter did not finish: {error}"
            ))
        }
    }
}

#[cfg(unix)]
fn verify_contextlint_cleanup_signal_cutoff(
    python: &Path,
    adapter: &str,
    cli_entry: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    const READY_ENV: &str = "VELVET_GLOVE_CONTEXTLINT_CLEANUP_PROBE_READY";
    const RELEASE_ENV: &str = "VELVET_GLOVE_CONTEXTLINT_CLEANUP_PROBE_RELEASE";
    const CLEANUP_ENTRY: &str =
        "        cleaning = True\n        try:\n            if run_root is not None:\n";
    if adapter.matches(CLEANUP_ENTRY).count() != 1 {
        return Err("Contextlint cleanup probe could not locate one cleanup entry".to_owned());
    }
    let instrumented_entry = format!(
        "        cleaning = True\n        cleanup_probe_ready = os.environ.get({READY_ENV:?})\n        cleanup_probe_release = os.environ.get({RELEASE_ENV:?})\n        if cleanup_probe_ready is not None and cleanup_probe_release is not None:\n            with open(cleanup_probe_ready, \"xb\"):\n                pass\n            while not os.path.exists(cleanup_probe_release):\n                time.sleep(0.01)\n        try:\n            if run_root is not None:\n"
    );
    let instrumented = adapter.replacen(CLEANUP_ENTRY, &instrumented_entry, 1);
    let project = prepare_contextlint_probe_project(root, "cleanup-signal")?;
    let node = root.join("contextlint-cleanup-signal-node");
    let private_config_record = root.join("contextlint-cleanup-signal-private-config");
    write_executable_probe(
        &node,
        &contextlint_fake_node_source(&format!(
            "printf '%s\\n' \"$config\" > '{}'\nprintf '%s\\n' '[]'\nexit 0\n",
            shell_probe_path(&private_config_record)?,
        )),
    )?;
    let ready = root.join("contextlint-cleanup-signal.ready");
    let release = root.join("contextlint-cleanup-signal.release");
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", &instrumented])
        .arg(&node)
        .arg(cli_entry)
        .arg(&project.config)
        .arg(CONTEXTLINT_FILES_MARKER)
        .arg(&project.document)
        .current_dir(&project.workspace)
        .env_clear()
        .env(PATH_ENV, CONTEXTLINT_CHILD_PATH)
        .env(TMPDIR_ENV, &project.temporary)
        .env(READY_ENV, &ready)
        .env(RELEASE_ENV, &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut outer = command
        .spawn()
        .map_err(|error| format!("spawn Contextlint cleanup-signal adapter: {error}"))?;
    let outer_pid = outer.id();
    wait_for_contextlint_barrier(&mut outer, outer_pid, &ready, "cleanup-signal", timeout)?;
    let private_config = PathBuf::from(
        std::fs::read_to_string(&private_config_record)
            .map_err(|error| format!("read Contextlint cleanup private config: {error}"))?
            .trim(),
    );
    let term = signal_process(outer_pid, "TERM")?;
    if !term.success() {
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!(
            "send cleanup-window SIGTERM to Contextlint adapter {outer_pid}: {term:?}"
        ));
    }
    std::fs::write(&release, b"release\n")
        .map_err(|error| format!("release Contextlint cleanup barrier: {error}"))?;
    let output = collect_contextlint_outer(outer, outer_pid, "cleanup-signal", timeout)?;
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || String::from_utf8_lossy(&output.stderr)
            != "velvet-glove-contextlint: received signal 15\n"
        || private_config.exists()
        || private_config.parent().is_some_and(Path::exists)
    {
        return Err(format!(
            "Contextlint cleanup-signal mismatch: status={:?} stdout={:?} stderr={:?} private={private_config:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    assert_contextlint_private_roots_removed(&project.temporary, "cleanup signal")
}

#[cfg(unix)]
fn verify_contextlint_guarded_spawn_signal(
    python: &Path,
    adapter: &str,
    cli_entry: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    const READY_ENV: &str = "VELVET_GLOVE_CONTEXTLINT_SPAWN_PROBE_READY";
    const RELEASE_ENV: &str = "VELVET_GLOVE_CONTEXTLINT_SPAWN_PROBE_RELEASE";
    const SPAWN_ENTRY: &str = "        spawn_mask = signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)\n        try:\n";
    if adapter.matches(SPAWN_ENTRY).count() != 1 {
        return Err("Contextlint spawn probe could not locate one guarded spawn entry".to_owned());
    }
    let instrumented_entry = format!(
        "        spawn_mask = signal.pthread_sigmask(signal.SIG_BLOCK, handled_signals)\n        spawn_probe_ready = os.environ.get({READY_ENV:?})\n        spawn_probe_release = os.environ.get({RELEASE_ENV:?})\n        if spawn_probe_ready is not None and spawn_probe_release is not None:\n            with open(spawn_probe_ready, \"xb\"):\n                pass\n            while not os.path.exists(spawn_probe_release):\n                time.sleep(0.01)\n        try:\n"
    );
    let instrumented = adapter.replacen(SPAWN_ENTRY, &instrumented_entry, 1);
    let project = prepare_contextlint_probe_project(root, "guarded-spawn")?;
    let node = root.join("contextlint-guarded-spawn-node");
    let node_marker = root.join("contextlint-guarded-spawn-node-ran");
    write_executable_probe(
        &node,
        &format!(
            "#!/bin/sh\nset -eu\n: > '{}'\ntrap '' HUP INT TERM\nwhile :; do sleep 1; done\n",
            shell_probe_path(&node_marker)?,
        ),
    )?;
    let ready = root.join("contextlint-guarded-spawn.ready");
    let release = root.join("contextlint-guarded-spawn.release");
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", &instrumented])
        .arg(&node)
        .arg(cli_entry)
        .arg(&project.config)
        .arg(CONTEXTLINT_FILES_MARKER)
        .arg(&project.document)
        .current_dir(&project.workspace)
        .env_clear()
        .env(PATH_ENV, CONTEXTLINT_CHILD_PATH)
        .env(TMPDIR_ENV, &project.temporary)
        .env(READY_ENV, &ready)
        .env(RELEASE_ENV, &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut outer = command
        .spawn()
        .map_err(|error| format!("spawn Contextlint guarded-spawn adapter: {error}"))?;
    let outer_pid = outer.id();
    wait_for_contextlint_barrier(&mut outer, outer_pid, &ready, "guarded-spawn", timeout)?;
    let term = signal_process(outer_pid, "TERM")?;
    if !term.success() {
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!(
            "send spawn-window SIGTERM to Contextlint adapter {outer_pid}: {term:?}"
        ));
    }
    std::fs::write(&release, b"release\n")
        .map_err(|error| format!("release Contextlint guarded-spawn barrier: {error}"))?;
    let output = collect_contextlint_outer(outer, outer_pid, "guarded-spawn", timeout)?;
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || node_marker.exists()
        || String::from_utf8_lossy(&output.stderr)
            != "velvet-glove-contextlint: received signal 15\n"
    {
        return Err(format!(
            "Contextlint guarded-spawn mismatch: status={:?} node_ran={} stdout={:?} stderr={:?}",
            output.status.code(),
            node_marker.exists(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    assert_contextlint_private_roots_removed(&project.temporary, "guarded spawn")
}

#[cfg(unix)]
fn verify_contextlint_preflight_paths(
    python: &Path,
    adapter: &str,
    cli_entry: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let node = root.join("contextlint-preflight-node");
    let marker = root.join("contextlint-preflight-node-ran");
    write_executable_probe(
        &node,
        &format!(
            "#!/bin/sh\nset -eu\n: > '{}'\nexit 99\n",
            shell_probe_path(&marker)?,
        ),
    )?;

    let symlink_project = prepare_contextlint_probe_project(root, "indicator-symlink")?;
    let alias = root.join("contextlint-indicator-workspace-alias");
    std::os::unix::fs::symlink(&symlink_project.workspace, &alias)
        .map_err(|error| format!("create Contextlint workspace alias: {error}"))?;
    let output = run_contextlint_adapter_probe(
        python,
        adapter,
        &node,
        cli_entry,
        &symlink_project,
        &alias.join("contextlint.config.json"),
        &alias.join("example.md"),
        timeout,
        &root.join("capture-contextlint-indicator-symlink"),
    )?;
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || String::from_utf8_lossy(&output.stderr)
            != "velvet-glove-contextlint: workspace indicator traverses a symbolic link\n"
        || marker.exists()
    {
        return Err(format!(
            "Contextlint indicator-symlink mismatch: status={:?} marker={} stdout={:?} stderr={:?}",
            output.status.code(),
            marker.exists(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }

    let inside_project = prepare_contextlint_probe_project(root, "inside-workspace-tmp")?;
    let inside_tmp = inside_project.workspace.join("tmp");
    std::fs::create_dir(&inside_tmp)
        .map_err(|error| format!("create Contextlint inside-workspace TMPDIR: {error}"))?;
    let inside = ContextlintProbeProject {
        workspace: inside_project.workspace.clone(),
        temporary: inside_tmp,
        config: inside_project.config.clone(),
        document: inside_project.document.clone(),
    };
    let output = run_contextlint_adapter_probe(
        python,
        adapter,
        &node,
        cli_entry,
        &inside,
        &inside.config,
        &inside.document,
        timeout,
        &root.join("capture-contextlint-inside-workspace-tmp"),
    )?;
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || String::from_utf8_lossy(&output.stderr)
            != "velvet-glove-contextlint: TMPDIR must resolve outside the Contextlint workspace\n"
        || marker.exists()
    {
        return Err(format!(
            "Contextlint inside-workspace TMPDIR mismatch: status={:?} marker={} stdout={:?} stderr={:?}",
            output.status.code(),
            marker.exists(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }

    let unwritable = prepare_contextlint_probe_project(root, "unwritable-tmp")?;
    let mut permissions = std::fs::metadata(&unwritable.temporary)
        .map_err(|error| format!("inspect Contextlint unwritable TMPDIR: {error}"))?
        .permissions();
    permissions.set_mode(0o500);
    std::fs::set_permissions(&unwritable.temporary, permissions)
        .map_err(|error| format!("make Contextlint TMPDIR unwritable: {error}"))?;
    let output = run_contextlint_adapter_probe(
        python,
        adapter,
        &node,
        cli_entry,
        &unwritable,
        &unwritable.config,
        &unwritable.document,
        timeout,
        &root.join("capture-contextlint-unwritable-tmp"),
    );
    let mut permissions = std::fs::metadata(&unwritable.temporary)
        .map_err(|error| format!("reinspect Contextlint unwritable TMPDIR: {error}"))?
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&unwritable.temporary, permissions)
        .map_err(|error| format!("restore Contextlint TMPDIR permissions: {error}"))?;
    let output = output?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() != Some(2)
        || !output.stdout.is_empty()
        || marker.exists()
        || stderr.contains(CONTEXTLINT_PRIVATE_ROOT_PREFIX)
        || stderr
            != "velvet-glove-contextlint: cannot create private completion-probe root: [Errno 13] Permission denied: '<private-probe>'\n"
    {
        return Err(format!(
            "Contextlint unwritable TMPDIR mismatch: status={:?} marker={} stdout={:?} stderr={stderr:?}",
            output.status.code(),
            marker.exists(),
            String::from_utf8_lossy(&output.stdout),
        ));
    }
    assert_contextlint_private_roots_removed(&unwritable.temporary, "unwritable TMPDIR")
}

fn verify_biome_adapter_lifecycle(spec: &ToolSpec, timeout: Duration) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (spec, timeout);
        return Ok(());
    }
    #[cfg(unix)]
    {
        const CHILD_PID_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_CHILD_PID";
        const DESCENDANT_PID_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_DESCENDANT_PID";
        const DESCENDANT_READY_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_DESCENDANT_READY";
        const CHILD_ARGV_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_CHILD_ARGV";
        const CHILD_MODE_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_CHILD_MODE";
        const LEADER_TERM_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_LEADER_TERM";
        let phase = spec
            .phases
            .get("verify")
            .ok_or_else(|| "biome lifecycle probe lacks a verify phase".to_owned())?;
        let [
            ArgvElement::Literal(isolated),
            ArgvElement::Literal(command),
            ArgvElement::Literal(adapter),
            ArgvElement::Token(ArgToken::ToolExecutable),
            ArgvElement::Literal(mode),
            ArgvElement::Token(ArgToken::ExtraArgs),
            ArgvElement::Literal(marker),
            ArgvElement::Token(ArgToken::Files),
        ] = phase.argv.as_slice()
        else {
            return Err("biome lifecycle probe could not extract the evaluated adapter".to_owned());
        };
        if isolated != "-I" || command != "-c" {
            return Err(format!(
                "biome lifecycle probe expected isolated Python -I -c, got {isolated:?} {command:?}"
            ));
        }
        if mode != "verify" || marker != BIOME_FILES_MARKER {
            return Err(format!(
                "biome lifecycle probe expected exact verify/marker shape, got mode={mode:?} marker={marker:?}"
            ));
        }
        let python_program = phase
            .program
            .as_deref()
            .ok_or_else(|| "biome lifecycle probe lacks an adapter program".to_owned())?;
        let python = resolve_program(python_program)
            .ok_or_else(|| format!("biome lifecycle probe cannot resolve {python_program:?}"))?;
        let python = python
            .canonicalize()
            .map_err(|error| format!("canonicalize Biome lifecycle Python {python:?}: {error}"))?;

        let root = unique_temp_dir("velvet-glove-biome-lifecycle");
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("create Biome lifecycle root {root:?}: {error}"))?;
        let result = (|| {
            let spaced_program_dir = root.join("state root with spaces");
            std::fs::create_dir(&spaced_program_dir)
                .map_err(|error| format!("create spaced Biome program root: {error}"))?;
            let lifecycle_python = spaced_program_dir.join("python");
            std::os::unix::fs::symlink(&python, &lifecycle_python).map_err(|error| {
                format!("link spaced Biome lifecycle Python {lifecycle_python:?}: {error}")
            })?;
            let fake_tool = root.join("biome-fake");
            let target_argument = "--dash.js";
            let target = root.join(target_argument);
            std::fs::write(&target, "const dash = true;\n")
                .map_err(|error| format!("write Biome lifecycle target {target:?}: {error}"))?;
            let fake_source = format!(
                r#"#!/bin/sh
set -eu
case "${{{CHILD_MODE_ENV}}}" in
  trap) trap 'printf "term\n" > "${{{LEADER_TERM_ENV}}}"; exit 0' HUP INT TERM ;;
  ignore) trap '' HUP INT TERM ;;
  *) exit 64 ;;
esac
: > "${{{CHILD_ARGV_ENV}}}"
for argument in "$@"; do
  printf '%s\n' "$argument" >> "${{{CHILD_ARGV_ENV}}}"
done
(
  trap '' HUP INT TERM
  : > "${{{DESCENDANT_READY_ENV}}}"
  while :; do
    :
  done
) &
while [ ! -f "${{{DESCENDANT_READY_ENV}}}" ]; do
  :
done
printf '%s\n' "$!" > "${{{DESCENDANT_PID_ENV}}}"
printf '%s\n' "$$" > "${{{CHILD_PID_ENV}}}"
printf 'ready\n'
while :; do
  :
done
"#
            );
            std::fs::write(&fake_tool, fake_source)
                .map_err(|error| format!("write Biome lifecycle fake {fake_tool:?}: {error}"))?;
            let mut permissions = std::fs::metadata(&fake_tool)
                .map_err(|error| format!("stat Biome lifecycle fake {fake_tool:?}: {error}"))?
                .permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(&fake_tool, permissions)
                .map_err(|error| format!("make Biome lifecycle fake executable: {error}"))?;

            let expected_arguments = [
                "check",
                "--colors=off",
                "--reporter=json",
                "--max-diagnostics=none",
                "--error-on-warnings",
                "--no-errors-on-unmatched",
                "--",
                target_argument,
            ]
            .join("\n")
                + "\n";
            for mode in ["trap", "ignore"] {
                let child_pid_path = root.join(format!("{mode}-child.pid"));
                let descendant_pid_path = root.join(format!("{mode}-descendant.pid"));
                let descendant_ready_path = root.join(format!("{mode}-descendant.ready"));
                let child_argv_path = root.join(format!("{mode}-child.argv"));
                let leader_term_path = root.join(format!("{mode}-leader.term"));
                run_biome_adapter_lifecycle_scenario(
                    &lifecycle_python,
                    adapter,
                    &fake_tool,
                    &root,
                    target_argument,
                    mode,
                    &child_pid_path,
                    &descendant_pid_path,
                    &descendant_ready_path,
                    &child_argv_path,
                    &leader_term_path,
                    timeout,
                )?;
                let observed_arguments =
                    std::fs::read_to_string(&child_argv_path).map_err(|error| {
                        format!(
                            "read Biome lifecycle {mode} child argv {child_argv_path:?}: {error}"
                        )
                    })?;
                if observed_arguments != expected_arguments {
                    return Err(format!(
                        "Biome lifecycle {mode} child argv mismatch for dash-leading selected path: expected {expected_arguments:?}, got {observed_arguments:?}"
                    ));
                }
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&root);
        result
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn run_biome_adapter_lifecycle_scenario(
    lifecycle_python: &Path,
    adapter: &str,
    fake_tool: &Path,
    root: &Path,
    target_argument: &str,
    mode: &str,
    child_pid_path: &Path,
    descendant_pid_path: &Path,
    descendant_ready_path: &Path,
    child_argv_path: &Path,
    leader_term_path: &Path,
    timeout: Duration,
) -> Result<(), String> {
    const CHILD_PID_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_CHILD_PID";
    const DESCENDANT_PID_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_DESCENDANT_PID";
    const DESCENDANT_READY_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_DESCENDANT_READY";
    const CHILD_ARGV_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_CHILD_ARGV";
    const CHILD_MODE_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_CHILD_MODE";
    const LEADER_TERM_ENV: &str = "VELVET_GLOVE_BIOME_LIFECYCLE_LEADER_TERM";
    let mut command = Command::new(lifecycle_python);
    command
        .args(["-I", "-c", adapter])
        .arg(fake_tool)
        .arg("verify")
        .arg(BIOME_FILES_MARKER)
        .arg(target_argument)
        .current_dir(root)
        .env(CHILD_PID_ENV, child_pid_path)
        .env(DESCENDANT_PID_ENV, descendant_pid_path)
        .env(DESCENDANT_READY_ENV, descendant_ready_path)
        .env(CHILD_ARGV_ENV, child_argv_path)
        .env(CHILD_MODE_ENV, mode)
        .env(LEADER_TERM_ENV, leader_term_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut outer = command
        .spawn()
        .map_err(|error| format!("spawn evaluated Biome {mode} lifecycle adapter: {error}"))?;
    let outer_pid = outer.id();
    let startup_timeout = timeout.min(Duration::from_secs(5));
    let startup_deadline = std::time::Instant::now() + startup_timeout;
    let child_pid = loop {
        if let Ok(value) = std::fs::read_to_string(child_pid_path) {
            match value.trim().parse::<u32>() {
                Ok(pid) => break pid,
                Err(error) => {
                    let _ = signal_process(outer_pid, "KILL");
                    let _ = outer.wait();
                    return Err(format!(
                        "parse Biome {mode} lifecycle child PID {value:?}: {error}"
                    ));
                }
            }
        }
        if let Some(status) = outer
            .try_wait()
            .map_err(|error| format!("poll Biome {mode} lifecycle adapter: {error}"))?
        {
            return Err(format!(
                "Biome {mode} lifecycle adapter exited {status:?} before its child became ready"
            ));
        }
        if std::time::Instant::now() >= startup_deadline {
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            return Err(format!(
                "Biome {mode} lifecycle child did not become ready within {startup_timeout:?}"
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let descendant_pid = std::fs::read_to_string(descendant_pid_path)
        .map_err(|error| {
            let _ = signal_process(child_pid, "KILL");
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            format!("read Biome {mode} lifecycle descendant PID {descendant_pid_path:?}: {error}")
        })?
        .trim()
        .parse::<u32>()
        .map_err(|error| {
            let _ = signal_process(child_pid, "KILL");
            let _ = signal_process(outer_pid, "KILL");
            let _ = outer.wait();
            format!("parse Biome {mode} lifecycle descendant PID: {error}")
        })?;
    if !signal_process_group(child_pid, "0")?.success() {
        let _ = signal_process(descendant_pid, "KILL");
        let _ = signal_process(child_pid, "KILL");
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!(
            "Biome {mode} lifecycle child {child_pid} did not lead an isolated process group"
        ));
    }

    let term = signal_process(outer_pid, "TERM")?;
    if !term.success() {
        let _ = signal_process(descendant_pid, "KILL");
        let _ = signal_process(child_pid, "KILL");
        let _ = signal_process(outer_pid, "KILL");
        let _ = outer.wait();
        return Err(format!(
            "send SIGTERM to Biome {mode} lifecycle adapter {outer_pid}: {term:?}"
        ));
    }

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(outer.wait_with_output());
    });
    let completion_timeout = timeout.min(Duration::from_secs(5));
    let output = match receiver.recv_timeout(completion_timeout) {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            let _ = signal_process(descendant_pid, "KILL");
            let _ = signal_process(child_pid, "KILL");
            return Err(format!(
                "wait for terminated Biome {mode} lifecycle adapter: {error}"
            ));
        }
        Err(error) => {
            let _ = signal_process(descendant_pid, "KILL");
            let _ = signal_process(child_pid, "KILL");
            let _ = signal_process(outer_pid, "KILL");
            let _ = receiver.recv_timeout(Duration::from_secs(2));
            return Err(format!(
                "Biome {mode} lifecycle adapter or inherited output pipe remained open for {completion_timeout:?}: {error}"
            ));
        }
    };
    let child_alive = process_survives(child_pid, Duration::from_secs(1))?;
    let descendant_alive = process_survives(descendant_pid, Duration::from_secs(1))?;
    let group_alive = process_group_survives(child_pid, Duration::from_secs(1))?;
    if child_alive {
        let _ = signal_process(child_pid, "KILL");
    }
    if descendant_alive {
        let _ = signal_process(descendant_pid, "KILL");
    }
    if group_alive {
        let _ = signal_process_group(child_pid, "KILL");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let leader_term = std::fs::read_to_string(leader_term_path).ok();
    if output.status.code() != Some(2) {
        Err(format!(
            "SIGTERM Biome {mode} lifecycle adapter exited {:?}, expected status 2; stdout={stdout:?}; stderr={stderr:?}",
            output.status.code()
        ))
    } else if child_alive {
        Err(format!(
            "SIGTERM Biome {mode} lifecycle adapter left child {child_pid} alive"
        ))
    } else if descendant_alive {
        Err(format!(
            "SIGTERM Biome {mode} lifecycle adapter left same-group descendant {descendant_pid} alive"
        ))
    } else if group_alive {
        Err(format!(
            "SIGTERM Biome {mode} lifecycle adapter left process group {child_pid} alive"
        ))
    } else if !matches!(stdout.as_ref(), "" | "ready\n") {
        Err(format!(
            "Biome {mode} lifecycle adapter emitted unexpected partial stdout: {stdout:?}"
        ))
    } else if stderr != "velvet-glove-biome: received signal 15\n" {
        Err(format!(
            "Biome {mode} lifecycle adapter emitted unexpected stderr: {stderr:?}"
        ))
    } else if mode == "trap" && leader_term.as_deref() != Some("term\n") {
        Err(format!(
            "Biome trapping lifecycle leader did not record graceful TERM: {leader_term:?}"
        ))
    } else if mode == "ignore" && leader_term.is_some() {
        Err(format!(
            "Biome ignoring lifecycle leader unexpectedly handled TERM: {leader_term:?}"
        ))
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn process_survives(pid: u32, timeout: Duration) -> Result<bool, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !signal_process(pid, "0")?.success() {
            return Ok(false);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn process_group_survives(pgid: u32, timeout: Duration) -> Result<bool, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !signal_process_group(pgid, "0")?.success() {
            return Ok(false);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn signal_process(pid: u32, signal: &str) -> Result<std::process::ExitStatus, String> {
    Command::new("/bin/kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("run /bin/kill -{signal} {pid}: {error}"))
}

#[cfg(unix)]
fn signal_process_group(pgid: u32, signal: &str) -> Result<std::process::ExitStatus, String> {
    Command::new("/bin/kill")
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(format!("-{pgid}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("run /bin/kill -{signal} -- -{pgid}: {error}"))
}

fn resolve_program(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.components().count() > 1 {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().ok()?.join(path)
        };
        return is_executable(&candidate).then_some(candidate);
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(program))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn run_probe_matrix(timeout: Duration, artifact_root: Option<&Path>) -> Result<usize, String> {
    let mut commands = 0;
    for surface in ProtocolSurface::ALL {
        match run_probe_case(surface, timeout, artifact_root) {
            Ok(executed) => commands += executed,
            Err(mut error) => {
                let report = probe_report(commands, Some((surface, &error)));
                println!("{REPORT_PREFIX}{report}");
                if let Some(root) = artifact_root {
                    match write_report(root, &report) {
                        Ok(path) => error.push_str(&format!(
                            "; machine-readable failure report: {}",
                            path.display()
                        )),
                        Err(report_error) => error.push_str(&format!(
                            "; failed to retain machine-readable probe report: {report_error}"
                        )),
                    }
                }
                return Err(error);
            }
        }
    }
    let report = probe_report(commands, None);
    println!("{REPORT_PREFIX}{report}");
    if commands == 0 {
        return Err("probe executed zero external commands".to_owned());
    }
    Ok(commands)
}

fn probe_report(commands: usize, failure: Option<(ProtocolSurface, &str)>) -> JsonValue {
    serde_json::json!({
        "formatVersion": 1,
        "kind": "probe",
        "status": if failure.is_some() { "fail" } else { "pass" },
        "totals": {
            "protocolProbeSurfaces": ProtocolSurface::ALL.len(),
            "commandsExecuted": commands,
        },
        "failure": failure.map(|(surface, detail)| serde_json::json!({
            "surface": surface.cli_name(),
            "detail": detail,
        })),
    })
}

fn run_probe_case(
    surface: ProtocolSurface,
    timeout: Duration,
    artifact_root: Option<&Path>,
) -> Result<usize, String> {
    run_probe_attempt(surface, artifact_root, |root| {
        run_probe_case_inner(surface, timeout, root)
    })
}

fn run_probe_attempt(
    surface: ProtocolSurface,
    artifact_root: Option<&Path>,
    execute: impl FnOnce(&Path) -> Result<usize, String>,
) -> Result<usize, String> {
    let root = unique_temp_dir(&format!("velvet-glove-probe-{surface}"));
    match execute(&root) {
        Ok(commands) => {
            let _ = std::fs::remove_dir_all(&root);
            Ok(commands)
        }
        Err(mut error) => {
            let evidence = root.join("evidence");
            if let Err(write_error) = std::fs::create_dir_all(&evidence)
                .map_err(|write_error| format!("create probe evidence: {write_error}"))
                .and_then(|()| {
                    write_json(
                        &evidence.join("probe-outcome.json"),
                        &serde_json::json!({
                            "formatVersion": 1,
                            "surface": surface.cli_name(),
                            "status": "fail",
                            "detail": error,
                        }),
                    )
                })
            {
                error.push_str(&format!("; failed to write probe outcome: {write_error}"));
            }
            if let Some(destination_root) = artifact_root {
                match retain_probe_failure(&root, destination_root, surface) {
                    Ok(destination) => {
                        let _ = std::fs::remove_dir_all(&root);
                        error.push_str(&format!(
                            "; retained probe artifacts: {}",
                            destination.display()
                        ));
                    }
                    Err(retain_error) => error.push_str(&format!(
                        "; {retain_error}; preserved temporary probe artifacts: {}",
                        root.display()
                    )),
                }
            } else {
                let _ = std::fs::remove_dir_all(&root);
            }
            Err(error)
        }
    }
}

fn run_probe_case_inner(
    surface: ProtocolSurface,
    timeout: Duration,
    root: &Path,
) -> Result<usize, String> {
    let project = root.join("workspace");
    let evidence = root.join("evidence");
    let probe_dir = root.join("probe");
    std::fs::create_dir_all(&project)
        .map_err(|error| format!("create probe workspace {project:?}: {error}"))?;
    std::fs::create_dir_all(&evidence)
        .map_err(|error| format!("create probe evidence {evidence:?}: {error}"))?;
    std::fs::create_dir_all(&probe_dir)
        .map_err(|error| format!("create probe directory {probe_dir:?}: {error}"))?;
    let target = project.join("example.fixture");
    std::fs::write(&target, "fixture\n")
        .map_err(|error| format!("write probe fixture {target:?}: {error}"))?;

    let probe = probe_dir.join("fixture-probe");
    std::fs::write(&probe, include_bytes!("support/fixture-probe.sh"))
        .map_err(|error| format!("write probe executable {probe:?}: {error}"))?;
    #[cfg(unix)]
    {
        let mut permissions = std::fs::metadata(&probe)
            .map_err(|error| format!("probe metadata {probe:?}: {error}"))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&probe, permissions)
            .map_err(|error| format!("make probe executable {probe:?}: {error}"))?;
    }
    let config = write_probe_config(&project, &probe)?;

    let input = PostToolUseBuilder::new(surface, &project, "example.fixture")
        .identity("probe-session", "probe-turn", "probe-tool")
        .build()?;
    std::fs::write(evidence.join("input.json"), input.bytes())
        .map_err(|error| format!("write probe input: {error}"))?;
    let sentinel = format!("surface:{}", surface.cli_name());
    let binary = env!("CARGO_BIN_EXE_velvet-glove");
    let mut command = Command::new(binary);
    command
        .args(["--harness", surface.cli_name(), "--config"])
        .arg(config)
        .arg("post-tool-immediate");
    input.configure_command(&mut command);
    command
        .env(PROBE_DIR_ENV, &probe_dir)
        .env(PROBE_SENTINEL_ENV, &sentinel);
    let output = run_with_timeout(&mut command, input.bytes(), timeout, &evidence)
        .map_err(|error| format!("{surface} probe through {binary}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{surface} probe exited {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout: JsonValue = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("parse {surface} probe stdout as JSON: {error}"))?;
    if stdout != serde_json::json!({}) {
        return Err(format!(
            "{surface} probe expected {{}} stdout, got {stdout}"
        ));
    }

    let invocations_dir = probe_dir.join("invocations");
    let invocations = sorted_entries(&invocations_dir)?
        .into_iter()
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    if invocations.len() != 1 {
        return Err(format!(
            "{surface} probe expected exactly one invocation, observed {} at {invocations_dir:?}",
            invocations.len()
        ));
    }
    let record = invocations[0].path();
    assert_record(&record, "program", probe.to_string_lossy().as_ref())?;
    assert_record(
        &record,
        "cwd",
        canonical_project(&project).to_string_lossy().trim_end(),
    )?;
    assert_record(&record, "sentinel", &sentinel)?;
    assert_record(&record, "argc", "2")?;
    assert_record(&record, "argv-0", "--fixture-contract")?;
    assert_record(
        &record,
        "argv-1",
        canonical_project(&target).to_string_lossy().as_ref(),
    )?;
    Ok(1)
}

fn write_probe_config(project: &Path, probe: &Path) -> Result<PathBuf, String> {
    let config_dir = project.join(".velvet-glove");
    std::fs::create_dir_all(&config_dir)
        .map_err(|error| format!("create probe config directory {config_dir:?}: {error}"))?;
    let probe = pkl_string(probe.to_string_lossy().as_ref());
    let config = format!(
        r#"amends "Config.pkl"

settings {{
  jobs = 1
  fileActivity {{ filesystemMtime = false }}
}}

tools {{
  ["fixture-probe"] = new ToolSpec {{
    id = "fixture-probe"
    displayName = "fixture probe"
    executable = "{probe}"
    files {{ include = new Listing {{ "*.fixture"; "**/*.fixture" }} }}
    phases {{
      ["verify"] = new Phase {{
        mode = "verify"
        argv = new Listing {{ "--fixture-contract"; new Files {{}} }}
      }}
    }}
    phaseOrder = new Listing {{ "verify" }}
  }}
}}
run = new Listing {{ "fixture-probe" }}
"#
    );
    let path = config_dir.join("post-tool-use.pkl");
    std::fs::write(&path, config).map_err(|error| format!("write probe config: {error}"))?;
    Ok(path)
}

fn assert_record(record: &Path, name: &str, expected: &str) -> Result<(), String> {
    let path = record.join(name);
    let actual = std::fs::read_to_string(&path)
        .map_err(|error| format!("read probe record {path:?}: {error}"))?;
    if actual.trim_end() == expected {
        Ok(())
    } else {
        Err(format!(
            "probe {name} mismatch: expected {expected:?}, got {:?}",
            actual.trim_end()
        ))
    }
}

fn pkl_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn require_pkl(timeout: Duration) -> Result<(), String> {
    let root = unique_temp_dir("velvet-glove-pkl-prerequisite");
    let mut command = Command::new("pkl");
    command.arg("--version");
    let result = run_with_timeout(&mut command, &[], timeout, &root);
    let _ = std::fs::remove_dir_all(&root);
    let output = result.map_err(|error| format!("required Pkl 0.31.1 unavailable: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "required Pkl prerequisite failed with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let version = String::from_utf8_lossy(&output.stdout);
    if !version.starts_with("Pkl 0.31.1 ") {
        return Err(format!(
            "required Pkl version is 0.31.1; found {}",
            version.trim()
        ));
    }
    Ok(())
}

fn configured_timeout() -> Result<Duration, String> {
    let Some(value) = std::env::var_os(TIMEOUT_ENV) else {
        return Ok(Duration::from_secs(DEFAULT_TIMEOUT_SECS));
    };
    let value = value
        .into_string()
        .map_err(|_| format!("{TIMEOUT_ENV} must be UTF-8"))?;
    let seconds = value
        .parse::<u64>()
        .map_err(|error| format!("invalid {TIMEOUT_ENV}={value:?}: {error}"))?;
    if seconds == 0 {
        return Err(format!("{TIMEOUT_ENV} must be greater than zero"));
    }
    Ok(Duration::from_secs(seconds))
}

fn configured_artifact_dir() -> Result<Option<PathBuf>, String> {
    match std::env::var_os(ARTIFACT_ENV) {
        None => Ok(None),
        Some(value) if value.is_empty() => Err(format!("{ARTIFACT_ENV} must not be empty")),
        Some(value) => {
            let path = PathBuf::from(value);
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|error| format!("resolve {ARTIFACT_ENV}: {error}"))?
                    .join(path)
            };
            std::fs::create_dir_all(&path)
                .map_err(|error| format!("create {ARTIFACT_ENV} {path:?}: {error}"))?;
            Ok(Some(path))
        }
    }
}

fn build_report(
    catalog: &FixtureCatalog,
    outcomes: &[FixtureOutcome],
    probe_commands: usize,
) -> JsonValue {
    let mut passed = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut skip_reasons = BTreeMap::<&str, usize>::new();
    let mut by_surface = BTreeMap::<&str, [usize; 3]>::new();
    for outcome in outcomes {
        let counts = by_surface
            .entry(outcome.surface.cli_name())
            .or_insert([0, 0, 0]);
        match &outcome.status {
            FixtureStatus::Pass => {
                passed += 1;
                counts[0] += 1;
            }
            FixtureStatus::Skip(reason) => {
                skipped += 1;
                counts[1] += 1;
                *skip_reasons.entry(reason.code).or_default() += 1;
            }
            FixtureStatus::Fail(_) => {
                failed += 1;
                counts[2] += 1;
            }
        }
    }
    let surface_totals = by_surface
        .into_iter()
        .map(|(surface, counts)| {
            (
                surface.to_owned(),
                serde_json::json!({
                    "passed": counts[0],
                    "skipped": counts[1],
                    "failed": counts[2],
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({
        "formatVersion": 1,
        "kind": "real-tool-fixtures",
        "totals": {
            "tools": catalog.tool_count,
            "cases": catalog.cases.len(),
            "fixtureSurfaces": REAL_TOOL_SURFACES.len(),
            "protocolProbeSurfaces": ProtocolSurface::ALL.len(),
            "plannedSurfaceCases": catalog.cases.len() * REAL_TOOL_SURFACES.len(),
            "attemptedSurfaceCases": passed + failed,
            "passed": passed,
            "skipped": skipped,
            "failed": failed,
            "probeCommandsExecuted": probe_commands,
        },
        "bySurface": surface_totals,
        "skipReasons": skip_reasons,
        "outcomes": outcomes.iter().map(FixtureOutcome::as_json).collect::<Vec<_>>(),
    })
}

fn print_outcomes(outcomes: &[FixtureOutcome]) {
    for outcome in outcomes {
        match &outcome.status {
            FixtureStatus::Pass => println!(
                "PASS  {}/{} ({})",
                outcome.tool, outcome.case, outcome.surface
            ),
            FixtureStatus::Skip(reason) => println!(
                "SKIP  {}/{} ({}): {} ({})",
                outcome.tool, outcome.case, outcome.surface, reason.detail, reason.code
            ),
            FixtureStatus::Fail(reason) => {
                eprintln!(
                    "FAIL  {}/{} ({}):\n{reason}",
                    outcome.tool, outcome.case, outcome.surface
                );
                if let Some(path) = &outcome.artifacts {
                    eprintln!("retained artifacts: {}", path.display());
                }
            }
        }
    }
}

fn retain_failure(
    source: &Path,
    artifact_root: &Path,
    case: &FixtureCase,
    surface: ProtocolSurface,
) -> Result<PathBuf, String> {
    let destination = artifact_root
        .join(sanitize_component(&case.tool))
        .join(sanitize_component(&case.case))
        .join(format!(
            "{}-{}-{}",
            surface.cli_name(),
            std::process::id(),
            unique_nonce()
        ));
    copy_tree(source, &destination).map_err(|error| {
        format!(
            "retain requested failure artifacts at {destination:?}: {error}; temporary evidence was {source:?}"
        )
    })?;
    Ok(destination)
}

fn retain_probe_failure(
    source: &Path,
    artifact_root: &Path,
    surface: ProtocolSurface,
) -> Result<PathBuf, String> {
    let destination = artifact_root
        .join("probe")
        .join(surface.cli_name())
        .join(format!("{}-{}", std::process::id(), unique_nonce()));
    copy_tree(source, &destination).map_err(|error| {
        format!(
            "retain requested probe artifacts at {destination:?}: {error}; temporary evidence is {source:?}"
        )
    })?;
    Ok(destination)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination)
        .map_err(|error| format!("create {destination:?}: {error}"))?;
    for entry in sorted_entries(source)? {
        let path = entry.path();
        let target = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file type for {path:?}: {error}"))?;
        if file_type.is_dir() {
            copy_tree(&path, &target)?;
        } else if file_type.is_file() {
            std::fs::copy(&path, &target)
                .map_err(|error| format!("copy {path:?} to {target:?}: {error}"))?;
        } else {
            return Err(format!("cannot retain unsupported entry {path:?}"));
        }
    }
    Ok(())
}

fn write_report(root: &Path, report: &JsonValue) -> Result<PathBuf, String> {
    let historical_path = root.join(format!(
        "report-{}-{}.json",
        std::process::id(),
        unique_nonce()
    ));
    write_json(&historical_path, report)?;
    let stable_path = root.join("report.json");
    write_json(&stable_path, report)?;
    Ok(stable_path)
}

fn write_json(path: &Path, value: &JsonValue) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("serialize JSON for {path:?}: {error}"))?;
    std::fs::write(path, bytes).map_err(|error| format!("write {path:?}: {error}"))
}

fn append_failure(outcome: &mut FixtureOutcome, extra: String) {
    match &mut outcome.status {
        FixtureStatus::Fail(reason) => {
            reason.push('\n');
            reason.push_str(&extra);
        }
        FixtureStatus::Pass | FixtureStatus::Skip(_) => {
            outcome.status = FixtureStatus::Fail(extra);
        }
    }
}

fn normalize(text: &str, project_aliases: &[String]) -> String {
    let mut output = text.to_owned();
    let mut aliases = project_aliases.iter().collect::<Vec<_>>();
    aliases.sort_by_key(|alias| std::cmp::Reverse(alias.len()));
    for alias in aliases {
        output = output.replace(alias, "<workspace>");
    }
    let mut contextlint_cli_aliases = contextlint_cli_path_aliases();
    contextlint_cli_aliases.sort_by_key(|alias| std::cmp::Reverse(alias.len()));
    for alias in contextlint_cli_aliases {
        output = output.replace(&alias, "contextlint");
    }
    let mut node_module_aliases = node_module_path_aliases();
    node_module_aliases.sort_by_key(|alias| std::cmp::Reverse(alias.len()));
    for alias in node_module_aliases {
        output = output.replace(&alias, "<node_modules>");
    }
    let mut prettier_cli_aliases = prettier_cli_path_aliases();
    prettier_cli_aliases.sort_by_key(|alias| std::cmp::Reverse(alias.len()));
    for alias in prettier_cli_aliases {
        output = output.replace(&alias, "<prettier-cli>");
    }
    let mut eslint_cli_aliases = eslint_cli_path_aliases();
    eslint_cli_aliases.sort_by_key(|alias| std::cmp::Reverse(alias.len()));
    for alias in eslint_cli_aliases {
        output = output.replace(&alias, "<eslint-cli>");
    }
    output = normalize_prettier_adapter_commands(output);
    output
        .split('\n')
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_fixture_output(case: &FixtureCase, text: &str, project_aliases: &[String]) -> String {
    let mut output = normalize(text, project_aliases);
    let inline_marker = match case.tool.as_str() {
        "go-fmt" => Some(GOFMT_FILES_MARKER),
        "dclint" => Some(DCLINT_FILES_MARKER),
        _ => None,
    };
    if let Some(inline_marker) = inline_marker {
        let adapter_scripts = case
            .spec
            .phases
            .values()
            .flat_map(|phase| phase.argv.iter())
            .filter_map(|argument| match argument {
                ArgvElement::Literal(script)
                    if script.contains(inline_marker) && script.contains('\n') =>
                {
                    Some(script)
                }
                ArgvElement::Literal(_) | ArgvElement::Token(_) => None,
            })
            .collect::<BTreeSet<_>>();
        for script in adapter_scripts {
            output = output.replace(script, "<inline-script>");
        }
    }
    if case.tool == "contextlint" {
        let adapter_scripts = case
            .spec
            .phases
            .values()
            .flat_map(|phase| phase.argv.iter())
            .filter_map(|argument| match argument {
                ArgvElement::Literal(script)
                    if script.contains(CONTEXTLINT_FILES_MARKER) && script.contains('\n') =>
                {
                    Some(script)
                }
                ArgvElement::Literal(_) | ArgvElement::Token(_) => None,
            })
            .collect::<BTreeSet<_>>();
        for script in adapter_scripts {
            output = output.replace(script, "<inline-script>");
        }
    }
    if case.tool == "eslint" {
        let adapter_scripts = case
            .spec
            .phases
            .values()
            .flat_map(|phase| phase.argv.iter())
            .filter_map(|argument| match argument {
                ArgvElement::Literal(script)
                    if script.contains(ESLINT_FILES_MARKER) && script.contains('\n') =>
                {
                    Some(script)
                }
                ArgvElement::Literal(_) | ArgvElement::Token(_) => None,
            })
            .collect::<BTreeSet<_>>();
        for script in adapter_scripts {
            output = output.replace(script, "<eslint-adapter>");
        }
    }
    if case.tool == "ghalint-workflow" {
        let adapter_scripts = case
            .spec
            .phases
            .values()
            .flat_map(|phase| phase.argv.iter())
            .filter_map(|argument| match argument {
                ArgvElement::Literal(script)
                    if script.contains(GHALINT_WORKFLOW_FILES_MARKER) && script.contains('\n') =>
                {
                    Some(script)
                }
                ArgvElement::Literal(_) | ArgvElement::Token(_) => None,
            })
            .collect::<BTreeSet<_>>();
        for script in adapter_scripts {
            output = output.replace(script, "<inline-script>");
        }
    }
    output
}

fn prettier_cli_path_aliases() -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(root) = std::env::var_os(PRETTIER_ROOT_ENV) {
        paths.push(PathBuf::from(root).join("package/node_modules/prettier/bin/prettier.cjs"));
    }
    if let Some(path) = resolve_program("prettier") {
        paths.push(path);
    }

    let mut aliases = Vec::new();
    for path in paths {
        let rendered = path.to_string_lossy().into_owned();
        if !rendered.is_empty() && !aliases.contains(&rendered) {
            aliases.push(rendered);
        }
        if let Ok(canonical) = path.canonicalize() {
            let canonical = canonical.to_string_lossy().into_owned();
            if !aliases.contains(&canonical) {
                aliases.push(canonical);
            }
        }
    }
    aliases
}

fn contextlint_cli_path_aliases() -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(root) = std::env::var_os(CONTEXTLINT_ROOT_ENV) {
        paths.push(PathBuf::from(root).join("package/node_modules/@contextlint/cli/dist/index.js"));
    }
    if let Ok(path) = resolve_contextlint_fixture_cli() {
        paths.push(path);
    }

    let mut aliases = Vec::new();
    for path in paths {
        let rendered = path.to_string_lossy().into_owned();
        if !rendered.is_empty() && !aliases.contains(&rendered) {
            aliases.push(rendered);
        }
        if let Ok(canonical) = path.canonicalize() {
            let canonical = canonical.to_string_lossy().into_owned();
            if !aliases.contains(&canonical) {
                aliases.push(canonical);
            }
        }
    }
    aliases
}

fn eslint_cli_path_aliases() -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(root) = std::env::var_os(ESLINT_ROOT_ENV) {
        paths.push(PathBuf::from(root).join("package/node_modules/eslint/bin/eslint.js"));
    }
    if let Some(path) = resolve_program("eslint") {
        paths.push(path);
    }

    let mut aliases = Vec::new();
    for path in paths {
        let rendered = path.to_string_lossy().into_owned();
        if !rendered.is_empty() && !aliases.contains(&rendered) {
            aliases.push(rendered);
        }
        if let Ok(canonical) = path.canonicalize() {
            let canonical = canonical.to_string_lossy().into_owned();
            if !aliases.contains(&canonical) {
                aliases.push(canonical);
            }
        }
    }
    aliases
}

fn normalize_prettier_adapter_commands(mut output: String) -> String {
    for phase in ["format", "verify"] {
        let start_marker = format!("[{phase}] command: python -I -c ");
        let end_marker = format!(" node <prettier-cli> {phase} ");
        let mut search_from = 0;
        while let Some(relative_start) = output[search_from..].find(&start_marker) {
            let script_start = search_from + relative_start + start_marker.len();
            let Some(relative_end) = output[script_start..].find(&end_marker) else {
                break;
            };
            let script_end = script_start + relative_end;
            output.replace_range(script_start..script_end, "<prettier-adapter>");
            search_from = script_start + "<prettier-adapter>".len() + end_marker.len();
        }
    }
    output
}

fn verify_tool_output_is_canonical(tool: &str, context: &str, text: &str) -> Result<(), String> {
    if tool == "buf-format" {
        for (line_index, line) in text.lines().enumerate() {
            if !(line.starts_with("--- ") || line.starts_with("+++ ")) {
                continue;
            }
            let Some((_, mtime)) = line.rsplit_once('\t') else {
                return Err(format!(
                    "buf-format {context} retained a diff header without a canonical mtime on line {}: {line:?}",
                    line_index + 1
                ));
            };
            if mtime != "<mtime>" {
                return Err(format!(
                    "buf-format {context} retained a dynamic diff mtime on line {} ({mtime:?}); the adapter must emit <mtime> before the harness captures output",
                    line_index + 1
                ));
            }
        }
        return Ok(());
    }
    if tool == "betterleaks" {
        const FATAL_SEPARATOR: &str = " FTL ";
        for (line_index, line) in text.lines().enumerate() {
            let Some((timestamp, _)) = line.split_once(FATAL_SEPARATOR) else {
                continue;
            };
            if is_betterleaks_console_time(timestamp) {
                return Err(format!(
                    "betterleaks {context} retained a dynamic console clock on line {} ({timestamp:?}); the adapter must emit <time> FTL before the harness captures output",
                    line_index + 1
                ));
            }
        }
    }
    Ok(())
}

fn is_betterleaks_console_time(value: &str) -> bool {
    let Some(time) = value
        .strip_suffix("AM")
        .or_else(|| value.strip_suffix("PM"))
    else {
        return false;
    };
    let Some((hour, minute)) = time.split_once(':') else {
        return false;
    };
    if hour.is_empty()
        || hour.len() > 2
        || minute.len() != 2
        || !hour.bytes().all(|byte| byte.is_ascii_digit())
        || !minute.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    matches!(hour.parse::<u8>(), Ok(1..=12)) && matches!(minute.parse::<u8>(), Ok(0..=59))
}

fn node_module_path_aliases() -> Vec<String> {
    let mut aliases = Vec::new();
    for path in std::env::var_os(NODE_PATH_ENV)
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
    {
        let path = path.to_string_lossy().into_owned();
        if !path.is_empty() && !aliases.contains(&path) {
            aliases.push(path.clone());
        }
        if let Ok(canonical) = Path::new(&path).canonicalize() {
            let canonical = canonical.to_string_lossy().into_owned();
            if !aliases.contains(&canonical) {
                aliases.push(canonical);
            }
        }
    }
    aliases
}

fn workspace_path_aliases(project: &Path) -> Vec<String> {
    let mut aliases = vec![project.to_string_lossy().into_owned()];
    if let Ok(canonical) = project.canonicalize() {
        let canonical = canonical.to_string_lossy().into_owned();
        if !aliases.contains(&canonical) {
            aliases.push(canonical);
        }
    }
    aliases
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tool-fixtures")
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    loop {
        let candidate = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            unique_nonce()
        ));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create temporary directory {candidate:?}: {error}"),
        }
    }
}

fn unique_nonce() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{timestamp}-{counter}")
}
