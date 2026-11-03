#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git_output(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
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

/// A `main` repository with a bare `origin` and a stub `gh` that reports the pull request
/// written to `pr.json`
struct Fixture {
    temp: TempDir,
    repo: PathBuf,
    origin: PathBuf,
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
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        git(
            &repo,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&repo, &["push", "-q", "origin", "main"]);

        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let pr_json = temp.path().join("pr.json");
        let stub = format!(
            "#!/bin/sh\ncase \"$1 $2\" in\n  \"--version \"*) echo 'gh version 2.0.0' ;;\n  \"pr view\") cat '{}' ;;\n  *) exit 1 ;;\nesac\n",
            pr_json.display()
        );
        let gh = bin.join("gh");
        std::fs::write(&gh, stub).unwrap();
        Command::new("chmod").arg("+x").arg(&gh).status().unwrap();
        Self { temp, repo, origin }
    }

    fn pull_request(&self, number: u32, head: &str, fork: bool) {
        std::fs::write(
            self.temp.path().join("pr.json"),
            format!(
                r#"{{"number":{number},"title":"demo","headRefName":"{head}","isCrossRepository":{fork}}}"#
            ),
        )
        .unwrap();
    }

    /// Push a new commit on top of `base` to `dest` on origin, and return its SHA
    fn push_commit(&self, base: &str, dest: &str) -> String {
        git(&self.repo, &["switch", "-q", "--detach", base]);
        git(&self.repo, &["commit", "-q", "--allow-empty", "-m", dest]);
        let sha = git_output(&self.repo, &["rev-parse", "HEAD"]);
        git(
            &self.repo,
            &["push", "-q", "origin", &format!("HEAD:{dest}")],
        );
        git(&self.repo, &["switch", "-q", "main"]);
        sha
    }

    fn ofsht_in(&self, dir: &Path, args: &[&str]) -> Output {
        let home = self.temp.path().join("home");
        let path = format!(
            "{}:{}",
            self.temp.path().join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::cargo_bin("ofsht")
            .unwrap()
            .current_dir(dir)
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("_ZO_DATA_DIR", home.join("zoxide"))
            .env_remove("TMUX")
            .args(args)
            .output()
            .unwrap()
    }

    fn ofsht(&self, args: &[&str]) -> Output {
        self.ofsht_in(&self.repo, args)
    }

    fn worktree(&self, name: &str) -> PathBuf {
        self.temp.path().join(format!("app-worktrees/{name}"))
    }

    fn worktree_count(&self) -> usize {
        git_output(&self.repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count()
    }

    fn branch_exists(&self, name: &str) -> bool {
        !git_output(&self.repo, &["branch", "--list", name]).is_empty()
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn add_pr_fast_forwards_stale_local_branch() {
    let fixture = Fixture::new();
    git(&fixture.repo, &["branch", "pr-branch"]);
    git(
        &fixture.repo,
        &["config", "branch.pr-branch.remote", "origin"],
    );
    git(
        &fixture.repo,
        &["config", "branch.pr-branch.merge", "refs/heads/custom"],
    );
    let head = fixture.push_commit("main", "refs/heads/pr-branch");
    fixture.pull_request(1, "pr-branch", false);

    let output = fixture.ofsht(&["add", "#1"]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(err.contains("Fast-forwarded pr-branch"), "{err}");
    assert_eq!(
        git_output(&fixture.worktree("pr-branch"), &["rev-parse", "HEAD"]),
        head
    );
    assert_eq!(
        git_output(&fixture.repo, &["config", "branch.pr-branch.merge"]),
        "refs/heads/custom"
    );
}

#[test]
fn add_pr_warns_when_local_branch_differs() {
    let fixture = Fixture::new();
    git(&fixture.repo, &["push", "-q", "origin", "main:pr-branch"]);
    git(&fixture.repo, &["switch", "-q", "-c", "pr-branch"]);
    git(
        &fixture.repo,
        &["commit", "-q", "--allow-empty", "-m", "local"],
    );
    let local = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    git(&fixture.repo, &["switch", "-q", "main"]);
    fixture.pull_request(1, "pr-branch", false);

    let output = fixture.ofsht(&["add", "#1"]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(err.contains("differs from the pull request"), "{err}");
    assert_eq!(
        git_output(&fixture.worktree("pr-branch"), &["rev-parse", "HEAD"]),
        local
    );
}

#[test]
fn add_pr_behind_checked_out_branch_warns() {
    let fixture = Fixture::new();
    let other = fixture.temp.path().join("other");
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "pr-branch",
            other.to_str().unwrap(),
        ],
    );
    let before = git_output(&fixture.repo, &["rev-parse", "pr-branch"]);
    std::fs::write(other.join("uncommitted"), "keep me").unwrap();
    fixture.push_commit("main", "refs/heads/pr-branch");
    fixture.pull_request(1, "pr-branch", false);

    let output = fixture.ofsht(&["add", "#1"]);
    let err = stderr(&output);
    assert!(!output.status.success(), "{err}");
    assert!(err.contains("is behind the pull request"), "{err}");
    assert!(err.contains("Use: ofsht cd"), "{err}");
    assert_eq!(
        git_output(&fixture.repo, &["rev-parse", "pr-branch"]),
        before
    );
    assert_eq!(
        std::fs::read_to_string(other.join("uncommitted")).unwrap(),
        "keep me"
    );
}

#[test]
fn add_fork_pr_keeps_owned_prefixed_branch_after_collision_disappears() {
    let fixture = Fixture::new();
    git(&fixture.repo, &["branch", "patch-1"]);
    git(
        &fixture.repo,
        &["push", "-q", "origin", "main:refs/pull/2/head"],
    );
    fixture.pull_request(2, "patch-1", true);
    assert!(fixture.ofsht(&["add", "#2"]).status.success());
    git(&fixture.repo, &["branch", "-d", "patch-1"]);

    let output = fixture.ofsht(&["add", "#2"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("Use: ofsht cd pr-2-patch-1"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fixture.worktree_count(), 2);
    assert!(!fixture.branch_exists("patch-1"));
}

#[test]
fn add_fork_pr_refuses_two_unrelated_candidates() {
    let fixture = Fixture::new();
    for branch in ["patch-1", "pr-2-patch-1"] {
        git(&fixture.repo, &["branch", branch]);
    }
    git(
        &fixture.repo,
        &["push", "-q", "origin", "main:refs/pull/2/head"],
    );
    fixture.pull_request(2, "patch-1", true);
    let output = fixture.ofsht(&["add", "#2"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("Neither branch is associated with PR #2"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fixture.worktree_count(), 1);
}

#[test]
fn add_fork_pr_remembers_creation_before_zoxide_failure() {
    let fixture = Fixture::new();
    let stub = fixture.temp.path().join("bin/zoxide");
    std::fs::write(
        &stub,
        "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\nexit 1\n",
    )
    .unwrap();
    Command::new("chmod").arg("+x").arg(&stub).status().unwrap();
    git(
        &fixture.repo,
        &["push", "-q", "origin", "main:refs/pull/2/head"],
    );
    fixture.pull_request(2, "patch-1", true);
    let first = fixture.ofsht(&["add", "#2"]);
    assert!(!first.status.success());
    assert!(stderr(&first).contains("zoxide add failed"));
    let again = fixture.ofsht(&["add", "#2"]);
    assert!(!again.status.success());
    assert!(
        stderr(&again).contains("Use: ofsht cd patch-1"),
        "{}",
        stderr(&again)
    );
    assert_eq!(fixture.worktree_count(), 2);
}

#[test]
fn add_fork_pr_recreates_removed_worktree_at_updated_head() {
    let fixture = Fixture::new();
    git(
        &fixture.repo,
        &["push", "-q", "origin", "main:refs/pull/2/head"],
    );
    fixture.pull_request(2, "patch-1", true);
    assert!(fixture.ofsht(&["add", "#2"]).status.success());
    git(
        &fixture.repo,
        &[
            "worktree",
            "remove",
            fixture.worktree("patch-1").to_str().unwrap(),
        ],
    );
    let head = fixture.push_commit("main", "refs/pull/2/head");
    let output = fixture.ofsht(&["add", "#2"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        git_output(&fixture.worktree("patch-1"), &["rev-parse", "HEAD"]),
        head
    );
    assert!(!fixture.branch_exists("pr-2-patch-1"));
}

#[test]
fn add_pr_preserves_divergent_branch() {
    let fixture = Fixture::new();
    git(&fixture.repo, &["switch", "-qc", "pr-branch"]);
    git(
        &fixture.repo,
        &["commit", "-q", "--allow-empty", "-m", "local"],
    );
    let local = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    git(&fixture.repo, &["switch", "-q", "main"]);
    fixture.push_commit("main", "refs/heads/pr-branch");
    fixture.pull_request(1, "pr-branch", false);
    let output = fixture.ofsht(&["add", "#1"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("differs from the pull request"));
    assert_eq!(
        git_output(&fixture.worktree("pr-branch"), &["rev-parse", "HEAD"]),
        local
    );
}

#[test]
fn add_pr_new_branch_retains_remote_tracking() {
    let fixture = Fixture::new();
    let head = fixture.push_commit("main", "refs/heads/pr-branch");
    fixture.pull_request(1, "pr-branch", false);
    let output = fixture.ofsht(&["add", "#1"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        git_output(&fixture.worktree("pr-branch"), &["rev-parse", "HEAD"]),
        head
    );
    assert_eq!(
        git_output(&fixture.repo, &["config", "branch.pr-branch.remote"]),
        "origin"
    );
    assert_eq!(
        git_output(&fixture.repo, &["config", "branch.pr-branch.merge"]),
        "refs/heads/pr-branch"
    );
}

#[test]
fn add_fork_pr_reports_marker_failure_and_preserves_worktree() {
    let fixture = Fixture::new();
    git(
        &fixture.repo,
        &["push", "-q", "origin", "main:refs/pull/2/head"],
    );
    fixture.pull_request(2, "patch-1", true);
    let lock = fixture.repo.join(".git/config.lock");
    std::fs::write(&lock, "").unwrap();

    let output = fixture.ofsht(&["add", "#2"]);
    let err = stderr(&output);
    assert!(!output.status.success());
    assert!(err.contains("PR association could not be saved"), "{err}");
    assert!(err.contains("branch.patch-1.ofsht-pr"), "{err}");
    assert!(fixture.worktree("patch-1").exists());
    assert!(output.stdout.is_empty());
    std::fs::remove_file(lock).unwrap();
    git(
        &fixture.repo,
        &["config", "--local", "branch.patch-1.ofsht-pr", "2"],
    );
    let again = fixture.ofsht(&["add", "#2"]);
    assert!(stderr(&again).contains("Use: ofsht cd patch-1"));
    assert_eq!(fixture.worktree_count(), 2);
}

#[test]
fn add_pr_rejects_invalid_remote_branch_names() {
    let fixture = Fixture::new();
    for branch in ["-option", "../escape", "topic:other"] {
        fixture.pull_request(1, branch, false);
        let output = fixture.ofsht(&["add", "#1"]);
        assert!(!output.status.success());
        assert!(stderr(&output).contains("Invalid pull request branch"));
        assert!(output.stdout.is_empty());
        assert_eq!(fixture.worktree_count(), 1);
    }
}

#[test]
fn add_fork_pr_twice_points_to_existing_worktree() {
    let fixture = Fixture::new();
    git(
        &fixture.repo,
        &["push", "-q", "origin", "main:refs/pull/2/head"],
    );
    fixture.pull_request(2, "patch-1", true);

    let first = fixture.ofsht(&["add", "#2"]);
    assert!(first.status.success(), "{}", stderr(&first));
    let count = fixture.worktree_count();

    for _ in 0..2 {
        let again = fixture.ofsht(&["add", "#2"]);
        let err = stderr(&again);
        assert!(!again.status.success(), "{err}");
        assert!(err.contains("Use: ofsht cd patch-1"), "{err}");
    }
    assert_eq!(fixture.worktree_count(), count);
    assert!(!fixture.branch_exists("pr-2-patch-1"));
}

#[test]
fn add_fork_pr_with_unrelated_local_branch_uses_prefixed_name() {
    let fixture = Fixture::new();
    git(&fixture.repo, &["branch", "patch-1"]);
    git(
        &fixture.repo,
        &["push", "-q", "origin", "main:refs/pull/2/head"],
    );
    fixture.pull_request(2, "patch-1", true);

    let first = fixture.ofsht(&["add", "#2"]);
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(fixture.worktree("pr-2-patch-1").exists());
    let count = fixture.worktree_count();

    let again = fixture.ofsht(&["add", "#2"]);
    let err = stderr(&again);
    assert!(!again.status.success(), "{err}");
    assert!(err.contains("Use: ofsht cd pr-2-patch-1"), "{err}");
    assert_eq!(fixture.worktree_count(), count);
}

#[test]
fn add_fork_pr_from_default_branch_name_uses_prefixed_name() {
    for main_checked_out in [true, false] {
        let fixture = Fixture::new();
        let main_before = git_output(&fixture.repo, &["rev-parse", "main"]);
        fixture.push_commit("main", "refs/pull/3/head");
        fixture.pull_request(3, "main", true);
        if !main_checked_out {
            git(&fixture.repo, &["switch", "-q", "--detach"]);
        }

        let output = fixture.ofsht(&["add", "#3"]);
        let err = stderr(&output);
        assert!(output.status.success(), "{main_checked_out}: {err}");
        assert!(fixture.worktree("pr-3-main").exists(), "{err}");
        assert_eq!(
            git_output(&fixture.repo, &["rev-parse", "main"]),
            main_before
        );
    }
}

#[test]
fn add_pr_in_single_branch_clone_creates_branch() {
    let fixture = Fixture::new();
    let head = fixture.push_commit("main", "refs/heads/pr-branch");
    let clone = fixture.temp.path().join("clone");
    git(
        fixture.temp.path(),
        &[
            "clone",
            "-q",
            "--single-branch",
            "--branch",
            "main",
            fixture.origin.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    fixture.pull_request(1, "pr-branch", false);

    let output = fixture.ofsht_in(&clone, &["add", "#1"]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    let worktree = fixture.temp.path().join("clone-worktrees/pr-branch");
    assert_eq!(git_output(&worktree, &["rev-parse", "HEAD"]), head);
}
