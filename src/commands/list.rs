//! List command - Display all worktrees with formatting options

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;

use crate::color;
use crate::domain::worktree::{
    display_path, format_worktree_table, get_last_commit_time, parse_simple_worktree_entries,
    parse_worktree_entries,
};

/// List all worktrees
///
/// # Errors
/// Returns an error if:
/// - Git worktree list command fails
/// - Output parsing fails
pub fn cmd_list(show_path: bool, color_mode: color::ColorMode) -> Result<()> {
    // Get worktree list in porcelain format
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .context("Failed to execute git worktree list --porcelain")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree list failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Get current directory for active worktree detection
    let current_dir = std::env::current_dir().ok();

    // Determine stream/format based ONLY on TTY status
    // Color mode only affects ANSI emission, not which stream or format
    let is_interactive = std::io::stdout().is_terminal();

    if is_interactive {
        // Interactive mode: enhanced table to stderr (with colors if enabled)
        let entries = parse_worktree_entries(&stdout, current_dir.as_deref());

        // Get commit times for all worktrees
        let commit_times: Vec<Option<DateTime<Utc>>> = entries
            .iter()
            .map(|entry| get_last_commit_time(&std::path::PathBuf::from(&entry.path)))
            .collect();

        // Format and print table to stderr (color_mode controls ANSI emission)
        let lines = format_worktree_table(&entries, &commit_times, show_path, color_mode);
        for line in lines {
            eprintln!("{line}");
        }
    } else {
        // Pipe mode: output to stdout (color_mode still controls ANSI emission)
        if show_path {
            // Full table output to stdout
            let entries = parse_worktree_entries(&stdout, current_dir.as_deref());

            let commit_times: Vec<Option<DateTime<Utc>>> = entries
                .iter()
                .map(|entry| get_last_commit_time(&std::path::PathBuf::from(&entry.path)))
                .collect();

            // Format and print table to stdout
            // color_mode determines whether ANSI codes are included
            let lines = format_worktree_table(&entries, &commit_times, show_path, color_mode);
            for line in lines {
                println!("{line}");
            }
        } else {
            // Simple mode: branch names only - use lightweight parsing without hashes
            let entries = parse_simple_worktree_entries(&stdout);

            for (index, entry) in entries.iter().enumerate() {
                if index == 0 {
                    // Main worktree
                    println!("@");
                } else if let Some(branch) = &entry.branch {
                    // Output branch name (actionable by cd and rm)
                    println!("{branch}");
                } else {
                    // Detached HEAD: output normalized path to make it actionable by cd and rm
                    println!("{}", display_path(&PathBuf::from(&entry.path)));
                }
            }
        }
    }

    Ok(())
}
