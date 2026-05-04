#![allow(clippy::missing_errors_doc)]
mod executor;
mod files;
mod output;
mod runner;
mod symlink;

pub use executor::{execute_hooks_lenient_with_mp, execute_hooks_with_mp};
pub use output::emit_line;
