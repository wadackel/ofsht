//! Open command - Open all worktrees in tmux windows or panes

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::color;
use crate::commands::context::CommandContext;
use crate::config::OpenMode;
use crate::domain::worktree::WorktreeList;
use crate::integrations::git::{GitClient, RealGitClient};
use crate::integrations::tmux::{RealTmuxLauncher, TmuxLauncher};
use crate::path_utils::canonicalize_allow_missing;

/// Worktree entry for the open command
struct OpenWorktree {
    path: String,
    name: String,
}

/// Resolve the open mode from CLI flags and config
const fn resolve_mode(pane: bool, window: bool, config_value: OpenMode) -> OpenMode {
    if pane {
        return OpenMode::Pane;
    }
    if window {
        return OpenMode::Window;
    }
    config_value
}

/// Get the current worktree path via git rev-parse --show-toplevel
fn get_current_worktree_path(git: &RealGitClient) -> Result<PathBuf> {
    let stdout = git
        .rev_parse(&["rev-parse", "--show-toplevel"], None)
        .map_err(|e| anyhow::anyhow!("Not in a git repository: {e}"))?;
    Ok(PathBuf::from(stdout.trim()))
}

/// Build worktree list with names, skipping the current worktree
fn build_worktree_list(
    list: &WorktreeList,
    root: Option<&Path>,
    current_path: &Path,
) -> (Vec<OpenWorktree>, Option<String>) {
    let canonical_current = canonicalize_allow_missing(current_path);
    let mut skipped_name: Option<String> = None;
    let mut result = Vec::new();

    for (entry, name) in list.entries().iter().zip(list.display_names(root)) {
        let canonical_path = canonicalize_allow_missing(&PathBuf::from(&entry.path));

        // Skip current worktree
        if canonical_path == canonical_current {
            skipped_name = Some(name);
            continue;
        }

        result.push(OpenWorktree {
            path: entry.path.clone(),
            name,
        });
    }

    (result, skipped_name)
}

/// Open all worktrees in tmux
///
/// # Errors
/// Returns an error if not in a git repository, not in a tmux session,
/// config loading fails, or tmux operations fail.
pub fn cmd_open(pane: bool, window: bool, color_mode: color::ColorMode) -> Result<()> {
    let ctx = CommandContext::new_strict(color_mode)?;

    // Detect tmux — hard error if not available
    let launcher = RealTmuxLauncher;
    launcher.detect()?;

    // Get worktree list
    let list_stdout = ctx.worktree_list_stdout()?;
    let list = WorktreeList::parse(&list_stdout, None);
    list.main()
        .context("git worktree list returned no entries")?;

    // Detect current worktree
    let current_path = get_current_worktree_path(&ctx.git)?;

    // Build list, skipping current worktree
    let root = ctx.worktree_root();
    let (open_list, skipped_name) = build_worktree_list(&list, root.as_deref(), &current_path);

    if open_list.is_empty() {
        eprintln!("No worktrees to open (all worktrees are already in the current session).");
        return Ok(());
    }

    let mode = resolve_mode(pane, window, ctx.config.integrations.tmux.open);

    match mode {
        OpenMode::Pane => open_as_panes(&launcher, &open_list, color_mode)?,
        OpenMode::Window => open_as_windows(&launcher, &open_list, color_mode)?,
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
    launcher: &dyn TmuxLauncher,
    worktrees: &[OpenWorktree],
    color_mode: color::ColorMode,
) -> Result<()> {
    for wt in worktrees {
        if let Err(e) = launcher.create_window(Path::new(&wt.path), &wt.name) {
            anyhow::bail!("Failed to create window for {}: {e}", wt.name);
        }
        eprintln!(
            "{}",
            color::info(color_mode, format!("  + window: {}", wt.name))
        );
    }
    Ok(())
}

fn open_as_panes(
    launcher: &dyn TmuxLauncher,
    worktrees: &[OpenWorktree],
    color_mode: color::ColorMode,
) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();
    let mut success_count = 0;

    for wt in worktrees {
        match launcher.split_pane(Path::new(&wt.path)) {
            Ok(()) => {
                success_count += 1;
                eprintln!(
                    "{}",
                    color::info(color_mode, format!("  + split: {}", wt.name))
                );
            }
            Err(e) => {
                let msg = e.to_string();
                errors.push(format!("{}: {msg}", wt.name));
                eprintln!(
                    "{}",
                    color::warn(
                        color_mode,
                        format!("  ! split failed for {}: {msg}", wt.name)
                    )
                );
            }
        }
    }

    // Apply tiled layout for even distribution
    if success_count > 0 {
        if let Err(e) = launcher.select_tiled_layout() {
            eprintln!(
                "{}",
                color::warn(
                    color_mode,
                    format!("Warning: select-layout tiled failed: {e}")
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
    use crate::color::ColorMode;
    use crate::integrations::tmux::tests::MockTmuxLauncher;

    fn worktrees_for_test(n: usize) -> Vec<OpenWorktree> {
        (0..n)
            .map(|i| OpenWorktree {
                path: format!("/tmp/wt{i}"),
                name: format!("wt{i}"),
            })
            .collect()
    }

    #[test]
    fn test_open_as_panes_all_success_calls_select_tiled_layout_once() {
        let mock = MockTmuxLauncher::default();
        let wts = worktrees_for_test(3);
        let result = open_as_panes(&mock, &wts, ColorMode::Never);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert_eq!(
            mock.select_tiled_layout_calls.get(),
            1,
            "select_tiled_layout should be invoked exactly once when at least one split succeeds"
        );
    }

    #[test]
    fn test_open_as_panes_all_split_fail_bails_and_skips_layout() {
        let mock = MockTmuxLauncher {
            split_pane_should_fail: true,
            ..Default::default()
        };
        let wts = worktrees_for_test(2);
        let result = open_as_panes(&mock, &wts, ColorMode::Never);
        assert!(result.is_err(), "expected Err when all splits fail");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("All pane splits failed"),
            "unexpected error message: {msg}"
        );
        assert_eq!(
            mock.select_tiled_layout_calls.get(),
            0,
            "select_tiled_layout must not be invoked when success_count is 0"
        );
    }

    #[test]
    fn test_open_as_panes_select_layout_failure_is_warn_only() {
        let mock = MockTmuxLauncher {
            select_tiled_layout_should_fail: true,
            ..Default::default()
        };
        let wts = worktrees_for_test(2);
        let result = open_as_panes(&mock, &wts, ColorMode::Never);
        assert!(
            result.is_ok(),
            "select_tiled_layout failure must not propagate as Err: {result:?}"
        );
        assert_eq!(mock.select_tiled_layout_calls.get(), 1);
    }

    #[test]
    fn test_resolve_mode_pane_flag() {
        assert_eq!(resolve_mode(true, false, OpenMode::Window), OpenMode::Pane);
    }

    #[test]
    fn test_resolve_mode_window_flag() {
        assert_eq!(resolve_mode(false, true, OpenMode::Pane), OpenMode::Window);
    }

    #[test]
    fn test_resolve_mode_no_flags_uses_config_pane() {
        assert_eq!(resolve_mode(false, false, OpenMode::Pane), OpenMode::Pane);
    }

    #[test]
    fn test_resolve_mode_no_flags_uses_config_window() {
        assert_eq!(
            resolve_mode(false, false, OpenMode::Window),
            OpenMode::Window
        );
    }

    #[test]
    fn test_resolve_mode_pane_flag_overrides_config() {
        assert_eq!(resolve_mode(true, false, OpenMode::Window), OpenMode::Pane);
    }

    #[test]
    fn test_resolve_mode_window_flag_overrides_config() {
        assert_eq!(resolve_mode(false, true, OpenMode::Pane), OpenMode::Window);
    }

    #[test]
    fn test_main_branch_via_worktree_list_normal() {
        let porcelain = "worktree /path/to/main\nHEAD abc123\nbranch refs/heads/main\n\n";
        let list = WorktreeList::parse(porcelain, None);
        assert_eq!(list.main().and_then(|m| m.branch.as_deref()), Some("main"));
    }

    #[test]
    fn test_main_branch_via_worktree_list_detached() {
        let porcelain = "worktree /path/to/main\nHEAD abc123\ndetached\n\n";
        let list = WorktreeList::parse(porcelain, None);
        assert_eq!(list.main().and_then(|m| m.branch.as_deref()), None);
    }

    #[test]
    fn test_main_branch_via_worktree_list_empty() {
        let list = WorktreeList::parse("", None);
        assert!(list.main().is_none());
    }

    /// Parse `(path, branch)` pairs; the first pair is the main worktree.
    fn list_of(worktrees: &[(&str, &str)]) -> WorktreeList {
        let blocks: Vec<String> = worktrees
            .iter()
            .map(|(path, branch)| {
                format!("worktree {path}\nHEAD abc123\nbranch refs/heads/{branch}\n\n")
            })
            .collect();
        WorktreeList::parse(&blocks.concat(), None)
    }

    #[test]
    fn test_build_worktree_list_skips_current() {
        let worktrees = list_of(&[
            ("/path/to/main", "main"),
            ("/worktrees/feature", "feature"),
            ("/worktrees/fix", "fix"),
        ]);
        let current = PathBuf::from("/path/to/main");

        let (list, skipped) =
            build_worktree_list(&worktrees, Some(Path::new("/worktrees")), &current);

        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "feature");
        assert_eq!(list[1].name, "fix");
        assert_eq!(skipped, Some("@".to_string()));
    }

    #[test]
    fn test_build_worktree_list_skips_non_main_current() {
        let worktrees = list_of(&[
            ("/path/to/main", "main"),
            ("/worktrees/feature", "feature"),
            ("/worktrees/fix", "fix"),
        ]);
        let current = PathBuf::from("/worktrees/feature");

        let (list, skipped) =
            build_worktree_list(&worktrees, Some(Path::new("/worktrees")), &current);

        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "@");
        assert_eq!(list[0].path, "/path/to/main");
        assert_eq!(list[1].name, "fix");
        assert_eq!(skipped, Some("feature".to_string()));
    }

    #[test]
    fn test_build_worktree_list_all_empty_after_skip() {
        let worktrees = list_of(&[("/path/to/main", "main")]);
        let current = PathBuf::from("/path/to/main");

        let (list, skipped) =
            build_worktree_list(&worktrees, Some(Path::new("/worktrees")), &current);

        assert!(list.is_empty());
        assert_eq!(skipped, Some("@".to_string()));
    }

    #[test]
    fn test_build_worktree_list_worktree_names_use_paths_below_root() {
        let worktrees = list_of(&[
            ("/path/to/main", "main"),
            ("/worktrees/feat/foo", "feat/foo"),
            ("/worktrees/fix/bar", "fix/bar"),
        ]);
        let current = PathBuf::from("/path/to/main");

        let (list, _) = build_worktree_list(&worktrees, Some(Path::new("/worktrees")), &current);

        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "feat/foo");
        assert_eq!(list[1].name, "fix/bar");
    }

    #[test]
    fn test_build_worktree_list_keeps_outside_worktree_short() {
        let worktrees = list_of(&[
            ("/path/to/main", "main"),
            ("/worktrees/feature", "feature"),
            ("/elsewhere/deep/review", "fix/typo"),
        ]);
        let current = PathBuf::from("/path/to/main");

        let (list, _) = build_worktree_list(&worktrees, Some(Path::new("/worktrees")), &current);

        assert_eq!(list[0].name, "feature");
        assert_eq!(list[1].name, "review");
    }
}
