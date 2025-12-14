//! Add command - Create new worktrees with GitHub integration and tmux support

use anyhow::{Context, Result};
use std::process::Command;

use crate::color;
use crate::commands::common::get_main_repo_root;
use crate::config;
use crate::domain::worktree::normalize_absolute_path;
use crate::hooks;
use crate::integrations;
use crate::integrations::gh::GhClient;
use crate::integrations::tmux::TmuxLauncher;

/// Process a PR and return branch name and start point
fn process_pr(
    pr: &integrations::gh::PrInfo,
    number: u32,
    repo_root: &std::path::Path,
    color_mode: color::ColorMode,
) -> Result<(String, Option<String>)> {
    eprintln!(
        "{}",
        color::success(
            color_mode,
            &format!("Creating worktree for PR #{}: {}", pr.number, pr.title)
        )
    );

    // Check if it's from a fork (cross-repository PR)
    let is_fork = pr.is_cross_repository;

    if is_fork {
        // Fork からの PR
        eprintln!("{}", color::info(color_mode, "Fetching PR from fork…"));

        // Fetch PR ref from GitHub without checking out
        let fetch_output = Command::new("git")
            .args(["fetch", "origin", &format!("refs/pull/{number}/head")])
            .current_dir(repo_root)
            .output()
            .context("Failed to execute git fetch")?;

        if !fetch_output.status.success() {
            let stderr = String::from_utf8_lossy(&fetch_output.stderr);
            anyhow::bail!("git fetch PR ref failed: {stderr}");
        }

        // Check if local branch with PR's name already exists
        let branch_exists = Command::new("git")
            .args(["rev-parse", "--verify", &pr.head_ref_name])
            .current_dir(repo_root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if branch_exists {
            // Conflict: local branch already exists, use unique name
            let sanitized_ref = pr.head_ref_name.replace('/', "-");
            let unique_branch = format!("pr-{number}-{sanitized_ref}");

            eprintln!(
                "{}",
                color::warn(
                    color_mode,
                    &format!(
                        "Local branch '{}' already exists. Using '{}' instead.",
                        pr.head_ref_name, unique_branch
                    )
                )
            );

            Ok((unique_branch, Some("FETCH_HEAD".to_string())))
        } else {
            // Use PR's original branch name
            Ok((pr.head_ref_name.clone(), Some("FETCH_HEAD".to_string())))
        }
    } else {
        // Same repository - fetch the branch
        eprintln!(
            "{}",
            color::info(
                color_mode,
                &format!("Fetching branch: {}", pr.head_ref_name)
            )
        );

        let fetch_output = Command::new("git")
            .args(["fetch", "origin", &pr.head_ref_name])
            .current_dir(repo_root)
            .output()
            .context("Failed to execute git fetch")?;

        if !fetch_output.status.success() {
            let stderr = String::from_utf8_lossy(&fetch_output.stderr);
            anyhow::bail!("git fetch failed: {stderr}");
        }

        // Check if local branch already exists
        let branch_exists = Command::new("git")
            .args(["rev-parse", "--verify", &pr.head_ref_name])
            .current_dir(repo_root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if branch_exists {
            // Existing local branch - use it directly
            Ok((pr.head_ref_name.clone(), None))
        } else {
            // Create new branch tracking remote
            Ok((
                pr.head_ref_name.clone(),
                Some(format!("origin/{}", pr.head_ref_name)),
            ))
        }
    }
}

/// Resolve branch name and start point from GitHub issue/PR
#[allow(clippy::type_complexity)]
fn resolve_github_ref(
    number: u32,
    start_point: Option<&str>,
    repo_root: &std::path::Path,
    color_mode: color::ColorMode,
) -> Result<(String, Option<String>)> {
    let gh_client = integrations::gh::RealGhClient;
    if !gh_client.is_available() {
        anyhow::bail!(
            "GitHub CLI (gh) is not installed or not available.\n\
             Please install gh from https://cli.github.com/ to use GitHub integration.\n\
             Alternatively, use a regular branch name instead of #{number}."
        );
    }

    eprintln!(
        "{}",
        color::info(color_mode, &format!("Fetching GitHub #{number} info…"))
    );

    // Try issue first, then PR if issue fails
    match gh_client.issue_info(number) {
        Ok(issue) => {
            // Check if this is actually a pull request
            if issue.is_pull_request {
                // This is a PR, so fetch PR info instead
                match gh_client.pr_info(number) {
                    Ok(pr) => process_pr(&pr, number, repo_root, color_mode),
                    Err(_pr_err) => {
                        anyhow::bail!(
                            "#{number} is not a valid pull request.\n\
                             Please check the number and try again."
                        );
                    }
                }
            } else {
                // This is a pure issue
                let branch_name = integrations::gh::build_issue_branch(number);
                eprintln!(
                    "{}",
                    color::success(
                        color_mode,
                        &format!(
                            "Creating worktree for issue #{}: {}",
                            issue.number, issue.title
                        )
                    )
                );
                Ok((branch_name, start_point.map(String::from)))
            }
        }
        Err(_issue_err) => {
            // Issue not found, try PR
            match gh_client.pr_info(number) {
                Ok(pr) => process_pr(&pr, number, repo_root, color_mode),
                Err(_pr_err) => {
                    anyhow::bail!(
                        "#{number} is not a valid issue or pull request.\n\
                         Please check the number and try again."
                    );
                }
            }
        }
    }
}

/// Determine if tmux integration should be used based on flags and config
const fn should_use_tmux(
    behavior: config::TmuxBehavior,
    tmux_flag: bool,
    no_tmux_flag: bool,
) -> bool {
    // Priority: --no-tmux > --tmux > behavior setting
    if no_tmux_flag {
        return false;
    }
    if tmux_flag {
        return true;
    }
    // behavior: Auto (default), Always, Never
    matches!(behavior, config::TmuxBehavior::Always)
}

/// Add command - Create new worktree with optional GitHub integration and tmux support
///
/// # Errors
/// Returns an error if:
/// - Not in a git repository
/// - Git worktree creation fails
/// - Hook execution fails
/// - Zoxide registration fails
/// - Tmux integration fails (warning only)
pub fn cmd_new(
    branch: &str,
    start_point: Option<&str>,
    tmux: bool,
    no_tmux: bool,
    color_mode: color::ColorMode,
) -> Result<()> {
    // Get main repository root
    let repo_root = get_main_repo_root()?;

    // Load configuration from repo root
    let config = config::Config::load_from_repo_root(&repo_root)?;

    // Parse branch input to detect GitHub issue/PR references
    let branch_input = integrations::gh::BranchInput::parse(branch);

    // Resolve actual branch name and optional start point from GitHub if needed
    let (actual_branch, actual_start_point) = match branch_input {
        integrations::gh::BranchInput::Github(number) if config.integrations.gh.enabled => {
            resolve_github_ref(number, start_point, &repo_root, color_mode)?
        }
        integrations::gh::BranchInput::Github(number) => {
            // GitHub integration is disabled
            eprintln!(
                "{}",
                color::warn(
                    color_mode,
                    &format!(
                        "GitHub integration is disabled. Treating '#{number}' as a literal branch name.\n\
                         To enable GitHub integration, set enabled = true in [integration.gh] in your global config."
                    )
                )
            );
            (branch.to_string(), start_point.map(String::from))
        }
        integrations::gh::BranchInput::Plain(name) => (name, start_point.map(String::from)),
    };

    let branch = &actual_branch;
    let start_point = actual_start_point.as_deref();

    // Determine if tmux should be used based on flags and config
    let use_tmux = should_use_tmux(config.integrations.tmux.behavior, tmux, no_tmux);

    // Early detection if tmux integration is requested
    if use_tmux {
        let launcher = integrations::tmux::RealTmuxLauncher;
        launcher.detect()?;
    }
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

    // Create tmux window or pane if enabled
    if use_tmux {
        let launcher = integrations::tmux::RealTmuxLauncher;
        let result = match config.integrations.tmux.create.as_str() {
            "pane" => launcher.create_pane(&worktree_path),
            _ => launcher.create_window(&worktree_path, branch),
        };
        if let Err(e) = result {
            eprintln!("Warning: tmux creation failed: {e}");
        }
        // Don't print path to stdout when using tmux
        // (prevents shell integration from cd'ing in the calling shell)
    } else {
        // Print normalized absolute path to STDOUT for shell wrapper integration
        println!("{}", normalize_absolute_path(&worktree_path));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_use_tmux_no_tmux_flag_priority() {
        use config::TmuxBehavior;
        // --no-tmux has highest priority
        assert!(!should_use_tmux(TmuxBehavior::Always, true, true));
        assert!(!should_use_tmux(TmuxBehavior::Always, false, true));
        assert!(!should_use_tmux(TmuxBehavior::Auto, true, true));
        assert!(!should_use_tmux(TmuxBehavior::Auto, false, true));
    }

    #[test]
    fn test_should_use_tmux_tmux_flag_priority() {
        use config::TmuxBehavior;
        // --tmux has second priority
        assert!(should_use_tmux(TmuxBehavior::Never, true, false));
        assert!(should_use_tmux(TmuxBehavior::Auto, true, false));
        assert!(should_use_tmux(TmuxBehavior::Always, true, false));
    }

    #[test]
    fn test_should_use_tmux_behavior_auto() {
        use config::TmuxBehavior;
        // behavior=Auto defaults to false
        assert!(!should_use_tmux(TmuxBehavior::Auto, false, false));
    }

    #[test]
    fn test_should_use_tmux_behavior_always() {
        use config::TmuxBehavior;
        // behavior=Always enables tmux
        assert!(should_use_tmux(TmuxBehavior::Always, false, false));
    }

    #[test]
    fn test_should_use_tmux_behavior_never() {
        use config::TmuxBehavior;
        // behavior=Never disables tmux (unless --tmux is specified)
        assert!(!should_use_tmux(TmuxBehavior::Never, false, false));
    }
}
