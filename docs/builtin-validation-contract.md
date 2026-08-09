# Built-in validation contract

The checked-in
[`manifest.json`](../crates/hookkit-pkl-config/validation/manifest.json)
is the source of truth for what Velvet Glove does and does not claim about
each bundled tool. It is intentionally separate from `ToolSpec`: runtime Pkl
configuration describes how to run a tool, while the validation manifest
describes the evidence required before that behavior is considered supported.
Its wire shape is documented by the adjacent
[`manifest.schema.json`](../crates/hookkit-pkl-config/validation/manifest.schema.json)
and enforced by strict Rust decoding plus semantic validation.

The manifest declares every enabled tool and every disabled draft. Validation
compares it with the evaluated Pkl catalog and the fixture tree in both
directions, so a new builtin, duplicate ID, stale declaration, or orphan
fixture cannot silently change the coverage denominator.

## Surfaces, targets, and capabilities

An enabled declaration has an immediate surface backed by its phases and a
deferred surface backed either by explicit workflows or the compatibility
translation. Disabled drafts declare neither surface and set both members of
`contracts` to `null`.

Contracts are surface-specific. Immediate and deferred behavior can have
different capabilities, cases, programs, arguments, and invocation shapes, so
there is deliberately no tool-wide `capabilities` or `requiredCases` field.
Each non-null surface contract contains independently validated command
targets:

- The immediate surface has one `immediate-pipeline` target. Its `commands`
  array is the complete ordered enabled phase pipeline; an individual phase is
  not a separate evidence target.
- The deferred surface has one target per effective workflow, in effective
  execution order. A target is either an `explicit-workflow` from the catalog
  or a `compatibility-workflow` produced by the phase-to-workflow translation.

This target boundary is atomic. Evidence for one workflow does not cover
another workflow, and evidence for one phase does not establish that the full
immediate pipeline was rendered or executed in the right order.

Capabilities and minimum cases are derived for every target. Each surface also
has an `orchestrationCases` contract for behavior spanning its targets. The
surface-level `capabilities` field is the exact union of target capabilities;
`requiredCases` is the exact union of target cases and orchestration cases.
These rollups make surface reporting useful and are checked for drift.
Capabilities are composable: a target can be a checker and mutator, use stdout
to signal issues, operate at workspace scope, and use a batch, per-file, or
workspace invocation shape.

Every target requires `command-coverage`, `clean`, and `operational-failure`
cases. Capabilities add the following cases:

| Capability | Additional required cases |
| --- | --- |
| Checker | Representative source issue and no unexpected mutation |
| Mutator | Expected mutation, complete workspace diff, and idempotence |
| Checker and mutator | Authoritative read-only verification after mutation |
| Stdout-signaled | Empty stdout on a clean exit and nonempty stdout classified as issues on a clean exit |
| Workspace-scoped | Workspace/root selection and workspace attribution |
| Batch | Multi-file invocation and conservative batch attribution |
| Per-file | A multi-file selection with one correctly rendered invocation per selected file |
| Workspace invocation | Multi-file workspace execution, workspace selection, and workspace attribution |

The immediate surface requires `immediate-phase-order`: phases run in catalog
order and stop under the runner's failure semantics. The deferred surface
requires `deferred-lifecycle`: all initial workflow checks run before remedies,
a dirty workflow receives at most one conditional remedy, and every check
invalidated by a remedy is run again. This explicitly does not claim that each
command runs exactly once.

The manifest must list the complete derived target and surface case sets.
Removing a required case does not reduce the contract; it makes validation
fail.

## Exact command and execution context

A target records the context needed to identify the invocation contract:
target ID and kind, execution order, invocation granularity, optional check
scope, working-directory policy, optional workspace indicator, and the exact
tool-local include and exclude globs. These fields are derived from the
evaluated catalog, not maintained as an independent approximation. Runtime
configuration can prepend global exclusion globs; those user-selected settings
are outside the builtin contract and must be explicit in rendered-command test
inputs.

Each entry in a target's ordered `commands` array records its stable command
ID, phase/workflow role, mode, optional reused immediate phase, resolved
program, typed argument template, exit-code classification, stdout issue
semantics, write scope, and `ExtraArgs` expansion. The complete signature and
context are part of rendered-command coverage. A test that merely starts the
right executable, or validates only one command in a multi-command target,
does not satisfy the contract.

## Evidence tiers

Evidence is recorded separately for three tiers:

1. **Schema** proves that the Pkl catalog evaluates, deserializes, and satisfies
   the structural catalog and manifest invariants.
2. **Rendered command** proves the exact executable, arguments, working
   directory, environment, and invocation count produced by Velvet Glove.
3. **Pinned real tool** proves classification and mutations against a recorded
   tool/runtime version in a controlled environment.

Schema evidence never implies either command-rendering or real-tool evidence.
Likewise, the presence of a legacy fixture does not promote either tier. The
generated
[`builtin-validation-coverage.md`](builtin-validation-coverage.md) keeps the
three totals separate.

For each enabled tool, every required execution cell must resolve exactly once
at the rendered-command and pinned-real-tool tiers. Target cases are keyed by
evidence tier, surface, target ID, and case; surface orchestration cases are
keyed by tier, surface, and case. An execution evidence record names exactly
one surface. Its `targets` and `cases` arrays expand to target cells, while its
`surfaceCases` array resolves orchestration cells. Schema evidence is instead
keyed to the `catalog` surface and names no execution target or case. A
resolution is one of:

- covered evidence with one or more stable references;
- an explicit gap with a reason and tracking issue; or
- a narrowly scoped, owned exception with a reason, tracking issue, and expiry.

There is no `skip` evidence state. The coverage report rolls execution state up
separately for immediate and deferred surfaces, retaining both surface
orchestration-case summaries and target summaries with per-case states, so a
covered scope cannot hide a gap in another scope.

Exceptions use the same target-case and surface-case coordinates. An exception
expires at the beginning of its `expiresOn` date and duplicate exception IDs
are rejected across the whole manifest. Expiry is enforced by the required
catalog-validation test using the current UTC date in CI; it is not a runtime
tool-execution or configuration-loading gate.

## Dependencies, provenance, and constraints

Every declaration names its provisioning group and primary executable.
`programOverrides` is the exact set of non-primary programs selected directly
by phase or workflow command overrides. `wrapperExecutables` covers additional
programs invoked inside opaque shell wrappers, which cannot be derived from the
typed command model and therefore must be declared and reviewed manually. A
wrapper executable must not duplicate the primary executable or a direct
program override.

Each declaration also references the catalog source and records upstream tool
provenance as either reviewed version/install information or an explicit gap.
Covered pinned-real-tool evidence is gated on recorded upstream provenance; a
covered record paired with a provenance gap is rejected, and the tier remains
a reported gap until the provenance is recorded. Platform, architecture, and
case-time network policy are always declared. They describe where a future
pinned contract may run, not evidence that it has already passed there.

## Fixture inventory

`fixtureCases` records the existing
`tests/tool-fixtures/<tool-id>/<case-id>` directories. A required, nonignored
test compares these declarations with the filesystem. Declared-but-missing and
undeclared fixture tools or cases are errors.

The current fixtures remain useful inputs to the opt-in compatibility lane,
but they are host-`PATH` dependent and unpinned. Until probe-backed and pinned
lanes attach evidence to them, the manifest records the corresponding contract
cells as gaps.

## Updating the catalog

When adding or changing a builtin:

1. update its Pkl spec and manifest declaration together;
2. update the exact per-surface targets, command signatures, capabilities,
   target cases, orchestration cases, and any fixture case IDs;
3. attach evidence to each target-case and surface-case cell at both execution
   tiers, or record an explicit tracked gap;
4. document direct program overrides and opaque wrapper executables, and update
   provenance, platform, architecture, and network requirements;
5. regenerate the coverage report and run the workspace test suite.

The report test rewrites the checked-in report when the update flag is set:

```sh
VELVET_GLOVE_UPDATE_VALIDATION_COVERAGE=1 \
  cargo test -p hookkit-pkl-config --test validation_manifest \
  generated_validation_coverage_is_current -- --exact
```

Use `VELVET_GLOVE_PRINT_VALIDATION_COVERAGE=1` with `-- --nocapture` to print
the same content between explicit markers without changing the file.

The validator reports all inconsistencies in deterministic order so one run
can repair the complete declaration rather than revealing drift one field at a
time.
