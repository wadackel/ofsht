//! Shell init command - Generate shell integration scripts

use anyhow::Result;

/// Generate shell integration script
///
/// # Errors
/// Returns an error if invalid shell is specified
pub fn cmd_shell_init(shell: &str) -> Result<()> {
    let script = match shell {
        "bash" | "zsh" => include_str!("../../templates/bash_zsh.sh"),
        "fish" => include_str!("../../templates/fish.fish"),
        _ => {
            anyhow::bail!("Invalid shell: {shell}. Supported shells: bash, zsh, fish");
        }
    };

    crate::output::write_stdout(format_args!("{script}"))?;

    Ok(())
}
