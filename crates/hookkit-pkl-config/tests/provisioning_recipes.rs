use hookkit_pkl_config::{Architecture, NetworkPolicy, Platform, SupportState, UpstreamProvenance};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const RECIPES_JSON: &str = include_str!("../validation/provisioning/recipes.json");
const GHALINT_SOURCE_BUILD_JSON: &str =
    include_str!("../validation/provisioning/ghalint-workflow/source-build.json");
const GOLINES_SOURCE_BUILD_JSON: &str =
    include_str!("../validation/provisioning/golines/source-build.json");
const VACUUM_PROVENANCE_JSON: &str =
    include_str!("../validation/provisioning/vacuum/provenance.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registry {
    #[serde(rename = "$schema")]
    schema: String,
    schema_version: u32,
    mise: Mise,
    controlled_environment: ControlledEnvironment,
    shared_components: Vec<Component>,
    shared_bootstrap: Vec<Bootstrap>,
    environments: Vec<Environment>,
    recipes: Vec<Recipe>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Mise {
    required_version: String,
    config: String,
    lock: String,
    provisioning_network: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ControlledEnvironment {
    clear_inherited: bool,
    home: String,
    seeded_variables: Vec<String>,
    set: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Component {
    id: String,
    version: String,
    mise_tool: Option<String>,
    installation_source: String,
    #[serde(default)]
    runtime_component_ids: Vec<String>,
    #[serde(default)]
    install_components: Vec<String>,
    integrity: Integrity,
    probe: Probe,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Integrity {
    kind: String,
    path: Option<String>,
    component_id: Option<String>,
    url: Option<String>,
    sha256: Option<String>,
    patch_sha256: Option<String>,
    module_manifest_path: Option<String>,
    module_manifest_sha256: Option<String>,
    module_lock_path: Option<String>,
    module_lock_sha256: Option<String>,
    built_artifact_sha256: Option<String>,
    build_toolchain_component_id: Option<String>,
    build_working_directory: Option<String>,
    #[serde(default)]
    build_argv: Vec<String>,
    #[serde(default)]
    build_environment: BTreeMap<String, String>,
    archive_format: Option<String>,
    archive_root: Option<String>,
    min_os_version: Option<String>,
    #[serde(default)]
    allowed_dylib_prefixes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Probe {
    argv: Vec<String>,
    #[serde(rename = "match")]
    match_kind: String,
    expected: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Bootstrap {
    id: String,
    argv: Vec<String>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    working_directory: Option<String>,
    network: String,
    lockfile: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Environment {
    id: String,
    provisioning_group: String,
    platform: String,
    architecture: String,
    os_version_constraint: String,
    sandbox: String,
    case_network: String,
    components: Vec<Component>,
    auxiliary_programs: Vec<String>,
    bootstrap: Vec<Bootstrap>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Recipe {
    id: String,
    tool_id: String,
    environment_id: String,
    version: String,
    installation_source: String,
    integrity: Integrity,
    probe: Probe,
    case_executables: Vec<String>,
    cases: Vec<String>,
    representative_case: String,
}

struct TestDirectory(PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn test_directory(name: &str) -> TestDirectory {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "velvet-glove-provisioning-{name}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&path).expect("create provisioning test directory");
    TestDirectory(path)
}

#[cfg(unix)]
fn write_test_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("write cache test executable");
    let mut permissions = std::fs::metadata(path)
        .expect("cache test executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("mark cache test executable executable");
}

#[test]
#[cfg(unix)]
fn cargo_toolchain_component_set_cache_ignores_the_legacy_root() {
    let repository = repository_root();
    let registry: serde_json::Value =
        serde_json::from_str(RECIPES_JSON).expect("provisioning registry JSON");
    let component = registry["environments"]
        .as_array()
        .expect("environments")
        .iter()
        .flat_map(|environment| {
            environment["components"]
                .as_array()
                .expect("environment components")
        })
        .find(|component| component["id"] == "cargo-clippy-toolchain")
        .expect("cargo-clippy toolchain component");
    let archive_integrity = serde_json::to_string(&component["integrity"])
        .expect("serialize cargo-clippy archive integrity");
    let installed_components = serde_json::to_string(&component["installComponents"])
        .expect("serialize installed component set");
    let legacy_identity = format!(
        "{{\"id\":\"cargo-clippy-toolchain\",\"version\":\"1.97.1\",\"integrity\":{archive_integrity}}}"
    );
    let component_set_identity = format!(
        "{{\"integrity\":{legacy_identity},\"installedComponents\":{installed_components}}}"
    );
    assert!(component_set_identity.contains("rustfmt-preview"));

    let temporary = test_directory("legacy-cargo-toolchain-cache");
    let state = &temporary.0;
    let legacy_root = state.join("cargo-clippy-toolchain-1.97.1");
    std::fs::create_dir_all(legacy_root.join("bin")).expect("legacy bin directory");
    std::fs::write(
        legacy_root.join(".velvet-glove-artifacts.json"),
        format!("{legacy_identity}\n"),
    )
    .expect("legacy archive-only cache stamp");
    for executable in ["cargo", "cargo-clippy", "clippy-driver", "rustc", "rustdoc"] {
        write_test_executable(&legacy_root.join("bin").join(executable));
    }
    assert!(!legacy_root.join("bin/cargo-fmt").exists());
    assert!(!legacy_root.join("bin/rustfmt").exists());

    let helper = repository.join("scripts/pinned-tool-cache.sh");
    let required = [
        "bin/cargo",
        "bin/cargo-clippy",
        "bin/cargo-fmt",
        "bin/clippy-driver",
        "bin/rustc",
        "bin/rustdoc",
        "bin/rustfmt",
    ];
    let shell = r#"
source "$1"
state=$2
identity=$3
legacy=$4
shift 4
root=$(pinned_component_cache_root "$state" cargo-clippy-toolchain-1.97.1 "$identity")
printf '%s\n' "$root"
if pinned_component_cache_valid "$legacy" "$identity" "$@"; then
  exit 20
fi
if pinned_component_cache_valid "$root" "$identity" "$@"; then
  printf 'valid\n'
else
  printf 'install-required\n'
fi
"#;
    let invoke_helper = |expected_state: &str| {
        let mut command = Command::new("/bin/bash");
        command
            .args(["-c", shell, "cache-regression"])
            .arg(&helper)
            .arg(state)
            .arg(&component_set_identity)
            .arg(&legacy_root)
            .args(required);
        let output = command.output().expect("execute exact cache helper");
        assert!(
            output.status.success(),
            "cache helper failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 cache helper output");
        assert!(stdout.ends_with(expected_state), "cache helper: {stdout:?}");
        PathBuf::from(stdout.lines().next().expect("qualified cache root"))
    };

    let qualified_root = invoke_helper("install-required\n");
    assert_ne!(qualified_root, legacy_root);
    assert!(!qualified_root.exists());

    std::fs::create_dir_all(qualified_root.join("bin")).expect("qualified cache bin directory");
    std::fs::write(
        qualified_root.join(".velvet-glove-artifacts.json"),
        format!("{component_set_identity}\n"),
    )
    .expect("component-set cache stamp");
    for executable in required.iter().map(|path| path.trim_start_matches("bin/")) {
        write_test_executable(&qualified_root.join("bin").join(executable));
    }
    assert_eq!(invoke_helper("valid\n"), qualified_root);

    let outer = std::fs::read_to_string(repository.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    let inner =
        std::fs::read_to_string(repository.join("scripts/run-pinned-tool-contract-inner.sh"))
            .expect("inner pinned runner");
    for (name, script) in [("outer", outer.as_str()), ("inner", inner.as_str())] {
        assert!(
            script.contains("source \"$cache_helpers\""),
            "{name} shared helper"
        );
        assert!(
            script.contains("pinned_component_install_identity"),
            "{name} canonical install identity"
        );
        assert!(
            script.contains("pinned_component_cache_root"),
            "{name} identity-qualified cache root"
        );
        assert!(
            !script.contains("$state_dir/cargo-clippy-toolchain-1.97.1\""),
            "{name} must not select the legacy root"
        );
    }
    assert!(outer.contains("if [[ ! -d $clippy_root ]]; then"));
    assert!(inner.contains(
        "VELVET_GLOVE_FIXTURE_CARGO_CLIPPY_TOOLCHAIN_ROOT=$(pinned_component_cache_root"
    ));
}

#[test]
fn representative_provisioning_recipes_are_complete_and_cross_linked() {
    let registry: Registry = serde_json::from_str(RECIPES_JSON).expect("strict recipe registry");
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../validation/provisioning/recipes.schema.json"
    ))
    .expect("recipe schema JSON");
    assert_eq!(registry.schema, "./recipes.schema.json");
    assert_eq!(registry.schema_version, 1);
    assert_eq!(schema["$defs"]["recipe"]["type"], "object");

    let root = repository_root();
    assert_eq!(registry.mise.required_version, "2026.5.15");
    assert_eq!(registry.mise.provisioning_network, "required");
    assert_file(&root, &registry.mise.config);
    assert_file(&root, &registry.mise.lock);

    assert!(registry.controlled_environment.clear_inherited);
    assert_eq!(
        registry.controlled_environment.home,
        "neutral-system-temporary"
    );
    assert_eq!(
        registry
            .controlled_environment
            .seeded_variables
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["HOME", "LANG", "PATH", "SHELL", "TERM", "USER"])
    );
    assert_eq!(
        registry.controlled_environment.set.get("LC_ALL"),
        Some(&"C".to_owned())
    );
    assert_eq!(
        registry.controlled_environment.set.get("TZ"),
        Some(&"UTC".to_owned())
    );

    let shared_ids = validate_components(&root, &registry.shared_components, "shared");
    assert_eq!(
        shared_ids,
        BTreeSet::from([
            "apple-clang",
            "jq",
            "macos-sdk",
            "pkl",
            "python",
            "rust",
            "xcode"
        ])
    );
    validate_bootstrap(&root, &registry.shared_bootstrap, "shared");
    assert!(
        registry
            .shared_bootstrap
            .iter()
            .any(|step| step.id == "cargo-fetch")
    );
    let cargo_fetch = registry
        .shared_bootstrap
        .iter()
        .find(|step| step.id == "cargo-fetch")
        .expect("Cargo bootstrap");
    assert_eq!(cargo_fetch.working_directory.as_deref(), Some("/"));
    assert_eq!(
        cargo_fetch.environment.get("CARGO_TARGET_DIR"),
        Some(&"{state}/cargo-target".to_owned())
    );

    let manifest = hookkit_pkl_config::builtin_validation_manifest().expect("builtin manifest");
    let tools = manifest
        .tools
        .iter()
        .map(|tool| (tool.id.as_str(), tool))
        .collect::<BTreeMap<_, _>>();
    let mut environment_ids = BTreeSet::new();
    let mut environments = BTreeMap::new();
    let mut groups = BTreeSet::new();
    for environment in &registry.environments {
        assert!(
            environment_ids.insert(environment.id.as_str()),
            "duplicate environment id {}",
            environment.id
        );
        assert_eq!(environment.platform, "macos", "{} platform", environment.id);
        assert_eq!(
            environment.architecture, "aarch64",
            "{} architecture",
            environment.id
        );
        assert_eq!(environment.os_version_constraint, ">=26");
        assert_eq!(environment.sandbox, "mise-deny-net");
        assert_eq!(environment.case_network, "denied");
        validate_components(&root, &environment.components, &environment.id);
        validate_bootstrap(&root, &environment.bootstrap, &environment.id);
        assert_unique(
            environment.auxiliary_programs.iter().map(String::as_str),
            &format!("{} auxiliary programs", environment.id),
        );
        groups.insert(environment.provisioning_group.as_str());
        environments.insert(environment.id.as_str(), environment);
    }
    let shared_node_environment = environments
        .get("macos-arm64-node")
        .expect("shared Node environment");
    assert_eq!(
        shared_node_environment
            .components
            .iter()
            .find(|component| component.id == "node")
            .expect("shared Node component")
            .version,
        "24.18.0",
        "Prettier provisioning must not churn the shared Node closure"
    );
    let prettier_environment = environments
        .get("macos-arm64-prettier")
        .expect("dedicated Prettier environment");
    assert_eq!(prettier_environment.provisioning_group, "prettier");
    assert_eq!(
        prettier_environment
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["prettier-node", "prettier-npm"])
    );
    assert_eq!(
        prettier_environment
            .auxiliary_programs
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["prettier-node", "prettier-npm"])
    );
    let prettier_bootstrap = prettier_environment
        .bootstrap
        .iter()
        .find(|step| step.id == "prettier-npm-ci")
        .expect("dedicated Prettier npm bootstrap");
    assert_eq!(prettier_bootstrap.network, "required");
    assert_eq!(
        prettier_bootstrap.lockfile.as_deref(),
        Some("crates/hookkit-pkl-config/validation/provisioning/prettier/package-lock.json")
    );
    assert_eq!(
        prettier_bootstrap.argv,
        [
            "{state}/prettier-environment-node-24.19.0-prettier-3.9.6/node/bin/node",
            "{state}/prettier-environment-node-24.19.0-prettier-3.9.6/node/lib/node_modules/npm/bin/npm-cli.js",
            "ci",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--prefix",
            "{state}/prettier-environment-node-24.19.0-prettier-3.9.6/package",
        ]
    );
    assert_eq!(
        prettier_bootstrap.environment,
        BTreeMap::from([
            (
                "NPM_CONFIG_GLOBALCONFIG".to_owned(),
                "{home}/npm-globalconfig".to_owned()
            ),
            ("NPM_CONFIG_USERCONFIG".to_owned(), "/dev/null".to_owned()),
        ])
    );
    let contextlint_environment = environments
        .get("macos-arm64-contextlint")
        .expect("dedicated Contextlint environment");
    assert_eq!(contextlint_environment.provisioning_group, "contextlint");
    let vacuum_environment = environments
        .get("macos-arm64-vacuum")
        .expect("dedicated Vacuum environment");
    assert_eq!(vacuum_environment.provisioning_group, "data-formats");
    assert_eq!(
        vacuum_environment
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["vacuum"])
    );
    assert!(vacuum_environment.auxiliary_programs.is_empty());
    assert!(vacuum_environment.bootstrap.is_empty());
    assert_eq!(
        contextlint_environment
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["contextlint-node", "contextlint-npm"])
    );
    assert_eq!(
        contextlint_environment
            .auxiliary_programs
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["contextlint-node", "contextlint-npm"])
    );
    let contextlint_bootstrap = contextlint_environment
        .bootstrap
        .iter()
        .find(|step| step.id == "contextlint-npm-ci")
        .expect("dedicated Contextlint npm bootstrap");
    assert_eq!(contextlint_bootstrap.network, "required");
    assert_eq!(
        contextlint_bootstrap.lockfile.as_deref(),
        Some("crates/hookkit-pkl-config/validation/provisioning/contextlint/package-lock.json")
    );
    assert_eq!(
        contextlint_bootstrap.argv,
        [
            "{state}/contextlint-environment-node-24.19.0-contextlint-1.1.1/node/bin/node",
            "{state}/contextlint-environment-node-24.19.0-contextlint-1.1.1/node/lib/node_modules/npm/bin/npm-cli.js",
            "ci",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--prefix",
            "{state}/contextlint-environment-node-24.19.0-contextlint-1.1.1/package",
        ]
    );
    let dclint_environment = environments
        .get("macos-arm64-dclint")
        .expect("dedicated dclint environment");
    assert_eq!(dclint_environment.provisioning_group, "dclint");
    assert_eq!(
        dclint_environment
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["dclint-node", "dclint-npm"])
    );
    assert_eq!(
        dclint_environment
            .auxiliary_programs
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["dclint-node", "dclint-npm"])
    );
    let dclint_bootstrap = dclint_environment
        .bootstrap
        .iter()
        .find(|step| step.id == "dclint-npm-ci")
        .expect("dedicated dclint npm bootstrap");
    assert_eq!(dclint_bootstrap.network, "required");
    assert_eq!(
        dclint_bootstrap.lockfile.as_deref(),
        Some("crates/hookkit-pkl-config/validation/provisioning/dclint/package-lock.json")
    );
    assert_eq!(
        dclint_bootstrap.argv,
        [
            "{state}/dclint-environment-node-24.19.0-dclint-3.1.0/node/bin/node",
            "{state}/dclint-environment-node-24.19.0-dclint-3.1.0/node/lib/node_modules/npm/bin/npm-cli.js",
            "ci",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--prefix",
            "{state}/dclint-environment-node-24.19.0-dclint-3.1.0/package",
        ]
    );
    assert_eq!(dclint_bootstrap.environment, prettier_bootstrap.environment);
    let eslint_environment = environments
        .get("macos-arm64-eslint")
        .expect("dedicated ESLint environment");
    assert_eq!(eslint_environment.provisioning_group, "eslint");
    assert_eq!(
        eslint_environment
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["eslint-node", "eslint-npm"])
    );
    assert_eq!(
        eslint_environment
            .auxiliary_programs
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["eslint-node", "eslint-npm"])
    );
    let eslint_bootstrap = eslint_environment
        .bootstrap
        .iter()
        .find(|step| step.id == "eslint-npm-ci")
        .expect("dedicated ESLint npm bootstrap");
    assert_eq!(eslint_bootstrap.network, "required");
    assert_eq!(
        eslint_bootstrap.lockfile.as_deref(),
        Some("crates/hookkit-pkl-config/validation/provisioning/eslint/package-lock.json")
    );
    assert_eq!(
        eslint_bootstrap.argv,
        [
            "{state}/eslint-environment-node-24.19.0-eslint-10.8.1/node/bin/node",
            "{state}/eslint-environment-node-24.19.0-eslint-10.8.1/node/lib/node_modules/npm/bin/npm-cli.js",
            "ci",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--prefix",
            "{state}/eslint-environment-node-24.19.0-eslint-10.8.1/package",
        ]
    );
    assert_eq!(eslint_bootstrap.environment, prettier_bootstrap.environment);
    let eslint_package: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            root.join("crates/hookkit-pkl-config/validation/provisioning/eslint/package.json"),
        )
        .expect("ESLint package manifest"),
    )
    .expect("ESLint package manifest JSON");
    assert_eq!(eslint_package["engines"]["node"], "24.19.0");
    assert_eq!(eslint_package["engines"]["npm"], "11.17.0");
    assert_eq!(eslint_package["dependencies"]["eslint"], "10.8.1");
    let eslint_lock: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            root.join("crates/hookkit-pkl-config/validation/provisioning/eslint/package-lock.json"),
        )
        .expect("ESLint package lock"),
    )
    .expect("ESLint package lock JSON");
    assert_eq!(eslint_lock["lockfileVersion"], 3);
    assert_eq!(
        eslint_lock["packages"]["node_modules/eslint"]["version"],
        "10.8.1"
    );
    assert_eq!(
        eslint_lock["packages"]["node_modules/eslint"]["resolved"],
        "https://registry.npmjs.org/eslint/-/eslint-10.8.1.tgz"
    );
    assert_eq!(
        eslint_lock["packages"]["node_modules/eslint"]["integrity"],
        "sha512-wqA7W2jbsC/BnV9Iv1UZpKVFkO1AdNoSmYW8NWG4HNOBbkAMvIqDZ27pI2f07dqn583NcIC44ckjAcOXDL1QbQ=="
    );
    let github_actions_environment = environments
        .get("macos-arm64-github-actions")
        .expect("dedicated GitHub Actions environment");
    assert_eq!(
        github_actions_environment.provisioning_group,
        "github-actions"
    );
    assert_eq!(
        github_actions_environment
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["ghalint-workflow", "go"])
    );
    assert!(github_actions_environment.auxiliary_programs.is_empty());
    assert_eq!(
        github_actions_environment
            .bootstrap
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>(),
        [
            "ghalint-apply-closure",
            "ghalint-go-mod-download",
            "ghalint-go-mod-verify",
            "ghalint-go-build",
        ]
    );
    assert_eq!(github_actions_environment.bootstrap[0].network, "denied");
    assert_eq!(github_actions_environment.bootstrap[1].network, "required");
    assert_eq!(github_actions_environment.bootstrap[2].network, "denied");
    assert_eq!(github_actions_environment.bootstrap[3].network, "denied");
    let errcheck_environment = environments
        .get("macos-arm64-errcheck")
        .expect("dedicated errcheck environment");
    assert_eq!(errcheck_environment.provisioning_group, "go");
    assert_eq!(
        errcheck_environment
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["errcheck-go"])
    );
    assert_eq!(errcheck_environment.auxiliary_programs, ["go"]);
    let errcheck_bootstrap = errcheck_environment
        .bootstrap
        .iter()
        .find(|step| step.id == "errcheck-go-mod-download")
        .expect("errcheck Go module bootstrap");
    assert_eq!(errcheck_bootstrap.network, "required");
    assert_eq!(errcheck_bootstrap.working_directory.as_deref(), Some("/"));
    assert_eq!(
        errcheck_bootstrap.lockfile.as_deref(),
        Some("crates/hookkit-pkl-config/validation/provisioning/errcheck/go.sum")
    );
    assert_eq!(
        errcheck_bootstrap.argv,
        [
            "go",
            "-C",
            "{repository}/crates/hookkit-pkl-config/validation/provisioning/errcheck",
            "mod",
            "download",
            "all",
        ]
    );
    assert_eq!(
        errcheck_bootstrap.environment,
        BTreeMap::from([
            (
                "GOCACHE".to_owned(),
                "{state}/errcheck-bootstrap-go1.26.5-build-cache".to_owned()
            ),
            (
                "GOMODCACHE".to_owned(),
                "{state}/errcheck-go1.26.5-mod-cache".to_owned()
            ),
            ("GOPROXY".to_owned(), "https://proxy.golang.org".to_owned()),
            ("GOSUMDB".to_owned(), "sum.golang.org".to_owned()),
            ("GOTOOLCHAIN".to_owned(), "local".to_owned()),
        ])
    );
    let mise_lock = std::fs::read_to_string(root.join(&registry.mise.lock)).expect("mise lock");
    for component in registry.shared_components.iter().chain(
        registry
            .environments
            .iter()
            .flat_map(|environment| environment.components.iter()),
    ) {
        validate_component_integrity(&root, &mise_lock, component);
    }
    assert_eq!(
        groups,
        BTreeSet::from([
            "cargo-clippy",
            "contextlint",
            "data-formats",
            "dclint",
            "eslint",
            "github-actions",
            "go",
            "node",
            "prettier",
            "python",
            "ruby",
            "rust",
            "security",
            "swift",
        ])
    );

    let mut recipe_ids = BTreeSet::new();
    let mut recipe_tools = BTreeSet::new();
    for recipe in &registry.recipes {
        assert!(recipe_ids.insert(recipe.id.as_str()), "duplicate recipe id");
        assert!(
            recipe_tools.insert(recipe.tool_id.as_str()),
            "duplicate representative recipe for {}",
            recipe.tool_id
        );
        let environment = environments
            .get(recipe.environment_id.as_str())
            .unwrap_or_else(|| panic!("{} references unknown environment", recipe.id));
        let tool = tools
            .get(recipe.tool_id.as_str())
            .unwrap_or_else(|| panic!("{} references unknown tool", recipe.id));
        assert_eq!(tool.support, SupportState::Enabled);
        assert_eq!(
            tool.dependencies.provisioning_group, environment.provisioning_group,
            "{} provisioning group",
            recipe.id
        );
        assert!(tool.constraints.platforms.contains(&Platform::Macos));
        assert!(
            tool.constraints
                .architectures
                .contains(&Architecture::Aarch64)
        );
        assert_eq!(tool.constraints.case_network, NetworkPolicy::Denied);
        assert!(recipe.cases.contains(&recipe.representative_case));
        for case in &recipe.cases {
            assert!(
                tool.fixture_cases.contains(case),
                "{} names undeclared fixture {case}",
                recipe.id
            );
        }
        assert_unique(
            recipe.case_executables.iter().map(String::as_str),
            &format!("{} case executables", recipe.id),
        );
        let declared_executables = recipe
            .case_executables
            .iter()
            .map(String::as_str)
            .chain(environment.auxiliary_programs.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        for executable in std::iter::once(tool.dependencies.primary_executable.as_str())
            .chain(
                tool.dependencies
                    .program_overrides
                    .iter()
                    .map(String::as_str),
            )
            .chain(
                tool.dependencies
                    .wrapper_executables
                    .iter()
                    .map(String::as_str),
            )
        {
            assert!(
                declared_executables.contains(executable),
                "{} omits required executable {executable}",
                recipe.id
            );
        }
        assert!(!recipe.installation_source.trim().is_empty());
        validate_integrity(&root, &recipe.integrity, &recipe.id);
        validate_probe(&recipe.probe, &recipe.id);
        if recipe.tool_id == "astro" {
            validate_npm_lock_package(
                &root,
                &recipe.integrity,
                "astro",
                &recipe.version,
                "sha512-lLTYzx3fOvCmtwD3JVBLQcbORbIOW1/j0R+3IvJx/XKwMGrk7mFnF0BYSOeRiNw1qHUR5mdA6+hRnyvyDfqrWQ==",
            );
        }
        if recipe.tool_id == "biome" {
            validate_npm_lock_package(
                &root,
                &recipe.integrity,
                "@biomejs/biome",
                &recipe.version,
                "sha512-zr8K/DcY5tYsQOQwqMJ0AWElo6QgmgNI7idXgXLhevVszlt8RGVpesEJPqx3ThazLaOwjJ5Y8fz3BtH5fGZNsw==",
            );
            validate_npm_lock_entry(
                &root,
                &recipe.integrity,
                "@biomejs/cli-darwin-arm64",
                &recipe.version,
                "sha512-vxo/Ls3/PYdQWyLhYYcgMOCzQypAjcY+iihS8M0wW03l16TCLW4zqZzGo75gm1VdCMj38hTVZ31KBWrZ4G9dJw==",
            );
        }
        if recipe.tool_id == "prettier" {
            validate_npm_lock_package(
                &root,
                &recipe.integrity,
                "prettier",
                &recipe.version,
                "sha512-OpN0zzVdiaiAhxpuuj5efpIS4sY9j7bY6uR5mnj5yPzGkdkjNKSJeUThPb60Jw29QuAZgA4o+/iB49kFiaBX6g==",
            );
            let lock_path = recipe.integrity.path.as_deref().expect("Prettier npm lock");
            let lock: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(root.join(lock_path)).expect("read Prettier npm lock"),
            )
            .expect("parse Prettier npm lock");
            assert_eq!(
                lock["packages"]
                    .as_object()
                    .expect("Prettier packages")
                    .len(),
                2,
                "Prettier closure must contain only its root and one package"
            );
            let package: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(root.join(
                    "crates/hookkit-pkl-config/validation/provisioning/prettier/package.json",
                ))
                .expect("read Prettier package manifest"),
            )
            .expect("parse Prettier package manifest");
            assert_eq!(package["engines"]["node"], "24.19.0");
            assert_eq!(package["dependencies"]["prettier"], "3.9.6");
        }
        if recipe.tool_id == "contextlint" {
            validate_npm_lock_package(
                &root,
                &recipe.integrity,
                "@contextlint/cli",
                &recipe.version,
                "sha512-QCyjqmdaoanH9L8AduX2jH7vRm2yryHpxroLai0PHHP2lijBTG96UEICCuSIHbkoQ4FXulrokQst5+eTf34v9g==",
            );
            validate_npm_lock_entry(
                &root,
                &recipe.integrity,
                "@contextlint/core",
                &recipe.version,
                "sha512-ui2ymL90ZlV260NZD8pgki6fwCUM1bX2wj1LbDy5H4u7w8JyTvxIBORxzhWlklDUmsXf1wVxIZXdbvuRYRsqfQ==",
            );
            let package: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(root.join(
                    "crates/hookkit-pkl-config/validation/provisioning/contextlint/package.json",
                ))
                .expect("read Contextlint package manifest"),
            )
            .expect("parse Contextlint package manifest");
            assert_eq!(package["engines"]["node"], "24.19.0");
            assert_eq!(package["dependencies"]["@contextlint/cli"], "1.1.1");
            assert_eq!(package["dependencies"]["@contextlint/core"], "1.1.1");
        }
        if recipe.tool_id == "dclint" {
            validate_npm_lock_package(
                &root,
                &recipe.integrity,
                "dclint",
                &recipe.version,
                "sha512-afTGdzRFUXK4yCpIiEW/LOR+9TOMEDhNldDp56VCWzn7JDmD451PcUi640GGlMHgbHKJ10rDBm4PtpcBbjqlXw==",
            );
            let lock_path = recipe.integrity.path.as_deref().expect("dclint npm lock");
            let lock: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(root.join(lock_path)).expect("read dclint npm lock"),
            )
            .expect("parse dclint npm lock");
            assert_eq!(lock["lockfileVersion"], 3);
            assert_eq!(lock["packages"][""]["dependencies"]["dclint"], "3.1.0");
            let package: serde_json::Value =
                serde_json::from_str(
                    &std::fs::read_to_string(root.join(
                        "crates/hookkit-pkl-config/validation/provisioning/dclint/package.json",
                    ))
                    .expect("read dclint package manifest"),
                )
                .expect("parse dclint package manifest");
            assert_eq!(package["engines"]["node"], "24.19.0");
            assert_eq!(package["dependencies"]["dclint"], "3.1.0");
        }
        if recipe.tool_id == "eslint" {
            validate_npm_lock_package(
                &root,
                &recipe.integrity,
                "eslint",
                &recipe.version,
                "sha512-wqA7W2jbsC/BnV9Iv1UZpKVFkO1AdNoSmYW8NWG4HNOBbkAMvIqDZ27pI2f07dqn583NcIC44ckjAcOXDL1QbQ==",
            );
            let package: serde_json::Value =
                serde_json::from_str(
                    &std::fs::read_to_string(root.join(
                        "crates/hookkit-pkl-config/validation/provisioning/eslint/package.json",
                    ))
                    .expect("read ESLint package manifest"),
                )
                .expect("parse ESLint package manifest");
            assert_eq!(package["engines"]["node"], "24.19.0");
            assert_eq!(package["engines"]["npm"], "11.17.0");
            assert_eq!(package["dependencies"]["eslint"], "10.8.1");
            assert!(
                recipe
                    .installation_source
                    .contains("npm artifact fb37d514c19b6dd5b2d6b70169fd26fddfa97967")
            );
            assert!(
                recipe
                    .installation_source
                    .contains("commit c049dc3c4294da7afe3d920a1a5fdeba388f4983")
            );
        }
        match &tool.provenance.upstream {
            UpstreamProvenance::Recorded {
                version,
                installation_source,
                version_command,
                ..
            } => {
                assert_eq!(version, &recipe.version, "{} recorded version", recipe.id);
                assert_eq!(
                    installation_source, &recipe.installation_source,
                    "{} recorded installation source",
                    recipe.id
                );
                assert_eq!(
                    version_command, &recipe.probe.argv,
                    "{} recorded version command",
                    recipe.id
                );
            }
            UpstreamProvenance::Gap { .. } => {
                panic!("{} must record upstream provenance", recipe.id)
            }
        }
    }
    assert_eq!(
        recipe_tools,
        BTreeSet::from([
            "asciidoctor",
            "astro",
            "betterleaks",
            "biome",
            "black",
            "buf-format",
            "cargo-clippy",
            "cargo-fmt",
            "contextlint",
            "dclint",
            "eslint",
            "ghalint-workflow",
            "errcheck",
            "go-fmt",
            "go-vet",
            "gofumpt",
            "goimports",
            "golines",
            "jq",
            "prettier",
            "rubocop",
            "rustfmt",
            "sort-package-json",
            "swiftlint",
            "vacuum"
        ])
    );

    let cargo_fmt = registry
        .recipes
        .iter()
        .find(|recipe| recipe.tool_id == "cargo-fmt")
        .expect("cargo-fmt pinned recipe");
    assert_eq!(cargo_fmt.id, "cargo-fmt-macos-arm64");
    assert_eq!(cargo_fmt.environment_id, "macos-arm64-cargo-clippy");
    assert_eq!(
        cargo_fmt.case_executables,
        ["cargo", "cargo-fmt", "python", "rustc", "rustfmt"]
    );
    assert_eq!(
        cargo_fmt.cases,
        [
            "clean",
            "coverage-failure",
            "operational-failure",
            "source-issue",
            "workspace-multi"
        ]
    );
    assert_eq!(cargo_fmt.representative_case, "workspace-multi");
    assert_eq!(cargo_fmt.probe.argv, ["cargo-fmt", "--version"]);
    assert_eq!(cargo_fmt.probe.match_kind, "exact");
    assert_eq!(
        cargo_fmt.probe.expected,
        "rustfmt 1.9.0-stable (8bab26f4f6 2026-07-14)"
    );

    let vacuum = registry
        .recipes
        .iter()
        .find(|recipe| recipe.tool_id == "vacuum")
        .expect("Vacuum pinned recipe");
    assert_eq!(vacuum.id, "vacuum-macos-arm64");
    assert_eq!(vacuum.environment_id, "macos-arm64-vacuum");
    assert_eq!(vacuum.version, "0.30.0");
    assert_eq!(vacuum.case_executables, ["vacuum", "python"]);
    assert_eq!(
        vacuum.cases,
        ["clean", "source-issue", "multi-file", "operational-failure"]
    );
    assert_eq!(vacuum.representative_case, "multi-file");
    assert_eq!(vacuum.probe.argv, ["vacuum", "version"]);
    assert_eq!(vacuum.probe.match_kind, "exact");
    assert_eq!(vacuum.probe.expected, "0.30.0");

    let ghalint = registry
        .recipes
        .iter()
        .find(|recipe| recipe.tool_id == "ghalint-workflow")
        .expect("ghalint-workflow pinned recipe");
    assert_eq!(ghalint.id, "ghalint-workflow-macos-arm64");
    assert_eq!(ghalint.environment_id, "macos-arm64-github-actions");
    assert_eq!(ghalint.version, "1.5.6+velvet-glove.1");
    assert_eq!(ghalint.case_executables, ["ghalint", "python"]);
    assert_eq!(
        ghalint.cases,
        [
            "clean",
            "config-failure",
            "malformed",
            "multi-workflow",
            "policy-grammar",
            "source-issue",
        ]
    );
    assert_eq!(ghalint.representative_case, "multi-workflow");
    assert_eq!(ghalint.probe.argv, ["ghalint", "--version"]);
    assert_eq!(ghalint.probe.match_kind, "exact");
    assert_eq!(
        ghalint.probe.expected,
        "ghalint version 1.5.6+velvet-glove.1"
    );

    let errcheck = registry
        .recipes
        .iter()
        .find(|recipe| recipe.tool_id == "errcheck")
        .expect("errcheck pinned recipe");
    assert_eq!(errcheck.id, "errcheck-macos-arm64");
    assert_eq!(errcheck.environment_id, "macos-arm64-errcheck");
    assert_eq!(errcheck.version, "1.20.0");
    assert_eq!(errcheck.case_executables, ["errcheck", "go", "python"]);
    assert_eq!(
        errcheck.cases,
        ["clean", "unhandled", "multi-file", "operational-failure"]
    );
    assert_eq!(errcheck.representative_case, "multi-file");
    assert_eq!(errcheck.probe.argv, ["go", "version", "-m", "errcheck"]);
    assert_eq!(errcheck.probe.match_kind, "exact");
    assert_eq!(
        errcheck.probe.expected,
        "github.com/kisielk/errcheck v1.20.0 h1:9rwHBNKzd4wkDWcROy3DvFGNqEPlkxBg305rvk7HabI=; go1.26.5; darwin/arm64; sha256:4f369aeb1bd8454d6ebb6789fedd948ef216fe04c6be629d5016aca78908aa0c"
    );

    let go_vet = registry
        .recipes
        .iter()
        .find(|recipe| recipe.tool_id == "go-vet")
        .expect("go-vet pinned recipe");
    assert_eq!(go_vet.id, "go-vet-macos-arm64");
    assert_eq!(go_vet.environment_id, "macos-arm64-go");
    assert_eq!(go_vet.version, "go1.26.5");
    assert_eq!(go_vet.case_executables, ["go", "python"]);
    assert_eq!(
        go_vet.cases,
        [
            "clean",
            "multi-package",
            "operational-failure",
            "printf-mismatch",
            "test-findings"
        ]
    );
    assert_eq!(go_vet.representative_case, "test-findings");
    assert_eq!(go_vet.probe.argv, ["go", "version"]);
    assert_eq!(go_vet.probe.match_kind, "exact");
    assert_eq!(go_vet.probe.expected, "go version go1.26.5 darwin/arm64");
    assert_eq!(go_vet.integrity.kind, "mise-lock");
    assert_eq!(
        go_vet.integrity.path.as_deref(),
        Some("crates/hookkit-pkl-config/validation/provisioning/mise.lock")
    );
    assert!(
        go_vet
            .installation_source
            .contains("efb87ff28af9a188d0536ef5d42e63dd52ba8263cd7344a993cc48dd11dedb6a")
    );
    assert!(
        go_vet
            .installation_source
            .contains("c19862e5f8415b4f24b189d065ed739517c548ba")
    );

    let gofumpt = registry
        .recipes
        .iter()
        .find(|recipe| recipe.tool_id == "gofumpt")
        .expect("gofumpt pinned recipe");
    assert_eq!(gofumpt.id, "gofumpt-macos-arm64");
    assert_eq!(gofumpt.environment_id, "macos-arm64-gofumpt");
    assert_eq!(gofumpt.version, "0.11.0");
    assert_eq!(gofumpt.case_executables, ["gofumpt", "go", "python"]);
    assert_eq!(
        gofumpt.cases,
        [
            "clean",
            "multi-file",
            "operational-failure",
            "standalone",
            "unformatted"
        ]
    );
    assert_eq!(gofumpt.representative_case, "multi-file");
    assert_eq!(gofumpt.probe.argv, ["gofumpt", "-version"]);
    assert_eq!(gofumpt.probe.match_kind, "exact");
    assert_eq!(gofumpt.probe.expected, "v0.11.0 (go1.26.5)");
    assert_eq!(gofumpt.integrity.kind, "mise-lock");
    assert_eq!(
        gofumpt.integrity.sha256.as_deref(),
        Some("18936628f195369a80a129c73ee33d23e39086286dab538781ba826effc7e10b")
    );
    for binding in [
        "2eb2409f833722c24213089299ba9d6778a441fa",
        "5dca7d819315c5c6338d290ad2e7847f07438693",
        "18936628f195369a80a129c73ee33d23e39086286dab538781ba826effc7e10b",
        "no retrievable artifact attestation",
    ] {
        assert!(
            gofumpt.installation_source.contains(binding),
            "gofumpt provenance omits {binding}"
        );
    }
    let gofumpt_environment = environments
        .get(gofumpt.environment_id.as_str())
        .expect("gofumpt controlled environment");
    assert_eq!(
        gofumpt_environment
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<Vec<_>>(),
        ["gofumpt", "gofumpt-go"]
    );
    assert!(
        gofumpt_environment.bootstrap.is_empty(),
        "official gofumpt asset requires no mutable module bootstrap"
    );

    let goimports = registry
        .recipes
        .iter()
        .find(|recipe| recipe.tool_id == "goimports")
        .expect("goimports pinned recipe");
    assert_eq!(goimports.id, "goimports-macos-arm64");
    assert_eq!(goimports.environment_id, "macos-arm64-goimports");
    assert_eq!(goimports.version, "0.48.0");
    assert_eq!(goimports.case_executables, ["goimports", "go", "python"]);
    assert_eq!(
        goimports.cases,
        [
            "clean",
            "missing-import",
            "multi-file",
            "operational-failure"
        ]
    );
    assert_eq!(goimports.representative_case, "multi-file");
    assert_eq!(goimports.probe.argv, ["go", "version", "-m", "goimports"]);
    assert_eq!(goimports.probe.match_kind, "exact");
    assert_eq!(
        goimports.probe.expected,
        "golang.org/x/tools v0.48.0 h1:3+hClM1aLL5mjMKm5ovokw9epgRXPuu2tILgismM6RE=; go1.26.5; darwin/arm64; sha256:2d7d2892651e4452091f0fe8e280c7b6e14f3b6964854516fd7372442d57fd27"
    );
    assert_eq!(goimports.integrity.kind, "go-module-build");
    assert_eq!(
        goimports.integrity.sha256.as_deref(),
        Some("8529e7bd696890fd79d3e1c37c7d1a3e2e26fb4b392b5beebfa7134ad2f65755")
    );
    assert_eq!(
        goimports.integrity.built_artifact_sha256.as_deref(),
        Some("2d7d2892651e4452091f0fe8e280c7b6e14f3b6964854516fd7372442d57fd27")
    );
    assert_eq!(
        goimports.integrity.module_manifest_sha256.as_deref(),
        Some("9de464c8f30dde87a846b165fadd6620a150e54352265f8b22a7b63959510778")
    );
    assert_eq!(
        goimports.integrity.module_lock_sha256.as_deref(),
        Some("d43f495d37c149ddc7145f20b13b84812ba3aea895834e7595d6eacd62bc7a44")
    );
    let goimports_environment = environments
        .get(goimports.environment_id.as_str())
        .expect("goimports controlled environment");
    assert_eq!(
        goimports_environment
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<Vec<_>>(),
        ["goimports-go"]
    );
    assert_eq!(goimports_environment.bootstrap.len(), 1);
    assert_eq!(
        goimports_environment.bootstrap[0].argv,
        [
            "go",
            "-C",
            "{repository}/crates/hookkit-pkl-config/validation/provisioning/goimports",
            "mod",
            "download",
            "golang.org/x/tools@v0.48.0",
            "golang.org/x/mod@v0.38.0",
            "golang.org/x/sync@v0.22.0",
            "golang.org/x/telemetry@v0.0.0-20260708182218-49f421fb7959",
        ]
    );

    let golines = registry
        .recipes
        .iter()
        .find(|recipe| recipe.tool_id == "golines")
        .expect("golines pinned recipe");
    assert_eq!(golines.id, "golines-macos-arm64");
    assert_eq!(golines.environment_id, "macos-arm64-golines");
    assert_eq!(golines.version, "0.13.0+velvet-glove.1");
    assert_eq!(golines.case_executables, ["golines", "python"]);
    assert_eq!(
        golines.cases,
        [
            "clean",
            "generated-explicit",
            "long-line",
            "multi-file",
            "operational-failure"
        ]
    );
    assert_eq!(golines.representative_case, "multi-file");
    assert_eq!(golines.probe.argv, ["golines", "--version"]);
    assert_eq!(golines.probe.match_kind, "exact");
    assert_eq!(
        golines.probe.expected,
        "golines v0.13.0+velvet-glove.1\n\nbuild information:\n\tbuild date: 2025-08-21T21:22:01Z\n\tgit commit ref: 8f32f0f7e89c30f572c7f2cd3b2a48016b9d8bbf"
    );
    assert_eq!(golines.integrity.kind, "go-source-build");
    assert_eq!(
        golines.integrity.patch_sha256.as_deref(),
        Some("c4a7fcf96b2f1a83440e824340e6d51e15ed34630415e044781a780fc7a2a4d3")
    );
    assert_eq!(
        golines.integrity.built_artifact_sha256.as_deref(),
        Some("4d7bf2a59b9b48bfc234078498b3ddf6a412cf9bd0ce525945bb19d558f6ab75")
    );
    let golines_environment = environments
        .get(golines.environment_id.as_str())
        .expect("golines controlled environment");
    assert_eq!(
        golines_environment
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<Vec<_>>(),
        ["golines-go", "golines"]
    );
    assert_eq!(
        golines_environment
            .bootstrap
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>(),
        [
            "golines-apply-closure",
            "golines-go-mod-download",
            "golines-go-mod-verify",
            "golines-go-build"
        ]
    );
    let download = &golines_environment.bootstrap[1];
    assert_eq!(download.argv[4], "download");
    assert_eq!(download.argv.len(), 17);
    assert!(
        !download.argv.iter().any(|argument| argument == "all"),
        "golines bootstrap must fetch only the 12 binary runtime modules"
    );

    let cargo_fmt_environment = environments
        .get(cargo_fmt.environment_id.as_str())
        .expect("cargo-fmt controlled environment");
    for required in [
        "cargo",
        "cargo-clippy",
        "cargo-fmt",
        "clippy-driver",
        "rustc",
        "rustdoc",
        "rustfmt",
    ] {
        assert!(
            cargo_fmt_environment
                .auxiliary_programs
                .iter()
                .any(|program| program == required),
            "cargo-fmt closure omits {required}"
        );
    }
}

#[test]
fn pinned_runner_registry_selection_is_exact_and_parser_stable() {
    let registry: Registry = serde_json::from_str(RECIPES_JSON).expect("strict recipe registry");
    let plan = |selected_tools: &BTreeSet<&str>| {
        let environment_ids = registry
            .recipes
            .iter()
            .filter(|recipe| selected_tools.contains(recipe.tool_id.as_str()))
            .map(|recipe| recipe.environment_id.clone())
            .collect::<BTreeSet<_>>();
        let selected_environments = registry
            .environments
            .iter()
            .filter(|environment| environment_ids.contains(&environment.id))
            .collect::<Vec<_>>();
        let groups = selected_environments
            .iter()
            .map(|environment| environment.provisioning_group.clone())
            .collect::<BTreeSet<_>>();
        let mise_tools = registry
            .shared_components
            .iter()
            .chain(
                selected_environments
                    .iter()
                    .flat_map(|environment| environment.components.iter()),
            )
            .filter_map(|component| component.mise_tool.clone())
            .collect::<BTreeSet<_>>();
        (environment_ids, groups, mise_tools)
    };

    let errcheck_tools = BTreeSet::from(["errcheck"]);
    let (environment_ids, groups, mise_tools) = plan(&errcheck_tools);
    assert_eq!(
        environment_ids,
        BTreeSet::from(["macos-arm64-errcheck".to_owned()])
    );
    assert_eq!(groups, BTreeSet::from(["go".to_owned()]));
    assert_eq!(
        mise_tools,
        BTreeSet::from([
            "go@1.26.5".to_owned(),
            "jq@1.8.2".to_owned(),
            "pkl@0.31.1".to_owned(),
            "python@3.14.5".to_owned(),
        ])
    );

    let representative_tools = registry
        .recipes
        .iter()
        .map(|recipe| recipe.tool_id.as_str())
        .collect::<BTreeSet<_>>();
    let (environment_ids, groups, mise_tools) = plan(&representative_tools);
    assert_eq!(
        environment_ids,
        [
            "macos-arm64-buf",
            "macos-arm64-cargo-clippy",
            "macos-arm64-contextlint",
            "macos-arm64-data-formats",
            "macos-arm64-dclint",
            "macos-arm64-errcheck",
            "macos-arm64-eslint",
            "macos-arm64-github-actions",
            "macos-arm64-go",
            "macos-arm64-gofumpt",
            "macos-arm64-goimports",
            "macos-arm64-golines",
            "macos-arm64-node",
            "macos-arm64-prettier",
            "macos-arm64-python",
            "macos-arm64-ruby",
            "macos-arm64-rust",
            "macos-arm64-security",
            "macos-arm64-swift",
            "macos-arm64-vacuum",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
    assert_eq!(
        groups,
        [
            "cargo-clippy",
            "contextlint",
            "data-formats",
            "dclint",
            "eslint",
            "github-actions",
            "go",
            "node",
            "prettier",
            "python",
            "ruby",
            "rust",
            "security",
            "swift",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
    assert_eq!(
        mise_tools,
        [
            "buf@1.72.0",
            "go@1.26.5",
            "gofumpt@0.11.0",
            "jq@1.8.2",
            "node@24.18.0",
            "pkl@0.31.1",
            "python@3.14.5",
            "swiftlint@0.65.0",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );

    let root = repository_root();
    let outer = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");
    for (name, runner) in [("outer", outer.as_str()), ("inner", inner.as_str())] {
        assert!(
            !runner.contains("select(.toolId as $tool | $tools | index($tool))"),
            "{name} must bind the recipe outside its membership predicate"
        );
        assert!(
            !runner.contains("select(.id as $id | $environmentIds | index($id))"),
            "{name} must bind the environment outside its membership predicate"
        );
    }
    assert!(!inner.contains("any(.recipes[] as"));
    assert!(inner.contains(". as $registry\n  | ([.recipes[] as $recipe"));
}

#[test]
#[cfg(unix)]
fn ghalint_source_provenance_and_runner_binding_are_exact() {
    let root = repository_root();
    let provenance: serde_json::Value = serde_json::from_str(GHALINT_SOURCE_BUILD_JSON)
        .expect("ghalint source-build provenance JSON");
    assert_eq!(provenance["schemaVersion"], 1);
    assert_eq!(provenance["status"], "integrated");
    assert_eq!(
        provenance["upstream"]["peeledCommit"],
        "050e825989101021ece297e4d2f726f519ba89ee"
    );
    assert_eq!(
        provenance["upstream"]["sourceArchive"]["sha256"],
        "1188047b654a86390d49b776153c1a7b3eddde30ebcc0d024dfab9585785b02b"
    );
    assert_eq!(
        provenance["closure"]["patchSha256"],
        "5e3c2480665eefffa019adf5c57e27e1c1d05a74b9dccf2d5bc345017a17d6ed"
    );
    assert_eq!(
        provenance["closure"]["moduleManifestSha256"],
        "ada0a9434578f54fd6a50fe8ed9ef26374afa631d5527660723062663d686f16"
    );
    assert_eq!(
        provenance["closure"]["moduleLockSha256"],
        "53a4a1b1a7dcd2a6da2dc1cc0cc32ca4bcb5b8ea86832749e18879b8be594dbb"
    );
    assert_eq!(provenance["toolchain"]["version"], "1.26.5");
    assert_eq!(provenance["build"]["sourceDateEpoch"], "1777591460");
    assert_eq!(
        provenance["artifact"]["sha256"],
        "03437b6c73d1332460d24f2c9fe22d3dea0fe68e4e52b0a8a534b3f2854274fa"
    );
    assert_eq!(
        provenance["artifact"]["embeddedBuildFacts"]["xText"],
        "v0.39.0"
    );
    let registry: serde_json::Value =
        serde_json::from_str(RECIPES_JSON).expect("provisioning registry JSON");
    let component = registry["environments"]
        .as_array()
        .expect("provisioning environments")
        .iter()
        .flat_map(|environment| {
            environment["components"]
                .as_array()
                .expect("environment components")
        })
        .find(|component| component["id"] == "ghalint-workflow")
        .expect("ghalint source-build component");
    assert_eq!(
        provenance["component"]["installationSource"],
        component["installationSource"]
    );
    assert_eq!(
        provenance["build"]["argv"],
        component["integrity"]["buildArgv"]
    );
    assert_eq!(
        provenance["build"]["environment"],
        component["integrity"]["buildEnvironment"]
    );
    assert_eq!(provenance["build"]["bootstrap"][1]["argv"][5], "all");
    assert_eq!(
        provenance["artifact"]["sha256"],
        component["integrity"]["builtArtifactSha256"]
    );

    let outer = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    for required in [
        "if needs_group github-actions; then",
        "ghalint_archive=$(fetch_component_archive ghalint-workflow)",
        "error: ghalint closure patch checksum mismatch",
        "error: ghalint patched module closure checksum mismatch",
        "GOPROXY=https://proxy.golang.org",
        "GOSUMDB=sum.golang.org",
        "mod download all",
        "$mise_bin\" -C \"$provisioning_dir\" exec --locked --fresh-env --deny-net -- \\",
        "mod verify",
        "-X=main.version=1.5.6+velvet-glove.1",
        "reproducible ghalint artifact checksum mismatch",
        "github.com/suzuki-shunsuke/ghalint/cmd/ghalint",
        "golang.org/x/text\\tv0.39.0",
        "verify_macho_closure \"$ghalint_staging_root\" ghalint-workflow",
        "ghalint version 1.5.6+velvet-glove.1",
    ] {
        assert!(
            outer.contains(required),
            "outer ghalint binding: {required}"
        );
    }
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");
    for required in [
        "if [[ ,$selection, == *,ghalint-workflow/* ]]; then",
        "ghalint_path_prefix=\"$state_dir/ghalint-1.5.6-vg1/bin:\"",
        "export PATH=\"${ghalint_path_prefix}${vacuum_path_prefix}",
    ] {
        assert!(
            inner.contains(required),
            "inner ghalint binding: {required}"
        );
    }
    assert!(
        !inner.contains("export PATH=\"$state_dir/ghalint-1.5.6-vg1/bin:"),
        "ghalint must be visible only for a selected ghalint contract"
    );
}

#[test]
#[cfg(unix)]
fn vacuum_provenance_and_content_addressed_binding_are_exact() {
    let root = repository_root();
    let provenance: serde_json::Value =
        serde_json::from_str(VACUUM_PROVENANCE_JSON).expect("Vacuum provenance JSON");
    assert_eq!(
        provenance,
        serde_json::json!({
            "schemaVersion": 1,
            "release": {
                "version": "0.30.0",
                "tag": "v0.30.0",
                "tagObject": "5502edc731a0f54a549620ea64e67eb9ef533739",
                "sourceCommit": "328ff253522138616096eeabf1dc1c8895dac215",
                "releaseUrl": "https://github.com/daveshanley/vacuum/releases/tag/v0.30.0"
            },
            "archive": {
                "url": "https://github.com/daveshanley/vacuum/releases/download/v0.30.0/vacuum_0.30.0_darwin_arm64.tar.gz",
                "sha256": "bebcc32f58db734bcf329ef6f0754d2b1051d55961ee92aac1d2b1192fad78e8",
                "members": ["LICENSE", "README.md", "vacuum"],
                "binarySha256": "b8fc23e87917742f2b81bb55addc8d1b968759c7ad5e7372ad23748197c53afa",
                "licenseSha256": "a4c0921c8f302fdb282c41bcb85e09375561f9c9b38e77c258d89d17492555cf",
                "readmeSha256": "b57124010840e63ce1263938b623b8e663599e265958d5ae2731ae7aca605522"
            },
            "upstreamEvidence": {
                "checksums": {
                    "url": "https://github.com/daveshanley/vacuum/releases/download/v0.30.0/checksums.txt",
                    "sha256": "2dac5adb73fe190e2657108f2ab408fafbc0fe5323b33825b03a6537de0207c8"
                },
                "sigstoreBundle": {
                    "url": "https://github.com/daveshanley/vacuum/releases/download/v0.30.0/checksums.txt.sigstore.json",
                    "sha256": "08dc6453c5f396db405f04f3c0709424fb0a549200e7fbb3768d268c0c2a07bc",
                    "subjectSha256": "2dac5adb73fe190e2657108f2ab408fafbc0fe5323b33825b03a6537de0207c8",
                    "certificate": {
                        "issuer": "sigstore-intermediate",
                        "issuedAt": "2026-07-23T12:18:43Z",
                        "san": "https://github.com/daveshanley/vacuum/.github/workflows/publish.yaml@refs/heads/main",
                        "repository": "daveshanley/vacuum",
                        "workflow": "Publish",
                        "sourceCommit": "328ff253522138616096eeabf1dc1c8895dac215"
                    }
                }
            },
            "darwin": {
                "architecture": "arm64",
                "minimumOsVersion": "12.0",
                "hardenedRuntimeFlag": "0x10000(runtime)",
                "teamIdentifier": "HFX5KEHACT",
                "allowedDylibPrefixes": ["/System/Library/", "/usr/lib/"]
            },
            "probe": {
                "argv": ["vacuum", "version"],
                "expected": "0.30.0"
            }
        })
    );

    let registry: serde_json::Value =
        serde_json::from_str(RECIPES_JSON).expect("provisioning registry JSON");
    let component = registry["environments"]
        .as_array()
        .expect("environments")
        .iter()
        .find(|environment| environment["id"] == "macos-arm64-vacuum")
        .and_then(|environment| environment["components"].as_array())
        .and_then(|components| components.first())
        .expect("Vacuum component");
    let recipe = registry["recipes"]
        .as_array()
        .expect("recipes")
        .iter()
        .find(|recipe| recipe["toolId"] == "vacuum")
        .expect("Vacuum recipe");
    for declaration in [component, recipe] {
        assert_eq!(declaration["version"], provenance["release"]["version"]);
        assert_eq!(
            declaration["integrity"]["url"],
            provenance["archive"]["url"]
        );
        assert_eq!(
            declaration["integrity"]["sha256"],
            provenance["archive"]["sha256"]
        );
        assert_eq!(
            declaration["integrity"]["minOsVersion"],
            provenance["darwin"]["minimumOsVersion"]
        );
        assert_eq!(declaration["probe"]["argv"], provenance["probe"]["argv"]);
        assert_eq!(
            declaration["probe"]["expected"],
            provenance["probe"]["expected"]
        );
    }

    let cache = std::fs::read_to_string(root.join("scripts/pinned-tool-cache.sh"))
        .expect("shared pinned cache helper");
    let outer = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");
    assert!(cache.contains("pinned_component_provenance_identity()"));
    for (name, runner) in [("outer", outer.as_str()), ("inner", inner.as_str())] {
        assert!(
            runner.contains("vacuum_identity=$(pinned_component_provenance_identity \\"),
            "{name} provenance-bound identity"
        );
        assert!(
            runner.contains("vacuum_root=$(pinned_component_cache_root \\"),
            "{name} content-addressed root"
        );
        assert!(
            runner.contains(
                "crates/hookkit-pkl-config/validation/provisioning/vacuum/provenance.json"
            ),
            "{name} provenance path"
        );
    }
    assert!(inner.contains("resolved=\"$vacuum_bin\""));
    assert!(inner.contains("probe_argv=(\"$vacuum_bin\" \"${probe_argv[@]:1}\")"));

    let temporary = test_directory("vacuum-cache-reuse");
    let identity = serde_json::json!({
        "component": {
            "id": "vacuum",
            "version": "0.30.0",
            "integrity": component["integrity"]
        },
        "provenance": {
            "path": "crates/hookkit-pkl-config/validation/provisioning/vacuum/provenance.json",
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }
    })
    .to_string();
    let shell = r#"
source "$1"
first=$(pinned_component_cache_root "$2" vacuum-0.30.0 "$3")
second=$(pinned_component_cache_root "$2" vacuum-0.30.0 "$3")
[[ $first == "$second" ]]
printf '%s\n' "$first"
if pinned_component_cache_valid "$first" "$3" bin/vacuum; then
  printf 'reused\n'
else
  printf 'install-required\n'
fi
"#;
    let invoke = || {
        Command::new("/bin/bash")
            .args(["-c", shell, "vacuum-cache-reuse"])
            .arg(root.join("scripts/pinned-tool-cache.sh"))
            .arg(&temporary.0)
            .arg(&identity)
            .output()
            .expect("execute Vacuum cache regression")
    };
    let initial = invoke();
    assert!(initial.status.success());
    let initial_stdout = String::from_utf8(initial.stdout).expect("UTF-8 cache output");
    assert!(initial_stdout.ends_with("install-required\n"));
    let cache_root = PathBuf::from(initial_stdout.lines().next().expect("Vacuum cache root"));
    std::fs::create_dir_all(cache_root.join("bin")).expect("Vacuum cache bin");
    std::fs::write(
        cache_root.join(".velvet-glove-artifacts.json"),
        format!("{identity}\n"),
    )
    .expect("Vacuum cache identity");
    write_test_executable(&cache_root.join("bin/vacuum"));
    let reused = invoke();
    assert!(reused.status.success());
    assert!(
        String::from_utf8(reused.stdout)
            .expect("UTF-8 cache reuse output")
            .ends_with("reused\n")
    );
}

#[test]
fn prettier_provisioning_uses_a_dedicated_runtime_and_case_only_binding() {
    let root = repository_root();
    let outer = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");
    let harness = std::fs::read_to_string(root.join("crates/velvet-glove/tests/tool_fixtures.rs"))
        .expect("real-tool fixture harness");
    let mise_lock = std::fs::read_to_string(
        root.join("crates/hookkit-pkl-config/validation/provisioning/mise.lock"),
    )
    .expect("shared mise lock");

    assert!(outer.contains("fetch_component_archive prettier-node"));
    assert!(
        outer.contains(
            "prettier_root=\"$state_dir/prettier-environment-node-24.19.0-prettier-3.9.6\""
        )
    );
    assert!(outer.contains("\"$prettier_install_root/node/bin/node\" \\"));
    assert!(
        outer.contains("\"$prettier_install_root/node/lib/node_modules/npm/bin/npm-cli.js\" \\")
    );
    assert!(outer.contains("ci --ignore-scripts --no-audit --no-fund"));
    assert!(outer.contains("verify_macho_closure \"$prettier_root/node\" prettier-node"));
    assert!(inner.contains(
        "export VELVET_GLOVE_FIXTURE_PRETTIER_ROOT=\"$state_dir/prettier-environment-node-24.19.0-prettier-3.9.6\""
    ));
    assert!(inner.contains("prettier_node=\"$VELVET_GLOVE_FIXTURE_PRETTIER_ROOT/node/bin/node\""));
    assert!(inner.contains(
        "prettier_cli=\"$VELVET_GLOVE_FIXTURE_PRETTIER_ROOT/package/node_modules/prettier/bin/prettier.cjs\""
    ));
    let path_export = inner
        .lines()
        .find(|line| line.starts_with("export PATH="))
        .expect("controlled PATH export");
    assert!(
        !path_export.contains("prettier-environment"),
        "dedicated Node must not leak into the shared representative PATH"
    );
    assert!(
        harness.contains("const PRETTIER_ROOT_ENV: &str = \"VELVET_GLOVE_FIXTURE_PRETTIER_ROOT\";")
    );
    assert!(harness.contains("\"node\" if prettier_toolchain.is_some()"));
    assert!(harness.contains("expected.arguments.first().map(PathBuf::from)"));
    assert!(mise_lock.contains("version = \"24.18.0\""));
    assert!(
        !mise_lock.contains("24.19.0"),
        "dedicated Prettier Node must not alter the shared mise graph"
    );
}

#[test]
fn contextlint_provisioning_uses_a_separate_exact_runtime_and_case_only_binding() {
    let root = repository_root();
    let outer = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");
    let harness = std::fs::read_to_string(root.join("crates/velvet-glove/tests/tool_fixtures.rs"))
        .expect("real-tool fixture harness");

    assert!(outer.contains("fetch_component_archive contextlint-node"));
    assert!(outer.contains(
        "contextlint_root=\"$state_dir/contextlint-environment-node-24.19.0-contextlint-1.1.1\""
    ));
    assert!(outer.contains("\"$contextlint_install_root/node/bin/node\" \\"));
    assert!(
        outer.contains("\"$contextlint_install_root/node/lib/node_modules/npm/bin/npm-cli.js\" \\")
    );
    assert!(outer.contains("verify_macho_closure \"$contextlint_root/node\" contextlint-node"));
    assert!(outer.contains("! -f $contextlint_cli"));
    assert!(outer.contains("\"$contextlint_node\" \"$contextlint_npm_cli\" ls --all --prefix"));
    assert!(inner.contains(
        "export VELVET_GLOVE_FIXTURE_CONTEXTLINT_ROOT=\"$state_dir/contextlint-environment-node-24.19.0-contextlint-1.1.1\""
    ));
    assert!(
        inner.contains("contextlint_node=\"$VELVET_GLOVE_FIXTURE_CONTEXTLINT_ROOT/node/bin/node\"")
    );
    assert!(inner.contains(
        "contextlint_cli=\"$VELVET_GLOVE_FIXTURE_CONTEXTLINT_ROOT/package/node_modules/@contextlint/cli/dist/index.js\""
    ));
    let path_export = inner
        .lines()
        .find(|line| line.starts_with("export PATH="))
        .expect("controlled PATH export");
    assert!(
        !path_export.contains("contextlint-environment"),
        "dedicated Contextlint graph must not leak into the shared representative PATH"
    );
    assert!(
        harness.contains(
            "const CONTEXTLINT_ROOT_ENV: &str = \"VELVET_GLOVE_FIXTURE_CONTEXTLINT_ROOT\";"
        )
    );
    assert!(harness.contains("\"node\" if contextlint_toolchain.is_some()"));
    assert!(harness.contains("Contextlint trace did not pass the dedicated managed CLI"));
}

#[test]
fn dclint_provisioning_uses_a_dedicated_runtime_and_case_only_binding() {
    let root = repository_root();
    let outer = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");
    let harness = std::fs::read_to_string(root.join("crates/velvet-glove/tests/tool_fixtures.rs"))
        .expect("real-tool fixture harness");
    let shared_package = std::fs::read_to_string(
        root.join("crates/hookkit-pkl-config/validation/provisioning/node/package.json"),
    )
    .expect("shared Node package manifest");

    assert!(outer.contains("fetch_component_archive dclint-node"));
    assert!(
        outer.contains("dclint_root=\"$state_dir/dclint-environment-node-24.19.0-dclint-3.1.0\"")
    );
    assert!(outer.contains("\"$dclint_install_root/node/bin/node\" \\"));
    assert!(outer.contains("\"$dclint_install_root/node/lib/node_modules/npm/bin/npm-cli.js\" \\"));
    assert!(outer.contains("verify_macho_closure \"$dclint_root/node\" dclint-node"));
    assert!(outer.contains("readlink \"$dclint_bin_link\") != \"../dclint/bin/dclint.cjs\""));
    assert!(inner.contains(
        "export VELVET_GLOVE_FIXTURE_DCLINT_ROOT=\"$state_dir/dclint-environment-node-24.19.0-dclint-3.1.0\""
    ));
    assert!(inner.contains("dclint_node=\"$VELVET_GLOVE_FIXTURE_DCLINT_ROOT/node/bin/node\""));
    assert!(inner.contains(
        "dclint_cli=\"$VELVET_GLOVE_FIXTURE_DCLINT_ROOT/package/node_modules/.bin/dclint\""
    ));
    let base_path_export = inner
        .lines()
        .find(|line| line.starts_with("export PATH="))
        .expect("controlled base PATH export");
    assert!(
        !base_path_export.contains("dclint-environment"),
        "dedicated dclint Node must not leak into unrelated case PATHs"
    );
    assert!(
        !inner
            .contains("export PATH=\"$VELVET_GLOVE_FIXTURE_DCLINT_ROOT/package/node_modules/.bin:"),
        "combined representatives must retain the shared Node runtime on PATH"
    );
    assert!(inner.contains(
        "if [[ $shared_node_selected == false ]]; then\n          resolved=\"$dclint_node\""
    ));
    assert!(
        harness.contains("const DCLINT_ROOT_ENV: &str = \"VELVET_GLOVE_FIXTURE_DCLINT_ROOT\";")
    );
    assert!(harness.contains("\"node\" if dclint_toolchain.is_some()"));
    assert!(harness.contains("\"dclint\" if dclint_toolchain.is_some()"));
    assert!(harness.contains("toolchain.package_bin.clone()"));
    assert!(harness.contains("path_entries.push(toolchain.node_bin.clone())"));
    assert!(!shared_package.contains("\"dclint\""));
}

#[test]
fn eslint_provisioning_uses_a_dedicated_runtime_and_case_only_binding() {
    let root = repository_root();
    let outer = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");
    let harness = std::fs::read_to_string(root.join("crates/velvet-glove/tests/tool_fixtures.rs"))
        .expect("real-tool fixture harness");

    assert!(outer.contains("fetch_component_archive eslint-node"));
    assert!(
        outer.contains("eslint_root=\"$state_dir/eslint-environment-node-24.19.0-eslint-10.8.1\"")
    );
    assert!(outer.contains("\"$eslint_install_root/node/bin/node\" \\"));
    assert!(outer.contains("\"$eslint_install_root/node/lib/node_modules/npm/bin/npm-cli.js\" \\"));
    assert!(outer.contains("verify_macho_closure \"$eslint_root/node\" eslint-node"));
    assert!(outer.contains("readlink \"$eslint_bin_link\") != \"../eslint/bin/eslint.js\""));
    assert!(outer.contains(
        "eslint_integrity='sha512-wqA7W2jbsC/BnV9Iv1UZpKVFkO1AdNoSmYW8NWG4HNOBbkAMvIqDZ27pI2f07dqn583NcIC44ckjAcOXDL1QbQ=='"
    ));
    assert!(outer.contains("eslint_shasum='fb37d514c19b6dd5b2d6b70169fd26fddfa97967'"));
    assert!(outer.contains("eslint_git_head='c049dc3c4294da7afe3d920a1a5fdeba388f4983'"));
    assert!(inner.contains(
        "export VELVET_GLOVE_FIXTURE_ESLINT_ROOT=\"$state_dir/eslint-environment-node-24.19.0-eslint-10.8.1\""
    ));
    assert!(inner.contains("eslint_node=\"$VELVET_GLOVE_FIXTURE_ESLINT_ROOT/node/bin/node\""));
    assert!(inner.contains(
        "eslint_cli=\"$VELVET_GLOVE_FIXTURE_ESLINT_ROOT/package/node_modules/eslint/bin/eslint.js\""
    ));
    let path_export = inner
        .lines()
        .find(|line| line.starts_with("export PATH="))
        .expect("controlled PATH export");
    assert!(
        !path_export.contains("eslint-environment"),
        "dedicated ESLint graph must not leak into unrelated case PATHs"
    );
    assert!(
        harness.contains("const ESLINT_ROOT_ENV: &str = \"VELVET_GLOVE_FIXTURE_ESLINT_ROOT\";")
    );
    assert!(harness.contains("\"node\" if eslint_toolchain.is_some()"));
    assert!(harness.contains("ESLint trace did not pass the dedicated managed CLI"));
}

#[test]
fn errcheck_provisioning_cross_links_proxy_module_artifact_and_go_identity() {
    let root = repository_root();
    let outer = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");
    let mise_lock = std::fs::read_to_string(
        root.join("crates/hookkit-pkl-config/validation/provisioning/mise.lock"),
    )
    .expect("shared mise lock");

    for runner in [&outer, &inner] {
        assert!(runner.contains("errcheck-macos-arm64"));
        assert!(
            runner.contains("4f369aeb1bd8454d6ebb6789fedd948ef216fe04c6be629d5016aca78908aa0c")
        );
        assert!(runner.contains("go1.26.5"));
        assert!(runner.contains("github.com/kisielk/errcheck\\tv1.20.0"));
        assert!(runner.contains("golang.org/x/mod\\tv0.35.0"));
        assert!(runner.contains("golang.org/x/sync\\tv0.20.0"));
        assert!(runner.contains("golang.org/x/tools\\tv0.44.0"));
    }
    assert!(outer.contains(
        "errcheck_proxy_zip=\"$errcheck_mod_cache/cache/download/github.com/kisielk/errcheck/@v/v1.20.0.zip\""
    ));
    assert!(outer.contains("\"$errcheck_go_bin\" install \\"));
    assert!(outer.contains("github.com/kisielk/errcheck@v1.20.0"));
    assert!(
        outer.contains(r#""$errcheck_go_bin" -C "${errcheck_manifest%/go.mod}" mod download \"#)
    );
    for module in [
        "github.com/kisielk/errcheck@v1.20.0",
        "golang.org/x/mod@v0.35.0",
        "golang.org/x/sync@v0.20.0",
        "golang.org/x/tools@v0.44.0",
    ] {
        assert!(
            outer.contains(module),
            "explicit errcheck bootstrap: {module}"
        );
    }
    let errcheck_block = outer
        .split("if [[ $errcheck_selected == true ]]; then")
        .nth(1)
        .expect("errcheck provisioning block")
        .split("if [[ $golines_selected == true ]]; then")
        .next()
        .expect("bounded errcheck provisioning block");
    assert!(
        !errcheck_block.contains("mod download all"),
        "errcheck bootstrap must not expand the reviewed module lock"
    );
    assert!(
        errcheck_block
            .contains("error: errcheck network bootstrap changed the exact module inputs")
    );
    assert!(outer.contains("GOPROXY=file://$errcheck_mod_cache/cache/download"));
    assert!(outer.contains("exec --locked --fresh-env --deny-net --"));
    assert!(inner.contains("pinned_component_cache_valid \\"));
    assert!(inner.contains("\"$errcheck_root\" \"$errcheck_identity\" bin/errcheck"));
    assert!(inner.contains("probe_argv=(\"$errcheck_go_bin\" version -m \"$errcheck_bin\")"));
    assert!(inner.contains("buildToolchainArtifactSha256"));
    assert!(mise_lock.contains("[[tools.go]]\nversion = \"1.26.5\""));
    assert!(mise_lock.contains(
        "checksum = \"sha256:efb87ff28af9a188d0536ef5d42e63dd52ba8263cd7344a993cc48dd11dedb6a\""
    ));
    assert!(
        !root
            .join("crates/hookkit-pkl-config/validation/provisioning/errcheck/recipe.draft.json")
            .exists()
    );
}

#[test]
fn go_vet_runner_binds_the_unchanged_managed_go_environment() {
    let root = repository_root();
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");
    let mise_lock = std::fs::read_to_string(
        root.join("crates/hookkit-pkl-config/validation/provisioning/mise.lock"),
    )
    .expect("shared mise lock");

    for required in [
        "go_vet_selected=false",
        "index(\"go-vet\") != null",
        "go_vet_go_bin=$(type -P go || true)",
        "denied-network go-vet Go resolves outside the managed mise root",
        "go version go1.26.5 darwin/arm64",
        "probe_argv=(\"$go_vet_go_bin\" version)",
        "observed=$(env GOTOOLCHAIN=local \"${probe_argv[@]}\" 2>&1)",
    ] {
        assert!(inner.contains(required), "go-vet runner omits {required:?}");
    }
    assert!(mise_lock.contains("[[tools.go]]\nversion = \"1.26.5\""));
    assert!(mise_lock.contains(
        "checksum = \"sha256:efb87ff28af9a188d0536ef5d42e63dd52ba8263cd7344a993cc48dd11dedb6a\""
    ));
}

#[test]
fn gofumpt_runner_binds_the_official_binary_and_exact_go_build_metadata() {
    let root = repository_root();
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");

    for required in [
        "gofumpt_selected=false",
        "index(\"gofumpt\") != null",
        "gofumpt_bin=$(type -P gofumpt || true)",
        "gofumpt_go_bin=$(type -P go || true)",
        "denied-network gofumpt closure resolves outside the managed mise root",
        "18936628f195369a80a129c73ee33d23e39086286dab538781ba826effc7e10b",
        "gofumpt_size != 3115666",
        "v0.11.0 (go1.26.5)",
        "go version go1.26.5 darwin/arm64",
        "version -m \"$gofumpt_bin\"",
        "gofumpt_dep_count != 3",
        "golang.org/x/mod\\tv0.38.0\\th1:MECBjubtXD7yj4HrhIUcywNaGeNVUdfVnxmPajOk4yk=",
        "golang.org/x/sync\\tv0.22.0\\th1:SZjpbeLmrCk4xhRSZFNZW5gFUeCeFgjekvI/+gfScek=",
        "golang.org/x/tools\\tv0.48.0\\th1:3+hClM1aLL5mjMKm5ovokw9epgRXPuu2tILgismM6RE=",
        "vcs.revision=5dca7d819315c5c6338d290ad2e7847f07438693",
        "vcs.time=2026-07-27T08:46:00Z",
        "vcs.modified=false",
        "denied-network gofumpt build metadata differs from the reviewed official asset",
    ] {
        assert!(
            inner.contains(required),
            "gofumpt runner omits {required:?}"
        );
    }
}

#[test]
fn goimports_runner_binds_the_exact_source_build_and_denied_network_artifact() {
    let root = repository_root();
    let outer = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");

    for required in [
        "goimports_selected=false",
        "index(\"goimports\") != null",
        "goimports-build-0.48.0",
        "goimports-go1.26.5-mod-cache",
        "golang.org/x/tools@v0.48.0",
        "golang.org/x/mod@v0.38.0",
        "golang.org/x/sync@v0.22.0",
        "golang.org/x/telemetry@v0.0.0-20260708182218-49f421fb7959",
        "8529e7bd696890fd79d3e1c37c7d1a3e2e26fb4b392b5beebfa7134ad2f65755",
        "e6b55566a172ecfd21e5f4a8750f2d25665287288b24ff8d4e6cea5d5078c608",
        "5487d5d99925cc2ad6884e66d70906ac13aa0180d88387bc66f0c706276c2f22",
        "GOPROXY=file://$goimports_mod_cache/cache/download",
        "-ldflags '-s -w -buildid='",
        "2d7d2892651e4452091f0fe8e280c7b6e14f3b6964854516fd7372442d57fd27",
        "validate_goimports_binary",
        "pinned_component_cache_valid",
    ] {
        assert!(
            outer.contains(required),
            "outer goimports runner omits {required:?}"
        );
    }
    for required in [
        "goimports_selected=false",
        "index(\"goimports\") != null",
        "goimports_bin=\"$goimports_root/bin/goimports\"",
        "goimports_go_bin=$(type -P go || true)",
        "denied-network goimports Go resolves outside the managed mise root",
        "observed_size != 5814322",
        "version -m \"$goimports_bin\"",
        "golang.org/x/tools/cmd/goimports",
        "golang.org/x/telemetry\\tv0.0.0-20260708182218-49f421fb7959",
        "validate_goimports_metadata",
        "probe_argv=(\"$goimports_go_bin\" version -m \"$goimports_bin\")",
        "resolved=\"$goimports_bin\"",
    ] {
        assert!(
            inner.contains(required),
            "inner goimports runner omits {required:?}"
        );
    }
}

#[test]
fn golines_runner_binds_the_patched_source_closure_and_denied_network_artifact() {
    let root = repository_root();
    let outer = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract.sh"))
        .expect("outer pinned runner");
    let inner = std::fs::read_to_string(root.join("scripts/run-pinned-tool-contract-inner.sh"))
        .expect("inner pinned runner");

    for required in [
        "golines_selected=false",
        "index(\"golines\") != null",
        "golines-build-0.13.0-vg1",
        "golines/source-build.json",
        "c4a7fcf96b2f1a83440e824340e6d51e15ed34630415e044781a780fc7a2a4d3",
        "8754d400db1f04a71e5e3eb13343bb051afaba153ea9cb9219fb217250adfa4b",
        "21eaf4b83c0df55ae2e7b94ee43fd72a01171bf4ed2729a578b1fc1e54c219fe",
        ".closure.runtimeModuleObjects | length) == 12",
        "golines source-build provenance did not declare exactly 12 runtime modules",
        "golines proxy object differs from the reviewed closure",
        "GOENV=off",
        "GOWORK=off",
        "golines_mod_cache=\"$state_dir/golines-go-mod-cache\"",
        "golines_bootstrap_cache=\"$state_dir/golines-bootstrap-go-build-cache\"",
        "golines_build_cache=\"$state_dir/golines-go-build-cache\"",
        "\"GOMODCACHE=$golines_mod_cache\"",
        "\"GOCACHE=$golines_bootstrap_cache\"",
        "\"GOCACHE=$golines_build_cache\"",
        "exec --locked --fresh-env --deny-net --",
        "-buildvcs=false",
        "0.13.0+velvet-glove.1",
        "4d7bf2a59b9b48bfc234078498b3ddf6a412cf9bd0ce525945bb19d558f6ab75",
        "validate_golines_binary",
        "pinned_component_cache_valid",
    ] {
        assert!(
            outer.contains(required),
            "outer golines runner omits {required:?}"
        );
    }
    let golines_block = outer
        .split("if [[ $golines_selected == true ]]; then")
        .nth(1)
        .expect("golines provisioning block")
        .split("if needs_group ruby; then")
        .next()
        .expect("bounded golines provisioning block");
    assert!(
        !golines_block.contains("mod download all \\")
            && !golines_block.contains("mod download all\n"),
        "golines bootstrap must not expand the reviewed 12-module runtime closure"
    );
    for forbidden in [
        "$golines_build_dir/go-mod-cache",
        "$golines_build_dir/bootstrap-go-build-cache",
        "$golines_build_dir/go-build-cache",
    ] {
        assert!(
            !golines_block.contains(forbidden),
            "golines runner must not place declared persistent caches under its transactional build root: {forbidden:?}"
        );
    }

    let registry: Registry = serde_json::from_str(RECIPES_JSON).expect("strict recipe registry");
    let environment = registry
        .environments
        .iter()
        .find(|environment| environment.id == "macos-arm64-golines")
        .expect("golines environment");
    let component = environment
        .components
        .iter()
        .find(|component| component.id == "golines")
        .expect("golines component");
    let download = environment
        .bootstrap
        .iter()
        .find(|step| step.id == "golines-go-mod-download")
        .expect("golines module download");
    let verify = environment
        .bootstrap
        .iter()
        .find(|step| step.id == "golines-go-mod-verify")
        .expect("golines module verification");
    let build = environment
        .bootstrap
        .iter()
        .find(|step| step.id == "golines-go-build")
        .expect("golines build");
    let assignment = |name: &str| {
        let prefix = format!("{name}=");
        let value = golines_block
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("golines runner omitted {name} assignment"));
        value.trim_matches('"').replace("$state_dir", "{state}")
    };
    let runner_mod_cache = assignment("golines_mod_cache");
    let runner_bootstrap_cache = assignment("golines_bootstrap_cache");
    let runner_build_cache = assignment("golines_build_cache");
    for declared in [
        download.environment.get("GOMODCACHE"),
        verify.environment.get("GOMODCACHE"),
        build.environment.get("GOMODCACHE"),
        component.integrity.build_environment.get("GOMODCACHE"),
    ] {
        assert_eq!(
            declared.map(String::as_str),
            Some(runner_mod_cache.as_str())
        );
    }
    for declared in [
        download.environment.get("GOCACHE"),
        verify.environment.get("GOCACHE"),
    ] {
        assert_eq!(
            declared.map(String::as_str),
            Some(runner_bootstrap_cache.as_str())
        );
    }
    for declared in [
        build.environment.get("GOCACHE"),
        component.integrity.build_environment.get("GOCACHE"),
    ] {
        assert_eq!(
            declared.map(String::as_str),
            Some(runner_build_cache.as_str())
        );
    }
    let source_build: serde_json::Value =
        serde_json::from_str(GOLINES_SOURCE_BUILD_JSON).expect("golines source-build provenance");
    assert_eq!(
        source_build["build"]["environment"]["GOMODCACHE"],
        runner_mod_cache
    );
    assert_eq!(
        source_build["build"]["environment"]["GOCACHE"],
        runner_build_cache
    );
    for index in [1, 2] {
        assert_eq!(
            source_build["build"]["bootstrap"][index]["environment"]["GOMODCACHE"],
            runner_mod_cache
        );
        assert_eq!(
            source_build["build"]["bootstrap"][index]["environment"]["GOCACHE"],
            runner_bootstrap_cache
        );
    }
    for required in [
        "golines_selected=false",
        "index(\"golines\") != null",
        "golines_bin=\"$golines_root/bin/golines\"",
        "golines_go_bin=$(type -P go || true)",
        "denied-network golines Go resolves outside the managed mise root",
        "observed_size != 7341970",
        "version -m \"$golines_bin\"",
        "github.com/segmentio/golines",
        "golang.org/x/tools\\tv0.36.0\\th1:kWS0uv/zsvHEle1LbV5LE8QujrxB3wfQyxHfhOk0Qkg=",
        "validate_golines_metadata",
        "probe_argv=(\"$golines_go_bin\" version -m \"$golines_bin\")",
        "resolved=\"$golines_bin\"",
    ] {
        assert!(
            inner.contains(required),
            "inner golines runner omits {required:?}"
        );
    }
}

fn validate_components<'a>(
    root: &Path,
    components: &'a [Component],
    owner: &str,
) -> BTreeSet<&'a str> {
    let mut ids = BTreeSet::new();
    for component in components {
        assert!(
            ids.insert(component.id.as_str()),
            "{owner}: duplicate component"
        );
        assert!(!component.version.trim().is_empty());
        assert!(!component.installation_source.trim().is_empty());
        assert_unique(
            component.runtime_component_ids.iter().map(String::as_str),
            &format!("{owner}: {} runtime components", component.id),
        );
        validate_integrity(root, &component.integrity, owner);
        validate_probe(&component.probe, owner);
    }
    ids
}

fn validate_bootstrap(root: &Path, steps: &[Bootstrap], owner: &str) {
    let mut ids = BTreeSet::new();
    for step in steps {
        assert!(
            ids.insert(step.id.as_str()),
            "{owner}: duplicate bootstrap id"
        );
        assert!(!step.argv.is_empty(), "{owner}: empty bootstrap argv");
        assert!(
            step.environment
                .iter()
                .all(|(name, value)| !name.trim().is_empty() && !value.trim().is_empty()),
            "{owner}: empty bootstrap environment entry"
        );
        assert!(matches!(step.network.as_str(), "denied" | "required"));
        if let Some(lockfile) = &step.lockfile {
            assert_file(root, lockfile);
        }
    }
}

fn validate_integrity(root: &Path, integrity: &Integrity, owner: &str) {
    assert!(
        matches!(
            integrity.kind.as_str(),
            "mise-lock"
                | "npm-lock"
                | "pip-requirements"
                | "bundler-lock"
                | "host-program"
                | "runtime-bundled"
                | "sha256-archive"
                | "go-source-build"
                | "go-module-build"
        ),
        "{owner}: unsupported integrity kind"
    );
    if integrity.kind == "go-module-build" {
        if owner == "goimports-macos-arm64" {
            assert!(integrity.component_id.is_none());
            assert_eq!(
                integrity.path.as_deref(),
                Some("crates/hookkit-pkl-config/validation/provisioning/goimports/go.mod")
            );
            assert_eq!(
                integrity.url.as_deref(),
                Some("https://proxy.golang.org/golang.org/x/tools/@v/v0.48.0.zip")
            );
            assert_eq!(
                integrity.sha256.as_deref(),
                Some("8529e7bd696890fd79d3e1c37c7d1a3e2e26fb4b392b5beebfa7134ad2f65755")
            );
            assert!(integrity.patch_sha256.is_none());
            assert_eq!(
                integrity.module_manifest_path.as_deref(),
                Some("crates/hookkit-pkl-config/validation/provisioning/goimports/go.mod")
            );
            assert_eq!(
                integrity.module_manifest_sha256.as_deref(),
                Some("9de464c8f30dde87a846b165fadd6620a150e54352265f8b22a7b63959510778")
            );
            assert_eq!(
                integrity.module_lock_path.as_deref(),
                Some("crates/hookkit-pkl-config/validation/provisioning/goimports/go.sum")
            );
            assert_eq!(
                integrity.module_lock_sha256.as_deref(),
                Some("d43f495d37c149ddc7145f20b13b84812ba3aea895834e7595d6eacd62bc7a44")
            );
            assert_eq!(
                integrity.built_artifact_sha256.as_deref(),
                Some("2d7d2892651e4452091f0fe8e280c7b6e14f3b6964854516fd7372442d57fd27")
            );
            assert_eq!(
                integrity.build_toolchain_component_id.as_deref(),
                Some("goimports-go")
            );
            assert_eq!(integrity.build_working_directory.as_deref(), Some("/"));
            assert_eq!(
                integrity.build_argv,
                [
                    "go",
                    "install",
                    "-trimpath",
                    "-ldflags",
                    "-s -w -buildid=",
                    "golang.org/x/tools/cmd/goimports@v0.48.0",
                ]
            );
            assert_eq!(
                integrity.build_environment,
                BTreeMap::from([
                    ("CGO_ENABLED".to_owned(), "0".to_owned()),
                    (
                        "GOBIN".to_owned(),
                        "{state}/goimports-build-0.48.0/install/bin".to_owned()
                    ),
                    (
                        "GOCACHE".to_owned(),
                        "{state}/goimports-build-0.48.0/go-build-cache".to_owned()
                    ),
                    (
                        "GOMODCACHE".to_owned(),
                        "{state}/goimports-go1.26.5-mod-cache".to_owned()
                    ),
                    ("GOOS".to_owned(), "darwin".to_owned()),
                    ("GOARCH".to_owned(), "arm64".to_owned()),
                    ("GOARM64".to_owned(), "v8.0".to_owned()),
                    (
                        "GOPROXY".to_owned(),
                        "file://{state}/goimports-go1.26.5-mod-cache/cache/download".to_owned()
                    ),
                    ("GOSUMDB".to_owned(), "off".to_owned()),
                    ("GOTOOLCHAIN".to_owned(), "local".to_owned()),
                ])
            );
            assert!(integrity.archive_format.is_none());
            assert!(integrity.archive_root.is_none());
            assert_eq!(integrity.min_os_version.as_deref(), Some("12.0"));
            assert_eq!(
                integrity.allowed_dylib_prefixes,
                ["/System/Library/", "/usr/lib/"]
            );

            let module_manifest_path = integrity
                .module_manifest_path
                .as_deref()
                .expect("goimports module manifest path");
            let module_lock_path = integrity
                .module_lock_path
                .as_deref()
                .expect("goimports module lock path");
            assert_file(root, module_manifest_path);
            assert_file(root, module_lock_path);
            let module_manifest =
                std::fs::read_to_string(root.join(module_manifest_path)).expect("goimports go.mod");
            assert_eq!(
                module_manifest,
                "module velvet-glove-goimports-build\n\ngo 1.26.5\n\nrequire golang.org/x/tools v0.48.0\n"
            );
            let module_lock =
                std::fs::read_to_string(root.join(module_lock_path)).expect("goimports go.sum");
            assert_eq!(module_lock.lines().count(), 8);
            for required in [
                "golang.org/x/mod v0.38.0 h1:MECBjubtXD7yj4HrhIUcywNaGeNVUdfVnxmPajOk4yk=",
                "golang.org/x/sync v0.22.0 h1:SZjpbeLmrCk4xhRSZFNZW5gFUeCeFgjekvI/+gfScek=",
                "golang.org/x/telemetry v0.0.0-20260708182218-49f421fb7959 h1:RJhm5l6Fo4rmEIcndxDllNhhf/fAx8qIm4t6A7vpm2A=",
                "golang.org/x/tools v0.48.0 h1:3+hClM1aLL5mjMKm5ovokw9epgRXPuu2tILgismM6RE=",
            ] {
                assert!(module_lock.lines().any(|line| line == required));
            }
            return;
        }
        assert_eq!(owner, "errcheck-macos-arm64");
        assert!(integrity.component_id.is_none());
        assert_eq!(
            integrity.path.as_deref(),
            Some("crates/hookkit-pkl-config/validation/provisioning/errcheck/go.mod")
        );
        assert_eq!(
            integrity.url.as_deref(),
            Some("https://proxy.golang.org/github.com/kisielk/errcheck/@v/v1.20.0.zip")
        );
        assert_eq!(
            integrity.sha256.as_deref(),
            Some("50dbdc1e07128552bda3dad27dfaad9dca100d16869bf58485fe05ed4a45f0b6")
        );
        assert!(integrity.patch_sha256.is_none());
        assert_eq!(
            integrity.module_manifest_path.as_deref(),
            Some("crates/hookkit-pkl-config/validation/provisioning/errcheck/go.mod")
        );
        assert_eq!(
            integrity.module_manifest_sha256.as_deref(),
            Some("06abec38397f045f72e5496d0430dd3473ef2be2fe0187b4d29cd7ff7dd968ef")
        );
        assert_eq!(
            integrity.module_lock_path.as_deref(),
            Some("crates/hookkit-pkl-config/validation/provisioning/errcheck/go.sum")
        );
        assert_eq!(
            integrity.module_lock_sha256.as_deref(),
            Some("594d33a278d8c5313b8b7015f6d8e9590ed0e53ea393296fa9c03ea58a8fa145")
        );
        assert_eq!(
            integrity.built_artifact_sha256.as_deref(),
            Some("4f369aeb1bd8454d6ebb6789fedd948ef216fe04c6be629d5016aca78908aa0c")
        );
        assert_eq!(
            integrity.build_toolchain_component_id.as_deref(),
            Some("errcheck-go")
        );
        assert_eq!(integrity.build_working_directory.as_deref(), Some("/"));
        assert_eq!(
            integrity.build_argv,
            [
                "go",
                "install",
                "-trimpath",
                "-ldflags",
                "-s -w -buildid=",
                "github.com/kisielk/errcheck@v1.20.0",
            ]
        );
        assert_eq!(
            integrity.build_environment,
            BTreeMap::from([
                ("CGO_ENABLED".to_owned(), "0".to_owned()),
                (
                    "GOBIN".to_owned(),
                    "{state}/errcheck-build-1.20.0/install/bin".to_owned()
                ),
                (
                    "GOCACHE".to_owned(),
                    "{state}/errcheck-build-1.20.0/go-build-cache".to_owned()
                ),
                (
                    "GOMODCACHE".to_owned(),
                    "{state}/errcheck-go1.26.5-mod-cache".to_owned()
                ),
                ("GOOS".to_owned(), "darwin".to_owned()),
                ("GOARCH".to_owned(), "arm64".to_owned()),
                (
                    "GOPROXY".to_owned(),
                    "file://{state}/errcheck-go1.26.5-mod-cache/cache/download".to_owned()
                ),
                ("GOSUMDB".to_owned(), "off".to_owned()),
                ("GOTOOLCHAIN".to_owned(), "local".to_owned()),
            ])
        );
        assert!(integrity.archive_format.is_none());
        assert!(integrity.archive_root.is_none());
        assert_eq!(integrity.min_os_version.as_deref(), Some("12.0"));
        assert_eq!(
            integrity.allowed_dylib_prefixes,
            ["/System/Library/", "/usr/lib/"]
        );

        let module_manifest_path = integrity
            .module_manifest_path
            .as_deref()
            .expect("errcheck module manifest path");
        let module_lock_path = integrity
            .module_lock_path
            .as_deref()
            .expect("errcheck module lock path");
        assert_file(root, module_manifest_path);
        assert_file(root, module_lock_path);
        let module_manifest =
            std::fs::read_to_string(root.join(module_manifest_path)).expect("errcheck go.mod");
        assert_eq!(
            module_manifest,
            "module velvet-glove.invalid/errcheck-provisioning\n\ngo 1.26.0\n\ntoolchain go1.26.5\n\nrequire github.com/kisielk/errcheck v1.20.0\n\nrequire (\n\tgolang.org/x/mod v0.35.0 // indirect\n\tgolang.org/x/sync v0.20.0 // indirect\n\tgolang.org/x/tools v0.44.0 // indirect\n)\n"
        );
        let module_lock =
            std::fs::read_to_string(root.join(module_lock_path)).expect("errcheck go.sum");
        assert_eq!(module_lock.lines().count(), 8);
        for required in [
            "github.com/kisielk/errcheck v1.20.0 h1:9rwHBNKzd4wkDWcROy3DvFGNqEPlkxBg305rvk7HabI=",
            "golang.org/x/mod v0.35.0 h1:Ww1D637e6Pg+Zb2KrWfHQUnH2dQRLBQyAtpr/haaJeM=",
            "golang.org/x/sync v0.20.0 h1:e0PTpb7pjO8GAtTs2dQ6jYa5BWYlMuX047Dco/pItO4=",
            "golang.org/x/tools v0.44.0 h1:UP4ajHPIcuMjT1GqzDWRlalUEoY+uzoZKnhOjbIPD2c=",
        ] {
            assert!(module_lock.lines().any(|line| line == required));
        }
        return;
    }
    if integrity.kind == "go-source-build" {
        assert!(
            integrity.component_id.is_none(),
            "{owner}: source build has parent component"
        );
        if integrity.path.as_deref()
            == Some("crates/hookkit-pkl-config/validation/provisioning/golines/closure.patch")
        {
            validate_golines_source_build(root, integrity, owner);
            return;
        }
        if integrity.path.as_deref()
            == Some(
                "crates/hookkit-pkl-config/validation/provisioning/ghalint-workflow/closure.patch",
            )
        {
            validate_ghalint_source_build(root, integrity, owner);
            return;
        }
        assert_eq!(
            integrity.path.as_deref(),
            Some("crates/hookkit-pkl-config/validation/provisioning/betterleaks/closure.patch")
        );
        assert_eq!(
            integrity.url.as_deref(),
            Some("https://github.com/betterleaks/betterleaks/archive/refs/tags/v1.7.3.tar.gz")
        );
        assert_eq!(
            integrity.sha256.as_deref(),
            Some("7359ae820c62c276d31cef3d1431eb8beb6db07d5c44830bad03dbe9c0cf3850")
        );
        assert_eq!(
            integrity.patch_sha256.as_deref(),
            Some("2d57aa396d9c7f0337cf13c05fa06f661099035cb5f753a12e79ca2f46a38147")
        );
        assert_eq!(integrity.archive_format.as_deref(), Some("tar-gz"));
        assert_eq!(integrity.archive_root.as_deref(), Some("betterleaks-1.7.3"));
        assert_eq!(
            integrity.module_manifest_path.as_deref(),
            Some("crates/hookkit-pkl-config/validation/provisioning/betterleaks/go.mod")
        );
        assert_eq!(
            integrity.module_manifest_sha256.as_deref(),
            Some("a669cc877c8dac1c9f3927b57e246902b81bc37665147e4a2d301104f534819e")
        );
        assert_eq!(
            integrity.module_lock_path.as_deref(),
            Some("crates/hookkit-pkl-config/validation/provisioning/betterleaks/go.sum")
        );
        assert_eq!(
            integrity.module_lock_sha256.as_deref(),
            Some("359a55b2abc25a4fa290093fed6bc6d7d3d2923906e4c77cf4d786581a61a38d")
        );
        assert_eq!(
            integrity.built_artifact_sha256.as_deref(),
            Some("046177cad9aa9f924fe57adca4a1a8c54d0ad74ceed593147b127f5a486f8144")
        );
        assert_eq!(
            integrity.build_toolchain_component_id.as_deref(),
            Some("go")
        );
        assert_eq!(
            integrity.build_working_directory.as_deref(),
            Some("{state}/betterleaks-build-1.7.3-vg1/source")
        );
        assert_eq!(
            integrity.build_argv,
            [
                "go",
                "-C",
                "{state}/betterleaks-build-1.7.3-vg1/source",
                "build",
                "-trimpath",
                "-buildvcs=false",
                "-ldflags",
                "-s -w -buildid= -X=github.com/betterleaks/betterleaks/version.Version=1.7.3+velvet-glove.1",
                "-o",
                "{state}/betterleaks-build-1.7.3-vg1/install/bin/betterleaks",
                ".",
            ]
        );
        assert_eq!(
            integrity.build_environment,
            BTreeMap::from([
                ("CGO_ENABLED".to_owned(), "0".to_owned()),
                ("GOARCH".to_owned(), "arm64".to_owned()),
                (
                    "GOCACHE".to_owned(),
                    "{state}/betterleaks-go-build-cache".to_owned()
                ),
                ("GOFLAGS".to_owned(), "-mod=readonly".to_owned()),
                (
                    "GOMODCACHE".to_owned(),
                    "{state}/betterleaks-go-mod-cache".to_owned()
                ),
                ("GOOS".to_owned(), "darwin".to_owned()),
                ("GOPROXY".to_owned(), "off".to_owned()),
                ("GOTOOLCHAIN".to_owned(), "local".to_owned()),
                ("SOURCE_DATE_EPOCH".to_owned(), "1785516069".to_owned()),
            ])
        );
        assert_eq!(integrity.min_os_version.as_deref(), Some("12.0"));
        assert_eq!(
            integrity.allowed_dylib_prefixes,
            ["/System/Library/", "/usr/lib/"]
        );

        let patch_path = integrity.path.as_deref().expect("source patch path");
        let module_manifest_path = integrity
            .module_manifest_path
            .as_deref()
            .expect("module manifest path");
        let module_lock_path = integrity
            .module_lock_path
            .as_deref()
            .expect("module lock path");
        assert_file(root, patch_path);
        assert_file(root, module_manifest_path);
        assert_file(root, module_lock_path);
        let patch = std::fs::read_to_string(root.join(patch_path)).expect("Betterleaks patch");
        assert!(patch.contains("-toolchain go1.25.10\n+toolchain go1.25.12"));
        assert!(patch.contains("-\tgithub.com/klauspost/compress v1.18.6 // indirect"));
        assert!(patch.contains("+\tgithub.com/klauspost/compress v1.18.7 // indirect"));
        assert!(patch.contains("-\tgolang.org/x/text v0.38.0 // indirect"));
        assert!(patch.contains("+\tgolang.org/x/text v0.39.0 // indirect"));
        let module_manifest =
            std::fs::read_to_string(root.join(module_manifest_path)).expect("Betterleaks go.mod");
        assert!(module_manifest.contains("toolchain go1.25.12"));
        assert!(module_manifest.contains("github.com/klauspost/compress v1.18.7"));
        assert!(module_manifest.contains("golang.org/x/text v0.39.0"));
        let module_lock =
            std::fs::read_to_string(root.join(module_lock_path)).expect("Betterleaks go.sum");
        assert!(module_lock.contains(
            "github.com/klauspost/compress v1.18.7 h1:aUyZsS4kH3QTKurYhAOwAHxllVPnOthb3vPfnF1Ehjw="
        ));
        assert!(
            module_lock.contains(
                "golang.org/x/text v0.39.0 h1:UbZz4pLOvn600D6Oh6GGEI6VAmndrEBLv8/6BEXzyus="
            )
        );
        return;
    }
    if integrity.kind == "sha256-archive" {
        assert!(integrity.path.is_none(), "{owner}: archive has lock path");
        assert!(
            integrity.component_id.is_none(),
            "{owner}: archive has parent component"
        );
        assert!(
            integrity
                .url
                .as_deref()
                .is_some_and(|url| url.starts_with("https://")),
            "{owner}: archive URL"
        );
        assert!(
            integrity
                .sha256
                .as_deref()
                .is_some_and(|digest| digest.len() == 64
                    && digest.bytes().all(|byte| byte.is_ascii_hexdigit())),
            "{owner}: archive SHA-256"
        );
        assert!(matches!(
            integrity.archive_format.as_deref(),
            Some("tar-gz" | "tar-xz")
        ));
        assert!(
            integrity
                .archive_root
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            "{owner}: archive root"
        );
        assert!(
            integrity
                .min_os_version
                .as_deref()
                .is_some_and(|value| value.contains('.')),
            "{owner}: archive minimum OS"
        );
        assert_eq!(
            integrity.allowed_dylib_prefixes,
            ["/System/Library/", "/usr/lib/"]
        );
        return;
    }

    if integrity.kind == "runtime-bundled" {
        assert!(
            integrity.path.is_none(),
            "{owner}: bundled component has lock path"
        );
        assert!(
            integrity
                .component_id
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            "{owner}: bundled component has no parent"
        );
        return;
    }

    if integrity.kind == "host-program" {
        assert!(
            integrity.component_id.is_none(),
            "{owner}: host program has parent component"
        );
        assert!(
            integrity
                .path
                .as_deref()
                .is_some_and(|path| path.starts_with("/usr/bin/")),
            "{owner}: host program must use an explicit macOS system shim"
        );
        return;
    }

    assert!(
        integrity.component_id.is_none(),
        "{owner}: lock integrity has parent component"
    );

    let path = integrity
        .path
        .as_deref()
        .unwrap_or_else(|| panic!("{owner}: missing integrity path"));
    assert_file(root, path);
    let contents = std::fs::read_to_string(root.join(path))
        .unwrap_or_else(|error| panic!("{owner}: read {path}: {error}"));
    match integrity.kind.as_str() {
        "npm-lock" => {
            let lock: serde_json::Value =
                serde_json::from_str(&contents).unwrap_or_else(|error| panic!("{owner}: {error}"));
            assert_eq!(lock["lockfileVersion"], 3);
            let packages = lock["packages"]
                .as_object()
                .unwrap_or_else(|| panic!("{owner}: npm lock has no packages"));
            for (path, package) in packages.iter().filter(|(path, _)| !path.is_empty()) {
                assert!(
                    package["version"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty()),
                    "{owner}: npm package {path} has no exact version"
                );
                assert!(
                    package["integrity"]
                        .as_str()
                        .is_some_and(|value| value.starts_with("sha512-")),
                    "{owner}: npm package {path} has no SHA-512 integrity"
                );
            }
        }
        "pip-requirements" => {
            let pins = contents.matches("==").count();
            let hashes = contents.matches("--hash=sha256:").count();
            assert!(pins > 0, "{owner}: no Python package pins");
            assert_eq!(pins, hashes, "{owner}: every Python pin needs one hash");
        }
        "bundler-lock" => {
            assert!(
                contents.contains("\nCHECKSUMS\n"),
                "{owner}: no Bundler checksums"
            );
            assert!(
                contents.contains("\nBUNDLED WITH\n   2.6.9\n"),
                "{owner}: unexpected Bundler version"
            );
            let specs = contents
                .split_once("  specs:\n")
                .expect("Bundler specs")
                .1
                .split_once("\n\nPLATFORMS\n")
                .expect("Bundler platforms")
                .0
                .lines()
                .filter(|line| line.starts_with("    ") && !line.starts_with("      "))
                .count();
            let checksum_lines = contents
                .split_once("\nCHECKSUMS\n")
                .expect("Bundler checksums")
                .1
                .split_once("\n\nBUNDLED WITH\n")
                .expect("Bundler version")
                .0
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect::<Vec<_>>();
            assert_eq!(checksum_lines.len(), specs, "{owner}: incomplete checksums");
            let archive_covered = checksum_lines
                .iter()
                .filter(|line| !line.contains(" sha256="))
                .map(|line| {
                    line.trim()
                        .split_once(" (")
                        .expect("Bundler checksum package")
                        .0
                })
                .collect::<BTreeSet<_>>();
            assert_eq!(
                archive_covered,
                BTreeSet::from(["base64", "racc"]),
                "{owner}: only Ruby-archive packages may omit package hashes"
            );
            assert_eq!(
                checksum_lines
                    .iter()
                    .filter(|line| line.contains(" sha256="))
                    .count(),
                specs - archive_covered.len(),
                "{owner}: package checksum count"
            );
        }
        _ => {}
    }
}

fn validate_ghalint_source_build(root: &Path, integrity: &Integrity, owner: &str) {
    assert_eq!(
        integrity.url.as_deref(),
        Some("https://github.com/suzuki-shunsuke/ghalint/archive/refs/tags/v1.5.6.tar.gz")
    );
    assert_eq!(
        integrity.sha256.as_deref(),
        Some("1188047b654a86390d49b776153c1a7b3eddde30ebcc0d024dfab9585785b02b")
    );
    assert_eq!(
        integrity.patch_sha256.as_deref(),
        Some("5e3c2480665eefffa019adf5c57e27e1c1d05a74b9dccf2d5bc345017a17d6ed")
    );
    assert_eq!(integrity.archive_format.as_deref(), Some("tar-gz"));
    assert_eq!(integrity.archive_root.as_deref(), Some("ghalint-1.5.6"));
    assert_eq!(
        integrity.module_manifest_path.as_deref(),
        Some("crates/hookkit-pkl-config/validation/provisioning/ghalint-workflow/go.mod")
    );
    assert_eq!(
        integrity.module_manifest_sha256.as_deref(),
        Some("ada0a9434578f54fd6a50fe8ed9ef26374afa631d5527660723062663d686f16")
    );
    assert_eq!(
        integrity.module_lock_path.as_deref(),
        Some("crates/hookkit-pkl-config/validation/provisioning/ghalint-workflow/go.sum")
    );
    assert_eq!(
        integrity.module_lock_sha256.as_deref(),
        Some("53a4a1b1a7dcd2a6da2dc1cc0cc32ca4bcb5b8ea86832749e18879b8be594dbb")
    );
    assert_eq!(
        integrity.built_artifact_sha256.as_deref(),
        Some("03437b6c73d1332460d24f2c9fe22d3dea0fe68e4e52b0a8a534b3f2854274fa")
    );
    assert_eq!(
        integrity.build_toolchain_component_id.as_deref(),
        Some("go")
    );
    assert_eq!(
        integrity.build_working_directory.as_deref(),
        Some("{state}/ghalint-build-1.5.6-vg1/source")
    );
    assert_eq!(
        integrity.build_argv,
        [
            "go",
            "-C",
            "{state}/ghalint-build-1.5.6-vg1/source",
            "build",
            "-trimpath",
            "-buildvcs=false",
            "-ldflags",
            "-s -w -buildid= -X=main.version=1.5.6+velvet-glove.1",
            "-o",
            "{state}/ghalint-build-1.5.6-vg1/install/bin/ghalint",
            "./cmd/ghalint",
        ]
    );
    assert_eq!(
        integrity.build_environment,
        BTreeMap::from([
            ("CGO_ENABLED".to_owned(), "0".to_owned()),
            ("GOARCH".to_owned(), "arm64".to_owned()),
            (
                "GOCACHE".to_owned(),
                "{state}/ghalint-go-build-cache".to_owned(),
            ),
            ("GOFLAGS".to_owned(), "-mod=readonly".to_owned()),
            (
                "GOMODCACHE".to_owned(),
                "{state}/ghalint-go-mod-cache".to_owned(),
            ),
            ("GOOS".to_owned(), "darwin".to_owned()),
            ("GOPROXY".to_owned(), "off".to_owned()),
            ("GOTOOLCHAIN".to_owned(), "local".to_owned()),
            ("SOURCE_DATE_EPOCH".to_owned(), "1777591460".to_owned()),
        ])
    );
    assert_eq!(integrity.min_os_version.as_deref(), Some("12.0"));
    assert_eq!(
        integrity.allowed_dylib_prefixes,
        ["/System/Library/", "/usr/lib/"]
    );

    let patch_path = integrity.path.as_deref().expect("ghalint patch path");
    let module_manifest_path = integrity
        .module_manifest_path
        .as_deref()
        .expect("ghalint module manifest path");
    let module_lock_path = integrity
        .module_lock_path
        .as_deref()
        .expect("ghalint module lock path");
    for path in [patch_path, module_manifest_path, module_lock_path] {
        assert_file(root, path);
    }
    let patch = std::fs::read_to_string(root.join(patch_path))
        .unwrap_or_else(|error| panic!("{owner}: read ghalint closure patch: {error}"));
    assert!(patch.contains("-\tgolang.org/x/text v0.28.0 // indirect"));
    assert!(patch.contains("+\tgolang.org/x/text v0.39.0 // indirect"));
    let module_manifest = std::fs::read_to_string(root.join(module_manifest_path))
        .unwrap_or_else(|error| panic!("{owner}: read ghalint go.mod: {error}"));
    assert!(module_manifest.contains("go 1.26.2"));
    assert!(module_manifest.contains("golang.org/x/text v0.39.0"));
    let module_lock = std::fs::read_to_string(root.join(module_lock_path))
        .unwrap_or_else(|error| panic!("{owner}: read ghalint go.sum: {error}"));
    assert!(
        module_lock
            .contains("golang.org/x/text v0.39.0 h1:UbZz4pLOvn600D6Oh6GGEI6VAmndrEBLv8/6BEXzyus=")
    );
}

fn validate_golines_source_build(root: &Path, integrity: &Integrity, owner: &str) {
    assert_eq!(
        integrity.url.as_deref(),
        Some("https://github.com/segmentio/golines/archive/refs/tags/v0.13.0.tar.gz")
    );
    assert_eq!(
        integrity.sha256.as_deref(),
        Some("ec1933e0fb73cf0517fd007d325603007aa65ce430267a70fc78cfea43d9716e")
    );
    assert_eq!(
        integrity.patch_sha256.as_deref(),
        Some("c4a7fcf96b2f1a83440e824340e6d51e15ed34630415e044781a780fc7a2a4d3")
    );
    assert_eq!(integrity.archive_format.as_deref(), Some("tar-gz"));
    assert_eq!(integrity.archive_root.as_deref(), Some("golines-0.13.0"));
    assert_eq!(
        integrity.module_manifest_path.as_deref(),
        Some("crates/hookkit-pkl-config/validation/provisioning/golines/go.mod")
    );
    assert_eq!(
        integrity.module_manifest_sha256.as_deref(),
        Some("8754d400db1f04a71e5e3eb13343bb051afaba153ea9cb9219fb217250adfa4b")
    );
    assert_eq!(
        integrity.module_lock_path.as_deref(),
        Some("crates/hookkit-pkl-config/validation/provisioning/golines/go.sum")
    );
    assert_eq!(
        integrity.module_lock_sha256.as_deref(),
        Some("21eaf4b83c0df55ae2e7b94ee43fd72a01171bf4ed2729a578b1fc1e54c219fe")
    );
    assert_eq!(
        integrity.built_artifact_sha256.as_deref(),
        Some("4d7bf2a59b9b48bfc234078498b3ddf6a412cf9bd0ce525945bb19d558f6ab75")
    );
    assert_eq!(
        integrity.build_toolchain_component_id.as_deref(),
        Some("golines-go")
    );
    assert_eq!(
        integrity.build_working_directory.as_deref(),
        Some("{state}/golines-build-0.13.0-vg1/source")
    );
    assert_eq!(
        integrity.build_argv,
        [
            "go",
            "-C",
            "{state}/golines-build-0.13.0-vg1/source",
            "build",
            "-trimpath",
            "-buildvcs=false",
            "-ldflags",
            "-s -w -buildid= -X=main.version=0.13.0+velvet-glove.1 -X=main.commit=8f32f0f7e89c30f572c7f2cd3b2a48016b9d8bbf -X=main.date=2025-08-21T21:22:01Z",
            "-o",
            "{state}/golines-build-0.13.0-vg1/install/bin/golines",
            ".",
        ]
    );
    assert_eq!(
        integrity.build_environment,
        BTreeMap::from([
            ("CGO_ENABLED".to_owned(), "0".to_owned()),
            ("GOARCH".to_owned(), "arm64".to_owned()),
            ("GOARM64".to_owned(), "v8.0".to_owned()),
            (
                "GOCACHE".to_owned(),
                "{state}/golines-go-build-cache".to_owned(),
            ),
            ("GOENV".to_owned(), "off".to_owned()),
            ("GOFLAGS".to_owned(), "-mod=readonly".to_owned()),
            (
                "GOMODCACHE".to_owned(),
                "{state}/golines-go-mod-cache".to_owned(),
            ),
            ("GOOS".to_owned(), "darwin".to_owned()),
            ("GOPROXY".to_owned(), "off".to_owned()),
            ("GOSUMDB".to_owned(), "off".to_owned()),
            ("GOTOOLCHAIN".to_owned(), "local".to_owned()),
            ("GOWORK".to_owned(), "off".to_owned()),
            ("SOURCE_DATE_EPOCH".to_owned(), "1755811321".to_owned()),
        ])
    );
    assert_eq!(integrity.min_os_version.as_deref(), Some("12.0"));
    assert_eq!(
        integrity.allowed_dylib_prefixes,
        ["/System/Library/", "/usr/lib/"]
    );

    for path in [
        integrity.path.as_deref().expect("golines patch path"),
        integrity
            .module_manifest_path
            .as_deref()
            .expect("golines module manifest"),
        integrity
            .module_lock_path
            .as_deref()
            .expect("golines module lock"),
    ] {
        assert_file(root, path);
    }
    let provenance: serde_json::Value = serde_json::from_str(GOLINES_SOURCE_BUILD_JSON)
        .unwrap_or_else(|error| panic!("{owner}: parse golines source build: {error}"));
    assert_eq!(provenance["schemaVersion"], 1);
    assert_eq!(provenance["status"], "integrated");
    assert_eq!(
        provenance["upstream"]["peeledCommit"],
        "8f32f0f7e89c30f572c7f2cd3b2a48016b9d8bbf"
    );
    assert_eq!(provenance["toolchain"]["componentId"], "golines-go");
    assert_eq!(provenance["upstream"]["repositoryArchived"], true);
    assert_eq!(provenance["upstream"]["finalRelease"], "v0.13.0");
    assert_eq!(provenance["upstream"]["tagKind"], "lightweight");
    assert_eq!(
        provenance["upstream"]["commitVerification"]["verified"],
        true
    );
    assert_eq!(
        provenance["upstream"]["license"]["sha256"],
        "d6d71a1f7dc6539e371120cc7af6e3257e55ca79634d473211f217b8965b0f16"
    );
    assert_eq!(
        provenance["upstream"]["moduleProxy"]["sha256"],
        "5166daf66491c02c7311e41009b6af6cafa7382a070b852171107b16567f806e"
    );
    assert_eq!(
        provenance["upstream"]["moduleProxy"]["moduleSum"],
        "h1:GfbpsxoF4eYuEZD3mxrlsN/XD30m6nOO4QLQj2JIa90="
    );
    assert_eq!(
        provenance["excludedArtifacts"]["officialDarwinUniversal"]["embeddedGoVersion"],
        "go1.24.6"
    );
    assert_eq!(
        provenance["excludedArtifacts"]["officialDarwinUniversal"]["retrievableArtifactAttestation"],
        false
    );
    assert_eq!(
        provenance["excludedArtifacts"]["unpatchedGo1265ModuleBuild"]["vulnerableDependency"],
        "golang.org/x/crypto@v0.41.0"
    );
    assert_eq!(
        provenance["excludedArtifacts"]["unpatchedGo1265ModuleBuild"]["govulncheckFindingIds"]
            .as_array()
            .expect("unpatched golines vulnerability IDs")
            .len(),
        17
    );
    assert_eq!(
        provenance["artifact"]["size"],
        serde_json::Value::from(7_341_970_u64)
    );
    assert_eq!(
        provenance["artifact"]["sha256"],
        "4d7bf2a59b9b48bfc234078498b3ddf6a412cf9bd0ce525945bb19d558f6ab75"
    );
    assert_eq!(
        provenance["closure"]["runtimeModuleObjects"]
            .as_array()
            .expect("golines runtime module objects")
            .len(),
        12
    );
    let download = provenance["build"]["bootstrap"][1]["argv"]
        .as_array()
        .expect("golines module download argv");
    assert_eq!(download.len(), 17);
    assert!(!download.iter().any(|argument| argument == "all"));
    for environment in [
        &provenance["build"]["environment"],
        &provenance["build"]["bootstrap"][1]["environment"],
        &provenance["build"]["bootstrap"][2]["environment"],
    ] {
        assert_eq!(environment["GOENV"], "off");
        assert_eq!(environment["GOWORK"], "off");
    }
    assert_eq!(
        provenance["build"]["argv"],
        serde_json::to_value(&integrity.build_argv).expect("golines build argv JSON")
    );
    assert_eq!(
        provenance["build"]["environment"],
        serde_json::to_value(&integrity.build_environment).expect("golines build environment JSON")
    );
    assert_eq!(
        provenance["vulnerabilityEvidence"]["source"]["result"],
        "no vulnerabilities found"
    );
    assert_eq!(
        provenance["vulnerabilityEvidence"]["binary"]["result"],
        "no vulnerabilities found"
    );
}

fn validate_component_integrity(root: &Path, mise_lock: &str, component: &Component) {
    match component.integrity.kind.as_str() {
        "mise-lock" => {
            let selector = component
                .mise_tool
                .as_deref()
                .unwrap_or_else(|| panic!("{}: missing mise selector", component.id));
            let (mise_id, mise_version) = selector
                .split_once('@')
                .unwrap_or_else(|| panic!("{}: invalid mise selector", component.id));
            assert_eq!(
                mise_version, component.version,
                "{}: mise version",
                component.id
            );
            let header = format!("[[tools.{mise_id}]]");
            let section = mise_lock
                .split_once(&header)
                .unwrap_or_else(|| panic!("{}: missing mise lock entry", component.id))
                .1
                .split("\n[[tools.")
                .next()
                .expect("mise lock section");
            assert!(
                section.contains(&format!("version = {:?}", component.version)),
                "{}: mise lock version mismatch",
                component.id
            );
            assert!(
                section.contains("[tools.") && section.contains("platforms.macos-arm64"),
                "{}: no macOS arm64 lock entry",
                component.id
            );
            assert!(
                section.contains("checksum = \"sha256:"),
                "{}: no SHA-256 artifact checksum",
                component.id
            );
            assert!(
                section.contains("url = \"https://"),
                "{}: no immutable artifact URL",
                component.id
            );
            if component.id == "jq" {
                assert_eq!(component.version, "1.8.2");
                assert!(section.contains(
                    "checksum = \"sha256:2d75340ba57a4b4b4c8708a21c2dc8e958a48aaa8bba13b27f77f6e4c0eca07e\""
                ));
                assert!(section.contains(
                    "url = \"https://github.com/jqlang/jq/releases/download/jq-1.8.2/jq-macos-arm64\""
                ));
                assert!(section.contains("provenance = \"github-attestations\""));
            }
            if matches!(component.id.as_str(), "go" | "errcheck-go" | "golines-go") {
                assert_eq!(component.version, "1.26.5");
                assert!(section.contains(
                    "checksum = \"sha256:efb87ff28af9a188d0536ef5d42e63dd52ba8263cd7344a993cc48dd11dedb6a\""
                ));
                if matches!(component.id.as_str(), "errcheck-go" | "golines-go") {
                    assert_eq!(selector, "go@1.26.5");
                    assert_eq!(
                        component.integrity.sha256.as_deref(),
                        Some("efb87ff28af9a188d0536ef5d42e63dd52ba8263cd7344a993cc48dd11dedb6a")
                    );
                }
            }
        }
        "sha256-archive" => {
            assert_eq!(component.mise_tool, None);
            match component.id.as_str() {
                "rust" => {
                    assert_eq!(component.version, "1.90.0");
                    assert_eq!(
                        component.integrity.sha256.as_deref(),
                        Some("9772d20d5cd736079a0ee84d00e6697cf2084f0fc4621b011e24e6f2d08d2d7f")
                    );
                    assert_eq!(component.integrity.min_os_version.as_deref(), Some("11.0"));
                }
                "rustfmt" => {
                    assert_eq!(component.version, "1.8.0");
                    assert_eq!(
                        component.integrity.sha256.as_deref(),
                        Some("ab2c6bdac9d3742de4a52c42aea132709b4f0bf66d4f14479f27b18543c255be")
                    );
                    assert_eq!(component.integrity.min_os_version.as_deref(), Some("11.0"));
                    assert_eq!(component.runtime_component_ids, ["rust"]);
                }
                "cargo-clippy-toolchain" => {
                    assert_eq!(component.version, "1.97.1");
                    assert_eq!(
                        component.integrity.sha256.as_deref(),
                        Some("c9748cc86107734a2a024069908a895de7caa2d37062fb641eef9f756938ace2")
                    );
                    assert_eq!(component.integrity.min_os_version.as_deref(), Some("11.0"));
                    assert!(component.runtime_component_ids.is_empty());
                    assert_eq!(
                        component.install_components,
                        [
                            "rustc",
                            "rust-std-aarch64-apple-darwin",
                            "cargo",
                            "clippy-preview",
                            "rustfmt-preview",
                        ]
                    );
                }
                "prettier-node" => {
                    assert_eq!(component.version, "24.19.0");
                    assert_eq!(
                        component.integrity.url.as_deref(),
                        Some("https://nodejs.org/dist/v24.19.0/node-v24.19.0-darwin-arm64.tar.gz")
                    );
                    assert_eq!(
                        component.integrity.sha256.as_deref(),
                        Some("8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d")
                    );
                    assert_eq!(component.integrity.min_os_version.as_deref(), Some("11.0"));
                    assert!(component.runtime_component_ids.is_empty());
                    assert_eq!(component.probe.argv, ["prettier-node", "--version"]);
                    assert_eq!(component.probe.expected, "v24.19.0");
                }
                "contextlint-node" => {
                    assert_eq!(component.version, "24.19.0");
                    assert_eq!(
                        component.integrity.url.as_deref(),
                        Some("https://nodejs.org/dist/v24.19.0/node-v24.19.0-darwin-arm64.tar.gz")
                    );
                    assert_eq!(
                        component.integrity.sha256.as_deref(),
                        Some("8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d")
                    );
                    assert_eq!(component.integrity.min_os_version.as_deref(), Some("11.0"));
                    assert!(component.runtime_component_ids.is_empty());
                    assert_eq!(component.probe.argv, ["contextlint-node", "--version"]);
                    assert_eq!(component.probe.expected, "v24.19.0");
                }
                "dclint-node" => {
                    assert_eq!(component.version, "24.19.0");
                    assert_eq!(
                        component.integrity.url.as_deref(),
                        Some("https://nodejs.org/dist/v24.19.0/node-v24.19.0-darwin-arm64.tar.gz")
                    );
                    assert_eq!(
                        component.integrity.sha256.as_deref(),
                        Some("8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d")
                    );
                    assert_eq!(component.integrity.min_os_version.as_deref(), Some("11.0"));
                    assert!(component.runtime_component_ids.is_empty());
                    assert_eq!(component.probe.argv, ["dclint-node", "--version"]);
                    assert_eq!(component.probe.expected, "v24.19.0");
                }
                "vacuum" => {
                    assert_eq!(component.version, "0.30.0");
                    assert_eq!(
                        component.integrity.url.as_deref(),
                        Some(
                            "https://github.com/daveshanley/vacuum/releases/download/v0.30.0/vacuum_0.30.0_darwin_arm64.tar.gz"
                        )
                    );
                    assert_eq!(
                        component.integrity.sha256.as_deref(),
                        Some("bebcc32f58db734bcf329ef6f0754d2b1051d55961ee92aac1d2b1192fad78e8")
                    );
                    assert_eq!(component.integrity.archive_root.as_deref(), Some("."));
                    assert_eq!(component.integrity.min_os_version.as_deref(), Some("12.0"));
                    assert!(component.runtime_component_ids.is_empty());
                    assert_eq!(component.probe.argv, ["vacuum", "version"]);
                    assert_eq!(component.probe.expected, "0.30.0");
                }
                "eslint-node" => {
                    assert_eq!(component.version, "24.19.0");
                    assert_eq!(
                        component.integrity.url.as_deref(),
                        Some("https://nodejs.org/dist/v24.19.0/node-v24.19.0-darwin-arm64.tar.gz")
                    );
                    assert_eq!(
                        component.integrity.sha256.as_deref(),
                        Some("8294b7aa9b03997481c06babf1e8b270c859358f27da57a11509afe537ac381d")
                    );
                    assert_eq!(component.integrity.min_os_version.as_deref(), Some("11.0"));
                    assert!(component.runtime_component_ids.is_empty());
                    assert_eq!(component.probe.argv, ["eslint-node", "--version"]);
                    assert_eq!(component.probe.expected, "v24.19.0");
                }
                "ruby" => {
                    assert_eq!(component.version, "3.4.10");
                    assert_eq!(
                        component.integrity.sha256.as_deref(),
                        Some("5aac8b6e16b0938a14f6d23346b5487020d1fea6b5856a33392e5eb37fa1ef62")
                    );
                    assert_eq!(component.integrity.min_os_version.as_deref(), Some("14.0"));
                }
                other => panic!("unexpected archive component {other}"),
            }
        }
        "npm-lock" => {
            assert_eq!(component.mise_tool, None);
            assert_eq!(
                component.runtime_component_ids,
                ["node"],
                "{}: Node runtime dependency",
                component.id
            );
            let (expected_integrity, package_probe) = match component.id.as_str() {
                "@astrojs/check" => (
                    "sha512-zgx/UQMozdjOa3bOxjgeCFdtpE3c9rRX6xHwa+2QXvy8z8Akifu2AtubHyv/zzC2znO8dl8fFWL4K+Ba9kS8HQ==",
                    "JSON.parse(require('node:fs').readFileSync(require('node:path').join(require.resolve('@astrojs/check'), '..', '..', 'package.json'))).version",
                ),
                "typescript" => (
                    "sha512-y2TvuxSZPDyQakkFRPZHKFm+KKVqIisdg9/CZwm9ftvKXLP8NRWj38/ODjNbr43SsoXqNuAisEf1GdCxqWcdBw==",
                    "require('typescript/package.json').version",
                ),
                other => panic!("unexpected npm-locked component {other}"),
            };
            validate_npm_lock_package(
                root,
                &component.integrity,
                &component.id,
                &component.version,
                expected_integrity,
            );
            assert_eq!(
                component
                    .probe
                    .argv
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                ["node", "-p", package_probe],
                "{}: observed package probe",
                component.id
            );
            assert_eq!(component.probe.match_kind, "exact");
            assert_eq!(component.probe.expected, component.version);
        }
        "go-source-build" => {
            assert_eq!(component.mise_tool, None);
            assert!(component.runtime_component_ids.is_empty());
            assert_eq!(component.probe.match_kind, "exact");
            match component.id.as_str() {
                "betterleaks" => {
                    assert_eq!(component.version, "1.7.3+velvet-glove.1");
                    assert_eq!(component.probe.argv, ["betterleaks", "--version"]);
                    assert_eq!(
                        component.probe.expected,
                        "betterleaks version 1.7.3+velvet-glove.1"
                    );
                }
                "ghalint-workflow" => {
                    assert_eq!(component.version, "1.5.6+velvet-glove.1");
                    assert_eq!(component.probe.argv, ["ghalint", "--version"]);
                    assert_eq!(
                        component.probe.expected,
                        "ghalint version 1.5.6+velvet-glove.1"
                    );
                }
                "golines" => {
                    assert_eq!(component.version, "0.13.0+velvet-glove.1");
                    assert_eq!(component.probe.argv, ["golines", "--version"]);
                    assert_eq!(
                        component.probe.expected,
                        "golines v0.13.0+velvet-glove.1\n\nbuild information:\n\tbuild date: 2025-08-21T21:22:01Z\n\tgit commit ref: 8f32f0f7e89c30f572c7f2cd3b2a48016b9d8bbf"
                    );
                }
                other => panic!("unexpected Go source-build component {other}"),
            }
        }
        "runtime-bundled" => {
            assert_eq!(component.mise_tool, None);
            match component.id.as_str() {
                "bundler" => {
                    assert_eq!(component.integrity.component_id.as_deref(), Some("ruby"));
                    let gem_lock = std::fs::read_to_string(root.join(
                        "crates/hookkit-pkl-config/validation/provisioning/ruby/Gemfile.lock",
                    ))
                    .expect("Gemfile lock");
                    assert!(gem_lock.contains("\nBUNDLED WITH\n   2.6.9\n"));
                }
                "base64" | "racc" => {
                    assert_eq!(component.integrity.component_id.as_deref(), Some("ruby"));
                    assert!(matches!(
                        (component.id.as_str(), component.version.as_str()),
                        ("base64", "0.2.0") | ("racc", "1.8.1")
                    ));
                }
                "pip" => {
                    assert_eq!(component.integrity.component_id.as_deref(), Some("python"));
                    assert!(mise_lock.contains("[[tools.python]]\nversion = \"3.14.5\""));
                    assert_eq!(component.version, "26.1.1");
                }
                "cargo-clippy-cargo" => {
                    assert_eq!(
                        component.integrity.component_id.as_deref(),
                        Some("cargo-clippy-toolchain")
                    );
                    assert_eq!(component.version, "1.97.1");
                }
                "cargo-fmt-driver" | "cargo-fmt-rustfmt" => {
                    assert_eq!(
                        component.integrity.component_id.as_deref(),
                        Some("cargo-clippy-toolchain")
                    );
                    assert_eq!(component.version, "1.9.0");
                    assert_eq!(component.probe.match_kind, "exact");
                    assert_eq!(
                        component.probe.expected,
                        "rustfmt 1.9.0-stable (8bab26f4f6 2026-07-14)"
                    );
                }
                "clippy" => {
                    assert_eq!(
                        component.integrity.component_id.as_deref(),
                        Some("cargo-clippy-toolchain")
                    );
                    assert_eq!(component.version, "0.1.97");
                }
                "prettier-npm" => {
                    assert_eq!(
                        component.integrity.component_id.as_deref(),
                        Some("prettier-node")
                    );
                    assert_eq!(component.version, "11.17.0");
                    assert_eq!(component.probe.argv, ["prettier-npm", "--version"]);
                    assert_eq!(component.probe.expected, "11.17.0");
                }
                "contextlint-npm" => {
                    assert_eq!(
                        component.integrity.component_id.as_deref(),
                        Some("contextlint-node")
                    );
                    assert_eq!(component.version, "11.17.0");
                    assert_eq!(component.probe.argv, ["contextlint-npm", "--version"]);
                    assert_eq!(component.probe.expected, "11.17.0");
                }
                "dclint-npm" => {
                    assert_eq!(
                        component.integrity.component_id.as_deref(),
                        Some("dclint-node")
                    );
                    assert_eq!(component.version, "11.17.0");
                    assert_eq!(component.probe.argv, ["dclint-npm", "--version"]);
                    assert_eq!(component.probe.expected, "11.17.0");
                }
                "eslint-npm" => {
                    assert_eq!(
                        component.integrity.component_id.as_deref(),
                        Some("eslint-node")
                    );
                    assert_eq!(component.version, "11.17.0");
                    assert_eq!(component.probe.argv, ["eslint-npm", "--version"]);
                    assert_eq!(component.probe.expected, "11.17.0");
                }
                other => panic!("unexpected runtime-bundled component {other}"),
            }
        }
        "bundler-lock" => {
            assert_eq!(component.mise_tool, None);
            let gem_lock = std::fs::read_to_string(
                root.join("crates/hookkit-pkl-config/validation/provisioning/ruby/Gemfile.lock"),
            )
            .expect("Gemfile lock");
            let expected = match component.id.as_str() {
                "benchmark" => {
                    "benchmark (0.4.0) sha256=0f12f8c495545e3710c3e4f0480f63f06b4c842cc94cec7f33a956f5180e874a"
                }
                "ostruct" => {
                    "ostruct (0.6.1) sha256=09a3fb7ecc1fa4039f25418cc05ae9c82bd520472c5c6a6f515f03e4988cb817"
                }
                other => panic!("unexpected Bundler-locked component {other}"),
            };
            assert!(gem_lock.contains(expected));
        }
        "host-program" => {
            assert_eq!(component.mise_tool, None);
            let expected = match component.id.as_str() {
                "apple-clang" => ("major >=17", "/usr/bin/cc"),
                "apple-diff" => ("Apple diff (based on FreeBSD diff)", "/usr/bin/diff"),
                "macos-sdk" => ("major >=26", "/usr/bin/xcrun"),
                "xcode" => ("major >=26", "/usr/bin/xcodebuild"),
                other => panic!("unexpected host program component {other}"),
            };
            assert_eq!(component.version, expected.0);
            assert_eq!(component.integrity.path.as_deref(), Some(expected.1));
        }
        other => panic!("{}: invalid component integrity {other}", component.id),
    }
}

fn validate_npm_lock_package(
    root: &Path,
    integrity: &Integrity,
    package_id: &str,
    expected_version: &str,
    expected_integrity: &str,
) {
    assert_eq!(integrity.kind, "npm-lock", "{package_id}: integrity kind");
    let lock_path = integrity
        .path
        .as_deref()
        .unwrap_or_else(|| panic!("{package_id}: missing npm lock path"));
    let lock: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(lock_path))
            .unwrap_or_else(|error| panic!("{package_id}: read {lock_path}: {error}")),
    )
    .unwrap_or_else(|error| panic!("{package_id}: parse {lock_path}: {error}"));
    assert_eq!(
        lock["packages"][""]["dependencies"][package_id], expected_version,
        "{package_id}: root dependency pin"
    );
    validate_npm_lock_entry(
        root,
        integrity,
        package_id,
        expected_version,
        expected_integrity,
    );
}

fn validate_npm_lock_entry(
    root: &Path,
    integrity: &Integrity,
    package_id: &str,
    expected_version: &str,
    expected_integrity: &str,
) {
    assert_eq!(integrity.kind, "npm-lock", "{package_id}: integrity kind");
    let lock_path = integrity
        .path
        .as_deref()
        .unwrap_or_else(|| panic!("{package_id}: missing npm lock path"));
    let lock: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(lock_path))
            .unwrap_or_else(|error| panic!("{package_id}: read {lock_path}: {error}")),
    )
    .unwrap_or_else(|error| panic!("{package_id}: parse {lock_path}: {error}"));
    let package_path = format!("node_modules/{package_id}");
    let package = &lock["packages"][&package_path];
    assert_eq!(
        package["version"], expected_version,
        "{package_id}: lock version"
    );
    assert_eq!(
        package["integrity"], expected_integrity,
        "{package_id}: lock integrity"
    );
}

fn validate_probe(probe: &Probe, owner: &str) {
    assert!(!probe.argv.is_empty(), "{owner}: empty probe argv");
    assert!(
        matches!(
            probe.match_kind.as_str(),
            "exact" | "prefix" | "major-at-least"
        ),
        "{owner}: unsupported probe match"
    );
    assert!(
        !probe.expected.is_empty(),
        "{owner}: empty probe expectation"
    );
    if probe.match_kind == "major-at-least" {
        assert!(
            probe.expected.bytes().all(|byte| byte.is_ascii_digit()),
            "{owner}: invalid minimum major"
        );
    }
}

fn assert_unique<'a>(items: impl Iterator<Item = &'a str>, label: &str) {
    let mut seen = BTreeSet::new();
    for item in items {
        assert!(seen.insert(item), "{label}: duplicate {item}");
    }
}

fn assert_file(root: &Path, relative: &str) {
    let path = root.join(relative);
    assert!(
        path.is_file(),
        "required file is missing: {}",
        path.display()
    );
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
