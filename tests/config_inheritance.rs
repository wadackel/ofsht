#![allow(deprecated, clippy::literal_string_with_formatting_args)]

use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
    home: PathBuf,
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

const GLOBAL_ACTIONS: &str = r#"
[hooks.create]
run = ["touch global-run"]
copy = ["copied.env"]
link = ["linked.env"]
[hooks.delete]
run = ['printf deleted > "$OFSHT_REPO_ROOT/deleted"']
copy = ["delete-copy"]
link = ["delete-link"]
"#;

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("app");
        let home = temp.path().join("isolated home");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&home).unwrap();
        git(&repo, &["init", "-qb", "main"]);
        git(&repo, &["commit", "--allow-empty", "-qm", "initial"]);
        for name in ["copied.env", "linked.env", "delete-copy", "delete-link"] {
            std::fs::write(repo.join(name), name).unwrap();
        }
        let f = Self { temp, repo, home };
        f.global("../global/{branch}", GLOBAL_ACTIONS);
        f
    }
    fn global(&self, dir: &str, actions: &str) {
        let folder = self.temp.path().join("config/ofsht");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("config.toml"),format!("[worktree]\ndir={}\n[integration.zoxide]\nenabled=false\n[integration.gh]\nenabled=false\n{actions}",serde_json::to_string(dir).unwrap())).unwrap();
    }
    fn local(&self, config: &str) {
        std::fs::write(self.repo.join(".ofsht.toml"), config).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::cargo_bin("ofsht")
            .unwrap()
            .args(args)
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("TMUX")
            .output()
            .unwrap()
    }
    fn path(&self, name: &str) -> PathBuf {
        self.temp.path().join("global").join(name)
    }
}
fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn hook_only_local_config_keeps_global_worktree_directory() {
    let f = Fixture::new();
    f.local("[hooks.create]\nrun=[]\n");
    let out = f.run(&["create", "slot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.path("slot").exists());
    assert!(!f.temp.path().join("app-worktrees/slot").exists());
}

#[test]
fn empty_local_file_and_sections_inherit_global_fields() {
    for local in ["", "[worktree]\n[hooks]\n[hooks.create]\n[hooks.delete]\n"] {
        let f = Fixture::new();
        f.local(local);
        let out = f.run(&["create", "slot"]);
        assert!(out.status.success(), "{}", stderr(&out));
        for name in ["global-run", "copied.env", "linked.env"] {
            assert!(f.path("slot").join(name).exists(), "{name}");
        }
    }
}

#[test]
fn local_run_replaces_run_but_inherits_other_actions_and_delete_hook() {
    let f = Fixture::new();
    f.local("[worktree]\ndir='../global/{branch}'\n[hooks.create]\nrun=['touch local-run']\n");
    let out = f.run(&["create", "slot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.path("slot/local-run").exists());
    assert!(!f.path("slot/global-run").exists());
    assert_eq!(
        std::fs::read_to_string(f.path("slot/copied.env")).unwrap(),
        "copied.env"
    );
    assert!(f.path("slot/linked.env").is_symlink());
    let out = f.run(&["rm", "slot", "--force"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(f.repo.join("deleted")).unwrap(),
        "deleted"
    );
    assert!(stderr(&out).contains("Copied: delete-copy"));
    assert!(stderr(&out).contains("Linked: delete-link"));
}

#[test]
fn explicit_empty_lists_disable_only_the_selected_create_action() {
    for field in ["run", "copy", "link"] {
        let f = Fixture::new();
        f.local(&format!(
            "[worktree]\ndir='../global/{{branch}}'\n[hooks.create]\n{field}=[]\n"
        ));
        let out = f.run(&["create", "slot"]);
        assert!(out.status.success(), "{}", stderr(&out));
        for (action, file) in [
            ("run", "global-run"),
            ("copy", "copied.env"),
            ("link", "linked.env"),
        ] {
            assert_eq!(
                f.path("slot").join(file).exists(),
                action != field,
                "{field}/{action}"
            );
        }
    }
}

#[test]
fn explicit_empty_lists_disable_only_the_selected_delete_action() {
    for field in ["run", "copy", "link"] {
        let f = Fixture::new();
        let out = f.run(&["create", "slot"]);
        assert!(out.status.success(), "{}", stderr(&out));
        f.local(&format!("[hooks.delete]\n{field}=[]\n"));
        let out = f.run(&["rm", "slot", "--force"]);
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(f.repo.join("deleted").exists(), field != "run");
        assert_eq!(
            stderr(&out).contains("Copied: delete-copy"),
            field != "copy"
        );
        assert_eq!(
            stderr(&out).contains("Linked: delete-link"),
            field != "link"
        );
    }
}

#[test]
fn explicit_local_directory_wins_and_global_integrations_remain() {
    let f = Fixture::new();
    f.local("[worktree]\ndir='../local/{branch}'\n");
    let out = f.run(&["add", "#77"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let dest = f.temp.path().join("local/#77");
    assert!(dest.exists());
    assert!(dest.join("global-run").exists());
    assert!(!f.path("#77").exists());
    assert!(stderr(&out).contains("GitHub integration is disabled"));
    f.local("[integration.gh]\nenabled=true\n");
    let out = f.run(&["create", "invalid"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("unknown field"));
}

#[cfg(unix)]
#[test]
fn home_template_round_trip_preserves_nested_names_and_branch_cleanup() {
    let f = Fixture::new();
    f.global("~/pool/{repo}/{branch}", "");
    f.local("[hooks.create]\nrun=[]\n");
    let out = f.run(&["add", "topic/slot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let dest = f.home.join("pool/app/topic/slot");
    assert!(dest.exists());
    assert!(!f.repo.join("~").exists());
    let printed = PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
    assert!(printed.is_absolute());
    assert_eq!(
        printed.canonicalize().unwrap(),
        dest.canonicalize().unwrap()
    );
    git(&dest, &["switch", "-qc", "renamed"]);
    let out = f.run(&["cd", "topic/slot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        dest.canonicalize().unwrap().to_str().unwrap()
    );
    let out = f.run(&["ls", "--show-path"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("topic/slot"));
    let out = f.run(&["rm", "topic/slot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!dest.exists());
    assert!(git(&f.repo, &["branch", "--list", "topic/slot"]).is_empty());
    assert!(git(&f.repo, &["branch", "--list", "renamed"]).is_empty());
    assert!(!git(&f.repo, &["branch", "--list", "main"]).is_empty());
}

#[cfg(unix)]
#[test]
fn repository_name_is_data_not_a_home_expansion_marker() {
    let mut f = Fixture::new();
    let renamed = f.temp.path().join("~");
    std::fs::rename(&f.repo, &renamed).unwrap();
    f.repo = renamed;
    f.global("{repo}/{branch}", "");
    let out = f.run(&["create", "slot"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.repo.join("~/slot").exists());
    assert!(!f.home.join("slot").exists());
}
