//! Worktree-list table formatting
//!
//! Display layer for `ofsht ls`: produces ANSI-aware aligned columns with
//! human-friendly commit-time strings. Sole consumer is `commands/list.rs`.

use chrono::{DateTime, Utc};
use chrono_humanize::{Accuracy, HumanTime, Tense};

use crate::color;
use crate::domain::worktree::{
    calculate_relative_path, calculate_worktree_root_from_paths, WorktreeEntry,
};
use crate::path_utils::display_path;

/// Worktree display information including commit time
struct WorktreeDisplay {
    path: Option<String>,
    hash: String,
    rel_path: Option<String>,
    branch: String,
    timestamp: String,
    is_active: bool,
}

/// Format worktree entries as a table with aligned columns
///
/// Returns formatted lines ready for display
/// If `show_path` is false and `config` is None: hash • branch • time
/// If `show_path` is false and `config` is Some: hash • `rel_path` • branch • time
/// If `show_path` is true: path • hash • `rel_path` • branch • time
///
/// # Panics
/// Panics if entries and `commit_times` have different lengths
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn format_worktree_table(
    entries: &[WorktreeEntry],
    commit_times: &[Option<DateTime<Utc>>],
    show_path: bool,
    color_mode: color::ColorMode,
    config: Option<&crate::config::Config>,
) -> Vec<String> {
    assert_eq!(
        entries.len(),
        commit_times.len(),
        "Entries and commit times must have same length"
    );

    let now = Utc::now();
    let mut displays: Vec<WorktreeDisplay> = Vec::new();

    // Calculate worktree root if config is provided
    // Collect all non-main worktree paths (skip index 0 which is main worktree)
    let worktree_root = config.and_then(|_cfg| {
        let non_main_paths: Vec<std::path::PathBuf> = entries
            .iter()
            .skip(1)
            .map(|entry| std::path::PathBuf::from(&entry.path))
            .collect();

        calculate_worktree_root_from_paths(&non_main_paths)
    });

    // Build display data
    for (index, (entry, commit_time)) in entries.iter().zip(commit_times.iter()).enumerate() {
        let path = if show_path {
            Some(display_path(&std::path::PathBuf::from(&entry.path)))
        } else {
            None
        };
        let hash = entry
            .hash
            .clone()
            .unwrap_or_else(|| "(unknown)".to_string());

        // Calculate relative path for non-main worktrees
        let rel_path = if index != 0 {
            worktree_root.as_ref().and_then(|root| {
                calculate_relative_path(&std::path::PathBuf::from(&entry.path), root)
            })
        } else {
            None
        };

        // Main worktree (index 0) is always displayed as "@"
        let branch = if index == 0 {
            "[@]".to_string()
        } else {
            entry
                .branch
                .as_ref()
                .map_or_else(|| "[detached]".to_string(), |b| format!("[{b}]"))
        };
        let timestamp = commit_time.as_ref().map_or_else(
            || "–".to_string(),
            |dt| {
                let duration = now.signed_duration_since(*dt);
                HumanTime::from(duration).to_text_en(Accuracy::Rough, Tense::Past)
            },
        );

        displays.push(WorktreeDisplay {
            path,
            hash,
            rel_path,
            branch,
            timestamp,
            is_active: entry.is_active,
        });
    }

    // Calculate column widths
    let max_path_width = if show_path {
        displays
            .iter()
            .filter_map(|d| d.path.as_ref().map(std::string::String::len))
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    let max_hash_width = displays.iter().map(|d| d.hash.len()).max().unwrap_or(0);
    let max_rel_path_width = displays
        .iter()
        .filter_map(|d| d.rel_path.as_ref().map(std::string::String::len))
        .max()
        .unwrap_or(0);
    let max_branch_width = displays.iter().map(|d| d.branch.len()).max().unwrap_or(0);

    // Format lines with padding and colors
    displays
        .iter()
        .enumerate()
        .map(|(index, d)| {
            // Create active marker
            let marker = if d.is_active {
                color_mode.colorize_active_marker("*")
            } else {
                " ".to_string()
            };

            // Apply colors to each component
            let colored_branch = if index == 0 {
                // Main worktree [@] in green
                color_mode.colorize_main_worktree(&d.branch)
            } else if d.branch == "[detached]" {
                // Detached HEAD in yellow
                color_mode.colorize_detached(&d.branch)
            } else {
                // Regular branch in cyan
                color_mode.colorize_branch(&d.branch)
            };
            let colored_timestamp = color_mode.colorize_secondary(&d.timestamp);

            // Manual padding (format! doesn't work correctly with ANSI codes)
            let hash_padding = " ".repeat(max_hash_width.saturating_sub(d.hash.len()));
            let branch_padding = " ".repeat(max_branch_width.saturating_sub(d.branch.len()));

            if show_path {
                let colored_path = d.path.as_ref().unwrap();
                let path_padding =
                    " ".repeat(max_path_width.saturating_sub(colored_path.len()));

                // Format relative path with padding
                let rel_path_str = d.rel_path.as_deref().unwrap_or("");
                let rel_path_padding =
                    " ".repeat(max_rel_path_width.saturating_sub(rel_path_str.len()));

                format!("{marker} {colored_path}{path_padding}  {}{hash_padding}  {rel_path_str}{rel_path_padding}  {colored_branch}{branch_padding}  {colored_timestamp}", d.hash)
            } else if max_rel_path_width > 0 {
                // Show relative path column when config is provided
                let rel_path_str = d.rel_path.as_deref().unwrap_or("");
                let rel_path_padding =
                    " ".repeat(max_rel_path_width.saturating_sub(rel_path_str.len()));

                format!("{marker} {}{hash_padding}  {rel_path_str}{rel_path_padding}  {colored_branch}{branch_padding}  {colored_timestamp}", d.hash)
            } else {
                // Original format without relative path
                format!("{marker} {}{hash_padding}  {colored_branch}{branch_padding}  {colored_timestamp}", d.hash)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_worktree_table_default_no_path() {
        let entries = vec![WorktreeEntry {
            path: "/path/to/main".to_string(),
            branch: Some("main".to_string()),
            hash: Some("a1b2c3d4".to_string()),
            is_active: false,
        }];
        let commit_times = vec![Some(
            DateTime::from_timestamp(Utc::now().timestamp() - 3600, 0).unwrap(),
        )];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            false,
            crate::color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 1);
        // Line should contain hash, branch, and timestamp (no path)
        assert!(!result[0].contains("/path/to/main"));
        // Main worktree (first entry) should be displayed as [@]
        assert!(result[0].contains("[@]"));
        assert!(result[0].contains("a1b2c3d4"));
    }

    #[test]
    fn test_format_worktree_table_with_path() {
        let entries = vec![WorktreeEntry {
            path: "/path/to/main".to_string(),
            branch: Some("main".to_string()),
            hash: Some("a1b2c3d4".to_string()),
            is_active: false,
        }];
        let commit_times = vec![Some(
            DateTime::from_timestamp(Utc::now().timestamp() - 3600, 0).unwrap(),
        )];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            true,
            crate::color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 1);
        // Line should contain path, hash, branch, and timestamp
        assert!(result[0].contains("/path/to/main"));
        // Main worktree (first entry) should be displayed as [@]
        assert!(result[0].contains("[@]"));
        assert!(result[0].contains("a1b2c3d4"));
    }

    #[test]
    fn test_format_worktree_table_multiple_entries() {
        let entries = vec![
            WorktreeEntry {
                path: "/path/to/main".to_string(),
                branch: Some("main".to_string()),
                hash: Some("a1b2c3d4".to_string()),
                is_active: false,
            },
            WorktreeEntry {
                path: "/path/to/feature-branch".to_string(),
                branch: Some("feature".to_string()),
                hash: Some("e5f6g7h8".to_string()),
                is_active: false,
            },
        ];
        let commit_times = vec![
            Some(DateTime::from_timestamp(Utc::now().timestamp() - 3600, 0).unwrap()),
            None,
        ];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            false,
            crate::color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 2);
        // Both lines should have same structure (no paths)
        assert!(!result[0].contains("/path/to/main"));
        // Main worktree (first entry) should be displayed as [@]
        assert!(result[0].contains("[@]"));
        assert!(result[0].contains("a1b2c3d4"));
        assert!(!result[1].contains("/path/to/feature-branch"));
        assert!(result[1].contains("[feature]"));
        assert!(result[1].contains("e5f6g7h8"));
        assert!(result[1].contains("–")); // No commit time
    }

    #[test]
    fn test_format_worktree_table_column_alignment() {
        let entries = vec![
            WorktreeEntry {
                path: "/short".to_string(),
                branch: Some("a".to_string()),
                hash: Some("12345678".to_string()),
                is_active: false,
            },
            WorktreeEntry {
                path: "/very/long/path/to/worktree".to_string(),
                branch: Some("feature-branch".to_string()),
                hash: Some("abcdefgh".to_string()),
                is_active: false,
            },
        ];
        let commit_times = vec![None, None];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            true,
            crate::color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 2);

        // Verify that both lines contain the expected content
        assert!(result[0].contains("/short"));
        // Main worktree (first entry) should be displayed as [@]
        assert!(result[0].contains("[@]"));
        assert!(result[0].contains("12345678"));
        assert!(result[1].contains("/very/long/path/to/worktree"));
        assert!(result[1].contains("[feature-branch]"));
        assert!(result[1].contains("abcdefgh"));

        // Both lines should end with the same timestamp placeholder
        assert!(result[0].ends_with("–"));
        assert!(result[1].ends_with("–"));
    }

    #[test]
    fn test_format_worktree_table_detached_head() {
        let entries = vec![WorktreeEntry {
            path: "/path/to/detached".to_string(),
            branch: None,
            hash: Some("deadbeef".to_string()),
            is_active: false,
        }];
        let commit_times = vec![None];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            false,
            crate::color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 1);
        // Main worktree (first entry) is always [@], even if detached
        assert!(result[0].contains("[@]"));
        assert!(result[0].contains("deadbeef"));
        assert!(result[0].contains("–"));
    }

    #[test]
    fn test_format_worktree_table_active_marker() {
        let entries = vec![
            WorktreeEntry {
                path: "/path/to/main".to_string(),
                branch: Some("main".to_string()),
                hash: Some("a1b2c3d4".to_string()),
                is_active: false,
            },
            WorktreeEntry {
                path: "/path/to/feature".to_string(),
                branch: Some("feature".to_string()),
                hash: Some("e5f6g7h8".to_string()),
                is_active: true,
            },
        ];
        let commit_times = vec![None, None];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            false,
            crate::color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 2);
        // First entry (inactive) should have space prefix
        assert!(result[0].starts_with("  "));
        // Second entry (active) should have * prefix
        assert!(result[1].starts_with("* "));
        assert!(result[1].contains("[feature]"));
    }

    #[test]
    fn test_format_worktree_table_active_marker_with_path() {
        let entries = vec![
            WorktreeEntry {
                path: "/path/to/main".to_string(),
                branch: Some("main".to_string()),
                hash: Some("a1b2c3d4".to_string()),
                is_active: false,
            },
            WorktreeEntry {
                path: "/path/to/feature".to_string(),
                branch: Some("feature".to_string()),
                hash: Some("e5f6g7h8".to_string()),
                is_active: true,
            },
        ];
        let commit_times = vec![None, None];

        let result = format_worktree_table(
            &entries,
            &commit_times,
            true,
            crate::color::ColorMode::Never,
            None,
        );
        assert_eq!(result.len(), 2);
        // Both entries should have marker prefix (space or *)
        assert!(result[0].starts_with("  "));
        assert!(result[1].starts_with("* "));
    }

    #[test]
    fn test_format_worktree_table_with_relative_paths() {
        // Test that relative paths are displayed when config is provided
        use crate::config::{Config, Hooks, IntegrationsConfig, WorktreeConfig};

        let entries = vec![
            WorktreeEntry {
                path: "/Users/test/repo-worktrees/main".to_string(),
                branch: Some("main".to_string()),
                hash: Some("a1b2c3d4".to_string()),
                is_active: false,
            },
            WorktreeEntry {
                path: "/Users/test/repo-worktrees/feature".to_string(),
                branch: Some("feature".to_string()),
                hash: Some("e5f6g7h8".to_string()),
                is_active: false,
            },
            WorktreeEntry {
                path: "/Users/test/repo-worktrees/docs/tweak".to_string(),
                branch: Some("docs/tweak".to_string()),
                hash: Some("i9j0k1l2".to_string()),
                is_active: true,
            },
        ];
        let commit_times = vec![None, None, None];

        let config = Config {
            worktree: WorktreeConfig {
                dir: "../{repo}-worktrees/{branch}".to_string(),
            },
            hooks: Hooks::default(),
            integrations: IntegrationsConfig::default(),
        };

        let result = format_worktree_table(
            &entries,
            &commit_times,
            false,
            crate::color::ColorMode::Never,
            Some(&config),
        );

        assert_eq!(result.len(), 3);

        // Main worktree (index 0) should have blank relative path column
        // Format: "  a1b2c3d4            [@]  unknown" (extra spaces where rel_path would be)
        let main_line = &result[0];
        assert!(main_line.contains("a1b2c3d4"));
        assert!(main_line.contains("[@]"));

        // Feature worktree should show "feature" as relative path
        // Format: "  e5f6g7h8  feature  [feature]  unknown"
        let feature_line = &result[1];
        assert!(feature_line.contains("e5f6g7h8"));
        assert!(feature_line.contains("feature"));
        assert!(feature_line.contains("[feature]"));

        // Nested worktree should show "docs/tweak" as relative path
        // Format: "* i9j0k1l2  docs/tweak  [docs/tweak]  unknown"
        let nested_line = &result[2];
        assert!(nested_line.contains("i9j0k1l2"));
        assert!(nested_line.contains("docs/tweak"));
        assert!(nested_line.contains("[docs/tweak]"));
        assert!(nested_line.starts_with("* ")); // Active marker
    }
}
