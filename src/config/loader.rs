//! Configuration loading logic

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::schema::{Config, IntegrationsConfig};

impl Config {
    /// Load configuration from a TOML file
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or parsed
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let config: Self = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok(config)
    }

    /// Load configuration with fallback (from current working directory)
    ///
    /// This function is provided for backward compatibility and cases where
    /// you're not in a git repository. For git repository operations, prefer
    /// `load_from_repo_root()` to ensure consistent behavior across worktrees.
    ///
    /// Load priority:
    /// 1. Local config (.ofsht.toml in current directory)
    /// 2. Global config (~/.config/ofsht/config.toml)
    /// 3. Default config
    ///
    /// # Errors
    /// Returns an error if configuration files exist but cannot be read or parsed
    #[allow(dead_code)]
    pub fn load() -> Result<Self> {
        Self::load_impl(None)
    }

    /// Load configuration with fallback (from specified repository root)
    ///
    /// This is the recommended way to load config for git operations. It ensures
    /// that .ofsht.toml is always loaded from the main repository root, not from
    /// individual worktrees.
    ///
    /// Load priority:
    /// 1. Local config (.ofsht.toml in `repo_root` directory)
    /// 2. Global config (~/.config/ofsht/config.toml)
    /// 3. Default config
    ///
    /// # Arguments
    /// * `repo_root` - Path to the main repository root (from `get_main_repo_root()`)
    ///
    /// # Errors
    /// Returns an error if configuration files exist but cannot be read or parsed
    pub fn load_from_repo_root(repo_root: &Path) -> Result<Self> {
        Self::load_impl(Some(repo_root))
    }

    /// Load integration settings from global config
    /// Falls back to default if global config doesn't exist or can't be read
    fn load_integration_from_global() -> IntegrationsConfig {
        Self::global_config_path()
            .and_then(|path| {
                if path.exists() {
                    Self::from_file(&path).ok()
                } else {
                    None
                }
            })
            .map(|config| config.integrations)
            .unwrap_or_default()
    }

    /// Internal implementation for config loading
    fn load_impl(repo_root: Option<&Path>) -> Result<Self> {
        // Try local config first
        let local_config = repo_root.map_or_else(Self::local_config_path, |root| {
            Self::local_config_path_from(root)
        });

        if local_config.exists() {
            let mut config = Self::from_file(&local_config)?;
            // Integration configuration is only available in global config
            // Load integration settings from global config (or defaults if unavailable)
            config.integrations = Self::load_integration_from_global();
            return Ok(config);
        }

        // Try global config
        if let Some(global_config) = Self::global_config_path() {
            if global_config.exists() {
                return Self::from_file(&global_config);
            }
        }

        // Return default config
        Ok(Self::default())
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

    /// Merge this config with another (other takes precedence)
    #[must_use]
    #[allow(dead_code)]
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            hooks: self.hooks.merge(&other.hooks),
            worktree: other.worktree.clone(),
            integrations: other.integrations.clone(),
        }
    }
}
