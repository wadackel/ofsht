//! Cd command - Navigate to a worktree by branch name

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

use crate::commands::common::get_main_repo_root;
use crate::config;
use crate::domain::worktree::{normalize_absolute_path, WorktreeList};
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

    // Resolve name: CLI arg > stdin (when piped) > fzf
    let resolved_name: Option<String> = match name {
        Some(n) => Some(n.to_string()),
        None => crate::stdin::try_read_stdin_first()?,
    };

    let Some(name) = resolved_name else {
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

        println!("{}", normalize_absolute_path(&PathBuf::from(&selected[0])));
        return Ok(());
    };
    let name = name.as_str();

    // Parse the porcelain output once and reuse the WorktreeList for all 3
    // resolution passes (`@`, branch name, relative path, absolute path).
    let list = WorktreeList::parse(&stdout, None);

    // Special handling for "@" (main worktree)
    if name == "@" {
        let main_path = list
            .main()
            .map(|m| m.path.as_str())
            .context("git worktree list returned no entries")?;
        println!("{}", normalize_absolute_path(&PathBuf::from(main_path)));
        return Ok(());
    }

    // Load config to get worktree template (for relative path resolution)
    let repo_root = get_main_repo_root()?;
    let config = config::Config::load_from_repo_root(&repo_root).ok();

    // Priority 1: Try to find by branch name
    if let Some(entry) = list.find_by_branch(name) {
        println!("{}", normalize_absolute_path(&PathBuf::from(&entry.path)));
        return Ok(());
    }

    // Priority 2: Try to resolve as relative path (if config is available)
    if config.is_some() {
        let worktree_paths: Vec<PathBuf> = list
            .non_main()
            .iter()
            .map(|e| PathBuf::from(&e.path))
            .collect();

        if let Some(worktree_root) =
            crate::domain::worktree::calculate_worktree_root_from_paths(&worktree_paths)
        {
            let abs_path = worktree_root.join(name);
            if let Some(entry) = list.find_by_path(&abs_path) {
                println!("{}", normalize_absolute_path(&PathBuf::from(&entry.path)));
                return Ok(());
            }
        }
    }

    // Priority 3: Try to resolve as absolute path (fallback)
    let input_path = PathBuf::from(name);
    if let Some(entry) = list.find_by_path(&input_path) {
        println!("{}", normalize_absolute_path(&PathBuf::from(&entry.path)));
        return Ok(());
    }

    anyhow::bail!("Worktree not found: {name}");
}
