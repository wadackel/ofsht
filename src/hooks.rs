#![allow(clippy::missing_errors_doc)]
#![allow(clippy::literal_string_with_formatting_args)]
use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use walkdir::WalkDir;

use crate::color;
use crate::config::HookActions;
use crate::domain::worktree::display_path;

/// Hook executor interface for running hook actions
#[allow(dead_code)]
pub trait HookExecutor {
    /// Execute hook actions in the specified worktree directory
    ///
    /// # Arguments
    /// * `actions` - Hook actions to execute (run, copy, link)
    /// * `worktree_path` - Path to the worktree where hooks will be executed
    /// * `source_path` - Path to the source repository for file operations
    fn execute_hooks(
        &self,
        actions: &HookActions,
        worktree_path: &Path,
        source_path: &Path,
    ) -> Result<()>;
}

/// Real hook executor implementation
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct RealHookExecutor;

impl HookExecutor for RealHookExecutor {
    fn execute_hooks(
        &self,
        actions: &HookActions,
        worktree_path: &Path,
        source_path: &Path,
    ) -> Result<()> {
        // Use ColorMode::Auto for trait implementation
        execute_hooks_impl(actions, worktree_path, source_path, color::ColorMode::Auto)
    }
}

/// Pattern type for file matching
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatternKind {
    /// Literal file path (no glob metacharacters)
    Literal,
    /// Glob pattern (contains *, ?, [, ], {, })
    Glob,
}

/// Detect whether a pattern is a glob or a literal path
fn detect_pattern_kind(pattern: &str) -> PatternKind {
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
fn expand_pattern(pattern: &str, base: &Path) -> Result<(PatternKind, Vec<PathBuf>)> {
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
            let glob =
                Glob::new(pattern).with_context(|| format!("Invalid glob pattern: {pattern}"))?;
            let mut builder = GlobSetBuilder::new();
            builder.add(glob);
            let globset = builder.build()?;

            expand_glob(&globset, base)
        }
    };
    Ok((kind, paths))
}

/// Expand glob pattern to matching paths using walkdir
fn expand_glob(globset: &GlobSet, base: &Path) -> Vec<PathBuf> {
    let mut matches = Vec::new();

    for entry in WalkDir::new(base)
        .follow_links(false)
        .into_iter()
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

/// Execute hook actions in the specified directory
///
/// This is a backward-compatible wrapper around `RealHookExecutor`
pub fn execute_hooks(
    actions: &HookActions,
    worktree_path: &Path,
    source_path: &Path,
    color_mode: color::ColorMode,
) -> Result<()> {
    execute_hooks_impl(actions, worktree_path, source_path, color_mode)
}

/// Execute hook actions in the specified directory (internal implementation)
fn execute_hooks_impl(
    actions: &HookActions,
    worktree_path: &Path,
    source_path: &Path,
    color_mode: color::ColorMode,
) -> Result<()> {
    let total_actions = actions.run.len() + actions.copy.len() + actions.link.len();
    let mut action_index = 0;

    // Execute commands
    for cmd in &actions.run {
        action_index += 1;
        let is_last = action_index == total_actions;
        execute_command(cmd, worktree_path, color_mode, is_last)?;
    }

    // Copy files from source to worktree
    for pattern in &actions.copy {
        action_index += 1;
        let is_last = action_index == total_actions;
        copy_files(pattern, source_path, worktree_path, color_mode, is_last)?;
    }

    // Create symbolic links
    for pattern in &actions.link {
        action_index += 1;
        let is_last = action_index == total_actions;
        create_symlinks(pattern, source_path, worktree_path, color_mode, is_last)?;
    }

    Ok(())
}

fn execute_command(
    cmd: &str,
    working_dir: &Path,
    color_mode: color::ColorMode,
    is_last: bool,
) -> Result<()> {
    // Show progress indicator if TTY
    let spinner = if color_mode.should_colorize() {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message(format!("Running hook command: {cmd}"));
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    let start = Instant::now();
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(working_dir)
        .output()
        .with_context(|| format!("Failed to execute command: {cmd}"))?;
    let elapsed = start.elapsed();

    // Clear spinner
    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }

    // Print result with timing
    let timing_info = format_duration(elapsed);
    eprintln!(
        "{}",
        color::tree_item(
            color_mode,
            color::info(
                color_mode,
                format!(
                    "Running hook command: {cmd} {}",
                    color::dim(color_mode, timing_info)
                )
            ),
            is_last,
            1
        )
    );

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Hook command failed: {cmd}\n{stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        eprint!("{stdout}");
    }

    Ok(())
}

/// Format duration for display (only if >= 100ms)
fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 100 {
        String::new()
    } else if millis < 1000 {
        format!("({millis}ms)")
    } else {
        let secs = duration.as_secs_f64();
        format!("({secs:.1}s)")
    }
}

/// Copy files for a pattern (supports glob)
fn copy_files(
    pattern: &str,
    source_path: &Path,
    dest_path: &Path,
    color_mode: color::ColorMode,
    is_last: bool,
) -> Result<()> {
    let (kind, paths) = expand_pattern(pattern, source_path)?;

    // If literal and not found, warn user
    if kind == PatternKind::Literal && paths.is_empty() {
        eprintln!(
            "{}",
            color::tree_item(
                color_mode,
                color::warn(
                    color_mode,
                    format!(
                        "Source file not found, skipping: {}",
                        source_path.join(pattern).display()
                    )
                ),
                is_last,
                1
            )
        );
        return Ok(());
    }

    // Copy each matched path
    for src_path in paths {
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

        eprintln!(
            "{}",
            color::tree_item(
                color_mode,
                color::info(
                    color_mode,
                    format!(
                        "Copying: {} → {}",
                        display_path(&src_path),
                        display_path(&dst_path)
                    )
                ),
                is_last,
                1
            )
        );

        if src_path.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
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
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("Failed to create directory: {}", dst.display()))?;

    for entry in std::fs::read_dir(src)
        .with_context(|| format!("Failed to read directory: {}", src.display()))?
    {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
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

/// Create symlinks for a pattern (supports glob)
fn create_symlinks(
    pattern: &str,
    source_path: &Path,
    worktree_path: &Path,
    color_mode: color::ColorMode,
    is_last: bool,
) -> Result<()> {
    let (kind, paths) = expand_pattern(pattern, source_path)?;

    // If literal and not found, warn user
    if kind == PatternKind::Literal && paths.is_empty() {
        eprintln!(
            "{}",
            color::tree_item(
                color_mode,
                color::warn(
                    color_mode,
                    format!(
                        "Source file not found for symlink, skipping: {}",
                        source_path.join(pattern).display()
                    )
                ),
                is_last,
                1
            )
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

        eprintln!(
            "{}",
            color::tree_item(
                color_mode,
                color::info(
                    color_mode,
                    format!(
                        "Creating symlink: {} → {}",
                        display_path(&src_path),
                        display_path(&dst_path)
                    )
                ),
                is_last,
                1
            )
        );

        #[cfg(unix)]
        std::os::unix::fs::symlink(&src_path, &dst_path).with_context(|| {
            format!(
                "Failed to create symlink from {} to {}",
                src_path.display(),
                dst_path.display()
            )
        })?;

        #[cfg(windows)]
        {
            if src_path.is_dir() {
                std::os::windows::fs::symlink_dir(&src_path, &dst_path)
            } else {
                std::os::windows::fs::symlink_file(&src_path, &dst_path)
            }
            .with_context(|| {
                format!(
                    "Failed to create symlink from {} to {}",
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

    /// Mock hook executor for testing
    struct MockHookExecutor {
        should_fail: bool,
    }

    impl MockHookExecutor {
        fn new() -> Self {
            Self { should_fail: false }
        }

        fn with_failure() -> Self {
            Self { should_fail: true }
        }
    }

    impl HookExecutor for MockHookExecutor {
        fn execute_hooks(
            &self,
            _actions: &HookActions,
            _worktree_path: &Path,
            _source_path: &Path,
        ) -> Result<()> {
            if self.should_fail {
                anyhow::bail!("Mock hook executor failure");
            }
            Ok(())
        }
    }

    #[test]
    fn test_mock_hook_executor_success() {
        let executor = MockHookExecutor::new();
        let actions = HookActions::default();
        let worktree_path = PathBuf::from("/test/worktree");
        let source_path = PathBuf::from("/test/source");
        let result = executor.execute_hooks(&actions, &worktree_path, &source_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_hook_executor_failure() {
        let executor = MockHookExecutor::with_failure();
        let actions = HookActions::default();
        let worktree_path = PathBuf::from("/test/worktree");
        let source_path = PathBuf::from("/test/source");
        let result = executor.execute_hooks(&actions, &worktree_path, &source_path);
        assert!(result.is_err());
    }

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

        let (kind, paths) = expand_pattern("test_expand_literal.txt", &temp_dir).unwrap();
        assert_eq!(kind, PatternKind::Literal);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], test_file);

        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_expand_pattern_literal_not_exists() {
        let temp_dir = std::env::temp_dir();
        let (kind, paths) = expand_pattern("nonexistent_file.txt", &temp_dir).unwrap();
        assert_eq!(kind, PatternKind::Literal);
        assert_eq!(paths.len(), 0);
    }

    #[test]
    fn test_expand_pattern_glob_single_match() {
        let temp_dir = std::env::temp_dir().join("test_glob_single");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let test_file = temp_dir.join("test.txt");
        std::fs::write(&test_file, "test").unwrap();

        let (kind, paths) = expand_pattern("*.txt", &temp_dir).unwrap();
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

        let (kind, mut paths) = expand_pattern("*.json", &temp_dir).unwrap();
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

        let (kind, paths) = expand_pattern("*.xyz", &temp_dir).unwrap();
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
        let (kind, paths) = expand_pattern("node_modules", &temp_dir).unwrap();
        assert_eq!(kind, PatternKind::Literal);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], dir1);
        assert!(paths[0].is_dir());

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_execute_hooks_empty() {
        let actions = HookActions::default();
        let temp_dir = std::env::temp_dir();
        let result = execute_hooks(&actions, &temp_dir, &temp_dir, color::ColorMode::Never);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_command_success() {
        let temp_dir = std::env::temp_dir();
        let result = execute_command("echo test", &temp_dir, color::ColorMode::Never, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_command_failure() {
        let temp_dir = std::env::temp_dir();
        let result = execute_command("exit 1", &temp_dir, color::ColorMode::Never, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_command_with_stdout() {
        // Commands with stdout should not pollute stdout stream
        // (Hook output should go to stderr to avoid breaking shell integration)
        let temp_dir = std::env::temp_dir();

        // This test verifies the command executes successfully
        // The actual stream verification is done via integration testing
        let result = execute_command(
            "echo 'hook output'",
            &temp_dir,
            color::ColorMode::Never,
            false,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_copy_files_literal_not_exists() {
        let temp_dir = std::env::temp_dir();
        let result = copy_files(
            "nonexistent.txt",
            &temp_dir,
            &temp_dir,
            color::ColorMode::Never,
            false,
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

        let result = copy_files("*.json", &src_dir, &dst_dir, color::ColorMode::Never, false);
        assert!(result.is_ok());

        // Verify files were copied
        assert!(dst_dir.join("test1.json").exists());
        assert!(dst_dir.join("test2.json").exists());

        std::fs::remove_dir_all(&src_dir).ok();
        std::fs::remove_dir_all(&dst_dir).ok();
    }
}
