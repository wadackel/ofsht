use assert_fs::prelude::*;
use assert_fs::TempDir;
use clap::CommandFactory;
use ofsht::cli::Cli;
use serial_test::serial;
use std::ffi::OsString;
use std::process::Command;

/// Helper to set up a temporary git repository with branches and remotes
struct GitTestRepo {
    dir: TempDir,
}

impl GitTestRepo {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();

        // Initialize git repo with main branch to be deterministic
        Self::run_git(&dir, &["init", "-b", "main"]);
        Self::run_git(&dir, &["config", "user.email", "test@example.com"]);
        Self::run_git(&dir, &["config", "user.name", "Test User"]);

        // Create initial commit
        dir.child("README.md").write_str("# Test").unwrap();
        Self::run_git(&dir, &["add", "README.md"]);
        Self::run_git(&dir, &["commit", "-m", "Initial commit"]);

        Self { dir }
    }

    pub fn run_git(dir: &TempDir, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("Failed to run git command");

        assert!(
            output.status.success(),
            "Git command failed: git {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn create_branch(&self, name: &str) {
        Self::run_git(&self.dir, &["branch", name]);
    }

    fn create_remote(&self, name: &str) {
        // Create a bare repository to simulate a remote
        let remote_path = self.dir.child("remote.git");
        std::fs::create_dir(remote_path.path()).expect("Failed to create remote dir");

        let output = Command::new("git")
            .args(["init", "--bare"])
            .current_dir(remote_path.path())
            .output()
            .expect("Failed to run git init --bare");

        assert!(
            output.status.success(),
            "Failed to create bare repo: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        Self::run_git(
            &self.dir,
            &["remote", "add", name, remote_path.path().to_str().unwrap()],
        );
    }

    fn create_remote_branch(&self, remote: &str, branch: &str) {
        // Push a branch to the remote
        Self::run_git(&self.dir, &["push", remote, &format!("main:{branch}")]);
        // Fetch to create remote-tracking branches
        Self::run_git(&self.dir, &["fetch", remote]);
    }

    fn create_worktree(&self, branch: &str) {
        let worktree_path = self.dir.child(format!("worktree-{branch}"));
        Self::run_git(
            &self.dir,
            &[
                "worktree",
                "add",
                "-b",
                branch,
                worktree_path.path().to_str().unwrap(),
            ],
        );
    }

    fn path(&self) -> &std::path::Path {
        self.dir.path()
    }
}

/// Helper to get completion candidates for a given command line
/// Uses `clap_complete::engine::complete` directly for fast, reliable testing
fn get_completions(args: &[&str], git_repo_dir: &std::path::Path) -> Vec<String> {
    use clap_complete::engine::complete;

    // Convert args to OsString vector, including binary name
    // The complete() function expects the full command line including the binary
    let mut os_args: Vec<OsString> = vec![OsString::from("ofsht")];
    os_args.extend(args.iter().map(OsString::from));

    // The index of the argument being completed (last argument)
    let current_index = os_args.len() - 1;

    // Change to the git repo directory so git commands work
    let original_dir = std::env::current_dir().expect("Failed to get current dir");
    std::env::set_current_dir(git_repo_dir).expect("Failed to change to git repo dir");

    // Call the completion engine directly
    let completions =
        complete(&mut Cli::command(), os_args, current_index, None).unwrap_or_default();

    // Restore original directory (only if it still exists)
    // This is important for tests that may delete temporary directories
    if original_dir.exists() {
        std::env::set_current_dir(original_dir).ok();
    }

    // Extract just the completion values
    completions
        .into_iter()
        .map(|candidate| candidate.get_value().to_string_lossy().to_string())
        .collect()
}

// Note: The branch argument for `add` command does NOT have completion
// because it's meant for a new branch name, not an existing ref.
// Completion is only available for the start_point argument.

#[test]
#[serial]
fn test_add_start_point_completion_includes_refs() {
    let repo = GitTestRepo::new();

    repo.create_branch("develop");
    repo.create_remote("origin");
    repo.create_remote_branch("origin", "main");

    // Create a tag
    GitTestRepo::run_git(&repo.dir, &["tag", "v1.0.0"]);

    // Test: ofsht add feature-branch <TAB>
    let candidates = get_completions(&["add", "new-feature", ""], repo.path());

    // Should show branches and tags as start point options
    assert!(
        candidates.contains(&"develop".to_string()),
        "Expected develop in candidates: {candidates:?}"
    );
    assert!(
        candidates.contains(&"origin/main".to_string()),
        "Expected origin/main in candidates: {candidates:?}"
    );
    assert!(
        candidates.contains(&"v1.0.0".to_string()),
        "Expected v1.0.0 tag in candidates: {candidates:?}"
    );
}

#[test]
#[serial]
fn test_create_start_point_completion_includes_refs() {
    let repo = GitTestRepo::new();

    repo.create_branch("feature");
    repo.create_remote("origin");
    repo.create_remote_branch("origin", "develop");

    // Create a tag
    GitTestRepo::run_git(&repo.dir, &["tag", "v2.0.0"]);

    // Test: ofsht create new-branch <TAB>
    let candidates = get_completions(&["create", "new-branch", ""], repo.path());

    // Should show branches and tags as start point options
    assert!(
        candidates.contains(&"feature".to_string()),
        "Expected feature in candidates: {candidates:?}"
    );
    assert!(
        candidates.contains(&"origin/develop".to_string()),
        "Expected origin/develop in candidates: {candidates:?}"
    );
    assert!(
        candidates.contains(&"v2.0.0".to_string()),
        "Expected v2.0.0 tag in candidates: {candidates:?}"
    );
}

#[test]
#[serial]
fn test_rm_completion_shows_worktrees_and_flags() {
    let repo = GitTestRepo::new();

    repo.create_worktree("feature-1");
    repo.create_worktree("feature-2");

    // Test: ofsht rm <TAB>
    let candidates = get_completions(&["rm", ""], repo.path());

    // Should show worktrees
    assert!(
        candidates.contains(&"feature-1".to_string()),
        "Expected feature-1 in candidates: {candidates:?}"
    );
    assert!(
        candidates.contains(&"feature-2".to_string()),
        "Expected feature-2 in candidates: {candidates:?}"
    );
    assert!(
        candidates.contains(&"@".to_string()),
        "Expected @ in candidates: {candidates:?}"
    ); // main worktree marker

    // May also show flags (standard CLI behavior, matches git/cargo/docker)
    // This is acceptable - users can filter by typing the first character
    // No assertion on flags, as this is implementation detail
}

#[test]
#[serial]
fn test_rm_completion_shows_flags_with_dash() {
    let repo = GitTestRepo::new();

    repo.create_worktree("feature");

    // Test: ofsht rm -<TAB>
    let candidates = get_completions(&["rm", "-"], repo.path());

    // Should show flags when dash is entered
    assert!(
        candidates.iter().any(|c| c.starts_with('-')),
        "Should contain flags in candidates: {candidates:?}"
    );
}
