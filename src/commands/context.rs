//! Command context — shared scaffolding aggregated for command handlers.
//!
//! Every command handler (`add`, `cd`, `create`, `list`, `open`, `rm`, `sync`)
//! repeats the same prologue: resolve the main repository root, load the
//! configuration from that root, and instantiate a `RealGitClient`. This module
//! collapses that prologue into a single `CommandContext::new_strict` /
//! `CommandContext::new_lenient` call.
//!
//! Strictness lives at the constructor level rather than in a separate type so
//! the four fields stay accessible without per-call unwrapping.

use anyhow::Result;
use std::path::PathBuf;

use crate::color::ColorMode;
use crate::commands::common::get_main_repo_root;
use crate::config::Config;
use crate::integrations::git::{GitClient, RealGitClient};

pub struct CommandContext {
    pub repo_root: PathBuf,
    pub config: Config,
    pub git: RealGitClient,
    pub color_mode: ColorMode,
}

impl CommandContext {
    /// Strict constructor: errors when the config file exists but cannot be
    /// parsed. Use for commands where hooks / integration settings drive
    /// behavior (`add`, `create`, `rm`, `sync`, `open`).
    ///
    /// # Errors
    /// Returns an error if not inside a git repository or if `.ofsht.toml`
    /// (local) or the global config file exists but fails to parse.
    pub fn new_strict(color_mode: ColorMode) -> Result<Self> {
        let repo_root = get_main_repo_root()?;
        let config = Config::load_from_repo_root(&repo_root)?;
        Ok(Self {
            repo_root,
            config,
            git: RealGitClient,
            color_mode,
        })
    }

    /// Lenient constructor: falls back to `Config::default()` when the config
    /// file fails to parse, emitting a warning to stderr. Use for commands
    /// that should keep working even when the user's config is malformed
    /// (`list`, `cd`).
    ///
    /// # Errors
    /// Returns an error only when not inside a git repository.
    pub fn new_lenient(color_mode: ColorMode) -> Result<Self> {
        let repo_root = get_main_repo_root()?;
        let config = match Config::load_from_repo_root(&repo_root) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!(
                    "{}",
                    crate::color::warn(
                        color_mode,
                        format!("Failed to load config: {e}; using defaults"),
                    )
                );
                Config::default()
            }
        };
        Ok(Self {
            repo_root,
            config,
            git: RealGitClient,
            color_mode,
        })
    }

    /// Run `git worktree list --porcelain` from the main repository root and
    /// return the raw stdout. Callers parse via
    /// `WorktreeList::parse(&stdout, active_path)` because each handler chooses
    /// `active_path` differently (`list` uses `current_dir.as_deref()`; `cd`,
    /// `sync`, `open` use `None`).
    ///
    /// # Errors
    /// Returns an error if the underlying `git worktree list` invocation fails.
    pub fn worktree_list_stdout(&self) -> Result<String> {
        self.git.list_worktrees(Some(&self.repo_root))
    }
}
