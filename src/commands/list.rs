//! List command - Display worktrees and their observed state.

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::color;
use crate::commands::common::current_worktree_toplevel;
use crate::commands::context::CommandContext;
use crate::commands::list_display::format_worktree_table;
use crate::domain::worktree::{WorktreeEntry, WorktreeList};
use crate::integrations::git::{worktree_state, GitClient, RealGitClient};
use crate::path_utils::normalize_absolute_path;

/// List worktrees as a table, plain names, or a JSON array.
///
/// # Errors
/// Returns an error if the Git listing, JSON encoding or stdout write fails.
pub fn cmd_list(
    show_path: bool,
    status: bool,
    json: bool,
    color_mode: color::ColorMode,
) -> Result<()> {
    let ctx = CommandContext::new_lenient(color_mode)?;
    let stdout = if json {
        RealGitClient::worktree_listing_z(&ctx.repo_root)?
    } else {
        ctx.worktree_list_stdout()?
    };
    let root = ctx.worktree_root();
    let current = current_worktree_toplevel().or_else(|| std::env::current_dir().ok());
    let mut list = WorktreeList::parse(&stdout, current.as_deref());
    let interactive = std::io::stdout().is_terminal();
    if !json && !interactive && !show_path && !status {
        for (index, entry) in list.entries().iter().enumerate() {
            let name = if index == 0 {
                "@".to_string()
            } else if let Some(branch) = &entry.branch {
                branch.clone()
            } else {
                normalize_absolute_path(Path::new(&entry.path))
            };
            crate::output::write_stdout(format_args!("{name}\n"))?;
        }
        return Ok(());
    }
    worktree_state::inspect(&mut list, &ctx.repo_root, status);
    let names = list.display_names(root.as_deref());
    if json {
        let rows: Vec<_> = list
            .entries()
            .iter()
            .zip(&names)
            .enumerate()
            .map(|(index, (entry, name))| json_entry(entry, name, index == 0))
            .collect();
        let json = serde_json::to_string_pretty(&rows)?;
        crate::output::write_stdout(format_args!("{json}\n"))?;
    } else {
        let commit_times: Vec<Option<DateTime<Utc>>> = list
            .entries()
            .iter()
            .map(|entry| ctx.git.last_commit_time(&PathBuf::from(&entry.path)))
            .collect();
        let lines = format_worktree_table(
            list.entries(),
            &commit_times,
            show_path,
            color_mode,
            Some(&names),
        );
        for line in lines {
            if interactive {
                eprintln!("{line}");
            } else {
                crate::output::write_stdout(format_args!("{line}\n"))?;
            }
        }
    }
    Ok(())
}

fn json_entry(entry: &WorktreeEntry, name: &str, is_main: bool) -> serde_json::Value {
    let state = &entry.state;
    serde_json::json!({
        "name": name, "path": entry.path, "branch": entry.branch,
        "head": entry.hash, "is_main": is_main, "is_current": entry.is_active,
        "state": {
            "operation": state.operation, "operation_branch": state.operation_branch,
            "locked": state.locked.is_some(), "lock_reason": state.locked.as_ref().filter(|s| !s.is_empty()),
            "prunable": state.prunable.is_some(), "prune_reason": state.prunable.as_ref().filter(|s| !s.is_empty()),
            "missing": state.missing, "dirty": state.dirty, "upstream": state.upstream,
            "ahead": state.ahead, "behind": state.behind, "error": state.error
        }
    })
}
