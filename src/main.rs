#![allow(clippy::literal_string_with_formatting_args)]
mod cli;
mod color;
mod commands;
mod config;
mod domain;
mod hooks;
mod integrations;
mod path_utils;
mod service;
mod shell_completion;
mod stdin;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::env::{CompleteEnv, Shells};

// Use shared CLI definitions from cli module
use cli::{Cli, Commands};
use shell_completion::{FilteredBash, FilteredFish, FilteredZsh};

fn main() -> Result<()> {
    // Handle dynamic completion via COMPLETE environment variable.
    // Custom shell adapters hide flag candidates unless the current word starts with `-`.
    CompleteEnv::with_factory(Cli::command)
        .shells(Shells(&[&FilteredBash, &FilteredZsh, &FilteredFish]))
        .complete();

    let cli = Cli::parse();

    // Resolve color mode from CLI flag and environment variables
    let color_mode = color::ColorMode::resolve(cli.color);

    match cli.command {
        Commands::Add {
            branch,
            start_point,
            tmux,
            no_tmux,
        } => commands::add::cmd_new(
            branch.as_deref(),
            start_point.as_deref(),
            tmux,
            no_tmux,
            color_mode,
        ),
        Commands::Create {
            branch,
            start_point,
        } => commands::create::cmd_create(branch.as_deref(), start_point.as_deref(), color_mode),
        Commands::Ls { show_path } => commands::list::cmd_list(show_path, color_mode),
        Commands::Rm { targets } => commands::rm::cmd_rm_many(&targets, color_mode),
        Commands::Cd { name } => commands::cd::cmd_goto(name.as_deref(), color_mode),
        Commands::Init {
            global,
            local,
            force,
        } => commands::init::cmd_init(global, local, force, color_mode),
        Commands::Completion { shell } => commands::completion::cmd_completion(&shell),
        Commands::ShellInit { shell } => commands::shell_init::cmd_shell_init(&shell),
        Commands::Open { pane, window } => commands::open::cmd_open(pane, window, color_mode),
        Commands::Sync { run, copy, link } => commands::sync::cmd_sync(run, copy, link, color_mode),
    }
}

// Re-export get_main_repo_root for backwards compatibility
pub use commands::common::get_main_repo_root;
