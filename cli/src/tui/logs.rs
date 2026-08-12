//! In-memory log capture for the TUI.
//!
//! `app_lib` emits several hundred `log` records — the vault, the credential
//! impls, the database layer — and the CLI installs no logger, so every one of
//! them is currently discarded. That is fine for one-shot subcommands, whose
//! output is the answer. It is a waste in the TUI, where "why did that fail?"
//! is the question and the answer was already written and thrown away.
//!
//! A logger cannot write to stderr here: the alternate screen is showing, and
//! a stray line would corrupt the frame. So records go into a bounded ring
//! buffer that the Logs tab renders.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// How many records to keep.
///
/// Bounded because a TUI session can run for hours and nothing here is ever
/// flushed to disk. Old records are dropped rather than new ones rejected —
/// when debugging, the most recent lines are the ones that matter.
const CAPACITY: usize = 5_000;

/// One captured record.
#[derive(Debug, Clone)]
pub struct Entry {
    pub level: Level,
    /// Module path the record came from, trimmed to something readable.
    pub target: String,
    pub message: String,
    /// Local wall-clock time, `HH:MM:SS.mmm`.
    pub time: String,
}

fn buffer() -> &'static Mutex<VecDeque<Entry>> {
    static BUFFER: OnceLock<Mutex<VecDeque<Entry>>> = OnceLock::new();
    BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(CAPACITY)))
}

struct Capture;

impl Log for Capture {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        // Capture everything the max level lets through; the view filters.
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let entry = Entry {
            level: record.level(),
            target: shorten_target(record.target()),
            message: record.args().to_string(),
            time: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
        };

        // A poisoned lock must not take the app down: logging is diagnostic,
        // and panicking inside the logger would be a poor trade.
        if let Ok(mut buf) = buffer().lock() {
            if buf.len() == CAPACITY {
                buf.pop_front();
            }
            buf.push_back(entry);
        }
    }

    fn flush(&self) {}
}

/// Keep the last two path segments: `app_lib::commands::credentials` reads as
/// `commands::credentials`, which is enough to place a line without eating the
/// width the message needs.
fn shorten_target(target: &str) -> String {
    let parts: Vec<&str> = target.split("::").collect();
    if parts.len() <= 2 {
        return target.to_string();
    }
    parts[parts.len() - 2..].join("::")
}

/// Start capturing. Idempotent, and never fatal.
///
/// `Debug` rather than `Trace`: trace is where dependencies emit per-packet
/// and per-row noise, which would push the interesting lines out of a bounded
/// buffer. The view can filter further but cannot recover what was never
/// captured, so this is the one level choice that matters.
pub fn install() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        // Another logger already being set is not an error worth surfacing —
        // it just means the Logs tab stays empty.
        let _ = log::set_boxed_logger(Box::new(Capture));
        log::set_max_level(LevelFilter::Debug);
    });
}

/// Snapshot the records at or above `minimum`, oldest first.
pub fn entries(minimum: LevelFilter) -> Vec<Entry> {
    let Ok(buf) = buffer().lock() else {
        return Vec::new();
    };
    buf.iter()
        .filter(|e| e.level.to_level_filter() <= minimum)
        .cloned()
        .collect()
}

/// Total captured, ignoring any filter.
pub fn len() -> usize {
    buffer().lock().map(|b| b.len()).unwrap_or(0)
}

pub fn clear() {
    if let Ok(mut buf) = buffer().lock() {
        buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(level: Level, message: &str) {
        Capture.log(
            &Record::builder()
                .level(level)
                .target("app_lib::commands::credentials")
                .args(format_args!("{}", message))
                .build(),
        );
    }

    #[test]
    fn targets_are_shortened_to_the_last_two_segments() {
        assert_eq!(
            shorten_target("app_lib::commands::credentials"),
            "commands::credentials"
        );
        // Already short enough: left alone rather than mangled.
        assert_eq!(shorten_target("alexandria"), "alexandria");
        assert_eq!(shorten_target("a::b"), "a::b");
    }

    #[test]
    fn the_buffer_drops_the_oldest_rather_than_growing() {
        clear();
        for i in 0..CAPACITY + 25 {
            push(Level::Info, &format!("line {i}"));
        }
        assert_eq!(len(), CAPACITY, "the buffer must stay bounded");

        let all = entries(LevelFilter::Trace);
        // The newest survived and the oldest went, which is the right way
        // round for debugging.
        assert!(all
            .last()
            .unwrap()
            .message
            .contains(&format!("line {}", CAPACITY + 24)));
        assert!(!all.iter().any(|e| e.message == "line 0"));
        clear();
    }

    #[test]
    fn filtering_keeps_the_levels_at_or_above_the_threshold() {
        clear();
        push(Level::Error, "an error");
        push(Level::Warn, "a warning");
        push(Level::Info, "some info");
        push(Level::Debug, "a detail");

        let warn_and_worse = entries(LevelFilter::Warn);
        assert_eq!(warn_and_worse.len(), 2);
        assert!(warn_and_worse.iter().all(|e| e.level <= Level::Warn));

        assert_eq!(entries(LevelFilter::Error).len(), 1);
        assert_eq!(entries(LevelFilter::Debug).len(), 4);
        // Off means nothing, not everything — a filter that inverted here
        // would flood the view at the moment someone tried to quieten it.
        assert_eq!(entries(LevelFilter::Off).len(), 0);
        clear();
    }

    #[test]
    fn entries_are_returned_oldest_first() {
        clear();
        push(Level::Info, "first");
        push(Level::Info, "second");
        let all = entries(LevelFilter::Trace);
        assert_eq!(all[0].message, "first");
        assert_eq!(all[1].message, "second");
        clear();
    }

    #[test]
    fn a_record_keeps_its_level_target_and_message() {
        clear();
        push(Level::Warn, "vault locked");
        let entry = &entries(LevelFilter::Trace)[0];
        assert_eq!(entry.level, Level::Warn);
        assert_eq!(entry.target, "commands::credentials");
        assert_eq!(entry.message, "vault locked");
        // HH:MM:SS.mmm
        assert_eq!(entry.time.len(), 12, "got {}", entry.time);
        clear();
    }
}
