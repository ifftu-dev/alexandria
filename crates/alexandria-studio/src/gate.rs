//! AI assistance is unavailable while an assessment or interview check runs.
//!
//! Every attempt and every interview check holds a live integrity session
//! (`integrity_sessions.ended_at IS NULL`) for as long as it is open, so that
//! row is the single source of truth for "an assessment is in progress". Every
//! AI entry point asks this module before doing anything: the lesson tutor,
//! Studio runs, external assistants through the broker, and global chat.
//!
//! Hiding the chat panel is presentation. This check is the control.
//!
//! A session left open by a crash has no sweep that closes it, so a session
//! only gates for [`MAX_GATED_SESSION_HOURS`] after it started. An attempt
//! still running past that is unusual enough for grading to treat on its own.

use rusqlite::Connection;

use crate::{Error, Result};

/// How long an unended integrity session keeps AI assistance paused.
pub const MAX_GATED_SESSION_HOURS: u32 = 6;

/// Whether an assessment attempt or interview check is in progress.
pub fn assessment_in_progress(db: &Connection) -> Result<bool> {
    let window = format!("-{MAX_GATED_SESSION_HOURS} hours");
    Ok(db.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM integrity_sessions
              WHERE ended_at IS NULL
                AND julianday(started_at) > julianday('now', ?1))",
        [window],
        |row| row.get(0),
    )?)
}

/// The integrity sessions that currently gate AI assistance.
pub fn live_session_ids(db: &Connection) -> Result<Vec<String>> {
    let window = format!("-{MAX_GATED_SESSION_HOURS} hours");
    let mut stmt = db.prepare(
        "SELECT id FROM integrity_sessions
          WHERE ended_at IS NULL
            AND julianday(started_at) > julianday('now', ?1)
          ORDER BY started_at",
    )?;
    let ids = stmt
        .query_map([window], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

/// Fails with [`Error::AssessmentActive`] while an assessment is in progress.
pub fn require_no_assessment(db: &Connection) -> Result<()> {
    if assessment_in_progress(db)? {
        Err(Error::AssessmentActive)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE integrity_sessions (
                 id TEXT PRIMARY KEY,
                 status TEXT NOT NULL DEFAULT 'active',
                 started_at TEXT NOT NULL DEFAULT (datetime('now')),
                 ended_at TEXT,
                 purpose TEXT NOT NULL DEFAULT 'assessment');",
        )
        .unwrap();
        db
    }

    #[test]
    fn a_live_session_pauses_assistance_until_it_ends() {
        let db = db();
        assert!(require_no_assessment(&db).is_ok());
        db.execute("INSERT INTO integrity_sessions(id) VALUES ('s')", [])
            .unwrap();
        assert!(matches!(
            require_no_assessment(&db),
            Err(Error::AssessmentActive)
        ));
        db.execute(
            "UPDATE integrity_sessions SET ended_at = datetime('now'), status = 'completed'",
            [],
        )
        .unwrap();
        assert!(require_no_assessment(&db).is_ok());
    }

    #[test]
    fn interview_checks_gate_too() {
        let db = db();
        db.execute(
            "INSERT INTO integrity_sessions(id, purpose) VALUES ('i', 'interview')",
            [],
        )
        .unwrap();
        assert!(assessment_in_progress(&db).unwrap());
    }

    #[test]
    fn a_session_abandoned_by_a_crash_stops_gating_after_the_window() {
        let db = db();
        db.execute(
            "INSERT INTO integrity_sessions(id, started_at) VALUES ('old', datetime('now', '-7 hours'))",
            [],
        )
        .unwrap();
        assert!(!assessment_in_progress(&db).unwrap());
        // ISO-8601 timestamps written by other paths compare correctly too.
        db.execute(
            "INSERT INTO integrity_sessions(id, started_at)
             VALUES ('iso', strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-5 hours'))",
            [],
        )
        .unwrap();
        assert!(assessment_in_progress(&db).unwrap());
    }

    #[test]
    fn the_refusal_names_the_reason_without_detail() {
        assert_eq!(
            Error::AssessmentActive.to_string(),
            "assessment_active: AI assistance is paused while an assessment is in progress"
        );
    }
}
