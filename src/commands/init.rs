//! Init command - Initialize configuration files

use anyhow::{Context, Result};

use crate::color;
use crate::commands::common::get_main_repo_root;
use crate::config;
use crate::domain::worktree::display_path;

/// Write config file if it doesn't exist (or force overwrite)
fn write_config_if_needed(
    path: &std::path::Path,
    template: &str,
    force: bool,
    label: &str,
    color_mode: color::ColorMode,
) -> Result<()> {
    // Check if already exists
    if path.exists() && !force {
        eprintln!(
            "{}",
            color::warn(
                color_mode,
                format!("{label} config already exists: {}", display_path(path))
            )
        );
        eprintln!("Use --force to overwrite");
        return Ok(());
    }

    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    // Write template
    std::fs::write(path, template)
        .with_context(|| format!("Failed to write config file: {}", path.display()))?;

    eprintln!(
        "{}",
        color::success(
            color_mode,
            format!("Created {label} config: {}", display_path(path))
        )
    );
    Ok(())
}

/// Initialize configuration files
///
/// # Errors
/// Returns an error if:
/// - Global config path cannot be determined
/// - File write fails
pub fn cmd_init(
    scope_global: bool,
    scope_local: bool,
    force: bool,
    color_mode: color::ColorMode,
) -> Result<()> {
    // Determine what to generate
    // Default (no flags): create both configs
    let generate_global = scope_global || !scope_local;
    let generate_local = scope_local || !scope_global;

    // Generate global config
    if generate_global {
        let Some(path) = config::Config::global_config_path() else {
            anyhow::bail!(
                "Could not determine global config path (HOME directory not found). \
                 Please set the HOME environment variable or XDG_CONFIG_HOME."
            );
        };
        write_config_if_needed(
            &path,
            config::Config::template_global(),
            force,
            "Global",
            color_mode,
        )?;
    }

    // Generate local config
    if generate_local {
        // Get repo root if we're in a git repository
        let config_path = get_main_repo_root().map_or_else(
            |_| config::Config::local_config_path(),
            |repo_root| config::Config::local_config_path_from(&repo_root),
        );

        write_config_if_needed(
            &config_path,
            config::Config::template_local(),
            force,
            "Local",
            color_mode,
        )?;
    }

    Ok(())
}
