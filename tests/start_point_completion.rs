use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// Helper to run git commands and assert success
fn run_git(dir: &PathBuf, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("Failed to run git command");

    assert!(
        output.status.success(),
        "Git command failed: git {}\nstderr: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Helper to create a temporary git repository with branches and tags for testing completion
fn setup_git_repo() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let repo_path = temp_dir.path().to_path_buf();

    // Initialize git repo
    run_git(&repo_path, &["init"]);

    // Configure git user
    run_git(&repo_path, &["config", "user.name", "Test User"]);
    run_git(&repo_path, &["config", "user.email", "test@example.com"]);

    // Create initial commit
    fs::write(repo_path.join("README.md"), "# Test Repo\n").expect("Failed to write README");
    run_git(&repo_path, &["add", "README.md"]);
    run_git(&repo_path, &["commit", "-m", "Initial commit"]);

    // Create a branch
    run_git(&repo_path, &["branch", "develop"]);

    // Create tags
    run_git(&repo_path, &["tag", "v1.0.0"]);
    run_git(&repo_path, &["tag", "v2.0.0"]);

    (temp_dir, repo_path)
}

#[test]
fn test_start_point_completion_includes_branches_and_tags() {
    let (_temp_dir, repo_path) = setup_git_repo();

    // Test completion for the start_point argument (3rd argument, index 3)
    // Command line being completed: "ofsht add feature-branch <TAB>"
    let output = Command::new(env!("CARGO_BIN_EXE_ofsht"))
        .env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "3")
        .env("_CLAP_IFS", "\n")
        .args(["--", "ofsht", "add", "feature-branch", ""])
        .current_dir(&repo_path)
        .output()
        .expect("Failed to execute command");

    assert!(
        output.status.success(),
        "Command should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // The completion should include branches and tags
    assert!(
        stdout.contains("develop"),
        "Completion should include 'develop' branch. Output: {stdout}"
    );
    assert!(
        stdout.contains("v1.0.0"),
        "Completion should include 'v1.0.0' tag. Output: {stdout}"
    );
    assert!(
        stdout.contains("v2.0.0"),
        "Completion should include 'v2.0.0' tag. Output: {stdout}"
    );
}

#[test]
fn test_start_point_completion_filters_by_prefix() {
    let (_temp_dir, repo_path) = setup_git_repo();

    // Test completion with prefix "v" (should match tags starting with v)
    let output = Command::new(env!("CARGO_BIN_EXE_ofsht"))
        .env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "3")
        .env("_CLAP_IFS", "\n")
        .args(["--", "ofsht", "add", "feature-branch", "v"])
        .current_dir(&repo_path)
        .output()
        .expect("Failed to execute command");

    assert!(
        output.status.success(),
        "Command should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should include tags starting with 'v'
    assert!(
        stdout.contains("v1.0.0"),
        "Completion should include 'v1.0.0' tag. Output: {stdout}"
    );
    assert!(
        stdout.contains("v2.0.0"),
        "Completion should include 'v2.0.0' tag. Output: {stdout}"
    );

    // Should NOT include 'develop' since it doesn't start with 'v'
    assert!(
        !stdout.contains("develop"),
        "Completion should NOT include 'develop' branch (doesn't match prefix 'v'). Output: {stdout}"
    );
}
