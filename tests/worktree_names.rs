#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::TempDir;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// `myapp` repository under an isolated `HOME`
///
/// ```text
/// ~/dev/myapp/                main
/// ~/dev/myapp-worktrees/wt1/  feature/login
/// ~/dev/myapp-worktrees/wt2/  feature/search
/// ~/tmp/review/               fix/typo        (`with_outside_worktrees`)
/// ~/tmp/check/                detached        (`with_outside_worktrees`)
/// ```
struct Fixture {
    _temp: TempDir,
    home: PathBuf,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        // git reports real paths, while `display_path` strips `HOME` lexically
        let home = temp.path().canonicalize().unwrap();
        let repo = home.join("dev/myapp");
        std::fs::create_dir_all(&repo).unwrap();

        let fixture = Self {
            _temp: temp,
            home,
            repo,
        };
        fixture.git(&["init", "-b", "main"]);
        fixture.git(&["config", "user.name", "Test User"]);
        fixture.git(&["config", "user.email", "test@example.com"]);
        fixture.git(&["commit", "--allow-empty", "-m", "Initial commit"]);
        fixture.add_worktree("feature/login", "dev/myapp-worktrees/wt1");
        fixture.add_worktree("feature/search", "dev/myapp-worktrees/wt2");
        fixture
    }

    fn with_outside_worktrees() -> Self {
        let fixture = Self::new();
        fixture.add_worktree("fix/typo", "tmp/review");
        fixture.add_detached_worktree("tmp/check");
        fixture
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.home.join(relative)
    }

    fn git(&self, args: &[&str]) {
        let output = self.command("git").args(args).output().unwrap();
        assert!(
            output.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn add_worktree(&self, branch: &str, relative: &str) {
        let path = self.path(relative);
        self.git(&["worktree", "add", "-b", branch, path.to_str().unwrap()]);
    }

    fn add_detached_worktree(&self, relative: &str) {
        let path = self.path(relative);
        self.git(&["worktree", "add", "--detach", path.to_str().unwrap()]);
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        self.isolate(&mut command, &self.repo);
        command
    }

    fn isolate(&self, command: &mut Command, cwd: &Path) {
        command
            .current_dir(cwd)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"));
    }

    fn ofsht_in(&self, cwd: &Path, args: &[&str]) -> Output {
        let mut command = Command::cargo_bin("ofsht").unwrap();
        self.isolate(&mut command, cwd);
        command.args(args).output().unwrap()
    }

    fn ofsht(&self, args: &[&str]) -> Output {
        self.ofsht_in(&self.repo, args)
    }

    /// The name column of `ls --show-path`, including `@` for the main worktree
    fn listed_names(&self) -> Vec<String> {
        let output = self.ofsht(&["ls", "--show-path"]);
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| {
                let tokens: Vec<&str> = line
                    .trim_start_matches(['*', ' '])
                    .split_whitespace()
                    .collect();
                // path, hash, then the name unless the branch column comes first
                let name = tokens[2];
                if name.starts_with('[') {
                    String::new()
                } else {
                    name.to_string()
                }
            })
            .collect()
    }

    fn assert_cd(&self, input: &str, expected: &Path) {
        assert_cd_output(&self.ofsht(&["cd", input]), input, expected);
    }

    fn assert_fails(&self, args: &[&str], message: &str) {
        let output = self.ofsht(args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?} should fail");
        assert!(
            stderr.contains(message),
            "{args:?}: expected {message:?} in stderr: {stderr}"
        );
    }
}

fn assert_cd_output(output: &Output, input: &str, expected: &Path) {
    assert!(
        output.status.success(),
        "cd {input}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        expected.to_str().unwrap(),
        "cd {input}"
    );
}

#[test]
fn main_branch_cd_prints_main_worktree() {
    let fixture = Fixture::new();
    fixture.assert_cd("main", &fixture.repo);
}

#[test]
fn main_branch_cd_works_from_linked_worktree() {
    let fixture = Fixture::new();
    let output = fixture.ofsht_in(&fixture.path("dev/myapp-worktrees/wt1"), &["cd", "main"]);
    assert_cd_output(&output, "main", &fixture.repo);
}

#[test]
fn main_branch_rm_is_refused() {
    let fixture = Fixture::new();
    fixture.assert_fails(&["rm", "main"], "Cannot remove main worktree");
    assert!(fixture.repo.exists());
}

#[test]
fn main_branch_rm_keeps_worktrees_in_directories_named_main() {
    let fixture = Fixture::new();
    fixture.add_worktree("other/inside", "dev/myapp-worktrees/main");
    fixture.add_detached_worktree("tmp/main");

    fixture.assert_fails(&["rm", "main"], "Cannot remove main worktree");

    assert!(fixture.path("dev/myapp-worktrees/main").exists());
    assert!(fixture.path("tmp/main").exists());
    fixture.assert_cd("main", &fixture.repo);
}

#[test]
fn resolve_worktree_in_template_directory() {
    let fixture = Fixture::with_outside_worktrees();
    let wt1 = fixture.path("dev/myapp-worktrees/wt1");

    fixture.assert_cd("feature/login", &wt1);
    fixture.assert_cd("wt1", &wt1);
    fixture.assert_cd(wt1.to_str().unwrap(), &wt1);
    fixture.assert_cd("../myapp-worktrees/wt1", &wt1);
}

#[test]
fn resolve_worktree_outside_template_directory() {
    let fixture = Fixture::with_outside_worktrees();
    let review = fixture.path("tmp/review");

    fixture.assert_cd("fix/typo", &review);
    fixture.assert_cd("review", &review);
    fixture.assert_cd(review.to_str().unwrap(), &review);
}

#[test]
fn resolve_detached_worktree_outside_template_directory() {
    let fixture = Fixture::with_outside_worktrees();
    let check = fixture.path("tmp/check");

    fixture.assert_cd("check", &check);
    fixture.assert_cd(check.to_str().unwrap(), &check);
}

#[test]
fn resolve_drops_names_derived_from_common_parent() {
    let fixture = Fixture::with_outside_worktrees();

    fixture.assert_fails(&["cd", "dev/myapp-worktrees/wt1"], "Worktree not found");
    fixture.assert_fails(&["cd", "tmp/review"], "Worktree not found");
    fixture.assert_fails(&["cd", "tmp/check"], "Worktree not found");
}

#[test]
fn resolve_path_relative_to_current_directory() {
    let fixture = Fixture::new();
    let sub = fixture.path("dev/myapp/sub");
    std::fs::create_dir_all(&sub).unwrap();

    // Joined to the root this is `~/myapp-worktrees/wt1`, so only the current directory
    // can resolve it.
    let input = "../../myapp-worktrees/wt1";
    let output = fixture.ofsht_in(&sub, &["cd", input]);

    assert_cd_output(&output, input, &fixture.path("dev/myapp-worktrees/wt1"));
}

#[test]
fn resolve_nested_worktree_by_path_below_root_only() {
    let fixture = Fixture::new();
    fixture.add_worktree("other/branch", "dev/myapp-worktrees/feature/a");

    fixture.assert_cd("feature/a", &fixture.path("dev/myapp-worktrees/feature/a"));
    fixture.assert_fails(&["cd", "a"], "Worktree not found");
}

#[test]
fn resolve_prefers_template_directory_over_same_name_outside() {
    let fixture = Fixture::with_outside_worktrees();
    fixture.add_worktree("feature/review", "dev/myapp-worktrees/review");

    fixture.assert_cd("review", &fixture.path("dev/myapp-worktrees/review"));
    fixture.assert_cd("fix/typo", &fixture.path("tmp/review"));
}

#[test]
fn resolve_reports_ambiguous_directory_name() {
    let fixture = Fixture::new();
    fixture.add_worktree("fix/a", "tmp/a/review");
    fixture.add_worktree("fix/b", "tmp/b/review");

    for command in ["cd", "rm"] {
        let output = fixture.ofsht(&[command, "review"]);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "{command} should fail");
        assert!(
            stderr.contains("Ambiguous worktree name: review"),
            "{stderr}"
        );
        assert!(stderr.contains("~/tmp/a/review [fix/a]"), "{stderr}");
        assert!(stderr.contains("~/tmp/b/review [fix/b]"), "{stderr}");
        assert!(stderr.contains("Specify a path"), "{stderr}");
    }
    assert!(fixture.path("tmp/a/review").exists());
    assert!(fixture.path("tmp/b/review").exists());
}

#[test]
fn resolve_reports_branch_taken_by_another_worktree() {
    let fixture = Fixture::new();
    fixture.add_worktree("slot-1", "dev/myapp-worktrees/slot-1");
    fixture.add_worktree("slot-2", "dev/myapp-worktrees/slot-2");
    let slot_1 = fixture.path("dev/myapp-worktrees/slot-1");
    let slot_2 = fixture.path("dev/myapp-worktrees/slot-2");
    fixture.git(&["-C", slot_2.to_str().unwrap(), "switch", "-qc", "other"]);
    fixture.git(&["-C", slot_1.to_str().unwrap(), "switch", "-q", "slot-2"]);

    for command in ["cd", "rm"] {
        let output = fixture.ofsht(&[command, "slot-2"]);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "{command} should fail");
        assert!(
            stderr.contains("Ambiguous worktree name: slot-2"),
            "{stderr}"
        );
        assert!(
            stderr.contains("~/dev/myapp-worktrees/slot-1 [slot-2]"),
            "{stderr}"
        );
        assert!(
            stderr.contains("~/dev/myapp-worktrees/slot-2 [other]"),
            "{stderr}"
        );
    }
    assert!(slot_1.exists());
    assert!(slot_2.exists());
    assert!(
        fixture
            .listed_names()
            .contains(&"~/dev/myapp-worktrees/slot-2".to_string()),
        "{:?}",
        fixture.listed_names()
    );

    // The path the error asks for removes only the intended worktree
    let output = fixture.ofsht(&["rm", "--keep-branch", slot_2.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(slot_1.exists());
    assert!(!slot_2.exists());
}

#[test]
fn cd_accepts_path_to_main_worktree() {
    let fixture = Fixture::new();
    let wt1 = fixture.path("dev/myapp-worktrees/wt1");

    assert_cd_output(
        &fixture.ofsht_in(&wt1, &["cd", "../../myapp"]),
        "../../myapp",
        &fixture.repo,
    );
    let absolute = fixture.repo.to_str().unwrap();
    assert_cd_output(
        &fixture.ofsht_in(&wt1, &["cd", absolute]),
        absolute,
        &fixture.repo,
    );
}

#[test]
fn resolve_rm_by_directory_name() {
    let fixture = Fixture::with_outside_worktrees();

    assert!(fixture.ofsht(&["rm", "wt1"]).status.success());
    assert!(fixture.ofsht(&["rm", "review"]).status.success());

    assert!(!fixture.path("dev/myapp-worktrees/wt1").exists());
    assert!(!fixture.path("tmp/review").exists());
}

#[test]
fn resolve_rm_never_reaches_main_worktree() {
    let fixture = Fixture::with_outside_worktrees();

    fixture.assert_fails(&["rm", "myapp"], "Worktree not found");
    fixture.assert_fails(
        &["rm", fixture.repo.to_str().unwrap()],
        "Cannot remove main worktree",
    );
    assert!(fixture.repo.exists());
}

#[test]
fn names_without_outside_worktrees() {
    let fixture = Fixture::new();
    assert_eq!(fixture.listed_names(), ["@", "wt1", "wt2"]);
}

#[test]
fn names_stay_short_with_outside_worktrees() {
    let fixture = Fixture::with_outside_worktrees();
    assert_eq!(
        fixture.listed_names(),
        ["@", "wt1", "wt2", "check", "review"]
    );
}

#[test]
fn names_keep_path_below_root_for_nested_worktree() {
    let fixture = Fixture::new();
    fixture.add_worktree("other/branch", "dev/myapp-worktrees/feature/a");
    assert_eq!(fixture.listed_names(), ["@", "feature/a", "wt1", "wt2"]);
}

#[test]
fn names_use_path_for_worktree_sharing_a_name() {
    let fixture = Fixture::with_outside_worktrees();
    fixture.add_worktree("feature/review", "dev/myapp-worktrees/review");
    assert_eq!(
        fixture.listed_names(),
        ["@", "review", "wt1", "wt2", "check", "~/tmp/review"]
    );
}

#[test]
fn ls_marks_current_worktree_from_subdirectory() {
    let fixture = Fixture::new();
    let subdirectory = fixture.path("dev/myapp-worktrees/wt1/sub");
    std::fs::create_dir_all(&subdirectory).unwrap();

    let output = fixture.ofsht_in(&subdirectory, &["ls", "--show-path"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let marked: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("* "))
        .collect();

    assert_eq!(marked.len(), 1, "{stdout}");
    assert!(marked[0].contains("~/dev/myapp-worktrees/wt1 "), "{stdout}");
}
