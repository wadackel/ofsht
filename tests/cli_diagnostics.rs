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
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let worktree = temp.path().join("pool/slot");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-qb", "main"]);
        git(&repo, &["commit", "--allow-empty", "-qm", "initial"]);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-qb",
                "feature",
                worktree.to_str().unwrap(),
            ],
        );
        let config = temp.path().join("config/ofsht");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("config.toml"),
            "[integration.zoxide]\nenabled=false\n",
        )
        .unwrap();
        Self {
            temp,
            repo,
            worktree,
        }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("ofsht").unwrap();
        cmd.args(args)
            .current_dir(&self.repo)
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("TMUX");
        cmd
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
}
fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn cd_rejects_missing_directories_for_name_and_path() {
    let f = Fixture::new();
    std::fs::remove_dir_all(&f.worktree).unwrap();
    for name in ["feature", "slot", f.worktree.to_str().unwrap()] {
        let out = f.run(&["cd", name]);
        assert!(!out.status.success(), "{name}: {}", stderr(&out));
        assert!(out.stdout.is_empty());
        assert!(
            stderr(&out).contains("directory not found"),
            "{}",
            stderr(&out)
        );
        assert!(stderr(&out).contains("git worktree prune"));
    }
}

#[test]
fn cd_rejects_registered_path_that_is_a_file() {
    let f = Fixture::new();
    std::fs::remove_dir_all(&f.worktree).unwrap();
    std::fs::write(&f.worktree, "preserve").unwrap();
    let out = f.run(&["cd", "feature"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(stderr(&out).contains("not a directory"), "{}", stderr(&out));
    assert_eq!(std::fs::read_to_string(&f.worktree).unwrap(), "preserve");
}

#[test]
fn cd_existing_main_and_linked_paths_keep_exact_stdout() {
    let f = Fixture::new();
    for (name, path) in [
        ("@", &f.repo),
        ("main", &f.repo),
        ("feature", &f.worktree),
        ("slot", &f.worktree),
    ] {
        let out = f.run(&["cd", name]);
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            format!("{}\n", path.canonicalize().unwrap().display())
        );
    }
}

#[test]
fn missing_target_hints_distinguish_free_branch_and_unknown_name() {
    let f = Fixture::new();
    git(&f.repo, &["branch", "free-branch"]);
    for command in ["cd", "rm"] {
        let out = f.run(&[command, "free-branch"]);
        assert!(!out.status.success());
        let err = stderr(&out);
        assert!(err.contains("local branch"), "{err}");
        assert!(err.contains("ofsht add free-branch"), "{err}");
        assert!(out.stdout.is_empty());
        let out = f.run(&[command, "unknown-name"]);
        let err = stderr(&out);
        assert!(!out.status.success());
        assert!(err.contains("Available worktrees"), "{err}");
        assert!(err.contains("slot"), "{err}");
    }
    assert!(f.worktree.exists());
}

#[cfg(unix)]
#[test]
fn suggested_add_command_quotes_shell_metacharacters() {
    let f = Fixture::new();
    let branch = "topic'$(touch${IFS}PWN)";
    git(&f.repo, &["branch", branch]);
    let out = f.run(&["cd", branch]);
    assert!(!out.status.success());
    let err = stderr(&out);
    let hint = err
        .lines()
        .find_map(|line| line.strip_prefix("Use: "))
        .expect("missing add hint");
    let binary = assert_cmd::cargo::cargo_bin("ofsht");
    let out = Command::new("sh")
        .args(["-c", hint])
        .current_dir(&f.repo)
        .env(
            "PATH",
            format!(
                "{}:{}",
                binary.parent().unwrap().display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .env("XDG_CONFIG_HOME", f.temp.path().join("config"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!f.repo.join("PWN").exists());
    assert!(f.temp.path().join("repo-worktrees").join(branch).exists());
}

#[cfg(unix)]
#[test]
fn github_shaped_local_branch_hint_never_invokes_gh() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    git(&f.repo, &["branch", "#123"]);
    let out = f.run(&["cd", "#123"]);
    let err = stderr(&out);
    let hint = err
        .lines()
        .find_map(|line| line.strip_prefix("Use: "))
        .unwrap();
    let bin = f.temp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh, "#!/bin/sh\nprintf called > \"$GH_CALLED\"\nexit 1\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    let binary = assert_cmd::cargo::cargo_bin("ofsht");
    let marker = f.temp.path().join("gh-called");
    let out = Command::new("sh")
        .args(["-c", hint])
        .current_dir(&f.repo)
        .env(
            "PATH",
            format!(
                "{}:{}:{}",
                bin.display(),
                binary.parent().unwrap().display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .env("GH_CALLED", &marker)
        .env("XDG_CONFIG_HOME", f.temp.path().join("config"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!marker.exists());
    assert!(f.temp.path().join("repo-worktrees/#123").exists());
}

#[test]
fn ambiguous_names_and_main_removal_refusal_remain_errors() {
    let f = Fixture::new();
    let other = f.temp.path().join("other/slot");
    git(
        &f.repo,
        &["worktree", "add", "-qb", "other", other.to_str().unwrap()],
    );
    for command in ["cd", "rm"] {
        let out = f.run(&[command, "slot"]);
        assert!(!out.status.success());
        assert!(stderr(&out).contains("Ambiguous worktree name"));
    }
    assert!(stderr(&f.run(&["rm", "main"])).contains("Cannot remove main worktree"));
}

#[test]
fn closed_stdout_exits_quietly_for_application_output() {
    let f = Fixture::new();
    for args in [
        vec!["ls"],
        vec!["ls", "--show-path"],
        vec!["shell-init", "bash"],
        vec!["shell-init", "zsh"],
        vec!["shell-init", "fish"],
        vec!["completion", "bash"],
        vec!["cd", "@"],
    ] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let out = f.command(&args).stdout(writer).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert!(out.stderr.is_empty(), "{}", stderr(&out));
    }
}

#[test]
fn closed_stdout_after_mutation_preserves_success_and_real_errors_still_fail() {
    let f = Fixture::new();
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = f.command(&["add", "new"]).stdout(writer).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!stderr(&out).contains("panicked"));
    let path = f.temp.path().join("repo-worktrees/new");
    assert!(path.exists());
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = f
        .command(&["rm", "."])
        .current_dir(&path)
        .stdout(writer)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!path.exists());
    let out = f.run(&["shell-init", "invalid"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("Invalid shell"));
}

#[test]
fn missing_target_with_no_removable_worktrees_has_a_clear_hint() {
    let f = Fixture::new();
    git(
        &f.repo,
        &["worktree", "remove", f.worktree.to_str().unwrap()],
    );
    let out = f.run(&["rm", "unknown"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("No non-main worktrees"),
        "{}",
        stderr(&out)
    );
    let out = f.run(&["cd", "unknown"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("Available worktrees:\n  @"),
        "{}",
        stderr(&out)
    );
}

#[cfg(unix)]
#[test]
fn inaccessible_directory_does_not_suggest_pruning() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let parent = f.worktree.parent().unwrap();
    let permissions = std::fs::metadata(parent).unwrap().permissions();
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o000)).unwrap();
    let inaccessible = std::fs::metadata(&f.worktree).is_err();
    let out = f.run(&["cd", "feature"]);
    std::fs::set_permissions(parent, permissions).unwrap();
    if inaccessible {
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert!(!stderr(&out).contains("prune"), "{}", stderr(&out));
    }
}
