//! Create command - Simple worktree creation without extra features

use anyhow::{Context, Result};
use std::process::Command;

use crate::color;
use crate::commands::common::get_main_repo_root;
use crate::config;
use crate::domain::worktree::display_path;
use crate::hooks;
use crate::integrations;

/// Create a new worktree (simple version without tmux/GitHub integration)
///
/// # Errors
/// Returns an error if:
/// - Not in a git repository
/// - Git worktree creation fails
/// - Hook execution fails
/// - Zoxide registration fails
pub fn cmd_create(
    branch: &str,
    start_point: Option<&str>,
    color_mode: color::ColorMode,
) -> Result<()> {
    // Get main repository root
    let repo_root = get_main_repo_root()?;

    // Load configuration from repo root
    let config = config::Config::load_from_repo_root(&repo_root)?;
    let repo_name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .context("Failed to get repository name")?;

    // Expand path template
    #[allow(clippy::literal_string_with_formatting_args)]
    let path_template = config
        .worktree
        .dir
        .replace("{repo}", repo_name)
        .replace("{branch}", branch);

    // Create worktree path (relative paths are resolved from repo root)
    let worktree_path = if path_template.starts_with('/') {
        std::path::PathBuf::from(&path_template)
    } else {
        repo_root.join(&path_template)
    };

    // Create worktree using git worktree add
    let mut cmd = Command::new("git");

    if let Some(sp) = start_point {
        // Create new branch with start point
        cmd.args(["worktree", "add", "-b", branch])
            .arg(&worktree_path)
            .arg(sp);
    } else {
        // No start_point: try to checkout existing branch, or create from HEAD
        // Check if branch exists
        let branch_exists = Command::new("git")
            .args(["rev-parse", "--verify", branch])
            .current_dir(&repo_root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if branch_exists {
            // Checkout existing branch (no -b flag)
            cmd.args(["worktree", "add"])
                .arg(&worktree_path)
                .arg(branch);
        } else {
            // Create new branch from HEAD
            cmd.args(["worktree", "add", "-b", branch])
                .arg(&worktree_path);
        }
    }

    let output = cmd.output().context("Failed to execute git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add failed: {stderr}");
    }

    eprintln!(
        "{}",
        color::success(
            color_mode,
            format!("Created worktree at: {}", display_path(&worktree_path))
        )
    );

    // Execute create hooks
    if !config.hooks.create.run.is_empty()
        || !config.hooks.create.copy.is_empty()
        || !config.hooks.create.link.is_empty()
    {
        eprintln!("{}", color::info(color_mode, "Executing create hooks…"));
        hooks::execute_hooks(&config.hooks.create, &worktree_path, &repo_root, color_mode)?;
    }

    // Add to zoxide if enabled
    integrations::zoxide::add_to_zoxide_if_enabled(
        &worktree_path,
        config.integrations.zoxide.enabled,
    )?;

    Ok(())
}
