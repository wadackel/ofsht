//! Remove command - Remove one or multiple worktrees

use anyhow::{Context, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::io::IsTerminal;
use std::time::Duration;

use crate::color;
use crate::commands::common::{current_worktree_toplevel, resolve_worktree_target};
use crate::commands::context::CommandContext;
use crate::domain::worktree::{branch_from_template, detached_heads, locked_paths, WorktreeList};
use crate::hooks;
use crate::integrations;
use crate::integrations::fzf::FzfPicker;
use crate::integrations::git::GitClient;
use crate::path_utils::{canonicalize_allow_missing, display_path};

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

/// The flags of `ofsht rm`
pub struct RmOptions {
    pub branch_deletion: BranchDeletion,
    /// `--force`: override every preflight refusal except a lock
    pub force: bool,
    /// `--dry-run`: print the removal plan and change nothing
    pub dry_run: bool,
    pub verbose: bool,
}

/// How `rm` treats worktrees and their branches, resolved once for every target
struct RemovalPolicy {
    verbose: bool,
    deletion: BranchDeletion,
    /// `git worktree remove --force`: discard modified and untracked files and submodules
    force_worktree: bool,
    /// Never deleted, even with `--force-branch`: losing unpushed work on them costs the most
    default_branches: Vec<String>,
}

impl RemovalPolicy {
    fn is_default(&self, branch: &str) -> bool {
        self.default_branches.iter().any(|name| name == branch)
    }
}

/// What `rm` does with one branch after removing a worktree
#[derive(Debug, PartialEq, Eq)]
enum BranchAction {
    /// A default branch, kept whatever the flags say
    KeepDefault(String),
    /// `--keep-branch`
    Keep(String),
    /// `git branch -d`, or `-D` when `force`; a failure prints a warning only when `warn`
    Delete {
        branch: String,
        force: bool,
        warn: bool,
    },
}

/// The branch actions for a worktree on `branch` that was created as `creation_branch`
///
/// The creation branch only goes with `-d`, and its failure stays quiet, because the name may
/// belong to a branch that is unmerged or in use elsewhere.
fn branch_actions(
    policy: &RemovalPolicy,
    branch: Option<&str>,
    creation_branch: Option<&str>,
) -> Vec<BranchAction> {
    let force = match policy.deletion {
        BranchDeletion::Keep => {
            return branch
                .map(|b| BranchAction::Keep(b.to_string()))
                .into_iter()
                .collect()
        }
        BranchDeletion::Safe => false,
        BranchDeletion::Force => true,
    };
    let mut actions = Vec::new();
    if let Some(branch) = branch {
        actions.push(if policy.is_default(branch) {
            BranchAction::KeepDefault(branch.to_string())
        } else {
            BranchAction::Delete {
                branch: branch.to_string(),
                force,
                warn: !force,
            }
        });
    }
    if let Some(name) =
        creation_branch.filter(|name| branch != Some(*name) && !policy.is_default(name))
    {
        actions.push(BranchAction::Delete {
            branch: name.to_string(),
            force: false,
            warn: false,
        });
    }
    actions
}

/// Remove a worktree, then handle its branch according to `policy`
fn remove_worktree_internal(
    ctx: &CommandContext,
    worktree_path: &std::path::Path,
    branch_name: Option<&str>,
    label: &str,
    policy: &RemovalPolicy,
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
        // non-TTY: the hooks' lines nest under this header, and `Removed` follows only success
        eprintln!("Removing {label}");
        None
    };

    // Execute delete hooks before removing the worktree (indent 4sp for nesting)
    if worktree_path.exists()
        && (!config.hooks.delete.run.is_empty()
            || !config.hooks.delete.copy.is_empty()
            || !config.hooks.delete.link.is_empty())
    {
        let hook_context = hooks::HookContext::new(
            worktree_path,
            repo_root,
            ctx.worktree_root().as_deref(),
            policy.verbose,
        )?;
        hooks::execute_hooks_lenient_with_mp(
            &config.hooks.delete,
            &hook_context,
            color_mode,
            "  ",
            mp,
        );
    }

    // Remove worktree using git worktree remove
    if let Err(e) = git.remove_worktree(worktree_path, policy.force_worktree, Some(repo_root)) {
        // Clear header spinner on error
        if let Some(pb) = header_pb {
            pb.finish_and_clear();
        }
        return Err(e);
    }

    // Finish header: Removing → Removed
    let removed = format!("{}", color::success(color_mode, format!("Removed {label}")));
    if let Some(pb) = header_pb {
        pb.set_style(ProgressStyle::with_template("{msg}").unwrap());
        pb.finish_with_message(removed);
    } else {
        eprintln!("{removed}");
    }

    let line = |message: String| hooks::emit_line(mp, is_tty, format!("  {message}"));
    let creation_branch = branch_from_template(&config.worktree.dir, repo_root, worktree_path);
    for action in branch_actions(policy, branch_name, creation_branch.as_deref()) {
        match action {
            BranchAction::Keep(_) => {}
            BranchAction::KeepDefault(branch) => line(
                color::warn(
                    color_mode,
                    format!("Kept branch: {branch} (default branch)"),
                )
                .to_string(),
            ),
            BranchAction::Delete {
                branch,
                force,
                warn,
            } => {
                if git
                    .remove_branch(&branch, force, Some(repo_root))
                    .unwrap_or(false)
                {
                    line(
                        color::success(color_mode, format!("Deleted branch: {branch}")).to_string(),
                    );
                } else if warn {
                    line(
                        color::warn(
                            color_mode,
                            format!(
                                "Kept branch: {branch} (not fully merged; use --force-branch to delete)"
                            ),
                        )
                        .to_string(),
                    );
                }
            }
        }
    }

    Ok(())
}

/// Print what removing the worktree would do, without changing anything
fn print_removal_plan(
    ctx: &CommandContext,
    worktree_path: &std::path::Path,
    branch_name: Option<&str>,
    policy: &RemovalPolicy,
) {
    let config = &ctx.config;
    let exists = worktree_path.exists();
    let note = if exists {
        ""
    } else {
        " (directory missing; only the registration)"
    };
    eprintln!("Would remove {}{note}", display_path(worktree_path));
    if exists {
        let hooks = &config.hooks.delete;
        for cmd in &hooks.run {
            eprintln!("  would run hook: {cmd}");
        }
        for pattern in &hooks.copy {
            eprintln!("  would copy: {pattern}");
        }
        for pattern in &hooks.link {
            eprintln!("  would link: {pattern}");
        }
    }
    let creation_branch = branch_from_template(&config.worktree.dir, &ctx.repo_root, worktree_path);
    for action in branch_actions(policy, branch_name, creation_branch.as_deref()) {
        match action {
            BranchAction::KeepDefault(branch) => {
                eprintln!("  would keep branch {branch} (default branch)");
            }
            BranchAction::Keep(branch) => eprintln!("  would keep branch {branch}"),
            BranchAction::Delete {
                branch,
                force: true,
                ..
            } => eprintln!("  would delete branch {branch}"),
            BranchAction::Delete { branch, .. } => {
                eprintln!("  would try git branch -d {branch}");
            }
        }
    }
}

/// Why git would refuse to remove each of `paths`, checked before any hook runs
///
/// The lock applies even with `force`, as `git worktree remove` needs `--force` twice for it.
/// A target that cannot be checked is refused, so nothing is removed on a guess.
fn preflight(
    git: &impl GitClient,
    paths: &[&std::path::Path],
    list_stdout: &str,
    repo_root: &std::path::Path,
    force: bool,
) -> Vec<(std::path::PathBuf, String)> {
    let locked: Vec<std::path::PathBuf> = locked_paths(list_stdout)
        .iter()
        .map(|path| canonicalize_allow_missing(std::path::Path::new(path)))
        .collect();
    let detached: Vec<(std::path::PathBuf, String)> = detached_heads(list_stdout)
        .into_iter()
        .map(|(path, head)| {
            (
                canonicalize_allow_missing(std::path::Path::new(&path)),
                head,
            )
        })
        .collect();
    let mut refused = Vec::new();
    for path in paths {
        if locked.contains(&canonicalize_allow_missing(path)) {
            refused.push((
                path.to_path_buf(),
                format!("locked (run git worktree unlock {} first)", path.display()),
            ));
            continue;
        }
        if force || !path.exists() {
            continue;
        }
        let checks = git
            .run(
                &["status", "--porcelain", "--ignore-submodules=none"],
                Some(path),
            )
            .and_then(|status| {
                let files = git.run(&["ls-files", "-s", "-z"], Some(path))?;
                let modules = git.run(&["rev-parse", "--git-path", "modules"], Some(path))?;
                Ok((status, files, path.join(modules.trim())))
            });
        match checks {
            Err(e) => refused.push((
                path.to_path_buf(),
                format!("cannot check: {}", e.to_string().trim_end()),
            )),
            Ok((status, files, modules)) => {
                if !status.trim().is_empty() {
                    refused.push((
                        path.to_path_buf(),
                        "has modified or untracked files (use --force to discard them)".to_string(),
                    ));
                }
                // Same rules as git: the worktree's modules directory (kept after a deinit), or a
                // gitlink whose directory holds a .git
                let initialized = modules.is_dir()
                    || files.split('\0').any(|entry| {
                        entry.starts_with("160000 ")
                            && entry
                                .split_once('\t')
                                .is_some_and(|(_, name)| path.join(name).join(".git").exists())
                    });
                if initialized {
                    refused.push((
                        path.to_path_buf(),
                        "contains initialized submodules (use --force)".to_string(),
                    ));
                }
                for reason in unrecoverable_work(git, path, &detached, repo_root) {
                    refused.push((path.to_path_buf(), reason));
                }
            }
        }
    }
    refused
}

/// Work that removing `path` would lose for good: commits only its detached HEAD reaches, and
/// an operation in progress in its git directory
fn unrecoverable_work(
    git: &impl GitClient,
    path: &std::path::Path,
    detached: &[(std::path::PathBuf, String)],
    repo_root: &std::path::Path,
) -> Vec<String> {
    let mut reasons = Vec::new();
    let canonical = canonicalize_allow_missing(path);
    if let Some((_, head)) = detached.iter().find(|(p, _)| *p == canonical) {
        // From the repository root: the target's own refs (refs/worktree, refs/bisect) go with it
        match git.run(
            &["for-each-ref", "--contains", head, "--count=1", "refs/"],
            Some(repo_root),
        ) {
            Ok(refs) if refs.trim().is_empty() => reasons.push(
                "has commits on a detached HEAD that no branch or tag contains (use --force to discard them)"
                    .to_string(),
            ),
            Ok(_) => {}
            Err(e) => reasons.push(format!("cannot check: {}", e.to_string().trim_end())),
        }
    }
    match git.run(&["rev-parse", "--absolute-git-dir"], Some(path)) {
        Ok(git_dir) => {
            let git_dir = std::path::Path::new(git_dir.trim());
            let has = |name: &str| git_dir.join(name).exists();
            let operation = if has("rebase-merge") || has("rebase-apply") {
                Some("rebase")
            } else if has("MERGE_HEAD") {
                Some("merge")
            } else if has("CHERRY_PICK_HEAD") {
                Some("cherry-pick")
            } else if has("REVERT_HEAD") {
                Some("revert")
            } else if has("BISECT_LOG") {
                Some("bisect")
            } else if has("sequencer") {
                // A resolved multi-commit cherry-pick or revert leaves only the sequencer
                Some("cherry-pick or revert")
            } else {
                None
            };
            if let Some(operation) = operation {
                reasons.push(format!(
                    "has a {operation} in progress (use --force to abandon it)"
                ));
            }
        }
        Err(e) => reasons.push(format!("cannot check: {}", e.to_string().trim_end())),
    }
    reasons
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
    options: &RmOptions,
    color_mode: color::ColorMode,
) -> Result<()> {
    let &RmOptions {
        branch_deletion,
        force,
        dry_run,
        verbose,
    } = options;
    // Resolve repo_root, config, and git client once for the whole removal flow.
    let ctx = CommandContext::new_strict(color_mode)?;

    // Get worktree list once for all targets
    let list_stdout = ctx.worktree_list_stdout()?;
    let root = ctx.worktree_root();
    let current = current_worktree_toplevel();
    let policy = RemovalPolicy {
        verbose,
        deletion: branch_deletion,
        force_worktree: force,
        default_branches: ctx.git.default_branches(Some(&ctx.repo_root)),
    };

    // Resolve targets: CLI args > stdin (when piped) > fzf
    let targets: Vec<String> = if targets.is_empty() {
        let stdin_targets = crate::stdin::try_read_stdin_lines()?;
        if stdin_targets.is_empty() {
            if !std::io::stdin().is_terminal() {
                anyhow::bail!("at least one target required (provide as argument or via stdin)");
            }
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

    let paths: Vec<&std::path::Path> = non_current_removals
        .iter()
        .chain(current_removal.iter())
        .map(|(_, path, _)| path.as_path())
        .collect();
    let refused = preflight(&ctx.git, &paths, &list_stdout, &ctx.repo_root, force);
    if !refused.is_empty() {
        let lines: Vec<String> = refused
            .iter()
            .map(|(path, reason)| format!("  {}: {reason}", display_path(path)))
            .collect();
        let count = refused
            .iter()
            .map(|(path, _)| path)
            .collect::<HashSet<_>>()
            .len();
        if dry_run {
            anyhow::bail!(
                "Would refuse:\n{}\nNothing would be removed: {count} worktree(s) refused",
                lines.join("\n")
            );
        }
        anyhow::bail!("Cannot remove {count} worktree(s):\n{}", lines.join("\n"));
    }

    let cleanup = if ctx.config.integrations.tmux.close_on_rm && std::env::var_os("TMUX").is_some()
    {
        match crate::integrations::git::RealGitClient::worktree_paths(&ctx.repo_root)
            .and_then(|roots| crate::integrations::tmux::CleanupPlan::capture(&roots, &paths))
        {
            Ok(plan) => Some(plan),
            Err(error) => {
                eprintln!("Warning: tmux cleanup skipped: {error:#}");
                None
            }
        }
    } else {
        None
    };

    if dry_run {
        for (_, worktree_path, branch_name) in non_current_removals.iter().chain(&current_removal) {
            print_removal_plan(&ctx, worktree_path, branch_name.as_deref(), &policy);
        }
        if let Some(plan) = &cleanup {
            plan.print_dry_run();
        }
        return Ok(());
    }

    let mut removed = Vec::new();
    let result = (|| -> Result<()> {
        for (_, worktree_path, branch_name) in &non_current_removals {
            let path_label = display_path(worktree_path);
            let label = branch_name.as_deref().unwrap_or(&path_label);
            remove_worktree_internal(
                &ctx,
                worktree_path,
                branch_name.as_deref(),
                label,
                &policy,
                &mp,
            )?;
            removed.push(canonicalize_allow_missing(worktree_path));
        }

        if let Some((_, worktree_path, branch_name)) = &current_removal {
            let path_label = display_path(worktree_path);
            let label = branch_name.as_deref().unwrap_or(&path_label);
            remove_worktree_internal(
                &ctx,
                worktree_path,
                branch_name.as_deref(),
                label,
                &policy,
                &mp,
            )?;
            removed.push(canonicalize_allow_missing(worktree_path));

            let list = WorktreeList::parse(&list_stdout, None);
            let main_path = list
                .main()
                .map(|m| m.path.as_str())
                .context("git worktree list returned no entries")?;
            crate::output::write_stdout(format_args!("{main_path}\n"))?;
        }

        Ok(())
    })();
    if let Some(plan) = &cleanup {
        plan.finish(&removed);
    }
    result
}
