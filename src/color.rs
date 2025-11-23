use std::env;
use std::fmt;
use std::str::FromStr;

use owo_colors::OwoColorize;

/// Color mode for terminal output
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Default)]
pub enum ColorMode {
    /// Always use colors
    Always,
    /// Automatically detect whether to use colors
    #[default]
    Auto,
    /// Never use colors
    Never,
}

impl FromStr for ColorMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("always") {
            Ok(Self::Always)
        } else if s.eq_ignore_ascii_case("auto") {
            Ok(Self::Auto)
        } else if s.eq_ignore_ascii_case("never") {
            Ok(Self::Never)
        } else {
            anyhow::bail!("Invalid color mode: {s}. Expected one of: always, auto, never")
        }
    }
}

impl ColorMode {
    /// Resolve color mode from CLI flag and environment variables
    ///
    /// Priority (highest to lowest):
    /// 1. CLI flag (`--color=always|auto|never`)
    /// 2. `NO_COLOR` environment variable
    /// 3. `TERM=dumb` environment variable
    /// 4. Default (Auto)
    #[must_use]
    pub fn resolve(cli_mode: Option<Self>) -> Self {
        // CLI flag has highest priority
        if let Some(mode) = cli_mode {
            return mode;
        }

        // Check NO_COLOR environment variable
        if env::var("NO_COLOR").is_ok() {
            return Self::Never;
        }

        // Check TERM=dumb
        if let Ok(term) = env::var("TERM") {
            if term == "dumb" {
                return Self::Never;
            }
        }

        // Default to Auto
        Self::Auto
    }

    /// Check if colors should be enabled based on the mode and TTY detection
    ///
    /// This checks stderr because that's where colored output is sent.
    /// stdout is reserved for shell integration (path output) and must never be colored.
    ///
    /// Uses `supports-color` crate's cached capability detection to respect platform nuances (ANSI support, etc.)
    #[must_use]
    pub fn should_colorize(self) -> bool {
        match self {
            Self::Always => true,
            Self::Auto => {
                // Use supports-color's cached detection for stderr
                // This respects platform differences (Windows ANSI support, etc.)
                supports_color::on_cached(supports_color::Stream::Stderr).is_some()
            }
            Self::Never => false,
        }
    }

    /// Colorize main worktree marker [@] in green
    #[must_use]
    pub fn colorize_main_worktree(self, text: &str) -> String {
        if self.should_colorize() {
            // Green: \x1b[32m
            format!("\x1b[32m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Colorize branch name in cyan
    #[must_use]
    pub fn colorize_branch(self, text: &str) -> String {
        if self.should_colorize() {
            // Cyan: \x1b[36m
            format!("\x1b[36m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Colorize detached HEAD marker in yellow
    #[must_use]
    pub fn colorize_detached(self, text: &str) -> String {
        if self.should_colorize() {
            // Yellow: \x1b[33m
            format!("\x1b[33m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Colorize secondary info (hash, timestamp) in dim/gray
    #[must_use]
    pub fn colorize_secondary(self, text: &str) -> String {
        if self.should_colorize() {
            // Bright black (gray): \x1b[90m
            format!("\x1b[90m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Colorize active worktree marker in bold magenta
    #[must_use]
    pub fn colorize_active_marker(self, text: &str) -> String {
        if self.should_colorize() {
            // Bold magenta: \x1b[1;35m
            format!("\x1b[1;35m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

/// Message style for different types of output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStyle {
    /// Success message (green ✓)
    Success,
    /// Info/progress message (cyan ℹ)
    Info,
    /// Warning message (yellow ⚠)
    Warn,
    /// Error message (red ✗)
    #[allow(dead_code)]
    Error,
}

impl MessageStyle {
    /// Get the symbol for this message style
    const fn symbol(self) -> &'static str {
        match self {
            Self::Success => "✓",
            Self::Info => "ℹ",
            Self::Warn => "⚠",
            Self::Error => "✗",
        }
    }

    /// Get the plain symbol (fallback for non-TTY)
    const fn plain_symbol(self) -> &'static str {
        match self {
            Self::Success => "✓",
            Self::Info => "ℹ",
            Self::Warn => "⚠",
            Self::Error => "✗",
        }
    }

    /// Format a message with this style
    #[allow(clippy::missing_const_for_fn)]
    pub fn format<D: fmt::Display>(self, mode: ColorMode, message: D) -> FormattedMessage<D> {
        FormattedMessage {
            style: self,
            mode,
            message,
        }
    }
}

/// A formatted message with color and symbol
pub struct FormattedMessage<D> {
    style: MessageStyle,
    mode: ColorMode,
    message: D,
}

impl<D: fmt::Display> fmt::Display for FormattedMessage<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mode.should_colorize() {
            let symbol = self.style.symbol();
            match self.style {
                MessageStyle::Success => {
                    write!(f, "{} {}", symbol.bright_green().bold(), self.message)
                }
                MessageStyle::Info => write!(f, "{} {}", symbol.bright_cyan(), self.message),
                MessageStyle::Warn => write!(f, "{} {}", symbol.bright_yellow(), self.message),
                MessageStyle::Error => write!(f, "{} {}", symbol.bright_red().bold(), self.message),
            }
        } else {
            write!(f, "{} {}", self.style.plain_symbol(), self.message)
        }
    }
}

/// Format a success message (green ✓)
pub fn success<D: fmt::Display>(mode: ColorMode, message: D) -> FormattedMessage<D> {
    MessageStyle::Success.format(mode, message)
}

/// Format an info/progress message (cyan ℹ)
pub fn info<D: fmt::Display>(mode: ColorMode, message: D) -> FormattedMessage<D> {
    MessageStyle::Info.format(mode, message)
}

/// Format a warning message (yellow ⚠)
pub fn warn<D: fmt::Display>(mode: ColorMode, message: D) -> FormattedMessage<D> {
    MessageStyle::Warn.format(mode, message)
}

/// Format an error message (red ✗)
#[allow(dead_code)]
pub fn error<D: fmt::Display>(mode: ColorMode, message: D) -> FormattedMessage<D> {
    MessageStyle::Error.format(mode, message)
}

/// Dimmed text for secondary information
pub struct DimmedText<D> {
    mode: ColorMode,
    text: D,
}

impl<D: fmt::Display> fmt::Display for DimmedText<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mode.should_colorize() {
            write!(f, "{}", format!("{}", self.text).dimmed())
        } else {
            write!(f, "{}", self.text)
        }
    }
}

/// Dim text for secondary information (paths, metadata, etc.)
#[allow(clippy::missing_const_for_fn)]
pub fn dim<D: fmt::Display>(mode: ColorMode, text: D) -> DimmedText<D> {
    DimmedText { mode, text }
}

/// Tree item formatting for nested output (hooks, etc.)
pub struct TreeItem<D> {
    mode: ColorMode,
    message: D,
    is_last: bool,
    indent_level: usize,
}

impl<D: fmt::Display> fmt::Display for TreeItem<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indent = "  ".repeat(self.indent_level);
        let branch = if self.is_last { "└─" } else { "├─" };

        if self.mode.should_colorize() {
            write!(f, "{indent}{} {}", branch.dimmed(), self.message)
        } else {
            write!(f, "{indent}{branch} {}", self.message)
        }
    }
}

/// Format a tree item for nested output
#[allow(clippy::missing_const_for_fn)]
pub fn tree_item<D: fmt::Display>(
    mode: ColorMode,
    message: D,
    is_last: bool,
    indent_level: usize,
) -> TreeItem<D> {
    TreeItem {
        mode,
        message,
        is_last,
        indent_level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_from_cli_always() {
        let mode = ColorMode::resolve(Some(ColorMode::Always));
        assert_eq!(mode, ColorMode::Always);
    }

    #[test]
    fn test_resolve_from_cli_auto() {
        let mode = ColorMode::resolve(Some(ColorMode::Auto));
        assert_eq!(mode, ColorMode::Auto);
    }

    #[test]
    fn test_resolve_from_cli_never() {
        let mode = ColorMode::resolve(Some(ColorMode::Never));
        assert_eq!(mode, ColorMode::Never);
    }

    #[test]
    fn test_from_str_case_insensitive() {
        assert_eq!("ALWAYS".parse::<ColorMode>().unwrap(), ColorMode::Always);
        assert_eq!("Auto".parse::<ColorMode>().unwrap(), ColorMode::Auto);
        assert_eq!("NeVeR".parse::<ColorMode>().unwrap(), ColorMode::Never);
    }

    #[test]
    fn test_from_str_invalid() {
        let result = "invalid".parse::<ColorMode>();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid color mode"));
    }

    #[test]
    fn test_resolve_no_color_env() {
        temp_env::with_var("NO_COLOR", Some("1"), || {
            let mode = ColorMode::resolve(None);
            assert_eq!(mode, ColorMode::Never);
        });
    }

    #[test]
    fn test_resolve_term_dumb() {
        temp_env::with_vars([("TERM", Some("dumb")), ("NO_COLOR", None::<&str>)], || {
            let mode = ColorMode::resolve(None);
            assert_eq!(mode, ColorMode::Never);
        });
    }

    #[test]
    fn test_resolve_cli_overrides_no_color() {
        temp_env::with_var("NO_COLOR", Some("1"), || {
            let mode = ColorMode::resolve(Some(ColorMode::Always));
            assert_eq!(mode, ColorMode::Always);
        });
    }

    #[test]
    fn test_resolve_default_auto() {
        temp_env::with_vars([("NO_COLOR", None::<&str>), ("TERM", None::<&str>)], || {
            let mode = ColorMode::resolve(None);
            assert_eq!(mode, ColorMode::Auto);
        });
    }

    #[test]
    fn test_should_colorize_always() {
        assert!(ColorMode::Always.should_colorize());
    }

    #[test]
    fn test_should_colorize_never() {
        assert!(!ColorMode::Never.should_colorize());
    }

    #[test]
    fn test_should_colorize_auto() {
        // Auto mode checks if stderr is a terminal
        // This test will pass/fail depending on test environment
        // We just verify it doesn't panic
        let _ = ColorMode::Auto.should_colorize();
    }

    #[test]
    fn test_colorize_main_worktree_always() {
        let text = "[@]";
        let colored = ColorMode::Always.colorize_main_worktree(text);
        // Should contain ANSI escape codes
        assert!(colored.contains('\x1b'));
        assert!(colored.contains("[@]"));
    }

    #[test]
    fn test_colorize_main_worktree_never() {
        let text = "[@]";
        let colored = ColorMode::Never.colorize_main_worktree(text);
        // Should not contain ANSI codes
        assert!(!colored.contains('\x1b'));
        assert_eq!(colored, text);
    }

    #[test]
    fn test_colorize_branch_always() {
        let text = "[feature]";
        let colored = ColorMode::Always.colorize_branch(text);
        assert!(colored.contains('\x1b'));
        assert!(colored.contains("[feature]"));
    }

    #[test]
    fn test_colorize_branch_never() {
        let text = "[feature]";
        let colored = ColorMode::Never.colorize_branch(text);
        assert!(!colored.contains('\x1b'));
        assert_eq!(colored, text);
    }

    #[test]
    fn test_colorize_detached_always() {
        let text = "[detached]";
        let colored = ColorMode::Always.colorize_detached(text);
        assert!(colored.contains('\x1b'));
        assert!(colored.contains("[detached]"));
    }

    #[test]
    fn test_colorize_detached_never() {
        let text = "[detached]";
        let colored = ColorMode::Never.colorize_detached(text);
        assert!(!colored.contains('\x1b'));
        assert_eq!(colored, text);
    }

    #[test]
    fn test_colorize_secondary_always() {
        let text = "abc123de";
        let colored = ColorMode::Always.colorize_secondary(text);
        assert!(colored.contains('\x1b'));
        assert!(colored.contains("abc123de"));
    }

    #[test]
    fn test_colorize_secondary_never() {
        let text = "abc123de";
        let colored = ColorMode::Never.colorize_secondary(text);
        assert!(!colored.contains('\x1b'));
        assert_eq!(colored, text);
    }

    #[test]
    fn test_colorize_active_marker_always() {
        let text = "*";
        let colored = ColorMode::Always.colorize_active_marker(text);
        // Should contain ANSI escape codes for bold magenta
        assert!(colored.contains('\x1b'));
        assert!(colored.contains('*'));
        // Bold magenta: \x1b[1;35m
        assert!(colored.starts_with("\x1b[1;35m"));
        assert!(colored.ends_with("\x1b[0m"));
    }

    #[test]
    fn test_colorize_active_marker_never() {
        let text = "*";
        let colored = ColorMode::Never.colorize_active_marker(text);
        assert!(!colored.contains('\x1b'));
        assert_eq!(colored, text);
    }

    #[test]
    fn test_success_message_never() {
        let msg = success(ColorMode::Never, "Created worktree");
        let output = msg.to_string();
        assert!(!output.contains('\x1b'));
        assert_eq!(output, "✓ Created worktree");
    }

    #[test]
    fn test_success_message_always() {
        let msg = success(ColorMode::Always, "Created worktree");
        let output = msg.to_string();
        // Should contain ANSI codes
        assert!(output.contains('\x1b'));
        assert!(output.contains("Created worktree"));
    }

    #[test]
    fn test_info_message_never() {
        let msg = info(ColorMode::Never, "Executing hooks");
        let output = msg.to_string();
        assert!(!output.contains('\x1b'));
        assert_eq!(output, "ℹ Executing hooks");
    }

    #[test]
    fn test_warn_message_never() {
        let msg = warn(ColorMode::Never, "Duplicate target");
        let output = msg.to_string();
        assert!(!output.contains('\x1b'));
        assert_eq!(output, "⚠ Duplicate target");
    }

    #[test]
    fn test_error_message_never() {
        let msg = error(ColorMode::Never, "Failed to create");
        let output = msg.to_string();
        assert!(!output.contains('\x1b'));
        assert_eq!(output, "✗ Failed to create");
    }

    #[test]
    fn test_dim_text_never() {
        let dimmed = dim(ColorMode::Never, "metadata");
        let output = dimmed.to_string();
        assert!(!output.contains('\x1b'));
        assert_eq!(output, "metadata");
    }

    #[test]
    fn test_dim_text_always() {
        let dimmed = dim(ColorMode::Always, "metadata");
        let output = dimmed.to_string();
        // Should contain ANSI codes for dimmed text
        assert!(output.contains('\x1b'));
    }

    #[test]
    fn test_tree_item_never() {
        let item = tree_item(ColorMode::Never, "Running command", false, 1);
        let output = item.to_string();
        assert!(!output.contains('\x1b'));
        assert_eq!(output, "  ├─ Running command");
    }

    #[test]
    fn test_tree_item_last_never() {
        let item = tree_item(ColorMode::Never, "Running command", true, 1);
        let output = item.to_string();
        assert!(!output.contains('\x1b'));
        assert_eq!(output, "  └─ Running command");
    }

    #[test]
    fn test_tree_item_nested_never() {
        let item = tree_item(ColorMode::Never, "Nested item", false, 2);
        let output = item.to_string();
        assert_eq!(output, "    ├─ Nested item");
    }

    #[test]
    fn test_tree_item_always() {
        let item = tree_item(ColorMode::Always, "Running command", false, 1);
        let output = item.to_string();
        // Should contain ANSI codes
        assert!(output.contains('\x1b'));
        assert!(output.contains("Running command"));
    }
}
