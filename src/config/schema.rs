//! Configuration schema and type definitions

use serde::{Deserialize, Serialize};

/// Configuration for ofsht
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
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

/// tmux integration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxConfig {
    /// Automatic tmux integration behavior
    #[serde(default)]
    pub behavior: TmuxBehavior,
    /// What to create when adding a worktree with --tmux
    /// Values: "window" or "pane"
    #[serde(default = "default_tmux_create")]
    pub create: String,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self {
            behavior: TmuxBehavior::default(),
            create: default_tmux_create(),
        }
    }
}

fn default_tmux_create() -> String {
    "window".to_string()
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

impl Hooks {
    #[allow(dead_code)]
    pub(super) fn merge(&self, other: &Self) -> Self {
        Self {
            create: self.create.merge(&other.create),
            delete: self.delete.merge(&other.delete),
        }
    }
}

impl HookActions {
    #[allow(dead_code)]
    pub(super) fn merge(&self, other: &Self) -> Self {
        let mut run = self.run.clone();
        run.extend(other.run.clone());

        let mut copy = self.copy.clone();
        copy.extend(other.copy.clone());

        let mut link = self.link.clone();
        link.extend(other.link.clone());

        Self { run, copy, link }
    }
}
