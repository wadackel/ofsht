//! Sync command - Re-apply hook file operations to existing worktrees

use anyhow::{Context, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::Path;
use std::time::Duration;

use crate::color;
use crate::commands::common::{ambiguous_name_error, current_worktree_toplevel};
use crate::commands::context::CommandContext;
use crate::config::HookActions;
use crate::domain::worktree::{NameResolution, WorktreeEntry, WorktreeList};
use crate::hooks;
use crate::path_utils::canonicalize_allow_missing;

pub struct SyncOptions {
    pub run: bool,
    pub copy: bool,
    pub link: bool,
}

/// Reapply create hooks to selected non-main worktrees, or all when targets are empty.
///
/// # Errors
/// Returns an error for invalid targets, configuration errors or failed hooks.
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
pub fn cmd_sync(
    targets: &[String],
    options: &SyncOptions,
    verbose: bool,
    color_mode: color::ColorMode,
) -> Result<()> {
    let &SyncOptions { run, copy, link } = options;
    let ctx = CommandContext::new_strict(color_mode)?;
    let root = ctx.worktree_root();
    let current = current_worktree_toplevel();
    let list_stdout = ctx.worktree_list_stdout()?;
    let list = WorktreeList::parse(&list_stdout, current.as_deref());
    let worktrees = select_worktrees(&list, targets, root.as_deref())?;
    let (do_run, do_copy, do_link) = if !run && !copy && !link {
        (true, true, true)
    } else {
        (run, copy, link)
    };
    let create = ctx.config.hooks.create.clone();
    let actions = HookActions {
        run: if do_run { create.run } else { vec![] },
        copy: if do_copy { create.copy } else { vec![] },
        link: if do_link { create.link } else { vec![] },
    };
    if actions.run.is_empty() && actions.copy.is_empty() && actions.link.is_empty() {
        eprintln!("No hook actions configured for hooks.create. Nothing to sync.");
        return Ok(());
    }
    if worktrees.is_empty() {
        eprintln!("No non-main worktrees found. Nothing to sync.");
        return Ok(());
    }

    let mp = MultiProgress::new();
    let is_tty = std::io::stderr().is_terminal() && ctx.color_mode.should_colorize();
    let mut failures = 0;
    for entry in worktrees {
        let path = Path::new(&entry.path);
        let label = list.display_name(entry, root.as_deref());
        if !path.exists() {
            mp.suspend(|| {
                eprintln!(
                    "{}",
                    color::warn(
                        color_mode,
                        format!(
                            "Skipped {label}: worktree directory not found: {}",
                            path.display()
                        )
                    )
                );
            });
            continue;
        }
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
            eprintln!("Syncing {label}");
            None
        };
        let result = hooks::HookContext::new(path, &ctx.repo_root, root.as_deref(), verbose)
            .and_then(|context| {
                hooks::execute_hooks_with_mp(&actions, &context, color_mode, "  ", &mp)
            });
        if let Some(pb) = header_pb {
            pb.finish_and_clear();
        }
        match result {
            Ok(()) => mp
                .suspend(|| eprintln!("{}", color::success(color_mode, format!("Synced {label}")))),
            Err(error) => {
                mp.suspend(|| {
                    eprintln!(
                        "{}",
                        color::warn(color_mode, format!("Failed {label}: {error:#}"))
                    );
                });
                failures += 1;
            }
        }
    }
    if failures > 0 {
        anyhow::bail!("Sync failed for {failures} worktree(s)");
    }
    Ok(())
}

fn select_worktrees<'a>(
    list: &'a WorktreeList,
    targets: &[String],
    root: Option<&Path>,
) -> Result<Vec<&'a WorktreeEntry>> {
    if targets.is_empty() {
        return Ok(list.non_main().iter().collect());
    }
    let main = list
        .main()
        .context("git worktree list returned no entries")?;
    let main_path = canonicalize_allow_missing(Path::new(&main.path));
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for target in targets {
        if target == "@" || main.branch.as_deref() == Some(target.as_str()) {
            anyhow::bail!("Cannot sync main worktree: {target}");
        }
        let entry = if target == "." {
            list.current()
                .context("Current directory does not match a registered worktree")?
        } else {
            match list.resolve_name(target, root) {
                NameResolution::Found(entry) => entry,
                NameResolution::Ambiguous(entries) => {
                    return Err(ambiguous_name_error(target, &entries))
                }
                NameResolution::NotFound => {
                    if canonicalize_allow_missing(Path::new(target)) == main_path {
                        anyhow::bail!("Cannot sync main worktree: {target}");
                    }
                    anyhow::bail!("Worktree not found: {target}");
                }
            }
        };
        if entry.path == main.path {
            anyhow::bail!("Cannot sync main worktree: {target}");
        }
        if seen.insert(canonicalize_allow_missing(Path::new(&entry.path))) {
            selected.push(entry);
        }
    }
    Ok(selected)
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
