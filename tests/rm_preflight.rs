#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
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
        std::fs::write(repo.join(".git/info/exclude"), ".ofsht.toml\n").unwrap();
        Self { temp, repo }
    }

    fn ofsht(&self, args: &[&str]) -> Output {
        let home = self.temp.path().join("home");
        Command::cargo_bin("ofsht")
            .unwrap()
            .current_dir(&self.repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("_ZO_DATA_DIR", home.join("zoxide"))
            .args(args)
            .output()
            .unwrap()
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

    fn hook_log(&self) -> PathBuf {
        self.temp.path().join("hook.log")
    }
}

const HOOK_CONFIG: &str = "[hooks.delete]\nrun = [\"echo ran >> ../../hook.log\"]\n";

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn refused_targets_are_all_listed_and_nothing_runs() {
    let fixture = Fixture::new(HOOK_CONFIG);
    let slot_1 = fixture.create("slot-1");
    let slot_2 = fixture.create("slot-2");
    let slot_3 = fixture.create("slot-3");
    std::fs::write(slot_2.join("untracked.txt"), "x").unwrap();
    git(
        &fixture.repo,
        &["worktree", "lock", slot_3.to_str().unwrap()],
    );

    let output = fixture.ofsht(&["--color=never", "rm", "slot-1", "slot-2", "slot-3"]);
    let stderr = stderr(&output);

    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("Cannot remove 2 worktree(s):"), "{stderr}");
    assert!(
        stderr.contains("has modified or untracked files"),
        "{stderr}"
    );
    assert!(
        stderr.contains("slot-3: locked (run git worktree unlock"),
        "{stderr}"
    );
    assert!(slot_1.exists() && slot_2.exists() && slot_3.exists());
    assert!(!fixture.hook_log().exists(), "a delete hook ran");
}

#[test]
fn locked_target_without_directory_is_refused() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");
    let slot_2 = fixture.create("slot-2");
    git(
        &fixture.repo,
        &["worktree", "lock", slot_2.to_str().unwrap()],
    );
    std::fs::remove_dir_all(&slot_2).unwrap();

    let output = fixture.ofsht(&["--color=never", "rm", "slot-1", "slot-2"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("locked"), "{}", stderr(&output));
    assert!(slot_1.exists());
}

#[test]
fn removal_refused_after_preflight_prints_no_removed() {
    let fixture = Fixture::new("[hooks.delete]\nrun = [\"touch late.txt\"]\n");
    let slot_1 = fixture.create("slot-1");

    let output = fixture.ofsht(&["--color=never", "rm", "slot-1"]);
    let stderr = stderr(&output);

    assert!(!output.status.success(), "{stderr}");
    assert!(slot_1.exists());
    assert!(stderr.contains("Removing slot-1"), "{stderr}");
    assert!(
        stderr.contains("contains modified or untracked files"),
        "{stderr}"
    );
    assert!(!stderr.contains("Removed slot-1"), "{stderr}");
}

#[test]
fn non_tty_prints_removing_then_removed() {
    let fixture = Fixture::new(HOOK_CONFIG);
    fixture.create("slot-1");

    let output = fixture.ofsht(&["--color=never", "rm", "slot-1"]);
    let stderr = stderr(&output);

    assert!(output.status.success(), "{stderr}");
    let removing = stderr.find("Removing slot-1").expect(&stderr);
    let removed = stderr.find("Removed slot-1").expect(&stderr);
    assert!(removing < removed, "{stderr}");
    assert!(fixture.hook_log().exists());
}

#[test]
fn force_removes_dirty_worktree() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");
    std::fs::write(slot_1.join("untracked.txt"), "x").unwrap();
    git(
        &slot_1,
        &["commit", "-q", "--allow-empty", "-m", "Unmerged work"],
    );

    let output = fixture.ofsht(&["--color=never", "rm", "--force", "slot-1"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!slot_1.exists());
    assert!(
        stderr(&output).contains("Kept branch: slot-1"),
        "--force must not imply --force-branch"
    );
}

#[test]
fn force_removes_worktree_with_submodule() {
    let fixture = Fixture::new("");
    let lib = fixture.temp.path().join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    git(&lib, &["init", "-q", "-b", "main"]);
    git(
        &lib,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "lib",
        ],
    );
    git(
        &fixture.repo,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            lib.to_str().unwrap(),
            "lib",
        ],
    );
    git(&fixture.repo, &["commit", "-q", "-m", "Add submodule"]);
    let slot_1 = fixture.create("slot-1");
    git(
        &slot_1,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "update",
            "--init",
            "-q",
        ],
    );

    let refused = fixture.ofsht(&["--color=never", "rm", "slot-1"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("contains initialized submodules"),
        "{}",
        stderr(&refused)
    );
    assert!(slot_1.exists());

    // git keeps refusing after a deinit, because the worktree's modules directory remains
    git(&slot_1, &["submodule", "deinit", "-q", "-f", "lib"]);
    let deinitialized = fixture.ofsht(&["--color=never", "rm", "slot-1"]);
    assert!(!deinitialized.status.success());
    assert!(
        stderr(&deinitialized).contains("contains initialized submodules"),
        "{}",
        stderr(&deinitialized)
    );

    let forced = fixture.ofsht(&["--color=never", "rm", "--force", "slot-1"]);
    assert!(forced.status.success(), "{}", stderr(&forced));
    assert!(!slot_1.exists());
}

#[test]
fn force_does_not_unlock() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");
    git(
        &fixture.repo,
        &["worktree", "lock", slot_1.to_str().unwrap()],
    );

    let output = fixture.ofsht(&["--color=never", "rm", "--force", "slot-1"]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("git worktree unlock"),
        "{}",
        stderr(&output)
    );
    assert!(slot_1.exists());
}

#[test]
fn detached_commit_unreachable_from_refs_is_refused() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");
    git(&slot_1, &["switch", "-q", "--detach"]);
    git(
        &slot_1,
        &["commit", "-q", "--allow-empty", "-m", "important work"],
    );

    let refused = fixture.ofsht(&["--color=never", "rm", "slot-1"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("has commits on a detached HEAD that no branch or tag contains"),
        "{}",
        stderr(&refused)
    );
    assert!(slot_1.exists());

    let forced = fixture.ofsht(&["--color=never", "rm", "--force", "slot-1"]);
    assert!(forced.status.success(), "{}", stderr(&forced));
    assert!(!slot_1.exists());
}

#[test]
fn detached_head_reachable_from_a_ref_is_allowed() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");
    git(&slot_1, &["switch", "-q", "--detach", "main"]);

    let output = fixture.ofsht(&["--color=never", "rm", "slot-1"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!slot_1.exists());
}

#[test]
fn paused_rebase_is_refused() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");
    std::fs::write(slot_1.join("f"), "1").unwrap();
    git(&slot_1, &["add", "f"]);
    git(&slot_1, &["commit", "-q", "-m", "one"]);
    std::fs::write(slot_1.join("f"), "2").unwrap();
    git(&slot_1, &["commit", "-q", "-am", "two"]);
    let status = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_SEQUENCE_EDITOR", "sed -i.bak s/^pick/edit/")
        .args(["rebase", "-q", "-i", "HEAD~1"])
        .current_dir(&slot_1)
        .status()
        .unwrap();
    assert!(status.success());

    let output = fixture.ofsht(&["--color=never", "rm", "slot-1"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("rebase in progress"),
        "{}",
        stderr(&output)
    );
    assert!(slot_1.exists());
}

#[test]
fn merge_in_progress_is_refused() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");
    std::fs::write(slot_1.join("f"), "base").unwrap();
    git(&slot_1, &["add", "f"]);
    git(&slot_1, &["commit", "-q", "-m", "base"]);
    git(&slot_1, &["switch", "-q", "-c", "other"]);
    std::fs::write(slot_1.join("f"), "other").unwrap();
    git(&slot_1, &["commit", "-q", "-am", "other"]);
    git(&slot_1, &["switch", "-q", "slot-1"]);
    std::fs::write(slot_1.join("f"), "mine").unwrap();
    git(&slot_1, &["commit", "-q", "-am", "mine"]);
    let _ = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["merge", "-q", "other"])
        .current_dir(&slot_1)
        .output()
        .unwrap();

    let output = fixture.ofsht(&["--color=never", "rm", "slot-1"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("merge in progress"),
        "{}",
        stderr(&output)
    );
    assert!(slot_1.exists());
}

#[test]
fn paused_multi_commit_cherry_pick_is_refused() {
    let fixture = Fixture::new("");
    let slot_1 = fixture.create("slot-1");
    std::fs::write(slot_1.join("f"), "base").unwrap();
    git(&slot_1, &["add", "f"]);
    git(&slot_1, &["commit", "-q", "-m", "base"]);
    git(&slot_1, &["switch", "-q", "-c", "picks"]);
    std::fs::write(slot_1.join("f"), "pick one").unwrap();
    git(&slot_1, &["commit", "-q", "-am", "pick one"]);
    std::fs::write(slot_1.join("g"), "pick two").unwrap();
    git(&slot_1, &["add", "g"]);
    git(&slot_1, &["commit", "-q", "-m", "pick two"]);
    git(&slot_1, &["switch", "-q", "slot-1"]);
    std::fs::write(slot_1.join("f"), "conflict").unwrap();
    git(&slot_1, &["commit", "-q", "-am", "conflict"]);
    let _ = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["cherry-pick", "picks~1", "picks"])
        .current_dir(&slot_1)
        .output()
        .unwrap();
    // Resolve the first pick and commit it; the sequencer still holds the second
    std::fs::write(slot_1.join("f"), "resolved").unwrap();
    git(&slot_1, &["add", "f"]);
    git(&slot_1, &["commit", "-q", "--no-edit"]);

    let output = fixture.ofsht(&["--color=never", "rm", "slot-1"]);
    assert!(!output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("in progress"),
        "{}",
        stderr(&output)
    );
    assert!(slot_1.exists());
}
