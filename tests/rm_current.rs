#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn test_rm_current_from_worktree() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo_dir = temp.child("test-repo");

    // Initialize a git repository
    repo_dir.create_dir_all().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();

    // Configure git user (required for commits in CI)
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();

    // Create initial commit
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "Initial commit"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();

    // Create a worktree
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("feature-test")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_path = temp.path().join("test-repo-worktrees/feature-test");
    assert!(worktree_path.exists());

    // Run ofsht rm . from worktree
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("rm")
        .arg(".")
        .current_dir(&worktree_path)
        .assert()
        .success()
        .stdout(predicate::str::contains(repo_dir.path().to_str().unwrap()))
        .stderr(predicate::str::contains("Removed worktree"));

    // Verify worktree was removed
    assert!(!worktree_path.exists());

    // Verify branch was deleted
    let output = Command::new("git")
        .args(["branch", "--list", "feature-test"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    temp.close().unwrap();
}

#[test]
fn test_rm_current_from_main_worktree() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo_dir = temp.child("test-repo");

    // Initialize a git repository
    repo_dir.create_dir_all().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();

    // Configure git user (required for commits in CI)
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();

    // Create initial commit
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "Initial commit"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();

    // Try to run ofsht rm . from main worktree
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("rm")
        .arg(".")
        .current_dir(repo_dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot remove main worktree"));

    temp.close().unwrap();
}

#[test]
fn test_rm_current_outside_git_repo() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Try to run ofsht rm . outside a git repository
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("rm")
        .arg(".")
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not in a git repository"));

    temp.close().unwrap();
}
