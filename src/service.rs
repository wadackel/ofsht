#![allow(clippy::missing_errors_doc)]
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::integrations::git::GitClient;
use crate::integrations::zoxide::ZoxideClient;

/// Request describing where and how to create a worktree.
///
/// Borrow-based: the caller owns `branch` / `repo_root` / `path_template`
/// for the duration of the `WorktreeService::create` call.
pub struct CreateWorktreeRequest<'a> {
    pub branch: &'a str,
    pub start_point: Option<&'a str>,
    pub repo_root: &'a Path,
    pub path_template: &'a str,
    pub zoxide_enabled: bool,
}

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

    /// Create a worktree from `req`: expand the path template, run
    /// `git worktree add`, invoke `on_after_git` (typically to execute
    /// user hooks), then register with zoxide when enabled.
    ///
    /// Returns the worktree path the service computed (the same value
    /// passed to `on_after_git`). No canonicalization is performed; the
    /// caller is responsible for normalization at any output boundary.
    pub fn create<F>(&self, req: &CreateWorktreeRequest<'_>, on_after_git: F) -> Result<PathBuf>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        let repo_name = req
            .repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .context("Failed to get repository name")?;

        #[allow(clippy::literal_string_with_formatting_args)]
        let expanded = req
            .path_template
            .replace("{repo}", repo_name)
            .replace("{branch}", req.branch);

        let worktree_path = if expanded.starts_with('/') {
            PathBuf::from(&expanded)
        } else {
            req.repo_root.join(&expanded)
        };

        self.git_client.create_worktree(
            req.branch,
            &worktree_path,
            req.start_point,
            Some(req.repo_root),
        )?;

        on_after_git(&worktree_path)?;

        if req.zoxide_enabled {
            self.zoxide_client.add(&worktree_path)?;
        }

        Ok(worktree_path)
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

    fn make_req<'a>(
        branch: &'a str,
        repo_root: &'a Path,
        path_template: &'a str,
        zoxide_enabled: bool,
    ) -> CreateWorktreeRequest<'a> {
        CreateWorktreeRequest {
            branch,
            start_point: None,
            repo_root,
            path_template,
            zoxide_enabled,
        }
    }

    #[test]
    fn test_create_success() {
        let service = WorktreeService::new(MockGitClient::default(), MockZoxideClient::new());
        let repo_root = PathBuf::from("/test/repo");
        let req = make_req("feature", &repo_root, "../{repo}-worktrees/{branch}", true);

        let result = service.create(&req, |_| Ok(()));

        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/test/repo/../repo-worktrees/feature")
        );
    }

    #[test]
    fn test_create_with_start_point() {
        let service = WorktreeService::new(MockGitClient::default(), MockZoxideClient::new());
        let repo_root = PathBuf::from("/test/repo");
        let req = CreateWorktreeRequest {
            branch: "feature",
            start_point: Some("main"),
            repo_root: &repo_root,
            path_template: "../{repo}-worktrees/{branch}",
            zoxide_enabled: false,
        };

        let result = service.create(&req, |_| Ok(()));

        assert!(result.is_ok());
    }

    #[test]
    fn test_create_git_failure_skips_callback_and_zoxide() {
        let service = WorktreeService::new(
            MockGitClient {
                create_should_fail: true,
                ..Default::default()
            },
            MockZoxideClient::with_failure(),
        );
        let callback_called = Cell::new(false);
        let repo_root = PathBuf::from("/test/repo");
        let req = make_req("feature", &repo_root, "../{repo}-worktrees/{branch}", true);

        let result = service.create(&req, |_| {
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
    fn test_create_callback_error_propagates_and_skips_zoxide() {
        let service = WorktreeService::new(
            MockGitClient::default(),
            MockZoxideClient::with_failure(), // would fail if reached
        );
        let repo_root = PathBuf::from("/test/repo");
        let req = make_req("feature", &repo_root, "../{repo}-worktrees/{branch}", true);

        let result = service.create(&req, |_| anyhow::bail!("callback boom"));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("callback boom"));
    }

    #[test]
    fn test_create_zoxide_failure() {
        let service =
            WorktreeService::new(MockGitClient::default(), MockZoxideClient::with_failure());
        let repo_root = PathBuf::from("/test/repo");
        let req = make_req("feature", &repo_root, "../{repo}-worktrees/{branch}", true);

        let result = service.create(&req, |_| Ok(()));

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Mock zoxide failure"));
    }

    #[test]
    fn test_create_zoxide_disabled_skips_zoxide() {
        let service = WorktreeService::new(
            MockGitClient::default(),
            MockZoxideClient::with_failure(), // would fail if reached
        );
        let repo_root = PathBuf::from("/test/repo");
        let req = make_req("feature", &repo_root, "../{repo}-worktrees/{branch}", true);
        let req = CreateWorktreeRequest {
            zoxide_enabled: false,
            ..req
        };

        let result = service.create(&req, |_| Ok(()));

        assert!(result.is_ok());
    }

    #[test]
    fn test_create_expands_relative_template() {
        let service = WorktreeService::new(MockGitClient::default(), MockZoxideClient::new());
        let repo_root = PathBuf::from("/Users/me/projects/myrepo");
        let req = make_req(
            "feat/auth",
            &repo_root,
            "../{repo}-worktrees/{branch}",
            false,
        );

        let result = service.create(&req, |_| Ok(())).unwrap();

        assert_eq!(
            result,
            PathBuf::from("/Users/me/projects/myrepo/../myrepo-worktrees/feat/auth")
        );
    }

    #[test]
    fn test_create_expands_absolute_template() {
        let service = WorktreeService::new(MockGitClient::default(), MockZoxideClient::new());
        let repo_root = PathBuf::from("/Users/me/projects/myrepo");
        let req = make_req("feature", &repo_root, "/tmp/wt/{repo}/{branch}", false);

        let result = service.create(&req, |_| Ok(())).unwrap();

        assert_eq!(result, PathBuf::from("/tmp/wt/myrepo/feature"));
    }
}
