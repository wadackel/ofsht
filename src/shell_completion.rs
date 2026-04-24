//! Custom shell completion adapters that hide flag candidates unless the current word starts with `-`.
//!
//! Wraps `clap_complete`'s built-in `EnvCompleter` implementations (Bash/Zsh/Fish) and post-filters
//! the candidate list returned from `clap_complete::engine::complete`, removing entries whose value
//! starts with `-` when the user has not yet typed a dash.

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::Path;

use clap::Command;
use clap_complete::engine::{complete, CompletionCandidate};
use clap_complete::env::{Bash, EnvCompleter, Fish, Zsh};

/// Drop flag candidates (values starting with `-`) unless the current word also starts with `-`.
fn filter_flag_candidates(
    completions: Vec<CompletionCandidate>,
    current_word: &OsStr,
) -> Vec<CompletionCandidate> {
    if current_word.to_string_lossy().starts_with('-') {
        return completions;
    }
    completions
        .into_iter()
        .filter(|c| !c.get_value().to_string_lossy().starts_with('-'))
        .collect()
}

/// Run `engine::complete` and apply the flag filter against the current word at `args[index]`.
fn filtered_candidates(
    cmd: &mut Command,
    args: Vec<OsString>,
    index: usize,
    current_dir: Option<&Path>,
) -> io::Result<Vec<CompletionCandidate>> {
    let current_word = args.get(index).cloned().unwrap_or_default();
    let completions = complete(cmd, args, index, current_dir)?;
    Ok(filter_flag_candidates(completions, &current_word))
}

/// Bash adapter: identical registration to the built-in, filtered completion output.
pub struct FilteredBash;

impl EnvCompleter for FilteredBash {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn is(&self, name: &str) -> bool {
        name == "bash"
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        Bash.write_registration(var, name, bin, completer, buf)
    }

    fn write_complete(
        &self,
        cmd: &mut Command,
        args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        let index: usize = std::env::var("_CLAP_COMPLETE_INDEX")
            .ok()
            .and_then(|i| i.parse().ok())
            .unwrap_or_default();
        let ifs: Option<String> = std::env::var("_CLAP_IFS").ok();
        let filtered = filtered_candidates(cmd, args, index, current_dir)?;
        for (i, candidate) in filtered.iter().enumerate() {
            if i != 0 {
                write!(buf, "{}", ifs.as_deref().unwrap_or("\n"))?;
            }
            write!(buf, "{}", candidate.get_value().to_string_lossy())?;
        }
        Ok(())
    }
}

/// Zsh adapter: identical registration, filtered output, preserves `value:help` display format.
pub struct FilteredZsh;

impl EnvCompleter for FilteredZsh {
    fn name(&self) -> &'static str {
        "zsh"
    }

    fn is(&self, name: &str) -> bool {
        name == "zsh"
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        Zsh.write_registration(var, name, bin, completer, buf)
    }

    fn write_complete(
        &self,
        cmd: &mut Command,
        args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        let index: usize = std::env::var("_CLAP_COMPLETE_INDEX")
            .ok()
            .and_then(|i| i.parse().ok())
            .unwrap_or_default();
        let ifs: Option<String> = std::env::var("_CLAP_IFS").ok();

        // Match built-in Zsh: if current word is one beyond the last arg, pad with "".
        // Source: clap_complete-4.5.60/src/env/shells.rs:410-414
        let mut args = args;
        if args.len() == index {
            args.push(OsString::new());
        }

        let filtered = filtered_candidates(cmd, args, index, current_dir)?;
        for (i, candidate) in filtered.iter().enumerate() {
            if i != 0 {
                write!(buf, "{}", ifs.as_deref().unwrap_or("\n"))?;
            }
            write!(
                buf,
                "{}",
                escape_zsh_value(&candidate.get_value().to_string_lossy())
            )?;
            if let Some(help) = candidate.get_help() {
                write!(
                    buf,
                    ":{}",
                    escape_zsh_help(help.to_string().lines().next().unwrap_or_default())
                )?;
            }
        }
        Ok(())
    }
}

/// Zsh escape: backslash and colon are special within `value:help` records.
/// Source: clap_complete-4.5.60/src/env/shells.rs:440-442
fn escape_zsh_value(s: &str) -> String {
    s.replace('\\', "\\\\").replace(':', "\\:")
}

/// Zsh help escape: only backslash needs doubling (colon already split by caller).
/// Source: clap_complete-4.5.60/src/env/shells.rs:445-447
fn escape_zsh_help(s: &str) -> String {
    s.replace('\\', "\\\\")
}

/// Fish adapter: identical registration, filtered output, `value\thelp\n` per record.
pub struct FilteredFish;

impl EnvCompleter for FilteredFish {
    fn name(&self) -> &'static str {
        "fish"
    }

    fn is(&self, name: &str) -> bool {
        name == "fish"
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        Fish.write_registration(var, name, bin, completer, buf)
    }

    fn write_complete(
        &self,
        cmd: &mut Command,
        args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        // Match built-in Fish: current word is the last arg.
        // Source: clap_complete-4.5.60/src/env/shells.rs:237
        let index = args.len().saturating_sub(1);
        let filtered = filtered_candidates(cmd, args, index, current_dir)?;
        for candidate in &filtered {
            write!(buf, "{}", candidate.get_value().to_string_lossy())?;
            if let Some(help) = candidate.get_help() {
                write!(
                    buf,
                    "\t{}",
                    help.to_string().lines().next().unwrap_or_default()
                )?;
            }
            writeln!(buf)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::CommandFactory;
    use serial_test::serial;
    use std::ffi::OsString;

    fn cand(value: &str) -> CompletionCandidate {
        CompletionCandidate::new(value)
    }

    #[test]
    fn filter_drops_flags_when_current_word_empty() {
        let completions = vec![
            cand("@"),
            cand("feature"),
            cand("--color"),
            cand("--help"),
            cand("-v"),
        ];
        let filtered = filter_flag_candidates(completions, OsStr::new(""));
        let values: Vec<&str> = filtered
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(values, vec!["@", "feature"]);
    }

    #[test]
    fn filter_keeps_all_when_current_word_is_dash() {
        let completions = vec![cand("@"), cand("--color"), cand("-v")];
        let filtered = filter_flag_candidates(completions, OsStr::new("-"));
        let values: Vec<&str> = filtered
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(values, vec!["@", "--color", "-v"]);
    }

    #[test]
    fn filter_keeps_all_when_current_word_is_long_prefix() {
        let completions = vec![cand("@"), cand("--color"), cand("--verbose")];
        let filtered = filter_flag_candidates(completions, OsStr::new("--c"));
        let values: Vec<&str> = filtered
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(values, vec!["@", "--color", "--verbose"]);
    }

    #[test]
    fn filter_drops_dashes_when_current_word_is_non_dash_text() {
        let completions = vec![cand("@"), cand("feature"), cand("--color"), cand("-v")];
        let filtered = filter_flag_candidates(completions, OsStr::new("foo"));
        let values: Vec<&str> = filtered
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert_eq!(values, vec!["@", "feature"]);
    }

    fn values_of(candidates: &[CompletionCandidate]) -> Vec<String> {
        candidates
            .iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    fn args(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    #[serial]
    fn filtered_candidates_cd_empty_excludes_flags() {
        let mut cmd = Cli::command();
        let result = filtered_candidates(&mut cmd, args(&["ofsht", "cd", ""]), 2, None)
            .expect("filtered_candidates must succeed");
        let values = values_of(&result);
        assert!(values.iter().any(|v| v == "@"), "expected @ in {values:?}");
        assert!(
            !values.iter().any(|v| v == "--color"),
            "--color must be filtered in {values:?}"
        );
        assert!(
            !values.iter().any(|v| v == "--help"),
            "--help must be filtered in {values:?}"
        );
        assert!(
            !values.iter().any(|v| v == "--verbose"),
            "--verbose must be filtered in {values:?}"
        );
    }

    #[test]
    #[serial]
    fn filtered_candidates_cd_dash_includes_flags() {
        let mut cmd = Cli::command();
        let result = filtered_candidates(&mut cmd, args(&["ofsht", "cd", "-"]), 2, None)
            .expect("filtered_candidates must succeed");
        let values = values_of(&result);
        assert!(
            values.iter().any(|v| v == "--color"),
            "--color must appear when dash is typed in {values:?}"
        );
    }

    #[test]
    fn filtered_shell_names_and_matches() {
        assert_eq!(FilteredBash.name(), "bash");
        assert!(FilteredBash.is("bash"));
        assert!(!FilteredBash.is("zsh"));

        assert_eq!(FilteredZsh.name(), "zsh");
        assert!(FilteredZsh.is("zsh"));
        assert!(!FilteredZsh.is("bash"));

        assert_eq!(FilteredFish.name(), "fish");
        assert!(FilteredFish.is("fish"));
        assert!(!FilteredFish.is("bash"));
    }

    #[test]
    fn zsh_escape_value_doubles_backslash_and_escapes_colon() {
        assert_eq!(escape_zsh_value("plain"), "plain");
        assert_eq!(escape_zsh_value("a:b"), "a\\:b");
        assert_eq!(escape_zsh_value("a\\b"), "a\\\\b");
        assert_eq!(escape_zsh_value("a\\b:c"), "a\\\\b\\:c");
    }

    #[test]
    fn zsh_escape_help_doubles_backslash_only() {
        assert_eq!(escape_zsh_help("plain"), "plain");
        assert_eq!(escape_zsh_help("a:b"), "a:b");
        assert_eq!(escape_zsh_help("a\\b"), "a\\\\b");
    }

    #[test]
    #[serial]
    fn filtered_candidates_option_value_regression() {
        // Option value completion path must not be affected by the filter — `--color` takes
        // a ValueEnum with PossibleValues auto/always/never; none of them start with `-`.
        let mut cmd = Cli::command();
        let result = filtered_candidates(&mut cmd, args(&["ofsht", "--color", ""]), 2, None)
            .expect("filtered_candidates must succeed");
        let values = values_of(&result);
        assert!(
            values.iter().any(|v| v == "auto"),
            "auto must be present in {values:?}"
        );
        assert!(
            values.iter().any(|v| v == "always"),
            "always must be present in {values:?}"
        );
        assert!(
            values.iter().any(|v| v == "never"),
            "never must be present in {values:?}"
        );
        assert!(
            !values.iter().any(|v| v.starts_with("--")),
            "no `--` prefixed value must appear in option-value path: {values:?}"
        );
    }
}
