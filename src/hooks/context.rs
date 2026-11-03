use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};

use super::files::GlobScope;
use crate::domain::worktree::WorktreeList;
use crate::integrations::git::{GitClient, RealGitClient};
use crate::path_utils::canonicalize_allow_missing;

#[derive(Clone, Copy, Default)]
pub enum FilePolicy {
    #[default]
    Apply,
    Sync {
        overwrite: bool,
    },
}

/// Identity and output options for one worktree's hooks.
pub struct HookContext {
    pub worktree_path: PathBuf,
    pub repo_root: PathBuf,
    pub worktree_name: String,
    pub branch: String,
    pub verbose: bool,
    pub file_policy: FilePolicy,
    pub(super) glob_scope: GlobScope,
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
        let common_dir = RealGitClient::common_git_dir(&repo_root)?;
        let registry = RealGitClient::worktree_paths(&repo_root)?;
        let glob_scope = GlobScope::new(
            registry
                .into_iter()
                .filter(|path| canonicalize_allow_missing(path) != repo_root)
                .chain(std::iter::once(repo_root.join(common_dir))),
        );
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
            file_policy: FilePolicy::Apply,
            glob_scope,
        })
    }

    pub(super) fn allows_destination(&self, path: &Path, directory: bool) -> Result<bool> {
        if matches!(self.file_policy, FilePolicy::Apply) {
            return Ok(true);
        }
        let relative = path.strip_prefix(&self.worktree_path).with_context(|| {
            format!(
                "Cannot sync destination outside worktree: {}",
                path.display()
            )
        })?;
        if relative.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            anyhow::bail!(
                "Cannot sync destination outside worktree: {}",
                path.display()
            );
        }
        let mut current = self.worktree_path.clone();
        let mut components = relative.components().peekable();
        while let Some(component) = components.next() {
            current.push(component);
            if components.peek().is_none() && !directory {
                break;
            }
            match current.symlink_metadata() {
                Ok(metadata) if metadata.file_type().is_symlink() => return Ok(false),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Failed to inspect destination: {}", current.display())
                    })
                }
            }
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(super) fn for_test(path: &Path) -> Self {
        Self {
            worktree_path: path.to_path_buf(),
            repo_root: path.to_path_buf(),
            worktree_name: "test".to_string(),
            branch: String::new(),
            verbose: false,
            file_policy: FilePolicy::Apply,
            glob_scope: GlobScope::default(),
        }
    }
}
