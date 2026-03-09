//! Open command - Open all worktrees in tmux windows or panes

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::color;
use crate::commands::common::{
    canonicalize_allow_missing, get_main_repo_root, parse_all_worktrees,
};
use crate::config;
use crate::domain::worktree::{calculate_relative_path, calculate_worktree_root_from_paths};
use crate::integrations::tmux::{sanitize_window_name, RealTmuxLauncher, TmuxLauncher};

/// Worktree entry for the open command
struct OpenWorktree {
    path: String,
    name: String,
}

/// Resolve the open mode from CLI flags and config
fn resolve_mode(pane: bool, window: bool, config_value: &str) -> &'static str {
    if pane {
        return "pane";
    }
    if window {
        return "window";
    }
    match config_value {
        "pane" => "pane",
        _ => "window",
    }
}

/// Get the current worktree path via git rev-parse --show-toplevel
fn get_current_worktree_path() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to get current worktree path")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Not in a git repository: {stderr}");
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(path))
}

/// Build worktree list with names, skipping the current worktree
fn build_worktree_list(
    main_path: &str,
    main_branch: Option<&str>,
    worktrees: &[(String, Option<String>)],
    current_path: &Path,
) -> (Vec<OpenWorktree>, Option<String>) {
    // Build full list: main first, then non-main worktrees
    let mut all_entries: Vec<(String, Option<String>)> = Vec::with_capacity(worktrees.len() + 1);
    all_entries.push((main_path.to_string(), main_branch.map(String::from)));
    all_entries.extend(worktrees.iter().cloned());

    // Calculate worktree root from non-main paths for name calculation
    let non_main_paths: Vec<PathBuf> = worktrees.iter().map(|(p, _)| PathBuf::from(p)).collect();
    let worktree_root = calculate_worktree_root_from_paths(&non_main_paths);

    let canonical_current = canonicalize_allow_missing(current_path);
    let mut skipped_name: Option<String> = None;
    let mut result = Vec::new();

    for (index, (path, _branch)) in all_entries.iter().enumerate() {
        let canonical_path = canonicalize_allow_missing(&PathBuf::from(path));

        // Calculate name
        let name = if index == 0 {
            "@".to_string()
        } else {
            worktree_root
                .as_ref()
                .and_then(|root| calculate_relative_path(&PathBuf::from(path), root))
                .unwrap_or_else(|| {
                    PathBuf::from(path)
                        .file_name()
                        .map_or_else(|| path.clone(), |n| n.to_string_lossy().to_string())
                })
        };

        // Skip current worktree
        if canonical_path == canonical_current {
            skipped_name = Some(name);
            continue;
        }

        result.push(OpenWorktree {
            path: path.clone(),
            name,
        });
    }

    (result, skipped_name)
}

/// Parse main branch from porcelain output
fn parse_main_branch(porcelain_output: &str) -> Option<String> {
    let mut in_first_worktree = false;

    for line in porcelain_output.lines() {
        if line.starts_with("worktree ") {
            if in_first_worktree {
                // We've reached the second worktree without finding a branch
                return None;
            }
            in_first_worktree = true;
        } else if in_first_worktree {
            if let Some(branch_ref) = line.strip_prefix("branch ") {
                let branch = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
                return Some(branch.to_string());
            }
            if line == "detached" {
                return None;
            }
            if line.is_empty() {
                return None;
            }
        }
    }

    None
}

/// Open all worktrees in tmux
///
/// # Errors
/// Returns an error if not in a git repository, not in a tmux session,
/// config loading fails, or tmux operations fail.
pub fn cmd_open(pane: bool, window: bool, color_mode: color::ColorMode) -> Result<()> {
    let repo_root = get_main_repo_root()?;
    let cfg = config::Config::load_from_repo_root(&repo_root)?;

    // Detect tmux — hard error if not available
    let launcher = RealTmuxLauncher;
    launcher.detect()?;

    // Get worktree list
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute git worktree list: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree list failed: {}", stderr.trim());
    }

    let list_stdout = String::from_utf8_lossy(&output.stdout);
    let (main_path, worktrees) = parse_all_worktrees(&list_stdout);
    let main_branch = parse_main_branch(&list_stdout);

    // Detect current worktree
    let current_path = get_current_worktree_path()?;

    // Build list, skipping current worktree
    let (open_list, skipped_name) = build_worktree_list(
        &main_path,
        main_branch.as_deref(),
        &worktrees,
        &current_path,
    );

    if open_list.is_empty() {
        eprintln!("No worktrees to open (all worktrees are already in the current session).");
        return Ok(());
    }

    let mode = resolve_mode(pane, window, &cfg.integrations.tmux.open);

    match mode {
        "pane" => open_as_panes(&open_list, color_mode)?,
        _ => open_as_windows(&launcher, &open_list, color_mode)?,
    }

    let skip_msg = skipped_name
        .as_ref()
        .map_or(String::new(), |name| format!(" (skipped current: {name})"));

    eprintln!(
        "{}",
        color::info(
            color_mode,
            format!(
                "Opened {} worktree(s) as {}s{skip_msg}",
                open_list.len(),
                mode
            )
        )
    );

    Ok(())
}

fn open_as_windows(
    launcher: &RealTmuxLauncher,
    worktrees: &[OpenWorktree],
    color_mode: color::ColorMode,
) -> Result<()> {
    for wt in worktrees {
        let window_name = sanitize_window_name(&wt.name);
        if let Err(e) = launcher.create_window(Path::new(&wt.path), &window_name) {
            anyhow::bail!("Failed to create window for {}: {e}", wt.name);
        }
        eprintln!(
            "{}",
            color::info(color_mode, format!("  + window: {}", wt.name))
        );
    }
    Ok(())
}

fn open_as_panes(worktrees: &[OpenWorktree], color_mode: color::ColorMode) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();
    let mut success_count = 0;

    for wt in worktrees {
        let output = Command::new("tmux")
            .arg("split-window")
            .arg("-h")
            .arg("-c")
            .arg(&wt.path)
            .output()
            .context("Failed to execute tmux split-window")?;

        if output.status.success() {
            success_count += 1;
            eprintln!(
                "{}",
                color::info(color_mode, format!("  + split: {}", wt.name))
            );
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            errors.push(format!("{}: {}", wt.name, stderr.trim()));
            eprintln!(
                "{}",
                color::warn(
                    color_mode,
                    format!("  ! split failed for {}: {}", wt.name, stderr.trim())
                )
            );
        }
    }

    // Apply tiled layout for even distribution
    if success_count > 0 {
        let layout_output = Command::new("tmux")
            .args(["select-layout", "tiled"])
            .output()
            .context("Failed to execute tmux select-layout")?;

        if !layout_output.status.success() {
            let stderr = String::from_utf8_lossy(&layout_output.stderr);
            eprintln!(
                "{}",
                color::warn(
                    color_mode,
                    format!("Warning: select-layout tiled failed: {}", stderr.trim())
                )
            );
        }
    }

    if !errors.is_empty() && success_count == 0 {
        anyhow::bail!("All pane splits failed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_mode_pane_flag() {
        assert_eq!(resolve_mode(true, false, "window"), "pane");
    }

    #[test]
    fn test_resolve_mode_window_flag() {
        assert_eq!(resolve_mode(false, true, "pane"), "window");
    }

    #[test]
    fn test_resolve_mode_no_flags_uses_config_pane() {
        assert_eq!(resolve_mode(false, false, "pane"), "pane");
    }

    #[test]
    fn test_resolve_mode_no_flags_uses_config_window() {
        assert_eq!(resolve_mode(false, false, "window"), "window");
    }

    #[test]
    fn test_resolve_mode_no_flags_invalid_config_defaults_to_window() {
        assert_eq!(resolve_mode(false, false, "invalid"), "window");
        assert_eq!(resolve_mode(false, false, ""), "window");
    }

    #[test]
    fn test_resolve_mode_pane_flag_overrides_config() {
        assert_eq!(resolve_mode(true, false, "window"), "pane");
    }

    #[test]
    fn test_resolve_mode_window_flag_overrides_config() {
        assert_eq!(resolve_mode(false, true, "pane"), "window");
    }

    #[test]
    fn test_parse_main_branch_normal() {
        let porcelain = "worktree /path/to/main\nHEAD abc123\nbranch refs/heads/main\n\n";
        assert_eq!(parse_main_branch(porcelain), Some("main".to_string()));
    }

    #[test]
    fn test_parse_main_branch_detached() {
        let porcelain = "worktree /path/to/main\nHEAD abc123\ndetached\n\n";
        assert_eq!(parse_main_branch(porcelain), None);
    }

    #[test]
    fn test_parse_main_branch_empty() {
        assert_eq!(parse_main_branch(""), None);
    }

    #[test]
    fn test_build_worktree_list_skips_current() {
        let main_path = "/path/to/main";
        let worktrees = vec![
            (
                "/worktrees/feature".to_string(),
                Some("feature".to_string()),
            ),
            ("/worktrees/fix".to_string(), Some("fix".to_string())),
        ];
        let current = PathBuf::from("/path/to/main");

        let (list, skipped) = build_worktree_list(main_path, Some("main"), &worktrees, &current);

        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "feature");
        assert_eq!(list[1].name, "fix");
        assert_eq!(skipped, Some("@".to_string()));
    }

    #[test]
    fn test_build_worktree_list_skips_non_main_current() {
        let main_path = "/path/to/main";
        let worktrees = vec![
            (
                "/worktrees/feature".to_string(),
                Some("feature".to_string()),
            ),
            ("/worktrees/fix".to_string(), Some("fix".to_string())),
        ];
        let current = PathBuf::from("/worktrees/feature");

        let (list, skipped) = build_worktree_list(main_path, Some("main"), &worktrees, &current);

        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "@");
        assert_eq!(list[0].path, "/path/to/main");
        assert_eq!(list[1].name, "fix");
        assert_eq!(skipped, Some("feature".to_string()));
    }

    #[test]
    fn test_build_worktree_list_all_empty_after_skip() {
        let main_path = "/path/to/main";
        let worktrees: Vec<(String, Option<String>)> = vec![];
        let current = PathBuf::from("/path/to/main");

        let (list, skipped) = build_worktree_list(main_path, Some("main"), &worktrees, &current);

        assert!(list.is_empty());
        assert_eq!(skipped, Some("@".to_string()));
    }

    #[test]
    fn test_build_worktree_list_worktree_names_use_relative_paths() {
        let main_path = "/path/to/main";
        let worktrees = vec![
            (
                "/worktrees/feat/foo".to_string(),
                Some("feat/foo".to_string()),
            ),
            (
                "/worktrees/fix/bar".to_string(),
                Some("fix/bar".to_string()),
            ),
        ];
        let current = PathBuf::from("/path/to/main");

        let (list, _) = build_worktree_list(main_path, Some("main"), &worktrees, &current);

        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "feat/foo");
        assert_eq!(list[1].name, "fix/bar");
    }
}
