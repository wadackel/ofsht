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
