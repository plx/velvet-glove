# Agent guide

Velvet Glove runs the formatters and linters the user already has installed,
autofixes quietly, and reports residual issues tersely to the coding agent
(verbosely to the user). Keep that product shape in mind: this tool adapts to
the user's environment; it never bundles, pins, wraps, or verifies the user's
tool binaries.

## Before touching builtin tool specs or fixtures

Read `docs/validation-architecture.md` first. It defines the two-tier
validation contract, per-tool budgets, anti-goals, and the salvage protocol
for the archived v1 attempt (`archive/tool-validation-v1`). Per-tool
validation work should match one of the archetype worked examples and land in
roughly 300 changed lines.

## Guardrails

Mechanical budget tripwires live in
`crates/hookkit-pkl-config/tests/guardrails.rs` and
`crates/velvet-glove/tests/guardrails.rs`. If one fails on your work, the
work is over budget — not the budget. **Stop, leave the guardrail alone, and
open an issue for human review.** Never raise a limit, weaken an assertion,
restructure code to evade a check, or apply the `guardrail-change` label
yourself.

## Checks

Run `just check` before opening a PR. The fixture harness docs are in
`crates/velvet-glove/tests/tool-fixtures/README.md`.
