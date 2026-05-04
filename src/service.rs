#![allow(clippy::missing_errors_doc)]
use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::integrations::git::GitClient;
use crate::integrations::zoxide::ZoxideClient;

/// Worktree service that coordinates git creation and zoxide registration.
///
/// Hook execution is delegated to the caller via the `on_after_git`
/// callback so the service stays free of UI state (`MultiProgress`,
/// color mode, etc.).
pub struct WorktreeService<G, Z>
where
    G: GitClient,
    Z: ZoxideClient,
{
    git_client: G,
    zoxide_client: Z,
}

impl<G, Z> WorktreeService<G, Z>
where
    G: GitClient,
    Z: ZoxideClient,
{
    pub const fn new(git_client: G, zoxide_client: Z) -> Self {
        Self {
            git_client,
            zoxide_client,
        }
    }

    /// Create a worktree, then run `on_after_git`, then register with
    /// zoxide when enabled.
    ///
    /// The callback runs after `git worktree add` succeeds and before
    /// zoxide registration; it is the caller's hook for printing
    /// progress messages and executing user-defined create hooks.
    /// Returning `Err` from the callback aborts the flow before zoxide
    /// is touched.
    pub fn create_worktree<F>(
        &self,
        branch: &str,
        worktree_path: &Path,
        start_point: Option<&str>,
        repo_root: &Path,
        zoxide_enabled: bool,
        on_after_git: F,
    ) -> Result<PathBuf>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        self.git_client
            .create_worktree(branch, worktree_path, start_point, Some(repo_root))?;

        on_after_git(worktree_path)?;

        if zoxide_enabled {
            self.zoxide_client.add(worktree_path)?;
        }

        Ok(worktree_path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::git::tests::MockGitClient;
    use std::cell::Cell;
    use std::path::PathBuf;

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
        let service = WorktreeService::new(MockGitClient::default(), MockZoxideClient::new());
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");

        let result = service.create_worktree(
            "feature",
            &worktree_path,
            None,
            &repo_root,
            true,
            |_| Ok(()),
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), worktree_path);
    }

    #[test]
    fn test_create_worktree_with_start_point() {
        let service = WorktreeService::new(MockGitClient::default(), MockZoxideClient::new());
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");

        let result = service.create_worktree(
            "feature",
            &worktree_path,
            Some("main"),
            &repo_root,
            false,
            |_| Ok(()),
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), worktree_path);
    }

    #[test]
    fn test_create_worktree_git_failure_skips_callback_and_zoxide() {
        let service = WorktreeService::new(
            MockGitClient {
                create_should_fail: true,
                ..Default::default()
            },
            MockZoxideClient::with_failure(),
        );
        let callback_called = Cell::new(false);
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");

        let result =
            service.create_worktree("feature", &worktree_path, None, &repo_root, true, |_| {
                callback_called.set(true);
                Ok(())
            });

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Mock git create worktree failure"));
        assert!(
            !callback_called.get(),
            "callback must not run when git fails"
        );
    }

    #[test]
    fn test_create_worktree_callback_error_propagates_and_skips_zoxide() {
        let service = WorktreeService::new(
            MockGitClient::default(),
            MockZoxideClient::with_failure(), // would fail if reached
        );
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");

        let result =
            service.create_worktree("feature", &worktree_path, None, &repo_root, true, |_| {
                anyhow::bail!("callback boom")
            });

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("callback boom"));
    }

    #[test]
    fn test_create_worktree_zoxide_failure() {
        let service =
            WorktreeService::new(MockGitClient::default(), MockZoxideClient::with_failure());
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");

        let result = service.create_worktree(
            "feature",
            &worktree_path,
            None,
            &repo_root,
            true, // zoxide enabled
            |_| Ok(()),
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Mock zoxide failure"));
    }

    #[test]
    fn test_create_worktree_zoxide_disabled_skips_zoxide() {
        let service = WorktreeService::new(
            MockGitClient::default(),
            MockZoxideClient::with_failure(), // would fail if reached
        );
        let worktree_path = PathBuf::from("/test/worktree");
        let repo_root = PathBuf::from("/test/repo");

        let result = service.create_worktree(
            "feature",
            &worktree_path,
            None,
            &repo_root,
            false, // zoxide disabled
            |_| Ok(()),
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), worktree_path);
    }
}
