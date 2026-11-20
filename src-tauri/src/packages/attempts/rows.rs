//! The `package_attempts` and `package_activity_state` tables.

use std::collections::BTreeMap;

use alexandria_studio::package::progress::ActivityOutcome;
use rusqlite::{params, Connection, OptionalExtension, Row};

use super::AttemptError;
use crate::crypto::hash::entity_id;
use crate::domain::course_document::PACKAGE_ELEMENT_TYPE;
use crate::domain::package_attempt::{PackageAttempt, PackageAttemptStatus};

const ATTEMPT_COLUMNS: &str = "id, enrollment_id, element_id, package_hash, attempt_number, \
                               status, started_at, ended_at, updated_at";

/// The package hash a package element of the enrolment's course carries.
/// Refused when the element is not in that course, is not a package element
/// or its package is not stored on this device.
pub(super) fn package_element(
    conn: &Connection,
    enrollment_id: &str,
    element_id: &str,
) -> Result<String, AttemptError> {
    let found: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT ce.element_type, ce.content_cid FROM course_elements ce \
             JOIN course_chapters cc ON cc.id = ce.chapter_id \
             JOIN enrollments e ON e.course_id = cc.course_id \
             WHERE e.id = ?1 AND ce.id = ?2",
            params![enrollment_id, element_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((element_type, content_cid)) = found else {
        return Err(AttemptError::NotFound(format!(
            "element {element_id} under enrolment {enrollment_id}"
        )));
    };
    if element_type != PACKAGE_ELEMENT_TYPE {
        return Err(AttemptError::NotAPackage(format!(
            "element {element_id} is a {element_type} element"
        )));
    }
    let hash = content_cid
        .ok_or_else(|| AttemptError::NotAPackage(format!("element {element_id} has no package")))?;
    let stored: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM packages WHERE hash = ?1)",
            params![hash],
            |row| row.get(0),
        )
        .map_err(AttemptError::from)?;
    if !stored {
        return Err(AttemptError::NotAPackage(format!(
            "package {hash} is not stored on this device"
        )));
    }
    Ok(hash)
}

/// The latest attempt at the element that a fresh attempt has not closed.
pub(super) fn current(
    conn: &Connection,
    enrollment_id: &str,
    element_id: &str,
) -> Result<Option<PackageAttempt>, AttemptError> {
    conn.query_row(
        &format!(
            "SELECT {ATTEMPT_COLUMNS} FROM package_attempts \
             WHERE enrollment_id = ?1 AND element_id = ?2 AND status <> 'closed' \
             ORDER BY attempt_number DESC LIMIT 1"
        ),
        params![enrollment_id, element_id],
        from_row,
    )
    .optional()?
    .transpose()
}

pub(super) fn get(conn: &Connection, attempt_id: &str) -> Result<PackageAttempt, AttemptError> {
    conn.query_row(
        &format!("SELECT {ATTEMPT_COLUMNS} FROM package_attempts WHERE id = ?1"),
        params![attempt_id],
        from_row,
    )
    .optional()?
    .transpose()?
    .ok_or_else(|| AttemptError::NotFound(format!("attempt {attempt_id}")))
}

/// Starts the next attempt at the element.
pub(super) fn start(
    conn: &Connection,
    enrollment_id: &str,
    element_id: &str,
    package_hash: &str,
) -> Result<PackageAttempt, AttemptError> {
    let number: i64 = conn.query_row(
        "SELECT COALESCE(MAX(attempt_number), 0) + 1 FROM package_attempts \
         WHERE enrollment_id = ?1 AND element_id = ?2",
        params![enrollment_id, element_id],
        |row| row.get(0),
    )?;
    let id = entity_id(&[enrollment_id, element_id, &number.to_string()]);
    let now = crate::clock::now_rfc3339();
    conn.execute(
        "INSERT INTO package_attempts \
         (id, enrollment_id, element_id, package_hash, attempt_number, status, started_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'in_progress', ?6, ?6)",
        params![id, enrollment_id, element_id, package_hash, number, now],
    )?;
    get(conn, &id)
}

/// The element's progress status, if any progress is recorded.
pub(super) fn element_status(
    conn: &Connection,
    enrollment_id: &str,
    element_id: &str,
) -> Result<Option<String>, AttemptError> {
    Ok(conn
        .query_row(
            "SELECT status FROM element_progress WHERE enrollment_id = ?1 AND element_id = ?2",
            params![enrollment_id, element_id],
            |row| row.get(0),
        )
        .optional()?)
}

/// What a launch needs of one activity's stored state in one attempt.
#[derive(Debug, Clone)]
pub(super) struct StoredActivity {
    /// The last commit's run-time data.
    pub(super) runtime: serde_json::Value,
    /// Time of the finished sessions.
    pub(super) total_seconds: f64,
}

pub(super) fn activity(
    conn: &Connection,
    attempt_id: &str,
    activity_id: &str,
) -> Result<Option<StoredActivity>, AttemptError> {
    let row: Option<(String, f64)> = conn
        .query_row(
            "SELECT runtime_json, total_seconds \
             FROM package_activity_state WHERE attempt_id = ?1 AND activity_id = ?2",
            params![attempt_id, activity_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map(|(runtime, total_seconds)| {
        Ok(StoredActivity {
            runtime: serde_json::from_str(&runtime)
                .map_err(|e| AttemptError::Database(format!("activity {activity_id}: {e}")))?,
            total_seconds,
        })
    })
    .transpose()
}

/// Ends a session the content left open: its time joins the total. Returns
/// the activity's state as it then stands.
pub(super) fn finish_open_session(
    conn: &Connection,
    attempt_id: &str,
    activity_id: &str,
) -> Result<Option<StoredActivity>, AttemptError> {
    conn.execute(
        "UPDATE package_activity_state \
         SET total_seconds = total_seconds + session_seconds, session_seconds = 0 \
         WHERE attempt_id = ?1 AND activity_id = ?2 AND session_seconds > 0",
        params![attempt_id, activity_id],
    )?;
    activity(conn, attempt_id, activity_id)
}

/// What a commit writes for one activity.
pub(super) struct ActivityWrite<'a> {
    pub(super) runtime_json: &'a str,
    pub(super) complete: bool,
    pub(super) score: Option<f64>,
    pub(super) total_seconds: f64,
    pub(super) session_seconds: f64,
}

pub(super) fn put_activity(
    conn: &Connection,
    attempt_id: &str,
    activity_id: &str,
    write: &ActivityWrite<'_>,
) -> Result<(), AttemptError> {
    let now = crate::clock::now_rfc3339();
    conn.execute(
        "INSERT INTO package_activity_state \
         (attempt_id, activity_id, runtime_json, complete, score, total_seconds, session_seconds, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(attempt_id, activity_id) DO UPDATE SET \
            runtime_json = ?3, complete = ?4, score = ?5, total_seconds = ?6, \
            session_seconds = ?7, updated_at = ?8",
        params![
            attempt_id,
            activity_id,
            write.runtime_json,
            write.complete,
            write.score,
            write.total_seconds,
            write.session_seconds,
            now
        ],
    )?;
    conn.execute(
        "UPDATE package_attempts SET updated_at = ?2 WHERE id = ?1",
        params![attempt_id, now],
    )?;
    Ok(())
}

/// Every activity of the attempt that has stored state, by identifier.
pub(super) fn outcomes(
    conn: &Connection,
    attempt_id: &str,
) -> Result<BTreeMap<String, ActivityOutcome>, AttemptError> {
    let mut statement = conn.prepare(
        "SELECT activity_id, complete, score FROM package_activity_state WHERE attempt_id = ?1",
    )?;
    let rows = statement.query_map(params![attempt_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            ActivityOutcome {
                complete: row.get(1)?,
                score: row.get(2)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The reported time of every session of every attempt at the element.
pub(super) fn element_seconds(
    conn: &Connection,
    enrollment_id: &str,
    element_id: &str,
) -> Result<f64, AttemptError> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(s.total_seconds + s.session_seconds), 0) \
         FROM package_activity_state s JOIN package_attempts a ON a.id = s.attempt_id \
         WHERE a.enrollment_id = ?1 AND a.element_id = ?2",
        params![enrollment_id, element_id],
        |row| row.get(0),
    )?)
}

/// Records that the package reported completion under the attempt.
pub(super) fn mark_completed(conn: &Connection, attempt_id: &str) -> Result<(), AttemptError> {
    let now = crate::clock::now_rfc3339();
    conn.execute(
        "UPDATE package_attempts SET status = 'completed', ended_at = COALESCE(ended_at, ?2), \
         updated_at = ?2 WHERE id = ?1 AND status = 'in_progress'",
        params![attempt_id, now],
    )?;
    Ok(())
}

/// Closes an attempt a fresh one replaces.
pub(super) fn close(conn: &Connection, attempt_id: &str) -> Result<(), AttemptError> {
    let now = crate::clock::now_rfc3339();
    conn.execute(
        "UPDATE package_attempts SET status = 'closed', ended_at = COALESCE(ended_at, ?2), \
         updated_at = ?2 WHERE id = ?1",
        params![attempt_id, now],
    )?;
    Ok(())
}

/// Ends every session of the attempt the content left open.
pub(super) fn finish_open_sessions(
    conn: &Connection,
    attempt_id: &str,
) -> Result<(), AttemptError> {
    conn.execute(
        "UPDATE package_activity_state \
         SET total_seconds = total_seconds + session_seconds, session_seconds = 0 \
         WHERE attempt_id = ?1 AND session_seconds > 0",
        params![attempt_id],
    )?;
    Ok(())
}

/// The profile's display name, empty when none is set.
pub(super) fn display_name(conn: &Connection) -> Result<String, AttemptError> {
    let name: Option<Option<String>> = conn
        .query_row(
            "SELECT display_name FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(name.flatten().unwrap_or_default())
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<PackageAttempt, AttemptError>> {
    let status: String = row.get(5)?;
    let attempt = PackageAttempt {
        id: row.get(0)?,
        enrollment_id: row.get(1)?,
        element_id: row.get(2)?,
        package_hash: row.get(3)?,
        attempt_number: row.get(4)?,
        status: PackageAttemptStatus::InProgress,
        started_at: row.get(6)?,
        ended_at: row.get(7)?,
        updated_at: row.get(8)?,
    };
    Ok(match PackageAttemptStatus::parse(&status) {
        Some(status) => Ok(PackageAttempt { status, ..attempt }),
        None => Err(AttemptError::Database(format!(
            "attempt {}: unknown status {status}",
            attempt.id
        ))),
    })
}
