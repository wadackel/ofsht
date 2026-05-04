//! Pure path-manipulation utilities
//!
//! Cross-cutting helpers that operate on `std::path::Path` / `PathBuf` without
//! depending on `domain` or `commands`. Both layers are free to depend on this
//! module.

use std::path::{Component, Path, PathBuf};

/// Canonicalize a path, even if it doesn't exist on the filesystem
///
/// For missing paths, canonicalize the deepest existing ancestor and append the tail.
/// Relative paths are resolved from the current working directory.
#[must_use]
pub fn canonicalize_allow_missing(path: &Path) -> PathBuf {
    // Convert relative paths to absolute using current_dir
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    };

    // Normalize the path by processing . and .. components
    let normalized = normalize_path_lexically(&absolute_path);

    // Try normal canonicalization first
    if let Ok(canonical) = normalized.canonicalize() {
        return canonical;
    }

    // Path doesn't exist - find the deepest existing ancestor
    let mut current = normalized.as_path();
    let mut tail_components = Vec::new();

    loop {
        // Record this component first (before checking parent)
        if let Some(file_name) = current.file_name() {
            tail_components.push(file_name);
        }

        if let Some(parent) = current.parent() {
            if parent.exists() {
                // Found an existing ancestor - canonicalize it
                if let Ok(canonical_parent) = parent.canonicalize() {
                    // Rebuild the path by appending the tail components
                    let mut result = canonical_parent;
                    for component in tail_components.iter().rev() {
                        result = result.join(component);
                    }
                    return result;
                }
            }
            // Move up to parent
            current = parent;
        } else {
            // Reached the root without finding an existing ancestor
            // Fall back to the normalized path
            return normalized;
        }
    }
}

/// Lexically normalize a path by resolving `.` and `..` components
///
/// Does NOT resolve symlinks or touch the filesystem
fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
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
pub fn display_path(path: &Path) -> String {
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

/// Normalize path to absolute form without tilde conversion
///
/// Resolves `.` and `..` components lexically.
/// Use this for programmatic output (stdout) where absolute paths are needed.
/// For human-friendly display, use `display_path()` instead.
///
/// If the input path is relative, it will be converted to an absolute path
/// first using `canonicalize_allow_missing`, which works even if the path
/// doesn't exist on the filesystem.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use ofsht::path_utils::normalize_absolute_path;
///
/// let path = Path::new("/Users/test/ofsht/../ofsht-worktrees/feature");
/// assert_eq!(
///     normalize_absolute_path(path),
///     "/Users/test/ofsht-worktrees/feature"
/// );
/// ```
#[must_use]
pub fn normalize_absolute_path(path: &Path) -> String {
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        canonicalize_allow_missing(path)
    };
    normalize_path_lexically(&abs_path).display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- canonicalize_allow_missing tests (moved from src/commands/common.rs) ---

    #[test]
    fn test_canonicalize_allow_missing_existing_path() {
        // Test with existing path (current directory)
        let current_dir = std::env::current_dir().unwrap();
        let result = canonicalize_allow_missing(&current_dir);
        // Should return canonicalized path
        assert_eq!(result, current_dir.canonicalize().unwrap());
    }

    #[test]
    fn test_canonicalize_allow_missing_nonexistent_absolute() {
        // Test with nonexistent absolute path
        let nonexistent = PathBuf::from("/nonexistent/path/to/worktree");
        let result = canonicalize_allow_missing(&nonexistent);
        // Should return the absolute path (may have symlinks resolved in existing parts)
        assert!(result.is_absolute());
        assert!(result.to_string_lossy().contains("nonexistent"));
    }

    #[test]
    fn test_canonicalize_allow_missing_relative() {
        // Test with relative path
        let relative = PathBuf::from("./test/path");
        let result = canonicalize_allow_missing(&relative);
        // Should convert to absolute path
        assert!(result.is_absolute());
    }

    #[test]
    fn test_canonicalize_allow_missing_relative_nonexistent() {
        // Test with nonexistent relative path
        let relative = PathBuf::from("./nonexistent/test/path");
        let result = canonicalize_allow_missing(&relative);
        // Should convert to absolute path
        assert!(result.is_absolute());
        assert!(result.to_string_lossy().contains("nonexistent"));
    }

    #[test]
    fn test_canonicalize_allow_missing_deep_nonexistent() {
        // Test with deeply nested nonexistent path
        let current_dir = std::env::current_dir().unwrap().canonicalize().unwrap();
        let deep_path = current_dir.join("a/b/c/d/e/f/nonexistent");
        let result = canonicalize_allow_missing(&deep_path);
        // Should start with canonicalized current dir (not symlinked version)
        assert!(
            result.starts_with(&current_dir),
            "Result {} should start with canonicalized current_dir {}",
            result.display(),
            current_dir.display()
        );
        assert!(result.to_string_lossy().contains("nonexistent"));
    }

    #[test]
    fn test_canonicalize_allow_missing_parent_dots() {
        // Test with parent directory references
        let current_dir = std::env::current_dir().unwrap().canonicalize().unwrap();
        let with_dots = current_dir.join("foo/../bar/./baz");
        let result = canonicalize_allow_missing(&with_dots);
        // Should resolve . and .. references even when path doesn't exist
        let expected = current_dir.join("bar/baz");
        assert_eq!(
            result, expected,
            "Should normalize .. and . even in nonexistent paths"
        );
        // Result should not contain literal . or .. components
        let result_str = result.to_string_lossy();
        assert!(
            !result_str.contains("/./"),
            "Result should not contain /./: {result_str}"
        );
        assert!(
            !result_str.contains("/../"),
            "Result should not contain /../: {result_str}"
        );
    }

    // --- normalize_path_lexically tests (moved from src/domain/worktree.rs) ---

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

    // --- display_path tests (moved from src/domain/worktree.rs) ---

    #[test]
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

    #[test]
    fn test_display_path_under_home() {
        // Test path under home directory
        if let Some(home) = dirs::home_dir() {
            let test_path = home.join("test/path");
            let result = display_path(&test_path);
            assert!(result.starts_with("~/"));
            assert_eq!(result, "~/test/path");
        }
    }

    #[test]
    fn test_display_path_outside_home_unix_exact() {
        // Test path outside home directory (Unix-only exact-match assertion).
        // Renamed from `test_display_path_outside_home` because that name is already
        // used above for a platform-aware (cfg!(windows)) version that only checks
        // the leading '~' is absent. This keeps both assertions.
        let test_path = std::path::PathBuf::from("/tmp/test/path");
        let result = display_path(&test_path);
        assert_eq!(result, "/tmp/test/path");
    }

    #[test]
    fn test_display_path_home_itself() {
        // Test home directory itself
        if let Some(home) = dirs::home_dir() {
            let result = display_path(&home);
            assert_eq!(result, "~");
        }
    }

    // --- normalize_absolute_path tests (moved from src/domain/worktree.rs) ---

    #[test]
    fn test_normalize_absolute_path_resolves_parent_dirs() {
        // Test that .. components are resolved
        use std::path::MAIN_SEPARATOR;
        let path = if cfg!(windows) {
            PathBuf::from(format!(
                "C:{MAIN_SEPARATOR}Users{MAIN_SEPARATOR}test{MAIN_SEPARATOR}ofsht{MAIN_SEPARATOR}..{MAIN_SEPARATOR}ofsht-worktrees{MAIN_SEPARATOR}feature"
            ))
        } else {
            PathBuf::from("/Users/test/ofsht/../ofsht-worktrees/feature")
        };
        let result = normalize_absolute_path(&path);
        let expected = if cfg!(windows) {
            format!("C:{MAIN_SEPARATOR}Users{MAIN_SEPARATOR}test{MAIN_SEPARATOR}ofsht-worktrees{MAIN_SEPARATOR}feature")
        } else {
            "/Users/test/ofsht-worktrees/feature".to_string()
        };
        assert_eq!(result, expected);
        assert!(!result.contains(".."));
    }

    #[test]
    fn test_normalize_absolute_path_removes_current_dirs() {
        // Test that . components are removed
        use std::path::MAIN_SEPARATOR;
        let path = if cfg!(windows) {
            PathBuf::from(format!(
                "C:{MAIN_SEPARATOR}Users{MAIN_SEPARATOR}.{MAIN_SEPARATOR}test{MAIN_SEPARATOR}.{MAIN_SEPARATOR}feature"
            ))
        } else {
            PathBuf::from("/Users/./test/./feature")
        };
        let result = normalize_absolute_path(&path);
        let expected = if cfg!(windows) {
            format!("C:{MAIN_SEPARATOR}Users{MAIN_SEPARATOR}test{MAIN_SEPARATOR}feature")
        } else {
            "/Users/test/feature".to_string()
        };
        assert_eq!(result, expected);
        // Check that . is removed (but be careful as it might appear in extensions)
        assert!(!result.split(MAIN_SEPARATOR).any(|x| x == "."));
    }

    #[test]
    fn test_normalize_absolute_path_outside_home() {
        // Test paths outside home directory (no tilde conversion)
        use std::path::MAIN_SEPARATOR;
        let path = if cfg!(windows) {
            PathBuf::from(format!("C:{MAIN_SEPARATOR}temp{MAIN_SEPARATOR}worktree"))
        } else {
            PathBuf::from("/tmp/worktree")
        };
        let result = normalize_absolute_path(&path);
        assert!(!result.starts_with('~'));
        // Verify path structure is preserved
        assert!(result.contains("worktree"));
    }

    #[test]
    fn test_normalize_absolute_path_consistency_with_display_path() {
        // Test that normalization is consistent between the two functions
        use std::path::MAIN_SEPARATOR;
        let path = if cfg!(windows) {
            PathBuf::from(format!(
                "C:{MAIN_SEPARATOR}Users{MAIN_SEPARATOR}test{MAIN_SEPARATOR}ofsht{MAIN_SEPARATOR}..{MAIN_SEPARATOR}worktrees{MAIN_SEPARATOR}.{MAIN_SEPARATOR}feature{MAIN_SEPARATOR}.."
            ))
        } else {
            PathBuf::from("/Users/test/ofsht/../worktrees/./feature/..")
        };
        let normalized_abs = normalize_absolute_path(&path);
        let displayed = display_path(&path);

        // Both should normalize the same way (only difference is tilde conversion)
        assert!(!normalized_abs.contains(".."));
        assert!(!normalized_abs.split(MAIN_SEPARATOR).any(|x| x == "."));
        assert!(!displayed.contains(".."));

        // Absolute result should not have tilde
        assert!(!normalized_abs.starts_with('~'));
    }

    #[test]
    fn test_normalize_absolute_path_handles_relative_paths() {
        // Test that relative paths are safely converted to absolute paths
        let relative = PathBuf::from("worktrees/feature");
        let result = normalize_absolute_path(&relative);

        // Should be converted to absolute path (works on both Unix and Windows)
        assert!(PathBuf::from(&result).is_absolute());
        assert!(result.contains("worktrees"));
        assert!(result.contains("feature"));
    }
}
