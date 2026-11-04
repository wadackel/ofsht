#![allow(deprecated)]

use assert_fs::TempDir;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    temp: TempDir,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::create(true)
    }
    fn create(commit: bool) -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().canonicalize().unwrap().join("app");
        std::fs::create_dir(&repo).unwrap();
        let f = Self { temp, repo };
        f.git(&f.repo, &["init", "-qb", "main"]);
        if commit {
            std::fs::write(f.repo.join("file"), "base\n").unwrap();
            f.git(&f.repo, &["add", "file"]);
            f.git(&f.repo, &["commit", "-qm", "base"]);
        }
        f
    }
    fn command(&self, program: &Path, dir: &Path) -> Command {
        let mut c = Command::new(program);
        c.current_dir(dir)
            .env("HOME", self.temp.path())
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .env_remove("TMUX")
            .env_remove("COMPLETE");
        c
    }
    fn git_result(&self, dir: &Path, args: &[&str]) -> Output {
        self.command(Path::new("git"), dir)
            .args(args)
            .output()
            .unwrap()
    }
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git_result(dir, args);
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout)
            .unwrap()
            .trim_end_matches('\n')
            .to_string()
    }
    fn add(&self, name: &str, branch: &str) -> PathBuf {
        let path = self.repo.parent().unwrap().join("pool").join(name);
        self.git(
            &self.repo,
            &["worktree", "add", "-qb", branch, path.to_str().unwrap()],
        );
        path
    }
    fn cli(&self, dir: &Path, args: &[&str]) -> Command {
        let mut cmd = self.command(&assert_cmd::cargo::cargo_bin("ofsht"), dir);
        cmd.args(args);
        cmd
    }
    fn run(&self, args: &[&str]) -> Output {
        self.cli(&self.repo, args).output().unwrap()
    }
    fn json(&self, status: bool) -> Value {
        let args = if status {
            vec!["ls", "--json", "--status"]
        } else {
            vec!["ls", "--json"]
        };
        json_output(&self.run(&args))
    }
    fn conflict(&self, kind: &str) -> PathBuf {
        let path = self.add("slot", "feature");
        std::fs::write(self.repo.join("file"), "main\n").unwrap();
        self.git(&self.repo, &["commit", "-qam", "main"]);
        std::fs::write(path.join("file"), "feature\n").unwrap();
        self.git(&path, &["commit", "-qam", "feature"]);
        let args = match kind {
            "rebase-merge" => vec!["rebase", "--merge", "main"],
            "rebase-apply" => vec!["rebase", "--apply", "main"],
            "merge" => vec!["merge", "main"],
            _ => unreachable!(),
        };
        assert!(!self.git_result(&path, &args).status.success());
        path
    }
}

fn json_output(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.stdout.contains(&27));
    serde_json::from_slice(&out.stdout).unwrap()
}
fn row<'a>(value: &'a Value, path: &Path) -> &'a Value {
    value
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["path"] == path.to_str().unwrap())
        .unwrap()
}

#[test]
fn json_has_full_identity_and_nullable_optional_status() {
    let f = Fixture::new();
    let path = f.add("slot", "feature");
    let listing = f.json(false);
    let main = row(&listing, &f.repo);
    assert_eq!(main["name"], "@");
    assert_eq!(main["branch"], "main");
    assert_eq!(main["head"], f.git(&f.repo, &["rev-parse", "HEAD"]));
    assert_eq!(main["is_main"], true);
    assert_eq!(main["is_current"], true);
    for key in [
        "operation",
        "operation_branch",
        "locked",
        "lock_reason",
        "prunable",
        "prune_reason",
        "missing",
        "dirty",
        "upstream",
        "ahead",
        "behind",
        "error",
    ] {
        assert!(
            main["state"].as_object().unwrap().contains_key(key),
            "Missing state key {key}"
        );
    }
    let linked = row(&listing, &path);
    assert_eq!(linked["name"], "slot");
    assert_eq!(linked["is_main"], false);
    assert!(linked["state"]["dirty"].is_null());
    assert!(linked["state"]["ahead"].is_null());
    let table = f.run(&["ls", "--show-path"]);
    let text = String::from_utf8_lossy(&table.stdout);
    let cells: Vec<_> = text.lines().next().unwrap().split_whitespace().collect();
    assert_eq!(cells[3], "@");
    assert_eq!(cells[4], "[main]");
    assert_eq!(
        String::from_utf8(f.run(&["ls"]).stdout).unwrap(),
        "@\nfeature\n"
    );
    let out = f.run(&["--color=always", "ls", "--json"]);
    assert_eq!(json_output(&out), listing);
    std::fs::create_dir(path.join("sub")).unwrap();
    let inside = json_output(
        &f.cli(&path.join("sub"), &["ls", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(row(&inside, &path)["is_current"], true);
    assert_eq!(row(&inside, &f.repo)["is_current"], false);
}

#[test]
fn lock_prune_missing_and_dirty_are_distinct() {
    let f = Fixture::new();
    let dirty = f.add("dirty", "dirty");
    let locked = f.add("locked", "locked");
    let missing = f.add("gone", "gone");
    std::fs::write(dirty.join("untracked"), "work").unwrap();
    f.git(
        &f.repo,
        &[
            "worktree",
            "lock",
            "--reason",
            "keep\nthis",
            locked.to_str().unwrap(),
        ],
    );
    std::fs::remove_dir_all(&missing).unwrap();
    let listing = f.json(true);
    assert_eq!(row(&listing, &dirty)["state"]["dirty"], true);
    assert_eq!(row(&listing, &locked)["state"]["locked"], true);
    assert_eq!(row(&listing, &locked)["state"]["lock_reason"], "keep\nthis");
    assert_eq!(row(&listing, &missing)["state"]["prunable"], true);
    assert_eq!(row(&listing, &missing)["state"]["missing"], true);
    assert!(row(&listing, &missing)["state"]["dirty"].is_null());
    let table = String::from_utf8(f.run(&["ls", "--status"]).stdout).unwrap();
    assert!(
        table.contains("dirty") && table.contains("locked") && table.contains("prunable"),
        "{table}"
    );
}

#[test]
fn actual_operations_are_visible_and_rebase_branch_navigates() {
    for kind in ["rebase-merge", "rebase-apply", "merge"] {
        let f = Fixture::new();
        let path = f.conflict(kind);
        let listing = f.json(true);
        let entry = row(&listing, &path);
        assert_eq!(
            entry["state"]["operation"],
            if kind == "merge" { "merge" } else { "rebase" }
        );
        assert_eq!(entry["state"]["dirty"], true);
        if kind != "merge" {
            assert!(entry["branch"].is_null());
            assert_eq!(entry["state"]["operation_branch"], "feature");
        }
        let out = f.run(&["cd", "feature"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            format!("{}\n", path.display())
        );
        let table = String::from_utf8(f.run(&["ls", "--show-path"]).stdout).unwrap();
        assert!(
            table.contains(if kind == "merge" {
                "[feature (merging)]"
            } else {
                "[feature (rebasing)]"
            }),
            "{table}"
        );
        let refused = f.run(&["rm", "--dry-run", "slot"]);
        assert!(!refused.status.success());
        assert!(path.exists());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("in progress"));
    }
}

#[test]
fn operation_alias_participates_in_ambiguity() {
    let f = Fixture::new();
    f.conflict("rebase-merge");
    f.add("feature", "other");
    let out = f.run(&["cd", "feature"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Ambiguous worktree name"));
}

#[test]
fn bisect_restores_logical_branch_without_claiming_symbolic_head() {
    let f = Fixture::new();
    let path = f.add("slot", "feature");
    for n in 0..2 {
        std::fs::write(path.join("file"), n.to_string()).unwrap();
        f.git(&path, &["commit", "-qam", "change"]);
    }
    f.git(&path, &["bisect", "start", "HEAD", "HEAD~2"]);
    let listing = f.json(false);
    let entry = row(&listing, &path);
    assert!(entry["branch"].is_null());
    assert_eq!(entry["state"]["operation"], "bisect");
    assert_eq!(entry["state"]["operation_branch"], "feature");
    let out = f.run(&["cd", "feature"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{}\n", path.display())
    );
}

#[test]
fn divergence_counts_are_local_and_absent_without_upstream() {
    let f = Fixture::new();
    let path = f.add("slot", "feature");
    f.git(&path, &["branch", "--set-upstream-to", "main"]);
    f.git(&path, &["commit", "--allow-empty", "-qm", "ahead"]);
    f.git(&f.repo, &["commit", "--allow-empty", "-qm", "behind"]);
    let listing = f.json(true);
    let entry = row(&listing, &path);
    assert_eq!(entry["state"]["ahead"], 1);
    assert_eq!(entry["state"]["behind"], 1);
    assert_eq!(entry["state"]["upstream"], "main");
    assert_eq!(entry["state"]["dirty"], false);
    assert!(row(&listing, &f.repo)["state"]["ahead"].is_null());
}

#[test]
fn unrelated_recreated_repository_never_looks_clean() {
    let f = Fixture::new();
    let path = f.add("slot", "feature");
    std::fs::remove_dir_all(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    f.git(&path, &["init", "-qb", "unrelated"]);
    f.git(&path, &["commit", "--allow-empty", "-qm", "other repo"]);
    let listing = f.json(true);
    let state = &row(&listing, &path)["state"];
    assert!(state["dirty"].is_null());
    assert!(state["operation_branch"].is_null());
    assert!(state["error"].as_str().unwrap().contains("repository"));
}

#[cfg(unix)]
#[test]
fn same_repository_alias_never_imports_another_worktrees_status() {
    let f = Fixture::new();
    let path = f.add("slot", "feature");
    std::fs::rename(&path, f.temp.path().join("saved-slot")).unwrap();
    std::os::unix::fs::symlink(&f.repo, &path).unwrap();
    let listing = f.json(true);
    let state = &row(&listing, &path)["state"];
    assert!(state["dirty"].is_null());
    assert!(state["error"].is_string());
}

#[test]
fn json_preserves_unusual_utf8_paths_and_unborn_head_is_null() {
    let f = Fixture::new();
    let path = f.add("slot\twith\n\"quote \n", "feature");
    let listing = f.json(false);
    assert_eq!(row(&listing, &path)["path"], path.to_str().unwrap());
    let subdirectory = path.join("sub directory");
    std::fs::create_dir(&subdirectory).unwrap();
    for directory in [&path, &subdirectory] {
        let listing = json_output(&f.cli(directory, &["ls", "--json"]).output().unwrap());
        assert_eq!(row(&listing, &path)["is_current"], true);
        assert_eq!(row(&listing, &f.repo)["is_current"], false);
    }
    let unborn = Fixture::create(false);
    let listing = unborn.json(true);
    assert_eq!(listing[0]["branch"], "main");
    assert!(listing[0]["head"].is_null());
    assert_eq!(listing[0]["state"]["dirty"], false);
}

#[test]
fn rename_source_cannot_forge_status_headers() {
    let f = Fixture::new();
    let old = "# branch.ab +99 -88";
    std::fs::write(f.repo.join(old), "contents").unwrap();
    f.git(&f.repo, &["add", "."]);
    f.git(&f.repo, &["commit", "-qm", "file"]);
    let path = f.add("slot", "feature");
    f.git(&path, &["branch", "--set-upstream-to", "main"]);
    f.git(&path, &["mv", old, "new-name"]);
    let listing = f.json(true);
    let state = &row(&listing, &path)["state"];
    assert_eq!(state["dirty"], true);
    assert_eq!(state["ahead"], 0);
    assert_eq!(state["behind"], 0);
}

#[cfg(unix)]
fn git_shim(f: &Fixture, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let output = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let actual = String::from_utf8(output.stdout).unwrap();
    let bin = f.temp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = format!("#!/bin/sh\nfor arg do\n if [ \"$arg\" = status ]; then\n{body}\n fi\ndone\nexec '{}' \"$@\"\n", actual.trim().replace('\'', "'\\''"));
    std::fs::write(bin.join("git"), script).unwrap();
    std::fs::set_permissions(bin.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

#[cfg(unix)]
#[test]
fn status_is_opt_in_and_failure_is_unknown() {
    let f = Fixture::new();
    let slot = f.add("slot", "feature");
    let marker = f.temp.path().join("status-called");
    let search_path = git_shim(
        &f,
        &format!(
            "touch '{}'; echo status-probe-failure >&2; exit 7",
            marker.display()
        ),
    );
    for args in [vec!["ls"], vec!["ls", "--show-path"], vec!["ls", "--json"]] {
        let out = f
            .cli(&f.repo, &args)
            .env("PATH", &search_path)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(!marker.exists());
    }
    let out = f
        .cli(&f.repo, &["ls", "--json", "--status"])
        .env("PATH", &search_path)
        .output()
        .unwrap();
    let listing = json_output(&out);
    let state = &row(&listing, &slot)["state"];
    assert!(marker.exists());
    assert!(state["dirty"].is_null());
    assert!(state["error"]
        .as_str()
        .unwrap()
        .contains("status-probe-failure"));
}

#[cfg(unix)]
#[test]
fn malformed_successful_status_is_not_clean() {
    use std::fmt::Write;
    let f = Fixture::new();
    let head = f.git(&f.repo, &["rev-parse", "HEAD"]);
    let header = format!("# branch.oid {head}\0# branch.head main\0");
    for bytes in [
        String::new(),
        format!("{header}garbage\0"),
        format!("{header}# branch.ab +oops -1\0"),
        format!("{header}1 invalid\0"),
    ] {
        let mut encoded = String::new();
        for byte in bytes.bytes() {
            write!(&mut encoded, "\\{byte:03o}").unwrap();
        }
        let search_path = git_shim(&f, &format!("printf '{encoded}'; exit 0"));
        let out = f
            .cli(&f.repo, &["ls", "--json", "--status"])
            .env("PATH", search_path)
            .output()
            .unwrap();
        let value = json_output(&out);
        assert!(value[0]["state"]["dirty"].is_null());
        assert!(value[0]["state"]["error"].is_string());
    }
}

#[test]
fn dirty_submodules_override_ignore_configuration() {
    let f = Fixture::new();
    let child = Fixture::new();
    let slot = f.add("slot", "feature");
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
    assert_eq!(row(&f.json(true), &slot)["state"]["dirty"], false);
    std::fs::write(slot.join("module/untracked"), "work").unwrap();
    assert_eq!(row(&f.json(true), &slot)["state"]["dirty"], true);
}

#[test]
fn missing_upstream_has_no_divergence_counts() {
    let f = Fixture::new();
    let slot = f.add("slot", "feature");
    f.git(&f.repo, &["branch", "upstream"]);
    f.git(&slot, &["branch", "--set-upstream-to", "upstream"]);
    f.git(&f.repo, &["branch", "-D", "upstream"]);
    let listing = f.json(true);
    let state = &row(&listing, &slot)["state"];
    assert!(state["ahead"].is_null());
    assert!(state["behind"].is_null());
    assert!(state["error"].is_null());
    assert_eq!(state["dirty"], false);
}

#[test]
fn main_rebase_navigation_and_detached_display() {
    let f = Fixture::new();
    let slot = f.conflict("rebase-merge");
    f.git(&slot, &["rebase", "--abort"]);
    assert!(!f
        .git_result(&f.repo, &["rebase", "--merge", "feature"])
        .status
        .success());
    let listing = f.json(false);
    assert!(listing[0]["branch"].is_null());
    assert_eq!(listing[0]["state"]["operation_branch"], "main");
    let out = f.run(&["cd", "main"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{}\n", f.repo.display())
    );
    f.git(&f.repo, &["rebase", "--abort"]);
    f.git(&f.repo, &["switch", "--detach"]);
    let table = String::from_utf8(f.run(&["ls", "--show-path"]).stdout).unwrap();
    assert!(table.lines().next().unwrap().contains("[detached]"));
    assert_eq!(
        String::from_utf8(f.run(&["ls"]).stdout).unwrap(),
        "@\nfeature\n"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn json_rejects_non_utf8_instead_of_replacing_path_bytes() {
    use std::os::unix::ffi::OsStrExt;
    let f = Fixture::new();
    let path = f
        .repo
        .parent()
        .unwrap()
        .join(std::ffi::OsStr::from_bytes(b"slot\xff"));
    let out = f
        .command(Path::new("git"), &f.repo)
        .args(["worktree", "add", "--detach"])
        .arg(path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = f.run(&["ls", "--json"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("UTF-8"));
}

#[test]
fn real_picker_items_show_operations_and_availability() {
    let f = Fixture::new();
    let slot = f.conflict("rebase-merge");
    let porcelain = f.git(&f.repo, &["worktree", "list", "--porcelain"]);
    let items = ofsht::integrations::fzf::build_worktree_items(&porcelain, None);
    assert!(items[0].display.contains("[main]"));
    let item = items
        .iter()
        .find(|item| item.value == slot.to_str().unwrap())
        .unwrap();
    assert!(item.display.contains("[feature (rebasing)]"));
    assert!(item.display.contains("dirty"));
}

#[test]
fn malformed_operation_branch_is_unknown_but_detached_rebase_is_valid() {
    let f = Fixture::new();
    let slot = f.conflict("rebase-merge");
    let git_dir = PathBuf::from(f.git(&slot, &["rev-parse", "--absolute-git-dir"]));
    std::fs::write(
        git_dir.join("rebase-merge/head-name"),
        "refs/heads/false branch\n",
    )
    .unwrap();
    let value = f.json(false);
    let state = &row(&value, &slot)["state"];
    assert!(state["error"].is_string());
    assert!(state["operation_branch"].is_null());
    std::fs::write(
        git_dir.join("rebase-merge/head-name"),
        "refs/heads/feature\n",
    )
    .unwrap();
    f.git(&slot, &["rebase", "--abort"]);
    f.git(&slot, &["switch", "--detach"]);
    assert!(!f
        .git_result(&slot, &["rebase", "--merge", "main"])
        .status
        .success());
    let value = f.json(false);
    let state = &row(&value, &slot)["state"];
    assert!(state["error"].is_null());
    assert!(state["operation_branch"].is_null());
    assert_eq!(state["operation"], "rebase");
}
