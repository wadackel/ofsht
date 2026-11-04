#![cfg(unix)]
#![allow(deprecated)]

use assert_fs::TempDir;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
    origin: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let repo = root.join("repo with spaces");
        let origin = root.join("origin.git");
        std::fs::create_dir(&repo).unwrap();
        let f = Self { temp, repo, origin };
        f.git(&f.repo, &["init", "-qb", "main"]);
        std::fs::write(f.repo.join("file"), "base\n").unwrap();
        std::fs::write(f.repo.join(".gitignore"), ".env\nnode_modules\n").unwrap();
        f.git(&f.repo, &["add", "."]);
        f.git(&f.repo, &["commit", "-qm", "base"]);
        f.git(
            &f.repo,
            &["init", "--bare", "-q", f.origin.to_str().unwrap()],
        );
        f.git(
            &f.repo,
            &["remote", "add", "origin", f.origin.to_str().unwrap()],
        );
        f.git(&f.repo, &["push", "-q", "origin", "main"]);
        std::fs::create_dir_all(f.temp.path().join("bin")).unwrap();
        let gh = f.temp.path().join("bin/gh");
        std::fs::write(&gh, "#!/bin/sh\ncase \"$1 $2\" in\n '--version '*) echo gh-test ;;\n 'pr view') test -f \"$GH_PR_JSON\" && cat \"$GH_PR_JSON\" ;;\n 'issue view') test -f \"$GH_ISSUE_JSON\" && cat \"$GH_ISSUE_JSON\" ;;\n *) exit 1 ;;\nesac\n").unwrap();
        std::fs::set_permissions(gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        f.config("");
        f
    }
    fn command(&self, program: &Path, dir: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .env("HOME", self.temp.path())
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.temp.path().join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("GH_PR_JSON", self.temp.path().join("pr.json"))
            .env("GH_ISSUE_JSON", self.temp.path().join("issue.json"))
            .env_remove("TMUX")
            .env_remove("COMPLETE")
            .stdin(Stdio::null());
        cmd
    }
    fn git_result(&self, dir: &Path, args: &[&str]) -> Output {
        self.command(Path::new("git"), dir)
            .args(args)
            .output()
            .unwrap()
    }
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git_result(dir, args);
        assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
        String::from_utf8(out.stdout)
            .unwrap()
            .trim_end_matches('\n')
            .to_string()
    }
    fn cli(&self, dir: &Path, args: &[&str]) -> Output {
        self.command(&assert_cmd::cargo::cargo_bin("ofsht"), dir)
            .args(args)
            .output()
            .unwrap()
    }
    fn run(&self, args: &[&str]) -> Output {
        self.cli(&self.repo, args)
    }
    fn slot(&self, name: &str) -> PathBuf {
        let path = self.repo.parent().unwrap().join("pool").join(name);
        self.git(
            &self.repo,
            &["worktree", "add", "-qb", name, path.to_str().unwrap()],
        );
        path
    }
    fn branch(&self, name: &str) {
        self.git(&self.repo, &["branch", name]);
    }
    fn head(&self, name: &str) -> String {
        self.git(&self.repo, &["rev-parse", "--verify", name])
    }
    fn count(&self) -> usize {
        self.git(&self.repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|l| l.starts_with("worktree "))
            .count()
    }
    fn config(&self, extra: &str) {
        let directory = self.temp.path().join("config/ofsht");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("config.toml"),
            format!("[integration.zoxide]\nenabled=false\n{extra}"),
        )
        .unwrap();
    }
    fn hooks(&self, run: &str) {
        self.config(&format!(
            "[hooks.create]\nrun=[{}]\n",
            serde_json::to_string(run).unwrap()
        ));
    }
    fn pr(&self, number: u32, branch: &str, fork: bool) {
        std::fs::write(self.temp.path().join("pr.json"), serde_json::json!({"number":number,"title":"demo","headRefName":branch,"isCrossRepository":fork}).to_string()).unwrap();
    }
    fn issue(&self, number: u32) {
        std::fs::write(
            self.temp.path().join("issue.json"),
            serde_json::json!({"number":number,"title":"demo"}).to_string(),
        )
        .unwrap();
    }
    fn remote_commit(&self, destination: &str, ignored_file: bool) -> String {
        self.git(&self.repo, &["switch", "--detach"]);
        if ignored_file {
            std::fs::write(self.repo.join(".env"), "branch env\n").unwrap();
            self.git(&self.repo, &["add", "-f", ".env"]);
        }
        self.git(
            &self.repo,
            &["commit", "--allow-empty", "-qm", "remote change"],
        );
        let sha = self.head("HEAD");
        self.git(
            &self.repo,
            &["push", "-q", "origin", &format!("{sha}:{destination}")],
        );
        self.git(&self.repo, &["switch", "main"]);
        sha
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}
fn navigates(out: &Output, path: &Path) {
    assert!(out.status.success(), "{}", stderr(out));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n", path.display())
    );
}
fn refused(out: &Output) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(
        !stderr(out).contains("unrecognized subcommand"),
        "{}",
        stderr(out)
    );
}
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

#[test]
fn existing_dirty_checkout_and_main_are_navigation_only() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    let other = f.slot("other");
    let marker = f.temp.path().join("hook");
    f.hooks(&format!("touch {}", quoted(&marker)));
    std::fs::write(slot.join("file"), "local work").unwrap();
    navigates(&f.run(&["switch", "slot", "--in", "other"]), &slot);
    navigates(&f.run(&["switch", "main"]), &f.repo);
    assert_eq!(
        std::fs::read_to_string(slot.join("file")).unwrap(),
        "local work"
    );
    assert_eq!(f.git(&other, &["branch", "--show-current"]), "other");
    assert!(!marker.exists());
    assert_eq!(f.count(), 3);
}

#[test]
fn explicit_name_path_and_dot_reuse_the_same_directory() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("feature");
    let old = f.head("slot");
    std::fs::create_dir(slot.join("sub")).unwrap();
    for (directory, target) in [
        (&f.repo, "slot"),
        (&f.repo, slot.to_str().unwrap()),
        (&slot.join("sub"), "."),
    ] {
        navigates(
            &f.cli(directory, &["switch", "feature", "--in", target]),
            &slot,
        );
        assert_eq!(f.git(&slot, &["branch", "--show-current"]), "feature");
        assert_eq!(f.head("slot"), old);
        assert_eq!(f.count(), 2);
        f.git(&slot, &["switch", "slot"]);
    }
}

#[test]
fn noninteractive_selection_and_unknown_branch_do_not_change_state() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("feature");
    let before = f.head("slot");
    let out = f.run(&["switch", "feature"]);
    refused(&out);
    assert!(stderr(&out).contains("--in"));
    refused(&f.run(&["switch"]));
    refused(&f.run(&["switch", "missing", "--in", "slot"]));
    refused(&f.run(&["switch", "feature", "--in", "@"]));
    assert_eq!(f.head("slot"), before);
    assert_eq!(f.git(&slot, &["branch", "--show-current"]), "slot");
    assert_eq!(f.count(), 2);
}

#[test]
fn dirty_locked_missing_and_foreign_members_are_refused() {
    for kind in [
        "tracked",
        "untracked",
        "staged",
        "locked",
        "missing",
        "foreign",
    ] {
        let f = Fixture::new();
        let slot = f.slot("slot");
        f.branch("feature");
        let target_before = f.head("feature");
        let marker = f.temp.path().join("hook");
        f.hooks(&format!("touch {}", quoted(&marker)));
        match kind {
            "tracked" => std::fs::write(slot.join("file"), "dirty").unwrap(),
            "untracked" => std::fs::write(slot.join("untracked"), "dirty").unwrap(),
            "staged" => {
                std::fs::write(slot.join("file"), "staged").unwrap();
                f.git(&slot, &["add", "file"]);
            }
            "locked" => {
                f.git(&f.repo, &["worktree", "lock", slot.to_str().unwrap()]);
            }
            "missing" | "foreign" => {
                std::fs::remove_dir_all(&slot).unwrap();
                if kind == "foreign" {
                    std::fs::create_dir(&slot).unwrap();
                    f.git(&slot, &["init", "-qb", "foreign"]);
                    f.git(&slot, &["commit", "--allow-empty", "-qm", "foreign"]);
                }
            }
            _ => unreachable!(),
        }
        refused(&f.run(&["switch", "feature", "--in", "slot"]));
        assert_eq!(f.head("feature"), target_before);
        assert!(!marker.exists());
        assert_eq!(f.count(), 2);
    }
}

#[test]
fn same_repository_alias_cannot_repurpose_main_or_another_member() {
    for main in [true, false] {
        let f = Fixture::new();
        let slot = f.slot("slot");
        let other = f.slot("other");
        f.branch("feature");
        std::fs::rename(&slot, f.temp.path().join("saved-slot")).unwrap();
        std::os::unix::fs::symlink(if main { &f.repo } else { &other }, &slot).unwrap();
        refused(&f.run(&["switch", "feature", "--in", "slot"]));
        assert_eq!(f.git(&f.repo, &["branch", "--show-current"]), "main");
        assert_eq!(f.git(&other, &["branch", "--show-current"]), "other");
    }
}

#[test]
fn detached_only_commits_need_a_shared_reference() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("feature");
    f.git(&slot, &["switch", "--detach"]);
    f.git(&slot, &["commit", "--allow-empty", "-qm", "detached"]);
    let detached = f.git(&slot, &["rev-parse", "HEAD"]);
    refused(&f.run(&["switch", "feature", "--in", "slot"]));
    assert_eq!(f.git(&slot, &["rev-parse", "HEAD"]), detached);
    f.git(&f.repo, &["tag", "saved", &detached]);
    navigates(&f.run(&["switch", "feature", "--in", "slot"]), &slot);
    assert_eq!(f.head("saved"), detached);
}

#[test]
fn ignored_file_collision_does_not_discard_local_data() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    let sha = f.remote_commit("refs/heads/feature", true);
    f.git(&f.repo, &["branch", "feature", &sha]);
    std::fs::write(slot.join(".env"), "local secret").unwrap();
    refused(&f.run(&["switch", "feature", "--in", "slot"]));
    assert_eq!(
        std::fs::read_to_string(slot.join(".env")).unwrap(),
        "local secret"
    );
    assert_eq!(f.git(&slot, &["branch", "--show-current"]), "slot");
}

#[test]
fn hook_failure_retains_switched_branch_and_suppresses_navigation() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("feature");
    f.hooks("exit 9");
    let out = f.run(&["switch", "feature", "--in", "slot"]);
    refused(&out);
    assert_eq!(f.git(&slot, &["branch", "--show-current"]), "feature");
    assert!(stderr(&out).contains("sync"));
    assert_eq!(f.count(), 2);
}

#[test]
fn issue_branch_uses_caller_commit_not_selected_member_head() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    let caller = f.slot("caller");
    f.git(
        &caller,
        &["commit", "--allow-empty", "-qm", "caller commit"],
    );
    let caller_head = f.git(&caller, &["rev-parse", "HEAD"]);
    let slot_head = f.head("slot");
    f.issue(7);
    navigates(&f.cli(&caller, &["switch", "#7", "--in", "slot"]), &slot);
    assert_eq!(f.head("issue-7"), caller_head);
    assert_ne!(caller_head, slot_head);
    assert_eq!(f.head("slot"), slot_head);
    assert_eq!(f.count(), 3);
}

#[test]
fn pr_switch_fast_forwards_unused_branch_then_reuses_without_fetch() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("topic");
    f.git(&f.repo, &["config", "branch.topic.remote", "origin"]);
    f.git(
        &f.repo,
        &["config", "branch.topic.merge", "refs/heads/custom"],
    );
    let sha = f.remote_commit("refs/heads/topic", false);
    f.pr(1, "topic", false);
    navigates(&f.run(&["switch", "#1", "--in", "slot"]), &slot);
    assert_eq!(f.head("topic"), sha);
    assert_eq!(
        f.git(&f.repo, &["config", "branch.topic.merge"]),
        "refs/heads/custom"
    );
    f.git(
        &f.repo,
        &[
            "remote",
            "set-url",
            "origin",
            f.temp.path().join("offline").to_str().unwrap(),
        ],
    );
    navigates(&f.run(&["switch", "#1"]), &slot);
    assert_eq!(f.count(), 2);
}

#[test]
fn fork_pr_reuses_its_recorded_branch_without_replacing_unrelated_branch() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("topic");
    let unrelated = f.head("topic");
    let sha = f.remote_commit("refs/pull/1/head", false);
    f.pr(1, "topic", true);
    navigates(&f.run(&["switch", "#1", "--in", "slot"]), &slot);
    assert_eq!(f.head("pr-1-topic"), sha);
    assert_eq!(f.head("topic"), unrelated);
    assert_eq!(
        f.git(&f.repo, &["config", "branch.pr-1-topic.ofsht-pr"]),
        "1"
    );
    navigates(&f.run(&["switch", "#1"]), &slot);
    assert_eq!(f.count(), 2);
}

#[test]
fn dirty_target_refusal_does_not_materialize_pr_refs() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("topic");
    let before = f.head("topic");
    f.remote_commit("refs/heads/topic", false);
    f.pr(1, "topic", false);
    std::fs::write(slot.join("file"), "dirty").unwrap();
    refused(&f.run(&["switch", "#1", "--in", "slot"]));
    assert_eq!(f.head("topic"), before);
    assert!(!f.repo.join(".git/FETCH_HEAD").exists());
}

#[test]
fn late_pr_checkout_refusal_reports_retained_refs_and_preserves_files() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("topic");
    let before = f.head("slot");
    let sha = f.remote_commit("refs/heads/topic", true);
    f.pr(1, "topic", false);
    std::fs::write(slot.join(".env"), "local secret").unwrap();
    let out = f.run(&["switch", "#1", "--in", "slot"]);
    refused(&out);
    assert_eq!(f.head("topic"), sha);
    assert_eq!(f.git(&slot, &["rev-parse", "HEAD"]), before);
    assert_eq!(f.git(&slot, &["branch", "--show-current"]), "slot");
    assert_eq!(
        std::fs::read_to_string(slot.join(".env")).unwrap(),
        "local secret"
    );
    assert!(stderr(&out).contains("retained"), "{}", stderr(&out));
}

#[test]
fn hooks_preserve_copies_and_tracked_links_with_new_branch_context() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.git(&f.repo, &["switch", "-c", "feature"]);
    std::fs::write(f.repo.join("node_modules"), "tracked branch file").unwrap();
    f.git(&f.repo, &["add", "-f", "node_modules"]);
    f.git(&f.repo, &["commit", "-qm", "tracked link destination"]);
    f.git(&f.repo, &["switch", "main"]);
    std::fs::create_dir(f.repo.join("node_modules")).unwrap();
    std::fs::write(f.repo.join(".env"), "source env").unwrap();
    std::fs::write(slot.join(".env"), "member env").unwrap();
    let marker = f.temp.path().join("context");
    let run = format!(
        "printf '%s|%s' \"$OFSHT_BRANCH\" \"$OFSHT_WORKTREE_NAME\" > {}",
        quoted(&marker)
    );
    f.config(&format!(
        "[hooks.create]\nrun=[{}]\ncopy=['.env']\nlink=['node_modules']\n",
        serde_json::to_string(&run).unwrap()
    ));
    navigates(&f.run(&["switch", "feature", "--in", "slot"]), &slot);
    assert_eq!(
        std::fs::read_to_string(slot.join(".env")).unwrap(),
        "member env"
    );
    assert_eq!(
        std::fs::read_to_string(slot.join("node_modules")).unwrap(),
        "tracked branch file"
    );
    assert!(!slot
        .join("node_modules")
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "feature|slot");
}

#[test]
fn existing_rebase_is_navigable_but_not_a_reuse_candidate() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("destination");
    std::fs::write(f.repo.join("file"), "main\n").unwrap();
    f.git(&f.repo, &["commit", "-qam", "main"]);
    std::fs::write(slot.join("file"), "slot\n").unwrap();
    f.git(&slot, &["commit", "-qam", "slot"]);
    assert!(!f
        .git_result(&slot, &["rebase", "--merge", "main"])
        .status
        .success());
    let marker = f.temp.path().join("hook");
    f.hooks(&format!("touch {}", quoted(&marker)));
    navigates(&f.run(&["switch", "slot"]), &slot);
    let out = f.run(&["switch", "destination", "--in", "slot"]);
    refused(&out);
    assert!(stderr(&out).contains("operation in progress"));
    assert!(!marker.exists());
}

#[test]
fn clean_sequencer_only_operation_is_refused() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("destination");
    f.git(&f.repo, &["switch", "-c", "source"]);
    std::fs::write(f.repo.join("file"), "source\n").unwrap();
    f.git(&f.repo, &["commit", "-qam", "first"]);
    let first = f.head("HEAD");
    std::fs::write(f.repo.join("second"), "second").unwrap();
    f.git(&f.repo, &["add", "second"]);
    f.git(&f.repo, &["commit", "-qm", "second"]);
    let second = f.head("HEAD");
    f.git(&f.repo, &["switch", "main"]);
    std::fs::write(slot.join("file"), "slot\n").unwrap();
    f.git(&slot, &["commit", "-qam", "slot"]);
    assert!(!f
        .git_result(&slot, &["cherry-pick", &first, &second])
        .status
        .success());
    std::fs::write(slot.join("file"), "resolved\n").unwrap();
    f.git(&slot, &["add", "file"]);
    f.git(&slot, &["commit", "-qm", "resolved"]);
    let admin = PathBuf::from(f.git(&slot, &["rev-parse", "--absolute-git-dir"]));
    assert!(admin.join("sequencer").is_dir());
    assert!(!admin.join("CHERRY_PICK_HEAD").exists());
    assert!(f.git(&slot, &["status", "--porcelain"]).is_empty());
    let out = f.run(&["switch", "destination", "--in", "slot"]);
    refused(&out);
    assert!(stderr(&out).contains("operation in progress"));
}

#[test]
fn dirty_submodule_is_not_reused() {
    let f = Fixture::new();
    let child = Fixture::new();
    let slot = f.slot("slot");
    f.branch("feature");
    f.git(
        &slot,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            child.repo.to_str().unwrap(),
            "module",
        ],
    );
    f.git(&slot, &["commit", "-qm", "module"]);
    f.git(&slot, &["config", "submodule.module.ignore", "all"]);
    std::fs::write(slot.join("module/file"), "local submodule work").unwrap();
    refused(&f.run(&["switch", "feature", "--in", "slot"]));
    assert_eq!(
        std::fs::read_to_string(slot.join("module/file")).unwrap(),
        "local submodule work"
    );
    assert_eq!(f.git(&slot, &["branch", "--show-current"]), "slot");
}

#[test]
fn relative_registration_backlinks_remain_usable() {
    let f = Fixture::new();
    let slot = f.repo.parent().unwrap().join("relative-slot");
    f.git(
        &f.repo,
        &[
            "-c",
            "worktree.useRelativePaths=true",
            "worktree",
            "add",
            "-b",
            "slot",
            slot.to_str().unwrap(),
        ],
    );
    f.branch("feature");
    let admin = PathBuf::from(f.git(&slot, &["rev-parse", "--absolute-git-dir"]));
    assert!(!std::fs::read_to_string(admin.join("gitdir"))
        .unwrap()
        .starts_with('/'));
    navigates(&f.run(&["switch", "feature", "--in", "slot"]), &slot);
}

#[test]
fn plain_branch_uses_git_remote_guessing() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    let sha = f.remote_commit("refs/heads/feature", false);
    navigates(&f.run(&["switch", "feature", "--in", "slot"]), &slot);
    assert_eq!(f.head("feature"), sha);
    assert_eq!(
        f.git(&f.repo, &["config", "branch.feature.remote"]),
        "origin"
    );
}

#[test]
fn local_commits_on_pr_branch_are_preserved() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.git(&f.repo, &["switch", "-c", "topic"]);
    f.git(&f.repo, &["commit", "--allow-empty", "-qm", "local"]);
    let local = f.head("HEAD");
    f.git(&f.repo, &["switch", "main"]);
    f.remote_commit("refs/heads/topic", false);
    f.pr(1, "topic", false);
    navigates(&f.run(&["switch", "#1", "--in", "slot"]), &slot);
    assert_eq!(f.head("topic"), local);
}

#[test]
fn disabled_github_uses_literal_branch() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("#1");
    f.config("[integration.gh]\nenabled=false\n");
    navigates(&f.run(&["switch", "#1", "--in", "slot"]), &slot);
    assert_eq!(f.git(&slot, &["branch", "--show-current"]), "#1");
}

#[test]
fn git_post_checkout_failure_retains_resulting_state_without_navigation() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("feature");
    let hook = f.repo.join(".git/hooks/post-checkout");
    std::fs::write(&hook, "#!/bin/sh\necho post-checkout-failed >&2\nexit 7\n").unwrap();
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = f.run(&["switch", "feature", "--in", "slot"]);
    refused(&out);
    assert_eq!(f.git(&slot, &["branch", "--show-current"]), "feature");
    assert!(stderr(&out).contains("post-checkout-failed"));
    assert!(stderr(&out).contains(slot.to_str().unwrap()));
}

fn install_git_shim(f: &Fixture, body: &str) {
    let output = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let actual = String::from_utf8(output.stdout).unwrap();
    let script = format!(
        "#!/bin/sh\nreal_git='{}'\n{body}\nexec \"$real_git\" \"$@\"\n",
        actual.trim().replace('\'', "'\\''")
    );
    let path = f.temp.path().join("bin/git");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn unknown_status_is_not_a_reuse_candidate() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("feature");
    install_git_shim(
        &f,
        "for arg do\n if [ \"$arg\" = status ]; then echo malformed; exit 0; fi\ndone",
    );
    let out = f.run(&["switch", "feature", "--in", "slot"]);
    refused(&out);
    assert!(stderr(&out).contains("Cannot inspect"));
    assert_eq!(f.git(&slot, &["branch", "--show-current"]), "slot");
}

#[test]
fn changed_member_head_after_fetch_is_not_overridden() {
    let f = Fixture::new();
    let slot = f.slot("slot");
    f.branch("topic");
    let target = f.remote_commit("refs/heads/topic", false);
    f.pr(1, "topic", false);
    let marker = f.temp.path().join("mutation");
    install_git_shim(&f, &format!("if [ \"$1\" = fetch ] && [ ! -e {} ]; then\n touch {}\n \"$real_git\" -C {} commit --allow-empty -qm concurrent\nfi", quoted(&marker), quoted(&marker), quoted(&slot)));
    let out = f.run(&["switch", "#1", "--in", "slot"]);
    refused(&out);
    assert!(marker.exists());
    assert_eq!(f.head("topic"), target);
    assert_eq!(f.git(&slot, &["branch", "--show-current"]), "slot");
    assert_eq!(f.git(&slot, &["log", "-1", "--format=%s"]), "concurrent");
    assert!(stderr(&out).contains("changed"));
    assert!(stderr(&out).contains("retained"));
}
