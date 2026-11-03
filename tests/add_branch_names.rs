#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git_output(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git(dir: &Path, args: &[&str]) {
    git_output(dir, args);
}

/// A `main` repository with an `origin` remote that has `foo` one commit ahead of `main`
struct Fixture {
    temp: TempDir,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("app");
        let origin = temp.path().join("origin.git");
        std::fs::create_dir_all(&repo).unwrap();
        git(
            temp.path(),
            &["init", "-q", "--bare", origin.to_str().unwrap()],
        );
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Test User"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(
            &repo,
            &["commit", "-q", "--allow-empty", "-m", "Initial commit"],
        );
        git(
            &repo,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&repo, &["switch", "-q", "-c", "foo"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "foo work"]);
        git(&repo, &["push", "-q", "origin", "main", "foo"]);
        git(&repo, &["switch", "-q", "main"]);
        git(&repo, &["branch", "-q", "-D", "foo"]);
        git(&repo, &["fetch", "-q", "origin"]);
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

    fn worktree(&self, name: &str) -> PathBuf {
        self.temp.path().join(format!("app-worktrees/{name}"))
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn add_with_tag_name_creates_branch() {
    let fixture = Fixture::new();
    git(&fixture.repo, &["tag", "v1"]);

    assert_success(&fixture.ofsht(&["create", "v1"]));

    let head = git_output(
        &fixture.worktree("v1"),
        &["rev-parse", "--symbolic-full-name", "HEAD"],
    );
    assert_eq!(head, "refs/heads/v1");
}

#[test]
fn add_with_commit_name_creates_branch() {
    let fixture = Fixture::new();
    let sha = git_output(&fixture.repo, &["rev-parse", "--short", "HEAD"]);

    assert_success(&fixture.ofsht(&["create", &sha]));

    let head = git_output(
        &fixture.worktree(&sha),
        &["rev-parse", "--symbolic-full-name", "HEAD"],
    );
    assert_eq!(head, format!("refs/heads/{sha}"));
}

#[test]
fn add_with_revision_suffix_does_not_create_detached_worktree() {
    let fixture = Fixture::new();
    git(
        &fixture.repo,
        &["commit", "-q", "--allow-empty", "-m", "Second commit"],
    );

    let _ = fixture.ofsht(&["create", "main~1"]);

    let detached = git_output(&fixture.repo, &["worktree", "list", "--porcelain"])
        .lines()
        .any(|line| line == "detached");
    assert!(!detached, "main~1 produced a detached worktree");
}

#[test]
fn add_with_remote_tracking_name_is_refused() {
    let fixture = Fixture::new();

    let output = fixture.ofsht(&["create", "origin/foo"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("use 'ofsht add foo origin/foo'"),
        "{stderr}"
    );
    assert!(!fixture.worktree("origin/foo").exists());
    assert!(git_output(&fixture.repo, &["branch", "--list", "origin/foo"]).is_empty());
}

#[test]
fn add_with_symbolic_remote_ref_is_refused_without_a_suggestion() {
    let fixture = Fixture::new();
    git(&fixture.repo, &["remote", "set-head", "origin", "main"]);

    let output = fixture.ofsht(&["create", "origin/HEAD"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "{stderr}");
    assert!(!stderr.contains("ofsht add HEAD"), "{stderr}");
}

#[test]
fn recommended_command_tracks_the_remote_branch() {
    let fixture = Fixture::new();

    assert_success(&fixture.ofsht(&["create", "foo", "origin/foo"]));

    let worktree = fixture.worktree("foo");
    assert_eq!(
        git_output(&worktree, &["rev-parse", "HEAD"]),
        git_output(&fixture.repo, &["rev-parse", "origin/foo"])
    );
    assert_eq!(
        git_output(&worktree, &["rev-parse", "--abbrev-ref", "@{upstream}"]),
        "origin/foo"
    );
}

fn create_slot_on_feature(fixture: &Fixture) -> PathBuf {
    assert_success(&fixture.ofsht(&["create", "slot-1"]));
    let slot_1 = fixture.worktree("slot-1");
    git(&slot_1, &["switch", "-q", "-c", "feature"]);
    slot_1
}

#[test]
fn add_branch_checked_out_elsewhere_points_to_it() {
    let fixture = Fixture::new();
    create_slot_on_feature(&fixture);

    let output = fixture.ofsht(&["add", "feature"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("Branch feature is already checked out in slot-1. Use: ofsht cd feature"),
        "{stderr}"
    );
    assert!(!stderr.contains("Preparing worktree"), "{stderr}");
    assert!(!fixture.worktree("feature").exists());
}

#[test]
fn create_branch_checked_out_elsewhere_points_to_it() {
    let fixture = Fixture::new();
    create_slot_on_feature(&fixture);

    let output = fixture.ofsht(&["create", "feature"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("Branch feature is already checked out in slot-1. Use: ofsht cd feature"),
        "{stderr}"
    );
    assert!(!fixture.worktree("feature").exists());
}

#[test]
fn add_branch_checked_out_in_main_points_to_main() {
    let fixture = Fixture::new();

    let output = fixture.ofsht(&["add", "main"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("Branch main is already checked out in @. Use: ofsht cd main"),
        "{stderr}"
    );
}

#[test]
fn add_branch_in_missing_worktree_suggests_prune() {
    let fixture = Fixture::new();
    let slot_1 = create_slot_on_feature(&fixture);
    std::fs::remove_dir_all(&slot_1).unwrap();

    let output = fixture.ofsht(&["add", "feature"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("is checked out in a missing worktree at"),
        "{stderr}"
    );
    assert!(stderr.contains("Run: git worktree prune"), "{stderr}");
    assert!(!stderr.contains("unlock"), "{stderr}");
}

#[test]
fn add_branch_in_locked_missing_worktree_suggests_unlock() {
    let fixture = Fixture::new();
    let slot_1 = create_slot_on_feature(&fixture);
    git(
        &fixture.repo,
        &["worktree", "lock", slot_1.to_str().unwrap()],
    );
    std::fs::remove_dir_all(&slot_1).unwrap();

    let output = fixture.ofsht(&["add", "feature"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("Run: git worktree unlock"), "{stderr}");
    assert!(stderr.contains("then git worktree prune"), "{stderr}");
}
