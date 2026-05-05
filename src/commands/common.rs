//! Common utility functions for command handlers
//!
//! This module contains shared helper functions used across multiple commands.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::domain::worktree::WorktreeList;
use crate::integrations::git::{GitClient, RealGitClient};
use crate::path_utils::canonicalize_allow_missing;

/// Get the main repository root path
///
/// # Errors
/// Returns an error if:
/// - Not in a git repository
/// - Git command fails
/// - Path canonicalization fails
pub fn get_main_repo_root() -> Result<PathBuf> {
    let git = RealGitClient;
    let stdout = git
        .rev_parse(&["rev-parse", "--git-common-dir"], None)
        .map_err(|e| {
            anyhow::anyhow!(
                "Not in a git repository. Please run ofsht from within a git repository.\nGit error: {e}"
            )
        })?;

    let git_dir = stdout.trim().to_string();
    let git_path = PathBuf::from(&git_dir);

    // Convert relative path to absolute
    let abs_git_path = if git_path.is_absolute() {
        git_path
    } else {
        std::env::current_dir()?.join(git_path).canonicalize()?
    };

    // Parent of .git directory is the repository root
    // For bare repositories, git_dir itself might be the root
    let repo_root = abs_git_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or(abs_git_path);

    Ok(repo_root)
}

/// Resolve a worktree target to its canonical path and metadata
///
/// Returns: (`canonical_path`, `worktree_path`, `branch_name`, `is_current_worktree`)
///
/// # Errors
/// Returns an error if the worktree target cannot be found or refers to the main worktree
#[allow(clippy::too_many_lines)]
pub fn resolve_worktree_target(
    name: &str,
    list_stdout: &str,
) -> Result<(PathBuf, PathBuf, Option<String>, bool)> {
    let is_current_worktree_removal = name == ".";

    // Get current path if resolving "."
    let current_path_opt = if is_current_worktree_removal {
        let git = RealGitClient;
        let stdout = git
            .rev_parse(&["rev-parse", "--show-toplevel"], None)
            .map_err(|e| anyhow::anyhow!("Not in a git repository: {e}"))?;
        Some(PathBuf::from(stdout.trim()))
    } else {
        None
    };

    // Parse worktrees, passing the current path so list.current() resolves
    // the active entry for "." removal.
    let list = WorktreeList::parse(list_stdout, current_path_opt.as_deref());
    let main_entry = list
        .main()
        .context("git worktree list returned no entries")?;
    let main_path = main_entry.path.clone();

    // Check for main worktree
    if name == "@" {
        anyhow::bail!("Cannot remove main worktree");
    }

    let worktree_path: PathBuf;
    let branch_name: Option<String>;
    let canonical_path: PathBuf;

    // Special handling for "." (current worktree)
    if let Some(current_path) = current_path_opt {
        let active = list.current().with_context(|| {
            format!(
                "Current directory {} does not match any tracked worktree",
                current_path.display()
            )
        })?;

        if active.path == main_path {
            anyhow::bail!("Cannot remove main worktree");
        }

        worktree_path = current_path;
        branch_name = active.branch.clone();
        canonical_path = canonicalize_allow_missing(&worktree_path);
    } else if let Some(entry) = list.find_by_branch(name) {
        // Found by branch name (excludes main automatically)
        worktree_path = PathBuf::from(&entry.path);
        branch_name = Some(name.to_string());
        canonical_path = canonicalize_allow_missing(&worktree_path);
    } else {
        // Try to resolve as relative path from worktree root
        let worktree_paths: Vec<PathBuf> = list
            .non_main()
            .iter()
            .map(|e| PathBuf::from(&e.path))
            .collect();

        let relative_match = crate::domain::worktree::calculate_worktree_root_from_paths(
            &worktree_paths,
        )
        .and_then(|root| {
            let abs_path = root.join(name);
            list.find_by_path(&abs_path).cloned()
        });

        if let Some(matched) = relative_match {
            worktree_path = PathBuf::from(&matched.path);
            branch_name = matched.branch;
            canonical_path = canonicalize_allow_missing(&worktree_path);
        } else {
            // Fallback: try to resolve as an absolute path
            let input_path_buf = PathBuf::from(name);
            let canonical_input = canonicalize_allow_missing(&input_path_buf);

            // Check if it's the main worktree
            let main_path_buf = PathBuf::from(&main_path);
            let canonical_main = canonicalize_allow_missing(&main_path_buf);
            if canonical_input == canonical_main {
                anyhow::bail!("Cannot remove main worktree");
            }

            if let Some(entry) = list.find_by_path(&input_path_buf) {
                worktree_path = PathBuf::from(&entry.path);
                branch_name = entry.branch.clone();
                canonical_path = canonical_input;
            } else {
                anyhow::bail!("Worktree not found: {name}");
            }
        }
    }

    Ok((
        canonical_path,
        worktree_path,
        branch_name,
        is_current_worktree_removal,
    ))
}
