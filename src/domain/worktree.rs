//! Worktree domain entities and parsers
//!
//! This module contains data structures and parsing logic for git worktrees.

use crate::path_utils::canonicalize_allow_missing;

/// Worktree entry for enhanced display
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    pub branch: Option<String>,
    pub hash: Option<String>,
    pub is_active: bool,
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

/// Unified worktree list parsed from `git worktree list --porcelain` output.
///
/// Single parser + six query methods (`entries`, `main`, `non_main`,
/// `find_by_branch`, `find_by_path`, `current`) replacing the previous family
/// of standalone porcelain scanners scattered across `commands/*` and `cli.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeList {
    entries: Vec<WorktreeEntry>,
}

impl WorktreeList {
    /// Parse `git worktree list --porcelain` output.
    ///
    /// `active_path`: when `Some`, the matching entry's `is_active` field is set to `true`
    /// using canonicalize-then-string-fallback comparison (same semantics as the legacy
    /// `is_path_active` helper).
    #[must_use]
    pub fn parse(porcelain: &str, active_path: Option<&std::path::Path>) -> Self {
        let mut entries = Vec::new();
        let mut current_path: Option<String> = None;
        let mut current_branch: Option<String> = None;
        let mut current_hash: Option<String> = None;

        let canonical_active =
            active_path.map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));

        for line in porcelain.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                if let Some(prev_path) = current_path.take() {
                    let is_active = is_path_active(&prev_path, canonical_active.as_ref());
                    entries.push(WorktreeEntry {
                        path: prev_path,
                        branch: current_branch.take(),
                        hash: current_hash.take(),
                        is_active,
                    });
                }
                current_path = Some(path.to_string());
            } else if let Some(full_hash) = line.strip_prefix("HEAD ") {
                current_hash = Some(full_hash.chars().take(8).collect());
            } else if let Some(branch_ref) = line.strip_prefix("branch ") {
                let branch = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
                current_branch = Some(branch.to_string());
            } else if line == "detached" {
                current_branch = None;
            } else if line.is_empty() {
                if let Some(prev_path) = current_path.take() {
                    let is_active = is_path_active(&prev_path, canonical_active.as_ref());
                    entries.push(WorktreeEntry {
                        path: prev_path,
                        branch: current_branch.take(),
                        hash: current_hash.take(),
                        is_active,
                    });
                }
            }
        }

        if let Some(prev_path) = current_path {
            let is_active = is_path_active(&prev_path, canonical_active.as_ref());
            entries.push(WorktreeEntry {
                path: prev_path,
                branch: current_branch,
                hash: current_hash,
                is_active,
            });
        }

        Self { entries }
    }

    /// All worktree entries in the order returned by `git worktree list --porcelain`.
    /// The main worktree (when present) is always at index 0.
    #[must_use]
    pub fn entries(&self) -> &[WorktreeEntry] {
        &self.entries
    }

    /// The main worktree (index 0). Returns `None` when porcelain output yields no entries
    /// (e.g. empty input, broken repo). git itself always emits at least one entry in
    /// healthy repositories.
    #[must_use]
    pub fn main(&self) -> Option<&WorktreeEntry> {
        self.entries.first()
    }

    /// All non-main worktree entries (everything after index 0). Empty when no extras exist.
    #[must_use]
    pub fn non_main(&self) -> &[WorktreeEntry] {
        if self.entries.is_empty() {
            &[]
        } else {
            &self.entries[1..]
        }
    }

    /// Find a non-main worktree by branch name (refs/heads/ already stripped).
    /// Returns the first match. The main worktree is excluded so `find_by_branch("main")`
    /// returns `None` when "main" is the main worktree's branch.
    #[must_use]
    pub fn find_by_branch(&self, branch_name: &str) -> Option<&WorktreeEntry> {
        self.non_main()
            .iter()
            .find(|e| e.branch.as_deref() == Some(branch_name))
    }

    /// Find a non-main worktree by absolute or relative path.
    /// Uses `canonicalize_allow_missing` for both sides so non-existent paths still compare
    /// by their lexical normalization. The main worktree is excluded.
    #[must_use]
    pub fn find_by_path(&self, target: &std::path::Path) -> Option<&WorktreeEntry> {
        let canonical_target = canonicalize_allow_missing(target);
        self.non_main().iter().find(|e| {
            let path_buf = std::path::PathBuf::from(&e.path);
            canonicalize_allow_missing(&path_buf) == canonical_target
        })
    }

    /// The currently-active worktree (matched against the `active_path` passed to `parse`).
    /// Returns `None` when no `active_path` was provided, or when no entry matched.
    ///
    /// Production callers currently access `WorktreeEntry.is_active` directly via
    /// `format_worktree_table`. This helper is part of the documented `WorktreeList`
    /// API surface and is exercised by unit tests.
    #[must_use]
    #[allow(dead_code)]
    pub fn current(&self) -> Option<&WorktreeEntry> {
        self.entries.iter().find(|e| e.is_active)
    }
}

/// Calculate the depth from {branch} placeholder to the worktree root
///
/// Returns the number of directory levels from the worktree root to where {branch} is located.
/// This is determined by counting normal path components before {branch}.
/// Uses `Path::components()` for cross-platform compatibility (handles both `/` and `\`).
///
/// # Examples
///
/// ```
/// # use ofsht::domain::worktree::calculate_branch_depth;
/// assert_eq!(calculate_branch_depth("../{repo}-worktrees/{branch}"), 1);
/// assert_eq!(calculate_branch_depth("../{repo}-worktrees/subdir/{branch}"), 2);
/// ```
#[must_use]
#[allow(dead_code)] // Used in doctests
pub fn calculate_branch_depth(template: &str) -> usize {
    use std::path::{Component, Path};

    // Get the part before {branch}
    let before_branch = template.split("{branch}").next().unwrap_or("");

    // Use Path::components() for cross-platform path parsing
    // This automatically handles both '/' and '\' separators
    // Count only Normal components (excludes '..' and '.')
    Path::new(before_branch)
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count()
}

/// Calculate the worktree root directory from a worktree path and template
///
/// Uses the template to determine how many levels to traverse upward
/// from the worktree path to find the root directory.
///
/// Returns None if the path doesn't have enough parent directories.
///
/// **Note**: This function assumes the branch name is a single directory component.
/// For nested branch names (e.g., `docs/tweak`), use `calculate_worktree_root_from_paths` instead.
///
/// # Examples
///
/// ```
/// # use std::path::PathBuf;
/// # use ofsht::domain::worktree::calculate_worktree_root;
/// let path = PathBuf::from("/Users/test/repo-worktrees/feature");
/// let root = calculate_worktree_root(&path, "../{repo}-worktrees/{branch}");
/// assert_eq!(root, Some(PathBuf::from("/Users/test/repo-worktrees")));
/// ```
#[must_use]
#[allow(dead_code)] // Used in doctests
pub fn calculate_worktree_root(
    worktree_path: &std::path::Path,
    template: &str,
) -> Option<std::path::PathBuf> {
    let depth = calculate_branch_depth(template);
    let mut root = worktree_path;
    for _ in 0..depth {
        root = root.parent()?;
    }
    Some(root.to_path_buf())
}

/// Calculate the worktree root directory from all non-main worktree paths
///
/// Finds the common parent directory that contains all worktrees.
/// This method correctly handles nested branch names (e.g., `docs/tweak`, `team/alice/fix`).
///
/// Returns None if:
/// - No worktree paths are provided
/// - Paths have no common parent
///
/// # Examples
///
/// ```
/// # use std::path::PathBuf;
/// # use ofsht::domain::worktree::calculate_worktree_root_from_paths;
/// let paths = vec![
///     PathBuf::from("/Users/test/repo-worktrees/feature"),
///     PathBuf::from("/Users/test/repo-worktrees/docs/tweak"),
/// ];
/// let root = calculate_worktree_root_from_paths(&paths);
/// assert_eq!(root, Some(PathBuf::from("/Users/test/repo-worktrees")));
/// ```
#[must_use]
pub fn calculate_worktree_root_from_paths(
    worktree_paths: &[std::path::PathBuf],
) -> Option<std::path::PathBuf> {
    use std::path::{Component, PathBuf};

    if worktree_paths.is_empty() {
        return None;
    }

    if worktree_paths.len() == 1 {
        // Only one worktree - return its parent as the root
        return worktree_paths[0].parent().map(std::path::Path::to_path_buf);
    }

    // Find common prefix by comparing path components
    let first_components: Vec<Component> = worktree_paths[0].components().collect();
    let mut common_depth = first_components.len();

    // Compare with all other paths to find the shortest common prefix
    for path in &worktree_paths[1..] {
        let components: Vec<Component> = path.components().collect();
        let mut current_depth = 0;

        for (comp_first, comp_other) in first_components.iter().zip(components.iter()) {
            if comp_first == comp_other {
                current_depth += 1;
            } else {
                break;
            }
        }

        common_depth = common_depth.min(current_depth);
    }

    if common_depth == 0 {
        return None;
    }

    // Build the common prefix path
    let mut result = PathBuf::new();
    for component in first_components.iter().take(common_depth) {
        result.push(component);
    }

    Some(result)
}

/// Calculate the relative path from worktree root to the worktree
///
/// Returns None if the worktree path is not under the worktree root.
///
/// # Examples
///
/// ```
/// # use std::path::PathBuf;
/// # use ofsht::domain::worktree::calculate_relative_path;
/// let worktree = PathBuf::from("/Users/test/repo-worktrees/feature");
/// let root = PathBuf::from("/Users/test/repo-worktrees");
/// assert_eq!(calculate_relative_path(&worktree, &root), Some("feature".to_string()));
/// ```
#[must_use]
pub fn calculate_relative_path(
    worktree_path: &std::path::Path,
    worktree_root: &std::path::Path,
) -> Option<String> {
    worktree_path
        .strip_prefix(worktree_root)
        .ok()
        .map(|p| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // --- Tests for relative path calculation utilities ---

    #[test]
    fn test_calculate_branch_depth_simple() {
        assert_eq!(calculate_branch_depth("../{repo}-worktrees/{branch}"), 1);
    }

    #[test]
    fn test_calculate_branch_depth_nested() {
        assert_eq!(
            calculate_branch_depth("../{repo}-worktrees/subdir/{branch}"),
            2
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_calculate_branch_depth_windows_separator() {
        // Test with Windows-style backslash separator
        assert_eq!(calculate_branch_depth(r"..\{repo}-worktrees\{branch}"), 1);
        assert_eq!(
            calculate_branch_depth(r"..\{repo}-worktrees\subdir\{branch}"),
            2
        );
    }

    #[test]
    fn test_calculate_branch_depth_mixed_separators() {
        // Path::components() normalizes mixed separators
        assert_eq!(calculate_branch_depth("../{repo}-worktrees\\{branch}"), 1);
    }

    #[test]
    fn test_calculate_worktree_root_from_feature_branch() {
        let path = PathBuf::from("/Users/test/repo-worktrees/feature");
        let root = calculate_worktree_root(&path, "../{repo}-worktrees/{branch}");
        assert_eq!(root, Some(PathBuf::from("/Users/test/repo-worktrees")));
    }

    #[test]
    fn test_calculate_worktree_root_insufficient_depth() {
        // Path has no parent (root path)
        let path = PathBuf::from("/");
        let root = calculate_worktree_root(&path, "../{repo}-worktrees/{branch}");
        assert_eq!(root, None);
    }

    #[test]
    fn test_calculate_relative_path_simple() {
        let worktree = PathBuf::from("/Users/test/repo-worktrees/feature");
        let root = PathBuf::from("/Users/test/repo-worktrees");
        assert_eq!(
            calculate_relative_path(&worktree, &root),
            Some("feature".to_string())
        );
    }

    #[test]
    fn test_calculate_relative_path_nested() {
        let worktree = PathBuf::from("/Users/test/repo-worktrees/docs/tweak");
        let root = PathBuf::from("/Users/test/repo-worktrees");
        assert_eq!(
            calculate_relative_path(&worktree, &root),
            Some("docs/tweak".to_string())
        );
    }

    #[test]
    fn test_calculate_relative_path_outside_root() {
        let worktree = PathBuf::from("/tmp/elsewhere");
        let root = PathBuf::from("/Users/test/repo-worktrees");
        assert_eq!(calculate_relative_path(&worktree, &root), None);
    }

    #[test]
    fn test_calculate_relative_path_deeply_nested() {
        let worktree = PathBuf::from("/Users/test/repo-worktrees/team/alice/fix");
        let root = PathBuf::from("/Users/test/repo-worktrees");
        assert_eq!(
            calculate_relative_path(&worktree, &root),
            Some("team/alice/fix".to_string())
        );
    }

    // -----------------------------------------------------------------
    // WorktreeList tests (Step 2: porcelain parser unification)
    // -----------------------------------------------------------------

    #[test]
    fn test_worktree_list_parse_basic() {
        let output = "worktree /path/to/main\nHEAD abc123def456789\nbranch refs/heads/main\n\nworktree /path/to/feat\nHEAD def456abc789012\nbranch refs/heads/feat\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries().len(), 2);
        assert_eq!(list.entries()[0].path, "/path/to/main");
        assert_eq!(list.entries()[0].branch.as_deref(), Some("main"));
        assert_eq!(list.entries()[1].path, "/path/to/feat");
        assert_eq!(list.entries()[1].branch.as_deref(), Some("feat"));
    }

    #[test]
    fn test_worktree_list_parse_detached_head_implicit() {
        // Detached HEAD with no `branch` line and no explicit `detached` marker
        let output = "worktree /path/to/main\nHEAD abc123def456789\nbranch refs/heads/main\n\nworktree /path/to/det\nHEAD aaaaaaaaaaaaaaaa\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries().len(), 2);
        assert_eq!(list.entries()[1].branch, None);
    }

    #[test]
    fn test_worktree_list_parse_detached_head_explicit_marker() {
        // Detached HEAD with explicit `detached` line (fzf.rs legacy behavior)
        let output = "worktree /path/to/main\nHEAD abc123def456789\nbranch refs/heads/main\n\nworktree /path/to/det\nHEAD aaaaaaaaaaaaaaaa\ndetached\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries().len(), 2);
        assert_eq!(list.entries()[1].branch, None);
    }

    #[test]
    fn test_worktree_list_parse_main_marker_at_index_0() {
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-a\nHEAD def67890xxxxxx\nbranch refs/heads/feature-a\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries()[0].path, "/repo");
        assert_eq!(list.entries()[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn test_worktree_list_parse_branch_with_slash() {
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-feat\nHEAD def67890xxxxxx\nbranch refs/heads/feature/foo\n\nworktree /wt-rel\nHEAD eee99999xxxxxx\nbranch refs/heads/release/1.0\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries().len(), 3);
        assert_eq!(list.entries()[1].branch.as_deref(), Some("feature/foo"));
        assert_eq!(list.entries()[2].branch.as_deref(), Some("release/1.0"));
    }

    #[test]
    fn test_worktree_list_parse_branch_with_at_sign() {
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-v2\nHEAD def67890xxxxxx\nbranch refs/heads/feature@v2\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries().len(), 2);
        assert_eq!(list.entries()[1].branch.as_deref(), Some("feature@v2"));
    }

    #[test]
    fn test_worktree_list_parse_trailing_newline_missing() {
        // Missing trailing blank line — last entry must still be captured
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-a\nHEAD def67890xxxxxx\nbranch refs/heads/feature-a";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries().len(), 2);
        assert_eq!(list.entries()[1].path, "/wt-a");
        assert_eq!(list.entries()[1].branch.as_deref(), Some("feature-a"));
    }

    #[test]
    fn test_worktree_list_parse_main_only_trailing_newline_missing() {
        // Single main entry without trailing newline; main() must not return None
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main";
        let list = WorktreeList::parse(output, None);
        assert!(list.main().is_some());
        assert_eq!(list.main().unwrap().path, "/repo");
        assert_eq!(list.main().unwrap().branch.as_deref(), Some("main"));
    }

    #[test]
    fn test_worktree_list_parse_single_entry() {
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries().len(), 1);
        assert!(list.non_main().is_empty());
    }

    #[test]
    fn test_worktree_list_parse_empty_input() {
        let list = WorktreeList::parse("", None);
        assert!(list.entries().is_empty());
        assert!(list.main().is_none());
        assert!(list.non_main().is_empty());
    }

    #[test]
    fn test_worktree_list_parse_no_trim_required_on_canonical_git_output() {
        // Canonical git porcelain output has no leading/trailing whitespace per line
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt\nHEAD def67890xxxxxx\nbranch refs/heads/feat\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries().len(), 2);
        assert_eq!(list.entries()[0].path, "/repo");
        assert_eq!(list.entries()[1].path, "/wt");
        assert_eq!(list.entries()[1].branch.as_deref(), Some("feat"));
    }

    #[test]
    fn test_worktree_list_parse_malformed_branch_before_worktree_panic_free() {
        // Malformed input where `branch` line appears before any `worktree` line
        // Must not panic; behavior is best-effort (the orphan branch is dropped)
        let output = "branch refs/heads/orphan\nworktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries().len(), 1);
        assert_eq!(list.entries()[0].path, "/repo");
        assert_eq!(list.entries()[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn test_worktree_list_main_returns_first_entry_when_present() {
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt\nHEAD def67890xxxxxx\nbranch refs/heads/feat\n\n";
        let list = WorktreeList::parse(output, None);
        assert!(list.main().is_some());
        assert_eq!(list.main().unwrap().path, "/repo");
    }

    #[test]
    fn test_worktree_list_main_returns_none_on_empty_list() {
        let list = WorktreeList::parse("", None);
        assert!(list.main().is_none());
    }

    #[test]
    fn test_worktree_list_non_main_excludes_first() {
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-a\nHEAD def67890xxxxxx\nbranch refs/heads/feature-a\n\nworktree /wt-b\nHEAD eee99999xxxxxx\nbranch refs/heads/feature-b\n\n";
        let list = WorktreeList::parse(output, None);
        let non_main = list.non_main();
        assert_eq!(non_main.len(), 2);
        assert_eq!(non_main[0].path, "/wt-a");
        assert_eq!(non_main[1].path, "/wt-b");
    }

    #[test]
    fn test_worktree_list_find_by_branch_hits() {
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-a\nHEAD def67890xxxxxx\nbranch refs/heads/feature-a\n\n";
        let list = WorktreeList::parse(output, None);
        let entry = list.find_by_branch("feature-a");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().path, "/wt-a");
    }

    #[test]
    fn test_worktree_list_find_by_branch_misses() {
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-a\nHEAD def67890xxxxxx\nbranch refs/heads/feature-a\n\n";
        let list = WorktreeList::parse(output, None);
        assert!(list.find_by_branch("nonexistent").is_none());
    }

    #[test]
    fn test_worktree_list_find_by_branch_main_excluded() {
        // find_by_branch must not return the main worktree even if branch matches
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-a\nHEAD def67890xxxxxx\nbranch refs/heads/feature-a\n\n";
        let list = WorktreeList::parse(output, None);
        assert!(list.find_by_branch("main").is_none());
    }

    #[test]
    fn test_worktree_list_find_by_branch_first_match_wins_on_duplicate() {
        // Abnormal input: same branch on two non-main worktrees — first wins
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-first\nHEAD def67890xxxxxx\nbranch refs/heads/dup\n\nworktree /wt-second\nHEAD eee99999xxxxxx\nbranch refs/heads/dup\n\n";
        let list = WorktreeList::parse(output, None);
        let entry = list.find_by_branch("dup");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().path, "/wt-first");
    }

    #[test]
    fn test_worktree_list_find_by_path_canonicalize_match() {
        // Use /tmp which always exists for canonicalize success path
        let tmp_canonical = std::fs::canonicalize("/tmp").unwrap();
        let tmp_str = tmp_canonical.to_string_lossy();
        let output = format!(
            "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree {tmp_str}\nHEAD def67890xxxxxx\nbranch refs/heads/feat\n\n"
        );
        let list = WorktreeList::parse(&output, None);
        let entry = list.find_by_path(std::path::Path::new("/tmp"));
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().path, tmp_str.as_ref());
    }

    #[test]
    fn test_worktree_list_find_by_path_string_fallback() {
        // Non-existent path — string comparison fallback path
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /nonexistent/path/wt\nHEAD def67890xxxxxx\nbranch refs/heads/feat\n\n";
        let list = WorktreeList::parse(output, None);
        let entry = list.find_by_path(std::path::Path::new("/nonexistent/path/wt"));
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().path, "/nonexistent/path/wt");
    }

    #[test]
    fn test_worktree_list_find_by_path_main_excluded() {
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-a\nHEAD def67890xxxxxx\nbranch refs/heads/feature-a\n\n";
        let list = WorktreeList::parse(output, None);
        assert!(list.find_by_path(std::path::Path::new("/repo")).is_none());
    }

    #[test]
    fn test_worktree_list_current_with_active_path_canonicalize() {
        let tmp_canonical = std::fs::canonicalize("/tmp").unwrap();
        let tmp_str = tmp_canonical.to_string_lossy();
        let output = format!(
            "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree {tmp_str}\nHEAD def67890xxxxxx\nbranch refs/heads/feat\n\n"
        );
        let list = WorktreeList::parse(&output, Some(std::path::Path::new("/tmp")));
        let cur = list.current();
        assert!(cur.is_some());
        assert_eq!(cur.unwrap().path, tmp_str.as_ref());
    }

    #[test]
    fn test_worktree_list_current_with_active_path_string_fallback() {
        // Non-existent active path — string comparison fallback
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /nonexistent/wt\nHEAD def67890xxxxxx\nbranch refs/heads/feat\n\n";
        let list = WorktreeList::parse(output, Some(std::path::Path::new("/nonexistent/wt")));
        let cur = list.current();
        assert!(cur.is_some());
        assert_eq!(cur.unwrap().path, "/nonexistent/wt");
    }

    #[test]
    fn test_worktree_list_current_no_active_returns_none() {
        let output = "worktree /repo\nHEAD abc12345xxxxxx\nbranch refs/heads/main\n\nworktree /wt-a\nHEAD def67890xxxxxx\nbranch refs/heads/feature-a\n\n";
        let list = WorktreeList::parse(output, None);
        assert!(list.current().is_none());
    }

    #[test]
    fn test_worktree_list_hash_truncated_to_8_chars() {
        let output = "worktree /repo\nHEAD abc12345def67890\nbranch refs/heads/main\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries()[0].hash.as_deref(), Some("abc12345"));
    }

    #[test]
    fn test_worktree_list_hash_none_when_head_line_missing() {
        // No HEAD line — hash should be None (pipe-mode behavior)
        let output = "worktree /repo\nbranch refs/heads/main\n\n";
        let list = WorktreeList::parse(output, None);
        assert_eq!(list.entries()[0].hash, None);
    }
}
