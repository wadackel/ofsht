#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use std::fs;
use std::process::Command;

#[test]
fn test_add_uses_main_repo_config_not_worktree_config() {
    let temp = assert_fs::TempDir::new().unwrap();
    let repo_dir = temp.child("test-repo");

    // Initialize git repo
    repo_dir.create_dir_all().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();

    // Configure git user
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();

    // Create initial commit
    fs::write(repo_dir.path().join("README.md"), "test").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();

    // Create main repo config with unique path template
    let main_config_path = repo_dir.path().join(".ofsht.toml");
    let main_config_content = format!(
        r#"
[worktree]
dir = "{}/main-{{branch}}"
"#,
        temp.path().display()
    );
    fs::write(&main_config_path, main_config_content).unwrap();

    // Create first worktree using main config
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.args(["add", "feature-1"])
        .current_dir(repo_dir.path())
        .assert()
        .success();

    // Verify first worktree was created at expected location
    let expected_feature1_path = temp.path().join("main-feature-1");
    assert!(
        expected_feature1_path.exists(),
        "First worktree should exist at {}",
        expected_feature1_path.display()
    );

    // Create different config in the first worktree (should be ignored)
    let worktree_config_path = expected_feature1_path.join(".ofsht.toml");
    let worktree_config_content = format!(
        r#"
[worktree]
dir = "{}/worktree-{{branch}}"
"#,
        temp.path().display()
    );
    fs::write(&worktree_config_path, worktree_config_content).unwrap();

    // Run add command FROM the worktree (should use main repo config, not worktree config)
    let mut cmd = Command::cargo_bin("ofsht").unwrap();
    cmd.args(["add", "feature-2"])
        .current_dir(&expected_feature1_path)
        .assert()
        .success();

    // Verify feature-2 was created at main config location (NOT worktree config location)
    let expected_feature2_path = temp.path().join("main-feature-2");
    let wrong_feature2_path = temp.path().join("worktree-feature-2");

    assert!(
        expected_feature2_path.exists(),
        "Second worktree should exist at {} (using main repo config)",
        expected_feature2_path.display()
    );
    assert!(
        !wrong_feature2_path.exists(),
        "Second worktree should NOT exist at {} (worktree config should be ignored)",
        wrong_feature2_path.display()
    );
}
