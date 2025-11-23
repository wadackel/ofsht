#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn test_add_worktree_from_main_repo() {
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

    // Run ofsht add from main repo
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("feature-test")
        .current_dir(repo_dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("test-repo-worktrees/feature-test"));

    // Verify worktree was created in the expected location
    let worktree_path = temp.path().join("test-repo-worktrees/feature-test");
    assert!(worktree_path.exists());

    temp.close().unwrap();
}

#[test]
fn test_add_worktree_from_existing_worktree() {
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

    // Create first worktree
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("worktree-1")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree1_path = temp.path().join("test-repo-worktrees/worktree-1");
    assert!(worktree1_path.exists());

    // Create second worktree from the first worktree's directory
    // This tests that relative paths are resolved from main repo, not current dir
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("worktree-2")
        .current_dir(&worktree1_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("test-repo-worktrees/worktree-2"));

    // Verify second worktree was created at the same level as the first
    let worktree2_path = temp.path().join("test-repo-worktrees/worktree-2");
    assert!(worktree2_path.exists());

    temp.close().unwrap();
}

#[test]
fn test_add_worktree_outside_git_repo() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Try to run ofsht add outside a git repository
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("should-fail")
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not in a git repository"));

    temp.close().unwrap();
}

#[test]
fn test_ls_command_from_worktree() {
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
        .arg("test-worktree")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_path = temp.path().join("test-repo-worktrees/test-worktree");

    // Run ofsht ls from within the worktree
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("ls")
        .current_dir(&worktree_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("test-worktree"));

    temp.close().unwrap();
}

#[test]
fn test_cd_command() {
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
        .arg("goto-test")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    // Run ofsht cd
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("cd")
        .arg("goto-test")
        .current_dir(repo_dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("test-repo-worktrees/goto-test"));

    temp.close().unwrap();
}
