#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]

use crate::integrations::fzf::is_fzf_available;
use crate::integrations::gh::{GhClient, RealGhClient};
use crate::integrations::tmux::{RealTmuxLauncher, TmuxLauncher};
use crate::integrations::zoxide::is_zoxide_available;

/// Context for template generation based on detected tool availability
///
/// This struct holds the availability status of external tools that ofsht
/// integrates with. It is used to generate appropriate configuration templates
/// that match the user's environment.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct TemplateContext {
    /// Whether GitHub CLI (gh) is available
    pub gh_available: bool,
    /// Whether zoxide is available
    pub zoxide_available: bool,
    /// Whether fzf is available
    pub fzf_available: bool,
    /// Whether tmux is available
    pub tmux_available: bool,
}

impl TemplateContext {
    /// Detect all tool availability in the current environment
    pub fn detect() -> Self {
        Self {
            gh_available: RealGhClient.is_available(),
            zoxide_available: is_zoxide_available(),
            fzf_available: is_fzf_available(),
            tmux_available: RealTmuxLauncher.detect().is_ok(),
        }
    }

    /// Generate global config template based on tool availability
    pub fn generate_global(&self) -> String {
        let zoxide_section = if self.zoxide_available {
            "[integration.zoxide]
# Enable automatic zoxide integration
# When true, new worktrees are automatically added to zoxide database
enabled = true"
        } else {
            "[integration.zoxide]
# Enable automatic zoxide integration
# zoxide not detected - install from https://github.com/ajeetdsouza/zoxide
enabled = false"
        };

        let fzf_section = if self.fzf_available {
            "[integration.fzf]
# Enable fzf integration for interactive worktree selection
# When enabled, running `ofsht cd` or `ofsht rm` without arguments
# will launch fzf for interactive selection
enabled = true
# Additional fzf command-line options (optional)
# options = [\"--height=50%\", \"--border\", \"--reverse\"]"
        } else {
            "[integration.fzf]
# Enable fzf integration for interactive worktree selection
# fzf not detected - install from https://github.com/junegunn/fzf
enabled = false"
        };

        let tmux_section = if self.tmux_available {
            "[integration.tmux]
# Configure tmux integration behavior
# behavior: \"auto\" (use --tmux flag), \"always\" (always enabled), \"never\" (disabled)
behavior = \"auto\"
# Determines what to create: a new window or split pane
create = \"window\"  # \"window\" or \"pane\""
        } else {
            "[integration.tmux]
# Configure tmux integration behavior
# tmux not detected - install from https://github.com/tmux/tmux
behavior = \"never\"
create = \"window\""
        };

        let gh_section = if self.gh_available {
            "[integration.gh]
# Enable GitHub CLI (gh) integration
# When enabled, `ofsht add #123` will create worktrees from GitHub issues/PRs
# Requires the gh CLI to be installed (https://cli.github.com/)
enabled = true"
        } else {
            "[integration.gh]
# Enable GitHub CLI (gh) integration
# gh CLI not detected - install from https://cli.github.com/
enabled = false"
        };

        format!(
            r#"# ofsht global configuration
# This file contains default settings applied to all repositories.
# Project-specific settings in .ofsht.toml will override these values.

[worktree]
# Directory template for new worktrees
# Variables: {{repo}} = repository name, {{branch}} = branch name
# Relative paths are resolved from the main repository root
dir = "../{{repo}}-worktrees/{{branch}}"

[hooks.create]
# Commands to run after creating a worktree (executed in worktree directory)
run = [
    # "pnpm install",
]

# Files to copy from main repository to new worktree
copy = [
    # ".env.local",
    # ".vscode/settings.json",
]

# Files to symlink from main repository to new worktree
# Supports glob patterns: "*.env", "config/**/*.json"
link = [
    # ".claude/settings.local.json",
]

[hooks.delete]
# Commands to run before deleting a worktree (executed in worktree directory)
run = [
    # "pnpm store prune",
]

{zoxide_section}

{fzf_section}

{tmux_section}

{gh_section}
"#
        )
    }

    /// Generate local config template
    #[allow(clippy::unused_self)]
    pub fn generate_local(&self) -> String {
        r#"# ofsht project configuration
# Location: .ofsht.toml (main repository root)
#
# This file is ALWAYS loaded from the main repository root, even when
# running ofsht commands from worktrees. This ensures consistent behavior
# across all worktrees.
#
# This file overrides global settings for this specific repository.
# Add this file to .gitignore if settings are user-specific,
# or commit it if settings should be shared with the team.

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
    # ".claude/settings.local.json",
]

[hooks.delete]
# Commands to run before deleting a worktree
run = []
"#
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_all_tools_available() {
        // This test will be environment-dependent
        // We'll test using direct construction instead
        let ctx = TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };

        assert!(ctx.gh_available);
        assert!(ctx.zoxide_available);
        assert!(ctx.fzf_available);
        assert!(ctx.tmux_available);
    }

    #[test]
    fn test_detect_some_tools_unavailable() {
        // Test with manually constructed context
        let ctx = TemplateContext {
            gh_available: false,
            zoxide_available: true,
            fzf_available: false,
            tmux_available: true,
        };

        assert!(!ctx.gh_available);
        assert!(ctx.zoxide_available);
        assert!(!ctx.fzf_available);
        assert!(ctx.tmux_available);
    }

    #[test]
    fn test_generate_global_all_enabled() {
        let ctx = TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };

        let template = ctx.generate_global();

        // Verify template contains all integration sections with enabled = true
        assert!(template.contains("[integration.zoxide]"));
        assert!(template.contains("[integration.fzf]"));
        assert!(template.contains("[integration.tmux]"));
        assert!(template.contains("[integration.gh]"));

        // Verify zoxide section has enabled = true (no comment)
        assert!(template.contains("enabled = true"));

        // Verify example uses .claude/settings.local.json instead of node_modules
        assert!(template.contains(".claude/settings.local.json"));
        assert!(!template.contains("node_modules"));

        // Verify "Note: copy and link..." is removed
        assert!(!template.contains("Note: copy and link actions are not supported in delete hooks"));
    }

    #[test]
    fn test_generate_global_gh_disabled() {
        let ctx = TemplateContext {
            gh_available: false,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };

        let template = ctx.generate_global();

        // Verify gh section has enabled = false with install comment
        assert!(template.contains("[integration.gh]"));
        assert!(template.contains("enabled = false"));
        assert!(template.contains("https://cli.github.com"));

        // Other tools should be enabled
        assert!(template.contains("[integration.zoxide]"));
        assert!(template.contains("enabled = true"));
    }

    #[test]
    fn test_generate_global_uses_claude_example() {
        let ctx = TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };

        let template = ctx.generate_global();

        // Verify .claude/settings.local.json is used in examples
        assert!(template.contains(".claude/settings.local.json"));

        // Verify old node_modules example is replaced
        assert!(!template.contains("# \"node_modules\""));
    }

    #[test]
    fn test_generate_local_no_worktree_section() {
        let ctx = TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };

        let template = ctx.generate_local();

        // Verify [worktree] section does not exist
        assert!(!template.contains("[worktree]"));
    }

    #[test]
    fn test_generate_local_has_hooks_only() {
        let ctx = TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };

        let template = ctx.generate_local();

        // Verify hooks sections exist
        assert!(template.contains("[hooks.create]"));
        assert!(template.contains("[hooks.delete]"));

        // Verify integration sections do NOT exist
        assert!(!template.contains("[integration.zoxide]"));
        assert!(!template.contains("[integration.fzf]"));
        assert!(!template.contains("[integration.tmux]"));
        assert!(!template.contains("[integration.gh]"));
    }

    #[test]
    fn test_generate_local_uses_claude_example() {
        let ctx = TemplateContext {
            gh_available: true,
            zoxide_available: true,
            fzf_available: true,
            tmux_available: true,
        };

        let template = ctx.generate_local();

        // Verify .claude/settings.local.json is used in examples
        assert!(template.contains(".claude/settings.local.json"));

        // Verify old node_modules example is replaced
        assert!(!template.contains("# \"node_modules\""));
    }
}
