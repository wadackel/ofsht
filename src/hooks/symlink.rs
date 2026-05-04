#![allow(clippy::missing_errors_doc)]
use anyhow::{Context, Result};
use indicatif::MultiProgress;
use std::path::Path;

use super::files::{expand_pattern, PatternKind};
use super::output::emit_line;
use crate::color;

/// Result of ensuring a symlink exists at the destination path
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SymlinkResult {
    /// A new symlink was created
    Created,
    /// The symlink already existed and points to the correct target
    AlreadyCorrect,
    /// The symlink existed but pointed to a different target; it was replaced
    Replaced,
}

/// Ensure a symlink at `dst` points to `src`, creating or replacing as needed
///
/// Returns an error if `dst` exists and is not a symlink (to protect user data).
pub(super) fn ensure_symlink(src: &Path, dst: &Path) -> Result<SymlinkResult> {
    let mut was_replaced = false;

    if let Ok(metadata) = dst.symlink_metadata() {
        if metadata.file_type().is_symlink() {
            let current_target = std::fs::read_link(dst)
                .with_context(|| format!("Failed to read symlink target: {}", dst.display()))?;
            if current_target == src {
                return Ok(SymlinkResult::AlreadyCorrect);
            }
            // Wrong target: remove and recreate
            // remove_file works for file symlinks; on Windows, directory symlinks need remove_dir
            std::fs::remove_file(dst)
                .or_else(|_| std::fs::remove_dir(dst))
                .with_context(|| format!("Failed to remove existing symlink: {}", dst.display()))?;
            was_replaced = true;
        } else {
            anyhow::bail!(
                "Cannot create symlink: {} already exists and is not a symlink",
                dst.display()
            );
        }
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(src, dst).with_context(|| {
        format!(
            "Failed to create symlink from {} to {}",
            src.display(),
            dst.display()
        )
    })?;

    #[cfg(windows)]
    {
        if src.is_dir() {
            std::os::windows::fs::symlink_dir(src, dst)
        } else {
            std::os::windows::fs::symlink_file(src, dst)
        }
        .with_context(|| {
            format!(
                "Failed to create symlink from {} to {}",
                src.display(),
                dst.display()
            )
        })?;
    }

    Ok(if was_replaced {
        SymlinkResult::Replaced
    } else {
        SymlinkResult::Created
    })
}

/// Create symlinks for a pattern (supports glob)
pub(super) fn create_symlinks(
    pattern: &str,
    source_path: &Path,
    worktree_path: &Path,
    color_mode: color::ColorMode,
    _is_last: bool,
    indent: &str,
    mp: &MultiProgress,
) -> Result<()> {
    let is_tty = color_mode.should_colorize();
    let (kind, paths) = expand_pattern(pattern, source_path)?;

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
                        "Source file not found for symlink, skipping: {}",
                        source_path.join(pattern).display()
                    )
                )
            ),
        );
        return Ok(());
    }

    // Create symlink for each matched path
    for src_path in paths {
        // Get relative path from source
        let rel_path = src_path
            .strip_prefix(source_path)
            .with_context(|| format!("Failed to get relative path for {}", src_path.display()))?;

        // Create same relative path in worktree
        let dst_path = worktree_path.join(rel_path);

        // Create parent directory if needed
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory: {}", parent.display())
            })?;
        }

        let result = ensure_symlink(&src_path, &dst_path)?;
        let msg = match result {
            SymlinkResult::Created | SymlinkResult::Replaced => {
                format!("Linked: {}", rel_path.display())
            }
            SymlinkResult::AlreadyCorrect => {
                format!("Linked (unchanged): {}", rel_path.display())
            }
        };
        emit_line(
            mp,
            is_tty,
            format!("{indent}{}", color::success(color_mode, msg)),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn test_create_symlinks_glob_directory_with_nested_files() {
        let src_dir = std::env::temp_dir().join("test_symlink_glob_src");
        let dst_dir = std::env::temp_dir().join("test_symlink_glob_dst");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dst_dir).unwrap();

        // Create: skills/skill-a/SKILL.md, skills/skill-b/SKILL.md
        let skill_a = src_dir.join("skills").join("skill-a");
        let skill_b = src_dir.join("skills").join("skill-b");
        std::fs::create_dir_all(&skill_a).unwrap();
        std::fs::create_dir_all(&skill_b).unwrap();
        std::fs::write(skill_a.join("SKILL.md"), "skill a").unwrap();
        std::fs::write(skill_b.join("SKILL.md"), "skill b").unwrap();

        // Should succeed without EEXIST errors - only directories are symlinked
        let result = create_symlinks(
            "skills/skill-*",
            &src_dir,
            &dst_dir,
            color::ColorMode::Never,
            false,
            "  ",
            &MultiProgress::new(),
        );
        assert!(result.is_ok(), "symlink creation failed: {result:?}");

        // Verify symlinks created for directories
        let link_a = dst_dir.join("skills").join("skill-a");
        let link_b = dst_dir.join("skills").join("skill-b");
        assert!(link_a.exists(), "skill-a symlink not found");
        assert!(link_b.exists(), "skill-b symlink not found");

        // Verify nested files are accessible through symlinks
        assert!(link_a.join("SKILL.md").exists());
        assert!(link_b.join("SKILL.md").exists());

        std::fs::remove_dir_all(&src_dir).ok();
        std::fs::remove_dir_all(&dst_dir).ok();
    }

    // ensure_symlink tests (unix only)
    #[test]
    #[cfg(unix)]
    fn test_ensure_symlink_creates_new() {
        let tmp = std::env::temp_dir().join("test_ensure_symlink_new");
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("src_file");
        let dst = tmp.join("dst_link");
        std::fs::write(&src, "hello").unwrap();

        let result = ensure_symlink(&src, &dst).unwrap();
        assert_eq!(result, SymlinkResult::Created);
        assert!(dst.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_link(&dst).unwrap(), src);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    #[cfg(unix)]
    fn test_ensure_symlink_already_correct() {
        let tmp = std::env::temp_dir().join("test_ensure_symlink_correct");
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("src_file");
        let dst = tmp.join("dst_link");
        std::fs::write(&src, "hello").unwrap();
        std::os::unix::fs::symlink(&src, &dst).unwrap();

        let result = ensure_symlink(&src, &dst).unwrap();
        assert_eq!(result, SymlinkResult::AlreadyCorrect);
        // Symlink should still point to the same target
        assert_eq!(std::fs::read_link(&dst).unwrap(), src);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    #[cfg(unix)]
    fn test_ensure_symlink_replaced_wrong_target() {
        let tmp = std::env::temp_dir().join("test_ensure_symlink_replace");
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("src_file");
        let other = tmp.join("other_file");
        let dst = tmp.join("dst_link");
        std::fs::write(&src, "hello").unwrap();
        std::fs::write(&other, "other").unwrap();
        // Point dst at 'other' first
        std::os::unix::fs::symlink(&other, &dst).unwrap();

        let result = ensure_symlink(&src, &dst).unwrap();
        assert_eq!(result, SymlinkResult::Replaced);
        assert_eq!(std::fs::read_link(&dst).unwrap(), src);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    #[cfg(unix)]
    fn test_ensure_symlink_dangling_replaced() {
        let tmp = std::env::temp_dir().join("test_ensure_symlink_dangling");
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("src_file");
        let nonexistent = tmp.join("gone");
        let dst = tmp.join("dst_link");
        std::fs::write(&src, "hello").unwrap();
        // Create dangling symlink (points to a path that doesn't exist)
        std::os::unix::fs::symlink(&nonexistent, &dst).unwrap();

        let result = ensure_symlink(&src, &dst).unwrap();
        assert_eq!(result, SymlinkResult::Replaced);
        assert_eq!(std::fs::read_link(&dst).unwrap(), src);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    #[cfg(unix)]
    fn test_ensure_symlink_regular_file_conflict() {
        let tmp = std::env::temp_dir().join("test_ensure_symlink_conflict_file");
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("src_file");
        let dst = tmp.join("dst_regular");
        std::fs::write(&src, "hello").unwrap();
        // dst is a regular file (not a symlink)
        std::fs::write(&dst, "regular").unwrap();

        let result = ensure_symlink(&src, &dst);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists and is not a symlink"),
            "unexpected error: {err}"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    #[cfg(unix)]
    fn test_ensure_symlink_directory_conflict() {
        let tmp = std::env::temp_dir().join("test_ensure_symlink_conflict_dir");
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("src_file");
        let dst = tmp.join("dst_directory");
        std::fs::write(&src, "hello").unwrap();
        // dst is a directory (not a symlink)
        std::fs::create_dir_all(&dst).unwrap();

        let result = ensure_symlink(&src, &dst);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists and is not a symlink"),
            "unexpected error: {err}"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }
}
