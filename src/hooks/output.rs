#![allow(clippy::missing_errors_doc)]
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

/// Emit a static line into a `MultiProgress`, preserving bar ordering in TTY mode.
///
/// In TTY mode, creates a bar that immediately finishes with the message,
/// keeping it positioned correctly relative to active spinners.
/// In non-TTY mode, simply prints to stderr.
#[allow(clippy::missing_panics_doc)]
pub fn emit_line(mp: &MultiProgress, is_tty: bool, msg: String) {
    if is_tty {
        let bar = mp.add(ProgressBar::new(0));
        // set_style MUST be called before finish_with_message —
        // the default bar style has no {msg} placeholder.
        bar.set_style(ProgressStyle::with_template("{msg}").unwrap());
        bar.finish_with_message(msg);
    } else {
        eprintln!("{msg}");
    }
}

/// Format duration for display (only if >= 100ms)
pub(super) fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 100 {
        String::new()
    } else if millis < 1000 {
        format!("({millis}ms)")
    } else {
        let secs = duration.as_secs_f64();
        format!("({secs:.1}s)")
    }
}
