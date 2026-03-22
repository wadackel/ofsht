#![allow(clippy::missing_errors_doc)]
#![allow(clippy::literal_string_with_formatting_args)]
use anyhow::{Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use walkdir::WalkDir;

use crate::color;
use crate::config::HookActions;

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
        let mp = MultiProgress::new();
        let errors = execute_hooks_impl(
            actions,
            worktree_path,
            source_path,
            color::ColorMode::Auto,
            "  ",
            &mp,
        );
        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("{}", errors.join("; "))
        }
    }
}

/// Emit a static line into a `MultiProgress`, preserving bar ordering in TTY mode.
///
/// In TTY mode, creates a bar that immediately finishes with the message,
/// keeping it positioned correctly relative to active spinners.
/// In non-TTY mode, simply prints to stderr.
#[allow(clippy::missing_panics_doc)]
pub fn emit_line(mp: &MultiProgress, is_tty: bool, msg: String) {
    if is_tty {
        let bar = mp.add(ProgressBar::new(0));
        // set_style MUST be called before finish_with_message —
        // the default bar style has no {msg} placeholder.
        bar.set_style(ProgressStyle::with_template("{msg}").unwrap());
        bar.finish_with_message(msg);
    } else {
        eprintln!("{msg}");
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
            let glob = GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .with_context(|| format!("Invalid glob pattern: {pattern}"))?;
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
/// Returns `Err` if any hook action fails, after executing all actions.
/// All errors are collected and joined with "; ".
#[allow(dead_code)]
pub fn execute_hooks(
    actions: &HookActions,
    worktree_path: &Path,
    source_path: &Path,
    color_mode: color::ColorMode,
    indent: &str,
) -> Result<()> {
    let mp = MultiProgress::new();
    execute_hooks_with_mp(actions, worktree_path, source_path, color_mode, indent, &mp)
}

/// Execute hook actions with a shared `MultiProgress`.
///
/// Use this variant when the caller manages its own header spinner
/// in the same `MultiProgress`, ensuring correct bar ordering.
pub fn execute_hooks_with_mp(
    actions: &HookActions,
    worktree_path: &Path,
    source_path: &Path,
    color_mode: color::ColorMode,
    indent: &str,
    mp: &MultiProgress,
) -> Result<()> {
    let errors = execute_hooks_impl(actions, worktree_path, source_path, color_mode, indent, mp);
    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("{}", errors.join("; "))
    }
}

/// Execute hook actions, printing warnings on failure but never returning Err.
///
/// Hooks are supplementary automation — failures should not block the primary
/// operation (worktree creation, removal, etc.).
#[cfg(test)]
pub fn execute_hooks_lenient(
    actions: &HookActions,
    worktree_path: &Path,
    source_path: &Path,
    color_mode: color::ColorMode,
    indent: &str,
) {
    let mp = MultiProgress::new();
    execute_hooks_lenient_with_mp(actions, worktree_path, source_path, color_mode, indent, &mp);
}

/// Execute hook actions leniently with a shared `MultiProgress`.
///
/// Use this variant when the caller manages its own header spinner
/// in the same `MultiProgress`, ensuring correct bar ordering.
pub fn execute_hooks_lenient_with_mp(
    actions: &HookActions,
    worktree_path: &Path,
    source_path: &Path,
    color_mode: color::ColorMode,
    indent: &str,
    mp: &MultiProgress,
) {
    let is_tty = color_mode.should_colorize();
    let errors = execute_hooks_impl(actions, worktree_path, source_path, color_mode, indent, mp);
    for err in &errors {
        emit_line(
            mp,
            is_tty,
            format!(
                "{indent}{}",
                color::warn(color_mode, format!("Hook error: {err}"))
            ),
        );
    }
}

/// Execute hook actions in the specified directory (internal implementation)
///
/// Executes all hook actions regardless of individual failures, collecting
/// error messages into a `Vec<String>`.
fn execute_hooks_impl(
    actions: &HookActions,
    worktree_path: &Path,
    source_path: &Path,
    color_mode: color::ColorMode,
    indent: &str,
    mp: &MultiProgress,
) -> Vec<String> {
    let total_actions = actions.run.len() + actions.copy.len() + actions.link.len();
    let mut action_index = 0;
    let mut errors = Vec::new();

    // Execute commands
    for cmd in &actions.run {
        action_index += 1;
        let is_last = action_index == total_actions;
        if let Err(e) = execute_command(cmd, worktree_path, color_mode, is_last, indent, mp) {
            errors.push(e.to_string());
        }
    }

    // Copy files from source to worktree
    for pattern in &actions.copy {
        action_index += 1;
        let is_last = action_index == total_actions;
        if let Err(e) = copy_files(
            pattern,
            source_path,
            worktree_path,
            color_mode,
            is_last,
            indent,
            mp,
        ) {
            errors.push(e.to_string());
        }
    }

    // Create symbolic links
    for pattern in &actions.link {
        action_index += 1;
        let is_last = action_index == total_actions;
        if let Err(e) = create_symlinks(
            pattern,
            source_path,
            worktree_path,
            color_mode,
            is_last,
            indent,
            mp,
        ) {
            errors.push(e.to_string());
        }
    }

    errors
}

/// Number of trailing output lines to keep for failure diagnostics
const FAILURE_TAIL_LINES: usize = 10;

fn execute_command(
    cmd: &str,
    working_dir: &Path,
    color_mode: color::ColorMode,
    _is_last: bool,
    indent: &str,
    mp: &MultiProgress,
) -> Result<()> {
    let start = Instant::now();

    // Merge stderr into stdout at shell level, pipe the single stream.
    // This avoids deadlock (only one pipe to drain) and keeps output ordering natural.
    let merged_cmd = format!("{cmd} 2>&1");
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&merged_cmd)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("Failed to execute command: {cmd}"))?;

    let child_stdout = child.stdout.take().expect("stdout was piped");

    // Setup spinner + preview bar in the shared MultiProgress (TTY only)
    let is_tty = color_mode.should_colorize();
    let (spinner, preview_bar) = if is_tty {
        let spinner = mp.add(ProgressBar::new_spinner());
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{prefix}{spinner:.cyan} {msg}")
                .unwrap(),
        );
        spinner.set_prefix(indent.to_string());
        spinner.set_message(cmd.to_string());
        spinner.enable_steady_tick(Duration::from_millis(100));

        let preview = mp.add(ProgressBar::new(0));
        preview.set_style(ProgressStyle::with_template("{prefix}  {msg:.dim}").unwrap());
        preview.set_prefix(indent.to_string());

        (Some(spinner), Some(preview))
    } else {
        (None, None)
    };

    // Consume output in a background thread.
    // Updates preview bar in real-time and keeps last N lines for failure diagnostics.
    let preview_clone = preview_bar.clone();
    let reader_handle = std::thread::spawn(move || {
        let reader = BufReader::new(child_stdout);
        let mut tail = VecDeque::<String>::with_capacity(FAILURE_TAIL_LINES);
        for line in reader.lines().map_while(Result::ok) {
            // Update preview bar with truncated last line
            if let Some(ref pb) = preview_clone {
                let display = if line.len() > 60 {
                    format!("{}…", &line[..59])
                } else {
                    line.clone()
                };
                pb.set_message(display);
            }
            // Ring buffer for failure diagnostics
            if tail.len() >= FAILURE_TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        tail
    });

    let status = child
        .wait()
        .with_context(|| format!("Failed to wait for command: {cmd}"))?;
    let elapsed = start.elapsed();

    // Join reader thread to get tail buffer
    let tail = reader_handle.join().unwrap_or_default();

    // Clear preview bar
    if let Some(pb) = preview_bar {
        pb.finish_and_clear();
    }

    if status.success() {
        let timing_info = format_duration(elapsed);
        let msg = format!(
            "{indent}{} {}",
            color::success(color_mode, cmd),
            color::dim(color_mode, timing_info)
        );
        if let Some(pb) = spinner {
            // TTY: transform spinner into completion message (stays in place)
            pb.set_style(ProgressStyle::with_template("{msg}").unwrap());
            pb.finish_with_message(msg);
        } else {
            // non-TTY: print directly
            eprintln!("{msg}");
        }
    } else {
        // Clear spinner on failure
        if let Some(pb) = spinner {
            pb.finish_and_clear();
        }
        // Show last N lines of output for diagnostics
        for line in &tail {
            emit_line(
                mp,
                is_tty,
                format!("{indent}  {}", color::dim(color_mode, line)),
            );
        }
        anyhow::bail!("Hook command failed: {cmd}");
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

/// Result of ensuring a symlink exists at the destination path
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SymlinkResult {
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
fn ensure_symlink(src: &Path, dst: &Path) -> Result<SymlinkResult> {
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
fn create_symlinks(
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
        let result = execute_hooks(
            &actions,
            &temp_dir,
            &temp_dir,
            color::ColorMode::Never,
            "  ",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_command_success() {
        let temp_dir = std::env::temp_dir();
        let result = execute_command(
            "echo test",
            &temp_dir,
            color::ColorMode::Never,
            false,
            "  ",
            &MultiProgress::new(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_command_failure() {
        let temp_dir = std::env::temp_dir();
        let result = execute_command(
            "exit 1",
            &temp_dir,
            color::ColorMode::Never,
            false,
            "  ",
            &MultiProgress::new(),
        );
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
            "  ",
            &MultiProgress::new(),
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
            &src_dir,
            &dst_dir,
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

        let (kind, paths) = expand_pattern(".claude/wadackel-*", &temp_dir).unwrap();
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

        let (kind, paths) = expand_pattern("config/**/*.json", &temp_dir).unwrap();
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

    #[test]
    fn test_execute_hooks_continues_after_run_failure() {
        let tmp = std::env::temp_dir().join("test_hooks_continue");
        std::fs::create_dir_all(&tmp).unwrap();

        // Create a marker file to prove the second command ran
        let marker = tmp.join("second_ran");
        let actions = HookActions {
            run: vec!["exit 1".to_string(), format!("touch {}", marker.display())],
            copy: vec![],
            link: vec![],
        };

        let errors = execute_hooks_impl(
            &actions,
            &tmp,
            &tmp,
            color::ColorMode::Never,
            "  ",
            &MultiProgress::new(),
        );

        // First command should have failed
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Hook command failed"));

        // Second command should still have executed
        assert!(marker.exists(), "Second hook command was not executed");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_execute_hooks_returns_all_errors() {
        let tmp = std::env::temp_dir().join("test_hooks_all_errors");
        std::fs::create_dir_all(&tmp).unwrap();

        let actions = HookActions {
            run: vec!["exit 1".to_string(), "exit 2".to_string()],
            copy: vec![],
            link: vec![],
        };

        let errors = execute_hooks_impl(
            &actions,
            &tmp,
            &tmp,
            color::ColorMode::Never,
            "  ",
            &MultiProgress::new(),
        );

        // Both commands should have failed
        assert_eq!(errors.len(), 2);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_execute_hooks_strict_returns_err_on_failure() {
        let tmp = std::env::temp_dir().join("test_hooks_strict_err");
        std::fs::create_dir_all(&tmp).unwrap();

        let actions = HookActions {
            run: vec!["exit 1".to_string()],
            copy: vec![],
            link: vec![],
        };

        let result = execute_hooks(&actions, &tmp, &tmp, color::ColorMode::Never, "  ");
        assert!(result.is_err());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_execute_hooks_lenient_does_not_panic() {
        let tmp = std::env::temp_dir().join("test_hooks_lenient");
        std::fs::create_dir_all(&tmp).unwrap();

        let actions = HookActions {
            run: vec!["exit 1".to_string()],
            copy: vec![],
            link: vec![],
        };

        // execute_hooks_lenient returns () — it should not panic
        execute_hooks_lenient(&actions, &tmp, &tmp, color::ColorMode::Never, "  ");

        std::fs::remove_dir_all(&tmp).ok();
    }
}
