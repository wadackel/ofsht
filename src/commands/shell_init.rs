//! Shell init command - Generate shell integration scripts

use anyhow::Result;

/// Generate shell integration script
///
/// # Errors
/// Returns an error if invalid shell is specified
pub fn cmd_shell_init(shell: &str) -> Result<()> {
    // Get shell integration script template
    let script = match shell {
        "bash" => include_str!("../../templates/bash.sh"),
        "zsh" => include_str!("../../templates/zsh.sh"),
        "fish" => include_str!("../../templates/fish.fish"),
        _ => {
            anyhow::bail!("Invalid shell: {shell}. Supported shells: bash, zsh, fish");
        }
    };

    print!("{script}");

    Ok(())
}
