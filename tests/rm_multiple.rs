#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn test_rm_multiple_branches() {
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

    // Create two worktrees
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("feature-a")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("feature-b")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_a_path = temp.path().join("test-repo-worktrees/feature-a");
    let worktree_b_path = temp.path().join("test-repo-worktrees/feature-b");
    assert!(worktree_a_path.exists());
    assert!(worktree_b_path.exists());

    // Remove both worktrees with a single command
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("rm")
        .arg("feature-a")
        .arg("feature-b")
        .current_dir(repo_dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed worktree").count(2));

    // Verify both worktrees were removed
    assert!(!worktree_a_path.exists());
    assert!(!worktree_b_path.exists());

    // Verify both branches were deleted
    let output = Command::new("git")
        .args(["branch", "--list", "feature-a"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    let output = Command::new("git")
        .args(["branch", "--list", "feature-b"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    temp.close().unwrap();
}

#[test]
fn test_rm_multiple_branches_from_worktree() {
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

    // Create two worktrees
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("feature-a")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("feature-b")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_a_path = temp.path().join("test-repo-worktrees/feature-a");
    let worktree_b_path = temp.path().join("test-repo-worktrees/feature-b");
    assert!(worktree_a_path.exists());
    assert!(worktree_b_path.exists());

    // Remove both worktrees from inside feature-a
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("rm")
        .arg("feature-a")
        .arg("feature-b")
        .current_dir(&worktree_a_path)
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed worktree").count(2));

    // Verify both worktrees were removed
    assert!(!worktree_a_path.exists());
    assert!(!worktree_b_path.exists());

    // Verify both branches were deleted
    let output = Command::new("git")
        .args(["branch", "--list", "feature-a"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    let output = Command::new("git")
        .args(["branch", "--list", "feature-b"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    temp.close().unwrap();
}

#[test]
fn test_rm_duplicate_targets() {
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
        .arg("feature-a")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_a_path = temp.path().join("test-repo-worktrees/feature-a");
    assert!(worktree_a_path.exists());

    // Try to remove the same worktree twice
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("--color=never")
        .arg("rm")
        .arg("feature-a")
        .arg("feature-a")
        .current_dir(repo_dir.path())
        .assert()
        .success()
        // Should only remove once
        .stderr(predicate::str::contains("Removed worktree").count(1))
        // Should warn about duplicate
        .stderr(predicate::str::contains("Duplicate").or(predicate::str::contains("already")));

    // Verify worktree was removed (only once)
    assert!(!worktree_a_path.exists());

    // Verify branch was deleted
    let output = Command::new("git")
        .args(["branch", "--list", "feature-a"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    temp.close().unwrap();
}

#[test]
fn test_rm_current_with_others() {
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

    // Create two worktrees
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("feature-a")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .arg("feature-b")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_a_path = temp.path().join("test-repo-worktrees/feature-a");
    let worktree_b_path = temp.path().join("test-repo-worktrees/feature-b");
    assert!(worktree_a_path.exists());
    assert!(worktree_b_path.exists());

    // Remove feature-b, current (.), and feature-a from inside feature-a
    // The order tests that . is handled correctly even when mixed with other targets
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("rm")
        .arg("feature-b")
        .arg(".")
        .current_dir(&worktree_a_path)
        .assert()
        .success()
        .stdout(predicate::str::contains(repo_dir.path().to_str().unwrap()))
        .stderr(predicate::str::contains("Removed worktree").count(2));

    // Verify all worktrees were removed
    assert!(!worktree_a_path.exists());
    assert!(!worktree_b_path.exists());

    // Verify all branches were deleted
    let output = Command::new("git")
        .args(["branch", "--list", "feature-a"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    let output = Command::new("git")
        .args(["branch", "--list", "feature-b"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    temp.close().unwrap();
}

#[test]
fn test_rm_alias_and_dot() {
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
        .arg("feature-a")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_a_path = temp.path().join("test-repo-worktrees/feature-a");
    assert!(worktree_a_path.exists());

    // Remove using both alias (feature-a) and current worktree (.)
    // The . should take precedence and be executed last
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("--color=never")
        .arg("rm")
        .arg("feature-a")
        .arg(".")
        .current_dir(&worktree_a_path)
        .assert()
        .success()
        // Should print main repo path for shell integration
        .stdout(predicate::str::contains(repo_dir.path().to_str().unwrap()))
        // Should warn about duplicate
        .stderr(predicate::str::contains("Duplicate"))
        // Should only remove once
        .stderr(predicate::str::contains("Removed worktree").count(1));

    // Verify worktree was removed
    assert!(!worktree_a_path.exists());

    // Verify branch was deleted
    let output = Command::new("git")
        .args(["branch", "--list", "feature-a"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    temp.close().unwrap();
}

#[test]
fn test_rm_with_invalid_target() {
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
        .arg("feature-a")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_a_path = temp.path().join("test-repo-worktrees/feature-a");
    assert!(worktree_a_path.exists());

    // Try to remove feature-a and a non-existent target
    // This should fail and leave feature-a intact
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("rm")
        .arg("feature-a")
        .arg("nonexistent-branch")
        .current_dir(repo_dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Worktree not found: nonexistent-branch",
        ));

    // Verify feature-a was NOT removed (regression test for Phase 2 issue)
    assert!(worktree_a_path.exists());

    // Verify branch still exists using show-ref (more reliable than branch --list)
    let output = Command::new("git")
        .args(["show-ref", "--verify", "refs/heads/feature-a"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Branch feature-a should still exist after failed rm command. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    temp.close().unwrap();
}

#[test]
fn test_rm_prunable_worktree_by_branch_name() {
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
        .arg("test-prunable")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_path = temp.path().join("test-repo-worktrees/test-prunable");
    assert!(worktree_path.exists());

    // Manually delete the worktree directory to make it prunable
    std::fs::remove_dir_all(&worktree_path).unwrap();
    assert!(!worktree_path.exists());

    // Verify it's shown as prunable
    let git_output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert!(git_output.status.success());
    let list_output = String::from_utf8_lossy(&git_output.stdout);
    assert!(list_output.contains("prunable"));
    assert!(list_output.contains("test-prunable"));

    // Remove the prunable worktree by branch name
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("rm")
        .arg("test-prunable")
        .current_dir(repo_dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed worktree"))
        .stderr(predicate::str::contains("test-prunable"));

    // Verify worktree list no longer contains the entry
    let git_output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert!(git_output.status.success());
    let list_output = String::from_utf8_lossy(&git_output.stdout);
    assert!(!list_output.contains("test-prunable"));

    // Verify the branch was deleted
    let output = Command::new("git")
        .args(["branch", "--list", "test-prunable"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    temp.close().unwrap();
}

#[test]
fn test_rm_prunable_worktree_by_absolute_path() {
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
        .arg("test-prunable-path")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_path = temp.path().join("test-repo-worktrees/test-prunable-path");
    assert!(worktree_path.exists());

    // Manually delete the worktree directory to make it prunable
    std::fs::remove_dir_all(&worktree_path).unwrap();
    assert!(!worktree_path.exists());

    // Verify it's shown as prunable
    let git_output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert!(git_output.status.success());
    let list_output = String::from_utf8_lossy(&git_output.stdout);
    assert!(list_output.contains("prunable"));
    assert!(list_output.contains("test-prunable-path"));

    // Remove the prunable worktree by absolute path
    let worktree_path_str = worktree_path.to_str().unwrap();
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("rm")
        .arg(worktree_path_str)
        .current_dir(repo_dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed worktree"))
        .stderr(predicate::str::contains("test-prunable-path"));

    // Verify worktree list no longer contains the entry
    let git_output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert!(git_output.status.success());
    let list_output = String::from_utf8_lossy(&git_output.stdout);
    assert!(!list_output.contains("test-prunable-path"));

    // Verify the branch was deleted
    let output = Command::new("git")
        .args(["branch", "--list", "test-prunable-path"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    temp.close().unwrap();
}

#[test]
fn test_rm_prunable_worktree_by_relative_path() {
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
        .arg("test-prunable-rel")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_path = temp.path().join("test-repo-worktrees/test-prunable-rel");
    assert!(worktree_path.exists());

    // Manually delete the worktree directory to make it prunable
    std::fs::remove_dir_all(&worktree_path).unwrap();
    assert!(!worktree_path.exists());

    // Verify it's shown as prunable
    let git_output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert!(git_output.status.success());
    let list_output = String::from_utf8_lossy(&git_output.stdout);
    assert!(list_output.contains("prunable"));
    assert!(list_output.contains("test-prunable-rel"));

    // Remove the prunable worktree by relative path
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.arg("rm")
        .arg("../test-repo-worktrees/test-prunable-rel")
        .current_dir(repo_dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed worktree"))
        .stderr(predicate::str::contains("test-prunable-rel"));

    // Verify worktree list no longer contains the entry
    let git_output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert!(git_output.status.success());
    let list_output = String::from_utf8_lossy(&git_output.stdout);
    assert!(!list_output.contains("test-prunable-rel"));

    // Verify the branch was deleted
    let output = Command::new("git")
        .args(["branch", "--list", "test-prunable-rel"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "");

    temp.close().unwrap();
}
