//! End-to-end tests for the `tools`, `doctor`, and `init` setup commands.
//!
//! Each test runs the real binary with `PATH` limited to a directory of fake
//! tool executables plus `pkl` and `git`, and `HOME` pointed at a scratch
//! directory, so results do not depend on what the host has installed.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    /// A scratch HOME, project directory, and bin directory holding `fakes`.
    /// Returns `None` (skipping the test) when `pkl` is not installed.
    fn new(fakes: &[&str]) -> Option<Self> {
        let pkl = which("pkl")?;
        let root = std::env::temp_dir().join(format!(
            "vg-onboarding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&root);
        for dir in ["home", "project", "bin"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let sandbox = Self {
            root: root.canonicalize().unwrap(),
        };
        std::os::unix::fs::symlink(pkl, sandbox.bin().join("pkl")).unwrap();
        if let Some(git) = which("git") {
            std::os::unix::fs::symlink(git, sandbox.bin().join("git")).unwrap();
        }
        for fake in fakes {
            sandbox.fake_tool(fake);
        }
        Some(sandbox)
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    fn fake_tool(&self, name: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.bin().join(name);
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.project().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_velvet-glove"))
            .args(args)
            .current_dir(self.project())
            .env_clear()
            .env("PATH", self.bin())
            .env("HOME", self.root.join("home"))
            .env("TMPDIR", &self.root)
            .output()
            .expect("run velvet-glove")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn which(program: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn describe(output: &Output) -> String {
    format!(
        "status: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        stdout(output),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A small polyglot project: Python, Rust, TypeScript with prettier, a
/// Dockerfile, and Go files hidden by `.gitignore`.
fn write_polyglot_project(sandbox: &Sandbox) {
    sandbox.write("pyproject.toml", "[project]\nname = \"demo\"\n");
    sandbox.write("app.py", "print('hi')\n");
    sandbox.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    sandbox.write("src/main.rs", "fn main() {}\n");
    sandbox.write(
        "package.json",
        "{\"devDependencies\": {\"prettier\": \"^3\"}}\n",
    );
    sandbox.write(".prettierrc", "{}\n");
    sandbox.write("web/index.ts", "export const answer = 42;\n");
    sandbox.write("Dockerfile", "FROM scratch\n");
    sandbox.write(".gitignore", "ignored/\n");
    sandbox.write("ignored/go.mod", "module ignored\n");
    sandbox.write("ignored/main.go", "package main\n");
}

const POLYGLOT_FAKES: &[&str] = &["ruff", "cargo", "prettier", "flake8", "gofmt", "go"];
const POLYGLOT_SELECTION: &[&str] = &["cargoClippy", "cargoFmt", "prettier", "ruff"];

fn assert_polyglot_policy(sandbox: &Sandbox) {
    let policy = sandbox.project().join(".velvet-glove/post-tool-use.pkl");
    let loaded = hookkit_pkl_config::load_explicit(&policy, &sandbox.project())
        .expect("generated policy evaluates with pkl");
    assert_eq!(loaded.config.run, POLYGLOT_SELECTION);
    for key in POLYGLOT_SELECTION {
        assert!(loaded.config.tools.contains_key(*key), "{key} is defined");
    }

    let text = fs::read_to_string(&policy).unwrap();
    // Why each tool was chosen.
    assert!(
        text.contains("// Ruff: default python-lint tool;"),
        "{text}"
    );
    assert!(text.contains("// cargo fmt: found Cargo.toml;"), "{text}");
    assert!(text.contains("// Prettier: found .prettierrc;"), "{text}");
    // Installed alternatives and wanted-but-missing tools stay commented out.
    assert!(text.contains("// [\"flake8\"] = Builtins.flake8  // alternative to ruff"));
    assert!(text.contains("// [\"hadolint\"] = Builtins.hadolint  // hadolint not found on PATH"));
    // Ignored Go files never make gofmt a candidate.
    assert!(!text.contains("Builtins.goFmt"), "{text}");
    assert!(text.contains("velvet-glove doctor"));
}

#[test]
fn init_selects_fitting_tools_by_walking_the_project() {
    let Some(sandbox) = Sandbox::new(POLYGLOT_FAKES) else {
        eprintln!("skipping: pkl is not on PATH");
        return;
    };
    write_polyglot_project(&sandbox);

    let output = sandbox.run(&["init"]);
    assert!(output.status.success(), "{}", describe(&output));
    assert!(stdout(&output).contains("Enabled: cargoClippy, cargoFmt, prettier, ruff."));
    assert_polyglot_policy(&sandbox);
}

#[test]
fn init_uses_git_file_listing_when_available() {
    let Some(sandbox) = Sandbox::new(POLYGLOT_FAKES) else {
        eprintln!("skipping: pkl is not on PATH");
        return;
    };
    if which("git").is_none() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    write_polyglot_project(&sandbox);
    let git_init = Command::new("git")
        .arg("init")
        .arg("--quiet")
        .arg(sandbox.project())
        .output()
        .unwrap();
    assert!(git_init.status.success());

    let project = sandbox.project();
    let output = sandbox.run(&["init", "--dir", project.to_str().unwrap()]);
    assert!(output.status.success(), "{}", describe(&output));
    assert_polyglot_policy(&sandbox);
}

#[test]
fn init_print_force_and_overwrite_protection() {
    let Some(sandbox) = Sandbox::new(&["ruff"]) else {
        eprintln!("skipping: pkl is not on PATH");
        return;
    };
    sandbox.write("app.py", "print('hi')\n");
    let policy = sandbox.project().join(".velvet-glove/post-tool-use.pkl");

    let printed = sandbox.run(&["init", "--print"]);
    assert!(printed.status.success(), "{}", describe(&printed));
    assert!(!policy.exists(), "--print must not write the policy");

    let written = sandbox.run(&["init"]);
    assert!(written.status.success(), "{}", describe(&written));
    assert_eq!(fs::read_to_string(&policy).unwrap(), stdout(&printed));

    fs::write(&policy, "// edited\n").unwrap();
    let refused = sandbox.run(&["init"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--force"));
    assert_eq!(fs::read_to_string(&policy).unwrap(), "// edited\n");

    let forced = sandbox.run(&["init", "--force"]);
    assert!(forced.status.success(), "{}", describe(&forced));
    assert_eq!(fs::read_to_string(&policy).unwrap(), stdout(&printed));
}

#[test]
fn tools_json_lists_the_catalog_with_resolution() {
    let Some(sandbox) = Sandbox::new(&["ruff"]) else {
        eprintln!("skipping: pkl is not on PATH");
        return;
    };
    let output = sandbox.run(&["tools", "--json"]);
    assert!(output.status.success(), "{}", describe(&output));
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    let by_key: BTreeMap<&str, &serde_json::Value> = entries
        .iter()
        .map(|entry| (entry["key"].as_str().unwrap(), entry))
        .collect();

    let catalog = hookkit_pkl_config::builtin_specs().unwrap();
    assert_eq!(by_key.len(), catalog.len());

    let ruff = by_key["ruff"];
    assert_eq!(ruff["id"], "ruff");
    assert_eq!(ruff["enabled"], true);
    assert_eq!(ruff["resolution"]["status"], "found");
    let ruff_path = sandbox.bin().join("ruff");
    assert_eq!(ruff["resolution"]["path"], ruff_path.to_str().unwrap());
    assert_eq!(ruff["detect"]["role"], "python-lint");

    let cargo_fmt = by_key["cargoFmt"];
    assert_eq!(cargo_fmt["id"], "cargo-fmt");
    assert_eq!(cargo_fmt["resolution"]["status"], "missing");

    let table = sandbox.run(&["tools"]);
    assert!(table.status.success());
    assert!(stdout(&table).starts_with("KEY"));
}

#[test]
fn doctor_reports_setup_and_fails_on_hard_problems() {
    let Some(sandbox) = Sandbox::new(&["ruff"]) else {
        eprintln!("skipping: pkl is not on PATH");
        return;
    };
    sandbox.write("app.py", "print('hi')\n");

    let empty = sandbox.run(&["doctor"]);
    assert!(empty.status.success(), "{}", describe(&empty));
    assert!(
        stdout(&empty).contains("no configuration found"),
        "{}",
        describe(&empty)
    );

    assert!(sandbox.run(&["init"]).status.success());
    let healthy = sandbox.run(&["doctor"]);
    assert!(healthy.status.success(), "{}", describe(&healthy));
    let text = stdout(&healthy);
    assert!(text.contains("project  "), "{text}");
    assert!(text.contains("Run list (1 tool):"), "{text}");
    assert!(text.contains("ruff -> "), "{text}");
    assert!(text.contains("State directory: "), "{text}");

    sandbox.write(
        ".velvet-glove/post-tool-use.local.pkl",
        "amends \"Config.pkl\"\nimport \"Builtins.pkl\"\ntools { [\"shellcheck\"] = Builtins.shellcheck }\nrun { \"ruff\"; \"shellcheck\"; \"ghost\" }\n",
    );
    let broken = sandbox.run(&["doctor"]);
    assert!(!broken.status.success(), "{}", describe(&broken));
    let text = stdout(&broken);
    assert!(text.contains("local    "), "{text}");
    assert!(text.contains("shellcheck missing"), "{text}");
    assert!(
        text.contains("\"ghost\", which no `tools` entry defines"),
        "{text}"
    );

    sandbox.write(".velvet-glove/post-tool-use.local.pkl", "this is not pkl\n");
    let invalid = sandbox.run(&["doctor"]);
    assert!(!invalid.status.success());
    assert!(stdout(&invalid).contains("the configuration does not load"));

    fs::remove_file(sandbox.bin().join("pkl")).unwrap();
    let no_pkl = sandbox.run(&["doctor"]);
    assert!(!no_pkl.status.success());
    assert!(stdout(&no_pkl).contains("Pkl: not available"));
}

#[test]
fn every_enabled_builtin_declares_detection_metadata() {
    if which("pkl").is_none() {
        eprintln!("skipping: pkl is not on PATH");
        return;
    }
    let catalog = hookkit_pkl_config::builtin_specs().unwrap();
    let mut defaults: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (key, spec) in &catalog {
        if !spec.enabled {
            continue;
        }
        let detect = spec
            .detect
            .as_ref()
            .unwrap_or_else(|| panic!("{key} has no `detect` block"));
        for pattern in &detect.indicators {
            globset::Glob::new(pattern)
                .unwrap_or_else(|error| panic!("{key}: bad indicator {pattern:?}: {error}"));
        }
        if detect.default {
            let role = detect
                .role
                .clone()
                .unwrap_or_else(|| panic!("{key}: a default needs a role"));
            defaults.entry(role).or_default().push(key);
        }
        let wanted_somehow = detect.default
            || !detect.indicators.is_empty()
            || !detect.contains.is_empty()
            || detect.role.is_some()
            || detect.note.is_some();
        assert!(
            wanted_somehow,
            "{key}: explain an empty detect block with a note"
        );
    }
    for (role, keys) in defaults {
        assert_eq!(keys.len(), 1, "role {role} has several defaults: {keys:?}");
    }
}

#[test]
fn setup_commands_do_not_need_a_harness() {
    use clap::Parser as _;
    use velvet_glove::scaffold::cli::Cli;
    for args in [
        &["velvet-glove", "tools", "--json"][..],
        &["velvet-glove", "doctor", "--dir", "."],
        &["velvet-glove", "--config", "p.pkl", "doctor"],
        &["velvet-glove", "init", "--print", "--force"],
    ] {
        let cli = Cli::try_parse_from(args).unwrap_or_else(|error| panic!("{args:?}: {error}"));
        assert!(cli.validate().is_ok(), "{args:?}");
    }
    for args in [
        &["velvet-glove", "post-tool"][..],
        &["velvet-glove", "--harness", "claude", "tools"],
        &["velvet-glove", "--config", "p.pkl", "init"],
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.validate().is_err(), "{args:?} must be rejected");
    }
}
