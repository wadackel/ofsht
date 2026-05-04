// Internal-only library surface. Modules are exposed solely so that the binary's
// own integration tests and doctests can reach them; this crate is published
// primarily as a CLI binary, not as a stable library API. Signatures here may
// change in any release without a major version bump.
#![allow(clippy::literal_string_with_formatting_args)]
pub mod cli;
pub mod color;
pub mod config;
pub mod hooks;
pub mod path_utils;
pub mod service;
pub mod stdin;

// Integration modules
pub mod integrations;

// Command modules — internal handlers, not a stable API surface.
#[doc(hidden)]
pub mod commands;

// Domain modules
pub mod domain;
