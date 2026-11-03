use std::fmt;
use std::io::{self, Write};

#[derive(Debug)]
pub struct StdoutError(pub io::Error);

impl fmt::Display for StdoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Failed to write stdout")
    }
}

impl std::error::Error for StdoutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

pub fn write_stdout(args: fmt::Arguments<'_>) -> anyhow::Result<()> {
    io::stdout()
        .lock()
        .write_fmt(args)
        .map_err(|error| StdoutError(error).into())
}
