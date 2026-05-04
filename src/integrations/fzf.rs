#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
use anyhow::{Context, Result};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::domain::worktree::{
    calculate_relative_path, calculate_worktree_root_from_paths, WorktreeList,
};
use crate::path_utils::display_path;

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
        // Extract the last field (path) and expand ~ to $HOME
        // Use % as placeholder to avoid conflicts with fzf's {}
        let preview_cmd =
            "echo {} | awk '{print $NF}' | sed \"s|^~|$HOME|\" | xargs -I % git -C % log --oneline -n 10 2>/dev/null";
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
        .is_ok_and(|output| output.status.success())
}

/// Build worktree items from git worktree list --porcelain output
///
/// Display format: `{name} · {branch} · {path}`
/// - Index 0 is the main worktree (displayed as `@`)
/// - Non-main worktrees show their relative path from the worktree root
/// - Columns are padded for alignment (except the last column)
pub fn build_worktree_items(porcelain_output: &str) -> Vec<FzfItem> {
    // Parse via the unified WorktreeList type (replaces the previous Pass 1
    // independent scanner). Real `git worktree list --porcelain` output never
    // has leading/trailing whitespace, so the legacy `.trim()` defense is
    // dropped — covered by `test_build_worktree_items_no_trim_behavior_equivalent`.
    let list = WorktreeList::parse(porcelain_output, None);
    let entries = list.entries();

    if entries.is_empty() {
        return Vec::new();
    }

    // Calculate worktree root from non-main paths
    let non_main_paths: Vec<PathBuf> = list
        .non_main()
        .iter()
        .map(|e| PathBuf::from(&e.path))
        .collect();
    let worktree_root = calculate_worktree_root_from_paths(&non_main_paths);

    // Build name, branch, display_path for each entry
    let display_entries: Vec<(String, String, String)> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let name = if index == 0 {
                "@".to_string()
            } else {
                worktree_root
                    .as_ref()
                    .and_then(|root| calculate_relative_path(&PathBuf::from(&entry.path), root))
                    .unwrap_or_else(|| {
                        // Fallback: use directory name
                        PathBuf::from(&entry.path)
                            .file_name()
                            .map_or_else(|| entry.path.clone(), |n| n.to_string_lossy().to_string())
                    })
            };

            let branch = if index == 0 {
                "[@]".to_string()
            } else {
                entry
                    .branch
                    .as_deref()
                    .map_or_else(|| "[detached]".to_string(), |b| format!("[{b}]"))
            };

            let path = display_path(&PathBuf::from(&entry.path));

            (name, branch, path)
        })
        .collect();

    // Pass 2: Calculate column widths and format display strings
    let max_name_width = display_entries
        .iter()
        .map(|(n, _, _)| n.len())
        .max()
        .unwrap_or(0);
    let max_branch_width = display_entries
        .iter()
        .map(|(_, b, _)| b.len())
        .max()
        .unwrap_or(0);

    display_entries
        .into_iter()
        .zip(entries.iter())
        .map(|((name, branch, path), entry)| {
            let name_padding = " ".repeat(max_name_width.saturating_sub(name.len()));
            let branch_padding = " ".repeat(max_branch_width.saturating_sub(branch.len()));

            // Last column (path) has no padding to avoid trailing whitespace
            let display = format!("{name}{name_padding} · {branch}{branch_padding} · {path}");

            FzfItem {
                display,
                value: entry.path.clone(),
            }
        })
        .collect()
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
            display: "feature · feat/new · /path/to/worktree".to_string(),
            value: "/path/to/worktree".to_string(),
        };
        assert_eq!(item.display, "feature · feat/new · /path/to/worktree");
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

        // First item: main worktree displayed as "@" with [@] branch
        assert_eq!(items[0].value, "/path/to/main");
        assert!(items[0].display.starts_with('@'));
        assert!(items[0].display.contains(" · [@]"));
        assert!(items[0].display.contains(" · /path/to/main"));

        // Second item: feature branch with name and [branch]
        assert_eq!(items[1].value, "/path/to/feature");
        assert!(items[1].display.contains("feature"));
        assert!(items[1].display.contains(" · [feature-branch]"));
        assert!(items[1].display.contains(" · /path/to/feature"));
    }

    #[test]
    fn test_build_worktree_items_detached() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

worktree /path/to/detached
HEAD def456
detached

";
        let items = build_worktree_items(porcelain);
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].value, "/path/to/detached");
        assert!(items[1].display.contains("detached"));
        assert!(items[1].display.contains(" · [detached]"));
    }

    #[test]
    fn test_build_worktree_items_single_main_only() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

";
        let items = build_worktree_items(porcelain);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].value, "/path/to/main");
        assert!(items[0].display.starts_with('@'));
        assert!(items[0].display.contains(" · [@]"));
    }

    #[test]
    fn test_build_worktree_items_nested_branch() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

worktree /worktrees/feat/foo
HEAD def456
branch refs/heads/feat/foo

worktree /worktrees/fix/bar
HEAD ghi789
branch refs/heads/fix/bar

";
        let items = build_worktree_items(porcelain);
        assert_eq!(items.len(), 3);

        // Nested worktree names should use relative path from root
        assert_eq!(items[1].value, "/worktrees/feat/foo");
        assert!(items[1].display.contains("feat/foo"));
        assert!(items[1].display.contains(" · [feat/foo]"));

        assert_eq!(items[2].value, "/worktrees/fix/bar");
        assert!(items[2].display.contains(" · [fix/bar]"));
    }

    #[test]
    fn test_build_worktree_items_padding_alignment() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

worktree /worktrees/a
HEAD def456
branch refs/heads/short

worktree /worktrees/long-name
HEAD ghi789
branch refs/heads/very-long-branch-name

";
        let items = build_worktree_items(porcelain);
        assert_eq!(items.len(), 3);

        // All "·" separators should be at the same column positions
        let first_sep_positions: Vec<usize> = items
            .iter()
            .map(|item| item.display.find(" · ").unwrap())
            .collect();
        assert!(
            first_sep_positions.windows(2).all(|w| w[0] == w[1]),
            "First separator positions should be aligned: {first_sep_positions:?}"
        );

        // Find second separator positions
        let second_sep_positions: Vec<usize> = items
            .iter()
            .map(|item| {
                let after_first = first_sep_positions[0] + 3;
                after_first + item.display[after_first..].find(" · ").unwrap()
            })
            .collect();
        assert!(
            second_sep_positions.windows(2).all(|w| w[0] == w[1]),
            "Second separator positions should be aligned: {second_sep_positions:?}"
        );
    }

    #[test]
    fn test_build_worktree_items_no_trailing_whitespace() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

worktree /worktrees/feature
HEAD def456
branch refs/heads/feature-branch

";
        let items = build_worktree_items(porcelain);
        for item in &items {
            assert_eq!(
                item.display,
                item.display.trim_end(),
                "Display should not have trailing whitespace: {:?}",
                item.display
            );
        }
    }

    #[test]
    fn test_build_worktree_items_no_trim_behavior_equivalent() {
        // Canonical git worktree list --porcelain output: no leading/trailing whitespace.
        // After WorktreeList unification the legacy `.trim()` defense was dropped.
        // This test pins display equivalence on canonical input so any future regression
        // in the unified parser surfaces here.
        let porcelain = "worktree /path/to/main\nHEAD abc123\nbranch refs/heads/main\n\nworktree /worktrees/feature\nHEAD def456\nbranch refs/heads/feature\n\n";
        let items = build_worktree_items(porcelain);
        assert_eq!(items.len(), 2);
        // Main entry — name `@`, branch `[@]`
        assert!(items[0].display.starts_with('@'), "main name marker");
        assert!(
            items[0].display.contains("[@]"),
            "main branch marker [@] present: {}",
            items[0].display
        );
        // Non-main entry — branch `[feature]`
        assert!(
            items[1].display.contains("[feature]"),
            "feature branch marker present: {}",
            items[1].display
        );
        assert_eq!(items[0].value, "/path/to/main");
        assert_eq!(items[1].value, "/worktrees/feature");
    }
}
