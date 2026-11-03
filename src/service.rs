#![allow(clippy::missing_errors_doc)]
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::domain::worktree::{
    expand_worktree_path, locked_paths, nesting, worktree_root_from_template, Nesting, WorktreeList,
};
use crate::integrations::git::{GitClient, RemoteTracking};
use crate::integrations::zoxide::ZoxideClient;
use crate::path_utils::canonicalize_allow_missing;

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
        let worktree_path = expand_worktree_path(req.path_template, req.repo_root, req.branch)
            .context("Failed to get repository name")?;

        let porcelain = self.git_client.list_worktrees(Some(req.repo_root))?;
        let list = WorktreeList::parse(&porcelain, None);
        if req.start_point.is_none() {
            if self
                .git_client
                .branch_exists(req.branch, Some(req.repo_root))?
            {
                refuse_branch_in_use(req, &porcelain, &list)?;
            } else {
                match self.git_client.remote_tracking(req.branch, Some(req.repo_root)) {
                    Some(RemoteTracking::Branch(branch)) => anyhow::bail!(
                        "{name} is a remote-tracking branch; use 'ofsht add {branch} {name}' to track it",
                        name = req.branch
                    ),
                    Some(RemoteTracking::Symbolic) => anyhow::bail!(
                        "{} is a symbolic remote ref, not a branch name",
                        req.branch
                    ),
                    None => {}
                }
            }
        }
        refuse_nesting(req, &worktree_path, &list)?;

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

/// Fail before `git worktree add` when another worktree has `req.branch` checked out,
/// naming that worktree and what to run instead
fn refuse_branch_in_use(
    req: &CreateWorktreeRequest<'_>,
    porcelain: &str,
    list: &WorktreeList,
) -> Result<()> {
    let Some(entry) = list
        .entries()
        .iter()
        .find(|entry| entry.branch.as_deref() == Some(req.branch))
    else {
        return Ok(());
    };
    let path = Path::new(&entry.path);
    if !path.exists() {
        let canonical = canonicalize_allow_missing(path);
        let locked = locked_paths(porcelain)
            .iter()
            .any(|locked| canonicalize_allow_missing(Path::new(locked)) == canonical);
        let fix = if locked {
            format!(
                "git worktree unlock {}, then git worktree prune",
                entry.path
            )
        } else {
            "git worktree prune".to_string()
        };
        anyhow::bail!(
            "Branch {} is checked out in a missing worktree at {}. Run: {fix}",
            req.branch,
            entry.path
        );
    }
    let root = worktree_root_from_template(req.path_template, req.repo_root);
    anyhow::bail!(
        "Branch {} is already checked out in {}. Use: ofsht cd {}",
        req.branch,
        list.display_name(entry, root.as_deref()),
        list.cd_name(entry, root.as_deref())
    );
}

/// Fail before `git worktree add` when `path` is inside, contains, or is an existing worktree
/// other than the main one, whose directory still exists
///
/// Git creates the new branch before it refuses a non-empty directory, so the containing and
/// same-path cases would leave that branch behind.
fn refuse_nesting(req: &CreateWorktreeRequest<'_>, path: &Path, list: &WorktreeList) -> Result<()> {
    let clash = list
        .non_main()
        .iter()
        .filter(|entry| Path::new(&entry.path).exists())
        .find_map(|entry| nesting(path, Path::new(&entry.path)).map(|kind| (kind, entry)));
    let Some((kind, entry)) = clash else {
        return Ok(());
    };
    let relation = match kind {
        Nesting::Inside => "is inside",
        Nesting::Contains => "contains",
        Nesting::Same => "is already",
    };
    let root = worktree_root_from_template(req.path_template, req.repo_root);
    anyhow::bail!(
        "Cannot create {}: it {relation} the worktree {}",
        path.display(),
        list.display_name(entry, root.as_deref())
    );
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
