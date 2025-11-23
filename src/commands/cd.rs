//! Cd command - Navigate to a worktree by branch name

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

use crate::commands::common::{get_main_repo_root, parse_all_worktrees};
use crate::config;
use crate::domain::worktree::display_path;
use crate::integrations;
use crate::integrations::fzf::FzfPicker;

/// Navigate to a worktree by branch name
///
/// # Errors
/// Returns an error if:
/// - Git worktree list command fails
/// - Worktree not found
/// - Fzf is required but not available
pub fn cmd_goto(name: Option<&str>, _color_mode: crate::color::ColorMode) -> Result<()> {
    // Get worktree list
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .context("Failed to execute git worktree list")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree list failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // If no name provided, use fzf for interactive selection
    let Some(name) = name else {
        let repo_root = get_main_repo_root()?;
        let config = config::Config::load_from_repo_root(&repo_root)?;

        if !config.integrations.fzf.enabled {
            anyhow::bail!("Provide a worktree name or enable fzf in config");
        }

        if !integrations::fzf::is_fzf_available() {
            anyhow::bail!("fzf is not installed. Install it or provide a worktree name");
        }

        // Build items for fzf
        let items = integrations::fzf::build_worktree_items(&stdout);

        if items.is_empty() {
            anyhow::bail!("No worktrees found");
        }

        // Use fzf to select
        let picker = integrations::fzf::RealFzfPicker::new(config.integrations.fzf.options);
        let selected = picker.pick(&items, false)?;

        if selected.is_empty() {
            // User pressed Esc or no selection
            return Ok(());
        }

        println!("{}", display_path(&PathBuf::from(&selected[0])));
        return Ok(());
    };

    // Special handling for "@" (main worktree)
    if name == "@" {
        let (main_path, _) = parse_all_worktrees(&stdout);
        println!("{}", display_path(&PathBuf::from(&main_path)));
        return Ok(());
    }

    // Parse worktree list to find the path
    let mut current_path: Option<&str> = None;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path);
        } else if let Some(branch_line) = line.strip_prefix("branch ") {
            let branch = branch_line
                .strip_prefix("refs/heads/")
                .unwrap_or(branch_line);

            // Check if this is the branch we're looking for
            if branch == name {
                if let Some(path) = current_path {
                    println!("{}", display_path(&PathBuf::from(path)));
                    return Ok(());
                }
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    anyhow::bail!("Worktree not found: {name}");
}
