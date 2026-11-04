use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use super::{build_command, git_path, run_capturing_bytes, RealGitClient};
use crate::domain::worktree::{WorktreeEntry, WorktreeList};
use crate::domain::worktree_state::Operation;

pub fn inspect(list: &mut WorktreeList, repo: &Path, full_status: bool) {
    let common = common_directory(repo);
    for (index, entry) in list.entries_mut().iter_mut().enumerate() {
        let result = common
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error:#}"))
            .and_then(|common| inspect_entry(entry, common, full_status, index == 0));
        if let Err(error) = result {
            entry.state.error = Some(format!("{error:#}"));
        }
    }
}

fn common_directory(path: &Path) -> Result<PathBuf> {
    let common = RealGitClient::common_git_dir(path)?;
    path.join(common)
        .canonicalize()
        .context("Failed to resolve repository identity")
}

fn inspect_entry(
    entry: &mut WorktreeEntry,
    common: &Path,
    full_status: bool,
    main: bool,
) -> Result<()> {
    let path = Path::new(&entry.path);
    match path.metadata() {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => bail!("Registered worktree path is not a directory"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            entry.state.missing = true;
            return Ok(());
        }
        Err(error) => return Err(error).context("Failed to inspect worktree path"),
    }
    if common_directory(path)? != common {
        bail!("Registered worktree belongs to a different repository");
    }
    if git_path(path, "--show-toplevel")?.canonicalize()? != path.canonicalize()? {
        bail!("Registered path is not the repository worktree root");
    }
    let git_dir = if main {
        let admin = git_path(path, "--absolute-git-dir")?.canonicalize()?;
        if admin != common {
            bail!("Main worktree administration identity changed");
        }
        admin
    } else {
        RealGitClient::linked_worktree_git_dir(path)?
    };
    let has = |name: &str| git_dir.join(name).try_exists();
    let operation = if has("rebase-merge")? {
        Some(Operation::Rebase)
    } else if has("rebase-apply")? {
        Some(if has("rebase-apply/applying")? {
            Operation::Am
        } else {
            Operation::Rebase
        })
    } else if has("MERGE_HEAD")? {
        Some(Operation::Merge)
    } else if has("BISECT_LOG")? {
        Some(Operation::Bisect)
    } else if has("CHERRY_PICK_HEAD")? {
        Some(Operation::CherryPick)
    } else if has("REVERT_HEAD")? {
        Some(Operation::Revert)
    } else {
        None
    };
    entry.state.operation = operation;
    entry.state.operation_branch = match operation {
        Some(Operation::Rebase) => {
            let directory = if has("rebase-merge")? {
                "rebase-merge"
            } else {
                "rebase-apply"
            };
            match read_line(&git_dir.join(directory).join("head-name"))? {
                None => None,
                Some(head) if head == "detached HEAD" => None,
                Some(head) => {
                    let branch = head
                        .strip_prefix("refs/heads/")
                        .filter(|branch| !branch.is_empty() && !branch.starts_with('-'))
                        .context("Invalid rebase branch metadata")?;
                    let mut command = build_command(Some(path));
                    command.args(["check-ref-format", &head]);
                    run_capturing_bytes(command, "check-ref-format")?;
                    Some(branch.to_string())
                }
            }
        }
        Some(Operation::Bisect) => match read_line(&git_dir.join("BISECT_START"))? {
            Some(branch) if !is_object_id(&branch) && local_branch_exists(path, &branch)? => {
                Some(branch)
            }
            _ => None,
        },
        _ => None,
    };
    if full_status {
        let mut cmd = build_command(Some(path));
        cmd.args([
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=none",
        ]);
        let status = parse_status(&run_capturing_bytes(cmd, "status")?)?;
        entry.state.dirty = Some(status.dirty);
        entry.state.upstream = status.upstream;
        entry.state.ahead = status.ahead;
        entry.state.behind = status.behind;
    }
    Ok(())
}

fn read_line(path: &Path) -> Result<Option<String>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", path.display()))
        }
    };
    let line = text.strip_suffix('\n').unwrap_or(&text);
    if line.is_empty() || line.bytes().any(|b| matches!(b, b'\n' | b'\r' | 0)) {
        bail!("Invalid operation branch metadata: {}", path.display());
    }
    Ok(Some(line.to_string()))
}

fn local_branch_exists(path: &Path, branch: &str) -> Result<bool> {
    let mut cmd = build_command(Some(path));
    let output = cmd
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .output()?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!(
            "Failed to verify operation branch: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Default)]
struct Status {
    dirty: bool,
    upstream: Option<String>,
    ahead: Option<u64>,
    behind: Option<u64>,
}

fn parse_status(bytes: &[u8]) -> Result<Status> {
    if !bytes.ends_with(&[0]) {
        bail!("Malformed Git status output: missing NUL terminator");
    }
    let mut result = Status::default();
    let mut oid = false;
    let mut head = false;
    let mut fields = bytes.split(|b| *b == 0).peekable();
    while let Some(field) = fields.next() {
        if field.is_empty() && fields.peek().is_none() {
            break;
        }
        if field.starts_with(b"# ") {
            let text = std::str::from_utf8(field).context("Invalid Git status header")?;
            let (key, value) = text[2..]
                .split_once(' ')
                .unwrap_or_else(|| (&text[2..], ""));
            match key {
                "branch.oid" => {
                    if oid || (value != "(initial)" && !is_object_id(value)) {
                        bail!("Malformed Git status object ID");
                    }
                    oid = true;
                }
                "branch.head" => {
                    if head || value.is_empty() || value.bytes().any(|b| b.is_ascii_whitespace()) {
                        bail!("Malformed Git status branch");
                    }
                    head = true;
                }
                "branch.upstream" => {
                    if result.upstream.is_some()
                        || value.is_empty()
                        || value.bytes().any(|b| b.is_ascii_whitespace())
                    {
                        bail!("Malformed Git status upstream");
                    }
                    result.upstream = Some(value.to_string());
                }
                "branch.ab" => {
                    if result.ahead.is_some() {
                        bail!("Duplicate Git status divergence");
                    }
                    let (ahead, behind) = value
                        .split_once(' ')
                        .context("Malformed Git status divergence")?;
                    result.ahead = Some(
                        ahead
                            .strip_prefix('+')
                            .context("Malformed Git status ahead count")?
                            .parse()?,
                    );
                    result.behind = Some(
                        behind
                            .strip_prefix('-')
                            .context("Malformed Git status behind count")?
                            .parse()?,
                    );
                }
                _ => {}
            }
        } else {
            let fields_needed = match field.first() {
                Some(b'1') => 9,
                Some(b'2') => 10,
                Some(b'u') => 11,
                Some(b'?' | b'!') => 2,
                _ => bail!("Malformed Git status record"),
            };
            if field.get(1) != Some(&b' ')
                || field.splitn(fields_needed, |b| *b == b' ').count() != fields_needed
                || field
                    .splitn(fields_needed, |b| *b == b' ')
                    .last()
                    .is_none_or(<[u8]>::is_empty)
            {
                bail!("Malformed Git status record");
            }
            if field[0] == b'2' {
                // A rename's source filename may resemble a branch header but is still path data.
                if fields.next().is_none_or(<[u8]>::is_empty) {
                    bail!("Missing Git status rename source");
                }
            }
            if field[0] != b'!' {
                result.dirty = true;
            }
        }
    }
    if !oid || !head || (result.ahead.is_some() && result.upstream.is_none()) {
        bail!("Incomplete Git status branch headers");
    }
    Ok(result)
}
