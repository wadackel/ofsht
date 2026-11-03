#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
    worktree: PathBuf,
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
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("app with spaces");
        let worktree = temp.path().join("pool/slot");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-qb", "main"]);
        git(&repo, &["commit", "--allow-empty", "-qm", "initial"]);
        git(
            &repo,
            &["worktree", "add", "-qb", "slot", worktree.to_str().unwrap()],
        );
        write(
            &temp.path().join("config/ofsht/config.toml"),
            "[integration.zoxide]\nenabled=false\n",
        );
        Self {
            temp,
            repo,
            worktree,
        }
    }
    fn config(&self, action: &str, patterns: &[&str]) {
        write(
            &self.repo.join(".ofsht.toml"),
            &format!(
                "[hooks.create]\n{action}={}\n",
                serde_json::to_string(patterns).unwrap()
            ),
        );
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("ofsht").unwrap();
        cmd.args(args)
            .current_dir(&self.repo)
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        cmd
    }
    fn track(&self, path: &str, content: &str) {
        write(&self.worktree.join(path), content);
        git(&self.worktree, &["add", "--", path]);
        git(&self.worktree, &["commit", "-qm", "track"]);
    }
}

#[test]
fn sync_copy_preserves_edits_and_copies_missing_files() {
    let f = Fixture::new();
    write(&f.repo.join("edited.env"), "source");
    write(&f.worktree.join("edited.env"), "local edit");
    write(&f.repo.join("missing.env"), "new");
    write(&f.repo.join("same.env"), "same");
    write(&f.worktree.join("same.env"), "same");
    let before = std::fs::metadata(f.worktree.join("same.env"))
        .unwrap()
        .modified()
        .unwrap();
    f.config("copy", &["*.env"]);
    let out = f.run(&["sync", "--copy"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("edited.env")).unwrap(),
        "local edit"
    );
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("missing.env")).unwrap(),
        "new"
    );
    assert_eq!(
        std::fs::metadata(f.worktree.join("same.env"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    assert!(
        stderr(&out).contains("Skipped") && stderr(&out).contains("edited.env"),
        "{}",
        stderr(&out)
    );
    assert!(out.stdout.is_empty());
    let overwrite = f.run(&["sync", "--copy", "--overwrite"]);
    assert!(overwrite.status.success(), "{}", stderr(&overwrite));
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("edited.env")).unwrap(),
        "source"
    );
}

#[test]
fn recursive_copy_applies_the_same_preservation_policy() {
    let f = Fixture::new();
    write(&f.repo.join("settings/edited"), "source");
    write(&f.worktree.join("settings/edited"), "local");
    write(&f.repo.join("settings/new"), "new");
    f.config("copy", &["settings"]);
    let out = f.run(&["sync", "--copy"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("settings/edited")).unwrap(),
        "local"
    );
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("settings/new")).unwrap(),
        "new"
    );
    assert!(f.run(&["sync", "--copy", "--overwrite"]).status.success());
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("settings/edited")).unwrap(),
        "source"
    );
}

#[cfg(unix)]
#[test]
fn overwrite_replaces_symlink_and_hardlink_entries_without_mutating_aliases() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let f = Fixture::new();
    let external = f.temp.path().join("external");
    write(&external, "external");
    for name in ["symbolic", "hard"] {
        write(&f.repo.join(name), "source");
    }
    symlink(&external, f.worktree.join("symbolic")).unwrap();
    std::fs::hard_link(&external, f.worktree.join("hard")).unwrap();
    std::fs::set_permissions(f.repo.join("hard"), std::fs::Permissions::from_mode(0o640)).unwrap();
    f.config("copy", &["symbolic", "hard"]);
    let out = f.run(&["sync", "--copy"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.worktree.join("symbolic").is_symlink());
    assert_eq!(std::fs::read_to_string(&external).unwrap(), "external");
    let out = f.run(&["sync", "--copy", "--overwrite"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!f.worktree.join("symbolic").is_symlink());
    for name in ["symbolic", "hard"] {
        assert_eq!(
            std::fs::read_to_string(f.worktree.join(name)).unwrap(),
            "source"
        );
    }
    assert_eq!(std::fs::read_to_string(&external).unwrap(), "external");
    assert_eq!(
        std::fs::metadata(f.worktree.join("hard"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert!(!std::fs::read_dir(&f.worktree).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".ofsht-copy-")));
}

#[cfg(unix)]
#[test]
fn copy_to_leaf_directory_symlink_skips_or_replaces_only_the_link() {
    let f = Fixture::new();
    let outside = f.temp.path().join("outside");
    write(&outside.join("marker"), "external");
    write(&f.repo.join("file"), "source");
    std::os::unix::fs::symlink(&outside, f.worktree.join("file")).unwrap();
    f.config("copy", &["file"]);
    let out = f.run(&["sync", "--copy"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.worktree.join("file").is_symlink());
    assert!(stderr(&out).contains("Skipped"));
    let out = f.run(&["sync", "--copy", "--overwrite"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!f.worktree.join("file").is_symlink());
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("file")).unwrap(),
        "source"
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("marker")).unwrap(),
        "external"
    );
}

#[cfg(unix)]
#[test]
fn sync_copy_and_link_never_traverse_destination_directory_symlinks() {
    use std::os::unix::fs::symlink;
    for action in ["copy", "link"] {
        for pattern in ["settings/file.env", "settings/*.env", "settings"] {
            let f = Fixture::new();
            let outside = f.temp.path().join("outside");
            write(&outside.join("file.env"), "external");
            write(&f.repo.join("settings/file.env"), "source");
            symlink(&outside, f.worktree.join("settings")).unwrap();
            f.config(action, &[pattern]);
            let out = f.run(&[
                "sync",
                if action == "copy" { "--copy" } else { "--link" },
                "--overwrite",
            ]);
            assert!(out.status.success(), "{action}/{pattern}: {}", stderr(&out));
            assert_eq!(
                std::fs::read_to_string(outside.join("file.env")).unwrap(),
                "external"
            );
            if action == "link" && pattern == "settings" {
                assert_eq!(
                    std::fs::read_link(f.worktree.join("settings")).unwrap(),
                    f.repo.join("settings").canonicalize().unwrap()
                );
            } else {
                assert_eq!(
                    std::fs::read_link(f.worktree.join("settings")).unwrap(),
                    outside
                );
                assert!(stderr(&out).contains("Skipped"), "{}", stderr(&out));
            }
        }
    }
}

#[test]
fn sync_rejects_destinations_outside_worktree_and_preserves_type_conflicts() {
    let f = Fixture::new();
    write(&f.temp.path().join("outside"), "outside");
    f.config("copy", &["../outside"]);
    let out = f.run(&["sync", "--copy", "--overwrite"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("outside worktree"),
        "{}",
        stderr(&out)
    );
    write(&f.repo.join("file"), "source");
    std::fs::create_dir(f.worktree.join("file")).unwrap();
    f.config("copy", &["file"]);
    assert!(!f.run(&["sync", "--copy", "--overwrite"]).status.success());
    assert!(f.worktree.join("file").is_dir());
}

#[test]
fn sync_links_skip_tracked_modified_and_staged_deleted_paths() {
    let f = Fixture::new();
    for name in ["clean", "modified", "deleted", "staged"] {
        write(&f.repo.join(name), "source");
    }
    for name in ["clean", "modified", "deleted"] {
        f.track(name, "branch");
    }
    write(&f.worktree.join("modified"), "local edit");
    git(&f.worktree, &["rm", "-q", "deleted"]);
    write(&f.worktree.join("staged"), "staged");
    git(&f.worktree, &["add", "staged"]);
    f.config("link", &["clean", "modified", "deleted", "staged"]);
    let out = f.run(&["sync", "--link"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("clean")).unwrap(),
        "branch"
    );
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("modified")).unwrap(),
        "local edit"
    );
    assert!(!f.worktree.join("deleted").exists());
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("staged")).unwrap(),
        "staged"
    );
    assert!(stderr(&out).contains("tracked"), "{}", stderr(&out));
}

#[cfg(unix)]
#[test]
fn tracked_directories_and_symlinks_are_preserved() {
    let f = Fixture::new();
    f.track("settings/tracked", "branch");
    write(&f.repo.join("settings/source"), "source");
    std::os::unix::fs::symlink("settings/tracked", f.worktree.join("tracked-link")).unwrap();
    git(&f.worktree, &["add", "tracked-link"]);
    git(&f.worktree, &["commit", "-qm", "symlink"]);
    write(&f.repo.join("tracked-link"), "source");
    f.config("link", &["settings", "tracked-link"]);
    let out = f.run(&["sync", "--link"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!f.worktree.join("settings").is_symlink());
    assert_eq!(
        std::fs::read_link(f.worktree.join("tracked-link")).unwrap(),
        PathBuf::from("settings/tracked")
    );
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("settings/tracked")).unwrap(),
        "branch"
    );
}

#[cfg(unix)]
#[test]
fn tracking_uses_literal_paths_and_untracked_conflicts_still_fail() {
    let f = Fixture::new();
    f.track("data/name1", "branch");
    write(&f.repo.join("data/name[1]"), "source");
    f.config("link", &["data/*"]);
    let out = f.run(&["sync", "--link"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.worktree.join("data/name[1]").is_symlink());
    let again = f.run(&["sync", "--link"]);
    assert!(again.status.success());
    assert!(stderr(&again).contains("unchanged"));
    write(&f.repo.join("untracked"), "source");
    write(&f.worktree.join("untracked"), "local");
    f.config("link", &["untracked"]);
    let out = f.run(&["sync", "--link"]);
    assert!(!out.status.success());
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("untracked")).unwrap(),
        "local"
    );
}

#[cfg(unix)]
#[test]
fn link_tracking_handles_detached_and_unborn_worktrees() {
    let f = Fixture::new();
    f.track("tracked", "branch");
    write(&f.repo.join("tracked"), "source");
    f.config("link", &["tracked"]);
    git(&f.worktree, &["switch", "--detach", "-q"]);
    let out = f.run(&["sync", "--link"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(f.worktree.join("tracked")).unwrap(),
        "branch"
    );
    git(&f.worktree, &["checkout", "--orphan", "empty", "-q"]);
    git(&f.worktree, &["rm", "-q", "-rf", "."]);
    let out = f.run(&["sync", "--link"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.worktree.join("tracked").is_symlink());
}

#[cfg(unix)]
#[test]
fn tracking_query_failure_does_not_become_untracked() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    write(&f.repo.join("config"), "source");
    f.config("link", &["config"]);
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real_git = String::from_utf8(real_git.stdout).unwrap();
    let real_git = real_git.trim();
    let bin = f.temp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    write(&bin.join("git"),&format!("#!/bin/sh\nfor arg do\n case \"$arg\" in ls-files|ls-tree) echo tracking-unavailable >&2; exit 128 ;; esac\ndone\nexec '{}' \"$@\"\n",real_git.replace('\'',"'\\''")));
    std::fs::set_permissions(bin.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = f
        .command(&["sync", "--link"])
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("tracking-unavailable"));
    assert!(f.worktree.join("config").symlink_metadata().is_err());
}

#[test]
fn creation_keeps_copy_behavior_and_skips_branch_tracked_links() {
    let f = Fixture::new();
    f.track("tracked", "branch");
    write(&f.repo.join("tracked"), "source");
    f.config("link", &["tracked"]);
    let out = f.run(&["create", "fresh-link", "slot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let root = f.temp.path().join("app with spaces-worktrees");
    assert_eq!(
        std::fs::read_to_string(root.join("fresh-link/tracked")).unwrap(),
        "branch"
    );
    assert!(!root.join("fresh-link/tracked").is_symlink());
    f.config("copy", &["tracked"]);
    let out = f.run(&["create", "fresh-copy", "slot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(root.join("fresh-copy/tracked")).unwrap(),
        "source"
    );
}
