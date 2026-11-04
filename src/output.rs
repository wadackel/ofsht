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

#[derive(Debug)]
pub struct ReportedError;

impl fmt::Display for ReportedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Error reported before cleanup")
    }
}

pub fn is_broken_stdout(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<StdoutError>()
        .is_some_and(|error| error.0.kind() == io::ErrorKind::BrokenPipe)
}

pub fn report_before_cleanup(error: anyhow::Error) -> anyhow::Error {
    if is_broken_stdout(&error) {
        return error;
    }
    // Caller-window cleanup can terminate this process before main receives the error.
    eprintln!("Error: {error:?}");
    error.context(ReportedError)
}

pub fn write_stdout(args: fmt::Arguments<'_>) -> anyhow::Result<()> {
    io::stdout()
        .lock()
        .write_fmt(args)
        .map_err(|error| StdoutError(error).into())
}
