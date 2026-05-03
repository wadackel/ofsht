//! Stdin input helpers
//!
//! When stdin is not a TTY (i.e. piped or redirected), commands can read positional
//! values from stdin instead of CLI arguments. The TTY check ensures interactive
//! sessions keep their existing behavior (errors / fzf fallbacks).

use anyhow::{Context, Result};
use std::io::{self, BufRead, BufReader, IsTerminal, Read};

/// Read the first non-empty trimmed line from stdin when it is piped/redirected.
///
/// Returns `Ok(None)` when stdin is a TTY or contains no non-empty lines.
///
/// # Errors
/// Returns an error if reading from stdin fails for an I/O reason other than EOF.
pub fn try_read_stdin_first() -> Result<Option<String>> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }
    read_first_from(stdin.lock())
}

/// Read all non-empty trimmed lines from stdin when it is piped/redirected.
///
/// Returns `Ok(vec![])` when stdin is a TTY or contains no non-empty lines.
///
/// # Errors
/// Returns an error if reading from stdin fails for an I/O reason other than EOF.
pub fn try_read_stdin_lines() -> Result<Vec<String>> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(vec![]);
    }
    read_lines_from(stdin.lock())
}

fn read_first_from<R: Read>(reader: R) -> Result<Option<String>> {
    // Read line by line and short-circuit on the first non-empty trimmed line.
    // This avoids blocking on producers that keep the pipe open after writing
    // a single line (e.g., `tail -f`).
    let buf = BufReader::new(reader);
    for line in buf.lines() {
        let line = line.context("Failed to read from stdin")?;
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }
    Ok(None)
}

fn read_lines_from<R: Read>(reader: R) -> Result<Vec<String>> {
    // Multi-target callers (e.g., `rm`) need every non-empty line, so we drain
    // the pipe until EOF.
    let buf = BufReader::new(reader);
    let mut out = Vec::new();
    for line in buf.lines() {
        let line = line.context("Failed to read from stdin")?;
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_first_from_single_line() {
        let result = read_first_from(Cursor::new(b"feat\n")).unwrap();
        assert_eq!(result, Some("feat".to_string()));
    }

    #[test]
    fn read_first_from_multiline_returns_first() {
        let result = read_first_from(Cursor::new(b"a\nb\nc\n")).unwrap();
        assert_eq!(result, Some("a".to_string()));
    }

    #[test]
    fn read_first_from_empty() {
        let result = read_first_from(Cursor::new(b"")).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn read_first_from_blank_only() {
        let result = read_first_from(Cursor::new(b"\n\n  \n")).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn read_first_from_trims_whitespace() {
        let result = read_first_from(Cursor::new(b"  feat  \n")).unwrap();
        assert_eq!(result, Some("feat".to_string()));
    }

    #[test]
    fn read_first_from_skips_leading_blank_lines() {
        let result = read_first_from(Cursor::new(b"\n\nfeat\nother\n")).unwrap();
        assert_eq!(result, Some("feat".to_string()));
    }

    #[test]
    fn read_first_from_short_circuits_before_eof() {
        // Custom reader that yields a non-empty line and then panics if read
        // again, proving the helper does not wait for EOF.
        struct OneLineThenPanic {
            yielded: bool,
        }
        impl std::io::Read for OneLineThenPanic {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                assert!(
                    !self.yielded,
                    "read_first_from must not read past the first non-empty line"
                );
                self.yielded = true;
                let payload = b"feat\n";
                buf[..payload.len()].copy_from_slice(payload);
                Ok(payload.len())
            }
        }

        let result = read_first_from(OneLineThenPanic { yielded: false }).unwrap();
        assert_eq!(result, Some("feat".to_string()));
    }

    #[test]
    fn read_lines_from_multiline() {
        let result = read_lines_from(Cursor::new(b"a\nb\nc\n")).unwrap();
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn read_lines_from_filters_blank_lines() {
        let result = read_lines_from(Cursor::new(b"a\n\nb\n\n  \nc\n")).unwrap();
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn read_lines_from_empty() {
        let result = read_lines_from(Cursor::new(b"")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn read_lines_from_blank_only() {
        let result = read_lines_from(Cursor::new(b"\n\n  \n")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn read_lines_from_trims_whitespace() {
        let result = read_lines_from(Cursor::new(b"  a  \n  b  \n")).unwrap();
        assert_eq!(result, vec!["a", "b"]);
    }
}
