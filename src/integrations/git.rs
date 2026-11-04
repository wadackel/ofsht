#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use std::process::Command;

pub mod worktree_state;

/// What a name means among remote-tracking refs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteTracking {
    /// A remote-tracking branch; holds the branch name without the remote
    Branch(String),
    /// A symbolic ref such as `origin/HEAD`
    Symbolic,
}

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

    /// Run `git worktree add --detach <path> [<start_point>]`.
    fn create_detached_worktree(
        &self,
        path: &Path,
        start_point: Option<&str>,
        dir: Option<&Path>,
    ) -> Result<()>;

    /// Run `git worktree list --porcelain`.
    fn list_worktrees(&self, dir: Option<&Path>) -> Result<String>;

    /// Run `git worktree remove <path>`, with `--force` when `force`.
    fn remove_worktree(&self, path: &Path, force: bool, dir: Option<&Path>) -> Result<()>;

    /// Run `git branch -d <branch>`, or `git branch -D <branch>` when `force`.
    ///
    /// Returns `Ok(true)` on success, `Ok(false)` when git exits non-zero
    /// (e.g. an unmerged branch without `force`; callers treat it as a warning),
    /// and `Err` only when the git process cannot be spawned.
    fn remove_branch(&self, branch: &str, force: bool, dir: Option<&Path>) -> Result<bool>;

    /// Branches treated as the repository's default: the target of `origin/HEAD`,
    /// `init.defaultBranch`, `main` and `master`, without duplicates.
    fn default_branches(&self, dir: Option<&Path>) -> Vec<String>;

    /// Whether `refs/remotes/<name>` exists, and what it is.
    fn remote_tracking(&self, name: &str, dir: Option<&Path>) -> Option<RemoteTracking>;

    /// Whether the local branch `refs/heads/<name>` exists.
    ///
    /// Returns `Ok(true)` when the ref exists, `Ok(false)` when git exits
    /// non-zero (ref does not exist), and `Err` only when the git process
    /// cannot be spawned.
    fn branch_exists(&self, name: &str, dir: Option<&Path>) -> Result<bool>;

    /// Run `git <args>` (caller supplies the full argument list including
    /// `rev-parse`) and return stdout on success.
    fn rev_parse(&self, args: &[&str], dir: Option<&Path>) -> Result<String>;

    /// Run `git <args>` (caller supplies the full argument list including
    /// `fetch`).
    fn fetch(&self, args: &[&str], dir: Option<&Path>) -> Result<()>;

    /// Run `git <args>` and return stdout; the error names the subcommand and git's stderr.
    fn run(&self, args: &[&str], dir: Option<&Path>) -> Result<String>;

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

impl RealGitClient {
    pub fn worktree_listing_z(dir: &Path) -> Result<String> {
        let mut cmd = build_command(Some(dir));
        cmd.args(["worktree", "list", "--porcelain", "-z"]);
        String::from_utf8(run_capturing_bytes(cmd, "worktree list")?)
            .context("JSON worktree listing requires UTF-8 paths and metadata")
    }

    /// Whether a literal path is tracked, including staged deletions from HEAD.
    pub fn path_is_tracked(dir: &Path, path: &Path) -> Result<bool> {
        let mut cmd = build_command(Some(dir));
        let output = cmd
            .args(["--literal-pathspecs", "ls-files", "--error-unmatch", "--"])
            .arg(path)
            .output()
            .context("Failed to query tracked paths")?;
        match output.status.code() {
            Some(0) => return Ok(true),
            Some(1) => {}
            _ => anyhow::bail!(
                "git ls-files failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        }
        // Index-only queries miss staged deletions; cached diff also works with an unborn HEAD.
        let mut cmd = build_command(Some(dir));
        cmd.args([
            "--literal-pathspecs",
            "diff",
            "--cached",
            "--no-renames",
            "--diff-filter=D",
            "--name-only",
            "-z",
            "--",
        ])
        .arg(path);
        Ok(!run_capturing_bytes(cmd, "diff --cached")?.is_empty())
    }

    /// Registered worktree paths without text decoding or porcelain quoting.
    pub fn worktree_paths(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut cmd = build_command(Some(dir));
        cmd.args(["worktree", "list", "--porcelain", "-z"]);
        run_capturing_bytes(cmd, "worktree list")?
            .split(|byte| *byte == 0)
            .filter_map(|field| field.strip_prefix(b"worktree "))
            .map(path_from_git)
            .collect()
    }

    /// Git's common directory, relative to `dir` or absolute as reported by Git.
    pub fn common_git_dir(dir: &Path) -> Result<PathBuf> {
        git_path(dir, "--git-common-dir")
    }

    pub fn worktree_toplevel(dir: &Path) -> Result<PathBuf> {
        git_path(dir, "--show-toplevel")
    }
}

fn git_path(path: &Path, flag: &str) -> Result<PathBuf> {
    let mut cmd = build_command(Some(path));
    cmd.args(["rev-parse", flag]);
    let output = run_capturing_bytes(cmd, "rev-parse")?;
    path_from_git(output.strip_suffix(b"\n").unwrap_or(&output))
}

fn path_from_git(bytes: &[u8]) -> Result<PathBuf> {
    if bytes.contains(&0) {
        anyhow::bail!("Git returned a path containing NUL");
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }
    #[cfg(not(unix))]
    {
        Ok(PathBuf::from(
            std::str::from_utf8(bytes).context("Git returned a non-UTF-8 path")?,
        ))
    }
}

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
fn run_capturing(cmd: Command, op: &str) -> Result<String> {
    Ok(String::from_utf8_lossy(&run_capturing_bytes(cmd, op)?).to_string())
}

fn run_capturing_bytes(mut cmd: Command, op: &str) -> Result<Vec<u8>> {
    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute git {op}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {op} failed: {stderr}");
    }
    Ok(output.stdout)
}

/// Whether the user left `branch.autoSetupMerge` unset (`git config --get` exits 1)
///
/// Only then does `create_worktree` pass `--no-track` for a remote start point with another
/// name, matching `branch.autoSetupMerge=simple`. Setting `simple` with `-c` would be shorter,
/// but git before 2.37 rejects the value and fails the whole `worktree add`.
fn auto_setup_merge_unset(dir: Option<&Path>) -> bool {
    let mut cmd = build_command(dir);
    cmd.args(["config", "--get", "branch.autoSetupMerge"]);
    cmd.output().is_ok_and(|out| out.status.code() == Some(1))
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
            if auto_setup_merge_unset(dir)
                && matches!(
                    self.remote_tracking(start, dir),
                    Some(RemoteTracking::Branch(name)) if name != branch
                )
            {
                cmd.arg("--no-track");
            }
            cmd.arg("-b").arg(branch).arg(path).arg(start);
        } else if self.branch_exists(branch, dir)? {
            cmd.arg(path).arg(branch);
        } else {
            cmd.arg("-b").arg(branch).arg(path);
        }

        run_capturing(cmd, "worktree add")?;
        Ok(())
    }

    fn create_detached_worktree(
        &self,
        path: &Path,
        start_point: Option<&str>,
        dir: Option<&Path>,
    ) -> Result<()> {
        let mut cmd = build_command(dir);
        cmd.args(["worktree", "add", "--detach"]).arg(path);
        if let Some(start) = start_point {
            cmd.arg(start);
        }
        run_capturing(cmd, "worktree add")?;
        Ok(())
    }

    fn list_worktrees(&self, dir: Option<&Path>) -> Result<String> {
        let mut cmd = build_command(dir);
        cmd.args(["worktree", "list", "--porcelain"]);
        run_capturing(cmd, "worktree list")
    }

    fn remove_worktree(&self, path: &Path, force: bool, dir: Option<&Path>) -> Result<()> {
        let mut cmd = build_command(dir);
        cmd.arg("worktree").arg("remove");
        if force {
            cmd.arg("--force");
        }
        cmd.arg(path);
        run_capturing(cmd, "worktree remove")?;
        Ok(())
    }

    fn remove_branch(&self, branch: &str, force: bool, dir: Option<&Path>) -> Result<bool> {
        let flag = if force { "-D" } else { "-d" };
        let mut cmd = build_command(dir);
        let output = cmd
            .args(["branch", flag, "--", branch])
            .output()
            .with_context(|| format!("Failed to execute git branch {flag}"))?;

        Ok(output.status.success())
    }

    fn default_branches(&self, dir: Option<&Path>) -> Vec<String> {
        let read = |args: &[&str]| {
            let mut cmd = build_command(dir);
            cmd.args(args);
            run_capturing(cmd, "default branch lookup")
                .ok()
                .map(|out| out.trim().to_string())
                .filter(|out| !out.is_empty())
        };
        // Any one source can be missing or stale (origin/HEAD exists only after a clone)
        // The full ref, because --short turns ambiguous names into remotes/origin/<x>
        let origin_head = read(&["symbolic-ref", "refs/remotes/origin/HEAD"]).and_then(|name| {
            name.strip_prefix("refs/remotes/origin/")
                .map(str::to_string)
        });
        let configured = read(&["config", "init.defaultBranch"]);
        let mut branches: Vec<String> = Vec::new();
        for name in origin_head
            .into_iter()
            .chain(configured)
            .chain(["main".to_string(), "master".to_string()])
        {
            if !branches.contains(&name) {
                branches.push(name);
            }
        }
        branches
    }

    fn remote_tracking(&self, name: &str, dir: Option<&Path>) -> Option<RemoteTracking> {
        let full = format!("refs/remotes/{name}");
        let succeeds = |args: &[&str]| {
            let mut cmd = build_command(dir);
            cmd.args(args)
                .output()
                .is_ok_and(|out| out.status.success())
        };
        // show-ref takes the exact ref name; rev-parse would also accept suffixes like `~1`
        if !succeeds(&["show-ref", "--verify", "--quiet", &full]) {
            return None;
        }
        if succeeds(&["symbolic-ref", "-q", &full]) {
            return Some(RemoteTracking::Symbolic);
        }
        // The longest remote name that prefixes it, since a remote name may contain '/'
        let mut cmd = build_command(dir);
        cmd.arg("remote");
        let remotes = run_capturing(cmd, "remote").unwrap_or_default();
        let branch = remotes
            .lines()
            .filter_map(|remote| name.strip_prefix(remote)?.strip_prefix('/'))
            .min_by_key(|branch| branch.len())?;
        Some(RemoteTracking::Branch(branch.to_string()))
    }

    fn branch_exists(&self, name: &str, dir: Option<&Path>) -> Result<bool> {
        let mut cmd = build_command(dir);
        let output = cmd
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{name}"),
            ])
            .output()
            .context("Failed to execute git show-ref --verify")?;
        Ok(output.status.success())
    }

    fn rev_parse(&self, args: &[&str], dir: Option<&Path>) -> Result<String> {
        let mut cmd = build_command(dir);
        cmd.args(args);
        run_capturing(cmd, "rev-parse")
    }

    fn run(&self, args: &[&str], dir: Option<&Path>) -> Result<String> {
        let mut cmd = build_command(dir);
        cmd.args(args);
        run_capturing(cmd, args.first().copied().unwrap_or_default())
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

        fn create_detached_worktree(
            &self,
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

        fn remove_worktree(&self, _path: &Path, _force: bool, _dir: Option<&Path>) -> Result<()> {
            if self.remove_worktree_should_fail {
                anyhow::bail!("Mock git remove worktree failure");
            }
            Ok(())
        }

        fn remove_branch(&self, _branch: &str, _force: bool, _dir: Option<&Path>) -> Result<bool> {
            Ok(self.remove_branch_returns)
        }

        fn default_branches(&self, _dir: Option<&Path>) -> Vec<String> {
            vec!["main".to_string(), "master".to_string()]
        }

        fn remote_tracking(&self, _name: &str, _dir: Option<&Path>) -> Option<RemoteTracking> {
            None
        }

        fn branch_exists(&self, _name: &str, _dir: Option<&Path>) -> Result<bool> {
            Ok(self.branch_exists_value)
        }

        fn rev_parse(&self, _args: &[&str], _dir: Option<&Path>) -> Result<String> {
            if self.rev_parse_should_fail {
                anyhow::bail!("Mock git rev-parse failure");
            }
            Ok(self.rev_parse_output.clone())
        }

        fn run(&self, _args: &[&str], _dir: Option<&Path>) -> Result<String> {
            Ok(String::new())
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
        let result = client.remove_worktree(&path, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_git_client_remove_branch_success() {
        let client = MockGitClient {
            remove_branch_returns: true,
            ..Default::default()
        };
        let result = client.remove_branch("feature", false, None);
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

    #[cfg(unix)]
    #[test]
    fn git_paths_preserve_non_utf8_and_trailing_whitespace() {
        use std::os::unix::ffi::OsStrExt;
        let bytes = b"/repo/metadata\xff \t\n";
        let path = path_from_git(bytes).unwrap();
        assert_eq!(path.as_os_str().as_bytes(), bytes);
    }

    #[test]
    fn git_paths_reject_nul() {
        assert!(path_from_git(b"/repo\0extra").is_err());
    }
}
