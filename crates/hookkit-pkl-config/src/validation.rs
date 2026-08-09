//! Machine-readable validation contracts for the embedded builtin catalog.

use crate::schema::{
    ArgToken, ArgvElement, CheckScope, ExitCodes, InvocationGranularity, Phase, PhaseMode,
    ToolSpec, WorkflowCommand, WriteBehavior,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Embedded validation-manifest source.
pub const VALIDATION_MANIFEST_JSON: &str = include_str!("../validation/manifest.json");

/// Versioned declaration of catalog support and evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidationManifest {
    /// Relative JSON Schema reference for editors and non-Rust consumers.
    #[serde(rename = "$schema")]
    pub schema: String,
    /// Manifest format version.
    pub schema_version: u32,
    /// Stable source identifiers referenced by tool provenance.
    pub catalog_sources: BTreeMap<String, String>,
    /// One declaration per builtin, retained as an array so duplicates remain visible.
    pub tools: Vec<ToolValidation>,
}

/// Validation declaration for one evaluated builtin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolValidation {
    /// Pkl property name in the generated `Builtins.pkl` aggregator.
    pub builtin: String,
    /// Canonical runtime tool ID.
    pub id: String,
    /// Repository-relative source path.
    pub spec_path: String,
    /// Per-tool GitHub tracking issue.
    pub tracking_issue: u32,
    /// Whether the catalog currently makes a support claim.
    pub support: SupportState,
    /// Immediate and deferred execution surfaces.
    pub surfaces: Surfaces,
    /// Surface-specific contracts and exact command targets.
    pub contracts: SurfaceContracts,
    /// Existing fixture scenario directory names; these are inventory, not evidence.
    pub fixture_cases: Vec<String>,
    /// Programs and provisioning domain needed by the contract.
    pub dependencies: Dependencies,
    /// Catalog and external-tool provenance.
    pub provenance: Provenance,
    /// Allowed execution environment for future pinned cases.
    pub constraints: Constraints,
    /// Evidence and explicit gaps, scoped by tier, surface, and case.
    pub evidence: Vec<EvidenceRecord>,
    /// Temporary reviewed waivers for otherwise unresolved contract cells.
    pub exceptions: Vec<ValidationException>,
}

/// Catalog support state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SupportState {
    /// Included in the supported catalog.
    Enabled,
    /// Retained as a draft without a support claim.
    Disabled,
}

/// Execution surfaces declared for one tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Surfaces {
    /// Immediate PostToolUse behavior.
    pub immediate: ImmediateSurface,
    /// Stop-time behavior.
    pub deferred: DeferredSurface,
}

/// Immediate and deferred command contracts for one tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfaceContracts {
    /// Immediate phase contract, or `None` when that surface is unsupported.
    pub immediate: Option<SurfaceContract>,
    /// Deferred workflow contract, or `None` when that surface is unsupported.
    pub deferred: Option<SurfaceContract>,
}

/// Derived contract for one execution surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfaceContract {
    /// Union of the surface's target capabilities.
    pub capabilities: Vec<Capability>,
    /// Union of the surface's target minimum cases.
    pub required_cases: Vec<ContractCase>,
    /// Cases that apply to orchestration across the whole surface.
    pub orchestration_cases: Vec<ContractCase>,
    /// Every phase or workflow command unit executed on this surface.
    pub targets: Vec<CommandTarget>,
}

/// One named phase or workflow whose evidence must resolve independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandTarget {
    /// Stable phase or workflow identifier from the evaluated catalog.
    pub id: String,
    /// Whether this target is the immediate pipeline or a deferred workflow.
    pub kind: CommandTargetKind,
    /// Zero-based execution position after catalog ordering is applied.
    pub order: usize,
    /// Invocation granularity used for this target.
    pub invocation: InvocationGranularity,
    /// Scope whose changes invalidate a deferred check result.
    pub check_scope: Option<CheckScope>,
    /// Directory policy used when invoking commands.
    pub working_directory: WorkingDirectory,
    /// Workspace marker used to partition candidates, when configured.
    pub workspace_indicator: Option<String>,
    /// Tool-local include globs that select command candidates.
    pub file_includes: Vec<String>,
    /// Tool-local exclude globs; runtime settings may prepend global excludes.
    pub file_excludes: Vec<String>,
    /// Complete ordered command model for the target.
    pub commands: Vec<CommandSignature>,
    /// Capabilities derived specifically for this target.
    pub capabilities: Vec<Capability>,
    /// Minimum semantic cases required specifically for this target.
    pub required_cases: Vec<ContractCase>,
}

/// Exact executable contract for one command within a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandSignature {
    /// Stable command identifier, including check/remedy role where applicable.
    pub id: String,
    /// Semantic role of the command.
    pub role: CommandRole,
    /// Runtime phase mode used for classification and orchestration.
    pub mode: PhaseMode,
    /// Immediate phase reused by a compatibility command, when applicable.
    pub reuses_immediate_phase: Option<String>,
    /// Resolved executable after applying any per-command override.
    pub program: String,
    /// Typed argument template rendered by the runner.
    pub argv: Vec<ArgvElement>,
    /// Exit-code classification contract.
    pub exit_codes: ExitCodes,
    /// Whether clean exit plus nonempty stdout reports issues.
    pub issues_on_stdout: bool,
    /// Files the command is allowed to modify.
    pub writes: WriteBehavior,
    /// Literal expansion used by the `ExtraArgs` token.
    pub extra_args: Vec<String>,
}

/// Semantic role of a command in a phase or workflow target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandRole {
    /// Command in the immediate ordered phase pipeline.
    Phase,
    /// Deferred workflow check.
    WorkflowCheck,
    /// Deferred workflow remedy.
    WorkflowRemedy,
}

/// Kind of independently resolved execution target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandTargetKind {
    /// The complete ordered immediate phase pipeline.
    ImmediatePipeline,
    /// A first-class deferred workflow.
    ExplicitWorkflow,
    /// A deferred workflow translated from immediate phases.
    CompatibilityWorkflow,
}

/// Working-directory policy used for a target's command invocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkingDirectory {
    /// Invoke at the discovered project root.
    ProjectRoot,
    /// Invoke at the nearest workspace-marker partition.
    Workspace,
}

/// Immediate surface state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImmediateSurface {
    /// Executable immediate phases exist.
    Supported,
    /// No immediate behavior is claimed.
    NotSupported,
}

/// Deferred surface state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeferredSurface {
    /// The spec declares first-class deferred workflows.
    Explicit,
    /// Deferred behavior is translated from immediate phases.
    Compatibility,
    /// No deferred behavior is claimed.
    NotSupported,
}

/// Composable command-model capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// Has an authoritative read-only check.
    Checker,
    /// Can modify repository files.
    Mutator,
    /// A clean exit can still report issues through stdout.
    StdoutSignaled,
    /// Runs in a workspace partition or can be invalidated by workspace changes.
    WorkspaceScoped,
    /// Invokes once per selected file.
    PerFile,
    /// Invokes once for a selected batch.
    Batch,
    /// Invokes once for each workspace partition.
    WorkspaceInvocation,
}

/// Named semantic case in a minimum tool contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContractCase {
    /// Every declared command definition is rendered and exercised on its applicable path.
    CommandCoverage,
    /// Immediate phases execute in order with the runner's failure short-circuit semantics.
    ImmediatePhaseOrder,
    /// Deferred checks, conditional remedies, invalidation, and final checks follow the lifecycle.
    DeferredLifecycle,
    /// A representative clean input is classified clean.
    Clean,
    /// A representative source problem is classified as an issue.
    SourceIssue,
    /// A tool/configuration failure is not classified as a source issue.
    OperationalFailure,
    /// A read-only check leaves the workspace unchanged.
    NoMutation,
    /// A remedy performs its expected mutation.
    Mutation,
    /// The complete workspace diff after a remedy is asserted.
    CompleteWorkspaceDiff,
    /// Mutation is followed by an authoritative read-only verification.
    PostMutationVerification,
    /// Repeating a successful mutating target makes no further change.
    Idempotence,
    /// Empty stdout with a clean exit is classified clean.
    StdoutClean,
    /// Nonempty stdout with a clean exit is classified as issues.
    StdoutIssue,
    /// Workspace/root selection is exercised.
    WorkspaceScope,
    /// Multiple candidates are exercised in one contract.
    MultiFile,
    /// A batch result's file attribution is verified.
    BatchAttribution,
    /// Per-file invocation count and arguments are verified.
    PerFileInvocation,
    /// A workspace result's file attribution is verified.
    WorkspaceAttribution,
}

/// External programs and provisioning grouping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dependencies {
    /// Provisioning/runtime domain used for sharding and setup.
    pub provisioning_group: String,
    /// Default executable from the Pkl spec.
    pub primary_executable: String,
    /// Exact non-primary programs selected by phase/workflow overrides.
    pub program_overrides: Vec<String>,
    /// Programs invoked inside opaque wrapper scripts and declared manually.
    pub wrapper_executables: Vec<String>,
}

/// Source lineage for one declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provenance {
    /// Key in the manifest's `catalogSources` map.
    pub catalog_source: String,
    /// Reviewed external source/version information or its explicit gap.
    pub upstream: UpstreamProvenance,
}

/// Reviewed upstream provenance or an explicit missing-provenance gap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum UpstreamProvenance {
    /// Complete reviewed provenance for a pinned external tool.
    Recorded {
        /// Canonical upstream project or release URL.
        url: String,
        /// Exact tested tool version.
        version: String,
        /// Package, image, archive, or other installation source.
        installation_source: String,
        /// Program and arguments used to observe the installed version.
        version_command: Vec<String>,
    },
    /// Missing provenance that is tracked as work rather than evidence.
    Gap {
        /// Why reviewed provenance is not yet available.
        reason: String,
        /// Issue responsible for closing the gap.
        tracking_issue: u32,
    },
}

/// Execution constraints for pinned contract cases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Constraints {
    /// Operating systems on which pinned cases may run.
    pub platforms: Vec<Platform>,
    /// Processor architectures on which pinned cases may run.
    pub architectures: Vec<Architecture>,
    /// Network access allowed while executing a contract case.
    pub case_network: NetworkPolicy,
}

/// Supported pinned-case operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    /// Linux runner or container.
    Linux,
    /// macOS runner.
    Macos,
}

/// Supported pinned-case processor architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Architecture {
    /// 64-bit x86.
    X86_64,
    /// 64-bit Arm.
    Aarch64,
}

/// Network access policy while a real-tool case executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkPolicy {
    /// No network access.
    Denied,
    /// Loopback access only.
    Loopback,
    /// External network access is an explicit case requirement.
    Required,
}

/// One covered or explicitly missing evidence scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceRecord {
    /// Evidence tier resolved by this record.
    pub tier: EvidenceTier,
    /// Whether this scope is covered or explicitly missing.
    pub status: EvidenceStatus,
    /// Catalog or execution surfaces resolved by the record.
    pub surfaces: Vec<EvidenceSurface>,
    /// Named execution targets resolved by the record.
    pub targets: Vec<String>,
    /// Semantic cases resolved by execution-tier records.
    pub cases: Vec<ContractCase>,
    /// Surface-wide orchestration cases resolved by the record.
    pub surface_cases: Vec<ContractCase>,
    /// Stable tests, artifacts, or reports supporting covered evidence.
    #[serde(default)]
    pub references: Vec<String>,
    /// Required explanation for a gap.
    pub reason: Option<String>,
    /// Required tracking issue for a gap.
    pub tracking_issue: Option<u32>,
}

/// Validation evidence tier; tiers are deliberately not interchangeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceTier {
    /// Evaluated schema and catalog/manifest invariants.
    Schema,
    /// Probe-backed rendered invocation contract.
    RenderedCommand,
    /// Controlled, version-pinned external-tool behavior.
    PinnedRealTool,
}

/// Disposition of an evidence scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceStatus {
    /// Stable references support the claim.
    Covered,
    /// The missing claim is explicitly tracked.
    Gap,
}

/// Surface to which an evidence record applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceSurface {
    /// Static catalog/schema validation.
    Catalog,
    /// Immediate PostToolUse execution.
    Immediate,
    /// Deferred turn-completion execution.
    Deferred,
}

/// A reviewed, expiring alternative to evidence or an explicit gap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidationException {
    /// Stable ID unique across the entire manifest.
    pub id: String,
    /// Person or team responsible for removing the waiver.
    pub owner: String,
    /// Narrow reason the requirement is temporarily waived.
    pub reason: String,
    /// Issue tracking removal of the waiver.
    pub tracking_issue: u32,
    /// First UTC date on which the waiver is invalid.
    pub expires_on: String,
    /// Evidence tiers covered by the waiver.
    pub tiers: Vec<EvidenceTier>,
    /// Surfaces covered by the waiver.
    pub surfaces: Vec<EvidenceSurface>,
    /// Named execution targets covered by the waiver.
    pub targets: Vec<String>,
    /// Contract cases covered by execution-tier waivers.
    pub cases: Vec<ContractCase>,
    /// Surface-wide orchestration cases covered by the waiver.
    pub surface_cases: Vec<ContractCase>,
}

/// All manifest violations found in one deterministic pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestValidationError {
    /// Deterministically sorted validation diagnostics.
    pub errors: Vec<String>,
}

impl fmt::Display for ManifestValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.errors.iter().enumerate() {
            if index > 0 {
                formatter.write_str("\n")?;
            }
            write!(formatter, "- {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ManifestValidationError {}

/// Per-tier counts in a validated coverage report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerTotals {
    /// Tools with complete evidence at the tier.
    pub covered: usize,
    /// Tools with one or more explicit evidence gaps.
    pub gap: usize,
    /// Tools relying on one or more active exceptions.
    pub exception: usize,
    /// Disabled tools for which execution evidence is not required.
    pub not_required: usize,
}

/// Rollup state for one tool and evidence tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayerState {
    /// All required scopes are backed by evidence.
    Covered,
    /// One or more required scopes are explicit gaps.
    Gap,
    /// One or more required scopes rely on active exceptions.
    Exception,
    /// The tier is outside the tool's support claim.
    NotRequired,
}

/// Validated coverage row for one builtin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCoverage {
    /// Pkl catalog property name.
    pub builtin: String,
    /// Canonical tool ID.
    pub id: String,
    /// Supported or disabled catalog state.
    pub support: SupportState,
    /// Declared execution surfaces.
    pub surfaces: Surfaces,
    /// Surface-specific command contracts.
    pub contracts: SurfaceContracts,
    /// Declared legacy fixture scenario IDs.
    pub fixture_cases: Vec<String>,
    /// Rollup state for every evidence tier.
    pub layers: BTreeMap<EvidenceTier, LayerState>,
    /// Per-surface state for execution evidence tiers.
    pub surface_layers: BTreeMap<EvidenceSurface, BTreeMap<EvidenceTier, LayerState>>,
    /// Per-surface orchestration case states.
    pub surface_case_layers:
        BTreeMap<EvidenceSurface, BTreeMap<ContractCase, BTreeMap<EvidenceTier, LayerState>>>,
    /// Per-target state so machine coverage never collapses distinct commands.
    pub target_layers: Vec<TargetCoverage>,
    /// Evidence and provenance gaps declared for the tool.
    pub gap_records: usize,
    /// Active validation exceptions declared for the tool.
    pub active_exceptions: usize,
}

/// Evidence rollup for one independently resolved execution target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetCoverage {
    /// Immediate or deferred surface containing the target.
    pub surface: EvidenceSurface,
    /// Stable phase-pipeline or workflow identifier.
    pub target: String,
    /// Target-specific capabilities.
    pub capabilities: Vec<Capability>,
    /// Target-specific required cases.
    pub required_cases: Vec<ContractCase>,
    /// Rendered-command and pinned-real-tool states.
    pub layers: BTreeMap<EvidenceTier, LayerState>,
    /// Per-case states retained beneath the target rollup.
    pub case_layers: BTreeMap<ContractCase, BTreeMap<EvidenceTier, LayerState>>,
}

/// Machine-readable coverage generated from a validated manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageSummary {
    /// Enabled and disabled declarations combined.
    pub total_tools: usize,
    /// Tools in the supported-catalog denominator.
    pub enabled_tools: usize,
    /// Disabled draft declarations.
    pub disabled_tools: usize,
    /// Tool IDs with one or more legacy fixture scenarios.
    pub fixture_tools: usize,
    /// Total declared fixture scenario directories.
    pub fixture_cases: usize,
    /// Aggregate coverage state by tier.
    pub layers: BTreeMap<EvidenceTier, LayerTotals>,
    /// Deterministically ordered per-tool coverage rows.
    pub tools: Vec<ToolCoverage>,
}

/// Parse the embedded manifest with strict unknown-field rejection.
pub fn builtin_validation_manifest() -> Result<ValidationManifest, serde_json::Error> {
    serde_json::from_str(VALIDATION_MANIFEST_JSON)
}

/// Return the UTC calendar date used for expiry checks.
pub fn current_utc_date() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Derive the exact immediate and deferred contracts from an evaluated spec.
pub fn derived_surface_contracts(spec: &ToolSpec) -> SurfaceContracts {
    if !spec.enabled {
        return SurfaceContracts {
            immediate: None,
            deferred: None,
        };
    }

    let phases = ordered_enabled_phases(spec);
    let immediate = if phases.is_empty() {
        None
    } else {
        let mut capabilities = BTreeSet::from([invocation_capability(spec.phase_invocation)]);
        if phases.iter().any(|(_, phase)| phase_is_checker(phase)) {
            capabilities.insert(Capability::Checker);
        }
        if phases.iter().any(|(_, phase)| !phase_is_checker(phase)) {
            capabilities.insert(Capability::Mutator);
        }
        if spec.workspace_indicator.is_some() {
            capabilities.insert(Capability::WorkspaceScoped);
        }
        let target = command_target(
            "pipeline",
            CommandTargetKind::ImmediatePipeline,
            0,
            spec.phase_invocation,
            None,
            spec,
            phases
                .iter()
                .map(|(id, phase)| phase_signature(id, phase, spec, CommandRole::Phase, None))
                .collect(),
            capabilities,
        );
        Some(surface_contract(
            vec![target],
            ContractCase::ImmediatePhaseOrder,
        ))
    };

    let deferred_targets = if spec.workflows.is_empty() {
        compatibility_targets(spec, &phases)
    } else {
        ordered_enabled_workflows(spec)
            .into_iter()
            .enumerate()
            .map(|(order, (id, workflow))| {
                let mut commands = Vec::new();
                let mut capabilities = BTreeSet::new();
                if let Some(check) = &workflow.check {
                    capabilities.insert(Capability::Checker);
                    commands.push(workflow_signature(
                        &format!("{id}.check"),
                        check,
                        spec,
                        CommandRole::WorkflowCheck,
                    ));
                }
                if let Some(remedy) = &workflow.remedy {
                    capabilities.insert(Capability::Mutator);
                    commands.push(workflow_signature(
                        &format!("{id}.remedy"),
                        remedy,
                        spec,
                        CommandRole::WorkflowRemedy,
                    ));
                }
                if workflow.check_scope == CheckScope::Workspace
                    || spec.workspace_indicator.is_some()
                {
                    capabilities.insert(Capability::WorkspaceScoped);
                }
                capabilities.insert(invocation_capability(workflow.invocation));
                command_target(
                    id,
                    CommandTargetKind::ExplicitWorkflow,
                    order,
                    workflow.invocation,
                    Some(workflow.check_scope),
                    spec,
                    commands,
                    capabilities,
                )
            })
            .collect()
    };
    let deferred = if deferred_targets.is_empty() {
        None
    } else {
        Some(surface_contract(
            deferred_targets,
            ContractCase::DeferredLifecycle,
        ))
    };

    SurfaceContracts {
        immediate,
        deferred,
    }
}

/// Derive the union of immediate and deferred capabilities.
pub fn derived_capabilities(spec: &ToolSpec) -> BTreeSet<Capability> {
    let contracts = derived_surface_contracts(spec);
    contracts
        .immediate
        .iter()
        .chain(contracts.deferred.iter())
        .flat_map(|contract| contract.capabilities.iter().copied())
        .collect()
}

/// Expand composable capabilities into the minimum semantic contract.
pub fn minimum_required_cases(capabilities: &BTreeSet<Capability>) -> BTreeSet<ContractCase> {
    if capabilities.is_empty() {
        return BTreeSet::new();
    }
    let mut cases = BTreeSet::from([
        ContractCase::CommandCoverage,
        ContractCase::Clean,
        ContractCase::OperationalFailure,
    ]);
    if capabilities.contains(&Capability::Checker) {
        cases.extend([ContractCase::SourceIssue, ContractCase::NoMutation]);
    }
    if capabilities.contains(&Capability::Mutator) {
        cases.extend([
            ContractCase::Mutation,
            ContractCase::CompleteWorkspaceDiff,
            ContractCase::Idempotence,
        ]);
    }
    if capabilities.contains(&Capability::Checker) && capabilities.contains(&Capability::Mutator) {
        cases.insert(ContractCase::PostMutationVerification);
    }
    if capabilities.contains(&Capability::StdoutSignaled) {
        cases.extend([ContractCase::StdoutClean, ContractCase::StdoutIssue]);
    }
    if capabilities.contains(&Capability::WorkspaceScoped) {
        cases.extend([
            ContractCase::WorkspaceScope,
            ContractCase::WorkspaceAttribution,
        ]);
    }
    if capabilities.contains(&Capability::Batch) {
        cases.extend([ContractCase::MultiFile, ContractCase::BatchAttribution]);
    }
    if capabilities.contains(&Capability::PerFile) {
        cases.extend([ContractCase::MultiFile, ContractCase::PerFileInvocation]);
    }
    if capabilities.contains(&Capability::WorkspaceInvocation) {
        cases.extend([
            ContractCase::MultiFile,
            ContractCase::WorkspaceScope,
            ContractCase::WorkspaceAttribution,
        ]);
    }
    cases
}

fn ordered_enabled_phases(spec: &ToolSpec) -> Vec<(&String, &Phase)> {
    let mut seen = BTreeSet::new();
    let mut phases = Vec::new();
    for id in &spec.phase_order {
        if let Some(phase) = spec.phases.get(id) {
            if phase.enabled && seen.insert(id.as_str()) {
                phases.push((id, phase));
            }
        }
    }
    let mut remaining = spec
        .phases
        .iter()
        .filter(|(id, phase)| phase.enabled && !seen.contains(id.as_str()))
        .collect::<Vec<_>>();
    remaining.sort_by(|left, right| {
        phase_rank(left.1.mode)
            .cmp(&phase_rank(right.1.mode))
            .then_with(|| left.0.cmp(right.0))
    });
    phases.extend(remaining);
    phases
}

fn ordered_enabled_workflows(spec: &ToolSpec) -> Vec<(&String, &crate::schema::Workflow)> {
    let mut seen = BTreeSet::new();
    let mut workflows = Vec::new();
    for id in &spec.workflow_order {
        if let Some(workflow) = spec.workflows.get(id) {
            if workflow.enabled && seen.insert(id.as_str()) {
                workflows.push((id, workflow));
            }
        }
    }
    workflows.extend(
        spec.workflows
            .iter()
            .filter(|(id, workflow)| workflow.enabled && !seen.contains(id.as_str())),
    );
    workflows
}

fn compatibility_targets(spec: &ToolSpec, phases: &[(&String, &Phase)]) -> Vec<CommandTarget> {
    let verifier = phases
        .iter()
        .rev()
        .find(|(_, phase)| phase_is_checker(phase))
        .copied();
    let mutators = phases
        .iter()
        .filter(|(_, phase)| !phase_is_checker(phase))
        .copied()
        .collect::<Vec<_>>();

    if !mutators.is_empty() {
        return mutators
            .into_iter()
            .enumerate()
            .map(|(order, (id, remedy))| {
                let check_scope =
                    if spec.workspace_indicator.is_some() && !has_file_arguments(&remedy.argv) {
                        CheckScope::Workspace
                    } else {
                        CheckScope::TargetFiles
                    };
                let mut capabilities = BTreeSet::from([
                    Capability::Mutator,
                    invocation_capability(spec.phase_invocation),
                ]);
                let mut commands = Vec::new();
                if let Some((check_id, check)) = verifier {
                    capabilities.insert(Capability::Checker);
                    commands.push(phase_signature(
                        check_id,
                        check,
                        spec,
                        CommandRole::WorkflowCheck,
                        Some(check_id),
                    ));
                }
                if check_scope == CheckScope::Workspace || spec.workspace_indicator.is_some() {
                    capabilities.insert(Capability::WorkspaceScoped);
                }
                commands.push(phase_signature(
                    id,
                    remedy,
                    spec,
                    CommandRole::WorkflowRemedy,
                    Some(id),
                ));
                command_target(
                    id,
                    CommandTargetKind::CompatibilityWorkflow,
                    order,
                    spec.phase_invocation,
                    Some(check_scope),
                    spec,
                    commands,
                    capabilities,
                )
            })
            .collect();
    }

    phases
        .iter()
        .filter(|(_, phase)| phase_is_checker(phase))
        .enumerate()
        .map(|(order, (id, check))| {
            let check_scope = if spec.workspace_indicator.is_some() {
                CheckScope::Workspace
            } else {
                CheckScope::TargetFiles
            };
            let mut capabilities = BTreeSet::from([
                Capability::Checker,
                invocation_capability(spec.phase_invocation),
            ]);
            if check_scope == CheckScope::Workspace {
                capabilities.insert(Capability::WorkspaceScoped);
            }
            command_target(
                id,
                CommandTargetKind::CompatibilityWorkflow,
                order,
                spec.phase_invocation,
                Some(check_scope),
                spec,
                vec![phase_signature(
                    id,
                    check,
                    spec,
                    CommandRole::WorkflowCheck,
                    Some(id),
                )],
                capabilities,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn command_target(
    id: &str,
    kind: CommandTargetKind,
    order: usize,
    invocation: InvocationGranularity,
    check_scope: Option<CheckScope>,
    spec: &ToolSpec,
    commands: Vec<CommandSignature>,
    mut capabilities: BTreeSet<Capability>,
) -> CommandTarget {
    if commands.iter().any(|command| command.issues_on_stdout) {
        capabilities.insert(Capability::StdoutSignaled);
    }
    CommandTarget {
        id: id.to_owned(),
        kind,
        order,
        invocation,
        check_scope,
        working_directory: if spec.workspace_indicator.is_some() {
            WorkingDirectory::Workspace
        } else {
            WorkingDirectory::ProjectRoot
        },
        workspace_indicator: spec.workspace_indicator.clone(),
        file_includes: sorted_strings(&spec.files.include),
        file_excludes: sorted_strings(&spec.files.exclude),
        commands,
        required_cases: minimum_required_cases(&capabilities).into_iter().collect(),
        capabilities: capabilities.into_iter().collect(),
    }
}

fn surface_contract(
    targets: Vec<CommandTarget>,
    orchestration_case: ContractCase,
) -> SurfaceContract {
    let capabilities = targets
        .iter()
        .flat_map(|target| target.capabilities.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut required_cases = targets
        .iter()
        .flat_map(|target| target.required_cases.iter().copied())
        .collect::<BTreeSet<_>>();
    required_cases.insert(orchestration_case);
    SurfaceContract {
        capabilities: capabilities.into_iter().collect(),
        required_cases: required_cases.into_iter().collect(),
        orchestration_cases: vec![orchestration_case],
        targets,
    }
}

fn phase_signature(
    id: &str,
    phase: &Phase,
    spec: &ToolSpec,
    role: CommandRole,
    reuses_immediate_phase: Option<&str>,
) -> CommandSignature {
    CommandSignature {
        id: id.to_owned(),
        role,
        mode: phase.mode,
        reuses_immediate_phase: reuses_immediate_phase.map(str::to_owned),
        program: phase
            .program
            .clone()
            .unwrap_or_else(|| spec.executable.clone()),
        argv: phase.argv.clone(),
        exit_codes: canonical_exit_codes(&phase.exit_codes),
        issues_on_stdout: false,
        writes: phase.writes,
        extra_args: phase.extra_args.clone(),
    }
}

fn workflow_signature(
    id: &str,
    command: &WorkflowCommand,
    spec: &ToolSpec,
    role: CommandRole,
) -> CommandSignature {
    CommandSignature {
        id: id.to_owned(),
        role,
        mode: match role {
            CommandRole::WorkflowCheck => PhaseMode::Verify,
            CommandRole::WorkflowRemedy => PhaseMode::Fix,
            CommandRole::Phase => unreachable!("workflow command cannot use phase role"),
        },
        reuses_immediate_phase: None,
        program: command
            .program
            .clone()
            .unwrap_or_else(|| spec.executable.clone()),
        argv: command.argv.clone(),
        exit_codes: canonical_exit_codes(&command.exit_codes),
        issues_on_stdout: command.issues_on_stdout,
        writes: command.writes,
        extra_args: command.extra_args.clone(),
    }
}

fn canonical_exit_codes(exit_codes: &ExitCodes) -> ExitCodes {
    ExitCodes {
        clean: sorted_i32s(&exit_codes.clean),
        issues: sorted_i32s(&exit_codes.issues),
        failure: sorted_i32s(&exit_codes.failure),
        unexpected: exit_codes.unexpected,
    }
}

fn sorted_i32s(values: &[i32]) -> Vec<i32> {
    values
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn sorted_strings(values: &[String]) -> Vec<String> {
    values
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn phase_is_checker(phase: &Phase) -> bool {
    matches!(phase.mode, PhaseMode::Verify | PhaseMode::CheckOnly)
}

fn phase_rank(mode: PhaseMode) -> u8 {
    match mode {
        PhaseMode::Format => 0,
        PhaseMode::Fix => 1,
        PhaseMode::Verify => 2,
        PhaseMode::CheckOnly => 3,
    }
}

fn invocation_capability(invocation: InvocationGranularity) -> Capability {
    match invocation {
        InvocationGranularity::PerFile => Capability::PerFile,
        InvocationGranularity::Batch => Capability::Batch,
        InvocationGranularity::Workspace => Capability::WorkspaceInvocation,
    }
}

fn has_file_arguments(argv: &[ArgvElement]) -> bool {
    argv.iter().any(|argument| {
        matches!(
            argument,
            ArgvElement::Token(ArgToken::Files | ArgToken::WorkspaceFiles)
        )
    })
}

fn required_auxiliary_programs(spec: &ToolSpec) -> BTreeSet<String> {
    let mut programs = BTreeSet::new();
    for phase in spec.phases.values().filter(|phase| phase.enabled) {
        if let Some(program) = phase.program.as_deref() {
            if program != spec.executable {
                programs.insert(program.to_owned());
            }
        }
    }
    for workflow in spec.workflows.values().filter(|workflow| workflow.enabled) {
        for command in [workflow.check.as_ref(), workflow.remedy.as_ref()]
            .into_iter()
            .flatten()
        {
            if let Some(program) = command.program.as_deref() {
                if program != spec.executable {
                    programs.insert(program.to_owned());
                }
            }
        }
    }
    programs
}

fn derived_surfaces(spec: &ToolSpec) -> Surfaces {
    let contracts = derived_surface_contracts(spec);
    Surfaces {
        immediate: if contracts.immediate.is_some() {
            ImmediateSurface::Supported
        } else {
            ImmediateSurface::NotSupported
        },
        deferred: match (contracts.deferred.is_some(), spec.workflows.is_empty()) {
            (true, false) => DeferredSurface::Explicit,
            (true, true) => DeferredSurface::Compatibility,
            (false, _) => DeferredSurface::NotSupported,
        },
    }
}

/// Validate the manifest against evaluated specs and, when supplied, fixtures.
///
/// Missing evidence is valid only when represented by an explicit `gap`
/// record or a nonexpired scoped exception. The returned summary is therefore
/// safe to render or serialize without inferring coverage from fixture counts.
pub fn validate_manifest(
    manifest: &ValidationManifest,
    specs: &BTreeMap<String, ToolSpec>,
    fixture_inventory: Option<&BTreeMap<String, BTreeSet<String>>>,
    today: &str,
) -> Result<CoverageSummary, ManifestValidationError> {
    let mut errors = Vec::new();
    let today = match IsoDate::parse(today) {
        Some(date) => Some(date),
        None => {
            errors.push(format!(
                "validation date is not a valid YYYY-MM-DD value: {today}"
            ));
            None
        }
    };

    if manifest.schema.trim().is_empty() {
        errors.push("manifest $schema is empty".into());
    }
    if manifest.schema_version != 1 {
        errors.push(format!(
            "unsupported manifest schemaVersion {}; expected 1",
            manifest.schema_version
        ));
    }
    if manifest.catalog_sources.is_empty() {
        errors.push("manifest has no catalogSources".into());
    }
    for (id, source) in &manifest.catalog_sources {
        if id.trim().is_empty() {
            errors.push("catalogSources contains an empty source ID".into());
        }
        if source.trim().is_empty() {
            errors.push(format!("catalog source {id} is empty"));
        }
    }

    validate_catalog_identities(specs, &mut errors);

    let mut by_builtin = BTreeMap::<&str, &ToolValidation>::new();
    let mut manifest_ids = BTreeMap::<&str, &str>::new();
    for tool in &manifest.tools {
        if tool.builtin.trim().is_empty() {
            errors.push("manifest declaration has an empty builtin key".into());
        } else if by_builtin.insert(tool.builtin.as_str(), tool).is_some() {
            errors.push(format!(
                "manifest repeats builtin declaration {}",
                tool.builtin
            ));
        }
        if tool.id.trim().is_empty() {
            errors.push(format!("{}: manifest tool id is empty", tool.builtin));
        } else if let Some(previous) = manifest_ids.insert(tool.id.as_str(), tool.builtin.as_str())
        {
            errors.push(format!(
                "{}: manifest tool id {} duplicates {}",
                tool.builtin, tool.id, previous
            ));
        }
    }

    for key in specs.keys() {
        if !by_builtin.contains_key(key.as_str()) {
            errors.push(format!("{key}: builtin has no validation declaration"));
        }
    }
    for tool in &manifest.tools {
        if !specs.contains_key(&tool.builtin) {
            errors.push(format!(
                "{} ({}): validation declaration has no builtin spec",
                tool.builtin, tool.id
            ));
        }
    }

    let mut exception_ids = BTreeMap::<String, String>::new();
    for tool in &manifest.tools {
        let Some(spec) = specs.get(&tool.builtin) else {
            continue;
        };
        validate_tool(manifest, tool, spec, today, &mut exception_ids, &mut errors);
    }

    if let Some(inventory) = fixture_inventory {
        validate_fixtures(&manifest.tools, inventory, &mut errors);
    }

    errors.sort();
    errors.dedup();
    if errors.is_empty() {
        Ok(build_coverage_summary(manifest, fixture_inventory))
    } else {
        Err(ManifestValidationError { errors })
    }
}

fn validate_catalog_identities(specs: &BTreeMap<String, ToolSpec>, errors: &mut Vec<String>) {
    let mut ids = BTreeMap::<&str, &str>::new();
    for (key, spec) in specs {
        if spec.id.trim().is_empty() {
            errors.push(format!("{key}: catalog tool id is empty"));
        } else if let Some(previous) = ids.insert(spec.id.as_str(), key.as_str()) {
            errors.push(format!(
                "{key}: catalog tool id {} duplicates {previous}",
                spec.id
            ));
        }
    }
}

fn validate_tool(
    manifest: &ValidationManifest,
    tool: &ToolValidation,
    spec: &ToolSpec,
    today: Option<IsoDate>,
    exception_ids: &mut BTreeMap<String, String>,
    errors: &mut Vec<String>,
) {
    let label = format!("{} ({})", tool.builtin, tool.id);
    if tool.id != spec.id {
        errors.push(format!(
            "{label}: manifest id does not match catalog id {}",
            spec.id
        ));
    }
    if tool.spec_path.trim().is_empty() || !tool.spec_path.ends_with(".pkl") {
        errors.push(format!("{label}: specPath must name a .pkl file"));
    }
    if tool.tracking_issue == 0 {
        errors.push(format!("{label}: trackingIssue must be nonzero"));
    }
    let expected_support = if spec.enabled {
        SupportState::Enabled
    } else {
        SupportState::Disabled
    };
    if tool.support != expected_support {
        errors.push(format!(
            "{label}: support is {:?}; catalog requires {:?}",
            tool.support, expected_support
        ));
    }
    let expected_surfaces = derived_surfaces(spec);
    if tool.surfaces != expected_surfaces {
        errors.push(format!(
            "{label}: surfaces are {:?}; catalog requires {:?}",
            tool.surfaces, expected_surfaces
        ));
    }

    let target_requirements = validate_contracts(tool, spec, &label, errors);

    validate_dependencies(tool, spec, &label, errors);
    validate_provenance(manifest, tool, &label, errors);
    validate_constraints(tool, &label, errors);
    unique_string_set(&tool.fixture_cases, &label, "fixtureCases", errors);

    let supported_surfaces = supported_execution_surfaces(&tool.surfaces);
    let mut resolutions = BTreeMap::<ResolutionKey, Vec<Resolution>>::new();
    for (index, evidence) in tool.evidence.iter().enumerate() {
        validate_evidence(
            evidence,
            index,
            &label,
            &target_requirements,
            &supported_surfaces,
            &mut resolutions,
            errors,
        );
    }
    if matches!(tool.provenance.upstream, UpstreamProvenance::Gap { .. })
        && tool.evidence.iter().any(|evidence| {
            evidence.tier == EvidenceTier::PinnedRealTool
                && evidence.status == EvidenceStatus::Covered
        })
    {
        errors.push(format!(
            "{label}: covered pinned-real-tool evidence requires recorded upstream provenance"
        ));
    }
    for exception in &tool.exceptions {
        validate_exception(
            exception,
            &label,
            today,
            &target_requirements,
            &supported_surfaces,
            exception_ids,
            &mut resolutions,
            errors,
        );
    }
    validate_resolution_matrix(tool, &label, &target_requirements, &resolutions, errors);
}

fn validate_contracts(
    tool: &ToolValidation,
    spec: &ToolSpec,
    label: &str,
    errors: &mut Vec<String>,
) -> ContractRequirements {
    let expected = derived_surface_contracts(spec);
    for (surface, contract) in [
        (
            EvidenceSurface::Immediate,
            tool.contracts.immediate.as_ref(),
        ),
        (EvidenceSurface::Deferred, tool.contracts.deferred.as_ref()),
    ] {
        let Some(contract) = contract else { continue };
        let surface_label = format!("{label}: {} contract", surface_name(surface));
        let capabilities = unique_copy_set(
            &contract.capabilities,
            &surface_label,
            "capabilities",
            errors,
        );
        let required_cases = unique_copy_set(
            &contract.required_cases,
            &surface_label,
            "requiredCases",
            errors,
        );
        let orchestration_cases = unique_copy_set(
            &contract.orchestration_cases,
            &surface_label,
            "orchestrationCases",
            errors,
        );
        if orchestration_cases.is_empty() {
            errors.push(format!("{surface_label}: orchestrationCases is empty"));
        }
        let mut target_ids = BTreeSet::new();
        let mut target_capabilities = BTreeSet::new();
        let mut target_cases = BTreeSet::new();
        for target in &contract.targets {
            let target_label = format!("{surface_label} target {}", target.id);
            if target.id.trim().is_empty() {
                errors.push(format!("{surface_label}: target ID is empty"));
            } else if !target_ids.insert(target.id.as_str()) {
                errors.push(format!(
                    "{surface_label}: target ID {} is duplicated",
                    target.id
                ));
            }
            if target.commands.is_empty() {
                errors.push(format!("{target_label}: commands is empty"));
            }
            let capabilities =
                unique_copy_set(&target.capabilities, &target_label, "capabilities", errors);
            let cases = unique_copy_set(
                &target.required_cases,
                &target_label,
                "requiredCases",
                errors,
            );
            let minimum = minimum_required_cases(&capabilities);
            if cases != minimum {
                errors.push(format!(
                    "{target_label}: requiredCases are {}; minimum contract requires {}",
                    debug_set(&cases),
                    debug_set(&minimum)
                ));
            }
            target_capabilities.extend(capabilities);
            target_cases.extend(cases);

            let mut command_ids = BTreeSet::new();
            for command in &target.commands {
                if command.id.trim().is_empty() {
                    errors.push(format!("{target_label}: command ID is empty"));
                } else if !command_ids.insert(command.id.as_str()) {
                    errors.push(format!(
                        "{target_label}: command ID {} is duplicated",
                        command.id
                    ));
                }
                if command.program.trim().is_empty() {
                    errors.push(format!(
                        "{target_label}: command {} has an empty program",
                        command.id
                    ));
                }
            }
            unique_string_set(&target.file_includes, &target_label, "fileIncludes", errors);
            unique_string_set(&target.file_excludes, &target_label, "fileExcludes", errors);
        }
        if capabilities != target_capabilities {
            errors.push(format!(
                "{surface_label}: capabilities do not equal the target union"
            ));
        }
        target_cases.extend(orchestration_cases);
        if required_cases != target_cases {
            errors.push(format!(
                "{surface_label}: requiredCases do not equal the target and orchestration union"
            ));
        }
    }
    if tool.contracts != expected {
        errors.push(format!(
            "{label}: contracts differ from the evaluated ordered command targets"
        ));
    }

    let mut requirements = ContractRequirements::default();
    for (surface, contract) in [
        (EvidenceSurface::Immediate, expected.immediate.as_ref()),
        (EvidenceSurface::Deferred, expected.deferred.as_ref()),
    ] {
        let Some(contract) = contract else { continue };
        for target in &contract.targets {
            requirements.targets.insert(
                ExecutionTargetKey {
                    surface,
                    target: target.id.clone(),
                },
                target.required_cases.iter().copied().collect(),
            );
        }
        requirements.surfaces.insert(
            surface,
            contract.orchestration_cases.iter().copied().collect(),
        );
    }
    requirements
}

fn validate_dependencies(
    tool: &ToolValidation,
    spec: &ToolSpec,
    label: &str,
    errors: &mut Vec<String>,
) {
    let dependencies = &tool.dependencies;
    if dependencies.provisioning_group.trim().is_empty() {
        errors.push(format!("{label}: provisioningGroup is empty"));
    }
    if dependencies.primary_executable != spec.executable {
        errors.push(format!(
            "{label}: primaryExecutable is {}; catalog requires {}",
            dependencies.primary_executable, spec.executable
        ));
    }
    let overrides = unique_string_set(
        &dependencies.program_overrides,
        label,
        "programOverrides",
        errors,
    );
    if overrides.contains(spec.executable.as_str()) {
        errors.push(format!(
            "{label}: programOverrides repeats the primary executable"
        ));
    }
    let required = required_auxiliary_programs(spec);
    if overrides != required.iter().map(String::as_str).collect() {
        errors.push(format!(
            "{label}: programOverrides are {}; catalog requires {}",
            overrides.iter().copied().collect::<Vec<_>>().join(", "),
            required.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    let wrappers = unique_string_set(
        &dependencies.wrapper_executables,
        label,
        "wrapperExecutables",
        errors,
    );
    for wrapper in wrappers {
        if wrapper == spec.executable || overrides.contains(wrapper) {
            errors.push(format!(
                "{label}: wrapperExecutables repeats directly invoked program {wrapper}"
            ));
        }
    }
}

fn validate_provenance(
    manifest: &ValidationManifest,
    tool: &ToolValidation,
    label: &str,
    errors: &mut Vec<String>,
) {
    if !manifest
        .catalog_sources
        .contains_key(&tool.provenance.catalog_source)
    {
        errors.push(format!(
            "{label}: provenance names unknown catalogSource {}",
            tool.provenance.catalog_source
        ));
    }
    match &tool.provenance.upstream {
        UpstreamProvenance::Recorded {
            url,
            version,
            installation_source,
            version_command,
        } => {
            for (field, value) in [
                ("url", url),
                ("version", version),
                ("installationSource", installation_source),
            ] {
                if value.trim().is_empty() {
                    errors.push(format!("{label}: upstream provenance {field} is empty"));
                }
            }
            if version_command.is_empty()
                || version_command.iter().any(|part| part.trim().is_empty())
            {
                errors.push(format!(
                    "{label}: upstream provenance versionCommand is empty"
                ));
            }
        }
        UpstreamProvenance::Gap {
            reason,
            tracking_issue,
        } => {
            if reason.trim().is_empty() {
                errors.push(format!("{label}: upstream provenance gap has no reason"));
            }
            if *tracking_issue == 0 {
                errors.push(format!(
                    "{label}: upstream provenance gap has no tracking issue"
                ));
            }
        }
    }
}

fn validate_constraints(tool: &ToolValidation, label: &str, errors: &mut Vec<String>) {
    if tool.constraints.platforms.is_empty() {
        errors.push(format!("{label}: constraints.platforms is empty"));
    }
    unique_copy_set(
        &tool.constraints.platforms,
        label,
        "constraints.platforms",
        errors,
    );
    if tool.constraints.architectures.is_empty() {
        errors.push(format!("{label}: constraints.architectures is empty"));
    }
    unique_copy_set(
        &tool.constraints.architectures,
        label,
        "constraints.architectures",
        errors,
    );
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExecutionTargetKey {
    surface: EvidenceSurface,
    target: String,
}

#[derive(Debug, Default)]
struct ContractRequirements {
    targets: BTreeMap<ExecutionTargetKey, BTreeSet<ContractCase>>,
    surfaces: BTreeMap<EvidenceSurface, BTreeSet<ContractCase>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ResolutionKey {
    tier: EvidenceTier,
    surface: EvidenceSurface,
    target: Option<String>,
    case: Option<ContractCase>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolution {
    Covered,
    Gap,
    Exception,
}

#[allow(clippy::too_many_arguments)]
fn validate_evidence(
    evidence: &EvidenceRecord,
    index: usize,
    label: &str,
    requirements: &ContractRequirements,
    supported_surfaces: &BTreeSet<EvidenceSurface>,
    resolutions: &mut BTreeMap<ResolutionKey, Vec<Resolution>>,
    errors: &mut Vec<String>,
) {
    let evidence_label = format!("{label}: evidence[{index}]");
    let surfaces = unique_copy_set(&evidence.surfaces, &evidence_label, "surfaces", errors);
    let targets = unique_string_set(&evidence.targets, &evidence_label, "targets", errors);
    let cases = unique_copy_set(&evidence.cases, &evidence_label, "cases", errors);
    let surface_cases = unique_copy_set(
        &evidence.surface_cases,
        &evidence_label,
        "surfaceCases",
        errors,
    );
    let references = unique_string_set(&evidence.references, &evidence_label, "references", errors);

    match evidence.status {
        EvidenceStatus::Covered => {
            if references.is_empty() {
                errors.push(format!(
                    "{evidence_label}: covered evidence has no references"
                ));
            }
            if evidence.reason.is_some() || evidence.tracking_issue.is_some() {
                errors.push(format!(
                    "{evidence_label}: covered evidence must not carry gap metadata"
                ));
            }
        }
        EvidenceStatus::Gap => {
            if evidence
                .reason
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            {
                errors.push(format!("{evidence_label}: gap has no reason"));
            }
            if evidence.tracking_issue.is_none_or(|issue| issue == 0) {
                errors.push(format!("{evidence_label}: gap has no tracking issue"));
            }
            if !references.is_empty() {
                errors.push(format!("{evidence_label}: gap must not claim references"));
            }
        }
    }

    match evidence.tier {
        EvidenceTier::Schema => {
            if surfaces != BTreeSet::from([EvidenceSurface::Catalog]) {
                errors.push(format!(
                    "{evidence_label}: schema evidence must target only catalog"
                ));
            }
            if !cases.is_empty() {
                errors.push(format!(
                    "{evidence_label}: schema evidence must not name contract cases"
                ));
            }
            if !surface_cases.is_empty() {
                errors.push(format!(
                    "{evidence_label}: schema evidence must not name orchestration cases"
                ));
            }
            if !targets.is_empty() {
                errors.push(format!(
                    "{evidence_label}: schema evidence must not name execution targets"
                ));
            }
        }
        EvidenceTier::RenderedCommand | EvidenceTier::PinnedRealTool => {
            if surfaces.len() != 1 {
                errors.push(format!(
                    "{evidence_label}: execution evidence must target exactly one surface"
                ));
            }
            if targets.is_empty() != cases.is_empty() {
                errors.push(format!(
                    "{evidence_label}: execution targets and target cases must both be empty or both be nonempty"
                ));
            }
            if cases.is_empty() && surface_cases.is_empty() {
                errors.push(format!(
                    "{evidence_label}: execution evidence has no contract cases"
                ));
            }
            for surface in &surfaces {
                if !supported_surfaces.contains(surface) {
                    errors.push(format!(
                        "{evidence_label}: {:?} is not a supported execution surface",
                        surface
                    ));
                }
            }
            for surface in &surfaces {
                for target in &targets {
                    let key = ExecutionTargetKey {
                        surface: *surface,
                        target: (*target).to_owned(),
                    };
                    let Some(required_cases) = requirements.targets.get(&key) else {
                        errors.push(format!(
                            "{evidence_label}: target {target} is not declared on {}",
                            surface_name(*surface)
                        ));
                        continue;
                    };
                    for case in &cases {
                        if !required_cases.contains(case) {
                            errors.push(format!(
                                "{evidence_label}: {case:?} is not required for {}/{target}",
                                surface_name(*surface)
                            ));
                        }
                    }
                }
                let Some(required_cases) = requirements.surfaces.get(surface) else {
                    continue;
                };
                for case in &surface_cases {
                    if !required_cases.contains(case) {
                        errors.push(format!(
                            "{evidence_label}: {case:?} is not a surface orchestration case for {}",
                            surface_name(*surface)
                        ));
                    }
                }
            }
        }
    }

    let resolution = match evidence.status {
        EvidenceStatus::Covered => Resolution::Covered,
        EvidenceStatus::Gap => Resolution::Gap,
    };
    if evidence.tier == EvidenceTier::Schema {
        for surface in surfaces {
            resolutions
                .entry(ResolutionKey {
                    tier: evidence.tier,
                    surface,
                    target: None,
                    case: None,
                })
                .or_default()
                .push(resolution);
        }
    } else {
        for surface in surfaces {
            for case in &surface_cases {
                resolutions
                    .entry(ResolutionKey {
                        tier: evidence.tier,
                        surface,
                        target: None,
                        case: Some(*case),
                    })
                    .or_default()
                    .push(resolution);
            }
            for target in &targets {
                for case in &cases {
                    resolutions
                        .entry(ResolutionKey {
                            tier: evidence.tier,
                            surface,
                            target: Some((*target).to_owned()),
                            case: Some(*case),
                        })
                        .or_default()
                        .push(resolution);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_exception(
    exception: &ValidationException,
    label: &str,
    today: Option<IsoDate>,
    requirements: &ContractRequirements,
    supported_surfaces: &BTreeSet<EvidenceSurface>,
    exception_ids: &mut BTreeMap<String, String>,
    resolutions: &mut BTreeMap<ResolutionKey, Vec<Resolution>>,
    errors: &mut Vec<String>,
) {
    let exception_label = format!("{label}: exception {}", exception.id);
    if exception.id.trim().is_empty() {
        errors.push(format!("{label}: exception has an empty id"));
    } else if let Some(previous) = exception_ids.insert(exception.id.clone(), label.to_owned()) {
        errors.push(format!(
            "{exception_label}: exception ID duplicates declaration in {previous}"
        ));
    }
    if exception.owner.trim().is_empty() {
        errors.push(format!("{exception_label}: owner is empty"));
    }
    if exception.reason.trim().is_empty() {
        errors.push(format!("{exception_label}: reason is empty"));
    }
    if exception.tracking_issue == 0 {
        errors.push(format!("{exception_label}: trackingIssue must be nonzero"));
    }
    let expiry = IsoDate::parse(&exception.expires_on);
    if expiry.is_none() {
        errors.push(format!(
            "{exception_label}: expiresOn is not a valid YYYY-MM-DD date"
        ));
    }
    let active = match (expiry, today) {
        (Some(expiry), Some(today)) if expiry <= today => {
            errors.push(format!(
                "{exception_label}: expired on {}",
                exception.expires_on
            ));
            false
        }
        (Some(_), _) => true,
        _ => false,
    };
    let tiers = unique_copy_set(&exception.tiers, &exception_label, "tiers", errors);
    let surfaces = unique_copy_set(&exception.surfaces, &exception_label, "surfaces", errors);
    let targets = unique_string_set(&exception.targets, &exception_label, "targets", errors);
    let cases = unique_copy_set(&exception.cases, &exception_label, "cases", errors);
    let surface_cases = unique_copy_set(
        &exception.surface_cases,
        &exception_label,
        "surfaceCases",
        errors,
    );
    if tiers.is_empty() {
        errors.push(format!("{exception_label}: tiers is empty"));
    }
    if surfaces.is_empty() {
        errors.push(format!("{exception_label}: surfaces is empty"));
    }
    for tier in &tiers {
        match tier {
            EvidenceTier::Schema => {
                if surfaces != BTreeSet::from([EvidenceSurface::Catalog])
                    || !targets.is_empty()
                    || !cases.is_empty()
                    || !surface_cases.is_empty()
                {
                    errors.push(format!(
                        "{exception_label}: schema exceptions must target catalog without execution cases"
                    ));
                }
            }
            EvidenceTier::RenderedCommand | EvidenceTier::PinnedRealTool => {
                if surfaces.len() != 1 {
                    errors.push(format!(
                        "{exception_label}: execution exception must target exactly one surface"
                    ));
                }
                if targets.is_empty() != cases.is_empty() {
                    errors.push(format!(
                        "{exception_label}: execution targets and target cases must both be empty or both be nonempty"
                    ));
                }
                if cases.is_empty() && surface_cases.is_empty() {
                    errors.push(format!(
                        "{exception_label}: execution exception has no contract cases"
                    ));
                }
                for surface in &surfaces {
                    if !supported_surfaces.contains(surface) {
                        errors.push(format!(
                            "{exception_label}: {:?} is not a supported execution surface",
                            surface
                        ));
                    }
                }
                for surface in &surfaces {
                    for target in &targets {
                        let key = ExecutionTargetKey {
                            surface: *surface,
                            target: (*target).to_owned(),
                        };
                        let Some(required_cases) = requirements.targets.get(&key) else {
                            errors.push(format!(
                                "{exception_label}: target {target} is not declared on {}",
                                surface_name(*surface)
                            ));
                            continue;
                        };
                        for case in &cases {
                            if !required_cases.contains(case) {
                                errors.push(format!(
                                    "{exception_label}: {case:?} is not required for {}/{target}",
                                    surface_name(*surface)
                                ));
                            }
                        }
                    }
                    let Some(required_cases) = requirements.surfaces.get(surface) else {
                        continue;
                    };
                    for case in &surface_cases {
                        if !required_cases.contains(case) {
                            errors.push(format!(
                                "{exception_label}: {case:?} is not a surface orchestration case for {}",
                                surface_name(*surface)
                            ));
                        }
                    }
                }
            }
        }
    }
    if !active {
        return;
    }
    for tier in tiers {
        if tier == EvidenceTier::Schema {
            for surface in &surfaces {
                resolutions
                    .entry(ResolutionKey {
                        tier,
                        surface: *surface,
                        target: None,
                        case: None,
                    })
                    .or_default()
                    .push(Resolution::Exception);
            }
        } else {
            for surface in &surfaces {
                for case in &surface_cases {
                    resolutions
                        .entry(ResolutionKey {
                            tier,
                            surface: *surface,
                            target: None,
                            case: Some(*case),
                        })
                        .or_default()
                        .push(Resolution::Exception);
                }
                for target in &targets {
                    for case in &cases {
                        resolutions
                            .entry(ResolutionKey {
                                tier,
                                surface: *surface,
                                target: Some((*target).to_owned()),
                                case: Some(*case),
                            })
                            .or_default()
                            .push(Resolution::Exception);
                    }
                }
            }
        }
    }
}

fn validate_resolution_matrix(
    tool: &ToolValidation,
    label: &str,
    requirements: &ContractRequirements,
    resolutions: &BTreeMap<ResolutionKey, Vec<Resolution>>,
    errors: &mut Vec<String>,
) {
    validate_resolution(
        ResolutionKey {
            tier: EvidenceTier::Schema,
            surface: EvidenceSurface::Catalog,
            target: None,
            case: None,
        },
        label,
        resolutions,
        errors,
    );
    if tool.support == SupportState::Disabled {
        if tool
            .evidence
            .iter()
            .any(|evidence| evidence.tier != EvidenceTier::Schema)
            || tool.exceptions.iter().any(|exception| {
                exception
                    .tiers
                    .iter()
                    .any(|tier| *tier != EvidenceTier::Schema)
            })
        {
            errors.push(format!(
                "{label}: disabled declaration must not claim execution evidence"
            ));
        }
        return;
    }
    for tier in [EvidenceTier::RenderedCommand, EvidenceTier::PinnedRealTool] {
        for (surface, required_cases) in &requirements.surfaces {
            for case in required_cases {
                validate_resolution(
                    ResolutionKey {
                        tier,
                        surface: *surface,
                        target: None,
                        case: Some(*case),
                    },
                    label,
                    resolutions,
                    errors,
                );
            }
        }
        for (target, required_cases) in &requirements.targets {
            for case in required_cases {
                validate_resolution(
                    ResolutionKey {
                        tier,
                        surface: target.surface,
                        target: Some(target.target.clone()),
                        case: Some(*case),
                    },
                    label,
                    resolutions,
                    errors,
                );
            }
        }
    }
}

fn validate_resolution(
    key: ResolutionKey,
    label: &str,
    resolutions: &BTreeMap<ResolutionKey, Vec<Resolution>>,
    errors: &mut Vec<String>,
) {
    let scope = key.target.as_deref().unwrap_or(if key.case.is_some() {
        "surface"
    } else {
        "catalog"
    });
    match resolutions.get(&key).map(Vec::len).unwrap_or(0) {
        1 => {}
        0 => errors.push(format!(
            "{label}: unresolved {:?}/{}/{}/{} contract cell",
            key.tier,
            surface_name(key.surface),
            scope,
            key.case
                .map(|case| format!("{case:?}"))
                .unwrap_or_else(|| "schema".into())
        )),
        count => errors.push(format!(
            "{label}: {:?}/{}/{}/{} contract cell has {count} resolutions",
            key.tier,
            surface_name(key.surface),
            scope,
            key.case
                .map(|case| format!("{case:?}"))
                .unwrap_or_else(|| "schema".into())
        )),
    }
}

fn supported_execution_surfaces(surfaces: &Surfaces) -> BTreeSet<EvidenceSurface> {
    let mut supported = BTreeSet::new();
    if surfaces.immediate == ImmediateSurface::Supported {
        supported.insert(EvidenceSurface::Immediate);
    }
    if surfaces.deferred != DeferredSurface::NotSupported {
        supported.insert(EvidenceSurface::Deferred);
    }
    supported
}

fn validate_fixtures(
    tools: &[ToolValidation],
    inventory: &BTreeMap<String, BTreeSet<String>>,
    errors: &mut Vec<String>,
) {
    let declarations = tools
        .iter()
        .filter(|tool| !tool.id.trim().is_empty())
        .map(|tool| (tool.id.as_str(), tool))
        .collect::<BTreeMap<_, _>>();
    for (tool_id, actual_cases) in inventory {
        if !declarations.contains_key(tool_id.as_str()) {
            errors.push(format!(
                "fixture tool {tool_id} has no validation declaration"
            ));
        }
        if actual_cases.is_empty() {
            errors.push(format!(
                "fixture tool {tool_id} has no scenario directories"
            ));
        }
        for case in actual_cases {
            if case.trim().is_empty() {
                errors.push(format!("fixture tool {tool_id} has an empty case ID"));
            }
        }
    }
    for tool in tools {
        let declared = tool
            .fixture_cases
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let actual = inventory
            .get(&tool.id)
            .map(|cases| cases.iter().map(String::as_str).collect::<BTreeSet<_>>())
            .unwrap_or_default();
        for case in declared.difference(&actual) {
            errors.push(format!(
                "{} ({}): declared fixture case {case} is missing",
                tool.builtin, tool.id
            ));
        }
        for case in actual.difference(&declared) {
            errors.push(format!(
                "{} ({}): fixture case {case} is not declared",
                tool.builtin, tool.id
            ));
        }
    }
}

fn build_coverage_summary(
    manifest: &ValidationManifest,
    fixture_inventory: Option<&BTreeMap<String, BTreeSet<String>>>,
) -> CoverageSummary {
    let mut tools = manifest.tools.iter().collect::<Vec<_>>();
    tools.sort_by(|left, right| left.builtin.cmp(&right.builtin));
    let tools = tools
        .into_iter()
        .map(|tool| {
            let mut surface_layers = BTreeMap::new();
            let mut surface_case_layers = BTreeMap::new();
            for (surface, contract) in [
                (
                    EvidenceSurface::Immediate,
                    tool.contracts.immediate.as_ref(),
                ),
                (EvidenceSurface::Deferred, tool.contracts.deferred.as_ref()),
            ] {
                let mut states = BTreeMap::new();
                for tier in [EvidenceTier::RenderedCommand, EvidenceTier::PinnedRealTool] {
                    states.insert(
                        tier,
                        if contract.is_none() || tool.support == SupportState::Disabled {
                            LayerState::NotRequired
                        } else {
                            scoped_layer_state(tool, tier, Some(surface), None, None)
                        },
                    );
                }
                surface_layers.insert(surface, states);

                let mut case_layers = BTreeMap::new();
                if let Some(contract) = contract {
                    for case in &contract.orchestration_cases {
                        let mut states = BTreeMap::new();
                        for tier in [EvidenceTier::RenderedCommand, EvidenceTier::PinnedRealTool] {
                            states.insert(
                                tier,
                                scoped_layer_state(tool, tier, Some(surface), None, Some(*case)),
                            );
                        }
                        case_layers.insert(*case, states);
                    }
                }
                surface_case_layers.insert(surface, case_layers);
            }

            let mut target_layers = Vec::new();
            for (surface, contract) in [
                (
                    EvidenceSurface::Immediate,
                    tool.contracts.immediate.as_ref(),
                ),
                (EvidenceSurface::Deferred, tool.contracts.deferred.as_ref()),
            ] {
                let Some(contract) = contract else { continue };
                for target in &contract.targets {
                    let mut case_layers = BTreeMap::new();
                    for case in &target.required_cases {
                        let mut states = BTreeMap::new();
                        for tier in [EvidenceTier::RenderedCommand, EvidenceTier::PinnedRealTool] {
                            states.insert(
                                tier,
                                scoped_layer_state(
                                    tool,
                                    tier,
                                    Some(surface),
                                    Some(&target.id),
                                    Some(*case),
                                ),
                            );
                        }
                        case_layers.insert(*case, states);
                    }
                    let mut states = BTreeMap::new();
                    for tier in [EvidenceTier::RenderedCommand, EvidenceTier::PinnedRealTool] {
                        states.insert(
                            tier,
                            scoped_layer_state(tool, tier, Some(surface), Some(&target.id), None),
                        );
                    }
                    target_layers.push(TargetCoverage {
                        surface,
                        target: target.id.clone(),
                        capabilities: sorted_unique(&target.capabilities),
                        required_cases: sorted_unique(&target.required_cases),
                        layers: states,
                        case_layers,
                    });
                }
            }

            let mut layers = BTreeMap::new();
            layers.insert(
                EvidenceTier::Schema,
                scoped_layer_state(tool, EvidenceTier::Schema, None, None, None),
            );
            for tier in [EvidenceTier::RenderedCommand, EvidenceTier::PinnedRealTool] {
                layers.insert(
                    tier,
                    if tool.support == SupportState::Disabled {
                        LayerState::NotRequired
                    } else {
                        combine_layer_states(surface_layers.values().map(|states| states[&tier]))
                    },
                );
            }
            ToolCoverage {
                builtin: tool.builtin.clone(),
                id: tool.id.clone(),
                support: tool.support,
                surfaces: tool.surfaces.clone(),
                contracts: tool.contracts.clone(),
                fixture_cases: {
                    let mut cases = tool.fixture_cases.clone();
                    cases.sort();
                    cases
                },
                layers,
                surface_layers,
                surface_case_layers,
                target_layers,
                gap_records: tool
                    .evidence
                    .iter()
                    .filter(|evidence| evidence.status == EvidenceStatus::Gap)
                    .count()
                    + usize::from(matches!(
                        tool.provenance.upstream,
                        UpstreamProvenance::Gap { .. }
                    )),
                active_exceptions: tool.exceptions.len(),
            }
        })
        .collect::<Vec<_>>();

    let mut layers = BTreeMap::new();
    for tier in [
        EvidenceTier::Schema,
        EvidenceTier::RenderedCommand,
        EvidenceTier::PinnedRealTool,
    ] {
        let mut totals = LayerTotals::default();
        for tool in &tools {
            match tool.layers[&tier] {
                LayerState::Covered => totals.covered += 1,
                LayerState::Gap => totals.gap += 1,
                LayerState::Exception => totals.exception += 1,
                LayerState::NotRequired => totals.not_required += 1,
            }
        }
        layers.insert(tier, totals);
    }

    let enabled_tools = tools
        .iter()
        .filter(|tool| tool.support == SupportState::Enabled)
        .count();
    let fixture_tools = fixture_inventory.map(BTreeMap::len).unwrap_or_else(|| {
        tools
            .iter()
            .filter(|tool| !tool.fixture_cases.is_empty())
            .count()
    });
    let fixture_cases = fixture_inventory
        .map(|inventory| inventory.values().map(BTreeSet::len).sum())
        .unwrap_or_else(|| tools.iter().map(|tool| tool.fixture_cases.len()).sum());
    CoverageSummary {
        total_tools: tools.len(),
        enabled_tools,
        disabled_tools: tools.len() - enabled_tools,
        fixture_tools,
        fixture_cases,
        layers,
        tools,
    }
}

fn scoped_layer_state(
    tool: &ToolValidation,
    tier: EvidenceTier,
    surface: Option<EvidenceSurface>,
    target: Option<&str>,
    case: Option<ContractCase>,
) -> LayerState {
    let records = tool
        .evidence
        .iter()
        .filter(|evidence| {
            evidence.tier == tier
                && surface.is_none_or(|surface| evidence.surfaces.contains(&surface))
                && target.is_none_or(|target| evidence.targets.iter().any(|item| item == target))
                && case.is_none_or(|case| {
                    if target.is_some() {
                        evidence.cases.contains(&case)
                    } else {
                        evidence.surface_cases.contains(&case)
                    }
                })
        })
        .collect::<Vec<_>>();
    if (tier == EvidenceTier::PinnedRealTool
        && matches!(tool.provenance.upstream, UpstreamProvenance::Gap { .. }))
        || records
            .iter()
            .any(|evidence| evidence.status == EvidenceStatus::Gap)
    {
        LayerState::Gap
    } else if !tool.exceptions.is_empty()
        && tool.exceptions.iter().any(|exception| {
            exception.tiers.contains(&tier)
                && surface.is_none_or(|surface| exception.surfaces.contains(&surface))
                && target.is_none_or(|target| exception.targets.iter().any(|item| item == target))
                && case.is_none_or(|case| {
                    if target.is_some() {
                        exception.cases.contains(&case)
                    } else {
                        exception.surface_cases.contains(&case)
                    }
                })
        })
    {
        LayerState::Exception
    } else {
        LayerState::Covered
    }
}

fn combine_layer_states(states: impl Iterator<Item = LayerState>) -> LayerState {
    let states = states.collect::<Vec<_>>();
    if states.contains(&LayerState::Gap) {
        LayerState::Gap
    } else if states.contains(&LayerState::Exception) {
        LayerState::Exception
    } else if states.contains(&LayerState::Covered) {
        LayerState::Covered
    } else {
        LayerState::NotRequired
    }
}

/// Render a deterministic human-readable report from validated coverage.
pub fn render_coverage_markdown(summary: &CoverageSummary) -> String {
    let mut output =
        String::from("# Built-in validation coverage\n\n<!-- markdownlint-disable MD013 -->\n\n");
    output.push_str(
        "Generated from the evaluated Pkl catalog, the validation manifest, and the declared fixture inventory. Fixture presence is inventory only: it does not establish rendered-command or pinned-real-tool evidence. `gap` is an explicit missing claim, never a successful skip. The per-tool gap count includes evidence records and missing upstream provenance. Disabled drafts remain in the complete catalog count but are not required in execution-evidence denominators.\n\n",
    );
    output.push_str(&format!(
        "Catalog: **{} total** (**{} enabled**, **{} disabled**). Fixture inventory: **{} tools**, **{} cases**.\n\n",
        summary.total_tools,
        summary.enabled_tools,
        summary.disabled_tools,
        summary.fixture_tools,
        summary.fixture_cases
    ));
    output.push_str("| Evidence tier | Covered | Gap | Exception | Not required |\n");
    output.push_str("| --- | ---: | ---: | ---: | ---: |\n");
    for tier in [
        EvidenceTier::Schema,
        EvidenceTier::RenderedCommand,
        EvidenceTier::PinnedRealTool,
    ] {
        let totals = &summary.layers[&tier];
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            evidence_tier_name(tier),
            totals.covered,
            totals.gap,
            totals.exception,
            totals.not_required
        ));
    }
    output.push_str("\n| Built-in | Tool ID | Support | Immediate contract | Deferred contract | Fixture cases | Schema | Rendered immediate | Rendered deferred | Pinned immediate | Pinned deferred | Gaps | Exceptions |\n");
    output.push_str(
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | ---: | ---: |\n",
    );
    for tool in &summary.tools {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(&tool.builtin),
            markdown_cell(&tool.id),
            support_name(tool.support),
            markdown_cell(&format!(
                "{} · {}",
                immediate_name(tool.surfaces.immediate),
                surface_contract_summary(tool.contracts.immediate.as_ref())
            )),
            markdown_cell(&format!(
                "{} · {}",
                deferred_name(tool.surfaces.deferred),
                surface_contract_summary(tool.contracts.deferred.as_ref())
            )),
            markdown_cell(&if tool.fixture_cases.is_empty() {
                "—".into()
            } else {
                tool.fixture_cases.join(", ")
            }),
            layer_state_name(tool.layers[&EvidenceTier::Schema]),
            layer_state_name(
                tool.surface_layers[&EvidenceSurface::Immediate][&EvidenceTier::RenderedCommand],
            ),
            layer_state_name(
                tool.surface_layers[&EvidenceSurface::Deferred][&EvidenceTier::RenderedCommand],
            ),
            layer_state_name(
                tool.surface_layers[&EvidenceSurface::Immediate][&EvidenceTier::PinnedRealTool],
            ),
            layer_state_name(
                tool.surface_layers[&EvidenceSurface::Deferred][&EvidenceTier::PinnedRealTool],
            ),
            tool.gap_records,
            tool.active_exceptions,
        ));
    }
    output
}

fn surface_contract_summary(contract: Option<&SurfaceContract>) -> String {
    let Some(contract) = contract else {
        return "—".into();
    };
    let target_cases = contract
        .targets
        .iter()
        .flat_map(|target| target.required_cases.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    format!(
        "targets: {}; capabilities: {}; orchestration: {}; target cases: {}",
        contract
            .targets
            .iter()
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        join_serialized(&contract.capabilities),
        join_serialized(&contract.orchestration_cases),
        join_serialized(&target_cases)
    )
}

fn evidence_tier_name(tier: EvidenceTier) -> &'static str {
    match tier {
        EvidenceTier::Schema => "Schema",
        EvidenceTier::RenderedCommand => "Rendered command",
        EvidenceTier::PinnedRealTool => "Pinned real tool",
    }
}

fn surface_name(surface: EvidenceSurface) -> &'static str {
    match surface {
        EvidenceSurface::Catalog => "catalog",
        EvidenceSurface::Immediate => "immediate",
        EvidenceSurface::Deferred => "deferred",
    }
}

fn support_name(state: SupportState) -> &'static str {
    match state {
        SupportState::Enabled => "enabled",
        SupportState::Disabled => "disabled",
    }
}

fn immediate_name(surface: ImmediateSurface) -> &'static str {
    match surface {
        ImmediateSurface::Supported => "supported",
        ImmediateSurface::NotSupported => "not-supported",
    }
}

fn deferred_name(surface: DeferredSurface) -> &'static str {
    match surface {
        DeferredSurface::Explicit => "explicit",
        DeferredSurface::Compatibility => "compatibility",
        DeferredSurface::NotSupported => "not-supported",
    }
}

fn layer_state_name(state: LayerState) -> &'static str {
    match state {
        LayerState::Covered => "covered",
        LayerState::Gap => "gap",
        LayerState::Exception => "exception",
        LayerState::NotRequired => "not-required",
    }
}

fn join_serialized<T: Serialize>(values: &[T]) -> String {
    if values.is_empty() {
        return "—".into();
    }
    values
        .iter()
        .map(|value| {
            serde_json::to_value(value)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "?".into())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn unique_copy_set<T>(
    values: &[T],
    label: &str,
    field: &str,
    errors: &mut Vec<String>,
) -> BTreeSet<T>
where
    T: Copy + Ord + fmt::Debug,
{
    let set = values.iter().copied().collect::<BTreeSet<_>>();
    if set.len() != values.len() {
        errors.push(format!("{label}: {field} contains duplicates"));
    }
    set
}

fn unique_string_set<'a>(
    values: &'a [String],
    label: &str,
    field: &str,
    errors: &mut Vec<String>,
) -> BTreeSet<&'a str> {
    let set = values.iter().map(String::as_str).collect::<BTreeSet<_>>();
    if set.len() != values.len() {
        errors.push(format!("{label}: {field} contains duplicates"));
    }
    if values.iter().any(|value| value.trim().is_empty()) {
        errors.push(format!("{label}: {field} contains an empty value"));
    }
    set
}

fn sorted_unique<T>(values: &[T]) -> Vec<T>
where
    T: Copy + Ord,
{
    values
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn debug_set<T: fmt::Debug>(values: &BTreeSet<T>) -> String {
    values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct IsoDate {
    year: u32,
    month: u32,
    day: u32,
}

impl IsoDate {
    fn parse(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        if bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
        {
            return None;
        }
        let year = value[0..4].parse().ok()?;
        let month = value[5..7].parse().ok()?;
        let day = value[8..10].parse().ok()?;
        if year == 0 || !(1..=12).contains(&month) {
            return None;
        }
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if !(1..=days).contains(&day) {
            return None;
        }
        Some(Self { year, month, day })
    }
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_parser_rejects_invalid_and_accepts_leap_day() {
        assert!(IsoDate::parse("2028-02-29").is_some());
        assert!(IsoDate::parse("2027-02-29").is_none());
        assert!(IsoDate::parse("2026-8-08").is_none());
        assert!(IsoDate::parse("0000-01-01").is_none());
    }

    #[test]
    fn epoch_conversion_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_673), (2026, 8, 8));
    }

    #[test]
    fn minimum_contracts_are_composable() {
        let capabilities = BTreeSet::from([
            Capability::Checker,
            Capability::Mutator,
            Capability::StdoutSignaled,
            Capability::Batch,
        ]);
        let cases = minimum_required_cases(&capabilities);
        assert!(cases.contains(&ContractCase::Clean));
        assert!(cases.contains(&ContractCase::Idempotence));
        assert!(cases.contains(&ContractCase::StdoutIssue));
        assert!(cases.contains(&ContractCase::BatchAttribution));
    }

    #[test]
    fn per_file_contract_requires_multiple_candidates() {
        let cases =
            minimum_required_cases(&BTreeSet::from([Capability::Checker, Capability::PerFile]));

        assert!(cases.contains(&ContractCase::MultiFile));
        assert!(cases.contains(&ContractCase::PerFileInvocation));
    }

    #[test]
    fn phase_invocation_drives_immediate_and_compatibility_contracts() {
        let spec = ToolSpec {
            executable: "example".into(),
            phase_invocation: InvocationGranularity::PerFile,
            phases: BTreeMap::from([("verify".into(), Phase::default())]),
            phase_order: vec!["verify".into()],
            ..ToolSpec::default()
        };

        let contracts = derived_surface_contracts(&spec);
        let immediate = contracts.immediate.expect("immediate contract");
        let deferred = contracts.deferred.expect("compatibility contract");
        assert_eq!(
            immediate.targets[0].invocation,
            InvocationGranularity::PerFile
        );
        assert_eq!(
            deferred.targets[0].invocation,
            InvocationGranularity::PerFile
        );
        assert!(immediate.capabilities.contains(&Capability::PerFile));
        assert!(deferred.capabilities.contains(&Capability::PerFile));
        assert_eq!(
            deferred.targets[0].kind,
            CommandTargetKind::CompatibilityWorkflow
        );
    }

    #[test]
    fn stdout_capability_is_derived_from_any_workflow_command() {
        let remedy = WorkflowCommand {
            issues_on_stdout: true,
            ..WorkflowCommand::default()
        };
        let spec = ToolSpec {
            executable: "example".into(),
            workflows: BTreeMap::from([(
                "fix".into(),
                crate::schema::Workflow {
                    check: Some(WorkflowCommand::default()),
                    remedy: Some(remedy),
                    ..crate::schema::Workflow::default()
                },
            )]),
            ..ToolSpec::default()
        };

        let deferred = derived_surface_contracts(&spec).deferred.unwrap();
        assert!(deferred.capabilities.contains(&Capability::StdoutSignaled));
        assert!(deferred.required_cases.contains(&ContractCase::StdoutIssue));
    }
}
