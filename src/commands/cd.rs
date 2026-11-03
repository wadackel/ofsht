//! Cd command - Navigate to a worktree by branch name

use anyhow::{Context, Result};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::commands::common::ambiguous_name_error;
use crate::commands::context::CommandContext;
use crate::domain::worktree::{NameResolution, WorktreeList};
use crate::integrations;
use crate::integrations::fzf::FzfPicker;
use crate::path_utils::{canonicalize_allow_missing, normalize_absolute_path};

/// Navigate to a worktree by branch name
///
/// # Errors
/// Returns an error if:
/// - Git worktree list command fails
/// - Worktree not found
/// - The name matches several worktrees by branch or directory name
/// - Fzf is required but not available
pub fn cmd_goto(name: Option<&str>, color_mode: crate::color::ColorMode) -> Result<()> {
    let ctx = CommandContext::new_lenient(color_mode)?;
    let stdout = ctx.worktree_list_stdout()?;
    let root = ctx.worktree_root();

    // Resolve name: CLI arg > stdin (when piped) > fzf
    let resolved_name: Option<String> = match name {
        Some(n) => Some(n.to_string()),
        None => crate::stdin::try_read_stdin_first()?,
    };

    let Some(name) = resolved_name else {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("worktree name required (provide as argument or via stdin)");
        }
        if !ctx.config.integrations.fzf.enabled {
            anyhow::bail!("Provide a worktree name or enable fzf in config");
        }

        if !integrations::fzf::is_fzf_available() {
            anyhow::bail!("fzf is not installed. Install it or provide a worktree name");
        }

        // Build items for fzf
        let items = integrations::fzf::build_worktree_items(&stdout, root.as_deref());

        if items.is_empty() {
            anyhow::bail!("No worktrees found");
        }

        // Use fzf to select (move out the options Vec; the rest of ctx is unused after this).
        let picker = integrations::fzf::RealFzfPicker::new(ctx.config.integrations.fzf.options);
        let selected = picker.pick(&items, false)?;

        if selected.is_empty() {
            // User pressed Esc or no selection
            return Ok(());
        }

        println!("{}", normalize_absolute_path(&PathBuf::from(&selected[0])));
        return Ok(());
    };
    let name = name.as_str();

    let list = WorktreeList::parse(&stdout, None);

    // "@" and the main worktree's branch: name resolution skips main because `rm` must not
    // reach it, but `cd` has to.
    let main_entry = list
        .main()
        .context("git worktree list returned no entries")?;
    let main_path = normalize_absolute_path(&PathBuf::from(&main_entry.path));
    if name == "@" || main_entry.branch.as_deref() == Some(name) {
        println!("{main_path}");
        return Ok(());
    }

    match list.resolve_name(name, root.as_deref()) {
        NameResolution::Found(entry) => {
            println!("{}", normalize_absolute_path(&PathBuf::from(&entry.path)));
            Ok(())
        }
        NameResolution::Ambiguous(entries) => Err(ambiguous_name_error(name, &entries)),
        NameResolution::NotFound
            if canonicalize_allow_missing(Path::new(name))
                == canonicalize_allow_missing(Path::new(&main_entry.path)) =>
        {
            println!("{main_path}");
            Ok(())
        }
        NameResolution::NotFound => anyhow::bail!("Worktree not found: {name}"),
    }
}
