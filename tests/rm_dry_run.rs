#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git_output(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let output = git_output(dir, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A `main` repository with `.ofsht.toml`, run with HOME and git's global/system config isolated
struct Fixture {
    temp: TempDir,
    repo: PathBuf,
}

impl Fixture {
    fn new(config: &str) -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("test-repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Test User"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(
            &repo,
            &["commit", "-q", "--allow-empty", "-m", "Initial commit"],
        );
        std::fs::write(repo.join(".ofsht.toml"), config).unwrap();
        git(&repo, &["config", "core.excludesFile", "/dev/null"]);
        std::fs::write(repo.join(".git/info/exclude"), ".ofsht.toml\nseed.txt\n").unwrap();
        Self { temp, repo }
    }

    fn ofsht_in(&self, dir: &Path, args: &[&str]) -> Output {
        let home = self.temp.path().join("home");
        Command::cargo_bin("ofsht")
            .unwrap()
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("_ZO_DATA_DIR", home.join("zoxide"))
            .args(args)
            .output()
            .unwrap()
    }

    fn ofsht(&self, args: &[&str]) -> Output {
        self.ofsht_in(&self.repo, args)
    }

    fn create(&self, name: &str) -> PathBuf {
        let output = self.ofsht(&["create", name]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        self.temp.path().join(format!("test-repo-worktrees/{name}"))
    }

    fn branch_exists(&self, name: &str) -> bool {
        git_output(
            &self.repo,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{name}"),
            ],
        )
        .status
        .success()
    }

    fn hook_log(&self) -> PathBuf {
        self.temp.path().join("hook.log")
    }
}

const HOOK_CONFIG: &str =
    "[hooks.delete]\nrun = [\"echo ran >> ../../hook.log\"]\ncopy = [\"seed.txt\"]\n";

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn dry_run_changes_nothing_and_lists_actions() {
    let fixture = Fixture::new(HOOK_CONFIG);
    std::fs::write(fixture.repo.join("seed.txt"), "seed").unwrap();
    let slot_1 = fixture.create("slot-1");
    git(&slot_1, &["switch", "-q", "-c", "other"]);

    let output = fixture.ofsht(&["--color=never", "rm", "--dry-run", "slot-1"]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert_eq!(stdout(&output), "");
    assert!(err.contains("Would remove"), "{err}");
    assert!(
        err.contains("would run hook: echo ran >> ../../hook.log"),
        "{err}"
    );
    assert!(err.contains("would copy: seed.txt"), "{err}");
    assert!(err.contains("would try git branch -d other"), "{err}");
    assert!(err.contains("would try git branch -d slot-1"), "{err}");

    assert!(slot_1.exists());
    assert!(!slot_1.join("seed.txt").exists());
    assert!(!fixture.hook_log().exists());
    assert!(fixture.branch_exists("other"));
    assert!(fixture.branch_exists("slot-1"));
}

#[test]
fn dry_run_reports_refusals_with_failure() {
    let fixture = Fixture::new(HOOK_CONFIG);
    let slot_1 = fixture.create("slot-1");
    let slot_2 = fixture.create("slot-2");
    std::fs::write(slot_2.join("untracked.txt"), "x").unwrap();

    let output = fixture.ofsht(&["--color=never", "rm", "--dry-run", "slot-1", "slot-2"]);
    let err = stderr(&output);
    assert!(!output.status.success(), "{err}");
    assert!(err.contains("has modified or untracked files"), "{err}");
    assert!(
        err.contains("Nothing would be removed: 1 worktree(s) refused"),
        "{err}"
    );
    assert!(!err.contains("Would remove"), "{err}");
    assert!(slot_1.exists());
    assert!(slot_2.exists());
    assert!(!fixture.hook_log().exists());
}

#[test]
fn dry_run_current_worktree_prints_nothing_on_stdout() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");

    let output = fixture.ofsht_in(&slot_1, &["--color=never", "rm", "--dry-run", "."]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert_eq!(stdout(&output), "");
    assert!(err.contains("Would remove"), "{err}");
    assert!(slot_1.exists());
    assert!(fixture.branch_exists("slot-1"));
}

#[test]
fn dry_run_branch_flags() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");
    git(&slot_1, &["switch", "-q", "-c", "other"]);

    let forced = fixture.ofsht(&[
        "--color=never",
        "rm",
        "--dry-run",
        "--force-branch",
        "slot-1",
    ]);
    let err = stderr(&forced);
    assert!(forced.status.success(), "{err}");
    assert!(err.contains("would delete branch other"), "{err}");
    assert!(err.contains("would try git branch -d slot-1"), "{err}");

    let kept = fixture.ofsht(&[
        "--color=never",
        "rm",
        "--dry-run",
        "--keep-branch",
        "slot-1",
    ]);
    let err = stderr(&kept);
    assert!(kept.status.success(), "{err}");
    assert!(err.contains("would keep branch other"), "{err}");
    assert!(!err.contains("would try"), "{err}");

    assert!(slot_1.exists());
    assert!(fixture.branch_exists("other"));
}

#[test]
fn dry_run_shows_default_branch_kept() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");
    git(&slot_1, &["switch", "-q", "--detach"]);
    git(&fixture.repo, &["switch", "-q", "--detach"]);
    git(&slot_1, &["switch", "-q", "main"]);

    let output = fixture.ofsht(&["--color=never", "rm", "--dry-run", "slot-1"]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(
        err.contains("would keep branch main (default branch)"),
        "{err}"
    );
    assert!(fixture.branch_exists("main"));
}

#[test]
fn dry_run_missing_directory_skips_hooks() {
    let fixture = Fixture::new(HOOK_CONFIG);
    let slot_1 = fixture.create("slot-1");
    std::fs::remove_dir_all(&slot_1).unwrap();

    let output = fixture.ofsht(&["--color=never", "rm", "--dry-run", "slot-1"]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(
        err.contains("directory missing; only the registration"),
        "{err}"
    );
    assert!(!err.contains("would run hook"), "{err}");
}
