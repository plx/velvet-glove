//! End-to-end tests for `velvet-glove check`: the Stop-time engine applied to
//! explicit or Git-changed files, outside any hook.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fake linter: `check` flags `messy` (fixable) and `FIXME` (not), `fix`
/// rewrites `messy`, and `CRASH` makes the check fail operationally.
const FAKE_LINT: &str = r#"#!/bin/sh
mode=$1; shift
status=0
for f in "$@"; do
  case "$mode" in
    check)
      if grep -q CRASH "$f"; then echo "internal error" >&2; exit 2; fi
      if grep -q FIXME "$f"; then printf '\033[31m%s:1:1: FIXME left\033[0m\n' "$f"; status=1; fi
      if grep -q messy "$f"; then printf '%s: messy\n' "$f"; status=1; fi ;;
    fix)
      sed 's/messy/tidy/' "$f" > "$f.tmp" && mv "$f.tmp" "$f" ;;
  esac
done
exit $status
"#;

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    /// A project with a fake-lint policy, or `None` when Pkl is missing.
    fn new() -> Option<Self> {
        let available = Command::new("pkl")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !available {
            eprintln!("skipping: pkl is not on PATH");
            return None;
        }
        let root = std::env::temp_dir().join(format!(
            "vg-check-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&root);
        for dir in ["home", "tmp", "project/.velvet-glove"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let sandbox = Self {
            root: root.canonicalize().unwrap(),
        };
        let lint = sandbox.root.join("fake-lint");
        fs::write(&lint, FAKE_LINT).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&lint, fs::Permissions::from_mode(0o755)).unwrap();
        }
        sandbox.policy(&lint.to_string_lossy());
        Some(sandbox)
    }

    fn policy(&self, executable: &str) {
        let command = |mode: &str, writes: &str| {
            format!(
                "new WorkflowCommand {{ argv {{ \"{mode}\"; new Files {{}} }}; exitCodes {{ issues {{ 1 }}; failure {{ 2 }} }}{writes} }}"
            )
        };
        let policy = format!(
            r#"amends "Config.pkl"

tools {{
  ["fakeLint"] = new ToolSpec {{
    id = "fake-lint"
    displayName = "FakeLint"
    executable = "{executable}"
    installHint = "install fake-lint"
    files {{ include {{ "**/*.txt" }} }}
    workflows {{
      ["lint"] = new Workflow {{
        check = {}
        remedy = {}
      }}
    }}
  }}
}}
run {{ "fakeLint" }}
"#,
            command("check", ""),
            command("fix", "; writes = \"target-files\""),
        );
        fs::write(
            self.project().join(".velvet-glove/post-tool-use.pkl"),
            policy,
        )
        .unwrap();
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.project().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.project().join(relative)).unwrap()
    }

    fn check(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_velvet-glove"))
            .arg("check")
            .args(args)
            .current_dir(self.project())
            .env("HOME", self.root.join("home"))
            .env("TMPDIR", self.root.join("tmp"))
            .output()
            .expect("run velvet-glove check")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", stdout(output)))
}

fn git(project: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn check_reports_each_file_and_exits_by_worst_outcome() {
    let Some(sandbox) = Sandbox::new() else {
        return;
    };
    sandbox.write("a.txt", "fine\n");
    sandbox.write("b.txt", "messy\n");
    sandbox.write("src/c.txt", "FIXME\n");
    sandbox.write("README.md", "docs\n");

    let output = sandbox.check(&["a.txt", "b.txt", "src/c.txt", "README.md"]);

    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    let text = stdout(&output);
    assert!(text.contains("a.txt: clean\n"), "{text}");
    assert!(text.contains("b.txt: auto-fixed by FakeLint\n"), "{text}");
    assert!(text.contains("src/c.txt: needs manual fixes\n"), "{text}");
    assert!(
        text.contains("\nFakeLint (lint): src/c.txt\n    src/c.txt:1:1: FIXME left\n"),
        "{text}"
    );
    assert!(
        text.contains("No configured tool applies to 1 file: README.md"),
        "{text}"
    );
    assert!(!text.contains('\u{1b}'), "excerpts are ANSI-free: {text}");
    assert!(
        !text.contains(&sandbox.project().to_string_lossy().into_owned()),
        "excerpts are project-relative: {text}"
    );
    assert_eq!(sandbox.read("b.txt"), "tidy\n");
    assert!(
        !sandbox.root.join("tmp/velvet-glove/state").exists(),
        "check never touches hook session state"
    );

    let fixed = sandbox.check(&["--json", "b.txt"]);
    assert_eq!(fixed.status.code(), Some(0));
    assert_eq!(json(&fixed)["status"], "clean", "already fixed above");
    sandbox.write("b.txt", "messy\n");
    let fixed = json(&sandbox.check(&["--json", "b.txt", "a.txt"]));
    assert_eq!(fixed["status"], "auto-fixed");
    assert_eq!(fixed["exitCode"], 0);
    assert_eq!(fixed["files"][0]["path"], "a.txt");
    assert_eq!(fixed["files"][1]["status"], "auto-fixed");
    assert_eq!(fixed["files"][1]["fixedBy"][0], "FakeLint");
    let summary = PathBuf::from(fixed["summaryPath"].as_str().unwrap());
    assert!(summary.is_file(), "{summary:?}");

    let manual = json(&sandbox.check(&["--json", "src"]));
    assert_eq!(manual["status"], "manual");
    assert_eq!(manual["exitCode"], 1);
    assert_eq!(manual["issues"][0]["files"][0], "src/c.txt");
    assert_eq!(manual["issues"][0]["excerpt"], "src/c.txt:1:1: FIXME left");
}

#[test]
fn check_exits_2_on_operational_and_configuration_errors() {
    let Some(sandbox) = Sandbox::new() else {
        return;
    };
    sandbox.write("crash.txt", "CRASH\n");
    sandbox.write("messy.txt", "messy\n");
    let crashed = sandbox.check(&["crash.txt"]);
    assert_eq!(crashed.status.code(), Some(2), "{}", stdout(&crashed));
    let text = stdout(&crashed);
    assert!(
        text.contains("crash.txt: not checked: FakeLint could not run"),
        "{text}"
    );
    assert!(text.contains("Could not run:\n  FakeLint: "), "{text}");

    sandbox.policy("/nonexistent/fake-lint");
    let missing = json(&sandbox.check(&["--json", "messy.txt"]));
    assert_eq!(missing["status"], "operational");
    assert_eq!(missing["exitCode"], 2);
    assert_eq!(missing["problems"][0]["missingTool"], true);
    assert_eq!(missing["problems"][0]["installHint"], "install fake-lint");

    sandbox.write(".velvet-glove/post-tool-use.pkl", "this is not pkl\n");
    let broken = sandbox.check(&["messy.txt"]);
    assert_eq!(broken.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&broken.stderr).starts_with("velvet-glove check: "));
    let broken = json(&sandbox.check(&["--json", "messy.txt"]));
    assert_eq!(broken["status"], "error");

    let absent = sandbox.check(&["absent.txt"]);
    assert_eq!(absent.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&absent.stderr).contains("does not exist"));
}

#[test]
fn check_defaults_to_git_changed_and_untracked_files() {
    let Some(sandbox) = Sandbox::new() else {
        return;
    };
    let project = sandbox.project();
    if !git(&project, &["init", "-q"]) {
        eprintln!("skipping: git is not available");
        return;
    }
    sandbox.write(".gitignore", "ignored/\n");
    sandbox.write("committed.txt", "FIXME but committed\n");
    sandbox.write("edited.txt", "fine\n");
    assert!(git(&project, &["add", "."]));
    assert!(git(
        &project,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-qm",
            "init"
        ]
    ));
    sandbox.write("edited.txt", "messy\n");
    sandbox.write("new/untracked.txt", "fine\n");
    sandbox.write("ignored/skip.txt", "FIXME\n");
    let policy = sandbox.read(".velvet-glove/post-tool-use.pkl");
    sandbox.write(
        ".velvet-glove/post-tool-use.pkl",
        &format!("{policy}// edited\n"),
    );

    let report = json(&sandbox.check(&["--json"]));
    assert!(
        !report.to_string().contains(".velvet-glove"),
        "the policy itself is never a candidate: {report}"
    );

    assert_eq!(report["status"], "auto-fixed", "{report}");
    let paths = report["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"edited.txt".to_owned()), "{paths:?}");
    assert!(paths.contains(&"new/untracked.txt".to_owned()), "{paths:?}");
    assert!(
        !paths.iter().any(|path| path.contains("committed")),
        "{paths:?}"
    );
    assert!(
        !paths.iter().any(|path| path.contains("ignored")),
        "{paths:?}"
    );
}
