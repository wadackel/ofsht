#![allow(clippy::missing_errors_doc)]
use anyhow::{Context, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::output::{emit_line, format_duration};
use super::HookContext;
use crate::color;

/// Number of trailing output lines to keep for failure diagnostics
const FAILURE_TAIL_LINES: usize = 10;

pub(super) fn execute_command(
    cmd: &str,
    context: &HookContext,
    color_mode: color::ColorMode,
    _is_last: bool,
    indent: &str,
    mp: &MultiProgress,
) -> Result<()> {
    let start = Instant::now();

    // A trailing redirection covers only the last command in a compound script.
    let merged_cmd = format!("exec 2>&1\n{cmd}");
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&merged_cmd)
        .current_dir(&context.worktree_path)
        .env("OFSHT_WORKTREE_PATH", &context.worktree_path)
        .env("OFSHT_WORKTREE_NAME", &context.worktree_name)
        .env("OFSHT_BRANCH", &context.branch)
        .env("OFSHT_REPO_ROOT", &context.repo_root)
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

    let preview_clone = preview_bar.clone();
    let output_progress = mp.clone();
    let verbose = context.verbose;
    let reader_handle = std::thread::spawn(move || {
        read_output(
            child_stdout,
            verbose,
            &output_progress,
            preview_clone.as_ref(),
        )
    });

    let status = child
        .wait()
        .with_context(|| format!("Failed to wait for command: {cmd}"))?;
    let elapsed = start.elapsed();

    let tail = reader_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Hook output reader panicked"))??;

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
        if !verbose {
            for line in &tail {
                emit_line(
                    mp,
                    is_tty,
                    format!("{indent}  {}", color::dim(color_mode, line)),
                );
            }
        }
        anyhow::bail!("Hook command failed: {cmd}");
    }

    Ok(())
}

fn read_output(
    mut stdout: std::process::ChildStdout,
    verbose: bool,
    mp: &MultiProgress,
    preview: Option<&ProgressBar>,
) -> Result<VecDeque<String>> {
    let mut tail = VecDeque::<String>::with_capacity(FAILURE_TAIL_LINES);
    let mut pending = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = stdout
            .read(&mut buffer)
            .context("Failed to read hook output")?;
        if count == 0 {
            break;
        }
        let bytes = &buffer[..count];
        if verbose {
            mp.suspend(|| std::io::stderr().lock().write_all(bytes))
                .context("Failed to write verbose hook output")?;
        }
        for &byte in bytes {
            if byte == b'\n' {
                retain_line(&mut tail, &pending, preview);
                pending.clear();
            } else {
                pending.push(byte);
            }
        }
        if preview.is_some() && !pending.is_empty() {
            let preview_bytes = &pending[..pending.len().min(61 * 4)];
            update_preview(preview, &String::from_utf8_lossy(preview_bytes));
        }
    }
    if !pending.is_empty() {
        retain_line(&mut tail, &pending, preview);
    }
    Ok(tail)
}

fn retain_line(tail: &mut VecDeque<String>, bytes: &[u8], preview: Option<&ProgressBar>) {
    let line = String::from_utf8_lossy(bytes)
        .trim_end_matches('\r')
        .to_string();
    update_preview(preview, &line);
    if tail.len() >= FAILURE_TAIL_LINES {
        tail.pop_front();
    }
    tail.push_back(line);
}

fn update_preview(preview: Option<&ProgressBar>, line: &str) {
    if let Some(pb) = preview {
        let display = if line.chars().count() > 60 {
            format!("{}…", line.chars().take(59).collect::<String>())
        } else {
            line.to_string()
        };
        pb.set_message(display);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_command_success() {
        let temp_dir = std::env::temp_dir();
        let result = execute_command(
            "echo test",
            &HookContext::for_test(&temp_dir),
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
            &HookContext::for_test(&temp_dir),
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
            &HookContext::for_test(&temp_dir),
            color::ColorMode::Never,
            false,
            "  ",
            &MultiProgress::new(),
        );
        assert!(result.is_ok());
    }
}
