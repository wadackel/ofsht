//! Common utility functions for command handlers
//!
//! This module contains shared helper functions used across multiple commands.

use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};

use crate::domain::worktree::WorktreeList;
use crate::integrations::git::{GitClient, RealGitClient};

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

/// Canonicalize a path, even if it doesn't exist on the filesystem
///
/// For missing paths, canonicalize the deepest existing ancestor and append the tail.
/// Relative paths are resolved from the current working directory.
#[must_use]
pub fn canonicalize_allow_missing(path: &Path) -> PathBuf {
    // Convert relative paths to absolute using current_dir
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    };

    // Normalize the path by processing . and .. components
    let mut normalized = PathBuf::new();
    for component in absolute_path.components() {
        match component {
            Component::CurDir => {
                // Skip "." components
            }
            Component::ParentDir => {
                // Pop the last component for ".."
                normalized.pop();
            }
            _ => {
                // Normal component (RootDir, Prefix, or Normal)
                normalized.push(component);
            }
        }
    }

    // Try normal canonicalization first
    if let Ok(canonical) = normalized.canonicalize() {
        return canonical;
    }

    // Path doesn't exist - find the deepest existing ancestor
    let mut current = normalized.as_path();
    let mut tail_components = Vec::new();

    loop {
        // Record this component first (before checking parent)
        if let Some(file_name) = current.file_name() {
            tail_components.push(file_name);
        }

        if let Some(parent) = current.parent() {
            if parent.exists() {
                // Found an existing ancestor - canonicalize it
                if let Ok(canonical_parent) = parent.canonicalize() {
                    // Rebuild the path by appending the tail components
                    let mut result = canonical_parent;
                    for component in tail_components.iter().rev() {
                        result = result.join(component);
                    }
                    return result;
                }
            }
            // Move up to parent
            current = parent;
        } else {
            // Reached the root without finding an existing ancestor
            // Fall back to the normalized path
            return normalized;
        }
    }
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
    _repo_root: &Path,
) -> Result<(PathBuf, PathBuf, Option<String>, bool)> {
    let is_current_worktree_removal = name == ".";

    // Get current path if resolving "."
    let current_path_opt = if is_current_worktree_removal {
        let git = RealGitClient;
        let stdout = git
            .rev_parse(&["rev-parse", "--show-toplevel"], None)
            .map_err(|e| anyhow::anyhow!("Not in a git repository: {e}"))?;
        Some(stdout.trim().to_string())
    } else {
        None
    };

    // Parse all worktrees once via the unified WorktreeList API.
    let list = WorktreeList::parse(list_stdout, None);
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
        let current_path_buf = PathBuf::from(&current_path);
        let canonical_current = canonicalize_allow_missing(&current_path_buf);
        let main_path_buf = PathBuf::from(&main_path);
        let canonical_main = canonicalize_allow_missing(&main_path_buf);

        if canonical_current == canonical_main {
            anyhow::bail!("Cannot remove main worktree");
        }

        // Find branch name for current worktree among non-main entries
        let current_branch = list
            .non_main()
            .iter()
            .find(|e| canonicalize_allow_missing(&PathBuf::from(&e.path)) == canonical_current)
            .and_then(|e| e.branch.clone());

        worktree_path = PathBuf::from(current_path);
        branch_name = current_branch;
        canonical_path = canonical_current;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonicalize_allow_missing_existing_path() {
        // Test with existing path (current directory)
        let current_dir = std::env::current_dir().unwrap();
        let result = canonicalize_allow_missing(&current_dir);
        // Should return canonicalized path
        assert_eq!(result, current_dir.canonicalize().unwrap());
    }

    #[test]
    fn test_canonicalize_allow_missing_nonexistent_absolute() {
        // Test with nonexistent absolute path
        let nonexistent = std::path::PathBuf::from("/nonexistent/path/to/worktree");
        let result = canonicalize_allow_missing(&nonexistent);
        // Should return the absolute path (may have symlinks resolved in existing parts)
        assert!(result.is_absolute());
        assert!(result.to_string_lossy().contains("nonexistent"));
    }

    #[test]
    fn test_canonicalize_allow_missing_relative() {
        // Test with relative path
        let relative = std::path::PathBuf::from("./test/path");
        let result = canonicalize_allow_missing(&relative);
        // Should convert to absolute path
        assert!(result.is_absolute());
    }

    #[test]
    fn test_canonicalize_allow_missing_relative_nonexistent() {
        // Test with nonexistent relative path
        let relative = std::path::PathBuf::from("./nonexistent/test/path");
        let result = canonicalize_allow_missing(&relative);
        // Should convert to absolute path
        assert!(result.is_absolute());
        assert!(result.to_string_lossy().contains("nonexistent"));
    }

    #[test]
    fn test_canonicalize_allow_missing_deep_nonexistent() {
        // Test with deeply nested nonexistent path
        let current_dir = std::env::current_dir().unwrap().canonicalize().unwrap();
        let deep_path = current_dir.join("a/b/c/d/e/f/nonexistent");
        let result = canonicalize_allow_missing(&deep_path);
        // Should start with canonicalized current dir (not symlinked version)
        assert!(
            result.starts_with(&current_dir),
            "Result {} should start with canonicalized current_dir {}",
            result.display(),
            current_dir.display()
        );
        assert!(result.to_string_lossy().contains("nonexistent"));
    }

    #[test]
    fn test_canonicalize_allow_missing_parent_dots() {
        // Test with parent directory references
        let current_dir = std::env::current_dir().unwrap().canonicalize().unwrap();
        let with_dots = current_dir.join("foo/../bar/./baz");
        let result = canonicalize_allow_missing(&with_dots);
        // Should resolve . and .. references even when path doesn't exist
        let expected = current_dir.join("bar/baz");
        assert_eq!(
            result, expected,
            "Should normalize .. and . even in nonexistent paths"
        );
        // Result should not contain literal . or .. components
        let result_str = result.to_string_lossy();
        assert!(
            !result_str.contains("/./"),
            "Result should not contain /./: {result_str}"
        );
        assert!(
            !result_str.contains("/../"),
            "Result should not contain /../: {result_str}"
        );
    }
}
