#![allow(clippy::missing_errors_doc)]
use anyhow::Result;
use indicatif::MultiProgress;

use super::output::emit_line;
use super::{files, runner, symlink, HookContext};
use crate::color;
use crate::config::HookActions;

/// Execute hook actions in the specified directory
///
/// Returns `Err` if any hook action fails, after executing all actions.
/// All errors are collected and joined with "; ".
#[cfg(test)]
pub(super) fn execute_hooks(
    actions: &HookActions,
    context: &HookContext,
    color_mode: color::ColorMode,
    indent: &str,
) -> Result<()> {
    let mp = MultiProgress::new();
    execute_hooks_with_mp(actions, context, color_mode, indent, &mp)
}

/// Execute hook actions with a shared `MultiProgress`.
///
/// Use this variant when the caller manages its own header spinner
/// in the same `MultiProgress`, ensuring correct bar ordering.
pub fn execute_hooks_with_mp(
    actions: &HookActions,
    context: &HookContext,
    color_mode: color::ColorMode,
    indent: &str,
    mp: &MultiProgress,
) -> Result<()> {
    let errors = execute_hooks_impl(actions, context, color_mode, indent, mp);
    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("{}", errors.join("; "))
    }
}

/// Execute hook actions, printing warnings on failure but never returning Err.
///
/// Hooks are supplementary automation — failures should not block the primary
/// operation (worktree creation, removal, etc.).
#[cfg(test)]
pub(super) fn execute_hooks_lenient(
    actions: &HookActions,
    context: &HookContext,
    color_mode: color::ColorMode,
    indent: &str,
) {
    let mp = MultiProgress::new();
    execute_hooks_lenient_with_mp(actions, context, color_mode, indent, &mp);
}

/// Execute hook actions leniently with a shared `MultiProgress`.
///
/// Use this variant when the caller manages its own header spinner
/// in the same `MultiProgress`, ensuring correct bar ordering.
pub fn execute_hooks_lenient_with_mp(
    actions: &HookActions,
    context: &HookContext,
    color_mode: color::ColorMode,
    indent: &str,
    mp: &MultiProgress,
) {
    let is_tty = color_mode.should_colorize();
    let errors = execute_hooks_impl(actions, context, color_mode, indent, mp);
    for err in &errors {
        emit_line(
            mp,
            is_tty,
            format!(
                "{indent}{}",
                color::warn(color_mode, format!("Hook error: {err}"))
            ),
        );
    }
}

/// Execute hook actions in the specified directory (internal implementation)
///
/// Executes all hook actions regardless of individual failures, collecting
/// error messages into a `Vec<String>`.
pub(super) fn execute_hooks_impl(
    actions: &HookActions,
    context: &HookContext,
    color_mode: color::ColorMode,
    indent: &str,
    mp: &MultiProgress,
) -> Vec<String> {
    let total_actions = actions.run.len() + actions.copy.len() + actions.link.len();
    let mut action_index = 0;
    let mut errors = Vec::new();

    // Execute commands
    for cmd in &actions.run {
        action_index += 1;
        let is_last = action_index == total_actions;
        if let Err(e) = runner::execute_command(cmd, context, color_mode, is_last, indent, mp) {
            errors.push(e.to_string());
        }
    }

    // Copy files from source to worktree
    for pattern in &actions.copy {
        action_index += 1;
        let is_last = action_index == total_actions;
        if let Err(e) = files::copy_files(
            pattern,
            &context.repo_root,
            &context.worktree_path,
            color_mode,
            is_last,
            indent,
            mp,
        ) {
            errors.push(e.to_string());
        }
    }

    // Create symbolic links
    for pattern in &actions.link {
        action_index += 1;
        let is_last = action_index == total_actions;
        if let Err(e) = symlink::create_symlinks(
            pattern,
            &context.repo_root,
            &context.worktree_path,
            color_mode,
            is_last,
            indent,
            mp,
        ) {
            errors.push(e.to_string());
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_hooks_empty() {
        let actions = HookActions::default();
        let temp_dir = std::env::temp_dir();
        let result = execute_hooks(
            &actions,
            &HookContext::for_test(&temp_dir),
            color::ColorMode::Never,
            "  ",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_hooks_continues_after_run_failure() {
        let tmp = std::env::temp_dir().join("test_hooks_continue");
        std::fs::create_dir_all(&tmp).unwrap();

        // Create a marker file to prove the second command ran
        let marker = tmp.join("second_ran");
        let actions = HookActions {
            run: vec!["exit 1".to_string(), format!("touch {}", marker.display())],
            copy: vec![],
            link: vec![],
        };

        let errors = execute_hooks_impl(
            &actions,
            &HookContext::for_test(&tmp),
            color::ColorMode::Never,
            "  ",
            &MultiProgress::new(),
        );

        // First command should have failed
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Hook command failed"));

        // Second command should still have executed
        assert!(marker.exists(), "Second hook command was not executed");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_execute_hooks_returns_all_errors() {
        let tmp = std::env::temp_dir().join("test_hooks_all_errors");
        std::fs::create_dir_all(&tmp).unwrap();

        let actions = HookActions {
            run: vec!["exit 1".to_string(), "exit 2".to_string()],
            copy: vec![],
            link: vec![],
        };

        let errors = execute_hooks_impl(
            &actions,
            &HookContext::for_test(&tmp),
            color::ColorMode::Never,
            "  ",
            &MultiProgress::new(),
        );

        // Both commands should have failed
        assert_eq!(errors.len(), 2);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_execute_hooks_strict_returns_err_on_failure() {
        let tmp = std::env::temp_dir().join("test_hooks_strict_err");
        std::fs::create_dir_all(&tmp).unwrap();

        let actions = HookActions {
            run: vec!["exit 1".to_string()],
            copy: vec![],
            link: vec![],
        };

        let result = execute_hooks(
            &actions,
            &HookContext::for_test(&tmp),
            color::ColorMode::Never,
            "  ",
        );
        assert!(result.is_err());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_execute_hooks_lenient_does_not_panic() {
        let tmp = std::env::temp_dir().join("test_hooks_lenient");
        std::fs::create_dir_all(&tmp).unwrap();

        let actions = HookActions {
            run: vec!["exit 1".to_string()],
            copy: vec![],
            link: vec![],
        };

        // execute_hooks_lenient returns () — it should not panic
        execute_hooks_lenient(
            &actions,
            &HookContext::for_test(&tmp),
            color::ColorMode::Never,
            "  ",
        );

        std::fs::remove_dir_all(&tmp).ok();
    }
}
