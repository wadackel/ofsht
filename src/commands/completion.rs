//! Completion command - Generate shell completion setup instructions

use anyhow::Result;
use clap_complete::Shell;

/// Generate shell completion setup instructions
///
/// # Errors
/// Returns an error if:
/// - Invalid shell specified
pub fn cmd_completion(shell: &str) -> Result<()> {
    // Validate shell type
    let _ = shell.parse::<Shell>().map_err(|_| {
        anyhow::anyhow!("Invalid shell: {shell}. Supported shells: bash, zsh, fish")
    })?;

    // Print dynamic completion setup instructions
    let instructions = match shell {
        "bash" => {
            r"# ofsht shell completion setup for Bash
# Add this to your ~/.bashrc:
source <(COMPLETE=bash ofsht)
"
        }
        "zsh" => {
            r"# ofsht shell completion setup for Zsh
# Add this to your ~/.zshrc:
source <(COMPLETE=zsh ofsht)
"
        }
        "fish" => {
            r"# ofsht shell completion setup for Fish
# Add this to your ~/.config/fish/config.fish:
source (COMPLETE=fish ofsht | psub)
"
        }
        _ => {
            anyhow::bail!("Unsupported shell: {shell}");
        }
    };

    print!("{instructions}");

    Ok(())
}
