use assert_cmd::Command;
use predicates::prelude::*;

#[test]
#[allow(deprecated)]
fn test_color_flag_always() {
    // --color=always should be accepted
    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=always", "ls"])
        .assert()
        .success();
}

#[test]
#[allow(deprecated)]
fn test_color_flag_auto() {
    // --color=auto should be accepted
    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=auto", "ls"])
        .assert()
        .success();
}

#[test]
#[allow(deprecated)]
fn test_color_flag_never() {
    // --color=never should be accepted
    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=never", "ls"])
        .assert()
        .success();
}

#[test]
#[allow(deprecated)]
fn test_color_flag_invalid() {
    // Invalid color mode should be rejected by clap
    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=invalid", "ls"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value 'invalid'"));
}

#[test]
#[allow(deprecated)]
fn test_color_flag_case_insensitive() {
    // Color mode should be case-insensitive
    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=ALWAYS", "ls"])
        .assert()
        .success();

    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=Never", "ls"])
        .assert()
        .success();
}

#[test]
#[allow(deprecated)]
fn test_no_color_env() {
    // NO_COLOR environment variable should be respected
    // This test just verifies it doesn't cause a crash
    // Actual color behavior will be tested in Phase 2
    Command::cargo_bin("ofsht")
        .unwrap()
        .env("NO_COLOR", "1")
        .arg("ls")
        .assert()
        .success();
}

#[test]
#[allow(deprecated)]
fn test_term_dumb_env() {
    // TERM=dumb should be respected
    Command::cargo_bin("ofsht")
        .unwrap()
        .env("TERM", "dumb")
        .arg("ls")
        .assert()
        .success();
}

#[test]
#[allow(deprecated)]
fn test_color_flag_overrides_no_color() {
    // CLI flag should override NO_COLOR environment variable
    Command::cargo_bin("ofsht")
        .unwrap()
        .env("NO_COLOR", "1")
        .args(["--color=always", "ls"])
        .assert()
        .success();
}

#[test]
#[allow(deprecated)]
fn test_color_flag_global() {
    // --color should work as a global flag (before subcommand)
    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=never", "ls"])
        .assert()
        .success();

    // And also after subcommand (due to global = true)
    Command::cargo_bin("ofsht")
        .unwrap()
        .args(["ls", "--color=never"])
        .assert()
        .success();
}

#[test]
#[allow(deprecated)]
fn test_ls_stdout_vs_stderr_separation() {
    // In terminal mode (TTY), output goes to stderr
    // In pipe mode (non-TTY), output goes to stdout

    // assert_cmd runs commands in pipe mode (non-TTY)
    // Stream selection is based on TTY, color mode only affects ANSI codes

    // In pipe mode with --color=always, output goes to stdout with colors
    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=always", "ls"])
        .output()
        .unwrap();

    // Pipe mode always outputs to stdout (regardless of color mode)
    assert!(
        !output.stdout.is_empty(),
        "pipe mode should output to stdout even with --color=always"
    );

    // With --color=never in pipe mode, output also goes to stdout
    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=never", "ls"])
        .output()
        .unwrap();

    // Pipe mode outputs to stdout
    assert!(
        !output.stdout.is_empty(),
        "pipe mode should output to stdout with --color=never"
    );
}

#[test]
#[allow(deprecated)]
fn test_ls_color_always_contains_ansi() {
    // --color=always with --show-path should produce ANSI escape codes even in pipe mode
    // Note: simple mode (without --show-path) remains plain text for shell integration
    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=always", "ls", "--show-path"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check for ANSI escape sequence (ESC character: \x1b or \u{1b})
    // Colors should be present in the output
    assert!(
        stdout.contains('\x1b') || stdout.contains('\u{1b}'),
        "pipe mode with --color=always --show-path should contain ANSI escape codes on stdout"
    );
}

#[test]
#[allow(deprecated)]
fn test_ls_color_never_no_ansi() {
    // --color=never should not produce ANSI escape codes
    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=never", "ls"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check that no ANSI escape sequences are present
    assert!(
        !stdout.contains('\x1b') && !stdout.contains('\u{1b}'),
        "stdout should not contain ANSI escape codes with --color=never"
    );
}

#[test]
#[allow(deprecated)]
fn test_ls_pipe_mode_simple_output() {
    // In pipe mode without --show-path, output should be simple branch names
    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=never", "ls"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain @ for main worktree
    assert!(
        stdout.contains('@'),
        "pipe mode should include @ for main worktree"
    );

    // Should be line-based output (one item per line)
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(!lines.is_empty(), "should have at least one line");

    // First line should be @ (main worktree marker)
    assert_eq!(lines[0], "@", "first line should be @ in pipe mode");
}

#[test]
#[allow(deprecated)]
fn test_no_color_env_produces_no_ansi() {
    // NO_COLOR environment variable should disable colors
    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .env("NO_COLOR", "1")
        .arg("ls")
        .output()
        .unwrap();

    // Output can be on either stdout or stderr depending on TTY detection
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    // No ANSI codes should be present
    assert!(
        !combined.contains('\x1b') && !combined.contains('\u{1b}'),
        "NO_COLOR should disable ANSI escape codes"
    );
}

#[test]
#[allow(deprecated)]
fn test_color_always_forces_ansi_in_pipe_mode() {
    // --color=always should force ANSI codes even in pipe mode (non-TTY)
    // Stream selection is independent of color mode
    // Note: requires --show-path for formatted output; simple mode is plain for shell integration
    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["--color=always", "ls", "--show-path"])
        .output()
        .unwrap();

    // Output should be on stdout (pipe mode)
    assert!(
        !output.stdout.is_empty(),
        "pipe mode should output to stdout even with --color=always"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain ANSI codes
    assert!(
        stdout.contains('\x1b') || stdout.contains('\u{1b}'),
        "--color=always with --show-path should produce ANSI codes even in pipe mode"
    );
}

#[test]
#[allow(deprecated)]
fn test_ls_show_path_pipe_mode() {
    // --show-path in pipe mode should output full table to stdout (no colors by default)
    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["ls", "--show-path"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should output to stdout in pipe mode
    assert!(
        !stdout.is_empty(),
        "--show-path in pipe mode should output to stdout"
    );

    // Should not contain ANSI codes (default is auto, which becomes never in pipe)
    assert!(
        !stdout.contains('\x1b') && !stdout.contains('\u{1b}'),
        "--show-path in pipe mode should not have ANSI codes by default"
    );

    // Should contain @ for main worktree
    assert!(
        stdout.contains('@'),
        "--show-path output should include @ marker"
    );
}

#[test]
#[allow(deprecated)]
fn test_ls_show_path_with_color_always() {
    // --show-path with --color=always in pipe mode should output colored table to stdout
    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["ls", "--show-path", "--color=always"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should output to stdout (pipe mode)
    assert!(
        !stdout.is_empty(),
        "--show-path with --color=always should output to stdout"
    );

    // Should contain ANSI codes
    assert!(
        stdout.contains('\x1b') || stdout.contains('\u{1b}'),
        "--show-path with --color=always should include ANSI codes"
    );
}

#[test]
#[allow(deprecated)]
fn test_ls_commit_hash_no_gray_color() {
    const GRAY_COLOR: &str = "\x1b[90m";

    // Commit hashes should NOT be colorized with gray (\x1b[90m)
    // Timestamps should still be gray (\x1b[90m)
    let output = Command::cargo_bin("ofsht")
        .unwrap()
        .args(["ls", "--show-path", "--color=always"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain ANSI codes (for other elements like branch names)
    assert!(
        stdout.contains('\x1b') || stdout.contains('\u{1b}'),
        "output should contain ANSI codes for colored elements"
    );

    // Timestamps should still be gray colored
    assert!(
        stdout.contains(GRAY_COLOR),
        "output should contain gray color (\\x1b[90m) for timestamps"
    );

    // Look for the pattern: \x1b[90m followed by 7 or 8 lowercase hex characters
    // This would indicate a commit hash is being colored gray
    // We search ALL occurrences of gray color to ensure no hash is colored
    for (gray_pos, _) in stdout.match_indices(GRAY_COLOR) {
        let after_gray = &stdout[gray_pos + GRAY_COLOR.len()..];
        // Check if the next characters form a commit hash (7-8 hex chars)
        let potential_hash: String = after_gray
            .chars()
            .take(8)
            .take_while(char::is_ascii_hexdigit)
            .collect();

        // If we found 7 or 8 hex characters right after gray color code,
        // and they're lowercase (typical git hash format), it's likely a commit hash
        if (potential_hash.len() == 7 || potential_hash.len() == 8)
            && potential_hash
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            panic!(
                "Found commit hash '{potential_hash}' colorized with gray (\\x1b[90m). \
                 Commit hashes should not be colored.\nFull output:\n{stdout}"
            );
        }
    }
}
