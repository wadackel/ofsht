#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Git client interface for worktree operations
#[allow(dead_code)]
pub trait GitClient {
    /// Create a new worktree
    ///
    /// # Arguments
    /// * `branch` - Branch name to create
    /// * `path` - Path where the worktree will be created
    /// * `start_point` - Optional start point (branch, tag, or commit)
    fn create_worktree(&self, branch: &str, path: &Path, start_point: Option<&str>) -> Result<()>;

    /// List all worktrees in porcelain format
    fn list_worktrees(&self) -> Result<String>;

    /// Remove a worktree
    fn remove_worktree(&self, path: &Path) -> Result<()>;

    /// Remove a branch
    fn remove_branch(&self, branch: &str) -> Result<()>;
}

/// Real git implementation
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct RealGitClient;

impl GitClient for RealGitClient {
    fn create_worktree(&self, branch: &str, path: &Path, start_point: Option<&str>) -> Result<()> {
        let mut cmd = Command::new("git");
        cmd.arg("worktree")
            .arg("add")
            .arg("-b")
            .arg(branch)
            .arg(path);

        if let Some(start) = start_point {
            cmd.arg(start);
        }

        let output = cmd.output().context("Failed to execute git worktree add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git worktree add failed: {stderr}");
        }

        Ok(())
    }

    fn list_worktrees(&self) -> Result<String> {
        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .output()
            .context("Failed to execute git worktree list")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git worktree list failed: {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(stdout)
    }

    fn remove_worktree(&self, path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["worktree", "remove", &path.display().to_string()])
            .output()
            .context("Failed to execute git worktree remove")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git worktree remove failed: {stderr}");
        }

        Ok(())
    }

    fn remove_branch(&self, branch: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["branch", "-D", branch])
            .output()
            .context("Failed to execute git branch -D")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git branch -D failed: {stderr}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Mock git client for testing
    struct MockGitClient {
        create_should_fail: bool,
        list_output: String,
        remove_worktree_should_fail: bool,
        remove_branch_should_fail: bool,
    }

    impl MockGitClient {
        fn new() -> Self {
            Self {
                create_should_fail: false,
                list_output: String::new(),
                remove_worktree_should_fail: false,
                remove_branch_should_fail: false,
            }
        }

        fn with_list_output(mut self, output: String) -> Self {
            self.list_output = output;
            self
        }
    }

    impl GitClient for MockGitClient {
        fn create_worktree(
            &self,
            _branch: &str,
            _path: &Path,
            _start_point: Option<&str>,
        ) -> Result<()> {
            if self.create_should_fail {
                anyhow::bail!("Mock git create worktree failure");
            }
            Ok(())
        }

        fn list_worktrees(&self) -> Result<String> {
            Ok(self.list_output.clone())
        }

        fn remove_worktree(&self, _path: &Path) -> Result<()> {
            if self.remove_worktree_should_fail {
                anyhow::bail!("Mock git remove worktree failure");
            }
            Ok(())
        }

        fn remove_branch(&self, _branch: &str) -> Result<()> {
            if self.remove_branch_should_fail {
                anyhow::bail!("Mock git remove branch failure");
            }
            Ok(())
        }
    }

    #[test]
    fn test_mock_git_client_create_worktree_success() {
        let client = MockGitClient::new();
        let path = PathBuf::from("/test/worktree");
        let result = client.create_worktree("feature", &path, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_git_client_create_worktree_with_start_point() {
        let client = MockGitClient::new();
        let path = PathBuf::from("/test/worktree");
        let result = client.create_worktree("feature", &path, Some("main"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_git_client_list_worktrees() {
        let expected_output = "worktree /path/to/worktree\nbranch refs/heads/main\n";
        let client = MockGitClient::new().with_list_output(expected_output.to_string());
        let result = client.list_worktrees();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_output);
    }

    #[test]
    fn test_mock_git_client_remove_worktree_success() {
        let client = MockGitClient::new();
        let path = PathBuf::from("/test/worktree");
        let result = client.remove_worktree(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_git_client_remove_branch_success() {
        let client = MockGitClient::new();
        let result = client.remove_branch("feature");
        assert!(result.is_ok());
    }
}
