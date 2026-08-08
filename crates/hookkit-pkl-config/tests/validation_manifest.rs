use hookkit_pkl_config::{
    Capability, ContractCase, EvidenceStatus, EvidenceSurface, EvidenceTier, LayerState,
    ValidationException, ValidationManifest,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn load() -> (
    ValidationManifest,
    BTreeMap<String, hookkit_pkl_config::ToolSpec>,
    BTreeMap<String, BTreeSet<String>>,
) {
    let manifest = hookkit_pkl_config::builtin_validation_manifest().expect("decode manifest");
    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    (manifest, specs, fixture_inventory())
}

fn fixture_inventory() -> BTreeMap<String, BTreeSet<String>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../velvet-glove/tests/tool-fixtures");
    let mut inventory = BTreeMap::new();
    for tool in
        std::fs::read_dir(&root).unwrap_or_else(|error| panic!("read {}: {error}", root.display()))
    {
        let tool = tool.unwrap_or_else(|error| panic!("read entry in {}: {error}", root.display()));
        if !tool
            .file_type()
            .unwrap_or_else(|error| panic!("read type for {}: {error}", tool.path().display()))
            .is_dir()
        {
            continue;
        }
        let mut cases = BTreeSet::new();
        for case in std::fs::read_dir(tool.path())
            .unwrap_or_else(|error| panic!("read {}: {error}", tool.path().display()))
        {
            let case = case
                .unwrap_or_else(|error| panic!("read entry in {}: {error}", tool.path().display()));
            if case
                .file_type()
                .unwrap_or_else(|error| panic!("read type for {}: {error}", case.path().display()))
                .is_dir()
            {
                cases.insert(case.file_name().to_string_lossy().into_owned());
            }
        }
        inventory.insert(tool.file_name().to_string_lossy().into_owned(), cases);
    }
    inventory
}

#[test]
fn validation_manifest_matches_catalog_and_fixtures() {
    let (manifest, specs, fixtures) = load();
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../validation/manifest.schema.json"))
            .expect("validation manifest JSON Schema is valid JSON");
    assert_eq!(manifest.schema, "./manifest.schema.json");
    assert_eq!(schema["$defs"]["tool"]["type"], "object");

    let summary = hookkit_pkl_config::validate_manifest(
        &manifest,
        &specs,
        Some(&fixtures),
        &hookkit_pkl_config::current_utc_date(),
    )
    .expect("valid catalog coverage manifest");

    assert_eq!(summary.total_tools, 134);
    assert_eq!(summary.enabled_tools, 122);
    assert_eq!(summary.disabled_tools, 12);
    assert_eq!(summary.fixture_tools, 108);
    assert_eq!(summary.fixture_cases, 179);
    assert_eq!(summary.layers[&EvidenceTier::Schema].covered, 134);
    assert_eq!(summary.layers[&EvidenceTier::RenderedCommand].gap, 122);
    assert_eq!(summary.layers[&EvidenceTier::PinnedRealTool].gap, 122);
    assert_eq!(
        summary.tools[0].surface_layers[&EvidenceSurface::Immediate]
            [&EvidenceTier::RenderedCommand],
        if summary.tools[0].support == hookkit_pkl_config::SupportState::Enabled {
            LayerState::Gap
        } else {
            LayerState::NotRequired
        }
    );

    let encoded = serde_json::to_value(&summary).expect("serialize coverage");
    assert_eq!(encoded["enabledTools"], 122);
    assert!(encoded["tools"].is_array());

    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut spec_paths = BTreeSet::new();
    let mut tracking_issues = BTreeSet::new();
    for tool in &manifest.tools {
        let expected_path = format!(
            "crates/hookkit-pkl-config/src/builtins/tools/{}.pkl",
            camel_to_snake(&tool.builtin)
        );
        assert_eq!(tool.spec_path, expected_path, "{} specPath", tool.builtin);
        assert!(
            repository_root.join(&tool.spec_path).is_file(),
            "{} names missing specPath {}",
            tool.builtin,
            tool.spec_path
        );
        assert!(spec_paths.insert(&tool.spec_path), "duplicate specPath");
        assert!(
            tracking_issues.insert(tool.tracking_issue),
            "duplicate trackingIssue {}",
            tool.tracking_issue
        );
    }
}

fn camel_to_snake(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        if character.is_ascii_uppercase() {
            result.push('_');
            result.push(character.to_ascii_lowercase());
        } else {
            result.push(character);
        }
    }
    result
}

#[test]
fn validation_manifest_rejects_missing_duplicate_and_orphan_declarations() {
    let (manifest, specs, fixtures) = load();

    let mut missing = manifest.clone();
    missing.tools.remove(0);
    let error =
        hookkit_pkl_config::validate_manifest(&missing, &specs, Some(&fixtures), "2026-08-08")
            .expect_err("missing declaration must fail");
    assert!(error.to_string().contains("has no validation declaration"));

    let mut duplicate = manifest.clone();
    duplicate.tools.push(duplicate.tools[0].clone());
    let error =
        hookkit_pkl_config::validate_manifest(&duplicate, &specs, Some(&fixtures), "2026-08-08")
            .expect_err("duplicate declaration must fail");
    assert!(error.to_string().contains("repeats builtin declaration"));

    let mut orphan = fixtures;
    orphan.insert("not-a-builtin".into(), BTreeSet::from(["clean".into()]));
    let error =
        hookkit_pkl_config::validate_manifest(&manifest, &specs, Some(&orphan), "2026-08-08")
            .expect_err("orphan fixture must fail");
    assert!(error.to_string().contains("has no validation declaration"));

    let mut orphan_case = fixture_inventory();
    orphan_case
        .entry(manifest.tools[0].id.clone())
        .or_default()
        .insert("undeclared-case".into());
    let error =
        hookkit_pkl_config::validate_manifest(&manifest, &specs, Some(&orphan_case), "2026-08-08")
            .expect_err("orphan fixture case must fail");
    assert!(
        error
            .to_string()
            .contains("fixture case undeclared-case is not declared")
    );
}

#[test]
fn validation_manifest_requires_explicit_resolution_and_unexpired_exceptions() {
    let (manifest, specs, fixtures) = load();
    let enabled = manifest
        .tools
        .iter()
        .position(|tool| tool.contracts.immediate.is_some())
        .unwrap();
    let target = manifest.tools[enabled]
        .contracts
        .immediate
        .as_ref()
        .unwrap()
        .targets[0]
        .id
        .clone();

    let mut implicit = manifest.clone();
    implicit.tools[enabled].evidence.retain(|record| {
        record.tier != EvidenceTier::RenderedCommand
            || !record.surfaces.contains(&EvidenceSurface::Immediate)
            || !record.targets.contains(&target)
    });
    let error =
        hookkit_pkl_config::validate_manifest(&implicit, &specs, Some(&fixtures), "2026-08-08")
            .expect_err("unresolved requirements must fail");
    assert!(
        error
            .to_string()
            .contains(&format!("unresolved RenderedCommand/immediate/{target}"))
    );

    let mut missing_orchestration = manifest.clone();
    missing_orchestration.tools[enabled]
        .evidence
        .iter_mut()
        .find(|record| {
            record.tier == EvidenceTier::RenderedCommand
                && record.surfaces == [EvidenceSurface::Immediate]
                && !record.surface_cases.is_empty()
        })
        .unwrap()
        .surface_cases
        .clear();
    let error = hookkit_pkl_config::validate_manifest(
        &missing_orchestration,
        &specs,
        Some(&fixtures),
        "2026-08-08",
    )
    .expect_err("unresolved surface orchestration must fail");
    assert!(
        error
            .to_string()
            .contains("unresolved RenderedCommand/immediate/surface/ImmediatePhaseOrder")
    );

    let mut incomplete = manifest.clone();
    let removed = incomplete.tools[enabled]
        .contracts
        .immediate
        .as_mut()
        .unwrap()
        .targets[0]
        .required_cases
        .pop()
        .unwrap();
    incomplete.tools[enabled]
        .contracts
        .immediate
        .as_mut()
        .unwrap()
        .required_cases
        .retain(|case| *case != removed);
    let error =
        hookkit_pkl_config::validate_manifest(&incomplete, &specs, Some(&fixtures), "2026-08-08")
            .expect_err("capability minimum cases cannot be omitted");
    assert!(error.to_string().contains("minimum contract requires"));

    let mut expired = manifest.clone();
    expired.tools[enabled].exceptions.push(ValidationException {
        id: "expired-test-waiver".into(),
        owner: "catalog-maintainers".into(),
        reason: "exercise expiry validation".into(),
        tracking_issue: 5,
        expires_on: "2026-08-08".into(),
        tiers: vec![EvidenceTier::RenderedCommand],
        surfaces: vec![EvidenceSurface::Immediate],
        targets: vec![target.clone()],
        cases: vec![ContractCase::Clean],
        surface_cases: Vec::new(),
    });
    let error =
        hookkit_pkl_config::validate_manifest(&expired, &specs, Some(&fixtures), "2026-08-08")
            .expect_err("exception expires at start of its expiry date");
    assert!(error.to_string().contains("expired on 2026-08-08"));

    let duplicate = ValidationException {
        id: "duplicate-waiver".into(),
        owner: "catalog-maintainers".into(),
        reason: "exercise global exception IDs".into(),
        tracking_issue: 5,
        expires_on: "2099-01-01".into(),
        tiers: vec![EvidenceTier::RenderedCommand],
        surfaces: vec![EvidenceSurface::Immediate],
        targets: vec![target],
        cases: vec![ContractCase::Clean],
        surface_cases: Vec::new(),
    };
    let mut duplicate_exceptions = manifest;
    duplicate_exceptions.tools[enabled]
        .exceptions
        .push(duplicate.clone());
    duplicate_exceptions.tools[enabled + 1]
        .exceptions
        .push(duplicate);
    let error = hookkit_pkl_config::validate_manifest(
        &duplicate_exceptions,
        &specs,
        Some(&fixtures),
        "2026-08-08",
    )
    .expect_err("exception IDs are globally unique");
    assert!(error.to_string().contains("exception ID duplicates"));
}

#[test]
fn contracts_are_surface_and_target_specific() {
    let (manifest, specs, fixtures) = load();
    hookkit_pkl_config::validate_manifest(&manifest, &specs, Some(&fixtures), "2026-08-08")
        .expect("valid manifest");

    for id in ["go-fmt", "yq"] {
        let tool = manifest.tools.iter().find(|tool| tool.id == id).unwrap();
        let immediate = tool.contracts.immediate.as_ref().unwrap();
        let deferred = tool.contracts.deferred.as_ref().unwrap();
        assert!(!immediate.capabilities.contains(&Capability::Checker));
        assert!(immediate.capabilities.contains(&Capability::Mutator));
        assert!(immediate.capabilities.contains(&Capability::Batch));
        assert!(
            !immediate
                .required_cases
                .contains(&ContractCase::PostMutationVerification)
        );
        assert_eq!(
            immediate.orchestration_cases,
            vec![ContractCase::ImmediatePhaseOrder]
        );
        assert!(deferred.capabilities.contains(&Capability::Checker));
        assert!(
            deferred
                .required_cases
                .contains(&ContractCase::PostMutationVerification)
        );
        assert_eq!(
            deferred.orchestration_cases,
            vec![ContractCase::DeferredLifecycle]
        );
    }
    let yq = manifest.tools.iter().find(|tool| tool.id == "yq").unwrap();
    assert!(
        yq.contracts
            .deferred
            .as_ref()
            .unwrap()
            .capabilities
            .contains(&Capability::PerFile)
    );
    assert!(
        !yq.contracts
            .immediate
            .as_ref()
            .unwrap()
            .capabilities
            .contains(&Capability::PerFile)
    );
    assert!(yq.dependencies.wrapper_executables.contains(&"rm".into()));
}

#[test]
fn validation_manifest_rejects_target_signature_drift_and_unproven_pinned_coverage() {
    let (manifest, specs, fixtures) = load();
    let enabled = manifest
        .tools
        .iter()
        .position(|tool| tool.contracts.immediate.is_some())
        .unwrap();

    let mut drift = manifest.clone();
    drift.tools[enabled]
        .contracts
        .immediate
        .as_mut()
        .unwrap()
        .targets[0]
        .commands[0]
        .argv
        .clear();
    let error =
        hookkit_pkl_config::validate_manifest(&drift, &specs, Some(&fixtures), "2026-08-08")
            .expect_err("signature drift must fail");
    assert!(
        error
            .to_string()
            .contains("contracts differ from the evaluated ordered command targets")
    );

    let mut false_pinned = manifest;
    let record = false_pinned.tools[enabled]
        .evidence
        .iter_mut()
        .find(|record| record.tier == EvidenceTier::PinnedRealTool)
        .unwrap();
    record.status = EvidenceStatus::Covered;
    record.references = vec!["cargo:test/not-actually-pinned".into()];
    record.reason = None;
    record.tracking_issue = None;
    let error =
        hookkit_pkl_config::validate_manifest(&false_pinned, &specs, Some(&fixtures), "2026-08-08")
            .expect_err("pinned coverage needs provenance");
    assert!(
        error
            .to_string()
            .contains("covered pinned-real-tool evidence requires recorded upstream provenance")
    );
}

#[test]
fn coverage_preserves_partial_surface_progress() {
    let (mut manifest, specs, fixtures) = load();
    let tool = manifest
        .tools
        .iter_mut()
        .find(|tool| tool.id == "yq")
        .unwrap();
    for record in &mut tool.evidence {
        if record.tier == EvidenceTier::RenderedCommand
            && record.surfaces == [EvidenceSurface::Immediate]
        {
            record.status = EvidenceStatus::Covered;
            record.references = vec!["cargo:test/example-immediate-target-contract".into()];
            record.reason = None;
            record.tracking_issue = None;
        }
    }
    let summary =
        hookkit_pkl_config::validate_manifest(&manifest, &specs, Some(&fixtures), "2026-08-08")
            .expect("surface-specific coverage remains valid");
    let yq = summary.tools.iter().find(|tool| tool.id == "yq").unwrap();
    assert_eq!(
        yq.surface_layers[&EvidenceSurface::Immediate][&EvidenceTier::RenderedCommand],
        LayerState::Covered
    );
    assert_eq!(
        yq.surface_layers[&EvidenceSurface::Deferred][&EvidenceTier::RenderedCommand],
        LayerState::Gap
    );
    assert_eq!(yq.layers[&EvidenceTier::RenderedCommand], LayerState::Gap);
    assert_eq!(
        yq.surface_case_layers[&EvidenceSurface::Immediate][&ContractCase::ImmediatePhaseOrder]
            [&EvidenceTier::RenderedCommand],
        LayerState::Covered
    );
    assert_eq!(
        yq.surface_case_layers[&EvidenceSurface::Deferred][&ContractCase::DeferredLifecycle]
            [&EvidenceTier::RenderedCommand],
        LayerState::Gap
    );
    let immediate_target = yq
        .target_layers
        .iter()
        .find(|target| target.surface == EvidenceSurface::Immediate)
        .unwrap();
    assert!(
        immediate_target
            .case_layers
            .values()
            .all(|layers| { layers[&EvidenceTier::RenderedCommand] == LayerState::Covered })
    );
}

#[test]
fn generated_validation_coverage_is_current() {
    let (manifest, specs, fixtures) = load();
    let summary = hookkit_pkl_config::validate_manifest(
        &manifest,
        &specs,
        Some(&fixtures),
        &hookkit_pkl_config::current_utc_date(),
    )
    .expect("valid catalog coverage manifest");
    let generated = hookkit_pkl_config::render_coverage_markdown(&summary);
    if std::env::var_os("VELVET_GLOVE_PRINT_VALIDATION_COVERAGE").is_some() {
        eprintln!("VALIDATION_COVERAGE_BEGIN\n{generated}VALIDATION_COVERAGE_END");
    }
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/builtin-validation-coverage.md");
    if std::env::var_os("VELVET_GLOVE_UPDATE_VALIDATION_COVERAGE").is_some() {
        std::fs::write(&path, &generated)
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        return;
    }
    let checked_in = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    assert_eq!(checked_in, generated, "regenerate {}", path.display());
}
