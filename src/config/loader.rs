//! Configuration loading logic

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::schema::{Config, HookActions, ProjectConfig, ProjectHookActions, UserConfig};

impl Config {
    /// Load configuration with fallback (from specified repository root)
    ///
    /// This is the recommended way to load config for git operations. It ensures
    /// that .ofsht.toml is always loaded from the main repository root, not from
    /// individual worktrees.
    ///
    /// Load priority:
    /// 1. Local config (.ofsht.toml in `repo_root` directory) — parsed as `ProjectConfig`
    /// 2. Global config (~/.config/ofsht/config.toml) — parsed as `UserConfig`
    /// 3. Default config
    ///
    /// # Arguments
    /// * `repo_root` - Path to the main repository root (from `get_main_repo_root()`)
    ///
    /// # Errors
    /// Returns an error if a configuration file exists but cannot be read or
    /// parsed.
    pub fn load_from_repo_root(repo_root: &Path) -> Result<Self> {
        let project = load_project_config(repo_root)?;
        let user = load_user_config()?;
        Ok(build_effective_config(project, user))
    }

    /// Get the local config path from a specific directory
    /// Returns the path to .ofsht.toml in the specified directory
    #[must_use]
    pub fn local_config_path_from(repo_root: &Path) -> PathBuf {
        repo_root.join(".ofsht.toml")
    }

    /// Get the local config path
    /// Returns the path to the local config file in the current directory
    #[must_use]
    pub fn local_config_path() -> PathBuf {
        PathBuf::from(".ofsht.toml")
    }

    /// Get the global config path
    /// Respects `XDG_CONFIG_HOME` environment variable on all platforms.
    /// Fallback: `$HOME/.config/ofsht/config.toml`
    #[must_use]
    pub fn global_config_path() -> Option<PathBuf> {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| dirs::home_dir().map(|home| home.join(".config")))?;

        Some(config_home.join("ofsht").join("config.toml"))
    }
}

/// Read a TOML config file with the 3-branch silent fallback.
///
/// - File doesn't exist (`io::ErrorKind::NotFound`) → `Ok(None)` (normal operation).
/// - Read fails (permissions, IO) or parse fails → propagate via `with_context`.
/// - Read + parse succeed → `Ok(Some(parsed))`.
///
/// `read_to_string` performs `open + read` atomically at the OS level, so the
/// `path.exists() + open` TOCTOU window present in the previous implementation
/// is eliminated by relying on `open` failure with `NotFound` instead.
fn read_config_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("Failed to read config file: {}", path.display())),
        Ok(content) => {
            let parsed = toml::from_str::<T>(&content)
                .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
            Ok(Some(parsed))
        }
    }
}

/// Load the project-local `.ofsht.toml` (if present) as a `ProjectConfig`.
///
/// Returns `Ok(None)` when the file is absent. Propagates read or parse
/// errors (including `deny_unknown_fields` violations such as a local file
/// containing `[integration.*]`).
fn load_project_config(repo_root: &Path) -> Result<Option<ProjectConfig>> {
    let path = Config::local_config_path_from(repo_root);
    read_config_file(&path)
}

/// Load the user-level `~/.config/ofsht/config.toml` (if present) as a
/// `UserConfig`.
///
/// Returns `Ok(None)` when the path cannot be resolved (no `HOME` and no
/// absolute `XDG_CONFIG_HOME`) or the file is absent. Propagates read or
/// parse errors.
fn load_user_config() -> Result<Option<UserConfig>> {
    let Some(path) = Config::global_config_path() else {
        return Ok(None);
    };
    read_config_file(&path)
}

/// Apply supplied local fields over global settings and built-in defaults.
fn build_effective_config(project: Option<ProjectConfig>, user: Option<UserConfig>) -> Config {
    let user = user.unwrap_or_default();
    let mut config = Config {
        hooks: user.hooks,
        worktree: user.worktree,
        integrations: user.integrations,
    };
    if let Some(project) = project {
        if let Some(dir) = project.worktree.dir {
            config.worktree.dir = dir;
        }
        merge_actions(&mut config.hooks.create, project.hooks.create);
        merge_actions(&mut config.hooks.delete, project.hooks.delete);
    }
    config
}

fn merge_actions(target: &mut HookActions, local: ProjectHookActions) {
    if let Some(run) = local.run {
        target.run = run;
    }
    if let Some(copy) = local.copy {
        target.copy = copy;
    }
    if let Some(link) = local.link {
        target.link = link;
    }
}
