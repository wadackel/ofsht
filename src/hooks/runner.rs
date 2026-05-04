#![allow(clippy::missing_errors_doc)]
use anyhow::{Context, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::output::{emit_line, format_duration};
use crate::color;

/// Number of trailing output lines to keep for failure diagnostics
const FAILURE_TAIL_LINES: usize = 10;

pub(super) fn execute_command(
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
