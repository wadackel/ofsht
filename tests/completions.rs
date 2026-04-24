use std::process::Command;

#[test]
fn test_dynamic_completion_with_complete_env() {
    let output = Command::new("cargo")
        .args(["run", "--"])
        .env("COMPLETE", "bash")
        .output()
        .expect("Failed to execute command");

    // When COMPLETE env is set, the program should handle completion and exit successfully
    assert!(
        output.status.success(),
        "Command should succeed with COMPLETE=bash"
    );
}

#[test]
fn test_completions_bash() {
    let output = Command::new("cargo")
        .args(["run", "--", "completion", "bash"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success(), "Command should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Output should not be empty");
    // Should contain dynamic completion setup instructions
    assert!(
        stdout.contains("source") || stdout.contains("COMPLETE"),
        "Should contain setup instructions for dynamic completion"
    );
}

#[test]
fn test_completions_zsh() {
    let output = Command::new("cargo")
        .args(["run", "--", "completion", "zsh"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success(), "Command should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Output should not be empty");
    assert!(
        stdout.contains("source") || stdout.contains("COMPLETE"),
        "Should contain setup instructions for dynamic completion"
    );
}

#[test]
fn test_completions_fish() {
    let output = Command::new("cargo")
        .args(["run", "--", "completion", "fish"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success(), "Command should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "Output should not be empty");
    assert!(
        stdout.contains("source") || stdout.contains("COMPLETE"),
        "Should contain setup instructions for dynamic completion"
    );
}

#[test]
fn test_completions_invalid_shell() {
    let output = Command::new("cargo")
        .args(["run", "--", "completion", "invalid"])
        .output()
        .expect("Failed to execute command");

    assert!(
        !output.status.success(),
        "Command should fail for invalid shell"
    );
}

// ----- Flag-filter integration tests -----
// These tests exercise the FilteredBash/Zsh/Fish adapters end-to-end by invoking
// the binary directly via CARGO_BIN_EXE_ofsht (no cargo run recompilation).

fn run_completion(shell: &str, index: Option<usize>, words: &[&str]) -> String {
    let bin = env!("CARGO_BIN_EXE_ofsht");
    let mut cmd = Command::new(bin);
    cmd.env("COMPLETE", shell);
    if let Some(i) = index {
        cmd.env("_CLAP_COMPLETE_INDEX", i.to_string());
    }
    cmd.env("_CLAP_IFS", "\n");
    cmd.arg("--");
    for word in words {
        cmd.arg(word);
    }
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke {bin}: {e}"));
    assert!(
        output.status.success(),
        "binary exited non-zero: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn test_cd_empty_word_excludes_flags() {
    let stdout = run_completion("bash", Some(2), &["ofsht", "cd", ""]);
    assert!(stdout.contains('@'), "expected @ in stdout: {stdout:?}");
    for banned in ["--color", "--help", "--verbose", "-v", "-h"] {
        assert!(
            !stdout.contains(banned),
            "{banned} must not appear in stdout: {stdout:?}"
        );
    }
}

#[test]
fn test_cd_dash_includes_flags() {
    let stdout = run_completion("bash", Some(2), &["ofsht", "cd", "-"]);
    assert!(
        stdout.contains("--color"),
        "--color must appear when dash typed: {stdout:?}"
    );
}

#[test]
fn test_toplevel_empty_word_excludes_flags() {
    let stdout = run_completion("bash", Some(1), &["ofsht", ""]);
    for expected in ["add", "cd", "rm"] {
        assert!(
            stdout.contains(expected),
            "{expected} must appear in top-level completion: {stdout:?}"
        );
    }
    for banned in ["--color", "--help", "--version"] {
        assert!(
            !stdout.contains(banned),
            "{banned} must not appear in top-level completion with empty current word: {stdout:?}"
        );
    }
}

#[test]
fn test_option_value_completion_intact() {
    let stdout = run_completion("bash", Some(2), &["ofsht", "--color", ""]);
    for expected in ["auto", "always", "never"] {
        assert!(
            stdout.contains(expected),
            "{expected} must appear in --color value completion: {stdout:?}"
        );
    }
    for token in stdout.split_whitespace() {
        assert!(
            !token.starts_with("--"),
            "unexpected flag-like token {token:?} in option-value completion: {stdout:?}"
        );
    }
}

#[test]
fn test_zsh_empty_word_excludes_flags() {
    let stdout = run_completion("zsh", Some(2), &["ofsht", "cd", ""]);
    assert!(stdout.contains('@'), "expected @ in zsh stdout: {stdout:?}");
    for banned in ["--color", "--help", "--verbose"] {
        assert!(
            !stdout.contains(banned),
            "{banned} must not appear in zsh output: {stdout:?}"
        );
    }
}

#[test]
fn test_fish_empty_word_excludes_flags() {
    // Fish uses args.len() - 1 as index; _CLAP_COMPLETE_INDEX is ignored.
    let stdout = run_completion("fish", None, &["ofsht", "cd", ""]);
    assert!(
        stdout.contains('@'),
        "expected @ in fish stdout: {stdout:?}"
    );
    for line in stdout.lines() {
        assert!(
            !line.starts_with("--"),
            "fish output line must not start with --: {line:?}"
        );
    }
}
