#![allow(deprecated)]

use assert_fs::prelude::*;
use std::process::{Command, Stdio};

/// Initialize a git repository with a single empty commit.
fn init_repo(repo_dir: &assert_fs::fixture::ChildPath) {
    repo_dir.create_dir_all().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
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
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "Initial commit"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
}

#[test]
fn add_reads_branch_from_piped_stdin() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo_dir = temp.child("test-repo");
    init_repo(&repo_dir);

    let mut cmd = assert_cmd::Command::cargo_bin("ofsht").unwrap();
    cmd.arg("add")
        .write_stdin("feat-from-stdin\n")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let worktree_path = temp.path().join("test-repo-worktrees/feat-from-stdin");
    assert!(
        worktree_path.exists(),
        "expected worktree at {worktree_path:?}"
    );
}

#[test]
fn cd_reads_name_from_piped_stdin() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo_dir = temp.child("test-repo");
    init_repo(&repo_dir);

    // Pre-create a worktree to navigate to
    assert_cmd::Command::cargo_bin("ofsht")
        .unwrap()
        .args(["add", "feat-cd"])
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let mut cmd = assert_cmd::Command::cargo_bin("ofsht").unwrap();
    let output = cmd
        .arg("cd")
        .write_stdin("feat-cd\n")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        stdout.contains("test-repo-worktrees/feat-cd"),
        "expected stdout to contain worktree path, got: {stdout}"
    );
}

#[test]
fn rm_reads_multiple_targets_from_piped_stdin() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo_dir = temp.child("test-repo");
    init_repo(&repo_dir);

    for name in ["feat-rm-a", "feat-rm-b"] {
        assert_cmd::Command::cargo_bin("ofsht")
            .unwrap()
            .args(["add", name])
            .current_dir(repo_dir.path())
            .assert()
            .success();
    }

    let worktree_a = temp.path().join("test-repo-worktrees/feat-rm-a");
    let worktree_b = temp.path().join("test-repo-worktrees/feat-rm-b");
    assert!(worktree_a.exists());
    assert!(worktree_b.exists());

    assert_cmd::Command::cargo_bin("ofsht")
        .unwrap()
        .arg("rm")
        .write_stdin("feat-rm-a\nfeat-rm-b\n")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    assert!(!worktree_a.exists());
    assert!(!worktree_b.exists());
}

#[test]
fn add_cli_arg_takes_priority_over_stdin() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo_dir = temp.child("test-repo");
    init_repo(&repo_dir);

    // CLI arg explicit-name should win; stdin "stdin-name" should be ignored.
    assert_cmd::Command::cargo_bin("ofsht")
        .unwrap()
        .args(["add", "explicit-name"])
        .write_stdin("stdin-name\n")
        .current_dir(repo_dir.path())
        .assert()
        .success();

    let cli_path = temp.path().join("test-repo-worktrees/explicit-name");
    let stdin_path = temp.path().join("test-repo-worktrees/stdin-name");
    assert!(cli_path.exists(), "CLI-arg worktree should exist");
    assert!(
        !stdin_path.exists(),
        "stdin-arg worktree must NOT exist when CLI arg is present"
    );
}

#[test]
fn add_with_closed_stdin_errors_with_branch_required_message() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo_dir = temp.child("test-repo");
    init_repo(&repo_dir);

    // Use std::process::Command directly with /dev/null on stdin to ensure
    // is_terminal() returns false but no input is available.
    let bin = assert_cmd::cargo::cargo_bin("ofsht");
    let output = Command::new(bin)
        .arg("add")
        .current_dir(repo_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected non-zero exit, got: {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("branch name required"),
        "expected stderr to contain 'branch name required', got: {stderr}"
    );
}
