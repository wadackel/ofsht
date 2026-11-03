#![allow(deprecated)]
use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("app");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-qb", "main"]);
        git(&repo, &["commit", "--allow-empty", "-qm", "initial"]);
        let f = Self { temp, repo };
        for name in ["one", "two"] {
            f.worktree(&f.path(name), name);
        }
        f.config("printf x >> runs");
        f
    }
    fn path(&self, name: &str) -> PathBuf {
        self.temp.path().join("pool").join(name)
    }
    fn worktree(&self, path: &Path, branch: &str) {
        git(
            &self.repo,
            &["worktree", "add", "-qb", branch, path.to_str().unwrap()],
        );
    }
    fn config(&self, hook: &str) {
        std::fs::write(
            self.repo.join(".ofsht.toml"),
            format!(
                "[worktree]\ndir='../pool/{{branch}}'\n[hooks.create]\nrun=[{}]\n",
                serde_json::to_string(hook).unwrap()
            ),
        )
        .unwrap();
    }
    fn run_in(&self, dir: &Path, args: &[&str]) -> Output {
        Command::cargo_bin("ofsht")
            .unwrap()
            .args(args)
            .current_dir(dir)
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
    }
    fn run(&self, args: &[&str]) -> Output {
        self.run_in(&self.repo, args)
    }
}
fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn no_targets_syncs_all_non_main_worktrees() {
    let f = Fixture::new();
    let out = f.run(&["sync", "--run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    for name in ["one", "two"] {
        assert_eq!(
            std::fs::read_to_string(f.path(name).join("runs")).unwrap(),
            "x"
        );
    }
    assert!(!f.repo.join("runs").exists());
    assert!(out.stdout.is_empty());
}

#[test]
fn explicit_targets_resolve_aliases_and_execute_duplicates_once() {
    let f = Fixture::new();
    git(&f.path("one"), &["switch", "-qc", "feature"]);
    let one = f.path("one");
    let out = f.run(&["sync", "one", "feature", one.to_str().unwrap(), "--run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(std::fs::read_to_string(one.join("runs")).unwrap(), "x");
    assert!(!f.path("two/runs").exists());
    assert!(out.stdout.is_empty());
}

#[test]
fn multiple_targets_include_relative_path_and_current_subdirectory() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.path("one/sub")).unwrap();
    let out = f.run_in(&f.path("one/sub"), &["sync", ".", "../../two", "--run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    for name in ["one", "two"] {
        assert_eq!(
            std::fs::read_to_string(f.path(name).join("runs")).unwrap(),
            "x"
        );
    }
}

#[test]
fn invalid_and_main_targets_fail_before_any_hook() {
    let f = Fixture::new();
    for name in ["missing", "@", "main", f.repo.to_str().unwrap(), "."] {
        let out = f.run(&["sync", "one", name]);
        assert!(!out.status.success(), "{name}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(if name == "missing" {
                "Worktree not found"
            } else {
                "Cannot sync main worktree"
            }),
            "{}",
            stderr(&out)
        );
        assert!(!f.path("one/runs").exists());
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn ambiguous_targets_fail_before_any_hook_even_when_actions_are_empty() {
    let f = Fixture::new();
    for (parent, branch) in [("a", "branch-a"), ("b", "branch-b")] {
        f.worktree(&f.temp.path().join(parent).join("slot"), branch);
    }
    let out = f.run(&["sync", "one", "slot"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("Ambiguous worktree name"),
        "{}",
        stderr(&out)
    );
    assert!(!f.path("one/runs").exists());
    std::fs::write(f.repo.join(".ofsht.toml"), "").unwrap();
    assert!(!f.run(&["sync", "missing"]).status.success());
}

#[test]
fn outcomes_distinguish_failed_skipped_and_successful_worktrees() {
    for color in ["never", "always"] {
        let f = Fixture::new();
        f.worktree(&f.path("gone"), "gone");
        std::fs::remove_dir_all(f.path("gone")).unwrap();
        f.config("printf x >> runs; test \"$OFSHT_WORKTREE_NAME\" != one");
        let out = f.run(&["--color", color, "sync", "--run"]);
        let err = stderr(&out);
        assert!(!out.status.success(), "{err}");
        assert!(err.contains("Failed one"), "{err}");
        assert!(err.contains("Synced two"), "{err}");
        assert!(err.contains("Skipped gone"), "{err}");
        assert!(!err.contains("Synced one"), "{err}");
        assert!(!err.contains("Synced gone"), "{err}");
        assert_eq!(std::fs::read_to_string(f.path("two/runs")).unwrap(), "x");
        assert!(out.stdout.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn registered_name_precedes_a_local_symlink_to_main() {
    let f = Fixture::new();
    std::os::unix::fs::symlink(&f.repo, f.repo.join("one")).unwrap();
    let output = f.run(&["sync", "one", "--run"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(std::fs::read_to_string(f.path("one/runs")).unwrap(), "x");
    assert!(!f.repo.join("runs").exists());
}
