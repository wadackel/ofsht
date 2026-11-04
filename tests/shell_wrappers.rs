#![cfg(unix)]
#![allow(deprecated)]

use assert_fs::TempDir;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
    worktree: PathBuf,
    bin: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
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
        let base = temp.path().canonicalize().unwrap();
        let repo = base.join("app with spaces");
        let worktree = base.join("pool/slot one");
        let bin = base.join("bin");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&bin).unwrap();
        std::fs::create_dir_all(base.join("config/ofsht")).unwrap();
        std::fs::create_dir(base.join("home")).unwrap();
        std::fs::write(
            base.join("config/ofsht/config.toml"),
            "[integration.zoxide]\nenabled=false\n[integration.tmux]\nbehavior='never'\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(assert_cmd::cargo::cargo_bin("ofsht"), bin.join("ofsht"))
            .unwrap();
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
        Self {
            temp,
            repo,
            worktree,
            bin,
        }
    }

    fn run(&self, shell: &str, cwd: &Path, args: &[&str], stdin: Option<&str>) -> Output {
        let wrapper = Command::new(self.bin.join("ofsht"))
            .args(["shell-init", shell])
            .output()
            .unwrap();
        assert!(wrapper.status.success());
        let mut script = String::from_utf8(wrapper.stdout).unwrap();
        if shell == "fish" {
            script.push_str("\nofsht $argv\nset -l result_code $status\nprintf '\\n__RESULT__:%s:%s\\n' $result_code \"$PWD\"\nexit $result_code\n");
        } else {
            script.push_str("\nset -u\nofsht \"$@\"\nresult_code=$?\nprintf '\\n__RESULT__:%s:%s\\n' \"$result_code\" \"$PWD\"\nexit \"$result_code\"\n");
        }
        let script_path = self.temp.path().join(format!("test.{shell}"));
        std::fs::write(&script_path, script).unwrap();
        let mut command = Command::new(shell);
        match shell {
            "bash" => {
                command.args(["--noprofile", "--norc"]);
            }
            "zsh" => {
                command.arg("-f");
            }
            "fish" => {
                command.arg("--no-config");
            }
            _ => unreachable!(),
        }
        command
            .arg(script_path)
            .args(args)
            .current_dir(cwd)
            .env(
                "PATH",
                format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("HOME", self.temp.path().join("home"))
            .env("ZDOTDIR", self.temp.path().join("home"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("XDG_DATA_HOME", self.temp.path().join("data"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("BASH_ENV")
            .env_remove("ENV")
            .env_remove("TMUX")
            .env_remove("COMPLETE")
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        if let Some(stdin) = stdin {
            input.write_all(stdin.as_bytes()).unwrap();
        }
        drop(input);
        child.wait_with_output().unwrap()
    }
}

fn assert_result(out: &Output, code: i32, path: &Path) -> String {
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(code), "{stdout}\n{stderr}");
    let suffix = format!("\n__RESULT__:{code}:{}\n", path.display());
    stdout
        .strip_suffix(&suffix)
        .unwrap_or_else(|| panic!("Expected {suffix:?}: {stdout}\n{stderr}"))
        .to_string()
}

fn check_shell(shell: &str) {
    let f = Fixture::new();
    check_help(shell, &f);
    check_navigation(shell, &f);
    check_errors(shell, &f);
    check_mutations(shell, &f);
}

fn check_help(shell: &str, f: &Fixture) {
    for command in ["cd", "add", "rm", "switch"] {
        for arguments in [
            vec![command, "--help"],
            vec![command, "-h"],
            vec!["--color=never", command, "--help"],
            vec!["--color", "never", "-v", command, "-vh"],
        ] {
            let out = f.run(shell, &f.repo, &arguments, None);
            let body = assert_result(&out, 0, &f.repo);
            assert!(body.contains("Usage:"), "{shell}: {body}");
            assert!(f.worktree.is_dir());
        }
    }
}

fn check_navigation(shell: &str, f: &Fixture) {
    assert!(assert_result(
        &f.run(
            shell,
            &f.repo,
            &["--color=never", "switch", "feature"],
            None
        ),
        0,
        &f.worktree
    )
    .is_empty());
    for globals in [
        vec![],
        vec!["--color=never"],
        vec!["--color", "never"],
        vec!["-v", "--color", "never"],
        vec!["--verbose"],
    ] {
        let mut args = globals;
        args.extend(["cd", "feature"]);
        assert!(assert_result(&f.run(shell, &f.repo, &args, None), 0, &f.worktree).is_empty());
    }
    assert!(assert_result(
        &f.run(shell, &f.repo, &["cd", "--", "feature"], None),
        0,
        &f.worktree
    )
    .is_empty());
    let help_path = f.repo.parent().unwrap().join("pool/--help");
    git(
        &f.repo,
        &["worktree", "add", "--detach", help_path.to_str().unwrap()],
    );
    assert!(assert_result(
        &f.run(shell, &f.repo, &["cd", "--", "--help"], None),
        0,
        &help_path
    )
    .is_empty());
    assert!(assert_result(
        &f.run(shell, &f.repo, &["rm", "--", "--help"], None),
        0,
        &f.repo
    )
    .is_empty());
    assert!(assert_result(
        &f.run(shell, &f.repo, &["--color=never", "cd"], Some("feature\n")),
        0,
        &f.worktree
    )
    .is_empty());
}

fn check_errors(shell: &str, f: &Fixture) {
    for (args, code) in [
        (vec!["cd", "missing"], 1),
        (vec!["--unknown", "cd", "feature"], 2),
        (vec!["--color", "cd", "feature"], 2),
        (vec!["--color"], 2),
        (vec![], 2),
    ] {
        let out = f.run(shell, &f.repo, &args, None);
        assert!(assert_result(&out, code, &f.repo).is_empty());
        assert!(!out.stderr.is_empty());
    }
}

fn check_mutations(shell: &str, f: &Fixture) {
    git(&f.repo, &["branch", "pool-feature"]);
    assert!(assert_result(
        &f.run(
            shell,
            &f.repo,
            &["--verbose", "switch", "pool-feature", "--in", "slot one"],
            None
        ),
        0,
        &f.worktree
    )
    .is_empty());
    assert!(assert_result(
        &f.run(
            shell,
            &f.repo,
            &["--color=never", "add", "new-branch"],
            None
        ),
        0,
        &f.repo
            .parent()
            .unwrap()
            .join("app with spaces-worktrees/new-branch")
    )
    .is_empty());
    assert!(assert_result(
        &f.run(shell, &f.worktree, &["--color", "never", "rm", "."], None),
        0,
        &f.repo
    )
    .is_empty());
    assert!(!f.worktree.exists());
    let out = f.run(shell, &f.repo, &["--verbose", "rm", "new-branch"], None);
    assert!(assert_result(&out, 0, &f.repo).is_empty());
    let out = f.run(shell, &f.repo, &["--color=never", "ls"], None);
    assert_eq!(assert_result(&out, 0, &f.repo), "@\n");
}

#[test]
fn bash_navigation_and_help() {
    check_shell("bash");
}

#[test]
fn zsh_navigation_and_help() {
    check_shell("zsh");
}

#[test]
fn fish_navigation_and_help() {
    check_shell("fish");
}
