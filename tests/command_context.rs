//! Integration tests for `CommandContext` (Step 7 — observation 7).
//!
//! These tests need a real on-disk git repository because `CommandContext`
//! shells out to `git rev-parse --git-common-dir` during construction. They
//! mutate the process-global current working directory, so each test is marked
//! `#[serial]` (mirrors the existing pattern in
//! `tests/completions_behavior.rs`).

use assert_fs::prelude::*;
use assert_fs::TempDir;
use ofsht::color::ColorMode;
use ofsht::commands::context::CommandContext;
use ofsht::config::Config;
use serial_test::serial;
use std::path::Path;
use std::process::Command;

/// Initialise a temp directory as a git repository with one empty commit.
/// Returns the `TempDir` so the caller can extend it (e.g. write `.ofsht.toml`)
/// and so the directory is dropped when the test ends.
fn setup_temp_repo() -> TempDir {
    let dir = TempDir::new().expect("Failed to create temp dir");
    run_git(dir.path(), &["init", "-b", "main"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test User"]);
    run_git(dir.path(), &["commit", "--allow-empty", "-m", "init"]);
    dir
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// RAII helper: switch process cwd into `dir` and restore it on drop.
struct CwdGuard {
    original: std::path::PathBuf,
}

impl CwdGuard {
    fn enter(dir: &Path) -> Self {
        let original = std::env::current_dir().expect("Failed to get cwd");
        std::env::set_current_dir(dir).expect("Failed to enter temp dir");
        Self { original }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        if self.original.exists() {
            let _ = std::env::set_current_dir(&self.original);
        }
    }
}

#[test]
#[serial]
fn test_new_strict_with_invalid_config_returns_err() {
    let repo = setup_temp_repo();
    // Schema-invalid TOML (parses as TOML but `hooks` should be a table).
    repo.child(".ofsht.toml")
        .write_str("hooks = \"not a table\"\n")
        .expect("Failed to write .ofsht.toml");

    let _cwd = CwdGuard::enter(repo.path());
    let result = CommandContext::new_strict(ColorMode::Never);
    assert!(
        result.is_err(),
        "new_strict must error on schema-invalid config, got Ok"
    );
}

#[test]
#[serial]
fn test_new_lenient_with_invalid_config_falls_back_to_default() {
    let repo = setup_temp_repo();
    repo.child(".ofsht.toml")
        .write_str("hooks = \"not a table\"\n")
        .expect("Failed to write .ofsht.toml");

    let _cwd = CwdGuard::enter(repo.path());
    let ctx = CommandContext::new_lenient(ColorMode::Never)
        .expect("new_lenient must succeed even when config is malformed");

    let default_dir = Config::default().worktree.dir;
    assert_eq!(
        ctx.config.worktree.dir, default_dir,
        "lenient context must fall back to Config::default() on parse error"
    );
}

#[test]
#[serial]
fn test_worktree_list_returns_porcelain_in_temp_repo() {
    let repo = setup_temp_repo();

    let _cwd = CwdGuard::enter(repo.path());
    let ctx = CommandContext::new_lenient(ColorMode::Never)
        .expect("new_lenient must succeed in a fresh git repo without config");

    let stdout = ctx
        .worktree_list_stdout()
        .expect("worktree_list_stdout must succeed in a valid git repo");

    assert!(
        stdout.starts_with("worktree "),
        "porcelain output must begin with 'worktree ': {stdout:?}"
    );
}
