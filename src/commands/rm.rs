//! Remove command - Remove one or multiple worktrees

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::process::Command;

use crate::color;
use crate::commands::common::{get_main_repo_root, parse_all_worktrees, resolve_worktree_target};
use crate::config;
use crate::domain::worktree::display_path;
use crate::hooks;
use crate::integrations;
use crate::integrations::fzf::FzfPicker;

/// Remove a worktree and optionally delete its branch
/// This is a shared helper function used by both `cmd_rm_many` and `cmd_finish`
fn remove_worktree_internal(
    worktree_path: &std::path::Path,
    branch_name: Option<&str>,
    config: &config::Config,
    repo_root: &std::path::Path,
    color_mode: color::ColorMode,
) -> Result<()> {
    // Execute delete hooks before removing the worktree
    if worktree_path.exists()
        && (!config.hooks.delete.run.is_empty()
            || !config.hooks.delete.copy.is_empty()
            || !config.hooks.delete.link.is_empty())
    {
        eprintln!("{}", color::info(color_mode, "Executing delete hooks…"));
        hooks::execute_hooks(&config.hooks.delete, worktree_path, repo_root, color_mode)?;
    }

    // Remove worktree using git worktree remove
    let output = Command::new("git")
        .args(["worktree", "remove"])
        .arg(worktree_path)
        .current_dir(repo_root)
        .output()
        .context("Failed to execute git worktree remove")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree remove failed: {stderr}");
    }

    eprintln!(
        "{}",
        color::success(
            color_mode,
            format!("Removed worktree: {}", display_path(worktree_path))
        )
    );

    // Try to delete the branch (optional, may fail if branch doesn't exist)
    if let Some(branch) = branch_name {
        let branch_output = Command::new("git")
            .args(["branch", "-D", branch])
            .current_dir(repo_root)
            .output()
            .context("Failed to execute git branch -D")?;

        if branch_output.status.success() {
            eprintln!(
                "{}",
                color::success(color_mode, format!("Deleted branch: {branch}"))
            );
        }
    }

    Ok(())
}

/// Remove one or multiple worktrees
///
/// # Errors
/// Returns an error if:
/// - Not in a git repository
/// - Git worktree list command fails
/// - Target resolution fails
/// - Worktree removal fails
#[allow(clippy::too_many_lines)]
pub fn cmd_rm_many(targets: &[String], color_mode: color::ColorMode) -> Result<()> {
    // Get main repository root first to avoid issues when current directory is removed
    let repo_root = get_main_repo_root()?;

    // Load configuration from repo root
    let config = config::Config::load_from_repo_root(&repo_root)?;

    // Get worktree list once for all targets
    let list_output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&repo_root)
        .output()
        .context("Failed to execute git worktree list --porcelain")?;

    if !list_output.status.success() {
        let stderr = String::from_utf8_lossy(&list_output.stderr);
        anyhow::bail!("git worktree list failed: {stderr}");
    }

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);

    // If no targets provided, use fzf for interactive multi-selection
    let targets: Vec<String> = if targets.is_empty() {
        if !config.integrations.fzf.enabled {
            anyhow::bail!("Provide at least one target or enable fzf in config");
        }

        if !integrations::fzf::is_fzf_available() {
            anyhow::bail!("fzf is not installed. Install it or provide at least one target");
        }

        // Build items for fzf
        let items = integrations::fzf::build_worktree_items(&list_stdout);

        if items.is_empty() {
            anyhow::bail!("No worktrees found");
        }

        // Use fzf to select (multi-select enabled)
        let picker = integrations::fzf::RealFzfPicker::new(config.integrations.fzf.options.clone());
        let selected = picker.pick(&items, true)?;

        if selected.is_empty() {
            // User pressed Esc or no selection
            return Ok(());
        }

        // selected contains paths, convert them to branch names or paths as targets
        selected
            .iter()
            .map(std::string::ToString::to_string)
            .collect()
    } else {
        targets.to_vec()
    };

    // First, resolve all targets to detect duplicates and validate them
    let mut non_current_removals = Vec::new();
    let mut current_removal: Option<(std::path::PathBuf, std::path::PathBuf, Option<String>)> =
        None;
    let mut seen_paths = HashSet::new();

    for target in &targets {
        match resolve_worktree_target(target, &list_stdout, &repo_root) {
            Ok((canonical_path, worktree_path, branch_name, is_current)) => {
                // Special handling for current worktree (.)
                if is_current {
                    // If we've already seen this path as a non-current target,
                    // remove it from non_current_removals and treat it as current
                    if seen_paths.contains(&canonical_path) {
                        non_current_removals.retain(|(path, _, _)| path != &canonical_path);
                        eprintln!(
                            "{}",
                            color::warn(
                                color_mode,
                                format!(
                                    "Duplicate target {} (treating as current worktree)",
                                    display_path(&canonical_path)
                                )
                            )
                        );
                    } else {
                        seen_paths.insert(canonical_path.clone());
                    }
                    current_removal = Some((canonical_path, worktree_path, branch_name));
                } else {
                    // Check for duplicates (non-current targets)
                    if seen_paths.contains(&canonical_path) {
                        eprintln!(
                            "{}",
                            color::warn(
                                color_mode,
                                format!(
                                    "Duplicate target {} (skipping)",
                                    display_path(&canonical_path)
                                )
                            )
                        );
                        continue;
                    }

                    seen_paths.insert(canonical_path.clone());
                    non_current_removals.push((canonical_path, worktree_path, branch_name));
                }
            }
            Err(e) => {
                return Err(e);
            }
        }
    }

    // Execute removals: non-current first, then current (if present)
    for (_, worktree_path, branch_name) in non_current_removals {
        remove_worktree_internal(
            &worktree_path,
            branch_name.as_deref(),
            &config,
            &repo_root,
            color_mode,
        )?;
    }

    // Remove current worktree last (if requested)
    if let Some((_, worktree_path, branch_name)) = current_removal {
        remove_worktree_internal(
            &worktree_path,
            branch_name.as_deref(),
            &config,
            &repo_root,
            color_mode,
        )?;

        // Print main worktree path for shell wrapper
        let (main_path, _) = parse_all_worktrees(&list_stdout);
        println!("{main_path}");
    }

    Ok(())
}
