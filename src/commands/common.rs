//! Common utility functions for command handlers
//!
//! This module contains shared helper functions used across multiple commands.

use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Get the main repository root path
///
/// # Errors
/// Returns an error if:
/// - Not in a git repository
/// - Git command fails
/// - Path canonicalization fails
pub fn get_main_repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .context("Failed to execute git rev-parse")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Not in a git repository. Please run ofsht from within a git repository.\nGit error: {}",
            stderr.trim()
        );
    }

    let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
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

/// Find worktree path by branch name
/// Returns None if branch not found or is main worktree
pub fn find_worktree_by_branch(output: &str, branch_name: &str) -> Option<String> {
    let mut current_path: Option<String> = None;
    let mut worktree_index = 0;

    for line in output.lines() {
        if line.starts_with("worktree ") {
            current_path = line.strip_prefix("worktree ").map(String::from);
            worktree_index += 1;
        } else if line.starts_with("branch ") {
            if let Some(branch_ref) = line.strip_prefix("branch ") {
                let branch = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);

                // Skip main worktree (index 1)
                if worktree_index > 1 && branch == branch_name {
                    return current_path;
                }
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    None
}

/// Find worktree path by absolute path
///
/// Uses canonicalization to match paths even when:
/// - Input path is relative
/// - Paths contain symlinks, `.` or `..` components
///
/// Returns None if path not found or is main worktree
pub fn find_worktree_by_path(output: &str, target_path: &Path) -> Option<String> {
    let mut worktree_index = 0;

    for line in output.lines() {
        if line.starts_with("worktree ") {
            let current_path = line.strip_prefix("worktree ").map(String::from);
            worktree_index += 1;

            // Skip main worktree (index 1)
            if worktree_index > 1 {
                if let Some(path) = &current_path {
                    let path_buf = PathBuf::from(path);
                    let canonical_target = canonicalize_allow_missing(target_path);
                    let canonical_worktree = canonicalize_allow_missing(&path_buf);

                    if canonical_target == canonical_worktree {
                        return current_path;
                    }
                }
            }
        }
    }

    None
}

/// Check if the given path or branch name refers to the main worktree
/// This function is only used in tests
#[cfg(test)]
pub fn is_main_worktree(output: &str, path_or_branch: &str) -> bool {
    // "@" is always main worktree
    if path_or_branch == "@" {
        return true;
    }

    let mut main_path: Option<String> = None;
    let mut main_branch: Option<String> = None;
    let mut worktree_index = 0;

    for line in output.lines() {
        if line.starts_with("worktree ") {
            worktree_index += 1;
            if worktree_index == 1 {
                main_path = line.strip_prefix("worktree ").map(String::from);
            }
        } else if line.starts_with("branch ") && worktree_index == 1 {
            if let Some(branch_ref) = line.strip_prefix("branch ") {
                let branch = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
                main_branch = Some(branch.to_string());
            }
        }

        // Early exit if we have both and are past main worktree
        if worktree_index > 1 && main_path.is_some() {
            break;
        }
    }

    // Check if matches main path or main branch
    main_path.as_deref() == Some(path_or_branch) || main_branch.as_deref() == Some(path_or_branch)
}

/// Parse all worktrees and return (`main_path`, `Vec<(path, Option<branch>)>`)
/// Returns the main worktree path and a list of all non-main worktrees
pub fn parse_all_worktrees(output: &str) -> (String, Vec<(String, Option<String>)>) {
    let mut main_path = String::new();
    let mut worktrees = Vec::new();
    let mut worktree_index = 0;
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;

    for line in output.lines() {
        if line.starts_with("worktree ") {
            // Save previous worktree
            if let Some(path) = current_path.take() {
                if worktree_index == 1 {
                    main_path = path;
                } else {
                    worktrees.push((path, current_branch.take()));
                }
            }
            worktree_index += 1;
            current_path = line.strip_prefix("worktree ").map(String::from);
            current_branch = None;
        } else if line.starts_with("branch ") {
            if let Some(branch_ref) = line.strip_prefix("branch ") {
                let branch = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
                current_branch = Some(branch.to_string());
            }
        } else if line.is_empty() {
            // End of worktree entry
            if let Some(path) = current_path.take() {
                if worktree_index == 1 {
                    main_path = path;
                } else {
                    worktrees.push((path, current_branch.take()));
                }
            }
        }
    }

    // Handle last worktree
    if let Some(path) = current_path {
        if worktree_index == 1 {
            main_path = path;
        } else {
            worktrees.push((path, current_branch));
        }
    }

    (main_path, worktrees)
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
pub fn resolve_worktree_target(
    name: &str,
    list_stdout: &str,
    _repo_root: &Path,
) -> Result<(PathBuf, PathBuf, Option<String>, bool)> {
    let is_current_worktree_removal = name == ".";

    // Get current path if resolving "."
    let current_path_opt = if is_current_worktree_removal {
        let current_output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .context("Failed to get current worktree path")?;

        if !current_output.status.success() {
            let stderr = String::from_utf8_lossy(&current_output.stderr);
            anyhow::bail!("Not in a git repository: {stderr}");
        }

        Some(
            String::from_utf8_lossy(&current_output.stdout)
                .trim()
                .to_string(),
        )
    } else {
        None
    };

    // Parse all worktrees
    let (main_path, worktrees) = parse_all_worktrees(list_stdout);

    // Check for main worktree
    if name == "@" {
        anyhow::bail!("Cannot remove main worktree");
    }

    let worktree_path: PathBuf;
    let branch_name: Option<String>;
    let canonical_path: PathBuf;

    // Try to find by branch name first (to avoid conflicts with files/dirs of the same name)
    let branch_match = if !is_current_worktree_removal && name != "@" {
        find_worktree_by_branch(list_stdout, name)
    } else {
        None
    };

    // Special handling for "." (current worktree)
    if let Some(current_path) = current_path_opt {
        let current_path_buf = PathBuf::from(&current_path);
        let canonical_current = canonicalize_allow_missing(&current_path_buf);
        let main_path_buf = PathBuf::from(&main_path);
        let canonical_main = canonicalize_allow_missing(&main_path_buf);

        if canonical_current == canonical_main {
            anyhow::bail!("Cannot remove main worktree");
        }

        // Find branch name for current worktree
        let mut current_branch: Option<String> = None;
        for (path, branch) in &worktrees {
            let path_buf = PathBuf::from(path);
            let canonical_wt = canonicalize_allow_missing(&path_buf);
            if canonical_wt == canonical_current {
                current_branch.clone_from(branch);
                break;
            }
        }

        worktree_path = PathBuf::from(current_path);
        branch_name = current_branch;
        canonical_path = canonical_current;
    } else if let Some(path) = branch_match {
        // Found by branch name
        worktree_path = PathBuf::from(&path);
        branch_name = Some(name.to_string());
        let path_buf = PathBuf::from(&path);
        canonical_path = canonicalize_allow_missing(&path_buf);
    } else {
        // Try to resolve as a path
        let input_path_buf = PathBuf::from(name);
        let canonical_input = canonicalize_allow_missing(&input_path_buf);

        // Check if it's the main worktree
        let main_path_buf = PathBuf::from(&main_path);
        let canonical_main = canonicalize_allow_missing(&main_path_buf);
        if canonical_input == canonical_main {
            anyhow::bail!("Cannot remove main worktree");
        }

        // Check if it matches any known worktree path
        let mut found_worktree = None;
        for (path, branch) in &worktrees {
            let path_buf = PathBuf::from(path);
            let canonical_worktree = canonicalize_allow_missing(&path_buf);
            if canonical_input == canonical_worktree {
                found_worktree = Some((path.clone(), branch.clone()));
                break;
            }
        }

        if let Some((path, branch)) = found_worktree {
            worktree_path = PathBuf::from(path);
            branch_name = branch;
            canonical_path = canonical_input;
        } else {
            anyhow::bail!("Worktree not found: {name}");
        }
    }

    Ok((
        canonical_path,
        worktree_path,
        branch_name,
        is_current_worktree_removal,
    ))
}
