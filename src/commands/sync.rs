//! Sync command - Re-apply hook file operations to existing worktrees

use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::path::Path;
use std::time::Duration;

use crate::color;
use crate::commands::common::get_main_repo_root;
use crate::config::{self, HookActions};
use crate::domain::worktree::WorktreeList;
use crate::hooks;
use crate::integrations::git::{GitClient, RealGitClient};

/// Sync hooks.create actions to all existing non-main worktrees
///
/// # Errors
/// Returns an error if not in a git repository, config loading fails,
/// git worktree list fails, or any hook execution fails.
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
pub fn cmd_sync(run: bool, copy: bool, link: bool, color_mode: color::ColorMode) -> Result<()> {
    let repo_root = get_main_repo_root()?;
    let cfg = config::Config::load_from_repo_root(&repo_root)?;

    // No flags = all actions; otherwise use only the specified ones
    let (do_run, do_copy, do_link) = if !run && !copy && !link {
        (true, true, true)
    } else {
        (run, copy, link)
    };

    let create = cfg.hooks.create;
    let actions = HookActions {
        run: if do_run { create.run } else { vec![] },
        copy: if do_copy { create.copy } else { vec![] },
        link: if do_link { create.link } else { vec![] },
    };

    if actions.run.is_empty() && actions.copy.is_empty() && actions.link.is_empty() {
        eprintln!("No hook actions configured for hooks.create. Nothing to sync.");
        return Ok(());
    }

    let git = RealGitClient;
    let list_stdout = git.list_worktrees(Some(&repo_root))?;
    let list = WorktreeList::parse(&list_stdout, None);
    let worktrees = list.non_main();

    if worktrees.is_empty() {
        eprintln!("No non-main worktrees found. Nothing to sync.");
        return Ok(());
    }

    let mp = MultiProgress::new();
    let is_tty = color_mode.should_colorize();
    let mut errors: Vec<String> = vec![];

    for entry in worktrees {
        let path = &entry.path;
        let label = entry.branch.as_deref().unwrap_or(path.as_str());

        // Header spinner (TTY) or pre-printed header (non-TTY)
        let header_pb = if is_tty {
            let pb = mp.add(ProgressBar::new_spinner());
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .unwrap(),
            );
            pb.set_message(format!("Syncing {label}"));
            pb.enable_steady_tick(Duration::from_millis(100));
            Some(pb)
        } else {
            eprintln!("{}", color::success(color_mode, format!("Synced {label}")));
            None
        };

        let worktree_path = Path::new(path);
        if !worktree_path.exists() {
            // Finish header before warning
            if let Some(pb) = header_pb {
                pb.set_style(ProgressStyle::with_template("{msg}").unwrap());
                pb.finish_with_message(format!(
                    "{}",
                    color::success(color_mode, format!("Synced {label}"))
                ));
            }
            hooks::emit_line(
                &mp,
                is_tty,
                format!(
                    "  {}",
                    color::warn(
                        color_mode,
                        format!("Worktree directory not found, skipping: {path}")
                    )
                ),
            );
            continue;
        }

        if let Err(e) =
            hooks::execute_hooks_with_mp(&actions, worktree_path, &repo_root, color_mode, "  ", &mp)
        {
            errors.push(format!("{path}: {e}"));
        }

        // Finish header: Syncing → Synced
        if let Some(pb) = header_pb {
            pb.set_style(ProgressStyle::with_template("{msg}").unwrap());
            pb.finish_with_message(format!(
                "{}",
                color::success(color_mode, format!("Synced {label}"))
            ));
        }
    }

    if !errors.is_empty() {
        let n = errors.len();
        for err in &errors {
            hooks::emit_line(
                &mp,
                is_tty,
                format!("  {}", color::warn(color_mode, format!("Error: {err}"))),
            );
        }
        anyhow::bail!("Sync failed for {n} worktree(s)");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::HookActions;

    fn build_actions(run: bool, copy: bool, link: bool, create: &HookActions) -> HookActions {
        let (do_run, do_copy, do_link) = if !run && !copy && !link {
            (true, true, true)
        } else {
            (run, copy, link)
        };

        HookActions {
            run: if do_run { create.run.clone() } else { vec![] },
            copy: if do_copy { create.copy.clone() } else { vec![] },
            link: if do_link { create.link.clone() } else { vec![] },
        }
    }

    #[test]
    fn test_no_flags_means_all_actions() {
        let create = HookActions {
            run: vec!["echo run".to_string()],
            copy: vec!["file.txt".to_string()],
            link: vec![".env".to_string()],
        };
        let actions = build_actions(false, false, false, &create);
        assert_eq!(actions.run, create.run);
        assert_eq!(actions.copy, create.copy);
        assert_eq!(actions.link, create.link);
    }

    #[test]
    fn test_link_only_flag() {
        let create = HookActions {
            run: vec!["echo run".to_string()],
            copy: vec!["file.txt".to_string()],
            link: vec![".env".to_string()],
        };
        let actions = build_actions(false, false, true, &create);
        assert!(actions.run.is_empty());
        assert!(actions.copy.is_empty());
        assert_eq!(actions.link, create.link);
    }

    #[test]
    fn test_run_copy_flags() {
        let create = HookActions {
            run: vec!["echo run".to_string()],
            copy: vec!["file.txt".to_string()],
            link: vec![".env".to_string()],
        };
        let actions = build_actions(true, true, false, &create);
        assert_eq!(actions.run, create.run);
        assert_eq!(actions.copy, create.copy);
        assert!(actions.link.is_empty());
    }

    #[test]
    fn test_all_flags_same_as_no_flags() {
        let create = HookActions {
            run: vec!["echo run".to_string()],
            copy: vec!["file.txt".to_string()],
            link: vec![".env".to_string()],
        };
        let all_flags = build_actions(true, true, true, &create);
        let no_flags = build_actions(false, false, false, &create);
        assert_eq!(all_flags.run, no_flags.run);
        assert_eq!(all_flags.copy, no_flags.copy);
        assert_eq!(all_flags.link, no_flags.link);
    }

    #[test]
    fn test_empty_config_no_flag() {
        let create = HookActions::default();
        let actions = build_actions(false, false, false, &create);
        assert!(actions.run.is_empty());
        assert!(actions.copy.is_empty());
        assert!(actions.link.is_empty());
    }

    #[test]
    fn test_run_only_config_with_link_flag_yields_empty() {
        let create = HookActions {
            run: vec!["echo run".to_string()],
            copy: vec![],
            link: vec![],
        };
        // --link flag but config has no link entries
        let actions = build_actions(false, false, true, &create);
        assert!(actions.run.is_empty());
        assert!(actions.copy.is_empty());
        assert!(actions.link.is_empty());
    }
}
