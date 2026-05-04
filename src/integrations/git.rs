#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::Path;
use std::process::Command;

/// Git client interface for git operations.
///
/// Methods that take `dir: Option<&Path>` use `dir` as the working
/// directory when `Some`, falling back to the current process working
/// directory when `None`.
pub trait GitClient {
    /// Create a worktree at `path`.
    ///
    /// When `start_point` is `Some`, runs `git worktree add -b <branch> <path> <start>`.
    /// When `start_point` is `None`, the implementation checks whether `branch`
    /// already exists; if so, runs `git worktree add <path> <branch>`,
    /// otherwise `git worktree add -b <branch> <path>`.
    fn create_worktree(
        &self,
        branch: &str,
        path: &Path,
        start_point: Option<&str>,
        dir: Option<&Path>,
    ) -> Result<()>;

    /// Run `git worktree list --porcelain`.
    fn list_worktrees(&self, dir: Option<&Path>) -> Result<String>;

    /// Run `git worktree remove <path>`.
    fn remove_worktree(&self, path: &Path, dir: Option<&Path>) -> Result<()>;

    /// Run `git branch -D <branch>`.
    ///
    /// Returns `Ok(true)` on success, `Ok(false)` when git exits non-zero
    /// (lenient case used by callers that treat deletion failure as a warning),
    /// and `Err` only when the git process cannot be spawned.
    fn remove_branch(&self, branch: &str, dir: Option<&Path>) -> Result<bool>;

    /// Run `git rev-parse --verify <ref>` and return whether it succeeded.
    ///
    /// Returns `Ok(true)` when the ref exists, `Ok(false)` when git exits
    /// non-zero (ref does not exist), and `Err` only when the git process
    /// cannot be spawned.
    fn branch_exists(&self, ref_: &str, dir: Option<&Path>) -> Result<bool>;

    /// Run `git <args>` (caller supplies the full argument list including
    /// `rev-parse`) and return stdout on success.
    fn rev_parse(&self, args: &[&str], dir: Option<&Path>) -> Result<String>;

    /// Run `git <args>` (caller supplies the full argument list including
    /// `fetch`).
    fn fetch(&self, args: &[&str], dir: Option<&Path>) -> Result<()>;

    /// Run `git for-each-ref --format=<format> <refs...>` and return stdout.
    fn for_each_ref(&self, refs: &[&str], format: &str, dir: Option<&Path>) -> Result<String>;

    /// Run `git -C <worktree_path> log -1 --format=%ct` and return the
    /// resulting timestamp. Returns `None` for any failure (spawn / non-zero
    /// exit / parse) to preserve the prior `domain::worktree::get_last_commit_time`
    /// silent-failure semantics.
    fn last_commit_time(&self, worktree_path: &Path) -> Option<DateTime<Utc>>;
}

/// Real git implementation. Zero-sized type.
#[derive(Debug, Default)]
pub struct RealGitClient;

fn build_command(dir: Option<&Path>) -> Command {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd
}

/// Spawn the configured command, fail with `git {op} failed: ...` on non-zero
/// exit, and return stdout on success. Centralizes the spawn-context, status
/// check, and bail pattern shared by every `RealGitClient` method that
/// propagates errors.
fn run_capturing(mut cmd: Command, op: &str) -> Result<String> {
    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute git {op}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {op} failed: {stderr}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

impl GitClient for RealGitClient {
    fn create_worktree(
        &self,
        branch: &str,
        path: &Path,
        start_point: Option<&str>,
        dir: Option<&Path>,
    ) -> Result<()> {
        let mut cmd = build_command(dir);
        cmd.arg("worktree").arg("add");

        if let Some(start) = start_point {
            cmd.arg("-b").arg(branch).arg(path).arg(start);
        } else if self.branch_exists(branch, dir)? {
            cmd.arg(path).arg(branch);
        } else {
            cmd.arg("-b").arg(branch).arg(path);
        }

        run_capturing(cmd, "worktree add")?;
        Ok(())
    }

    fn list_worktrees(&self, dir: Option<&Path>) -> Result<String> {
        let mut cmd = build_command(dir);
        cmd.args(["worktree", "list", "--porcelain"]);
        run_capturing(cmd, "worktree list")
    }

    fn remove_worktree(&self, path: &Path, dir: Option<&Path>) -> Result<()> {
        let mut cmd = build_command(dir);
        cmd.arg("worktree").arg("remove").arg(path);
        run_capturing(cmd, "worktree remove")?;
        Ok(())
    }

    fn remove_branch(&self, branch: &str, dir: Option<&Path>) -> Result<bool> {
        let mut cmd = build_command(dir);
        let output = cmd
            .args(["branch", "-D", branch])
            .output()
            .context("Failed to execute git branch -D")?;

        Ok(output.status.success())
    }

    fn branch_exists(&self, ref_: &str, dir: Option<&Path>) -> Result<bool> {
        let mut cmd = build_command(dir);
        let output = cmd
            .args(["rev-parse", "--verify", ref_])
            .output()
            .context("Failed to execute git rev-parse --verify")?;
        Ok(output.status.success())
    }

    fn rev_parse(&self, args: &[&str], dir: Option<&Path>) -> Result<String> {
        let mut cmd = build_command(dir);
        cmd.args(args);
        run_capturing(cmd, "rev-parse")
    }

    fn fetch(&self, args: &[&str], dir: Option<&Path>) -> Result<()> {
        let mut cmd = build_command(dir);
        cmd.args(args);
        run_capturing(cmd, "fetch")?;
        Ok(())
    }

    fn for_each_ref(&self, refs: &[&str], format: &str, dir: Option<&Path>) -> Result<String> {
        let mut cmd = build_command(dir);
        cmd.arg("for-each-ref")
            .arg(format!("--format={format}"))
            .args(refs);
        run_capturing(cmd, "for-each-ref")
    }

    fn last_commit_time(&self, worktree_path: &Path) -> Option<DateTime<Utc>> {
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
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Mock git client for testing.
    ///
    /// Construct with `MockGitClient::default()` and override individual
    /// fields with struct-update syntax:
    ///
    /// ```ignore
    /// MockGitClient { create_should_fail: true, ..Default::default() }
    /// ```
    #[derive(Default)]
    #[allow(clippy::struct_excessive_bools)]
    pub struct MockGitClient {
        pub create_should_fail: bool,
        pub list_output: String,
        pub remove_worktree_should_fail: bool,
        pub remove_branch_returns: bool,
        pub branch_exists_value: bool,
        pub rev_parse_output: String,
        pub rev_parse_should_fail: bool,
        pub fetch_should_fail: bool,
        pub for_each_ref_output: String,
        pub last_commit_time_value: Option<DateTime<Utc>>,
    }

    impl GitClient for MockGitClient {
        fn create_worktree(
            &self,
            _branch: &str,
            _path: &Path,
            _start_point: Option<&str>,
            _dir: Option<&Path>,
        ) -> Result<()> {
            if self.create_should_fail {
                anyhow::bail!("Mock git create worktree failure");
            }
            Ok(())
        }

        fn list_worktrees(&self, _dir: Option<&Path>) -> Result<String> {
            Ok(self.list_output.clone())
        }

        fn remove_worktree(&self, _path: &Path, _dir: Option<&Path>) -> Result<()> {
            if self.remove_worktree_should_fail {
                anyhow::bail!("Mock git remove worktree failure");
            }
            Ok(())
        }

        fn remove_branch(&self, _branch: &str, _dir: Option<&Path>) -> Result<bool> {
            Ok(self.remove_branch_returns)
        }

        fn branch_exists(&self, _ref_: &str, _dir: Option<&Path>) -> Result<bool> {
            Ok(self.branch_exists_value)
        }

        fn rev_parse(&self, _args: &[&str], _dir: Option<&Path>) -> Result<String> {
            if self.rev_parse_should_fail {
                anyhow::bail!("Mock git rev-parse failure");
            }
            Ok(self.rev_parse_output.clone())
        }

        fn fetch(&self, _args: &[&str], _dir: Option<&Path>) -> Result<()> {
            if self.fetch_should_fail {
                anyhow::bail!("Mock git fetch failure");
            }
            Ok(())
        }

        fn for_each_ref(
            &self,
            _refs: &[&str],
            _format: &str,
            _dir: Option<&Path>,
        ) -> Result<String> {
            Ok(self.for_each_ref_output.clone())
        }

        fn last_commit_time(&self, _worktree_path: &Path) -> Option<DateTime<Utc>> {
            self.last_commit_time_value
        }
    }

    #[test]
    fn test_mock_git_client_create_worktree_success() {
        let client = MockGitClient::default();
        let path = PathBuf::from("/test/worktree");
        let result = client.create_worktree("feature", &path, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_git_client_create_worktree_with_start_point() {
        let client = MockGitClient::default();
        let path = PathBuf::from("/test/worktree");
        let result = client.create_worktree("feature", &path, Some("main"), Some(Path::new(".")));
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_git_client_list_worktrees() {
        let expected_output = "worktree /path/to/worktree\nbranch refs/heads/main\n";
        let client = MockGitClient {
            list_output: expected_output.to_string(),
            ..Default::default()
        };
        let result = client.list_worktrees(None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_output);
    }

    #[test]
    fn test_mock_git_client_remove_worktree_success() {
        let client = MockGitClient::default();
        let path = PathBuf::from("/test/worktree");
        let result = client.remove_worktree(&path, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_git_client_remove_branch_success() {
        let client = MockGitClient {
            remove_branch_returns: true,
            ..Default::default()
        };
        let result = client.remove_branch("feature", None);
        assert!(result.unwrap());
    }

    #[test]
    fn test_mock_git_client_last_commit_time_returns_canned_value() {
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let client = MockGitClient {
            last_commit_time_value: Some(now),
            ..Default::default()
        };
        let result = client.last_commit_time(Path::new("/any"));
        assert_eq!(result, Some(now));
    }

    #[test]
    fn test_real_git_client_last_commit_time_current_repo() {
        // Run RealGitClient against the current process working dir, which
        // during `cargo test` is the project root — a git repository with
        // commits.
        let client = RealGitClient;
        let current_dir = std::env::current_dir().unwrap();
        let result = client.last_commit_time(&current_dir);
        assert!(result.is_some(), "Current repository should have commits");
    }

    #[test]
    fn test_real_git_client_last_commit_time_nonexistent_path() {
        let client = RealGitClient;
        let nonexistent = std::path::PathBuf::from("/nonexistent/path/to/worktree");
        let result = client.last_commit_time(&nonexistent);
        assert!(result.is_none(), "Non-existent path should return None");
    }
}
