// CommandFactory is used by tests via Cli::command()
#[allow(unused_imports)]
use clap::CommandFactory;

use clap::{Parser, Subcommand};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

/// Git worktree management tool
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// When to use colored output
    #[arg(long, value_name = "WHEN", global = true, ignore_case = true)]
    pub color: Option<crate::color::ColorMode>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Create a new worktree with a branch
    Add {
        /// Branch name for the new worktree
        branch: String,
        /// Start point (branch, tag, or commit) for the new branch.
        /// Defaults to HEAD if not specified.
        #[arg(add = ArgValueCompleter::new(list_git_refs))]
        start_point: Option<String>,
        /// Create a new tmux window for the worktree
        #[arg(long, conflicts_with = "no_tmux")]
        tmux: bool,
        /// Skip tmux window creation (overrides config behavior)
        #[arg(long, conflicts_with = "tmux")]
        no_tmux: bool,
    },
    /// Create a new worktree without navigation
    Create {
        /// Branch name for the new worktree
        branch: String,
        /// Start point (branch, tag, or commit) for the new branch.
        /// Defaults to HEAD if not specified.
        #[arg(add = ArgValueCompleter::new(list_git_refs))]
        start_point: Option<String>,
    },
    /// List all worktrees
    Ls {
        /// Show worktree paths
        #[arg(long)]
        show_path: bool,
    },
    /// Remove a worktree
    /// When no targets are provided, fzf will be used for interactive multi-selection (if enabled)
    Rm {
        /// Worktree name(s) to remove (optional with fzf)
        #[arg(num_args = 0.., value_name = "TARGET", add = ArgValueCompleter::new(list_git_worktrees))]
        targets: Vec<String>,
    },
    /// Navigate to a worktree (prints path)
    /// When name is not provided, fzf will be used for interactive selection (if enabled)
    Cd {
        /// Worktree name to navigate to (optional with fzf)
        #[arg(add = ArgValueCompleter::new(list_git_worktrees))]
        name: Option<String>,
    },
    /// Initialize configuration files (creates both global and local configs by default)
    Init {
        /// Generate only global config
        #[arg(long, conflicts_with = "local")]
        global: bool,
        /// Generate only local config
        #[arg(long, conflicts_with = "global")]
        local: bool,
        /// Overwrite existing config files
        #[arg(short, long)]
        force: bool,
    },
    /// Generate shell completion script
    Completion {
        /// Shell type (bash, zsh, fish)
        shell: String,
    },
    /// Generate shell integration script
    ShellInit {
        /// Shell type (bash, zsh, fish)
        shell: String,
    },
}

/// List Git refs (branches and tags) for completion of start-point arguments
///
/// Returns empty Vec if git command fails (e.g., not in a git repository)
/// Includes local branches, remote branches, and tags
/// Filters refs by the provided prefix
/// Excludes symbolic refs like origin/HEAD
#[must_use]
pub fn list_git_refs(current: &OsStr) -> Vec<CompletionCandidate> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname:short)%09%(symref)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let prefix = current.to_string_lossy();

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            let refname = parts.first()?.trim();
            let symref = parts.get(1).map_or("", |s| s.trim());

            // Filter out symbolic refs (symref column is non-empty)
            if !symref.is_empty() {
                return None;
            }

            // Filter by prefix
            if !refname.starts_with(&*prefix) {
                return None;
            }

            Some(CompletionCandidate::new(refname))
        })
        .collect()
}

/// List Git branches for completion
///
/// Returns empty Vec if git command fails (e.g., not in a git repository)
/// Includes both local and remote branches
/// Filters branches by the provided prefix
/// Excludes symbolic refs like origin/HEAD
#[must_use]
#[allow(dead_code)] // Reserved for future use
pub fn list_git_branches(current: &OsStr) -> Vec<CompletionCandidate> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname:short)%09%(symref)",
            "refs/heads",
            "refs/remotes",
        ])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let prefix = current.to_string_lossy();

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            let refname = parts.first()?.trim();
            let symref = parts.get(1).map_or("", |s| s.trim());

            // Filter out symbolic refs (symref column is non-empty)
            if !symref.is_empty() {
                return None;
            }

            // Filter by prefix
            if !refname.starts_with(&*prefix) {
                return None;
            }

            Some(CompletionCandidate::new(refname))
        })
        .collect()
}

/// List Git worktrees for completion
///
/// Returns empty Vec if git command fails
/// Filters worktree branch names by the provided prefix
/// Includes "@" as the main worktree
pub fn list_git_worktrees(current: &OsStr) -> Vec<CompletionCandidate> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let prefix = current.to_string_lossy();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Use HashSet to deduplicate branch names and relative paths
    let mut candidates_set = HashSet::new();

    // Always include "@" if it matches the prefix
    if "@".starts_with(&*prefix) {
        candidates_set.insert("@".to_string());
    }

    // Add branch names (existing behavior)
    for branch in parse_worktree_list(&stdout) {
        candidates_set.insert(branch);
    }

    // Try to add relative paths (new behavior)
    if let Ok(repo_root) = crate::commands::common::get_main_repo_root() {
        if crate::config::Config::load_from_repo_root(&repo_root).is_ok() {
            // Parse worktrees to get paths
            let entries = crate::domain::worktree::parse_worktree_entries(&stdout, None);

            // Collect all non-main worktree paths (skip index 0 which is main)
            let worktree_paths: Vec<PathBuf> = entries
                .iter()
                .skip(1)
                .map(|entry| PathBuf::from(&entry.path))
                .collect();

            // Calculate worktree root from all non-main worktrees
            if let Some(worktree_root) =
                crate::domain::worktree::calculate_worktree_root_from_paths(&worktree_paths)
            {
                // Add relative paths for all non-main worktrees
                for entry in entries.iter().skip(1) {
                    let worktree_path = PathBuf::from(&entry.path);
                    if let Some(rel_path) = crate::domain::worktree::calculate_relative_path(
                        &worktree_path,
                        &worktree_root,
                    ) {
                        candidates_set.insert(rel_path);
                    }
                }
            }
        }
    }

    // Filter by prefix and convert to CompletionCandidate
    candidates_set
        .into_iter()
        .filter(|name| name.starts_with(&*prefix))
        .map(CompletionCandidate::new)
        .collect()
}

/// Parse git worktree list --porcelain output and extract branch names
/// Excludes the main worktree (first worktree in the list)
#[must_use]
pub fn parse_worktree_list(output: &str) -> Vec<String> {
    let mut branches = Vec::new();
    let mut worktree_index = 0;
    let mut current_branch: Option<String> = None;

    for line in output.lines() {
        if line.starts_with("worktree ") {
            // Save previous worktree's branch (skip first/main worktree at index 0)
            if let Some(branch) = current_branch.take() {
                if worktree_index > 0 {
                    branches.push(branch);
                }
            }
            worktree_index += 1;
        } else if line.starts_with("branch ") {
            if let Some(branch_ref) = line.strip_prefix("branch ") {
                // Strip refs/heads/ prefix
                let branch = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
                current_branch = Some(branch.to_string());
            }
        } else if line.is_empty() {
            // End of worktree entry
            if let Some(branch) = current_branch.take() {
                if worktree_index > 1 {
                    branches.push(branch);
                }
            }
        }
    }

    // Handle last worktree if exists
    if let Some(branch) = current_branch {
        if worktree_index > 1 {
            branches.push(branch);
        }
    }

    branches
}
