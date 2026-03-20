//! Sync command - Re-apply hook file operations to existing worktrees

use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::color;
use crate::commands::common::{get_main_repo_root, parse_all_worktrees};
use crate::config::{self, HookActions};
use crate::hooks;

/// Sync hooks.create actions to all existing non-main worktrees
///
/// # Errors
/// Returns an error if not in a git repository, config loading fails,
/// git worktree list fails, or any hook execution fails.
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
    let (_main_path, worktrees) = parse_all_worktrees(&list_stdout);

    if worktrees.is_empty() {
        eprintln!("No non-main worktrees found. Nothing to sync.");
        return Ok(());
    }

    let mut errors: Vec<String> = vec![];

    for (path, branch) in &worktrees {
        let label = branch
            .as_ref()
            .map_or_else(|| format!("Synced {path}"), |b| format!("Synced {b}"));
        eprintln!("  {}", color::success(color_mode, label));

        let worktree_path = Path::new(path);
        if !worktree_path.exists() {
            eprintln!(
                "    {}",
                color::warn(
                    color_mode,
                    format!("Worktree directory not found, skipping: {path}")
                )
            );
            continue;
        }

        if let Err(e) =
            hooks::execute_hooks(&actions, worktree_path, &repo_root, color_mode, "    ")
        {
            errors.push(format!("{path}: {e}"));
        }
    }

    if !errors.is_empty() {
        let n = errors.len();
        for err in &errors {
            eprintln!("    {}", color::warn(color_mode, format!("Error: {err}")));
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
