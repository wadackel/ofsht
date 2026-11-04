use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::path_utils::canonicalize_allow_missing;

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub pane: String,
    pub window: String,
    pub grouped: bool,
    pub ambiguous: bool,
    server: String,
}

#[derive(Clone, Debug)]
pub struct Pane {
    pub id: String,
    pub window: String,
    pub count: usize,
    pub path: Option<PathBuf>,
}

impl Pane {
    pub fn path_after_removal(&self) -> Option<PathBuf> {
        let path = self.path.as_ref()?;
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::ffi::OsStrExt;
            // Normalizing this suffix before removal could match a different live worktree.
            if !path.exists() {
                if let Some(bytes) = path.as_os_str().as_bytes().strip_suffix(b" (deleted)") {
                    return Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)));
                }
            }
        }
        Some(path.clone())
    }
}

pub(super) fn command() -> Result<Command> {
    let context = std::env::var_os("TMUX").context("Missing tmux context")?;
    let fields: Vec<_> = context
        .as_encoded_bytes()
        .rsplitn(3, |b| *b == b',')
        .collect();
    if fields.len() != 3 || fields[2].is_empty() {
        bail!("Invalid TMUX context");
    }
    let mut command = Command::new("tmux");
    command.arg("-S");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        command.arg(std::ffi::OsStr::from_bytes(fields[2]));
    }
    #[cfg(not(unix))]
    command.arg(std::str::from_utf8(fields[2]).context("Invalid tmux socket path")?);
    // The calling directory may have just been removed; all directory arguments are absolute.
    #[cfg(unix)]
    command.current_dir("/");
    Ok(command)
}

pub(super) fn output(mut command: Command) -> Result<Vec<u8>> {
    let output = command.output().context("Failed to execute tmux")?;
    if !output.status.success() {
        bail!(
            "tmux command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

pub(super) fn text(args: &[&str]) -> Result<String> {
    let mut cmd = command()?;
    cmd.args(args);
    Ok(String::from_utf8(output(cmd)?)
        .context("Invalid tmux identifier output")?
        .trim()
        .to_string())
}

fn valid_id(value: &str, prefix: char) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

impl Session {
    pub fn current() -> Result<Self> {
        let tmux = std::env::var_os("TMUX").context("tmux integration requires running inside a tmux session; try again from a tmux pane or use --no-tmux to disable")?;
        let fields: Vec<_> = tmux.as_encoded_bytes().rsplitn(3, |b| *b == b',').collect();
        if fields.len() != 3 || fields[2].is_empty() {
            bail!("Invalid TMUX context");
        }
        let server = std::str::from_utf8(fields[1])
            .context("Invalid TMUX server identity")?
            .to_string();
        if server.is_empty() || !server.bytes().all(|b| b.is_ascii_digit()) {
            bail!("Invalid TMUX server identity");
        }
        let supplied_pane = std::env::var("TMUX_PANE").ok();
        let pane = match &supplied_pane {
            Some(pane) => pane.clone(),
            None => text(&["display-message", "-p", "#{pane_id}"])?,
        };
        if !valid_id(&pane, '%') {
            bail!("Invalid tmux pane identity");
        }
        let info = text(&["display-message", "-p", "-t", &pane, "#{pid}\t#{window_id}"])?;
        let (actual_server, window) = info
            .split_once('\t')
            .context("Invalid tmux server/window identity")?;
        if actual_server != server || !valid_id(window, '@') {
            bail!("tmux server or caller window changed");
        }
        let memberships = text(&["list-windows", "-a", "-F", "#{session_id}\t#{window_id}"])?;
        let mut sessions = Vec::new();
        for row in memberships.lines() {
            let (session, candidate) = row
                .split_once('\t')
                .context("Invalid tmux membership output")?;
            if !valid_id(session, '$') || !valid_id(candidate, '@') {
                bail!("Invalid tmux membership identity");
            }
            if candidate == window {
                sessions.push(session.to_string());
            }
        }
        sessions.sort();
        sessions.dedup();
        let id = if sessions.len() == 1 {
            sessions[0].clone()
        } else {
            let resolved = text(&["display-message", "-p", "#{session_id}"])?;
            if !sessions.contains(&resolved) {
                bail!("Cannot resolve tmux caller session");
            }
            resolved
        };
        let grouped =
            match text(&["display-message", "-p", "-t", &id, "#{session_grouped}"])?.as_str() {
                "0" => false,
                "1" => true,
                _ => bail!("Cannot determine tmux session group status"),
            };
        Ok(Self {
            id,
            pane,
            window: window.to_string(),
            grouped,
            ambiguous: supplied_pane.is_none() || sessions.len() != 1,
            server,
        })
    }

    pub fn panes(&self) -> Result<Vec<Pane>> {
        let rows = text(&[
            "list-panes",
            "-s",
            "-t",
            &self.id,
            "-F",
            "#{pane_id}\t#{window_id}\t#{window_panes}",
        ])?;
        rows.lines()
            .map(|row| {
                let fields: Vec<_> = row.split('\t').collect();
                if fields.len() != 3 || !valid_id(fields[0], '%') || !valid_id(fields[1], '@') {
                    bail!("Invalid tmux pane record");
                }
                let count: usize = fields[2].parse().context("Invalid tmux pane count")?;
                if count == 0 {
                    bail!("Invalid empty tmux window");
                }
                let mut cmd = command()?;
                cmd.args([
                    "display-message",
                    "-p",
                    "-t",
                    fields[0],
                    "#{pane_current_path}",
                ]);
                Ok(Pane {
                    id: fields[0].to_string(),
                    window: fields[1].to_string(),
                    count,
                    path: path_from_output(&output(cmd)?)?,
                })
            })
            .collect()
    }

    pub fn validate_cleanup_scope(&self) -> Result<()> {
        let now = Self::current()?;
        if now.grouped {
            bail!("tmux session is grouped");
        }
        if now.ambiguous {
            bail!("caller window is shared or its session is unknown");
        }
        if self.server != now.server
            || self.id != now.id
            || self.pane != now.pane
            || self.window != now.window
        {
            bail!("tmux caller context changed");
        }
        Ok(())
    }
}

pub fn owning_worktree<'a>(path: &Path, roots: &'a [PathBuf]) -> Option<&'a PathBuf> {
    let path = canonicalize_allow_missing(path);
    roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
}

pub(super) fn directory_arg(path: &Path) -> Result<OsString> {
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        bail!("Invalid NUL in tmux directory");
    }
    let path = path
        .canonicalize()
        .context("Failed to resolve tmux directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let mut escaped = Vec::new();
        for &byte in path.as_os_str().as_bytes() {
            if byte == b'#' {
                escaped.push(b'#');
            }
            escaped.push(byte);
        }
        Ok(OsString::from_vec(escaped))
    }
    #[cfg(not(unix))]
    {
        Ok(path
            .to_str()
            .context("tmux directory is not UTF-8")?
            .replace('#', "##")
            .into())
    }
}

fn path_from_output(output: &[u8]) -> Result<Option<PathBuf>> {
    let bytes = output.strip_suffix(b"\n").unwrap_or(output);
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.contains(&0) {
        bail!("Invalid NUL in tmux path");
    }
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    };
    #[cfg(not(unix))]
    let path = PathBuf::from(std::str::from_utf8(bytes).context("tmux path is not UTF-8")?);
    Ok(path.is_absolute().then_some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deepest_root_wins_over_containing_main() {
        let root = std::env::temp_dir();
        let roots = vec![
            canonicalize_allow_missing(&root),
            canonicalize_allow_missing(&root.join("pool/slot")),
        ];
        assert_eq!(
            owning_worktree(&root.join("pool/slot/sub"), &roots),
            Some(&roots[1])
        );
        assert_eq!(
            owning_worktree(&root.join("pool/slot-other"), &roots),
            Some(&roots[0])
        );
    }
    #[cfg(unix)]
    #[test]
    fn native_paths_keep_whitespace_and_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let path = path_from_output(b"/tmp/pane\xff \t\n\n").unwrap().unwrap();
        assert_eq!(path.as_os_str().as_bytes(), b"/tmp/pane\xff \t\n");
        assert!(path_from_output(b"\n").unwrap().is_none());
        assert!(path_from_output(b"/tmp/\0\n").is_err());
    }
}
