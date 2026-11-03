#![allow(clippy::missing_errors_doc)]
use anyhow::{Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use indicatif::MultiProgress;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::output::emit_line;
use super::HookContext;
use crate::color;
use crate::path_utils::canonicalize_allow_missing;

#[derive(Default)]
pub(super) struct GlobScope {
    excluded_roots: Vec<PathBuf>,
}

impl GlobScope {
    pub fn new(roots: impl Iterator<Item = PathBuf>) -> Self {
        Self {
            excluded_roots: roots
                .map(|path| canonicalize_allow_missing(&path))
                .collect(),
        }
    }

    fn excludes(&self, path: &Path) -> bool {
        let canonical = canonicalize_allow_missing(path);
        path.components().any(|part| part.as_os_str() == ".git")
            || canonical
                .components()
                .any(|part| part.as_os_str() == ".git")
            || self
                .excluded_roots
                .iter()
                .any(|root| canonical.starts_with(root))
    }

    pub fn can_link_directory(&self, path: &Path) -> Result<bool> {
        let mut pending = vec![path.to_path_buf()];
        let mut visited = HashSet::new();
        while let Some(path) = pending.pop() {
            if self.excludes(&path) {
                return Ok(false);
            }
            let canonical = path.canonicalize().with_context(|| {
                format!("Failed to inspect glob link source: {}", path.display())
            })?;
            if !canonical.is_dir() || !visited.insert(canonical.clone()) {
                continue;
            }
            if self
                .excluded_roots
                .iter()
                .any(|root| root.starts_with(&canonical))
            {
                return Ok(false);
            }
            // A directory link exposes nested aliases even though glob traversal does not follow them.
            for entry in std::fs::read_dir(&canonical).with_context(|| {
                format!(
                    "Failed to inspect glob link directory: {}",
                    canonical.display()
                )
            })? {
                pending.push(entry?.path());
            }
        }
        Ok(true)
    }
}

/// Pattern type for file matching
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PatternKind {
    /// Literal file path (no glob metacharacters)
    Literal,
    /// Glob pattern (contains *, ?, [, ], {, })
    Glob,
}

/// Detect whether a pattern is a glob or a literal path
pub(super) fn detect_pattern_kind(pattern: &str) -> PatternKind {
    const GLOB_CHARS: &[char] = &['*', '?', '[', ']', '{', '}'];
    if pattern.chars().any(|c| GLOB_CHARS.contains(&c)) {
        PatternKind::Glob
    } else {
        PatternKind::Literal
    }
}

/// Expand a pattern to a list of matching paths
///
/// Returns a tuple of (`PatternKind`, `Vec<PathBuf>`)
/// - For literal patterns: returns the path if it exists, empty vec otherwise
/// - For glob patterns: returns all matching paths, empty vec if no matches
pub(super) fn expand_pattern(
    pattern: &str,
    base: &Path,
    scope: &GlobScope,
) -> Result<(PatternKind, Vec<PathBuf>)> {
    let kind = detect_pattern_kind(pattern);
    let paths = match kind {
        PatternKind::Literal => {
            let path = base.join(pattern);
            if path.exists() {
                vec![path]
            } else {
                vec![]
            }
        }
        PatternKind::Glob => {
            let glob = GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .with_context(|| format!("Invalid glob pattern: {pattern}"))?;
            let mut builder = GlobSetBuilder::new();
            builder.add(glob);
            let globset = builder.build()?;

            expand_glob(&globset, base, scope)
        }
    };
    Ok((kind, paths))
}

/// Expand glob pattern to matching paths using walkdir
fn expand_glob(globset: &GlobSet, base: &Path, scope: &GlobScope) -> Vec<PathBuf> {
    let mut matches = Vec::new();

    for entry in WalkDir::new(base)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !scope.excludes(entry.path()))
        .filter_map(std::result::Result::ok)
    {
        let path = entry.path();
        // Get relative path from base for glob matching
        if let Ok(rel_path) = path.strip_prefix(base) {
            if globset.is_match(rel_path) {
                matches.push(path.to_path_buf());
            }
        }
    }

    matches
}

/// Copy files for a pattern (supports glob)
pub(super) fn copy_files(
    pattern: &str,
    context: &HookContext,
    color_mode: color::ColorMode,
    _is_last: bool,
    indent: &str,
    mp: &MultiProgress,
) -> Result<()> {
    let is_tty = color_mode.should_colorize();
    let source_path = &context.repo_root;
    let dest_path = &context.worktree_path;
    let (kind, paths) = expand_pattern(pattern, source_path, &context.glob_scope)?;

    // If literal and not found, warn user
    if kind == PatternKind::Literal && paths.is_empty() {
        emit_line(
            mp,
            is_tty,
            format!(
                "{indent}{}",
                color::warn(
                    color_mode,
                    format!(
                        "Source file not found, skipping: {}",
                        source_path.join(pattern).display()
                    )
                )
            ),
        );
        return Ok(());
    }

    // Copy each matched path
    for src_path in paths {
        if kind == PatternKind::Glob && src_path.is_symlink() && src_path.is_dir() {
            continue;
        }
        // Get relative path from source
        let rel_path = src_path
            .strip_prefix(source_path)
            .with_context(|| format!("Failed to get relative path for {}", src_path.display()))?;

        // Create same relative path in destination
        let dst_path = dest_path.join(rel_path);

        // Create parent directory if needed
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory: {}", parent.display())
            })?;
        }

        emit_line(
            mp,
            is_tty,
            format!(
                "{indent}{}",
                color::success(color_mode, format!("Copied: {}", rel_path.display()))
            ),
        );

        if src_path.is_dir() {
            copy_dir_all(
                &src_path,
                &dst_path,
                (kind == PatternKind::Glob).then_some(&context.glob_scope),
            )?;
        } else {
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }

    Ok(())
}

/// Recursively copy a directory
fn copy_dir_all(src: &Path, dst: &Path, scope: Option<&GlobScope>) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("Failed to create directory: {}", dst.display()))?;

    for entry in std::fs::read_dir(src)
        .with_context(|| format!("Failed to read directory: {}", src.display()))?
    {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if scope.is_some_and(|scope| {
            scope.excludes(&src_path) || (src_path.is_symlink() && src_path.is_dir())
        }) {
            continue;
        }

        if src_path.is_dir() {
            copy_dir_all(&src_path, &dst_path, scope)?;
        } else {
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_pattern_kind_literal() {
        assert_eq!(detect_pattern_kind("file.txt"), PatternKind::Literal);
        assert_eq!(detect_pattern_kind("dir/file.txt"), PatternKind::Literal);
        assert_eq!(detect_pattern_kind(".env.local"), PatternKind::Literal);
        assert_eq!(
            detect_pattern_kind("config/test.toml"),
            PatternKind::Literal
        );
    }

    #[test]
    fn test_detect_pattern_kind_glob() {
        assert_eq!(detect_pattern_kind("*.txt"), PatternKind::Glob);
        assert_eq!(detect_pattern_kind("dir/**/*.rs"), PatternKind::Glob);
        assert_eq!(detect_pattern_kind(".env.*"), PatternKind::Glob);
        assert_eq!(detect_pattern_kind("file?.txt"), PatternKind::Glob);
        assert_eq!(detect_pattern_kind("file[0-9].txt"), PatternKind::Glob);
        assert_eq!(detect_pattern_kind("file{1,2}.txt"), PatternKind::Glob);
    }

    #[test]
    fn test_expand_pattern_literal_exists() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_expand_literal.txt");
        std::fs::write(&test_file, "test").unwrap();

        let (kind, paths) =
            expand_pattern("test_expand_literal.txt", &temp_dir, &GlobScope::default()).unwrap();
        assert_eq!(kind, PatternKind::Literal);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], test_file);

        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_expand_pattern_literal_not_exists() {
        let temp_dir = std::env::temp_dir();
        let (kind, paths) =
            expand_pattern("nonexistent_file.txt", &temp_dir, &GlobScope::default()).unwrap();
        assert_eq!(kind, PatternKind::Literal);
        assert_eq!(paths.len(), 0);
    }

    #[test]
    fn test_expand_pattern_glob_single_match() {
        let temp_dir = std::env::temp_dir().join("test_glob_single");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let test_file = temp_dir.join("test.txt");
        std::fs::write(&test_file, "test").unwrap();

        let (kind, paths) = expand_pattern("*.txt", &temp_dir, &GlobScope::default()).unwrap();
        assert_eq!(kind, PatternKind::Glob);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], test_file);

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_expand_pattern_glob_multiple_matches() {
        let temp_dir = std::env::temp_dir().join("test_glob_multiple");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let file1 = temp_dir.join("test1.json");
        let file2 = temp_dir.join("test2.json");
        std::fs::write(&file1, "{}").unwrap();
        std::fs::write(&file2, "{}").unwrap();

        let (kind, mut paths) = expand_pattern("*.json", &temp_dir, &GlobScope::default()).unwrap();
        assert_eq!(kind, PatternKind::Glob);
        assert_eq!(paths.len(), 2);
        paths.sort();
        assert!(paths.contains(&file1));
        assert!(paths.contains(&file2));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_expand_pattern_glob_no_match() {
        let temp_dir = std::env::temp_dir().join("test_glob_no_match");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let (kind, paths) = expand_pattern("*.xyz", &temp_dir, &GlobScope::default()).unwrap();
        assert_eq!(kind, PatternKind::Glob);
        assert_eq!(paths.len(), 0);

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_expand_pattern_directory_match() {
        let temp_dir = std::env::temp_dir().join("test_dir_match");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let dir1 = temp_dir.join("node_modules");
        std::fs::create_dir_all(&dir1).unwrap();

        // Test literal directory match
        let (kind, paths) =
            expand_pattern("node_modules", &temp_dir, &GlobScope::default()).unwrap();
        assert_eq!(kind, PatternKind::Literal);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], dir1);
        assert!(paths[0].is_dir());

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_expand_pattern_glob_star_does_not_cross_path_separator() {
        let temp_dir = std::env::temp_dir().join("test_glob_no_cross_sep");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create directory structure: .claude/wadackel-a/SKILL.md, .claude/wadackel-b/SKILL.md
        let dir_a = temp_dir.join(".claude").join("wadackel-a");
        let dir_b = temp_dir.join(".claude").join("wadackel-b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        std::fs::write(dir_a.join("SKILL.md"), "skill a").unwrap();
        std::fs::write(dir_b.join("SKILL.md"), "skill b").unwrap();

        let (kind, paths) =
            expand_pattern(".claude/wadackel-*", &temp_dir, &GlobScope::default()).unwrap();
        assert_eq!(kind, PatternKind::Glob);
        // Should match only the two directories, not the nested SKILL.md files
        assert_eq!(paths.len(), 2);
        let path_strs: Vec<_> = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert!(path_strs.iter().any(|s| s.ends_with("wadackel-a")));
        assert!(path_strs.iter().any(|s| s.ends_with("wadackel-b")));
        // Must NOT include nested files
        assert!(!path_strs.iter().any(|s| s.ends_with("SKILL.md")));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_expand_pattern_glob_double_star_matches_recursively() {
        let temp_dir = std::env::temp_dir().join("test_glob_double_star");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create: config/a.json, config/sub/b.json
        let config_dir = temp_dir.join("config");
        let sub_dir = config_dir.join("sub");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(config_dir.join("a.json"), "{}").unwrap();
        std::fs::write(sub_dir.join("b.json"), "{}").unwrap();

        let (kind, paths) =
            expand_pattern("config/**/*.json", &temp_dir, &GlobScope::default()).unwrap();
        assert_eq!(kind, PatternKind::Glob);
        // Both files should match via ** recursive glob
        assert_eq!(paths.len(), 2);
        let path_strs: Vec<_> = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert!(path_strs.iter().any(|s| s.ends_with("a.json")));
        assert!(path_strs.iter().any(|s| s.ends_with("b.json")));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_copy_files_literal_not_exists() {
        let temp_dir = std::env::temp_dir();
        let result = copy_files(
            "nonexistent.txt",
            &HookContext::for_test(&temp_dir),
            color::ColorMode::Never,
            false,
            "  ",
            &MultiProgress::new(),
        );
        assert!(result.is_ok()); // Should warn but not fail
    }

    #[test]
    fn test_copy_files_glob() {
        let src_dir = std::env::temp_dir().join("test_copy_glob_src");
        let dst_dir = std::env::temp_dir().join("test_copy_glob_dst");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dst_dir).unwrap();

        // Create test files
        std::fs::write(src_dir.join("test1.json"), "{}").unwrap();
        std::fs::write(src_dir.join("test2.json"), "{}").unwrap();

        let result = copy_files(
            "*.json",
            &HookContext {
                worktree_path: dst_dir.clone(),
                ..HookContext::for_test(&src_dir)
            },
            color::ColorMode::Never,
            false,
            "  ",
            &MultiProgress::new(),
        );
        assert!(result.is_ok());

        // Verify files were copied
        assert!(dst_dir.join("test1.json").exists());
        assert!(dst_dir.join("test2.json").exists());

        std::fs::remove_dir_all(&src_dir).ok();
        std::fs::remove_dir_all(&dst_dir).ok();
    }
}
