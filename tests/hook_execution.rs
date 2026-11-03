#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("app with spaces ' $literal");
        std::fs::create_dir_all(&repo).unwrap();
        let fixture = Self { temp, repo };
        Self::git(&fixture.repo, &["init", "-qb", "main"]);
        Self::git(
            &fixture.repo,
            &["commit", "-q", "--allow-empty", "-m", "initial"],
        );
        let config = fixture.temp.path().join("config/ofsht");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("config.toml"),
            "[integration.zoxide]\nenabled=false\n",
        )
        .unwrap();
        fixture
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

    fn config(&self, text: &str) {
        std::fs::write(
            self.repo.join(".ofsht.toml"),
            format!("[worktree]\ndir='../pool/{{branch}}'\n{text}"),
        )
        .unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::cargo_bin("ofsht")
            .unwrap()
            .args(args)
            .current_dir(&self.repo)
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("TMUX")
            .output()
            .unwrap()
    }

    fn worktree(&self, name: &str) -> PathBuf {
        self.temp.path().join("pool").join(name)
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn failed_create_hooks_return_error_preserve_worktree_and_continue_actions() {
    for command in ["add", "create"] {
        let f = Fixture::new();
        f.config("[hooks.create]\nrun=['false', 'touch second-ran']\n");
        let out = f.run(&[command, "slot"]);
        assert!(!out.status.success(), "{command}: {}", stderr(&out));
        assert!(out.stdout.is_empty());
        assert!(stderr(&out).contains("Hook command failed"));
        assert!(stderr(&out).contains("preserved"), "{}", stderr(&out));
        assert!(f.worktree("slot/second-ran").exists());
        let sync = f.run(&["sync", "--run"]);
        assert!(!sync.status.success());
    }
}

#[test]
fn verbose_outputs_both_streams_without_polluting_stdout() {
    for command in ["add", "create"] {
        let f = Fixture::new();
        f.config("[hooks.create]\nrun=['printf OUT; printf ERR >&2; printf END']\n");
        let quiet = f.run(&[command, "quiet"]);
        assert!(quiet.status.success(), "{}", stderr(&quiet));
        assert!(!stderr(&quiet).contains("OUTERREND"));
        let verbose = f.run(&["--verbose", command, "verbose"]);
        assert!(verbose.status.success(), "{}", stderr(&verbose));
        assert!(
            stderr(&verbose).contains("OUTERREND"),
            "{}",
            stderr(&verbose)
        );
        assert!(!String::from_utf8_lossy(&verbose.stdout).contains("OUT"));
        if command == "add" {
            assert_eq!(
                String::from_utf8_lossy(&verbose.stdout).trim(),
                f.worktree("verbose")
                    .canonicalize()
                    .unwrap()
                    .to_str()
                    .unwrap()
            );
        } else {
            assert!(verbose.stdout.is_empty());
        }
    }
}

#[test]
fn verbose_preserves_invalid_utf8_and_long_unicode_output() {
    let f = Fixture::new();
    f.config("[hooks.create]\nrun=['printf \\\\377; printf 日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語']\n");
    let out = f.run(&["--verbose", "--color", "always", "create", "slot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(out.stderr.contains(&255), "{}", stderr(&out));
    assert!(stderr(&out).contains("日本語日本語日本語"));
}

const ENV_HOOK: &str = "printf '%s\\n' \"$OFSHT_WORKTREE_PATH\" \"$OFSHT_WORKTREE_NAME\" \"$OFSHT_BRANCH\" \"$OFSHT_REPO_ROOT\" > identity";

#[test]
fn hooks_receive_actual_worktree_identity_on_create_and_sync() {
    let f = Fixture::new();
    f.config(&format!(
        "[hooks.create]\nrun=[{}]\n",
        serde_json::to_string(ENV_HOOK).unwrap()
    ));
    let out = f.run(&["add", "nested/slot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let path = f.worktree("nested/slot");
    let expected = |branch: &str| {
        format!(
            "{}\nnested/slot\n{branch}\n{}\n",
            path.canonicalize().unwrap().display(),
            f.repo.canonicalize().unwrap().display()
        )
    };
    assert_eq!(
        std::fs::read_to_string(path.join("identity")).unwrap(),
        expected("nested/slot")
    );
    Fixture::git(&path, &["switch", "-qc", "feature"]);
    let out = f.run(&["sync", "--run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(path.join("identity")).unwrap(),
        expected("feature")
    );
    Fixture::git(&path, &["switch", "-q", "--detach"]);
    let out = f.run(&["sync", "--run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(path.join("identity")).unwrap(),
        expected("")
    );
}

#[test]
fn verbose_and_environment_reach_sync_and_delete_hooks() {
    let f = Fixture::new();
    f.config("");
    assert!(f.run(&["add", "slot"]).status.success());
    f.config("[hooks.create]\nrun=['printf sync-$OFSHT_WORKTREE_NAME']\n[hooks.delete]\nrun=['printf delete-$OFSHT_WORKTREE_NAME; false']\n");
    let sync = f.run(&["--verbose", "sync", "--run"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    assert!(stderr(&sync).contains("sync-slot"), "{}", stderr(&sync));
    let remove = f.run(&["--verbose", "rm", "--keep-branch", "slot"]);
    assert!(remove.status.success(), "{}", stderr(&remove));
    assert!(
        stderr(&remove).contains("delete-slot"),
        "{}",
        stderr(&remove)
    );
    assert!(!f.worktree("slot").exists());
}

#[test]
fn quiet_failure_keeps_tail_and_verbose_failure_does_not_duplicate_it() {
    let f = Fixture::new();
    f.config("[hooks.create]\nrun=['i=0; while [ $i -lt 12 ]; do printf \"line-%s\\n\" \"$i\"; i=$((i + 1)); done; false']\n");
    let quiet = f.run(&["create", "quiet"]);
    assert!(!quiet.status.success());
    let err = stderr(&quiet);
    assert!(!err.lines().any(|line| line.trim() == "line-0"));
    assert!(err.lines().any(|line| line.trim() == "line-2"));
    assert!(err.lines().any(|line| line.trim() == "line-11"));
    let verbose = f.run(&["--verbose", "create", "verbose"]);
    assert!(!verbose.status.success());
    let err = stderr(&verbose);
    for n in 0..12 {
        assert_eq!(
            err.lines()
                .filter(|line| line.trim() == format!("line-{n}"))
                .count(),
            1
        );
    }
}

#[test]
fn verbose_prompt_is_visible_before_sending_hook_input() {
    let f = Fixture::new();
    f.config("[hooks.create]\nrun=['printf hook-ready; read answer; test \"$answer\" = yes']\n");
    let mut child = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--verbose", "create", "slot"])
        .current_dir(&f.repo)
        .env("XDG_CONFIG_HOME", f.temp.path().join("config"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buf = [0; 256];
        let mut announced = false;
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            captured.extend_from_slice(&buf[..n]);
            if !announced && captured.windows(10).any(|bytes| bytes == b"hook-ready") {
                let _ = send.send(());
                announced = true;
            }
        }
    });
    if receive.recv_timeout(Duration::from_secs(5)).is_err() {
        child.stdin.take();
        let _ = child.kill();
        let _ = child.wait();
        reader.join().unwrap();
        panic!("Hook prompt was buffered until newline or exit");
    }
    child.stdin.take().unwrap().write_all(b"yes\n").unwrap();
    assert!(child.wait().unwrap().success());
    reader.join().unwrap();
}
