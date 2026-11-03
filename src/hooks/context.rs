use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::domain::worktree::WorktreeList;
use crate::integrations::git::{GitClient, RealGitClient};

/// Identity and output options for one worktree's hooks.
pub struct HookContext {
    pub worktree_path: PathBuf,
    pub repo_root: PathBuf,
    pub worktree_name: String,
    pub branch: String,
    pub verbose: bool,
}

impl HookContext {
    pub fn new(worktree: &Path, repo: &Path, root: Option<&Path>, verbose: bool) -> Result<Self> {
        let worktree_path = worktree
            .canonicalize()
            .context("Failed to resolve hook worktree")?;
        let repo_root = repo
            .canonicalize()
            .context("Failed to resolve hook repository")?;
        let porcelain = RealGitClient.list_worktrees(Some(&repo_root))?;
        let list = WorktreeList::parse(&porcelain, None);
        let entry = list
            .entries()
            .iter()
            .find(|entry| {
                Path::new(&entry.path)
                    .canonicalize()
                    .is_ok_and(|p| p == worktree_path)
            })
            .context("Hook worktree is not registered with Git")?;
        Ok(Self {
            worktree_path,
            repo_root,
            worktree_name: list.display_name(entry, root),
            branch: entry.branch.clone().unwrap_or_default(),
            verbose,
        })
    }

    #[cfg(test)]
    pub(super) fn for_test(path: &Path) -> Self {
        Self {
            worktree_path: path.to_path_buf(),
            repo_root: path.to_path_buf(),
            worktree_name: "test".to_string(),
            branch: String::new(),
            verbose: false,
        }
    }
}
