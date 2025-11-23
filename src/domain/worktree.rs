//! Worktree domain entities and parsers
//!
//! This module contains data structures and parsing logic for git worktrees.

use chrono::{DateTime, Utc};
use chrono_humanize::{Accuracy, HumanTime, Tense};
use std::process::Command;

use crate::color;

/// Simple worktree entry without hash information
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleWorktreeEntry {
    pub path: String,
    pub branch: Option<String>,
}

/// Worktree entry for enhanced display
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    pub branch: Option<String>,
    pub hash: String,
    pub is_active: bool,
}

/// Worktree display information including commit time
struct WorktreeDisplay {
    path: Option<String>,
    hash: String,
    branch: String,
    timestamp: String,
    is_active: bool,
}

/// Parse worktree list --porcelain output into structured entries
///
/// Returns a Vec of all worktrees (including main) with their commit hashes.
/// Parses the HEAD line from porcelain output to avoid expensive git rev-parse calls.
pub fn parse_worktree_entries(
    output: &str,
    active_path: Option<&std::path::Path>,
) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;
    let mut current_hash: Option<String> = None;

    // Try to canonicalize active_path, but keep original if canonicalization fails
    let canonical_active =
        active_path.map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));

    for line in output.lines() {
        if line.starts_with("worktree ") {
            // Save previous worktree
            if let Some(path) = current_path.take() {
                let hash = current_hash
                    .take()
                    .unwrap_or_else(|| "(unknown)".to_string());
                let is_active = is_path_active(&path, canonical_active.as_ref());
                entries.push(WorktreeEntry {
                    path,
                    branch: current_branch.take(),
                    hash,
                    is_active,
                });
            }
            current_path = line.strip_prefix("worktree ").map(String::from);
            current_branch = None;
            current_hash = None;
        } else if line.starts_with("HEAD ") {
            // Parse HEAD hash and truncate to 8 characters
            if let Some(full_hash) = line.strip_prefix("HEAD ") {
                current_hash = Some(full_hash.chars().take(8).collect());
            }
        } else if line.starts_with("branch ") {
            if let Some(branch_ref) = line.strip_prefix("branch ") {
                let branch = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
                current_branch = Some(branch.to_string());
            }
        } else if line.is_empty() {
            // End of worktree entry
            if let Some(path) = current_path.take() {
                let hash = current_hash
                    .take()
                    .unwrap_or_else(|| "(unknown)".to_string());
                let is_active = is_path_active(&path, canonical_active.as_ref());
                entries.push(WorktreeEntry {
                    path,
                    branch: current_branch.take(),
                    hash,
                    is_active,
                });
            }
        }
    }

    // Handle last worktree
    if let Some(path) = current_path {
        let hash = current_hash.unwrap_or_else(|| "(unknown)".to_string());
        let is_active = is_path_active(&path, canonical_active.as_ref());
        entries.push(WorktreeEntry {
            path,
            branch: current_branch,
            hash,
            is_active,
        });
    }

    entries
}

/// Check if a worktree path matches the active path
fn is_path_active(worktree_path: &str, canonical_active: Option<&std::path::PathBuf>) -> bool {
    if let Some(active) = canonical_active {
        // Try canonical comparison first (works for real paths)
        if let Ok(canonical_worktree) = std::path::Path::new(worktree_path).canonicalize() {
            return &canonical_worktree == active;
        }
        // Fallback to string comparison (useful for tests with non-existent paths)
        return std::path::Path::new(worktree_path) == active.as_path();
    }
    false
}

/// Parse worktree entries without expensive hash lookups (for pipe mode)
/// Returns lightweight entries with only path and branch information
pub fn parse_simple_worktree_entries(output: &str) -> Vec<SimpleWorktreeEntry> {
    let mut entries = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;

    for line in output.lines() {
        if line.starts_with("worktree ") {
            // Save previous worktree
            if let Some(path) = current_path.take() {
                entries.push(SimpleWorktreeEntry {
                    path,
                    branch: current_branch.take(),
                });
            }
            current_path = line.strip_prefix("worktree ").map(String::from);
            current_branch = None;
        } else if line.starts_with("branch ") {
            if let Some(branch_ref) = line.strip_prefix("branch ") {
                let branch = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
                current_branch = Some(branch.to_string());
            }
        } else if line.is_empty() {
            // End of worktree entry
            if let Some(path) = current_path.take() {
                entries.push(SimpleWorktreeEntry {
                    path,
                    branch: current_branch.take(),
                });
            }
        }
    }

    // Handle last worktree
    if let Some(path) = current_path {
        entries.push(SimpleWorktreeEntry {
            path,
            branch: current_branch,
        });
    }

    entries
}

/// Get the last commit time for a worktree
///
/// Returns None if the worktree has no commits or if git command fails
#[must_use]
pub fn get_last_commit_time(worktree_path: &std::path::Path) -> Option<DateTime<Utc>> {
    let output = Command::new("git")
        .args([
            "-C",
            &worktree_path.display().to_string(),
            "log",
            "-1",
            "--format=%ct",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let timestamp_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let timestamp: i64 = timestamp_str.parse().ok()?;

    DateTime::from_timestamp(timestamp, 0)
}

/// Lexically normalize a path by resolving `.` and `..` components
///
/// Does NOT resolve symlinks or touch the filesystem
fn normalize_path_lexically(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;

    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {
                // Skip "." components
            }
            Component::ParentDir => {
                // Pop the last component for ".."
                normalized.pop();
            }
            _ => {
                // Normal component (RootDir, Prefix, or Normal)
                normalized.push(component);
            }
        }
    }
    normalized
}

/// Convert absolute path to home-relative display format
///
/// Returns "~/path" if under home directory, otherwise absolute path
/// Normalizes paths lexically by resolving `.` and `..` components
#[must_use]
pub fn display_path(path: &std::path::Path) -> String {
    // Normalize the path lexically (without resolving symlinks)
    let normalized = normalize_path_lexically(path);

    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = normalized.strip_prefix(&home) {
            let rel_str = rel.display().to_string();
            if rel_str.is_empty() {
                return "~".to_string();
            }
            return format!("~/{rel_str}");
        }
    }
    normalized.display().to_string()
}

/// Format worktree entries as a table with aligned columns
///
/// Returns formatted lines ready for display
/// If `show_path` is false: hash • branch • time
/// If `show_path` is true: path • hash • branch • time
///
/// # Panics
/// Panics if entries and `commit_times` have different lengths
#[must_use]
pub fn format_worktree_table(
    entries: &[WorktreeEntry],
    commit_times: &[Option<DateTime<Utc>>],
    show_path: bool,
    color_mode: color::ColorMode,
) -> Vec<String> {
    assert_eq!(
        entries.len(),
        commit_times.len(),
        "Entries and commit times must have same length"
    );

    let now = Utc::now();
    let mut displays: Vec<WorktreeDisplay> = Vec::new();

    // Build display data
    for (index, (entry, commit_time)) in entries.iter().zip(commit_times.iter()).enumerate() {
        let path = if show_path {
            Some(display_path(&std::path::PathBuf::from(&entry.path)))
        } else {
            None
        };
        let hash = entry.hash.clone();
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
                format!("{marker} {colored_path}{path_padding}  {}{hash_padding}  {colored_branch}{branch_padding}  {colored_timestamp}", d.hash)
            } else {
                format!("{marker} {}{hash_padding}  {colored_branch}{branch_padding}  {colored_timestamp}", d.hash)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_normalize_path_lexically_removes_parent_dirs() {
        // Test that .. components are resolved lexically
        let path = PathBuf::from("/Users/test/ofsht/../ofsht-worktrees/feature");
        let result = normalize_path_lexically(&path);
        assert_eq!(result, PathBuf::from("/Users/test/ofsht-worktrees/feature"));
    }

    #[test]
    fn test_normalize_path_lexically_removes_current_dirs() {
        // Test that . components are skipped
        let path = PathBuf::from("/Users/./test/./feature");
        let result = normalize_path_lexically(&path);
        assert_eq!(result, PathBuf::from("/Users/test/feature"));
    }

    #[test]
    fn test_normalize_path_lexically_preserves_symlinks() {
        // Test that symlinks are NOT resolved (lexical only)
        // This test just verifies the function doesn't touch the filesystem
        let path = PathBuf::from("/path/to/symlink/../target");
        let result = normalize_path_lexically(&path);
        assert_eq!(result, PathBuf::from("/path/to/target"));
    }

    #[test]
    #[allow(clippy::literal_string_with_formatting_args)]
    fn test_display_path_normalizes_parent_dirs() {
        // Test that paths with .. are normalized
        use std::path::MAIN_SEPARATOR;
        let path = PathBuf::from(format!(
            "{MAIN_SEPARATOR}Users{MAIN_SEPARATOR}test{MAIN_SEPARATOR}ofsht{MAIN_SEPARATOR}..{MAIN_SEPARATOR}ofsht-worktrees{MAIN_SEPARATOR}feature"
        ));
        let result = display_path(&path);
        // Should not contain ../
        assert!(!result.contains(".."));
        // Should contain normalized path components
        assert!(result.contains(&format!("ofsht-worktrees{MAIN_SEPARATOR}feature")));
    }

    #[test]
    fn test_display_path_normalizes_home_relative_with_parent_dirs() {
        // Test that home-relative paths with .. are normalized
        if let Some(home) = dirs::home_dir() {
            let path = home
                .join("projects")
                .join("ofsht")
                .join("..")
                .join("ofsht-worktrees")
                .join("feature");
            let result = display_path(&path);
            // Should not contain ../
            assert!(!result.contains(".."));
            // Should contain normalized path
            let sep = std::path::MAIN_SEPARATOR;
            assert!(
                result.contains(&format!("ofsht-worktrees{sep}feature"))
                    || result.contains("ofsht-worktrees/feature")
            ); // Unix-style in tilde paths
        }
    }

    #[test]
    #[allow(clippy::literal_string_with_formatting_args)]
    fn test_display_path_outside_home() {
        // Test paths outside home directory
        use std::path::MAIN_SEPARATOR;
        let path = if cfg!(windows) {
            PathBuf::from(format!("C:{MAIN_SEPARATOR}temp{MAIN_SEPARATOR}worktree"))
        } else {
            PathBuf::from(format!("{MAIN_SEPARATOR}tmp{MAIN_SEPARATOR}worktree"))
        };
        let result = display_path(&path);
        assert!(!result.starts_with('~'));
    }
}
