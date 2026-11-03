//! Common utility functions for command handlers
//!
//! This module contains shared helper functions used across multiple commands.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::domain::worktree::{NameResolution, WorktreeEntry, WorktreeList};
use crate::integrations::git::{GitClient, RealGitClient};
use crate::path_utils::{canonicalize_allow_missing, display_path};

/// Get the main worktree's root path
///
/// From the main worktree itself this is its toplevel, whatever the `.git` layout. From a
/// linked worktree it is the parent of the common `.git` directory, which only the standard
/// layout can tell.
///
/// # Errors
/// Returns an error if:
/// - Not in a git repository
/// - The repository is bare
/// - The main worktree cannot be located (a linked worktree of a `--separate-git-dir` repository)
pub fn get_main_repo_root() -> Result<PathBuf> {
    let git = RealGitClient;
    let common = git
        .rev_parse(&["rev-parse", "--git-common-dir"], None)
        .map_err(|e| {
            anyhow::anyhow!(
                "Not in a git repository. Please run ofsht from within a git repository.\nGit error: {e}"
            )
        })?;
    let cwd = std::env::current_dir()?;
    let absolute = |path: &str| canonicalize_allow_missing(&cwd.join(path.trim()));
    let common_dir = absolute(&common);

    let porcelain = git.list_worktrees(None)?;
    let first_block = porcelain.split("\n\n").next().unwrap_or_default();
    if first_block.lines().any(|line| line == "bare") {
        let path = first_block
            .lines()
            .find_map(|line| line.strip_prefix("worktree "))
            .unwrap_or_default();
        anyhow::bail!("Bare repositories are not supported: {path}");
    }

    let in_main_worktree = git
        .rev_parse(&["rev-parse", "--git-dir"], None)
        .is_ok_and(|git_dir| absolute(&git_dir) == common_dir);
    if in_main_worktree {
        // Inside the `.git` directory itself there is no toplevel; the parent rule below applies
        if let Ok(toplevel) = git.rev_parse(&["rev-parse", "--show-toplevel"], None) {
            return Ok(canonicalize_allow_missing(Path::new(toplevel.trim())));
        }
    }

    if common_dir.file_name().is_some_and(|name| name == ".git") {
        if let Some(parent) = common_dir.parent() {
            if canonicalize_allow_missing(&parent.join(".git")) == common_dir {
                return Ok(parent.to_path_buf());
            }
        }
    }

    anyhow::bail!(
        "Cannot locate the main worktree of this repository; run ofsht from the main worktree"
    )
}

/// Error for a name that matches several worktrees by branch or directory name
#[must_use]
pub fn ambiguous_name_error(name: &str, entries: &[&WorktreeEntry]) -> anyhow::Error {
    let candidates: Vec<String> = entries
        .iter()
        .map(|entry| {
            let path = display_path(Path::new(&entry.path));
            entry.branch.as_ref().map_or_else(
                || format!("  {path}"),
                |branch| format!("  {path} [{branch}]"),
            )
        })
        .collect();
    anyhow::anyhow!(
        "Ambiguous worktree name: {name}\n{}\nSpecify a path",
        candidates.join("\n")
    )
}

/// Resolve a worktree target to its canonical path and metadata
///
/// `root` is the directory that `worktree.dir` places worktrees under.
///
/// Returns: (`canonical_path`, `worktree_path`, `branch_name`, `is_current_worktree`)
///
/// # Errors
/// Returns an error if the worktree target cannot be found, is ambiguous, or refers to the
/// main worktree
pub fn resolve_worktree_target(
    name: &str,
    list_stdout: &str,
    root: Option<&Path>,
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

    // Refuse the main branch name even when another worktree's directory shares it, because
    // `cd <name>` goes to the main worktree and both commands must mean the same target.
    if name == "@" || main_entry.branch.as_deref() == Some(name) {
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
    } else {
        match list.resolve_name(name, root) {
            NameResolution::Found(entry) => {
                worktree_path = PathBuf::from(&entry.path);
                branch_name = entry.branch.clone();
                canonical_path = canonicalize_allow_missing(&worktree_path);
            }
            NameResolution::Ambiguous(entries) => return Err(ambiguous_name_error(name, &entries)),
            NameResolution::NotFound => {
                let canonical_input = canonicalize_allow_missing(Path::new(name));
                if canonical_input == canonicalize_allow_missing(Path::new(&main_path)) {
                    anyhow::bail!("Cannot remove main worktree");
                }
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
