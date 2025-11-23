#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

/// Item to display in fzf
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FzfItem {
    /// Text to display in fzf
    pub display: String,
    /// Actual value to return when selected
    pub value: String,
}

/// Fzf picker interface
pub trait FzfPicker {
    fn pick(&self, items: &[FzfItem], multi: bool) -> Result<Vec<String>>;
}

/// Real fzf implementation
#[derive(Debug)]
pub struct RealFzfPicker {
    extra_options: Vec<String>,
}

impl RealFzfPicker {
    pub const fn new(extra_options: Vec<String>) -> Self {
        Self { extra_options }
    }
}

impl FzfPicker for RealFzfPicker {
    fn pick(&self, items: &[FzfItem], multi: bool) -> Result<Vec<String>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }

        // Build input for fzf (display strings)
        let input = items
            .iter()
            .map(|item| item.display.clone())
            .collect::<Vec<_>>()
            .join("\n");

        // Build fzf command
        let mut cmd = Command::new("fzf");

        // Add multi-select if requested
        if multi {
            cmd.arg("--multi");
        }

        // Add extra options from config
        for opt in &self.extra_options {
            cmd.arg(opt);
        }

        // Add preview command to show git log for each worktree
        // Use % as placeholder to avoid conflicts with fzf's {}
        let preview_cmd =
            "echo {} | awk '{print $NF}' | xargs -I % git -C % log --oneline -n 10 2>/dev/null";
        cmd.arg("--preview").arg(preview_cmd);

        // Add some default options for better UX
        cmd.arg("--height=50%")
            .arg("--reverse")
            .arg("--border")
            .arg("--prompt=Select worktree: ");

        // Execute fzf with stdin
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()); // fzf draws TUI directly to terminal

        let mut child = cmd.spawn().context("Failed to spawn fzf")?;

        // Write input to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .context("Failed to write to fzf stdin")?;
            stdin.flush().context("Failed to flush fzf stdin")?;
            // stdin is dropped here and EOF is sent
        }

        let output = child.wait_with_output().context("Failed to wait for fzf")?;

        // Handle exit codes
        match output.status.code() {
            Some(0) => {
                // Success - parse selected items
                let stdout = String::from_utf8_lossy(&output.stdout);
                let selected_displays: Vec<&str> = stdout.lines().collect();

                // Map selected display strings back to values
                let mut results = Vec::new();
                for display in selected_displays {
                    if let Some(item) = items.iter().find(|item| item.display == display) {
                        results.push(item.value.clone());
                    }
                }

                Ok(results)
            }
            Some(130 | 1) => {
                // User pressed Esc or no selection - not an error
                Ok(Vec::new())
            }
            Some(code) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("fzf exited with code {code}: {stderr}")
            }
            None => {
                anyhow::bail!("fzf was terminated by signal")
            }
        }
    }
}

/// Check if fzf is available in the system
pub fn is_fzf_available() -> bool {
    Command::new("fzf")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Build worktree items from git worktree list --porcelain output
pub fn build_worktree_items(porcelain_output: &str) -> Vec<FzfItem> {
    let mut items = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;

    for line in porcelain_output.lines() {
        let line = line.trim();
        if line.is_empty() {
            // End of a worktree entry
            if let Some(path) = current_path.take() {
                let display = current_branch.take().map_or_else(
                    || format!("(detached) {path}"),
                    |branch| format!("{branch} {path}"),
                );
                items.push(FzfItem {
                    display,
                    value: path,
                });
            }
            continue;
        }

        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            // Extract branch name from refs/heads/branch-name
            if let Some(branch_name) = branch_ref.strip_prefix("refs/heads/") {
                current_branch = Some(branch_name.to_string());
            }
        } else if line == "detached" {
            current_branch = None;
        }
    }

    // Handle last entry if file doesn't end with empty line
    if let Some(path) = current_path {
        let display = current_branch.map_or_else(
            || format!("(detached) {path}"),
            |branch| format!("{branch} {path}"),
        );
        items.push(FzfItem {
            display,
            value: path,
        });
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockFzfPicker {
        return_values: Vec<String>,
        should_fail: bool,
    }

    impl FzfPicker for MockFzfPicker {
        fn pick(&self, _items: &[FzfItem], _multi: bool) -> Result<Vec<String>> {
            if self.should_fail {
                anyhow::bail!("Mock fzf failure");
            }
            Ok(self.return_values.clone())
        }
    }

    #[test]
    fn test_fzf_item_creation() {
        let item = FzfItem {
            display: "feature-branch /path/to/worktree".to_string(),
            value: "/path/to/worktree".to_string(),
        };
        assert_eq!(item.display, "feature-branch /path/to/worktree");
        assert_eq!(item.value, "/path/to/worktree");
    }

    #[test]
    fn test_mock_fzf_picker_success() {
        let picker = MockFzfPicker {
            return_values: vec!["/path/to/worktree".to_string()],
            should_fail: false,
        };
        let items = vec![FzfItem {
            display: "test".to_string(),
            value: "/path/to/worktree".to_string(),
        }];
        let result = picker.pick(&items, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["/path/to/worktree"]);
    }

    #[test]
    fn test_mock_fzf_picker_failure() {
        let picker = MockFzfPicker {
            return_values: Vec::new(),
            should_fail: true,
        };
        let items = vec![FzfItem {
            display: "test".to_string(),
            value: "/test".to_string(),
        }];
        let result = picker.pick(&items, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_fzf_picker_cancel() {
        // User pressing Esc should return empty vec
        let picker = MockFzfPicker {
            return_values: Vec::new(),
            should_fail: false,
        };
        let items = vec![FzfItem {
            display: "test".to_string(),
            value: "/test".to_string(),
        }];
        let result = picker.pick(&items, false);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_is_fzf_available() {
        // This test will pass or fail depending on whether fzf is installed
        // We're just checking that the function doesn't panic
        let _ = is_fzf_available();
    }

    #[test]
    fn test_build_worktree_items_basic() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

worktree /path/to/feature
HEAD def456
branch refs/heads/feature-branch

";
        let items = build_worktree_items(porcelain);
        assert_eq!(items.len(), 2);

        // First item should be main worktree
        assert_eq!(items[0].value, "/path/to/main");
        assert!(items[0].display.contains("main"));

        // Second item should be feature branch
        assert_eq!(items[1].value, "/path/to/feature");
        assert!(items[1].display.contains("feature-branch"));
    }

    #[test]
    fn test_build_worktree_items_detached() {
        let porcelain = r"worktree /path/to/detached
HEAD abc123
detached

";
        let items = build_worktree_items(porcelain);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].value, "/path/to/detached");
        assert!(items[0].display.contains("(detached)"));
    }
}
