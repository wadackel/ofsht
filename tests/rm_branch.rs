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

/// A repository on `main`, independent of the machine's `init.defaultBranch`
fn init_main_repo(temp: &assert_fs::TempDir) -> PathBuf {
    let repo_dir = temp.child("test-repo");
    repo_dir.create_dir_all().unwrap();
    git(repo_dir.path(), &["init", "-b", "main"]);
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

/// ofsht with git's global and system config and the user's ofsht config out of reach
fn ofsht_isolated(temp: &assert_fs::TempDir, dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", temp.path().join("home"))
        .env("XDG_CONFIG_HOME", temp.path().join("home/.config"))
        .env("_ZO_DATA_DIR", temp.path().join("home/zoxide"));
    cmd
}

fn create_isolated(temp: &assert_fs::TempDir, repo: &Path, branch: &str) -> PathBuf {
    ofsht_isolated(temp, repo)
        .args(["create", branch])
        .assert()
        .success();
    temp.path().join(format!("test-repo-worktrees/{branch}"))
}

#[test]
fn test_rm_keeps_default_branch_held_by_worktree() {
    for flags in [&[][..], &["--force-branch"][..]] {
        let temp = assert_fs::TempDir::new().unwrap();
        let repo = init_main_repo(&temp);
        let worktree = create_isolated(&temp, &repo, "slot-1");
        git(&repo, &["switch", "-q", "-c", "develop"]);
        git(&worktree, &["switch", "-q", "main"]);

        let mut args = vec!["--color=never", "rm"];
        args.extend_from_slice(flags);
        args.push("slot-1");
        ofsht_isolated(&temp, &repo)
            .args(&args)
            .assert()
            .success()
            .stderr(predicate::str::contains(
                "Kept branch: main (default branch)",
            ));

        assert!(branch_exists(&repo, "main"), "{flags:?}");
        assert!(!worktree.exists());
    }
}

#[test]
fn test_rm_keeps_origin_head_branch() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo = init_main_repo(&temp);
    let worktree = create_isolated(&temp, &repo, "slot-1");
    git(&repo, &["branch", "trunk"]);
    git(&repo, &["update-ref", "refs/remotes/origin/trunk", "HEAD"]);
    git(
        &repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ],
    );
    git(&worktree, &["switch", "-q", "trunk"]);

    ofsht_isolated(&temp, &repo)
        .args(["--color=never", "rm", "slot-1"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Kept branch: trunk (default branch)",
        ));
    assert!(branch_exists(&repo, "trunk"));
}

#[test]
fn test_rm_deletes_merged_creation_branch() {
    let cases: [(&[&str], bool, bool); 4] = [
        // (flags, add an unmerged commit on slot-1, slot-1 expected to remain)
        (&[], false, false),
        (&[], true, true),
        (&["--force-branch"], true, true),
        (&["--keep-branch"], false, true),
    ];
    for (flags, unmerged, remains) in cases {
        let temp = assert_fs::TempDir::new().unwrap();
        let repo = init_main_repo(&temp);
        let worktree = create_isolated(&temp, &repo, "slot-1");
        if unmerged {
            add_unmerged_commit(&worktree);
        }
        git(&worktree, &["switch", "-q", "-c", "other"]);

        let mut args = vec!["--color=never", "rm"];
        args.extend_from_slice(flags);
        args.push("slot-1");
        let output = ofsht_isolated(&temp, &repo).args(&args).output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{flags:?}: {stderr}");

        assert_eq!(
            branch_exists(&repo, "slot-1"),
            remains,
            "{flags:?} {unmerged}: {stderr}"
        );
        assert_eq!(
            stderr.contains("Deleted branch: slot-1"),
            !remains,
            "{flags:?} {unmerged}: {stderr}"
        );
    }
}

#[test]
fn test_rm_creation_branch_with_slash_spares_same_basename() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo = init_main_repo(&temp);
    let worktree = create_isolated(&temp, &repo, "feature/foo");
    git(&repo, &["branch", "foo"]);
    git(&worktree, &["switch", "-q", "-c", "other"]);

    ofsht_isolated(&temp, &repo)
        .args(["--color=never", "rm", "--keep-branch", "feature/foo"])
        .assert()
        .success();
    assert!(
        branch_exists(&repo, "feature/foo"),
        "--keep-branch keeps it"
    );

    let worktree = create_isolated(&temp, &repo, "feature/bar");
    git(&worktree, &["switch", "-q", "-c", "other-2"]);
    git(&repo, &["branch", "bar"]);
    ofsht_isolated(&temp, &repo)
        .args(["--color=never", "rm", "other-2"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Deleted branch: feature/bar"));
    assert!(!branch_exists(&repo, "feature/bar"));
    assert!(branch_exists(&repo, "bar"));
    assert!(branch_exists(&repo, "foo"));
}

#[test]
fn test_rm_creation_branch_of_detached_worktree() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo = init_main_repo(&temp);
    let worktree = create_isolated(&temp, &repo, "slot-1");
    git(&worktree, &["switch", "-q", "--detach"]);

    ofsht_isolated(&temp, &repo)
        .args(["--color=never", "rm", "slot-1"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Deleted branch: slot-1"));
    assert!(!branch_exists(&repo, "slot-1"));
}

#[test]
fn test_rm_creation_branch_checked_out_elsewhere() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo = init_main_repo(&temp);
    let slot_1 = create_isolated(&temp, &repo, "slot-1");
    let slot_2 = create_isolated(&temp, &repo, "slot-2");
    git(&slot_1, &["switch", "-q", "-c", "other"]);
    git(&slot_2, &["switch", "-q", "slot-1"]);

    ofsht_isolated(&temp, &repo)
        .args(["--color=never", "rm", "other"])
        .assert()
        .success();
    assert!(branch_exists(&repo, "slot-1"));
    assert!(slot_2.exists());
}
