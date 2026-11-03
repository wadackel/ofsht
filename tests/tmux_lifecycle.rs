#![cfg(unix)]
#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
    socket: PathBuf,
    session: String,
    pane: String,
    window: String,
    server: String,
}

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}
fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

impl Fixture {
    fn new() -> Self {
        Self::with_socket("s")
    }
    fn with_socket(socket_name: &str) -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("app with spaces");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-qb", "main"]);
        git(&repo, &["commit", "--allow-empty", "-qm", "initial"]);
        let socket = temp.path().join(socket_name);
        let config = temp.path().join("config/ofsht");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("config.toml"),
            "[integration.zoxide]\nenabled=false\n",
        )
        .unwrap();
        let out = Command::new("tmux")
            .args([
                "-S",
                socket.to_str().unwrap(),
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-P",
                "-F",
                "#{session_id} #{pane_id} #{window_id} #{pid}",
                "-s",
                "test",
                "-x",
                "80",
                "-y",
                "24",
                "-c",
                repo.to_str().unwrap(),
                "exec /bin/sh",
            ])
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("SHELL", "/bin/sh")
            .env_remove("ENV")
            .env_remove("BASH_ENV")
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let values = String::from_utf8(out.stdout).unwrap();
        let values = values.split_whitespace().collect::<Vec<_>>();
        Self {
            repo,
            socket,
            session: values[0].to_string(),
            pane: values[1].to_string(),
            window: values[2].to_string(),
            server: values[3].to_string(),
            temp,
        }
    }
    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket)
            .args(args)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .output()
            .unwrap();
        assert!(out.status.success(), "tmux {args:?}: {}", stderr(&out));
        String::from_utf8(out.stdout)
            .unwrap()
            .trim_end_matches('\n')
            .to_string()
    }
    fn worktree(&self, name: &str) -> PathBuf {
        let path = self.temp.path().join("pool").join(name);
        git(
            &self.repo,
            &["worktree", "add", "-qb", name, path.to_str().unwrap()],
        );
        path
    }
    fn window_at(&self, path: &Path) -> (String, String) {
        let ids = self.tmux(&[
            "new-window",
            "-d",
            "-P",
            "-F",
            "#{window_id} #{pane_id}",
            "-t",
            &format!("{}:", self.session),
            "-c",
            path.to_str().unwrap(),
            "exec /bin/sh",
        ]);
        let (window, pane) = ids.split_once(' ').unwrap();
        (window.to_string(), pane.to_string())
    }
    fn windows(&self, session: &str) -> Vec<String> {
        self.tmux(&["list-windows", "-t", session, "-F", "#{window_id}"])
            .lines()
            .map(str::to_string)
            .collect()
    }
    fn enable_cleanup(&self) {
        std::fs::write(
            self.temp.path().join("config/ofsht/config.toml"),
            "[integration.zoxide]\nenabled=false\n[integration.tmux]\nclose_on_rm=true\n",
        )
        .unwrap();
    }
    fn hook(&self, command: &str) {
        std::fs::write(
            self.repo.join(".ofsht.toml"),
            format!(
                "[hooks.delete]\nrun=[{}]\n",
                serde_json::to_string(command).unwrap()
            ),
        )
        .unwrap();
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("ofsht").unwrap();
        cmd.args(args)
            .current_dir(&self.repo)
            .env(
                "TMUX",
                format!(
                    "{},{},{}",
                    self.socket.display(),
                    self.server,
                    self.session.trim_start_matches('$')
                ),
            )
            .env("TMUX_PANE", &self.pane)
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        cmd
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket)
            .arg("kill-server")
            .output();
    }
}

#[test]
fn repeated_open_does_not_duplicate_windows_or_panes() {
    for mode in ["--window", "--pane"] {
        let f = Fixture::new();
        f.worktree("one");
        f.worktree("two");
        let first = f.run(&["open", mode]);
        assert!(first.status.success(), "{}", stderr(&first));
        let before = f.tmux(&["list-panes", "-s", "-t", &f.session, "-F", "#{pane_id}"]);
        let again = f.run(&["open", mode]);
        assert!(again.status.success(), "{}", stderr(&again));
        assert_eq!(
            f.tmux(&["list-panes", "-s", "-t", &f.session, "-F", "#{pane_id}"]),
            before
        );
        assert!(
            stderr(&again).contains("Opened 0 worktree(s)"),
            "{}",
            stderr(&again)
        );
    }
}

#[test]
fn partial_splits_report_actual_count_and_failure() {
    let f = Fixture::new();
    for n in 0..12 {
        f.worktree(&format!("slot-{n}"));
    }
    let out = f.run(&["open", "--pane"]);
    let panes = f
        .tmux(&["list-panes", "-s", "-t", &f.session, "-F", "#{pane_id}"])
        .lines()
        .count();
    assert!(
        panes > 1 && panes < 13,
        "pane fixture did not produce partial splits: {panes}"
    );
    assert!(!out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&format!("Opened {} worktree(s)", panes - 1)),
        "{}",
        stderr(&out)
    );
}

#[test]
fn cleanup_is_opt_in_and_dry_run_does_not_close_windows() {
    let f = Fixture::new();
    let one = f.worktree("one");
    let (one_window, _) = f.window_at(&one);
    let out = f
        .command(&["rm", "one"])
        .env("TMUX", "invalid")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.windows(&f.session).contains(&one_window));
    let two = f.worktree("two");
    let (two_window, _) = f.window_at(&two);
    f.enable_cleanup();
    let out = f.run(&["rm", "two", "--dry-run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("Would close tmux window"),
        "{}",
        stderr(&out)
    );
    assert!(two.exists() && f.windows(&f.session).contains(&two_window));
    let out = f.run(&["rm", "two"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!two.exists());
    assert!(!f.windows(&f.session).contains(&two_window));
    assert!(f.windows(&f.session).contains(&one_window));
}

#[test]
fn removal_keeps_multi_pane_windows_and_other_session_links() {
    let f = Fixture::new();
    let one = f.worktree("one");
    let two = f.worktree("two");
    let (one_window, one_pane) = f.window_at(&one);
    let (two_window, _) = f.window_at(&two);
    f.tmux(&[
        "split-window",
        "-d",
        "-t",
        &one_pane,
        "-c",
        f.repo.to_str().unwrap(),
        "exec /bin/sh",
    ]);
    f.tmux(&["new-session", "-d", "-s", "other", "exec /bin/sh"]);
    f.tmux(&["link-window", "-s", &two_window, "-t", "other:1"]);
    f.enable_cleanup();
    let out = f.run(&["rm", "one", "two"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.windows(&f.session).contains(&one_window));
    assert!(!f.windows(&f.session).contains(&two_window));
    assert!(f.windows("other").contains(&two_window));
}

#[test]
fn groups_and_shared_callers_disable_cleanup() {
    for grouped in [true, false] {
        let f = Fixture::new();
        let path = f.worktree("one");
        let (window, _) = f.window_at(&path);
        if grouped {
            f.tmux(&["new-session", "-d", "-s", "other", "-t", &f.session]);
        } else {
            f.tmux(&["new-session", "-d", "-s", "other", "exec /bin/sh"]);
            f.tmux(&["link-window", "-s", &f.window, "-t", "other:1"]);
        }
        f.enable_cleanup();
        let out = f.run(&["rm", "one"]);
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(!path.exists());
        assert!(f.windows(&f.session).contains(&window));
        assert!(
            stderr(&out).contains(if grouped { "grouped" } else { "shared" }),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn failed_removal_keeps_window_but_prior_success_is_cleaned() {
    let f = Fixture::new();
    let one = f.worktree("one");
    let two = f.worktree("two");
    let (w1, _) = f.window_at(&one);
    let (w2, _) = f.window_at(&two);
    f.enable_cleanup();
    f.hook("case \"$OFSHT_WORKTREE_NAME\" in two) touch untracked ;; esac");
    let out = f.run(&["rm", "one", "two"]);
    assert!(!out.status.success());
    assert!(!one.exists() && two.exists());
    assert!(!f.windows(&f.session).contains(&w1));
    assert!(f.windows(&f.session).contains(&w2));
}

#[test]
fn moved_panes_and_new_session_groups_are_revalidated() {
    for change in ["move", "group", "split"] {
        let f = Fixture::new();
        let path = f.worktree("one");
        let (window, pane) = f.window_at(&path);
        f.enable_cleanup();
        let command = if change == "group" {
            format!("tmux new-session -d -s joined -t '{}'", f.session)
        } else if change == "split" {
            format!(
                "tmux split-window -d -t '{pane}' -c '{}' 'exec /bin/sh'",
                f.repo.display()
            )
        } else {
            format!(
                "tmux respawn-pane -k -t '{pane}' -c '{}' 'exec /bin/sh'",
                f.repo.display()
            )
        };
        f.hook(&command);
        let out = f.run(&["rm", "one"]);
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(f.windows(&f.session).contains(&window));
    }
}

#[test]
fn local_config_cannot_enable_cleanup() {
    let f = Fixture::new();
    let path = f.worktree("one");
    let (window, _) = f.window_at(&path);
    std::fs::write(
        f.repo.join(".ofsht.toml"),
        "[integration.tmux]\nclose_on_rm=true\n",
    )
    .unwrap();
    let out = f.run(&["rm", "one"]);
    assert!(!out.status.success());
    assert!(path.exists());
    assert!(f.windows(&f.session).contains(&window));
}

#[test]
fn open_preserves_literal_tmux_formats_in_directory_paths() {
    for mode in ["--window", "--pane"] {
        let f = Fixture::new();
        let path = f.worktree("literal-#{pid}");
        let out = f.run(&["open", mode]);
        assert!(out.status.success(), "{}", stderr(&out));
        let panes = f.tmux(&["list-panes", "-s", "-t", &f.session, "-F", "#{pane_id}"]);
        assert!(panes.lines().any(|pane| Path::new(&f.tmux(&[
            "display-message",
            "-p",
            "-t",
            pane,
            "#{pane_current_path}"
        ])) == path.canonicalize().unwrap()));
    }
}

#[test]
fn open_uses_deepest_owner_and_does_not_count_another_session() {
    let f = Fixture::new();
    let nested = f.repo.join(".pool/nested");
    git(
        &f.repo,
        &["worktree", "add", "-qb", "nested", nested.to_str().unwrap()],
    );
    std::fs::create_dir(nested.join("sub")).unwrap();
    let alias = f.temp.path().join("alias");
    std::os::unix::fs::symlink(nested.join("sub"), &alias).unwrap();
    f.tmux(&[
        "respawn-pane",
        "-k",
        "-t",
        &f.pane,
        "-c",
        alias.to_str().unwrap(),
        "exec /bin/sh",
    ]);
    let out = f
        .command(&["open", "--window"])
        .current_dir(&nested)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("Opened 1 worktree(s)"));
    let one = f.worktree("one");
    f.tmux(&[
        "new-session",
        "-d",
        "-s",
        "other",
        "-c",
        one.to_str().unwrap(),
        "exec /bin/sh",
    ]);
    let before = f.windows("other");
    let out = f.run(&["open", "--window"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("Opened 1 worktree(s)"));
    assert_eq!(f.windows("other"), before);
}

#[test]
fn sole_live_membership_wins_over_stale_creation_session() {
    let f = Fixture::new();
    let path = f.worktree("one");
    let (old_window, _) = f.window_at(&path);
    f.tmux(&["new-session", "-d", "-s", "other", "exec /bin/sh"]);
    f.tmux(&["link-window", "-s", &f.window, "-t", "other:1"]);
    f.tmux(&[
        "unlink-window",
        "-t",
        &format!("{}:{}", f.session, f.window),
    ]);
    let new_window = f.tmux(&[
        "new-window",
        "-d",
        "-P",
        "-F",
        "#{window_id}",
        "-t",
        "other:",
        "-c",
        path.to_str().unwrap(),
        "exec /bin/sh",
    ]);
    f.enable_cleanup();
    let out = f.run(&["rm", "one"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.windows(&f.session).contains(&old_window));
    assert!(!f.windows("other").contains(&new_window));
}

fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

#[test]
fn current_window_is_closed_after_all_git_work_and_output() {
    let f = Fixture::new();
    let one = f.worktree("one");
    let two = f.worktree("two");
    f.tmux(&[
        "respawn-pane",
        "-k",
        "-t",
        &f.pane,
        "-c",
        one.to_str().unwrap(),
        "exec /bin/sh",
    ]);
    let (duplicate, _) = f.window_at(&one);
    let (second, _) = f.window_at(&two);
    f.tmux(&["new-session", "-d", "-s", "keeper", "exec /bin/sh"]);
    f.enable_cleanup();
    let output = f.temp.path().join("stdout");
    let errors = f.temp.path().join("stderr");
    let binary = assert_cmd::cargo::cargo_bin("ofsht");
    let command = format!(
        "{} rm two . > {} 2> {}",
        quoted(&binary),
        quoted(&output),
        quoted(&errors)
    );
    f.tmux(&["send-keys", "-t", &f.pane, &command, "Enter"]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let windows = f.tmux(&["list-windows", "-a", "-F", "#{window_id}"]);
        if !windows.lines().any(|id| id == f.window) {
            assert!(!windows.lines().any(|id| id == duplicate || id == second));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{}",
            std::fs::read_to_string(&errors).unwrap_or_default()
        );
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert!(!one.exists() && !two.exists());
    assert_eq!(
        std::fs::read_to_string(output).unwrap(),
        format!("{}\n", f.repo.canonicalize().unwrap().display())
    );
    assert!(git(&f.repo, &["branch", "--list", "one", "two"]).is_empty());
}

#[test]
fn tmux_lookup_failure_warns_without_blocking_git_removal() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let path = f.worktree("one");
    let (window, _) = f.window_at(&path);
    f.enable_cleanup();
    let actual = Command::new("sh")
        .args(["-c", "command -v tmux"])
        .output()
        .unwrap();
    let actual = String::from_utf8(actual.stdout).unwrap();
    let bin = f.temp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::write(bin.join("tmux"),format!("#!/bin/sh\nfor arg do\n [ \"$arg\" = list-panes ] && echo lookup-failure >&2 && exit 1\ndone\nexec '{}' \"$@\"\n",actual.trim().replace('\'',"'\\''"))).unwrap();
    std::fs::set_permissions(bin.join("tmux"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = f
        .command(&["rm", "one"])
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!path.exists());
    assert!(f.windows(&f.session).contains(&window));
    assert!(stderr(&out).contains("lookup-failure"));
}

#[test]
fn all_creation_failures_report_zero_opened() {
    use std::os::unix::fs::PermissionsExt;
    for mode in ["--window", "--pane"] {
        let f = Fixture::new();
        f.worktree("one");
        f.worktree("two");
        let actual = Command::new("sh")
            .args(["-c", "command -v tmux"])
            .output()
            .unwrap();
        let actual = String::from_utf8(actual.stdout).unwrap();
        let bin = f.temp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(bin.join("tmux"),format!("#!/bin/sh\nfor arg do\n case \"$arg\" in new-window|split-window) echo creation-failure >&2; exit 1 ;; esac\ndone\nexec '{}' \"$@\"\n",actual.trim().replace('\'',"'\\''"))).unwrap();
        std::fs::set_permissions(bin.join("tmux"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = f
            .command(&["open", mode])
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            )
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(
            stderr(&out).contains("Opened 0 worktree(s)"),
            "{}",
            stderr(&out)
        );
        assert!(stderr(&out).contains("failed 2"));
        assert_eq!(f.windows(&f.session).len(), 1);
    }
}

#[test]
fn comma_socket_path_is_not_truncated_by_context_parsing() {
    let f = Fixture::with_socket("s,comma");
    let path = f.worktree("one");
    let out = f.run(&["open", "--window"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(f.windows(&f.session).len(), 2);
    f.enable_cleanup();
    let out = f.run(&["rm", "one"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!path.exists());
    assert_eq!(f.windows(&f.session).len(), 1);
}

#[test]
fn invocation_via_shared_session_with_stale_environment_abstains() {
    let f = Fixture::new();
    let path = f.worktree("one");
    let (window, _) = f.window_at(&path);
    f.tmux(&["new-session", "-d", "-s", "other", "exec /bin/sh"]);
    f.tmux(&["link-window", "-s", &f.window, "-t", "other:1"]);
    f.enable_cleanup();
    let log = f.temp.path().join("log");
    let status = f.temp.path().join("status");
    let binary = assert_cmd::cargo::cargo_bin("ofsht");
    let command = format!(
        "{} rm one > {} 2>&1; printf '%s' $? > {}",
        quoted(&binary),
        quoted(&log),
        quoted(&status)
    );
    f.tmux(&[
        "send-keys",
        "-t",
        &format!("other:{}", f.window),
        &command,
        "Enter",
    ]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !status.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "{}",
            std::fs::read_to_string(&log).unwrap_or_default()
        );
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert_eq!(std::fs::read_to_string(status).unwrap(), "0");
    assert!(!path.exists());
    assert!(f.windows(&f.session).contains(&window));
    assert!(std::fs::read_to_string(log).unwrap().contains("shared"));
}
