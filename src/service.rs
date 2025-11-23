#![allow(clippy::missing_errors_doc)]
use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::config::HookActions;
use crate::hooks::HookExecutor;
use crate::integrations::git::GitClient;
use crate::integrations::zoxide::ZoxideClient;

/// Worktree service that coordinates git operations, hooks, and zoxide
#[allow(dead_code)]
pub struct WorktreeService<G, H, Z>
where
    G: GitClient,
    H: HookExecutor,
    Z: ZoxideClient,
{
    git_client: G,
    hook_executor: H,
    zoxide_client: Z,
}

impl<G, H, Z> WorktreeService<G, H, Z>
where
    G: GitClient,
    H: HookExecutor,
    Z: ZoxideClient,
{
    /// Create a new worktree service
    #[allow(dead_code)]
    pub const fn new(git_client: G, hook_executor: H, zoxide_client: Z) -> Self {
        Self {
            git_client,
            hook_executor,
            zoxide_client,
        }
    }

    /// Create a new worktree with hooks and zoxide integration
    #[allow(dead_code)]
    ///
    /// # Arguments
    /// * `branch` - Branch name for the new worktree
    /// * `worktree_path` - Path where the worktree will be created
    /// * `start_point` - Optional start point (branch, tag, or commit)
    /// * `repo_root` - Main repository root path
    /// * `hooks` - Hook actions to execute after creation
    /// * `zoxide_enabled` - Whether to add the worktree to zoxide
    ///
    /// # Returns
    /// The path to the created worktree
    pub fn create_worktree(
        &self,
        branch: &str,
        worktree_path: &Path,
        start_point: Option<&str>,
        repo_root: &Path,
        hooks: &HookActions,
        zoxide_enabled: bool,
    ) -> Result<PathBuf> {
        // Create worktree using git
        self.git_client
            .create_worktree(branch, worktree_path, start_point)?;

        // Execute create hooks
        self.hook_executor
            .execute_hooks(hooks, worktree_path, repo_root)?;

        // Add to zoxide if enabled
        if zoxide_enabled {
            self.zoxide_client.add(worktree_path)?;
        }

        Ok(worktree_path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Mock implementations for testing
    struct MockGitClient {
        should_fail: bool,
    }

    impl MockGitClient {
        fn new() -> Self {
            Self { should_fail: false }
        }

        fn with_failure() -> Self {
            Self { should_fail: true }
        }
    }

    impl GitClient for MockGitClient {
        fn create_worktree(
            &self,
            _branch: &str,
            _path: &Path,
            _start_point: Option<&str>,
        ) -> Result<()> {
            if self.should_fail {
                anyhow::bail!("Mock git failure");
            }
            Ok(())
        }

        fn list_worktrees(&self) -> Result<String> {
            Ok(String::new())
        }

        fn remove_worktree(&self, _path: &Path) -> Result<()> {
            Ok(())
        }

        fn remove_branch(&self, _branch: &str) -> Result<()> {
            Ok(())
        }
    }

    struct MockHookExecutor {
        should_fail: bool,
    }

    impl MockHookExecutor {
        fn new() -> Self {
            Self { should_fail: false }
        }

        fn with_failure() -> Self {
            Self { should_fail: true }
        }
    }

    impl HookExecutor for MockHookExecutor {
        fn execute_hooks(
            &self,
            _actions: &HookActions,
            _worktree_path: &Path,
            _source_path: &Path,
        ) -> Result<()> {
            if self.should_fail {
                anyhow::bail!("Mock hook failure");
            }
            Ok(())
        }
    }

    struct MockZoxideClient {
        should_fail: bool,
    }

    impl MockZoxideClient {
        fn new() -> Self {
            Self { should_fail: false }
        }

        fn with_failure() -> Self {
            Self { should_fail: true }
        }
    }

    impl ZoxideClient for MockZoxideClient {
        fn add(&self, _path: &Path) -> Result<()> {
            if self.should_fail {
                anyhow::bail!("Mock zoxide failure");
            }
            Ok(())
        }
    }

    #[test]
    fn test_create_worktree_success() {
        let service = WorktreeService::new(
            MockGitClient::new(),
            MockHookExecutor::new(),
            MockZoxideClient::new(),
        );

        let branch = "feature";
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");
        let hooks = HookActions::default();

        let result = service.create_worktree(
            branch,
            &worktree_path,
            None,
            &repo_root,
            &hooks,
            true, // zoxide enabled
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), worktree_path);
    }

    #[test]
    fn test_create_worktree_with_start_point() {
        let service = WorktreeService::new(
            MockGitClient::new(),
            MockHookExecutor::new(),
            MockZoxideClient::new(),
        );

        let branch = "feature";
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");
        let hooks = HookActions::default();

        let result = service.create_worktree(
            branch,
            &worktree_path,
            Some("main"),
            &repo_root,
            &hooks,
            false, // zoxide disabled
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), worktree_path);
    }

    #[test]
    fn test_create_worktree_git_failure() {
        let service = WorktreeService::new(
            MockGitClient::with_failure(),
            MockHookExecutor::new(),
            MockZoxideClient::new(),
        );

        let branch = "feature";
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");
        let hooks = HookActions::default();

        let result =
            service.create_worktree(branch, &worktree_path, None, &repo_root, &hooks, true);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Mock git failure"));
    }

    #[test]
    fn test_create_worktree_hook_failure() {
        let service = WorktreeService::new(
            MockGitClient::new(),
            MockHookExecutor::with_failure(),
            MockZoxideClient::new(),
        );

        let branch = "feature";
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");
        let hooks = HookActions::default();

        let result =
            service.create_worktree(branch, &worktree_path, None, &repo_root, &hooks, true);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Mock hook failure"));
    }

    #[test]
    fn test_create_worktree_zoxide_failure() {
        let service = WorktreeService::new(
            MockGitClient::new(),
            MockHookExecutor::new(),
            MockZoxideClient::with_failure(),
        );

        let branch = "feature";
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");
        let hooks = HookActions::default();

        let result = service.create_worktree(
            branch,
            &worktree_path,
            None,
            &repo_root,
            &hooks,
            true, // zoxide enabled
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Mock zoxide failure"));
    }

    #[test]
    fn test_create_worktree_zoxide_disabled() {
        let service = WorktreeService::new(
            MockGitClient::new(),
            MockHookExecutor::new(),
            MockZoxideClient::with_failure(), // Will fail if called
        );

        let branch = "feature";
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");
        let hooks = HookActions::default();

        let result = service.create_worktree(
            branch,
            &worktree_path,
            None,
            &repo_root,
            &hooks,
            false, // zoxide disabled - should not call zoxide
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), worktree_path);
    }
}
