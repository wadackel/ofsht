//! Create command - Simple worktree creation without extra features

use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::color;
use crate::commands::common::get_main_repo_root;
use crate::config;
use crate::hooks;
use crate::integrations::git::RealGitClient;
use crate::integrations::zoxide::{is_zoxide_available, RealZoxideClient};
use crate::path_utils::display_path;
use crate::service::{CreateWorktreeRequest, WorktreeService};

/// Create a new worktree (simple version without tmux/GitHub integration)
///
/// # Errors
/// Returns an error if:
/// - Not in a git repository
/// - Git worktree creation fails
/// - Zoxide registration fails
#[allow(clippy::missing_panics_doc)]
pub fn cmd_create(
    branch: Option<&str>,
    start_point: Option<&str>,
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

    let mp = MultiProgress::new();
    let is_tty = color_mode.should_colorize();

    // Header spinner (TTY) or deferred header (non-TTY)
    let header_pb = if is_tty {
        let pb = mp.add(ProgressBar::new_spinner());
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message(format!("Creating {branch}"));
        pb.enable_steady_tick(Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    // Resolve zoxide gating before handing control to the service so the
    // service does not need to know about zoxide-availability detection.
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
        // non-TTY: print "Created..." header before hooks (matches rm/sync pattern)
        if !is_tty {
            eprintln!(
                "{}",
                color::success(
                    color_mode,
                    format!("Created worktree at: {}", display_path(path))
                )
            );
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

    match result {
        Err(e) => {
            if let Some(pb) = header_pb {
                pb.finish_and_clear();
            }
            Err(e)
        }
        Ok(path) => {
            if let Some(pb) = header_pb {
                pb.set_style(ProgressStyle::with_template("{msg}").unwrap());
                pb.finish_with_message(format!(
                    "{}",
                    color::success(
                        color_mode,
                        format!("Created worktree at: {}", display_path(&path))
                    )
                ));
            }
            Ok(())
        }
    }
}
