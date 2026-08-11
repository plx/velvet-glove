//! Tripwires enforcing the builtin-spec complexity budgets from
//! `docs/validation-architecture.md`.
//!
//! These limits are intentional. If one fails on work you are doing, the
//! default assumption is that the work is over budget — not that the budget
//! is wrong. Do NOT raise a limit, weaken an assertion, or restructure code
//! to evade a check: stop and open an issue for human review instead.
//! Changes to this file require the `guardrail-change` label (human
//! sign-off) to pass CI.

use std::fs;
use std::path::PathBuf;

const DOC: &str = "docs/validation-architecture.md";

/// Maximum lines for a single builtin tool spec. The pre-reboot baseline
/// maximum was 136 (`ruff.pkl`); the v1 failure mode was 1,900-line specs
/// with embedded adapter programs.
const TOOL_SPEC_LINE_BUDGET: usize = 200;

/// Maximum length of one literal argv token. The longest legitimate token in
/// the catalog is yq's ~230-char `sh -c` diff shim; embedded programs run to
/// tens of kilobytes.
const ARGV_TOKEN_CHAR_BUDGET: usize = 500;

/// Per-tool `program` overrides that differ from the spec's executable.
/// Additions here are guardrail changes and need human sign-off.
const PROGRAM_ALLOWLIST: &[(&str, &str)] = &[
    ("php-cs", "phpcbf"), // fixer binary differs from the checker binary
    ("yq", "sh"),         // one-line diff shim; yq has no native check mode
];

fn builtins_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/builtins")
}

fn builtin_pkl_sources() -> Vec<(PathBuf, String)> {
    fn walk(dir: &PathBuf, out: &mut Vec<(PathBuf, String)>) {
        for entry in fs::read_dir(dir).expect("read builtins dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "pkl") {
                let text = fs::read_to_string(&path).expect("read pkl source");
                out.push((path, text));
            }
        }
    }
    let mut out = Vec::new();
    walk(&builtins_dir(), &mut out);
    assert!(!out.is_empty(), "no builtin pkl sources found");
    out
}

#[test]
fn tool_specs_fit_the_line_budget() {
    for (path, text) in builtin_pkl_sources() {
        if !path.parent().is_some_and(|p| p.ends_with("tools")) {
            continue;
        }
        let lines = text.lines().count();
        assert!(
            lines <= TOOL_SPEC_LINE_BUDGET,
            "{} is {lines} lines (budget: {TOOL_SPEC_LINE_BUDGET}). A spec \
             this large almost certainly embeds logic that belongs in the \
             runner or nowhere. See {DOC}; do not raise this budget — open \
             an issue for human review instead.",
            path.display(),
        );
    }
}

#[test]
fn builtin_specs_contain_no_multiline_strings() {
    for (path, text) in builtin_pkl_sources() {
        assert!(
            !text.contains("\"\"\""),
            "{} contains a multiline string literal. Builtin specs must not \
             embed programs or documents; the v1 attempt used these to embed \
             1,800-line adapter scripts. See {DOC}; do not weaken this check \
             — open an issue for human review instead.",
            path.display(),
        );
    }
}

#[test]
fn builtin_specs_contain_no_pinned_hashes() {
    for (path, text) in builtin_pkl_sources() {
        for (number, line) in text.lines().enumerate() {
            let mut run = 0usize;
            let has_long_hex = line
                .chars()
                .map(|c| {
                    if c.is_ascii_hexdigit() {
                        run += 1;
                    } else {
                        run = 0;
                    }
                    run
                })
                .any(|r| r >= 40);
            assert!(
                !has_long_hex,
                "{}:{} contains a 40+ character hex literal, which looks like \
                 a pinned digest. Shipped specs must not pin binary hashes or \
                 exact builds. See {DOC}; do not weaken this check — open an \
                 issue for human review instead.",
                path.display(),
                number + 1,
            );
        }
    }
}

fn pkl_available() -> bool {
    std::process::Command::new("pkl")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn evaluated_commands_respect_program_and_token_budgets() {
    if !pkl_available() {
        eprintln!("skipping test: pkl binary not on PATH");
        return;
    }
    use hookkit_pkl_config::schema::ArgvElement;

    let specs = hookkit_pkl_config::builtin_specs().expect("evaluate builtins");
    for (name, spec) in &specs {
        let mut commands: Vec<(String, Option<&String>, &[ArgvElement])> = Vec::new();
        for (id, phase) in &spec.phases {
            commands.push((format!("phase {id}"), phase.program.as_ref(), &phase.argv));
        }
        for (id, workflow) in &spec.workflows {
            if let Some(check) = &workflow.check {
                commands.push((
                    format!("workflow {id} check"),
                    check.program.as_ref(),
                    &check.argv,
                ));
            }
            if let Some(remedy) = &workflow.remedy {
                commands.push((
                    format!("workflow {id} remedy"),
                    remedy.program.as_ref(),
                    &remedy.argv,
                ));
            }
        }

        for (context, program, argv) in commands {
            if let Some(program) = program {
                let allowed = program == &spec.executable
                    || PROGRAM_ALLOWLIST
                        .iter()
                        .any(|(id, p)| *id == spec.id && *p == program.as_str());
                assert!(
                    allowed,
                    "{name} ({context}): program override {program:?} is not \
                     the spec executable {:?} and is not in the reviewed \
                     allowlist. Specs must invoke the tool itself, not a \
                     wrapper. See {DOC}; adding an allowlist entry is a \
                     guardrail change requiring human sign-off.",
                    spec.executable,
                );
            }
            for element in argv {
                let ArgvElement::Literal(token) = element else {
                    continue;
                };
                assert!(
                    !token.contains('\n'),
                    "{name} ({context}): argv token contains a newline, which \
                     means an embedded program. See {DOC}; do not weaken this \
                     check — open an issue for human review instead.",
                );
                assert!(
                    token.chars().count() <= ARGV_TOKEN_CHAR_BUDGET,
                    "{name} ({context}): argv token is {} chars (budget: \
                     {ARGV_TOKEN_CHAR_BUDGET}). See {DOC}; do not raise this \
                     budget — open an issue for human review instead.",
                    token.chars().count(),
                );
            }
        }
    }
}
