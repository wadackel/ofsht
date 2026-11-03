//! Common utility functions for command handlers
//!
//! This module contains shared helper functions used across multiple commands.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::domain::worktree::{NameResolution, WorktreeEntry, WorktreeList};
use crate::integrations::git::{GitClient, RealGitClient};
use crate::path_utils::{canonicalize_allow_missing, display_path};

/// The git directory that `<dir>/.git` leads to: the directory itself, or a gitfile's target
fn git_dir_at(dir: &Path) -> Option<PathBuf> {
    let dot_git = dir.join(".git");
    if dot_git.is_dir() {
        return Some(canonicalize_allow_missing(&dot_git));
    }
    let content = std::fs::read_to_string(&dot_git).ok()?;
    let target = content
        .lines()
        .find_map(|line| line.strip_prefix("gitdir: "))?;
    Some(canonicalize_allow_missing(&dir.join(target.trim())))
}

/// Get the main worktree's root path
///
/// From the main worktree itself this is its toplevel, whatever the `.git` layout. Otherwise
/// it is the directory whose `.git` leads to the common git directory, which covers linked
/// worktrees of the standard layout and bare repositories kept as `<root>/.git` or behind a
/// `<root>/.git` gitfile.
///
/// # Errors
/// Returns an error if:
/// - Not in a git repository
/// - The repository is bare with no such directory
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

    let parent_root = common_dir
        .parent()
        .filter(|parent| git_dir_at(parent).as_deref() == Some(common_dir.as_path()));

    let porcelain = git.list_worktrees(None)?;
    let first_block = porcelain.split("\n\n").next().unwrap_or_default();
    if first_block.lines().any(|line| line == "bare") {
        let path = first_block
            .lines()
            .find_map(|line| line.strip_prefix("worktree "))
            .unwrap_or_default();
        return parent_root.map(Path::to_path_buf).ok_or_else(|| {
            anyhow::anyhow!(
                "Bare repositories are not supported unless kept as <root>/.git or behind a <root>/.git file: {path}"
            )
        });
    }

    let in_main_worktree = git
        .rev_parse(&["rev-parse", "--git-dir"], None)
        .is_ok_and(|git_dir| absolute(&git_dir) == common_dir);
    if in_main_worktree {
        // Inside the `.git` directory itself there is no toplevel (older git prints an empty
        // one); the parent rule below applies
        if let Ok(toplevel) = git.rev_parse(&["rev-parse", "--show-toplevel"], None) {
            if !toplevel.trim().is_empty() {
                return Ok(canonicalize_allow_missing(Path::new(toplevel.trim())));
            }
        }
    }

    if let Some(parent) = parent_root {
        return Ok(parent.to_path_buf());
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

/// The worktree containing the current directory, normalized like `canonicalize_allow_missing`
///
/// `None` when git cannot tell, for example in a bare repository or a deleted directory.
#[must_use]
pub fn current_worktree_toplevel() -> Option<PathBuf> {
    let stdout = RealGitClient
        .rev_parse(&["rev-parse", "--show-toplevel"], None)
        .ok()?;
    Some(canonicalize_allow_missing(Path::new(stdout.trim())))
}

/// Resolve a worktree target to its canonical path and metadata
///
/// `root` is the directory that `worktree.dir` places worktrees under, and `current` the
/// result of `current_worktree_toplevel`. A target is the current worktree when it is `.` or
/// resolves to `current`.
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
    current: Option<&Path>,
) -> Result<(PathBuf, PathBuf, Option<String>, bool)> {
    let is_dot = name == ".";
    let current_path_opt = if is_dot {
        Some(current.context("Not in a git repository: cannot find the current worktree")?)
    } else {
        None
    };

    // Parse worktrees, passing the current path so list.current() resolves
    // the active entry for "." removal.
    let list = WorktreeList::parse(list_stdout, current_path_opt);
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

        worktree_path = current_path.to_path_buf();
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

    let is_current = is_dot || current == Some(canonical_path.as_path());
    Ok((canonical_path, worktree_path, branch_name, is_current))
}
