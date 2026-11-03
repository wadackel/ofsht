//! Remove command - Remove one or multiple worktrees

use anyhow::{Context, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::time::Duration;

use crate::color;
use crate::commands::common::{current_worktree_toplevel, resolve_worktree_target};
use crate::commands::context::CommandContext;
use crate::domain::worktree::{branch_from_template, WorktreeList};
use crate::hooks;
use crate::integrations;
use crate::integrations::fzf::FzfPicker;
use crate::integrations::git::GitClient;
use crate::path_utils::display_path;

/// How `ofsht rm` treats the branch of a removed worktree
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchDeletion {
    /// `git branch -d`: delete only when fully merged, otherwise keep and warn
    Safe,
    /// Leave the branch untouched
    Keep,
    /// `git branch -D`: delete even when unmerged
    Force,
}

/// Remove a worktree, then handle its branch according to `branch_policy`
fn remove_worktree_internal(
    ctx: &CommandContext,
    worktree_path: &std::path::Path,
    branch_name: Option<&str>,
    label: &str,
    branch_policy: &BranchPolicy,
    mp: &MultiProgress,
) -> Result<()> {
    let git = &ctx.git;
    let config = &ctx.config;
    let repo_root = ctx.repo_root.as_path();
    let color_mode = ctx.color_mode;
    let is_tty = color_mode.should_colorize();

    // Header spinner (TTY) or pre-printed header (non-TTY)
    let header_pb = if is_tty {
        let pb = mp.add(ProgressBar::new_spinner());
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message(format!("Removing {label}"));
        pb.enable_steady_tick(Duration::from_millis(100));
        Some(pb)
    } else {
        // non-TTY: print header before hooks (sync pattern)
        eprintln!("{}", color::success(color_mode, format!("Removed {label}")));
        None
    };

    // Execute delete hooks before removing the worktree (indent 4sp for nesting)
    if worktree_path.exists()
        && (!config.hooks.delete.run.is_empty()
            || !config.hooks.delete.copy.is_empty()
            || !config.hooks.delete.link.is_empty())
    {
        hooks::execute_hooks_lenient_with_mp(
            &config.hooks.delete,
            worktree_path,
            repo_root,
            color_mode,
            "  ",
            mp,
        );
    }

    // Remove worktree using git worktree remove
    if let Err(e) = git.remove_worktree(worktree_path, Some(repo_root)) {
        // Clear header spinner on error
        if let Some(pb) = header_pb {
            pb.finish_and_clear();
        }
        return Err(e);
    }

    // Finish header: Removing → Removed
    if let Some(pb) = header_pb {
        pb.set_style(ProgressStyle::with_template("{msg}").unwrap());
        pb.finish_with_message(format!(
            "{}",
            color::success(color_mode, format!("Removed {label}"))
        ));
    }

    let force = match branch_policy.deletion {
        BranchDeletion::Keep => return Ok(()),
        BranchDeletion::Safe => false,
        BranchDeletion::Force => true,
    };
    let deleted = |branch: &str| {
        hooks::emit_line(
            mp,
            is_tty,
            format!(
                "  {}",
                color::success(color_mode, format!("Deleted branch: {branch}"))
            ),
        );
    };
    let kept = |message: String| {
        hooks::emit_line(
            mp,
            is_tty,
            format!("  {}", color::warn(color_mode, message)),
        );
    };

    if let Some(branch) = branch_name {
        if branch_policy.is_default(branch) {
            kept(format!("Kept branch: {branch} (default branch)"));
        } else if git
            .remove_branch(branch, force, Some(repo_root))
            .unwrap_or(false)
        {
            deleted(branch);
        } else if !force {
            kept(format!(
                "Kept branch: {branch} (not fully merged; use --force-branch to delete)"
            ));
        }
    }

    // The branch the worktree was created with; only a merged one goes, and a failure stays
    // quiet because the name may belong to a branch that is unmerged or in use elsewhere
    let creation_branch = branch_from_template(&config.worktree.dir, repo_root, worktree_path)
        .filter(|name| branch_name != Some(name.as_str()) && !branch_policy.is_default(name));
    if let Some(name) = creation_branch {
        if git
            .remove_branch(&name, false, Some(repo_root))
            .unwrap_or(false)
        {
            deleted(&name);
        }
    }

    Ok(())
}

/// How `rm` treats branches, resolved once for every target
pub struct BranchPolicy {
    pub deletion: BranchDeletion,
    /// Never deleted, even with `--force-branch`: losing unpushed work on them costs the most
    pub default_branches: Vec<String>,
}

impl BranchPolicy {
    fn is_default(&self, branch: &str) -> bool {
        self.default_branches.iter().any(|name| name == branch)
    }
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
pub fn cmd_rm_many(
    targets: &[String],
    branch_deletion: BranchDeletion,
    color_mode: color::ColorMode,
) -> Result<()> {
    // Resolve repo_root, config, and git client once for the whole removal flow.
    let ctx = CommandContext::new_strict(color_mode)?;

    // Get worktree list once for all targets
    let list_stdout = ctx.worktree_list_stdout()?;
    let root = ctx.worktree_root();
    let current = current_worktree_toplevel();
    let branch_policy = BranchPolicy {
        deletion: branch_deletion,
        default_branches: ctx.git.default_branches(Some(&ctx.repo_root)),
    };

    // Resolve targets: CLI args > stdin (when piped) > fzf
    let targets: Vec<String> = if targets.is_empty() {
        let stdin_targets = crate::stdin::try_read_stdin_lines()?;
        if stdin_targets.is_empty() {
            if !ctx.config.integrations.fzf.enabled {
                anyhow::bail!("Provide at least one target or enable fzf in config");
            }

            if !integrations::fzf::is_fzf_available() {
                anyhow::bail!("fzf is not installed. Install it or provide at least one target");
            }

            // Build items for fzf
            let items = integrations::fzf::build_worktree_items(&list_stdout, root.as_deref());

            if items.is_empty() {
                anyhow::bail!("No worktrees found");
            }

            // Use fzf to select (multi-select enabled)
            let picker =
                integrations::fzf::RealFzfPicker::new(ctx.config.integrations.fzf.options.clone());
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
            stdin_targets
        }
    } else {
        targets.to_vec()
    };

    let mp = MultiProgress::new();

    // First, resolve all targets to detect duplicates and validate them
    let mut non_current_removals = Vec::new();
    let mut current_removal: Option<(std::path::PathBuf, std::path::PathBuf, Option<String>)> =
        None;
    let mut seen_paths = HashSet::new();

    for target in &targets {
        match resolve_worktree_target(target, &list_stdout, root.as_deref(), current.as_deref()) {
            Ok((canonical_path, worktree_path, branch_name, is_current)) => {
                // The current worktree, named as `.` or otherwise, is removed last
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
    for (_, worktree_path, branch_name) in &non_current_removals {
        let path_label = display_path(worktree_path);
        let label = branch_name.as_deref().unwrap_or(&path_label);
        remove_worktree_internal(
            &ctx,
            worktree_path,
            branch_name.as_deref(),
            label,
            &branch_policy,
            &mp,
        )?;
    }

    // Remove current worktree last (if requested)
    if let Some((_, worktree_path, branch_name)) = &current_removal {
        let path_label = display_path(worktree_path);
        let label = branch_name.as_deref().unwrap_or(&path_label);
        remove_worktree_internal(
            &ctx,
            worktree_path,
            branch_name.as_deref(),
            label,
            &branch_policy,
            &mp,
        )?;

        // Print main worktree path for shell wrapper
        let list = WorktreeList::parse(&list_stdout, None);
        let main_path = list
            .main()
            .map(|m| m.path.as_str())
            .context("git worktree list returned no entries")?;
        println!("{main_path}");
    }

    Ok(())
}
