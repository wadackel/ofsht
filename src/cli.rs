// CommandFactory is used by tests via Cli::command()
#[allow(unused_imports)]
use clap::CommandFactory;

use clap::{Parser, Subcommand};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;

use crate::integrations::git::{GitClient, RealGitClient};

/// Git worktree management tool
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// When to use colored output
    #[arg(long, value_name = "WHEN", global = true, ignore_case = true)]
    pub color: Option<crate::color::ColorMode>,

    /// Show verbose output (e.g., full hook command output)
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Create a new worktree with a branch
    Add {
        /// Branch name for the new worktree (read from stdin when omitted and stdin is piped)
        branch: Option<String>,
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
        /// Branch name for the new worktree (read from stdin when omitted and stdin is piped)
        branch: Option<String>,
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
    /// Open all worktrees in tmux windows or panes
    Open {
        /// Open each worktree in a separate pane (in the current window)
        #[arg(long, conflicts_with = "window")]
        pane: bool,
        /// Open each worktree in a separate tmux window
        #[arg(long, conflicts_with = "pane")]
        window: bool,
    },
    /// Sync hook file operations to existing worktrees
    ///
    /// Re-applies hooks.create (run/copy/link) to all existing non-main worktrees.
    /// When no flags are specified, all actions are executed.
    Sync {
        /// Only execute run commands
        #[arg(long)]
        run: bool,
        /// Only execute copy operations
        #[arg(long)]
        copy: bool,
        /// Only execute link operations
        #[arg(long)]
        link: bool,
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
    let git = RealGitClient;
    let Ok(stdout) = git.for_each_ref(
        &["refs/heads", "refs/remotes", "refs/tags"],
        "%(refname:short)%09%(symref)",
        None,
    ) else {
        return Vec::new();
    };

    let prefix = current.to_string_lossy();

    stdout
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
    let git = RealGitClient;
    let Ok(stdout) = git.for_each_ref(
        &["refs/heads", "refs/remotes"],
        "%(refname:short)%09%(symref)",
        None,
    ) else {
        return Vec::new();
    };

    let prefix = current.to_string_lossy();

    stdout
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
    let git = RealGitClient;
    let Ok(stdout) = git.list_worktrees(None) else {
        return Vec::new();
    };

    let prefix = current.to_string_lossy();

    // Use HashSet to deduplicate branch names and relative paths
    let mut candidates_set = HashSet::new();

    // Always include "@" if it matches the prefix
    if "@".starts_with(&*prefix) {
        candidates_set.insert("@".to_string());
    }

    // Parse the porcelain output once via the unified WorktreeList type.
    let list = crate::domain::worktree::WorktreeList::parse(&stdout, None);

    // Add branch names from non-main worktrees (excludes main automatically).
    // This naturally fixes a latent inconsistency in the legacy parser
    // (`worktree_index > 0` vs `> 1`) where main could leak into completion candidates
    // when entry separators were missing in malformed porcelain.
    for entry in list.non_main() {
        if let Some(branch) = &entry.branch {
            candidates_set.insert(branch.clone());
        }
    }

    // Try to add relative paths (new behavior)
    if let Ok(repo_root) = crate::commands::common::get_main_repo_root() {
        if crate::config::Config::load_from_repo_root(&repo_root).is_ok() {
            // Collect all non-main worktree paths
            let worktree_paths: Vec<PathBuf> = list
                .non_main()
                .iter()
                .map(|entry| PathBuf::from(&entry.path))
                .collect();

            // Calculate worktree root from all non-main worktrees
            if let Some(worktree_root) =
                crate::domain::worktree::calculate_worktree_root_from_paths(&worktree_paths)
            {
                // Add relative paths for all non-main worktrees
                for entry in list.non_main() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::worktree::WorktreeList;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn test_list_git_branches_returns_branches_in_git_repo() {
        // When running in a git repo, should return branch list (at least one branch exists)
        use std::ffi::OsStr;
        let result = list_git_branches(OsStr::new(""));
        // In a valid git repo, there should be at least one branch
        assert!(
            !result.is_empty(),
            "Should return branches when in git repo"
        );
    }

    #[test]
    fn test_list_git_branches_returns_completion_candidates() {
        // Verify that the returned values are valid CompletionCandidates
        use std::ffi::OsStr;
        let result = list_git_branches(OsStr::new(""));
        for candidate in result {
            // Each candidate should have non-empty value
            assert!(
                !candidate.get_value().is_empty(),
                "Branch name should not be empty"
            );
        }
    }

    #[test]
    fn test_list_git_branches_filters_by_prefix() {
        // Test that branches are filtered by prefix
        use std::ffi::OsStr;

        // Get all branches first
        let all_branches = list_git_branches(OsStr::new(""));

        if let Some(first_branch) = all_branches.first() {
            let branch_str = first_branch.get_value().to_string_lossy();
            if branch_str.len() >= 2 {
                let prefix = &branch_str[..2]; // Take first 2 characters as prefix
                let filtered = list_git_branches(OsStr::new(prefix));

                // All filtered branches should start with the prefix
                for candidate in &filtered {
                    let value = candidate.get_value().to_string_lossy();
                    assert!(
                        value.starts_with(prefix),
                        "Branch '{value}' should start with prefix '{prefix}'"
                    );
                }

                // Filtered list should be <= all branches
                assert!(filtered.len() <= all_branches.len());
            }
        }
    }

    /// Helper: extract branch names from non-main worktrees, mirroring the
    /// completion path inside `list_git_worktrees`.
    fn worktree_list_branches(output: &str) -> Vec<String> {
        WorktreeList::parse(output, None)
            .non_main()
            .iter()
            .filter_map(|e| e.branch.clone())
            .collect()
    }

    #[test]
    fn test_worktree_list_branches_excludes_main() {
        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

";
        let result = worktree_list_branches(output);
        assert_eq!(result, vec!["feature"]);
    }

    #[test]
    fn test_worktree_list_branches_multiple_worktrees() {
        let output = "worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature-a
branch refs/heads/feature-a

worktree /path/to/feature-b
branch refs/heads/feature-b

";
        let result = worktree_list_branches(output);
        assert_eq!(result, vec!["feature-a", "feature-b"]);
    }

    #[test]
    fn test_worktree_list_branches_empty() {
        let output = "";
        let result = worktree_list_branches(output);
        assert!(result.is_empty());
    }

    #[test]
    fn test_completion_excludes_main_branch_for_malformed_separator_missing() {
        // Regression for a latent bug in the previous standalone branch-list parser
        // (worktree-index threshold mismatch): when entry separators were missing
        // the main branch leaked into completion candidates. WorktreeList::parse uses
        // index-0-is-main semantics so the main branch must never appear here.
        let output = "worktree /path/to/main\nbranch refs/heads/main\nworktree /path/to/feat\nbranch refs/heads/feat\n";
        let result = worktree_list_branches(output);
        assert!(
            !result.contains(&"main".to_string()),
            "main branch must not be in completion candidates even with missing entry separators: {result:?}"
        );
        assert!(
            result.contains(&"feat".to_string()),
            "non-main branch must still be present: {result:?}"
        );
    }

    #[test]
    fn test_list_git_worktrees_includes_at_symbol() {
        // Test that @ is included in worktree completion candidates
        use std::ffi::OsStr;
        let result = list_git_worktrees(OsStr::new(""));
        // Should include @ as the first candidate (or at least include it)
        let has_at = result.iter().any(|c| c.get_value() == "@");
        assert!(has_at, "Completion candidates should include @");
    }

    #[test]
    fn test_list_git_worktrees_filters_at_symbol() {
        // Test that @ is filtered correctly by prefix
        use std::ffi::OsStr;
        let result = list_git_worktrees(OsStr::new("@"));
        // Should include @ when prefix is @
        let has_at = result.iter().any(|c| c.get_value() == "@");
        assert!(
            has_at,
            "Completion candidates should include @ when prefix is @"
        );
    }

    #[test]
    fn test_list_git_worktrees_excludes_at_with_different_prefix() {
        // Test that @ is excluded when prefix doesn't match
        use std::ffi::OsStr;
        let result = list_git_worktrees(OsStr::new("feature"));
        // Should not include @ when prefix is "feature"
        let has_at = result.iter().any(|c| c.get_value() == "@");
        assert!(
            !has_at,
            "Completion candidates should not include @ when prefix is 'feature'"
        );
    }
}
