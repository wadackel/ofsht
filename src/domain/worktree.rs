//! Worktree domain entities and parsers
//!
//! This module contains data structures and parsing logic for git worktrees.

use crate::path_utils::{canonicalize_allow_missing, display_path};

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

/// Outcome of resolving a name against the non-main worktrees
#[derive(Debug, PartialEq, Eq)]
pub enum NameResolution<'a> {
    Found(&'a WorktreeEntry),
    /// The name matches several worktrees by branch or directory name
    Ambiguous(Vec<&'a WorktreeEntry>),
    NotFound,
}

/// Unified worktree list parsed from `git worktree list --porcelain` output.
///
/// Single parser + query methods replacing the previous family of standalone
/// porcelain scanners scattered across `commands/*` and `cli.rs`.
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
    #[must_use]
    pub fn current(&self) -> Option<&WorktreeEntry> {
        self.entries.iter().find(|e| e.is_active)
    }

    /// Resolve a name without consulting the current directory.
    ///
    /// The name matches every worktree on that branch, plus the worktree at that path below
    /// `root`, or failing that every worktree outside `root` with that directory name. One
    /// distinct match is found; several are ambiguous. The main worktree is never matched.
    #[must_use]
    pub fn resolve_short_name(
        &self,
        name: &str,
        root: Option<&std::path::Path>,
    ) -> NameResolution<'_> {
        let below_root = root.and_then(|r| self.find_by_path(&r.join(name)));
        let mut matches: Vec<&WorktreeEntry> = self
            .non_main()
            .iter()
            .filter(|e| {
                let by_branch = e.branch.as_deref() == Some(name);
                let by_directory = below_root.map_or_else(
                    || directory_name(e) == Some(name) && name_below_root(e, root).is_none(),
                    |found| found.path == e.path,
                );
                by_branch || by_directory
            })
            .collect();
        match matches.len() {
            0 => NameResolution::NotFound,
            1 => NameResolution::Found(matches.remove(0)),
            _ => NameResolution::Ambiguous(matches),
        }
    }

    /// Resolve user input: `resolve_short_name`, then an absolute path or a path relative to
    /// the current directory.
    #[must_use]
    pub fn resolve_name(&self, name: &str, root: Option<&std::path::Path>) -> NameResolution<'_> {
        match self.resolve_short_name(name, root) {
            NameResolution::NotFound => self
                .find_by_path(std::path::Path::new(name))
                .map_or(NameResolution::NotFound, NameResolution::Found),
            resolution => resolution,
        }
    }

    /// The short name of `entry`, or `None` when typing that name would reach something else.
    #[must_use]
    pub fn short_name(
        &self,
        entry: &WorktreeEntry,
        root: Option<&std::path::Path>,
    ) -> Option<String> {
        let name =
            name_below_root(entry, root).or_else(|| directory_name(entry).map(str::to_string))?;
        self.reaches(&name, entry, root).then_some(name)
    }

    /// Whether typing `name` reaches `entry`
    #[must_use]
    pub fn reaches(
        &self,
        name: &str,
        entry: &WorktreeEntry,
        root: Option<&std::path::Path>,
    ) -> bool {
        // `resolve_short_name` skips the main worktree, but `cd` takes `@` and the main branch
        // to it before any name is resolved
        let main_branch = self.main().and_then(|m| m.branch.as_deref());
        if name == "@" || main_branch == Some(name) {
            return false;
        }

        matches!(
            self.resolve_short_name(name, root),
            NameResolution::Found(found) if found.path == entry.path
        )
    }

    /// Names to show for `entries()`, in the same order: `@` for the main worktree, the short
    /// name where one exists, and the home-relative path otherwise.
    #[must_use]
    pub fn display_names(&self, root: Option<&std::path::Path>) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| self.display_name(entry, root))
            .collect()
    }

    /// The name `display_names` shows for `entry`
    #[must_use]
    pub fn display_name(&self, entry: &WorktreeEntry, root: Option<&std::path::Path>) -> String {
        if self.is_main(entry) {
            "@".to_string()
        } else {
            self.short_name(entry, root)
                .unwrap_or_else(|| display_path(std::path::Path::new(&entry.path)))
        }
    }

    /// A name that `ofsht cd` takes to `entry`: its branch when that reaches it, otherwise the
    /// name `display_names` shows
    #[must_use]
    pub fn cd_name(&self, entry: &WorktreeEntry, root: Option<&std::path::Path>) -> String {
        // `cd` takes the main branch to the main worktree before resolving names, so it reaches
        // main although `reaches` says no
        match entry.branch.as_deref() {
            Some(branch) if self.is_main(entry) || self.reaches(branch, entry, root) => {
                branch.to_string()
            }
            _ => self.display_name(entry, root),
        }
    }

    fn is_main(&self, entry: &WorktreeEntry) -> bool {
        self.main().is_some_and(|main| main.path == entry.path)
    }
}

/// The path of `entry` below `root`, or `None` when it is outside `root` or is `root` itself
fn name_below_root(entry: &WorktreeEntry, root: Option<&std::path::Path>) -> Option<String> {
    let root = root?;
    let path = canonicalize_allow_missing(std::path::Path::new(&entry.path));
    calculate_relative_path(&path, root).filter(|name| !name.is_empty())
}

fn directory_name(entry: &WorktreeEntry) -> Option<&str> {
    std::path::Path::new(&entry.path)
        .file_name()
        .and_then(|name| name.to_str())
}

/// Paths of the worktrees that `git worktree list --porcelain` reports as locked
#[must_use]
pub fn locked_paths(porcelain: &str) -> Vec<String> {
    porcelain
        .split("\n\n")
        .filter(|block| {
            block
                .lines()
                .any(|line| line == "locked" || line.starts_with("locked "))
        })
        .filter_map(|block| {
            block
                .lines()
                .find_map(|line| line.strip_prefix("worktree "))
                .map(str::to_string)
        })
        .collect()
}

/// Path and full HEAD of each worktree that `git worktree list --porcelain` reports as detached
#[must_use]
pub fn detached_heads(porcelain: &str) -> Vec<(String, String)> {
    porcelain
        .split("\n\n")
        .filter(|block| block.lines().any(|line| line == "detached"))
        .filter_map(|block| {
            let path = block
                .lines()
                .find_map(|line| line.strip_prefix("worktree "))?;
            let head = block.lines().find_map(|line| line.strip_prefix("HEAD "))?;
            Some((path.to_string(), head.to_string()))
        })
        .collect()
}

/// The path `worktree.dir` gives a worktree for `branch`
///
/// `{repo}` is the repository root's directory name; a relative result is resolved from the
/// repository root. `None` when the root has no usable name.
#[must_use]
pub fn expand_worktree_path(
    template: &str,
    repo_root: &std::path::Path,
    branch: &str,
) -> Option<std::path::PathBuf> {
    let repo_name = repo_root.file_name()?.to_str()?;
    #[allow(clippy::literal_string_with_formatting_args)]
    let expanded = template
        .replace("{repo}", repo_name)
        .replace("{branch}", branch);
    Some(if expanded.starts_with('/') {
        std::path::PathBuf::from(expanded)
    } else {
        repo_root.join(expanded)
    })
}

/// The branch whose `worktree.dir` expansion is `worktree_path`, if any
///
/// Takes the part of the path where `{branch}` sits and accepts it only when expanding the
/// template with it gives the same path again.
#[must_use]
pub fn branch_from_template(
    template: &str,
    repo_root: &std::path::Path,
    worktree_path: &std::path::Path,
) -> Option<String> {
    let repo_name = repo_root.file_name()?.to_str()?;
    #[allow(clippy::literal_string_with_formatting_args)]
    let expanded = template.replace("{repo}", repo_name);
    let (before, after) = expanded.split_once("{branch}")?;
    let (directory, partial) = before.split_at(
        before
            .rfind(std::path::is_separator)
            .map_or(0, |index| index + 1),
    );
    let base = if directory.starts_with('/') {
        std::path::PathBuf::from(directory)
    } else {
        repo_root.join(directory)
    };

    let path = canonicalize_allow_missing(worktree_path);
    let relative = path.strip_prefix(canonicalize_allow_missing(&base)).ok()?;
    let candidate = relative
        .to_str()?
        .strip_prefix(partial)?
        .strip_suffix(after)?;
    if candidate.is_empty() {
        return None;
    }
    let again = expand_worktree_path(template, repo_root, candidate)?;
    (canonicalize_allow_missing(&again) == path).then(|| candidate.to_string())
}

/// The directory that `worktree.dir` places worktrees under
///
/// Follows the expansion in `WorktreeService`. Only complete path components count, so
/// `../{repo}-{branch}` gives the parent of the repository. Returns `None` when the template
/// has no `{branch}` or the repository root has no usable name.
#[must_use]
pub fn worktree_root_from_template(
    template: &str,
    repo_root: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let repo_name = repo_root.file_name()?.to_str()?;
    let (before_branch, _) = template.split_once("{branch}")?;
    let expanded = before_branch.replace("{repo}", repo_name);
    let directory = expanded
        .rfind(std::path::is_separator)
        .map_or("", |index| &expanded[..=index]);
    Some(canonicalize_allow_missing(&repo_root.join(directory)))
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
/// For nested branch names (e.g., `docs/tweak`), use `worktree_root_from_template` instead.
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
    fn test_resolve_short_name_finds_worktree_by_branch() {
        let list = list_of(&[("/repo", Some("main")), ("/wt-a", Some("feature-a"))]);
        assert_eq!(
            found_path(&list.resolve_short_name("feature-a", None)).as_deref(),
            Some("/wt-a")
        );
        assert_eq!(
            list.resolve_short_name("nonexistent", None),
            NameResolution::NotFound
        );
    }

    #[test]
    fn test_resolve_short_name_never_matches_main_branch() {
        let list = list_of(&[("/repo", Some("main")), ("/wt-a", Some("feature-a"))]);
        assert_eq!(
            list.resolve_short_name("main", None),
            NameResolution::NotFound
        );
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

    const ROOT: &str = "/work/myapp-worktrees";

    /// Build a list from `(path, branch)` pairs; the first pair is the main worktree.
    fn list_of(worktrees: &[(&str, Option<&str>)]) -> WorktreeList {
        let blocks: Vec<String> = worktrees
            .iter()
            .map(|(path, branch)| {
                let head = branch.map_or_else(
                    || "detached".to_string(),
                    |b| format!("branch refs/heads/{b}"),
                );
                format!("worktree {path}\nHEAD abc12345xxxxxx\n{head}\n\n")
            })
            .collect();
        WorktreeList::parse(&blocks.concat(), None)
    }

    fn root() -> &'static std::path::Path {
        std::path::Path::new(ROOT)
    }

    fn found_path(resolution: &NameResolution<'_>) -> Option<String> {
        match resolution {
            NameResolution::Found(entry) => Some(entry.path.clone()),
            _ => None,
        }
    }

    #[test]
    #[allow(clippy::literal_string_with_formatting_args)]
    fn test_branch_from_template_inverts_the_expansion() {
        let repo = std::path::Path::new("/work/myapp");
        let default = "../{repo}-worktrees/{branch}";
        let branch = |template: &str, path: &str| {
            branch_from_template(template, repo, std::path::Path::new(path))
        };
        assert_eq!(
            branch(default, "/work/myapp-worktrees/slot-1").as_deref(),
            Some("slot-1")
        );
        assert_eq!(
            branch(default, "/work/myapp-worktrees/feature/foo").as_deref(),
            Some("feature/foo")
        );
        assert_eq!(
            branch("../{repo}-{branch}", "/work/myapp-slot-1").as_deref(),
            Some("slot-1")
        );
        assert_eq!(
            branch("/pool/{repo}/{branch}/src", "/pool/myapp/slot-1/src").as_deref(),
            Some("slot-1")
        );
        assert_eq!(branch(default, "/elsewhere/slot-1"), None);
        assert_eq!(branch(default, "/work/myapp-worktrees"), None);
        assert_eq!(branch("../fixed", "/work/fixed"), None);
    }

    #[test]
    #[allow(clippy::literal_string_with_formatting_args)]
    fn test_expand_worktree_path_resolves_relative_templates_from_the_root() {
        let repo = std::path::Path::new("/work/myapp");
        assert_eq!(
            expand_worktree_path("../{repo}-worktrees/{branch}", repo, "a/b"),
            Some(std::path::PathBuf::from(
                "/work/myapp/../myapp-worktrees/a/b"
            ))
        );
        assert_eq!(
            expand_worktree_path("/pool/{branch}", repo, "x"),
            Some(std::path::PathBuf::from("/pool/x"))
        );
    }

    #[test]
    fn test_locked_paths_reads_locked_lines_with_and_without_reason() {
        let porcelain = "worktree /repo\nHEAD a\nbranch refs/heads/main\n\nworktree /wt-a\nHEAD b\nbranch refs/heads/a\nlocked\n\nworktree /wt-b\nHEAD c\ndetached\nlocked on a usb disk\n\nworktree /wt-c\nHEAD d\nbranch refs/heads/c\n\n";
        assert_eq!(locked_paths(porcelain), ["/wt-a", "/wt-b"]);
        assert!(locked_paths("worktree /repo\nHEAD a\nbranch refs/heads/main\n").is_empty());
    }

    #[test]
    fn test_detached_heads_returns_path_and_full_sha() {
        let porcelain = "worktree /repo\nHEAD aaa\nbranch refs/heads/main\n\nworktree /wt-a\nHEAD 0123456789abcdef0123456789abcdef01234567\ndetached\n\nworktree /wt-b\nHEAD bbb\nbranch refs/heads/b\n\n";
        assert_eq!(
            detached_heads(porcelain),
            [(
                "/wt-a".to_string(),
                "0123456789abcdef0123456789abcdef01234567".to_string()
            )]
        );
    }

    #[test]
    fn test_worktree_root_from_template_default() {
        let root = worktree_root_from_template(
            "../{repo}-worktrees/{branch}",
            std::path::Path::new("/work/myapp"),
        );
        assert_eq!(root, Some(PathBuf::from(ROOT)));
    }

    #[test]
    fn test_worktree_root_from_template_absolute() {
        let root = worktree_root_from_template(
            "/srv/worktrees/{repo}/{branch}",
            std::path::Path::new("/work/myapp"),
        );
        assert_eq!(root, Some(PathBuf::from("/srv/worktrees/myapp")));
    }

    #[test]
    fn test_worktree_root_from_template_branch_inside_path_component() {
        let root =
            worktree_root_from_template("../{repo}-{branch}", std::path::Path::new("/work/myapp"));
        assert_eq!(root, Some(PathBuf::from("/work")));
    }

    #[test]
    fn test_worktree_root_from_template_without_separator_is_repo_root() {
        let repo_root = std::path::Path::new("/work/myapp");
        assert_eq!(
            worktree_root_from_template("{branch}", repo_root),
            Some(PathBuf::from("/work/myapp"))
        );
        assert_eq!(
            worktree_root_from_template("{repo}-{branch}", repo_root),
            Some(PathBuf::from("/work/myapp"))
        );
    }

    #[test]
    fn test_worktree_root_from_template_without_branch_is_none() {
        let root = worktree_root_from_template("../fixed", std::path::Path::new("/work/myapp"));
        assert_eq!(root, None);
    }

    fn ambiguous_paths(resolution: &NameResolution<'_>) -> Option<Vec<String>> {
        match resolution {
            NameResolution::Ambiguous(entries) => {
                Some(entries.iter().map(|e| e.path.clone()).collect())
            }
            _ => None,
        }
    }

    #[test]
    fn test_resolve_short_name_reports_branch_and_root_name_collision() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/review", Some("feature/review")),
            ("/elsewhere/other", Some("review")),
        ]);
        assert_eq!(
            ambiguous_paths(&list.resolve_short_name("review", Some(root()))),
            Some(vec![
                "/work/myapp-worktrees/review".to_string(),
                "/elsewhere/other".to_string()
            ])
        );
    }

    #[test]
    fn test_resolve_short_name_reports_branch_and_outside_name_collision() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/elsewhere/a/review", Some("review")),
            ("/elsewhere/b/review", Some("fix/b")),
        ]);
        assert_eq!(
            ambiguous_paths(&list.resolve_short_name("review", Some(root()))),
            Some(vec![
                "/elsewhere/a/review".to_string(),
                "/elsewhere/b/review".to_string()
            ])
        );
    }

    #[test]
    fn test_resolve_short_name_reports_duplicate_branches() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/wt1", Some("dup")),
            ("/work/myapp-worktrees/wt2", Some("dup")),
        ]);
        assert_eq!(
            ambiguous_paths(&list.resolve_short_name("dup", Some(root()))),
            Some(vec![
                "/work/myapp-worktrees/wt1".to_string(),
                "/work/myapp-worktrees/wt2".to_string()
            ])
        );
    }

    #[test]
    fn test_resolve_short_name_finds_worktree_matching_branch_and_directory() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/review", Some("review")),
        ]);
        assert_eq!(
            found_path(&list.resolve_short_name("review", Some(root()))).as_deref(),
            Some("/work/myapp-worktrees/review")
        );
    }

    #[test]
    fn test_resolve_short_name_prefers_root_name_over_outside_directory_name() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/review", Some("feature/review")),
            ("/elsewhere/review", Some("fix/typo")),
        ]);
        assert_eq!(
            found_path(&list.resolve_short_name("review", Some(root()))).as_deref(),
            Some("/work/myapp-worktrees/review")
        );
    }

    #[test]
    fn test_resolve_short_name_finds_outside_worktree_by_directory_name() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/wt1", Some("feature/login")),
            ("/elsewhere/check", None),
        ]);
        assert_eq!(
            found_path(&list.resolve_short_name("check", Some(root()))).as_deref(),
            Some("/elsewhere/check")
        );
    }

    #[test]
    fn test_resolve_short_name_reports_outside_duplicates_as_ambiguous() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/elsewhere/a/review", Some("fix/a")),
            ("/elsewhere/b/review", Some("fix/b")),
        ]);
        let NameResolution::Ambiguous(entries) = list.resolve_short_name("review", Some(root()))
        else {
            panic!("expected Ambiguous");
        };
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["/elsewhere/a/review", "/elsewhere/b/review"]);
    }

    #[test]
    fn test_resolve_short_name_skips_tail_of_nested_root_worktree() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/feature/a", Some("x1")),
        ]);
        assert_eq!(
            list.resolve_short_name("a", Some(root())),
            NameResolution::NotFound
        );
    }

    #[test]
    fn test_resolve_short_name_never_matches_main_directory() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/elsewhere/other", Some("fix/other")),
        ]);
        assert_eq!(
            list.resolve_short_name("myapp", Some(root())),
            NameResolution::NotFound
        );
    }

    #[test]
    fn test_resolve_short_name_without_root_treats_all_as_outside() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/wt1", Some("feature/login")),
        ]);
        assert_eq!(
            found_path(&list.resolve_short_name("wt1", None)).as_deref(),
            Some("/work/myapp-worktrees/wt1")
        );
    }

    #[test]
    fn test_resolve_name_falls_back_to_path() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/elsewhere/review", Some("fix/typo")),
        ]);
        assert_eq!(
            found_path(&list.resolve_name("/elsewhere/review", None)).as_deref(),
            Some("/elsewhere/review")
        );
    }

    #[test]
    fn test_cd_name_is_the_branch_when_it_reaches_the_worktree() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/slot-1", Some("feature")),
        ]);
        assert_eq!(list.cd_name(&list.non_main()[0], Some(root())), "feature");
    }

    #[test]
    fn test_cd_name_falls_back_when_the_branch_is_ambiguous() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/slot-1", Some("slot-2")),
            ("/work/myapp-worktrees/slot-2", Some("other")),
        ]);
        assert_eq!(list.cd_name(&list.non_main()[0], Some(root())), "slot-1");
    }

    #[test]
    fn test_cd_name_of_main_is_its_branch() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/slot-1", Some("feature")),
        ]);
        assert_eq!(list.cd_name(&list.entries()[0], Some(root())), "main");
    }

    #[test]
    fn test_short_name_inside_root_is_relative_path() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/feature/a", Some("x1")),
        ]);
        let entry = &list.non_main()[0];
        assert_eq!(
            list.short_name(entry, Some(root())).as_deref(),
            Some("feature/a")
        );
    }

    #[test]
    fn test_short_name_outside_root_is_directory_name() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/elsewhere/review", Some("fix/typo")),
        ]);
        let entry = &list.non_main()[0];
        assert_eq!(
            list.short_name(entry, Some(root())).as_deref(),
            Some("review")
        );
    }

    #[test]
    fn test_short_name_is_none_when_root_worktree_shares_directory_name() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/review", Some("feature/review")),
            ("/elsewhere/review", Some("fix/typo")),
        ]);
        let inside = &list.non_main()[0];
        let outside = &list.non_main()[1];
        assert_eq!(
            list.short_name(inside, Some(root())).as_deref(),
            Some("review")
        );
        assert_eq!(list.short_name(outside, Some(root())), None);
    }

    #[test]
    fn test_short_name_is_none_when_outside_worktrees_share_directory_name() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/elsewhere/a/review", Some("fix/a")),
            ("/elsewhere/b/review", Some("fix/b")),
        ]);
        assert_eq!(list.short_name(&list.non_main()[0], Some(root())), None);
        assert_eq!(list.short_name(&list.non_main()[1], Some(root())), None);
    }

    #[test]
    fn test_short_name_is_none_when_another_branch_takes_the_name() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/review", Some("feature/review")),
            ("/elsewhere/other", Some("review")),
        ]);
        assert_eq!(list.short_name(&list.non_main()[0], Some(root())), None);
    }

    #[test]
    fn test_short_name_is_none_when_main_branch_takes_the_name() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/main", Some("feature/other")),
        ]);
        assert_eq!(list.short_name(&list.non_main()[0], Some(root())), None);
    }

    #[test]
    fn test_short_name_is_none_for_directory_named_at_sign() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/elsewhere/@", Some("fix/typo")),
        ]);
        assert_eq!(list.short_name(&list.non_main()[0], Some(root())), None);
    }

    #[test]
    fn test_short_name_of_worktree_at_root_is_directory_name() {
        let list = list_of(&[("/work/myapp", Some("main")), (ROOT, Some("feature/root"))]);
        assert_eq!(
            list.short_name(&list.non_main()[0], Some(root()))
                .as_deref(),
            Some("myapp-worktrees")
        );
    }

    #[test]
    fn test_display_names_follow_entry_order() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/work/myapp-worktrees/wt1", Some("feature/login")),
            ("/elsewhere/check", None),
        ]);
        assert_eq!(list.display_names(Some(root())), ["@", "wt1", "check"]);
    }

    #[test]
    fn test_display_names_fall_back_to_path_for_duplicates() {
        let list = list_of(&[
            ("/work/myapp", Some("main")),
            ("/elsewhere/a/review", Some("fix/a")),
            ("/elsewhere/b/review", Some("fix/b")),
        ]);
        assert_eq!(
            list.display_names(Some(root())),
            ["@", "/elsewhere/a/review", "/elsewhere/b/review"]
        );
    }
}
