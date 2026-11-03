#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status;
    assert!(status.success(), "git {args:?} failed");
}

fn init_repo(temp: &assert_fs::TempDir) -> PathBuf {
    let repo_dir = temp.child("test-repo");
    repo_dir.create_dir_all().unwrap();
    git(repo_dir.path(), &["init"]);
    git(repo_dir.path(), &["config", "user.name", "Test User"]);
    git(
        repo_dir.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(
        repo_dir.path(),
        &["commit", "--allow-empty", "-m", "Initial commit"],
    );
    repo_dir.path().to_path_buf()
}

fn add_worktree(temp: &assert_fs::TempDir, repo: &Path, branch: &str) -> PathBuf {
    Command::cargo_bin("ofsht")
        .unwrap()
        .arg("add")
        .arg(branch)
        .current_dir(repo)
        .assert()
        .success();
    let path = temp.path().join(format!("test-repo-worktrees/{branch}"));
    assert!(path.exists());
    path
}

fn add_unmerged_commit(worktree: &Path) {
    git(
        worktree,
        &["commit", "--allow-empty", "-m", "Unmerged work"],
    );
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    let output = Command::new("git")
        .args(["branch", "--list", branch])
        .current_dir(repo)
        .output()
        .unwrap();
    !String::from_utf8_lossy(&output.stdout).trim().is_empty()
}

#[test]
fn test_rm_keeps_unmerged_branch_by_default() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo = init_repo(&temp);
    let worktree = add_worktree(&temp, &repo, "feature-x");
    add_unmerged_commit(&worktree);

    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=never", "rm", "feature-x"])
        .current_dir(&repo)
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed feature-x"))
        .stderr(predicate::str::contains(
            "Kept branch: feature-x (not fully merged; use --force-branch to delete)",
        ))
        .stderr(predicate::str::contains("Deleted branch").not());

    assert!(!worktree.exists());
    assert!(branch_exists(&repo, "feature-x"));

    temp.close().unwrap();
}

#[test]
fn test_rm_current_keeps_unmerged_branch_and_prints_only_main_path() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo = init_repo(&temp);
    let worktree = add_worktree(&temp, &repo, "feature-x");
    add_unmerged_commit(&worktree);

    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=never", "rm", "."])
        .current_dir(&worktree)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout:?}");
    assert!(
        stdout.trim_end().ends_with("test-repo"),
        "stdout: {stdout:?}"
    );
    assert!(
        stderr.contains("Kept branch: feature-x"),
        "stderr: {stderr:?}"
    );

    assert!(!worktree.exists());
    assert!(branch_exists(&repo, "feature-x"));

    temp.close().unwrap();
}

#[test]
fn test_rm_force_branch_deletes_unmerged_branch() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo = init_repo(&temp);
    let worktree = add_worktree(&temp, &repo, "feature-x");
    add_unmerged_commit(&worktree);

    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=never", "rm", "--force-branch", "feature-x"])
        .current_dir(&repo)
        .assert()
        .success()
        .stderr(predicate::str::contains("Deleted branch: feature-x"));

    assert!(!worktree.exists());
    assert!(!branch_exists(&repo, "feature-x"));

    temp.close().unwrap();
}

#[test]
fn test_rm_keep_branch_keeps_merged_branch() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo = init_repo(&temp);
    let worktree = add_worktree(&temp, &repo, "feature-x");

    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=never", "rm", "--keep-branch", "feature-x"])
        .current_dir(&repo)
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed feature-x"))
        .stderr(predicate::str::contains("Deleted branch").not())
        .stderr(predicate::str::contains("Kept branch").not());

    assert!(!worktree.exists());
    assert!(branch_exists(&repo, "feature-x"));

    temp.close().unwrap();
}

#[test]
fn test_rm_rejects_keep_branch_with_force_branch() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo = init_repo(&temp);
    let worktree = add_worktree(&temp, &repo, "feature-x");

    Command::cargo_bin("ofsht")
        .unwrap()
        .args([
            "--color=never",
            "rm",
            "--keep-branch",
            "--force-branch",
            "feature-x",
        ])
        .current_dir(&repo)
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));

    assert!(worktree.exists());
    assert!(branch_exists(&repo, "feature-x"));

    temp.close().unwrap();
}
