#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// tmux integration trait
pub trait TmuxLauncher {
    /// Detect if tmux is available and we're inside a tmux session
    fn detect(&self) -> Result<()>;
    /// Create a new tmux window at the specified path
    fn create_window(&self, path: &Path, branch: &str) -> Result<()>;
    /// Create a new tmux pane at the specified path
    fn create_pane(&self, path: &Path) -> Result<()>;
    /// Split the current tmux window horizontally into a new pane at `path`.
    ///
    /// Used by `ofsht open --pane` for batch worktree fan-out. Failures are
    /// surfaced via `Result` and the caller decides whether to bail or warn.
    fn split_pane(&self, path: &Path) -> Result<()>;
    /// Apply the `tiled` layout to the current tmux window.
    ///
    /// Called by `ofsht open --pane` after splitting all worktrees so that
    /// the resulting panes are evenly distributed.
    fn select_tiled_layout(&self) -> Result<()>;
}

/// Real tmux launcher that executes actual tmux commands
pub struct RealTmuxLauncher;

impl TmuxLauncher for RealTmuxLauncher {
    /// Detect if tmux is available and we're inside a tmux session
    fn detect(&self) -> Result<()> {
        // Check if we're inside a tmux session
        if std::env::var_os("TMUX").is_none() {
            bail!(
                "tmux integration requires running inside a tmux session; \
                 try again from a tmux pane or use --no-tmux to disable"
            );
        }

        // Check if tmux binary exists
        let status = Command::new("tmux")
            .arg("-V")
            .output()
            .context("Failed to execute tmux command")?;

        if !status.status.success() {
            bail!("tmux binary not found or not executable");
        }

        Ok(())
    }

    /// Create a new tmux window at the specified path
    fn create_window(&self, path: &Path, branch: &str) -> Result<()> {
        // Ensure we're in a tmux session
        self.detect()?;

        let name = sanitize_window_name(branch);

        let output = Command::new("tmux")
            .arg("new-window")
            .arg("-n")
            .arg(&name)
            .arg("-c")
            .arg(path)
            .output()
            .context("Failed to execute tmux new-window command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux new-window command failed: {}", stderr.trim());
        }

        Ok(())
    }

    /// Create a new tmux pane at the specified path
    fn create_pane(&self, path: &Path) -> Result<()> {
        // Ensure we're in a tmux session
        self.detect()?;
        self.split_pane(path)
    }

    fn split_pane(&self, path: &Path) -> Result<()> {
        let output = Command::new("tmux")
            .arg("split-window")
            .arg("-h")
            .arg("-c")
            .arg(path)
            .output()
            .context("Failed to execute tmux split-window command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux split-window command failed: {}", stderr.trim());
        }

        Ok(())
    }

    fn select_tiled_layout(&self) -> Result<()> {
        let output = Command::new("tmux")
            .args(["select-layout", "tiled"])
            .output()
            .context("Failed to execute tmux select-layout command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux select-layout command failed: {}", stderr.trim());
        }

        Ok(())
    }
}

/// Sanitize branch name for use as tmux window name
/// - Replaces `/` and spaces with `·` (middle dot)
/// - Truncates to 50 characters
/// - Returns "worktree" if empty
pub fn sanitize_window_name(branch: &str) -> String {
    if branch.is_empty() {
        return "worktree".to_string();
    }

    let sanitized = branch.replace(['/', ' '], "·");

    // Truncate to 50 characters
    if sanitized.len() > 50 {
        sanitized.chars().take(50).collect()
    } else {
        sanitized
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::Cell;

    /// Mock tmux launcher for testing.
    ///
    /// Construct with `MockTmuxLauncher::default()` and override individual
    /// fields with struct-update syntax:
    ///
    /// ```ignore
    /// MockTmuxLauncher { split_pane_should_fail: true, ..Default::default() }
    /// ```
    #[derive(Default)]
    pub struct MockTmuxLauncher {
        pub detect_should_fail: bool,
        pub split_pane_should_fail: bool,
        pub select_tiled_layout_should_fail: bool,
        /// Counts how many times `select_tiled_layout` was invoked. Used by
        /// partial-failure regression tests to assert it is only called when at
        /// least one split succeeded.
        pub select_tiled_layout_calls: Cell<u32>,
    }

    impl TmuxLauncher for MockTmuxLauncher {
        fn detect(&self) -> Result<()> {
            if self.detect_should_fail {
                bail!("Mock tmux detect failure");
            }
            Ok(())
        }

        fn create_window(&self, _path: &Path, _branch: &str) -> Result<()> {
            Ok(())
        }

        fn create_pane(&self, _path: &Path) -> Result<()> {
            Ok(())
        }

        fn split_pane(&self, _path: &Path) -> Result<()> {
            if self.split_pane_should_fail {
                bail!("Mock tmux split_pane failure");
            }
            Ok(())
        }

        fn select_tiled_layout(&self) -> Result<()> {
            self.select_tiled_layout_calls
                .set(self.select_tiled_layout_calls.get() + 1);
            if self.select_tiled_layout_should_fail {
                bail!("Mock tmux select_tiled_layout failure");
            }
            Ok(())
        }
    }

    #[test]
    fn test_split_pane_success() {
        let mock = MockTmuxLauncher::default();
        assert!(mock.split_pane(Path::new("/tmp/x")).is_ok());
    }

    #[test]
    fn test_split_pane_failure() {
        let mock = MockTmuxLauncher {
            split_pane_should_fail: true,
            ..Default::default()
        };
        assert!(mock.split_pane(Path::new("/tmp/x")).is_err());
    }

    #[test]
    fn test_select_tiled_layout_success() {
        let mock = MockTmuxLauncher::default();
        assert!(mock.select_tiled_layout().is_ok());
        assert_eq!(mock.select_tiled_layout_calls.get(), 1);
    }

    #[test]
    fn test_select_tiled_layout_failure() {
        let mock = MockTmuxLauncher {
            select_tiled_layout_should_fail: true,
            ..Default::default()
        };
        assert!(mock.select_tiled_layout().is_err());
    }

    #[test]
    fn test_sanitize_window_name_simple() {
        assert_eq!(sanitize_window_name("feature"), "feature");
    }

    #[test]
    fn test_sanitize_window_name_with_slash() {
        assert_eq!(sanitize_window_name("feature/login"), "feature·login");
    }

    #[test]
    fn test_sanitize_window_name_with_space() {
        assert_eq!(sanitize_window_name("feature bug fix"), "feature·bug·fix");
    }

    #[test]
    fn test_sanitize_window_name_mixed() {
        assert_eq!(sanitize_window_name("feat/bug fix"), "feat·bug·fix");
    }

    #[test]
    fn test_sanitize_window_name_long() {
        let long_name = "a".repeat(100);
        let result = sanitize_window_name(&long_name);
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn test_sanitize_window_name_empty() {
        assert_eq!(sanitize_window_name(""), "worktree");
    }

    #[test]
    fn test_sanitize_window_name_only_special_chars() {
        assert_eq!(sanitize_window_name("///"), "···");
    }
}
