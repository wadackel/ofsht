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

/// Template for global configuration file
const TEMPLATE_GLOBAL: &str = r#"# ofsht global configuration
# Location: ~/.config/ofsht/config.toml
#
# This file contains default settings applied to all repositories.
# Project-specific settings in .ofsht.toml will override these values.

[worktree]
# Directory template for new worktrees
# Variables: {repo} = repository name, {branch} = branch name
# Relative paths are resolved from the main repository root
dir = "../{repo}-worktrees/{branch}"

[hooks.create]
# Commands to run after creating a worktree (executed in worktree directory)
run = [
    # "pnpm install",
    # "npm install",
]

# Files to copy from main repository to new worktree
copy = [
    # ".env.local",
    # ".vscode/settings.json",
]

# Files to symlink from main repository to new worktree
# Supports glob patterns: "*.env", "config/**/*.json"
link = [
    # ".env",
    # "node_modules",
]

[hooks.delete]
# Commands to run before deleting a worktree (executed in worktree directory)
run = [
    # "pnpm store prune",
]

# Note: copy and link actions are not supported in delete hooks

[integration.zoxide]
# Enable automatic zoxide integration
# When true, new worktrees are automatically added to zoxide database
enabled = true

[integration.fzf]
# Enable fzf integration for interactive worktree selection
# When enabled, running `ofsht cd` or `ofsht rm` without arguments
# will launch fzf for interactive selection
enabled = true
# Additional fzf command-line options (optional)
# options = ["--height=50%", "--border", "--reverse"]

[integration.tmux]
# Configure tmux integration behavior
# behavior: "auto" (use --tmux flag), "always" (always enabled), "never" (disabled)
behavior = "auto"
# Determines what to create: a new window or split pane
create = "window"  # "window" or "pane"

[integration.gh]
# Enable GitHub CLI (gh) integration
# When enabled, `ofsht add #123` will create worktrees from GitHub issues/PRs
# Requires the gh CLI to be installed (https://cli.github.com/)
enabled = true
"#;

/// Template for local configuration file
const TEMPLATE_LOCAL: &str = r#"# ofsht project configuration
# Location: .ofsht.toml (main repository root)
#
# This file is ALWAYS loaded from the main repository root, even when
# running ofsht commands from worktrees. This ensures consistent behavior
# across all worktrees.
#
# This file overrides global settings for this specific repository.
# Add this file to .gitignore if settings are user-specific,
# or commit it if settings should be shared with the team.

[worktree]
# Directory template for new worktrees (optional, overrides global config)
# Variables: {repo} = repository name, {branch} = branch name
# dir = "../{repo}-worktrees/{branch}"

[hooks.create]
# Commands to run after creating a worktree
run = [
    # "pnpm install",
]

# Files to copy from main repository
copy = [
    # ".env.local",
]

# Files to symlink (supports glob patterns)
link = [
    # "node_modules",
]

[hooks.delete]
# Commands to run before deleting a worktree
run = []

# Note: integration configuration (zoxide, fzf, tmux) is only available in global config
#       (~/.config/ofsht/config.toml)
"#;

impl Config {
    /// Get the global configuration template
    #[must_use]
    pub const fn template_global() -> &'static str {
        TEMPLATE_GLOBAL
    }

    /// Get the local configuration template
    #[must_use]
    pub const fn template_local() -> &'static str {
        TEMPLATE_LOCAL
    }
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
