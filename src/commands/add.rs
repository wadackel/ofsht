//! Add command - Create new worktrees with GitHub integration and tmux support

use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::color;
use crate::commands::common::get_main_repo_root;
use crate::config;
use crate::hooks;
use crate::integrations;
use crate::integrations::git::{GitClient, RealGitClient};
use crate::integrations::tmux::TmuxLauncher;
use crate::integrations::zoxide::{is_zoxide_available, RealZoxideClient};
use crate::path_utils::normalize_absolute_path;
use crate::service::{CreateWorktreeRequest, WorktreeService};

/// Process a PR and return branch name and start point
fn process_pr(
    pr: &integrations::gh::PrInfo,
    number: u32,
    repo_root: &std::path::Path,
    color_mode: color::ColorMode,
) -> Result<(String, Option<String>)> {
    // Check if it's from a fork (cross-repository PR)
    let is_fork = pr.is_cross_repository;

    let git = RealGitClient;
    if is_fork {
        // Fork PR - fetch PR ref from GitHub without checking out
        git.fetch(
            &["fetch", "origin", &format!("refs/pull/{number}/head")],
            Some(repo_root),
        )
        .map_err(|e| anyhow::anyhow!("git fetch PR ref failed: {e}"))?;

        // Check if local branch with PR's name already exists
        let branch_exists = git.branch_exists(&pr.head_ref_name, Some(repo_root))?;

        eprintln!(
            "{}",
            color::success(
                color_mode,
                &format!("Fetched PR #{}: {} (fork)", pr.number, pr.title)
            )
        );

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
        git.fetch(&["fetch", "origin", &pr.head_ref_name], Some(repo_root))
            .map_err(|e| anyhow::anyhow!("git fetch failed: {e}"))?;

        eprintln!(
            "{}",
            color::success(
                color_mode,
                &format!("Fetched PR #{}: {}", pr.number, pr.title)
            )
        );

        // Check if local branch already exists
        let branch_exists = git.branch_exists(&pr.head_ref_name, Some(repo_root))?;

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
    gh_client: &impl integrations::gh::GhClient,
    number: u32,
    start_point: Option<&str>,
    repo_root: &std::path::Path,
    color_mode: color::ColorMode,
) -> Result<(String, Option<String>)> {
    if !gh_client.is_available() {
        anyhow::bail!(
            "GitHub CLI (gh) is not installed or not available.\n\
             Please install gh from https://cli.github.com/ to use GitHub integration.\n\
             Alternatively, use a regular branch name instead of #{number}."
        );
    }

    // Try PR first, then issue if PR fails
    match gh_client.pr_info(number) {
        Ok(pr) => process_pr(&pr, number, repo_root, color_mode),
        Err(_pr_err) => match gh_client.issue_info(number) {
            Ok(issue) => {
                let branch_name = integrations::gh::build_issue_branch(number);
                eprintln!(
                    "{}",
                    color::success(
                        color_mode,
                        &format!("Fetched issue #{}: {}", issue.number, issue.title)
                    )
                );
                Ok((branch_name, start_point.map(String::from)))
            }
            Err(_issue_err) => {
                anyhow::bail!(
                    "#{number} is not a valid issue or pull request.\n\
                     Please check the number and try again."
                );
            }
        },
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
/// - Zoxide registration fails
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
pub fn cmd_new(
    branch: Option<&str>,
    start_point: Option<&str>,
    tmux: bool,
    no_tmux: bool,
    color_mode: color::ColorMode,
) -> Result<()> {
    // Resolve branch: CLI arg > stdin (when piped) > error
    let branch_owned = match branch {
        Some(b) => b.to_string(),
        None => crate::stdin::try_read_stdin_first()?.ok_or_else(|| {
            anyhow::anyhow!("branch name required (provide as argument or via stdin)")
        })?,
    };
    let branch = branch_owned.as_str();

    // Get main repository root
    let repo_root = get_main_repo_root()?;

    // Load configuration from repo root
    let config = config::Config::load_from_repo_root(&repo_root)?;

    // Parse branch input to detect GitHub issue/PR references
    let branch_input = integrations::gh::BranchInput::parse(branch);

    // Resolve actual branch name and optional start point from GitHub if needed
    let (actual_branch, actual_start_point) = match branch_input {
        integrations::gh::BranchInput::Github(number) if config.integrations.gh.enabled => {
            let gh_client = integrations::gh::RealGhClient;
            resolve_github_ref(&gh_client, number, start_point, &repo_root, color_mode)?
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

    let mp = MultiProgress::new();
    let is_tty = color_mode.should_colorize();

    // Header spinner (TTY) — after GH fetch, before git worktree add
    let header_pb = if is_tty {
        let pb = mp.add(ProgressBar::new_spinner());
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message(format!("Adding {branch}"));
        pb.enable_steady_tick(Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    // Resolve zoxide gating before handing control to the service.
    let zoxide_enabled = config.integrations.zoxide.enabled && is_zoxide_available();

    let service = WorktreeService::new(RealGitClient, RealZoxideClient);
    let hook_actions = &config.hooks.create;
    let req = CreateWorktreeRequest {
        branch,
        start_point,
        repo_root: &repo_root,
        path_template: &config.worktree.dir,
        zoxide_enabled,
    };

    let result = service.create(&req, |path| {
        // non-TTY: print header before hooks (rm/sync pattern)
        if !is_tty {
            eprintln!("{}", color::success(color_mode, format!("Added {branch}")));
        }

        if !hook_actions.run.is_empty()
            || !hook_actions.copy.is_empty()
            || !hook_actions.link.is_empty()
        {
            hooks::execute_hooks_lenient_with_mp(
                hook_actions,
                path,
                &repo_root,
                color_mode,
                "  ",
                &mp,
            );
        }

        Ok(())
    });

    let worktree_path = match result {
        Err(e) => {
            if let Some(pb) = header_pb {
                pb.finish_and_clear();
            }
            return Err(e);
        }
        Ok(path) => path,
    };

    // Finish header: Adding → Added
    if let Some(pb) = header_pb {
        pb.set_style(ProgressStyle::with_template("{msg}").unwrap());
        pb.finish_with_message(format!(
            "{}",
            color::success(color_mode, format!("Added {branch}"))
        ));
    }

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

    #[test]
    fn test_resolve_github_ref_issue_path() {
        let mock = integrations::gh::MockGhClient::new()
            .with_pr_error("not found")
            .with_issue(integrations::gh::IssueInfo {
                number: 33,
                title: "Test issue".to_string(),
                url: "https://github.com/owner/repo/issues/33".to_string(),
            });

        let result = resolve_github_ref(
            &mock,
            33,
            None,
            std::path::Path::new("/tmp"),
            color::ColorMode::Never,
        );

        let (branch, start_point) = result.unwrap();
        assert_eq!(branch, "issue-33");
        assert!(start_point.is_none());
    }

    #[test]
    fn test_resolve_github_ref_issue_path_with_start_point() {
        let mock = integrations::gh::MockGhClient::new()
            .with_pr_error("not found")
            .with_issue(integrations::gh::IssueInfo {
                number: 33,
                title: "Test issue".to_string(),
                url: "https://github.com/owner/repo/issues/33".to_string(),
            });

        let result = resolve_github_ref(
            &mock,
            33,
            Some("develop"),
            std::path::Path::new("/tmp"),
            color::ColorMode::Never,
        );

        let (branch, start_point) = result.unwrap();
        assert_eq!(branch, "issue-33");
        assert_eq!(start_point.as_deref(), Some("develop"));
    }

    #[test]
    fn test_resolve_github_ref_both_fail() {
        let mock = integrations::gh::MockGhClient::new()
            .with_pr_error("no pr")
            .with_issue_error("no issue");

        let result = resolve_github_ref(
            &mock,
            999,
            None,
            std::path::Path::new("/tmp"),
            color::ColorMode::Never,
        );

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not a valid issue or pull request"),
            "unexpected error: {err}"
        );
    }
}
