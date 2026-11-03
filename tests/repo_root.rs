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

/// Run git in `dir`, failing the test on error
fn git(dir: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
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

/// ofsht in `dir` with HOME and the global config isolated under `home`
fn ofsht_isolated(dir: &std::path::Path, home: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.current_dir(dir)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("_ZO_DATA_DIR", home.join("zoxide"));
    cmd
}

fn init_with_commit(dir: &std::path::Path, extra: &[&str]) {
    let mut args = vec!["init", "-q", "-b", "main"];
    args.extend_from_slice(extra);
    git(
        dir.parent().unwrap(),
        &[&args[..], &[dir.to_str().unwrap()]].concat(),
    );
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(
        dir,
        &["commit", "-q", "--allow-empty", "-m", "Initial commit"],
    );
}

#[test]
#[allow(clippy::literal_string_with_formatting_args)]
fn separate_git_dir_main_worktree_is_root() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().join("home");
    let git_dir = temp.path().join("gitdir");
    let work_tree = temp.path().join("app");
    init_with_commit(
        &work_tree,
        &["--separate-git-dir", git_dir.to_str().unwrap()],
    );
    std::fs::write(
        work_tree.join(".ofsht.toml"),
        "[worktree]\ndir = \"../{repo}-wt/{branch}\"\n",
    )
    .unwrap();

    ofsht_isolated(&work_tree, &home)
        .args(["create", "slot-1"])
        .assert()
        .success();

    assert!(temp.path().join("app-wt/slot-1").exists());
    assert!(!temp.path().join("gitdir-wt").exists());
}

#[test]
fn separate_git_dir_linked_worktree_reports_unknown_main() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().join("home");
    let git_dir = temp.path().join("gitdir");
    let work_tree = temp.path().join("app");
    init_with_commit(
        &work_tree,
        &["--separate-git-dir", git_dir.to_str().unwrap()],
    );
    let linked = temp.path().join("linked");
    git(
        &work_tree,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );

    ofsht_isolated(&linked, &home)
        .arg("ls")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot locate the main worktree"));
}

#[test]
fn bare_repository_is_rejected() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().join("home");
    let seed = temp.path().join("seed");
    init_with_commit(&seed, &[]);
    let bare = temp.path().join("app.git");
    git(
        temp.path(),
        &[
            "clone",
            "-q",
            "--bare",
            seed.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    let linked = temp.path().join("main-worktree");
    git(
        &bare,
        &["worktree", "add", "-q", linked.to_str().unwrap(), "main"],
    );

    for dir in [&bare, &linked] {
        ofsht_isolated(dir, &home)
            .arg("ls")
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "Bare repositories are not supported",
            ));
    }
}

#[test]
fn init_local_in_bare_repository_fails() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().join("home");
    let seed = temp.path().join("seed");
    init_with_commit(&seed, &[]);
    let bare = temp.path().join("app.git");
    git(
        temp.path(),
        &[
            "clone",
            "-q",
            "--bare",
            seed.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );

    ofsht_isolated(&bare, &home)
        .args(["init", "--local"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Bare repositories are not supported",
        ));
    assert!(!bare.join(".ofsht.toml").exists());
    assert!(!temp.path().join(".ofsht.toml").exists());
}
