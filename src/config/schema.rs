//! Configuration schema and type definitions

use serde::{Deserialize, Serialize};
use std::fmt;

/// Effective runtime configuration for ofsht.
///
/// Composed by `build_effective_config` from a `ProjectConfig` (local
/// `.ofsht.toml`) and a `UserConfig` (global `~/.config/ofsht/config.toml`).
/// Consumers receive this type via `Config::load_from_repo_root`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub hooks: Hooks,
    #[serde(default)]
    pub worktree: WorktreeConfig,
    #[serde(default, alias = "integration")]
    pub integrations: IntegrationsConfig,
}

/// Parse-only DTO for `.ofsht.toml` (project-root local config).
///
/// Top-level `deny_unknown_fields` enforces "integrations only in global
/// config" at parse time — a local file containing `[integration.*]` fails
/// loudly instead of being silently ignored.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default)]
    pub hooks: Hooks,
    #[serde(default)]
    pub worktree: WorktreeConfig,
}

/// Parse-only DTO for `~/.config/ofsht/config.toml` (user-level global
/// config). Carries integrations alongside hooks/worktree.
///
/// Top-level `deny_unknown_fields` surfaces top-level typos (`[hook]` instead
/// of `[hooks]`, etc.) as parse errors rather than silently dropping them.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    #[serde(default)]
    pub hooks: Hooks,
    #[serde(default)]
    pub worktree: WorktreeConfig,
    #[serde(default, alias = "integration")]
    pub integrations: IntegrationsConfig,
}

/// Hook configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Hooks {
    #[serde(default)]
    pub create: HookActions,
    #[serde(default)]
    pub delete: HookActions,
}

/// Actions to perform in a hook
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookActions {
    /// Commands to run
    #[serde(default)]
    pub run: Vec<String>,
    /// Files to copy from source repository
    #[serde(default)]
    pub copy: Vec<String>,
    /// Symbolic links to create
    /// Patterns are expanded and linked to the same relative path in the worktree
    #[serde(default)]
    pub link: Vec<String>,
}

/// Worktree settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeConfig {
    /// Directory template for worktree creation
    /// Variables: {repo}, {branch}
    #[serde(default = "default_dir")]
    pub dir: String,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self { dir: default_dir() }
    }
}

fn default_dir() -> String {
    "../{repo}-worktrees/{branch}".to_string()
}

/// Integration configurations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrationsConfig {
    #[serde(default)]
    pub zoxide: ZoxideConfig,
    #[serde(default)]
    pub fzf: FzfConfig,
    #[serde(default)]
    pub tmux: TmuxConfig,
    #[serde(default)]
    pub gh: GhConfig,
}

/// zoxide integration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoxideConfig {
    /// Enable zoxide integration
    #[serde(default = "default_zoxide_enabled")]
    pub enabled: bool,
}

impl Default for ZoxideConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

const fn default_zoxide_enabled() -> bool {
    true
}

/// fzf integration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FzfConfig {
    /// Enable fzf integration
    #[serde(default = "default_fzf_enabled")]
    pub enabled: bool,
    /// Additional fzf command-line options
    #[serde(default)]
    pub options: Vec<String>,
}

impl Default for FzfConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            options: Vec::new(),
        }
    }
}

const fn default_fzf_enabled() -> bool {
    true
}

/// tmux integration behavior
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TmuxBehavior {
    /// Flag-based (default): only use tmux when --tmux is specified
    #[default]
    Auto,
    /// Always use tmux integration (can be overridden with --no-tmux)
    Always,
    /// Never use tmux integration
    Never,
}

/// What kind of tmux entity to create / open: a new window or a split pane.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum OpenMode {
    /// Create a new tmux window.
    #[default]
    Window,
    /// Split the current window into a pane.
    Pane,
}

impl fmt::Display for OpenMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Window => f.write_str("window"),
            Self::Pane => f.write_str("pane"),
        }
    }
}

/// tmux integration configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TmuxConfig {
    /// Automatic tmux integration behavior
    #[serde(default)]
    pub behavior: TmuxBehavior,
    /// What to create when adding a worktree with --tmux
    #[serde(default)]
    pub create: OpenMode,
    /// Default mode for `ofsht open`
    #[serde(default)]
    pub open: OpenMode,
}

/// GitHub CLI integration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhConfig {
    /// Enable GitHub CLI integration
    #[serde(default = "default_gh_enabled")]
    pub enabled: bool,
}

impl Default for GhConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

const fn default_gh_enabled() -> bool {
    true
}
