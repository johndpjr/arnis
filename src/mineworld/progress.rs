//! `MINEWORLD_PROGRESS=1`: progress lines for a front end driving the command line.
//!
//! Arnis reports its progress (a percentage and a stage message) to its desktop GUI; the
//! command-line build drops those reports. With the environment variable set, each report is
//! printed to stdout as one line, which mineworld's editor reads to show a progress bar:
//!
//! ```text
//! @progress 42.5 Generating area...
//! @progress - Extended build height: ...     (a message that leaves the bar alone)
//! ```
//!
//! Reports repeating the previous line (to a tenth of a percent) are skipped.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// Arnis's "message only" progress value (`progress::MESSAGE_ONLY`).
const MESSAGE_ONLY: f64 = -1.0;

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("MINEWORLD_PROGRESS").is_some_and(|v| v == "1"))
}

/// The line for a report.
fn line(progress: f64, message: &str) -> String {
    let message = message.replace(['\n', '\r'], " ");
    let message = message.trim();
    let pct = if progress <= MESSAGE_ONLY {
        "-".to_string()
    } else {
        format!("{:.1}", progress.clamp(0.0, 100.0))
    };
    if message.is_empty() {
        format!("@progress {pct}")
    } else {
        format!("@progress {pct} {message}")
    }
}

static LAST: Mutex<String> = Mutex::new(String::new());

/// Prints a progress report, when a front end asked for them.
pub fn emit(progress: f64, message: &str) {
    if !enabled() {
        return;
    }
    let l = line(progress, message);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if *last == l {
        return;
    }
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{l}");
    let _ = out.flush();
    *last = l;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_carry_a_percentage_or_a_dash() {
        assert_eq!(line(42.04, "Generating area..."), "@progress 42.0 Generating area...");
        assert_eq!(line(120.0, ""), "@progress 100.0");
        assert_eq!(line(MESSAGE_ONLY, "a\nb"), "@progress - a b");
    }
}
