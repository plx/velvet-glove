use hookkit_pkl_config::{Architecture, NetworkPolicy, Platform, SupportState, UpstreamProvenance};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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
            "data-formats",
            "go",
            "node",
            "python",
            "ruby",
            "rust",
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
            "black",
            "go-fmt",
            "jq",
            "rubocop",
            "rustfmt",
            "sort-package-json",
            "swiftlint"
        ])
    );
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
        ),
        "{owner}: unsupported integrity kind"
    );
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
