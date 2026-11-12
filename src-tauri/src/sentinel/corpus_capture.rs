//! Opt-in capture of a session's committed windows for the Sentinel
//! measurement corpus (docs/sentinel-corpus-runbook.md).
//!
//! Off unless the process was started with [`CAPTURE_DIR_ENV`] set, which a
//! maintainer does for a scripted red-team session or for a clean session
//! whose participant has signed the separate capture consent. Ordinary use
//! never sets it, so ordinary sessions are never captured. Each committed
//! window is appended as one [`CorpusWindow`] JSON line to
//! `<dir>/<session_id>.jsonl`; `integrity_evaluate entry` turns that file
//! into a corpus entry. Nothing leaves the device.

use std::io::Write;
use std::path::Path;

use crate::sentinel::scoring::measurement::CorpusWindow;

/// Directory to capture into. Unset means no capture.
pub const CAPTURE_DIR_ENV: &str = "ALEXANDRIA_SENTINEL_CAPTURE_DIR";

/// Appends a committed window to the session's capture file when capture is
/// on. A capture failure is logged and never fails the snapshot.
pub(crate) fn capture(session_id: &str, window: impl FnOnce() -> CorpusWindow) {
    let Some(dir) = std::env::var_os(CAPTURE_DIR_ENV) else {
        return;
    };
    if let Err(error) = capture_to(Path::new(&dir), session_id, &window()) {
        log::warn!("sentinel corpus capture failed: {error}");
    }
}

pub(crate) fn capture_to(
    dir: &Path,
    session_id: &str,
    window: &CorpusWindow,
) -> Result<(), String> {
    let safe = !session_id.is_empty()
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !safe {
        return Err(format!("session id {session_id:?} is not a safe file name"));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let line = serde_json::to_string(window).map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{session_id}.jsonl")))
        .map_err(|e| e.to_string())?;
    writeln!(file, "{line}").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sentinel::scoring::{ClipboardFeatures, WindowFeatures};

    #[test]
    fn captured_windows_replay_to_the_flags_the_session_committed() {
        let dir = tempfile::tempdir().unwrap();
        let pasted = CorpusWindow {
            features: Some(WindowFeatures {
                clipboard: ClipboardFeatures {
                    paste_events: 1,
                    pasted_chars: 800,
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        let backend = CorpusWindow {
            backend_flags: vec!["mcp_call_during_attempt".into()],
            ..Default::default()
        };
        capture_to(dir.path(), "session-1", &pasted).unwrap();
        capture_to(dir.path(), "session-1", &backend).unwrap();

        let text = std::fs::read_to_string(dir.path().join("session-1.jsonl")).unwrap();
        let flags: Vec<Vec<String>> = text
            .lines()
            .map(|l| serde_json::from_str::<CorpusWindow>(l).unwrap().flags())
            .collect();
        assert_eq!(
            flags,
            [vec!["paste_detected"], vec!["mcp_call_during_attempt"]]
        );
    }

    #[test]
    fn a_session_id_that_is_not_a_plain_file_name_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let window = CorpusWindow::default();
        assert!(capture_to(dir.path(), "../escape", &window).is_err());
        assert!(capture_to(dir.path(), "", &window).is_err());
    }
}
