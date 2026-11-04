#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::domain::worktree::WorktreeList;
use crate::path_utils::display_path;

const PREVIEW_COMMAND: &str = r#"sh -c 'p=$(printf "%b/." "$1"); git -C "$p" status --short --branch; printf "\n"; git -C "$p" log --oneline -n 10' sh {2}"#;

fn encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len() * 5);
    for byte in path.bytes() {
        encoded.push_str("\\0");
        encoded.push(char::from(b'0' + (byte >> 6)));
        encoded.push(char::from(b'0' + ((byte >> 3) & 7)));
        encoded.push(char::from(b'0' + (byte & 7)));
    }
    encoded
}

fn supports_accept_nth() -> Result<bool> {
    let output = Command::new("fzf")
        .arg("--help")
        .env_remove("FZF_DEFAULT_OPTS")
        .env_remove("FZF_DEFAULT_OPTS_FILE")
        .output()
        .context("Failed to inspect fzf options")?;
    if !output.status.success() {
        anyhow::bail!(
            "fzf --help failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).contains("--accept-nth"))
}

/// Item to display in fzf
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FzfItem {
    /// Text to display in fzf
    pub display: String,
    /// Actual value to return when selected
    pub value: String,
}

/// Fzf picker interface
pub trait FzfPicker {
    fn pick(&self, items: &[FzfItem], multi: bool) -> Result<Vec<String>>;
}

/// Real fzf implementation
#[derive(Debug)]
pub struct RealFzfPicker {
    extra_options: Vec<String>,
}

impl RealFzfPicker {
    pub const fn new(extra_options: Vec<String>) -> Self {
        Self { extra_options }
    }
}

impl FzfPicker for RealFzfPicker {
    fn pick(&self, items: &[FzfItem], multi: bool) -> Result<Vec<String>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }

        // Display text can contain whitespace and duplicate labels; it cannot identify a selection.
        let records = items
            .iter()
            .enumerate()
            .map(|(index, item)| format!("{index}\t{}\t{}", encode_path(&item.value), item.display))
            .collect::<Vec<_>>();
        let input = records.join("\0") + "\0";

        // Build fzf command
        let mut cmd = Command::new("fzf");

        // Add multi-select if requested
        if multi {
            cmd.arg("--multi");
        }

        // Add extra options from config
        for opt in &self.extra_options {
            cmd.arg(opt);
        }

        // Raw paths would split metadata fields; printf decodes the quoted octal argument as data.
        cmd.args([
            "--read0",
            "--print0",
            "--no-print-query",
            "--delimiter",
            "\t",
            "--with-nth",
            "3..",
            "--nth",
            "1..",
        ]);
        if supports_accept_nth()? {
            cmd.arg("--accept-nth=1..");
        }
        cmd.arg("--preview").arg(PREVIEW_COMMAND);

        // Add some default options for better UX
        cmd.arg("--height=50%")
            .arg("--reverse")
            .arg("--border")
            .arg("--prompt=Select worktree: ");

        // Execute fzf with stdin
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()); // fzf draws TUI directly to terminal

        let mut child = cmd.spawn().context("Failed to spawn fzf")?;

        // Write input to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .context("Failed to write to fzf stdin")?;
            stdin.flush().context("Failed to flush fzf stdin")?;
            // stdin is dropped here and EOF is sent
        }

        let output = child.wait_with_output().context("Failed to wait for fzf")?;

        // Handle exit codes
        match output.status.code() {
            Some(0) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                Ok(stdout
                    .split('\0')
                    .filter_map(|record| {
                        let (index, _) = record.split_once('\t')?;
                        let index: usize = index.parse().ok()?;
                        (records.get(index)?.as_str() == record).then(|| items[index].value.clone())
                    })
                    .collect())
            }
            Some(130 | 1) => {
                // User pressed Esc or no selection - not an error
                Ok(Vec::new())
            }
            Some(code) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("fzf exited with code {code}: {stderr}")
            }
            None => {
                anyhow::bail!("fzf was terminated by signal")
            }
        }
    }
}

/// Check if fzf is available in the system
pub fn is_fzf_available() -> bool {
    Command::new("fzf")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Build worktree items from git worktree list --porcelain output
///
/// Display format: `{name} · {branch} · {path}`
/// - Index 0 is the main worktree (displayed as `@`)
/// - Non-main worktrees show the name that `ofsht cd` accepts for them
/// - Columns are padded for alignment (except the last column)
pub fn build_worktree_items(porcelain_output: &str, root: Option<&Path>) -> Vec<FzfItem> {
    let mut list = WorktreeList::parse(porcelain_output, None);
    if let Some(repo) = list.main().map(|entry| PathBuf::from(&entry.path)) {
        crate::integrations::git::worktree_state::inspect(&mut list, &repo, true);
    }
    format_worktree_items(&list, root)
}

fn format_worktree_items(list: &WorktreeList, root: Option<&Path>) -> Vec<FzfItem> {
    let entries = list.entries();

    if entries.is_empty() {
        return Vec::new();
    }

    // Build name, branch, display_path for each entry
    let display_entries: Vec<(String, String, String)> = entries
        .iter()
        .zip(list.display_names(root))
        .map(|(entry, name)| {
            let mut branch = entry.branch_label();
            let markers = entry.state.markers();
            if !markers.is_empty() {
                branch.push(' ');
                branch.push_str(&markers);
            }

            let path = display_path(&PathBuf::from(&entry.path));

            (name, branch, path)
        })
        .collect();

    // Pass 2: Calculate column widths and format display strings
    let max_name_width = display_entries
        .iter()
        .map(|(n, _, _)| n.len())
        .max()
        .unwrap_or(0);
    let max_branch_width = display_entries
        .iter()
        .map(|(_, b, _)| b.len())
        .max()
        .unwrap_or(0);

    display_entries
        .into_iter()
        .zip(entries.iter())
        .map(|((name, branch, path), entry)| {
            let name_padding = " ".repeat(max_name_width.saturating_sub(name.len()));
            let branch_padding = " ".repeat(max_branch_width.saturating_sub(branch.len()));

            // Last column (path) has no padding to avoid trailing whitespace
            let display = format!("{name}{name_padding} · {branch}{branch_padding} · {path}");

            FzfItem {
                display,
                value: entry.path.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format_items(porcelain: &str, root: Option<&Path>) -> Vec<FzfItem> {
        format_worktree_items(&WorktreeList::parse(porcelain, None), root)
    }

    #[test]
    #[serial_test::serial]
    fn real_picker_preserves_duplicate_displays_and_unusual_values() {
        temp_env::with_vars(
            [
                ("FZF_DEFAULT_OPTS", None::<&str>),
                ("FZF_DEFAULT_OPTS_FILE", None),
            ],
            || {
                let items = vec![
                    FzfItem {
                        display: "same display".to_string(),
                        value: "/tmp/space path".to_string(),
                    },
                    FzfItem {
                        display: "same display".to_string(),
                        value: "/tmp/日本語\ttab\n".to_string(),
                    },
                ];
                let picker = RealFzfPicker::new(vec!["--filter=same".to_string()]);
                assert_eq!(
                    picker.pick(&items, true).unwrap(),
                    items
                        .iter()
                        .map(|item| item.value.clone())
                        .collect::<Vec<_>>()
                );
                let picker = RealFzfPicker::new(vec!["--filter=no-match".to_string()]);
                assert!(picker.pick(&items, false).unwrap().is_empty());
            },
        );
    }

    #[test]
    #[serial_test::serial]
    fn real_picker_owns_protocol_despite_config_and_environment_options() {
        let options_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            options_file.path(),
            "--print-query --read0 --print0 --with-nth=1\n",
        )
        .unwrap();
        temp_env::with_vars(
            [
                (
                    "FZF_DEFAULT_OPTS",
                    Some("--delimiter=: --with-nth=1 --nth=2.. --print-query"),
                ),
                ("FZF_DEFAULT_OPTS_FILE", options_file.path().to_str()),
            ],
            || {
                let items = [FzfItem {
                    display: "chosen display".to_string(),
                    value: "/tmp/value".to_string(),
                }];
                let mut options = vec![
                    "--filter=chosen".to_string(),
                    "--with-nth=1".to_string(),
                    "--print-query".to_string(),
                ];
                let help = Command::new("fzf")
                    .arg("--help")
                    .env_remove("FZF_DEFAULT_OPTS")
                    .env_remove("FZF_DEFAULT_OPTS_FILE")
                    .output()
                    .unwrap();
                if String::from_utf8_lossy(&help.stdout).contains("--accept-nth") {
                    options.push("--accept-nth=3..".to_string());
                }
                let picker = RealFzfPicker::new(options);
                assert_eq!(picker.pick(&items, false).unwrap(), vec!["/tmp/value"]);
            },
        );
    }

    #[test]
    fn preview_preserves_path_data_and_shows_status_before_history() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["space path", "日本語 'quote' $(touch CODE_EXECUTED)\ttab\n"] {
            let repo = temp.path().join(name);
            std::fs::create_dir(&repo).unwrap();
            for args in [
                vec!["init", "-qb", "main"],
                vec!["commit", "--allow-empty", "-qm", "history-marker"],
            ] {
                let out = Command::new("git")
                    .args(args)
                    .current_dir(&repo)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_AUTHOR_NAME", "t")
                    .env("GIT_AUTHOR_EMAIL", "t@t")
                    .env("GIT_COMMITTER_NAME", "t")
                    .env("GIT_COMMITTER_EMAIL", "t@t")
                    .output()
                    .unwrap();
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            std::fs::write(repo.join("dirty-marker"), "dirty").unwrap();
            let path = repo.to_str().unwrap();
            let encoded = encode_path(path);
            let display = format!("slot · [main] · {path}");
            let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
            let command = PREVIEW_COMMAND
                .replace("{}", &quote(&display))
                .replace("{2}", &quote(&encoded));
            let out = Command::new("sh")
                .args(["-c", &command])
                .current_dir(temp.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(stdout.contains("dirty-marker"), "{stdout}");
            assert!(stdout.contains("history-marker"), "{stdout}");
            assert!(stdout.find("dirty-marker").unwrap() < stdout.find("history-marker").unwrap());
            assert!(!temp.path().join("CODE_EXECUTED").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn old_picker_without_accept_nth_and_cancel_are_supported() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let stub = temp.path().join("fzf");
        std::fs::write(&stub, "#!/bin/sh\n[ \"$1\" = --help ] && echo '--read0 --print0' && exit 0\ncancel=0\nfor arg do\n [ \"$arg\" = --accept-nth=1.. ] && exit 2\n [ \"$arg\" = --cancel ] && cancel=1\ndone\nif [ \"$cancel\" = 1 ]; then cat >/dev/null; exit 130; fi\ncat\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!(
            "{}:{}",
            temp.path().display(),
            std::env::var("PATH").unwrap()
        );
        temp_env::with_vars(
            [
                ("PATH", Some(path.as_str())),
                ("FZF_DEFAULT_OPTS", None),
                ("FZF_DEFAULT_OPTS_FILE", None),
            ],
            || {
                let items = [FzfItem {
                    display: "label".to_string(),
                    value: "/tmp/path".to_string(),
                }];
                assert_eq!(
                    RealFzfPicker::new(vec![]).pick(&items, false).unwrap(),
                    vec!["/tmp/path"]
                );
                assert!(RealFzfPicker::new(vec!["--cancel".to_string()])
                    .pick(&items, true)
                    .unwrap()
                    .is_empty());
            },
        );
    }

    struct MockFzfPicker {
        return_values: Vec<String>,
        should_fail: bool,
    }

    impl FzfPicker for MockFzfPicker {
        fn pick(&self, _items: &[FzfItem], _multi: bool) -> Result<Vec<String>> {
            if self.should_fail {
                anyhow::bail!("Mock fzf failure");
            }
            Ok(self.return_values.clone())
        }
    }

    #[test]
    fn test_fzf_item_creation() {
        let item = FzfItem {
            display: "feature · feat/new · /path/to/worktree".to_string(),
            value: "/path/to/worktree".to_string(),
        };
        assert_eq!(item.display, "feature · feat/new · /path/to/worktree");
        assert_eq!(item.value, "/path/to/worktree");
    }

    #[test]
    fn test_mock_fzf_picker_success() {
        let picker = MockFzfPicker {
            return_values: vec!["/path/to/worktree".to_string()],
            should_fail: false,
        };
        let items = vec![FzfItem {
            display: "test".to_string(),
            value: "/path/to/worktree".to_string(),
        }];
        let result = picker.pick(&items, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["/path/to/worktree"]);
    }

    #[test]
    fn test_mock_fzf_picker_failure() {
        let picker = MockFzfPicker {
            return_values: Vec::new(),
            should_fail: true,
        };
        let items = vec![FzfItem {
            display: "test".to_string(),
            value: "/test".to_string(),
        }];
        let result = picker.pick(&items, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_fzf_picker_cancel() {
        // User pressing Esc should return empty vec
        let picker = MockFzfPicker {
            return_values: Vec::new(),
            should_fail: false,
        };
        let items = vec![FzfItem {
            display: "test".to_string(),
            value: "/test".to_string(),
        }];
        let result = picker.pick(&items, false);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_is_fzf_available() {
        // This test will pass or fail depending on whether fzf is installed
        // We're just checking that the function doesn't panic
        let _ = is_fzf_available();
    }

    #[test]
    fn test_build_worktree_items_basic() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

worktree /path/to/feature
HEAD def456
branch refs/heads/feature-branch

";
        let items = format_items(porcelain, Some(Path::new("/worktrees")));
        assert_eq!(items.len(), 2);

        assert_eq!(items[0].value, "/path/to/main");
        assert!(items[0].display.starts_with('@'));
        assert!(items[0].display.contains(" · [main]"));
        assert!(items[0].display.contains(" · /path/to/main"));

        // Second item: feature branch with name and [branch]
        assert_eq!(items[1].value, "/path/to/feature");
        assert!(items[1].display.contains("feature"));
        assert!(items[1].display.contains(" · [feature-branch]"));
        assert!(items[1].display.contains(" · /path/to/feature"));
    }

    #[test]
    fn test_build_worktree_items_detached() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

worktree /path/to/detached
HEAD def456
detached

";
        let items = format_items(porcelain, Some(Path::new("/worktrees")));
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].value, "/path/to/detached");
        assert!(items[1].display.contains("detached"));
        assert!(items[1].display.contains(" · [detached]"));
    }

    #[test]
    fn test_build_worktree_items_single_main_only() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

";
        let items = format_items(porcelain, Some(Path::new("/worktrees")));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].value, "/path/to/main");
        assert!(items[0].display.starts_with('@'));
        assert!(items[0].display.contains(" · [main]"));
    }

    #[test]
    fn test_build_worktree_items_nested_branch() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

worktree /worktrees/feat/foo
HEAD def456
branch refs/heads/feat/foo

worktree /worktrees/fix/bar
HEAD ghi789
branch refs/heads/fix/bar

";
        let items = format_items(porcelain, Some(Path::new("/worktrees")));
        assert_eq!(items.len(), 3);

        // Nested worktree names should use relative path from root
        assert_eq!(items[1].value, "/worktrees/feat/foo");
        assert!(items[1].display.contains("feat/foo"));
        assert!(items[1].display.contains(" · [feat/foo]"));

        assert_eq!(items[2].value, "/worktrees/fix/bar");
        assert!(items[2].display.contains(" · [fix/bar]"));
    }

    #[test]
    fn test_build_worktree_items_padding_alignment() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

worktree /worktrees/a
HEAD def456
branch refs/heads/short

worktree /worktrees/long-name
HEAD ghi789
branch refs/heads/very-long-branch-name

";
        let items = format_items(porcelain, Some(Path::new("/worktrees")));
        assert_eq!(items.len(), 3);

        // All "·" separators should be at the same column positions
        let first_sep_positions: Vec<usize> = items
            .iter()
            .map(|item| item.display.find(" · ").unwrap())
            .collect();
        assert!(
            first_sep_positions.windows(2).all(|w| w[0] == w[1]),
            "First separator positions should be aligned: {first_sep_positions:?}"
        );

        // Find second separator positions
        let second_sep_positions: Vec<usize> = items
            .iter()
            .map(|item| {
                let after_first = first_sep_positions[0] + 3;
                after_first + item.display[after_first..].find(" · ").unwrap()
            })
            .collect();
        assert!(
            second_sep_positions.windows(2).all(|w| w[0] == w[1]),
            "Second separator positions should be aligned: {second_sep_positions:?}"
        );
    }

    #[test]
    fn test_build_worktree_items_no_trailing_whitespace() {
        let porcelain = r"worktree /path/to/main
HEAD abc123
branch refs/heads/main

worktree /worktrees/feature
HEAD def456
branch refs/heads/feature-branch

";
        let items = format_items(porcelain, Some(Path::new("/worktrees")));
        for item in &items {
            assert_eq!(
                item.display,
                item.display.trim_end(),
                "Display should not have trailing whitespace: {:?}",
                item.display
            );
        }
    }

    #[test]
    fn test_build_worktree_items_no_trim_behavior_equivalent() {
        // Canonical git worktree list --porcelain output: no leading/trailing whitespace.
        // After WorktreeList unification the legacy `.trim()` defense was dropped.
        // This test pins display equivalence on canonical input so any future regression
        // in the unified parser surfaces here.
        let porcelain = "worktree /path/to/main\nHEAD abc123\nbranch refs/heads/main\n\nworktree /worktrees/feature\nHEAD def456\nbranch refs/heads/feature\n\n";
        let items = format_items(porcelain, Some(Path::new("/worktrees")));
        assert_eq!(items.len(), 2);
        assert!(items[0].display.starts_with('@'), "main name marker");
        assert!(
            items[0].display.contains("[main]"),
            "main branch marker [main] present: {}",
            items[0].display
        );
        // Non-main entry — branch `[feature]`
        assert!(
            items[1].display.contains("[feature]"),
            "feature branch marker present: {}",
            items[1].display
        );
        assert_eq!(items[0].value, "/path/to/main");
        assert_eq!(items[1].value, "/worktrees/feature");
    }
}
