use hookkit_pkl_config::{Architecture, NetworkPolicy, Platform, SupportState, UpstreamProvenance};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const RECIPES_JSON: &str = include_str!("../validation/provisioning/recipes.json");

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
            "go-fmt",
            "jq",
            "prettier",
            "rubocop",
            "rustfmt",
            "sort-package-json",
            "swiftlint"
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
        ),
        "{owner}: unsupported integrity kind"
    );
    if integrity.kind == "go-source-build" {
        assert!(
            integrity.component_id.is_none(),
            "{owner}: source build has parent component"
        );
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

fn validate_component_integrity(root: &Path, mise_lock: &str, component: &Component) {
    match component.integrity.kind.as_str() {
        "mise-lock" => {
            let expected_selector = format!("{}@{}", component.id, component.version);
            assert_eq!(
                component.mise_tool.as_deref(),
                Some(expected_selector.as_str()),
                "{}: mise install selector",
                component.id
            );
            let header = format!("[[tools.{}]]", component.id);
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
            if component.id == "go" {
                assert_eq!(component.version, "1.26.5");
                assert!(section.contains(
                    "checksum = \"sha256:efb87ff28af9a188d0536ef5d42e63dd52ba8263cd7344a993cc48dd11dedb6a\""
                ));
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
            assert_eq!(component.id, "betterleaks");
            assert_eq!(component.version, "1.7.3+velvet-glove.1");
            assert_eq!(component.mise_tool, None);
            assert!(component.runtime_component_ids.is_empty());
            assert_eq!(component.probe.argv, ["betterleaks", "--version"]);
            assert_eq!(component.probe.match_kind, "exact");
            assert_eq!(
                component.probe.expected,
                "betterleaks version 1.7.3+velvet-glove.1"
            );
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
