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
        self.ofsht_in(&self.repo, args)
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

/// `slot-1` moved to another branch, with the branch `slot-1` deleted: the pool state in #131
fn create_slot_without_its_branch(fixture: &Fixture, name: &str) -> PathBuf {
    assert_success(&fixture.ofsht(&["create", name]));
    let path = git_output(&fixture.repo, &["worktree", "list", "--porcelain"])
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .find(|path| path.ends_with(name))
        .unwrap();
    git(
        &path,
        &[
            "switch",
            "-q",
            "-c",
            &format!("moved-{}", name.replace('/', "-")),
        ],
    );
    git(&fixture.repo, &["branch", "-q", "-D", name]);
    path
}

fn branch_exists(fixture: &Fixture, name: &str) -> bool {
    !git_output(&fixture.repo, &["branch", "--list", name]).is_empty()
}

#[test]
fn create_inside_existing_worktree_is_refused() {
    let fixture = Fixture::new();
    create_slot_without_its_branch(&fixture, "slot-1");

    for command in ["create", "add"] {
        let output = fixture.ofsht(&[command, "slot-1/foo"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(stderr.contains("inside the worktree slot-1"), "{stderr}");
    }
    assert!(!fixture.worktree("slot-1/foo").exists());
    assert!(!branch_exists(&fixture, "slot-1/foo"));
}

#[test]
fn add_containing_existing_worktree_is_refused() {
    let fixture = Fixture::new();
    create_slot_without_its_branch(&fixture, "a/b");

    let output = fixture.ofsht(&["add", "a"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("contains the worktree a/b"), "{stderr}");
    assert!(!branch_exists(&fixture, "a"));
}

#[test]
fn add_at_existing_worktree_path_is_refused() {
    let fixture = Fixture::new();
    create_slot_without_its_branch(&fixture, "slot-1");

    let output = fixture.ofsht(&["add", "slot-1"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("is already the worktree slot-1"),
        "{stderr}"
    );
    assert!(!branch_exists(&fixture, "slot-1"));
}

const IN_REPO_CONFIG: &str = "[worktree]\ndir = \".worktrees/{branch}\"\n";

#[test]
fn worktree_dir_inside_main_still_refuses_nesting() {
    let fixture = Fixture::new();
    std::fs::write(fixture.repo.join(".ofsht.toml"), IN_REPO_CONFIG).unwrap();
    create_slot_without_its_branch(&fixture, "slot-1");

    let output = fixture.ofsht(&["add", "slot-1/foo"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("inside the worktree"), "{stderr}");
    assert!(!fixture.repo.join(".worktrees/slot-1/foo").exists());
}

#[test]
fn worktree_dir_inside_main_is_allowed() {
    let fixture = Fixture::new();
    std::fs::write(fixture.repo.join(".ofsht.toml"), IN_REPO_CONFIG).unwrap();

    assert_success(&fixture.ofsht(&["add", "foo"]));
    assert!(fixture.repo.join(".worktrees/foo").exists());
}

#[test]
fn add_inside_missing_worktree_is_not_nesting() {
    let fixture = Fixture::new();
    let slot_1 = create_slot_without_its_branch(&fixture, "slot-1");
    std::fs::remove_dir_all(&slot_1).unwrap();

    let output = fixture.ofsht(&["add", "slot-1/foo"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("Cannot create"), "{stderr}");
}

/// `slot-1` two commits ahead on `feature`, while the main worktree moved on to another commit
fn diverged_slot(fixture: &Fixture) -> PathBuf {
    assert_success(&fixture.ofsht(&["create", "slot-1"]));
    let slot_1 = fixture.worktree("slot-1");
    git(
        &fixture.repo,
        &["commit", "-q", "--allow-empty", "-m", "main moves"],
    );
    git(&slot_1, &["switch", "-q", "-c", "feature"]);
    git(
        &slot_1,
        &["commit", "-q", "--allow-empty", "-m", "feature work 1"],
    );
    git(
        &slot_1,
        &["commit", "-q", "--allow-empty", "-m", "feature work 2"],
    );
    slot_1
}

fn rev(fixture: &Fixture, rev: &str) -> String {
    git_output(&fixture.repo, &["rev-parse", rev])
}

#[test]
fn create_without_start_point_starts_from_current_worktree() {
    let fixture = Fixture::new();
    let slot_1 = diverged_slot(&fixture);

    assert_success(&fixture.ofsht_in(&slot_1, &["create", "from-here"]));

    assert_eq!(rev(&fixture, "from-here"), rev(&fixture, "feature"));
}

#[test]
fn add_without_start_point_starts_from_current_worktree() {
    let fixture = Fixture::new();
    let slot_1 = diverged_slot(&fixture);

    assert_success(&fixture.ofsht_in(&slot_1, &["add", "from-here"]));

    assert_eq!(rev(&fixture, "from-here"), rev(&fixture, "feature"));
}

#[test]
fn create_from_detached_worktree_starts_from_its_commit() {
    let fixture = Fixture::new();
    let slot_1 = diverged_slot(&fixture);
    git(&slot_1, &["switch", "-q", "--detach"]);
    git(
        &slot_1,
        &["commit", "-q", "--allow-empty", "-m", "detached work"],
    );
    let head = git_output(&slot_1, &["rev-parse", "HEAD"]);

    assert_success(&fixture.ofsht_in(&slot_1, &["create", "d"]));

    assert_eq!(rev(&fixture, "d"), head);
}

#[test]
fn create_with_relative_start_point_uses_current_worktree() {
    let fixture = Fixture::new();
    let slot_1 = diverged_slot(&fixture);

    assert_success(&fixture.ofsht_in(&slot_1, &["create", "z", "HEAD~1"]));

    assert_eq!(rev(&fixture, "z"), rev(&fixture, "feature~1"));
}

#[test]
fn create_from_main_worktree_starts_from_main_head() {
    let fixture = Fixture::new();
    diverged_slot(&fixture);

    assert_success(&fixture.ofsht(&["create", "x"]));

    assert_eq!(rev(&fixture, "x"), rev(&fixture, "main"));
}

#[test]
fn create_with_start_point_is_unchanged() {
    let fixture = Fixture::new();
    let slot_1 = diverged_slot(&fixture);

    assert_success(&fixture.ofsht_in(&slot_1, &["create", "y", "main"]));

    assert_eq!(rev(&fixture, "y"), rev(&fixture, "main"));
}

#[test]
fn create_in_unborn_repository_succeeds() {
    let fixture = Fixture::new();
    let unborn = fixture.temp.path().join("unborn");
    std::fs::create_dir_all(&unborn).unwrap();
    git(&unborn, &["init", "-q", "-b", "main"]);

    assert_success(&fixture.ofsht_in(&unborn, &["create", "x"]));
}

fn upstream(fixture: &Fixture, branch: &str) -> Option<String> {
    let output = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "rev-parse",
            "--abbrev-ref",
            &format!("{branch}@{{upstream}}"),
        ])
        .current_dir(&fixture.repo)
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[test]
fn add_from_remote_start_point_with_another_name_sets_no_upstream() {
    let fixture = Fixture::new();

    assert_success(&fixture.ofsht(&["create", "feature", "origin/main"]));

    assert_eq!(upstream(&fixture, "feature"), None);
}

#[test]
fn explicit_auto_setup_merge_is_respected() {
    let fixture = Fixture::new();
    git(
        &fixture.repo,
        &["config", "branch.autoSetupMerge", "always"],
    );

    assert_success(&fixture.ofsht(&["create", "bar", "origin/main"]));

    assert_eq!(upstream(&fixture, "bar").as_deref(), Some("origin/main"));
}

fn is_detached(path: &Path) -> bool {
    Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["symbolic-ref", "-q", "HEAD"])
        .current_dir(path)
        .status()
        .is_ok_and(|status| !status.success())
}

#[test]
fn create_detach_makes_detached_worktree_without_branch() {
    let fixture = Fixture::new();

    assert_success(&fixture.ofsht(&["create", "--detach", "slot-1", "origin/foo"]));

    let slot_1 = fixture.worktree("slot-1");
    assert!(is_detached(&slot_1));
    assert_eq!(
        git_output(&slot_1, &["rev-parse", "HEAD"]),
        rev(&fixture, "origin/foo")
    );
    assert_ne!(rev(&fixture, "origin/foo"), rev(&fixture, "main"));
    assert!(!branch_exists(&fixture, "slot-1"));
}

#[test]
fn create_detach_with_name_of_checked_out_branch_succeeds() {
    let fixture = Fixture::new();

    assert_success(&fixture.ofsht(&["create", "--detach", "main"]));

    assert!(is_detached(&fixture.worktree("main")));
}

#[test]
fn add_detach_without_start_point_uses_current_worktree_head() {
    let fixture = Fixture::new();
    let slot_1 = diverged_slot(&fixture);

    assert_success(&fixture.ofsht_in(&slot_1, &["add", "--detach", "slot-2"]));

    let slot_2 = fixture.worktree("slot-2");
    assert!(is_detached(&slot_2));
    assert_eq!(
        git_output(&slot_2, &["rev-parse", "HEAD"]),
        rev(&fixture, "feature")
    );
    assert!(!branch_exists(&fixture, "slot-2"));
}

#[test]
fn add_detach_runs_create_hooks() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.repo.join(".ofsht.toml"),
        "[hooks.create]\nrun = [\"touch hooked.txt\"]\n",
    )
    .unwrap();

    assert_success(&fixture.ofsht(&["create", "--detach", "slot-1", "main"]));

    assert!(fixture.worktree("slot-1").join("hooked.txt").exists());
}

#[test]
fn add_detach_rejects_github_reference() {
    let fixture = Fixture::new();

    let output = fixture.ofsht(&["add", "--detach", "#1"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("--detach takes a worktree name"),
        "{stderr}"
    );
}

#[test]
fn create_detach_rejects_invalid_name() {
    let fixture = Fixture::new();
    // A previous checkout, so that `@{-1}` names a branch
    git(&fixture.repo, &["switch", "-q", "-c", "previous"]);
    git(&fixture.repo, &["switch", "-q", "main"]);

    for name in ["", "../x", "/abs", "-x", "@{-1}"] {
        let output = fixture.ofsht(&["create", "--detach", "--", name]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{name:?}: {stderr}");
        assert!(
            stderr.contains("Invalid worktree name"),
            "{name:?}: {stderr}"
        );
    }
    assert!(!fixture.temp.path().join("app-worktrees").exists());
    assert!(!fixture.temp.path().join("x").exists());
}

#[test]
fn add_detach_inside_existing_worktree_is_refused() {
    let fixture = Fixture::new();
    create_slot_without_its_branch(&fixture, "slot-1");

    let output = fixture.ofsht(&["create", "--detach", "slot-1/foo", "main"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("inside the worktree slot-1"), "{stderr}");
}
