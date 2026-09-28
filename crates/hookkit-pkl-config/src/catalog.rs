//! Structural validation and deterministic audit output for embedded tools.

use crate::schema::{
    ArgToken, ArgvElement, CheckScope, ExitCodes, InvocationGranularity, Phase, PhaseMode,
    RunnerConfig, ToolSpec, WorkflowCommand, WorkspaceFallback, WriteBehavior,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// All structural catalog violations found in one validation pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogValidationError {
    /// Human-readable structural violations in deterministic order.
    pub errors: Vec<String>,
}

impl fmt::Display for CatalogValidationError {
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

impl std::error::Error for CatalogValidationError {}

/// Reject catalog entries that cannot support authoritative deferred checks.
///
/// Legacy `phases` remain valid when their compatibility translation pairs
/// each mutator with a read-only verifier. A mutating-only legacy entry must
/// carry a nonempty `unverifiedRemedyFallback` explanation; the shipped
/// catalog intentionally contains no such fallback.
pub fn validate_builtin_catalog(
    specs: &BTreeMap<String, ToolSpec>,
) -> Result<(), CatalogValidationError> {
    let mut errors = Vec::new();
    validate_specs(specs, Strictness::Builtin, &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CatalogValidationError { errors })
    }
}

/// Validate the tools a resolved configuration will run: every `run` entry
/// must name a defined tool, every glob (tool files and `settings.exclude`)
/// must compile, and each enabled tool must pass the builtin catalog's
/// structural rules. Two exceptions apply to user tools: one whose phases
/// only mutate (an hk-style formatter) is accepted without an
/// `unverifiedRemedyFallback`, since immediate mode runs it as written and
/// the deferred runner reports its fixes as unverified auto-fixes; and one
/// whose workflows are all disabled is accepted as immediate-only.
pub fn validate_run_config(config: &RunnerConfig) -> Result<(), CatalogValidationError> {
    let mut errors = Vec::new();
    let mut selected = BTreeMap::new();
    for key in &config.run {
        match config.tools.get(key) {
            Some(spec) => {
                selected.insert(key.clone(), spec.clone());
            }
            None => errors.push(format!(
                "run names unknown tool `{key}`; define it under `tools` (for a builtin: `[\"{key}\"] = Builtins.{key}`) or remove it from `run`"
            )),
        }
    }
    validate_globs("settings.exclude", &config.settings.exclude, &mut errors);
    validate_specs(&selected, Strictness::User, &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CatalogValidationError { errors })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Strictness {
    Builtin,
    User,
}

fn validate_specs(
    specs: &BTreeMap<String, ToolSpec>,
    strictness: Strictness,
    errors: &mut Vec<String>,
) {
    let mut tool_ids = BTreeSet::new();
    for (key, spec) in specs {
        if !spec.enabled {
            continue;
        }
        let prefix = format!("{key} ({})", spec.id);
        if spec.id.trim().is_empty() {
            errors.push(format!("{key}: tool id is empty"));
        } else if !tool_ids.insert(spec.id.as_str()) {
            errors.push(format!(
                "{prefix}: duplicate tool id; give each tool a distinct `id`"
            ));
        }
        if spec.executable.trim().is_empty() {
            errors.push(format!("{prefix}: executable is empty"));
        }
        validate_globs(
            &format!("{prefix}: files.include"),
            &spec.files.include,
            errors,
        );
        validate_globs(
            &format!("{prefix}: files.exclude"),
            &spec.files.exclude,
            errors,
        );
        validate_order(
            &prefix,
            "workflowOrder",
            &spec.workflow_order,
            spec.workflows.keys(),
            errors,
        );
        validate_order(
            &prefix,
            "phaseOrder",
            &spec.phase_order,
            spec.phases.keys(),
            errors,
        );

        validate_workspace_fallback(&prefix, spec, errors);

        if spec.workflows.is_empty() {
            validate_compatibility_tool(&prefix, spec, strictness, errors);
        } else {
            validate_explicit_tool(&prefix, spec, strictness, errors);
        }
    }
}

/// A project-root fallback groups files that have no workspace indicator, so
/// it needs one, and its jobs have no marker for a `WorkspaceIndicator`
/// token to name.
fn validate_workspace_fallback(prefix: &str, spec: &ToolSpec, errors: &mut Vec<String>) {
    if spec.workspace_fallback != WorkspaceFallback::ProjectRoot {
        return;
    }
    if spec.workspace_indicator.is_none() {
        errors.push(format!(
            "{prefix}: workspaceFallback = \"project-root\" needs a workspaceIndicator"
        ));
    }
    let phases = spec.phases.values().map(|phase| &phase.argv);
    let workflows = spec.workflows.values().flat_map(|workflow| {
        let commands = workflow.check.iter().chain(workflow.remedy.iter());
        commands.map(|command| &command.argv)
    });
    let names_marker = |argv: &Vec<ArgvElement>| {
        argv.iter()
            .any(|element| matches!(element, ArgvElement::Token(ArgToken::WorkspaceIndicator)))
    };
    if phases.chain(workflows).any(names_marker) {
        errors.push(format!(
            "{prefix}: workspaceFallback = \"project-root\" runs jobs without a marker, so no command may use WorkspaceIndicator"
        ));
    }
}

/// Reject patterns the runner's glob matcher cannot compile, so a typo fails
/// loudly at load time (and in `doctor`) instead of on every edit.
fn validate_globs(label: &str, patterns: &[String], errors: &mut Vec<String>) {
    for pattern in patterns {
        if let Err(error) = globset::Glob::new(pattern) {
            errors.push(format!(
                "{label} has an invalid glob `{pattern}`: {}",
                error.kind()
            ));
        }
    }
}

fn validate_order<'a>(
    prefix: &str,
    field: &str,
    order: &[String],
    available: impl Iterator<Item = &'a String>,
    errors: &mut Vec<String>,
) {
    let available = available.map(String::as_str).collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for id in order {
        if !available.contains(id.as_str()) {
            errors.push(format!("{prefix}: {field} names unknown entry {id}"));
        }
        if !seen.insert(id) {
            errors.push(format!("{prefix}: {field} repeats {id}"));
        }
    }
}

fn validate_explicit_tool(
    prefix: &str,
    spec: &ToolSpec,
    strictness: Strictness,
    errors: &mut Vec<String>,
) {
    if spec.unverified_remedy_fallback.is_some() {
        errors.push(format!(
            "{prefix}: unverifiedRemedyFallback is stale because explicit workflows exist"
        ));
    }
    let mut enabled = 0;
    for (id, workflow) in ordered_workflows(spec) {
        if !workflow.enabled {
            continue;
        }
        enabled += 1;
        let label = format!("{prefix}: workflow {id}");
        let Some(check) = workflow.check.as_ref() else {
            errors.push(format!("{label} has no authoritative check"));
            continue;
        };
        validate_command(&format!("{label} check"), check, true, errors);
        if let Some(remedy) = &workflow.remedy {
            validate_command(&format!("{label} remedy"), remedy, false, errors);
        }
    }
    // A user tool with every workflow disabled is immediate-only: Stop skips
    // it, and immediate mode still runs its phases. Built-ins must support
    // Stop.
    if enabled == 0 && strictness == Strictness::Builtin {
        errors.push(format!("{prefix}: no deferred workflow is enabled"));
    }
}

fn validate_compatibility_tool(
    prefix: &str,
    spec: &ToolSpec,
    strictness: Strictness,
    errors: &mut Vec<String>,
) {
    let phases = ordered_phases(spec);
    let enabled = phases
        .into_iter()
        .filter(|(_, phase)| phase.enabled)
        .collect::<Vec<_>>();
    let mutators = enabled
        .iter()
        .filter(|(_, phase)| !is_verifier(phase.mode))
        .collect::<Vec<_>>();
    let verifiers = enabled
        .iter()
        .filter(|(_, phase)| is_verifier(phase.mode))
        .collect::<Vec<_>>();

    for (id, phase) in &enabled {
        validate_exit_codes(&format!("{prefix}: phase {id}"), &phase.exit_codes, errors);
        if is_verifier(phase.mode) && phase.writes != WriteBehavior::None {
            errors.push(format!(
                "{prefix}: verifier phase {id} declares writes; verify/check-only phases must use writes = \"none\""
            ));
        }
        if !is_verifier(phase.mode) && phase.writes == WriteBehavior::None {
            errors.push(format!(
                "{prefix}: mutating phase {id} has writes=none; declare writes = \"target-files\" (or \"matching-globs\"/\"workspace\") so changes are detected"
            ));
        }
    }

    if enabled.is_empty() {
        errors.push(format!("{prefix}: no phase is enabled"));
    }
    if !mutators.is_empty() && verifiers.is_empty() {
        match spec.unverified_remedy_fallback.as_deref().map(str::trim) {
            Some(reason) if !reason.is_empty() => {}
            _ if strictness == Strictness::User => {}
            _ => errors.push(format!(
                "{prefix}: auto-fix compatibility translation has no authoritative final check"
            )),
        }
    } else if spec.unverified_remedy_fallback.is_some() {
        errors.push(format!(
            "{prefix}: unverifiedRemedyFallback is stale because a verifier is available"
        ));
    }
}

fn validate_command(
    label: &str,
    command: &WorkflowCommand,
    is_check: bool,
    errors: &mut Vec<String>,
) {
    validate_exit_codes(label, &command.exit_codes, errors);
    if command
        .program
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        errors.push(format!("{label} has an empty program override"));
    }
    if is_check && command.writes != WriteBehavior::None {
        errors.push(format!("{label} is not read-only"));
    }
    if !is_check && command.writes == WriteBehavior::None {
        errors.push(format!("{label} has no declared write scope"));
    }
}

fn validate_exit_codes(label: &str, codes: &ExitCodes, errors: &mut Vec<String>) {
    if codes.clean.is_empty() {
        errors.push(format!("{label} has no clean exit code"));
    }
    let mut classified = BTreeMap::<i32, &'static str>::new();
    for (kind, values) in [
        ("clean", &codes.clean),
        ("issues", &codes.issues),
        ("failure", &codes.failure),
    ] {
        for code in values {
            if let Some(previous) = classified.insert(*code, kind) {
                errors.push(format!(
                    "{label} classifies exit code {code} as both {previous} and {kind}"
                ));
            }
        }
    }
}

/// Render the checked-in catalog audit. The output is intentionally derived
/// from the same decoded specs the validator inspects so command, scope, and
/// granularity changes cannot silently drift from the inventory.
pub fn render_builtin_catalog_markdown(specs: &BTreeMap<String, ToolSpec>) -> String {
    let mut output = String::from(
        "# Built-in deferred workflow audit\n\n<!-- markdownlint-disable MD013 -->\n\n",
    );
    output.push_str(
        "Generated from the embedded Pkl catalog. `explicit` rows use declared deferred workflows; `compatibility` rows are structurally translated from legacy phases by pairing each mutator with the final enabled read-only verifier. This inventory does not claim cross-version real-tool verification: command semantics remain version-dependent unless a limitation says otherwise, and the opt-in real-tool lane must use controlled tool versions. Batch and workspace findings are conservatively attributed to every candidate in the invocation when no diagnostic adapter identifies exact files.\n\n",
    );
    output.push_str("| Built-in | Tool ID | Mode | Checks | Remedies | Check scope | Invocation | Precision / known limitation |\n");
    output.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for (key, spec) in specs {
        let mut audit = audit_tool(spec);
        if let (WorkspaceFallback::ProjectRoot, Some(indicator)) =
            (spec.workspace_fallback, &spec.workspace_indicator)
        {
            for invocation in &mut audit.invocations {
                invocation.push_str(&format!(
                    " (per {indicator} directory, else the project root)"
                ));
            }
        }
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            cell(key),
            cell(&spec.id),
            audit.mode,
            cell(&audit.checks.join("; ")),
            cell(&audit.remedies.join("; ")),
            cell(&audit.scopes.join("; ")),
            cell(&audit.invocations.join("; ")),
            cell(&audit.limitation),
        ));
    }
    output
}

struct ToolAudit {
    mode: &'static str,
    checks: Vec<String>,
    remedies: Vec<String>,
    scopes: Vec<String>,
    invocations: Vec<String>,
    limitation: String,
}

fn audit_tool(spec: &ToolSpec) -> ToolAudit {
    if !spec.enabled {
        return ToolAudit {
            mode: "disabled",
            checks: vec!["—".into()],
            remedies: vec!["—".into()],
            scopes: vec!["—".into()],
            invocations: vec!["—".into()],
            limitation: "Disabled draft; no deferred support is claimed.".into(),
        };
    }
    if !spec.workflows.is_empty() {
        let workflows = ordered_workflows(spec)
            .into_iter()
            .filter(|(_, workflow)| workflow.enabled)
            .collect::<Vec<_>>();
        return ToolAudit {
            mode: "explicit",
            checks: workflows
                .iter()
                .filter_map(|(id, workflow)| {
                    workflow
                        .check
                        .as_ref()
                        .map(|command| format!("{id}: {}", workflow_command(spec, command)))
                })
                .collect(),
            remedies: nonempty_or_dash(
                workflows
                    .iter()
                    .filter_map(|(id, workflow)| {
                        workflow
                            .remedy
                            .as_ref()
                            .map(|command| format!("{id}: {}", workflow_command(spec, command)))
                    })
                    .collect(),
            ),
            scopes: workflows
                .iter()
                .map(|(id, workflow)| format!("{id}: {}", check_scope(workflow.check_scope)))
                .collect(),
            invocations: workflows
                .iter()
                .map(|(id, workflow)| format!("{id}: {}", invocation(workflow.invocation)))
                .collect(),
            limitation: explicit_limitation(spec),
        };
    }

    let phases = ordered_phases(spec)
        .into_iter()
        .filter(|(_, phase)| phase.enabled)
        .collect::<Vec<_>>();
    let verifier = phases
        .iter()
        .rev()
        .find(|(_, phase)| is_verifier(phase.mode));
    let mutators = phases
        .iter()
        .filter(|(_, phase)| !is_verifier(phase.mode))
        .collect::<Vec<_>>();
    let checks = if mutators.is_empty() {
        phases
            .iter()
            .filter(|(_, phase)| is_verifier(phase.mode))
            .map(|(id, phase)| format!("{id}: {}", phase_command(spec, phase)))
            .collect()
    } else {
        verifier
            .map(|(id, phase)| vec![format!("{id}: {}", phase_command(spec, phase))])
            .unwrap_or_else(|| vec!["—".into()])
    };
    let remedies = nonempty_or_dash(
        mutators
            .iter()
            .map(|(id, phase)| format!("{id}: {}", phase_command(spec, phase)))
            .collect(),
    );
    let scopes = if mutators.is_empty() {
        vec![if spec.workspace_indicator.is_some() {
            "workspace".into()
        } else {
            "target-files".into()
        }]
    } else {
        mutators
            .iter()
            .map(|(id, phase)| format!("{id}: {}", compatibility_scope(spec, phase)))
            .collect()
    };
    ToolAudit {
        mode: "compatibility",
        checks,
        remedies,
        scopes,
        invocations: vec![invocation(spec.phase_invocation).into()],
        limitation: if spec.id == "jq" {
            "The per-file parse check accepts an empty stream and multiple whitespace-separated top-level JSON values; exact-one-document validation is not claimed.".into()
        } else if mutators.is_empty() {
            if spec.phase_invocation == InvocationGranularity::Batch {
                "Read-only checks are compatibility-translated as batched invocations; real-tool behavior is version-dependent.".into()
            } else {
                format!(
                    "Read-only checks are compatibility-translated with {} invocation granularity; real-tool behavior is version-dependent.",
                    invocation(spec.phase_invocation)
                )
            }
        } else if let Some(reason) = &spec.unverified_remedy_fallback {
            format!("Unverified mutator-first fallback: {reason}")
        } else {
            "Each remedy is compatibility-paired with the final read-only phase; real-tool behavior is version-dependent.".into()
        },
    }
}

fn explicit_limitation(spec: &ToolSpec) -> String {
    match spec.id.as_str() {
        "go-fmt" | "gofumpt" | "goimports" => {
            "Read-only list mode reports dirty files through stdout with exit 0; parse failures depend on the installed tool version.".into()
        }
        "golines" => {
            "Read-only dry-run diffs are detected through stdout; the original upstream is archived and installed-version behavior may vary.".into()
        }
        "gomod-tidy" => {
            "Requires a Go release with `go mod tidy -diff`; exit 1 is treated as source issues and may be ambiguous with some command failures.".into()
        }
        "yq" => {
            "Per-file check requires POSIX `sh`, `mktemp`, and `diff`; formatting behavior depends on the installed yq version.".into()
        }
        "ruff" => {
            "Lint remedies precede format remedies; a lint fix that dirties an initially clean format check makes the runner rerun that check and format before the final verification.".into()
        }
        _ => "Explicit checks are structurally validated; real-tool behavior is version-dependent.".into(),
    }
}

fn ordered_workflows(spec: &ToolSpec) -> Vec<(&String, &crate::schema::Workflow)> {
    let mut seen = BTreeSet::new();
    let mut workflows = Vec::new();
    for id in &spec.workflow_order {
        if let Some(workflow) = spec.workflows.get(id) {
            if seen.insert(id.as_str()) {
                workflows.push((id, workflow));
            }
        }
    }
    workflows.extend(
        spec.workflows
            .iter()
            .filter(|(id, _)| !seen.contains(id.as_str())),
    );
    workflows
}

fn ordered_phases(spec: &ToolSpec) -> Vec<(&String, &Phase)> {
    let mut seen = BTreeSet::new();
    let mut phases = Vec::new();
    for id in &spec.phase_order {
        if let Some(phase) = spec.phases.get(id) {
            if seen.insert(id.as_str()) {
                phases.push((id, phase));
            }
        }
    }
    let mut remaining = spec
        .phases
        .iter()
        .filter(|(id, _)| !seen.contains(id.as_str()))
        .collect::<Vec<_>>();
    remaining.sort_by(|left, right| {
        phase_rank(left.1.mode)
            .cmp(&phase_rank(right.1.mode))
            .then_with(|| left.0.cmp(right.0))
    });
    phases.extend(remaining);
    phases
}

fn phase_rank(mode: PhaseMode) -> u8 {
    match mode {
        PhaseMode::Format => 0,
        PhaseMode::Fix => 1,
        PhaseMode::Verify => 2,
        PhaseMode::CheckOnly => 3,
    }
}

fn is_verifier(mode: PhaseMode) -> bool {
    matches!(mode, PhaseMode::Verify | PhaseMode::CheckOnly)
}

fn workflow_command(spec: &ToolSpec, command: &WorkflowCommand) -> String {
    command_text(
        command.program.as_deref().unwrap_or(&spec.executable),
        &command.argv,
    )
}

fn phase_command(spec: &ToolSpec, phase: &Phase) -> String {
    command_text(
        phase.program.as_deref().unwrap_or(&spec.executable),
        &phase.argv,
    )
}

fn command_text(program: &str, argv: &[ArgvElement]) -> String {
    std::iter::once(program.to_owned())
        .chain(argv.iter().map(argv_text))
        .collect::<Vec<_>>()
        .join(" ")
}

fn argv_text(element: &ArgvElement) -> String {
    match element {
        ArgvElement::Literal(value) => value.replace('`', "'").replace('|', "\\|"),
        ArgvElement::Token(token) => format!(
            "{{{}}}",
            match token {
                ArgToken::Files => "files",
                ArgToken::WorkspaceFiles => "workspace-files",
                ArgToken::Workspace => "workspace",
                ArgToken::WorkspaceIndicator => "workspace-indicator",
                ArgToken::ProjectRoot => "project-root",
                ArgToken::ToolExecutable => "tool-executable",
                ArgToken::ExtraArgs => "extra-args",
            }
        ),
    }
}

fn check_scope(scope: CheckScope) -> &'static str {
    match scope {
        CheckScope::TargetFiles => "target-files",
        CheckScope::Workspace => "workspace",
    }
}

fn invocation(value: InvocationGranularity) -> &'static str {
    match value {
        InvocationGranularity::PerFile => "per-file",
        InvocationGranularity::Batch => "batch",
        InvocationGranularity::Workspace => "workspace",
    }
}

fn compatibility_scope(spec: &ToolSpec, phase: &Phase) -> &'static str {
    if spec.workspace_indicator.is_some()
        && !phase.argv.iter().any(|argument| {
            matches!(
                argument,
                ArgvElement::Token(ArgToken::Files | ArgToken::WorkspaceFiles)
            )
        })
    {
        "workspace"
    } else {
        "target-files"
    }
}

fn nonempty_or_dash(values: Vec<String>) -> Vec<String> {
    if values.is_empty() {
        vec!["—".into()]
    } else {
        values
    }
}

fn cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Phase, PhaseMode};

    fn fallback_spec(indicator: Option<&str>, argv: Vec<ArgvElement>) -> ToolSpec {
        let verify = Phase {
            mode: PhaseMode::Verify,
            argv,
            ..Phase::default()
        };
        ToolSpec {
            id: "tool".into(),
            display_name: "Tool".into(),
            executable: "tool".into(),
            workspace_indicator: indicator.map(str::to_owned),
            workspace_fallback: WorkspaceFallback::ProjectRoot,
            phases: BTreeMap::from([("verify".to_owned(), verify)]),
            ..ToolSpec::default()
        }
    }

    fn fallback_errors(spec: ToolSpec) -> Vec<String> {
        let mut errors = Vec::new();
        validate_workspace_fallback("tool (tool)", &spec, &mut errors);
        errors
    }

    #[test]
    fn project_root_fallback_needs_an_indicator_and_no_marker_token() {
        let files = vec![ArgvElement::Token(ArgToken::Files)];
        assert!(fallback_errors(fallback_spec(Some("package.json"), files.clone())).is_empty());
        assert_eq!(
            fallback_errors(fallback_spec(None, files)),
            vec!["tool (tool): workspaceFallback = \"project-root\" needs a workspaceIndicator"]
        );
        let marker = vec![ArgvElement::Token(ArgToken::WorkspaceIndicator)];
        let errors = fallback_errors(fallback_spec(Some("Cargo.toml"), marker));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("no command may use WorkspaceIndicator"));
    }
}
