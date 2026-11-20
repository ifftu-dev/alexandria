//! What cmi5 units write on their own: the statements a unit sends beyond
//! the cmi5 defined verbs (stored in `cmi5_statements` with no verb), and
//! the xAPI documents it keeps (`xapi_documents`). The defined-verb
//! statements already have a learning record each; these do not, so the
//! activity log reaches them here.
//!
//! - **Listed** in the log as interaction-level entries, by the unit's
//!   statement id.
//! - **Counted** against the log's size limit, and the first to go past it
//!   (oldest first, never from a session still open).
//! - **Exported** with the log, rewritten for the learner: the learner's DID
//!   as the actor, statement and registration identifiers keyed for the
//!   activity log like every other record.
//! - **Deleted** with the log: deleting everything removes them, and the
//!   documents of every package with no session open.
//!
//! They are not sent to record destinations or shares (a known gap).

use alexandria_studio::xapi::{Agent, Recipient};
use rusqlite::{params, Connection};
use serde_json::Value;

use super::RecordError;
use crate::domain::learning_record::LearningRecordEntry;

/// The rows of `cmi5_statements` a unit wrote on its own.
pub(super) const UNIT_WRITTEN: &str = "verb IS NULL";

/// Not from a session still open: the endpoint may still read it.
const SESSION_CLOSED: &str = "NOT EXISTS (SELECT 1 FROM cmi5_sessions x \
                              WHERE x.id = cmi5_statements.session_id AND x.status = 'open')";

/// The log's combined listing: learning records and unit statements, newest
/// first, as the columns [`entry`] reads. Bound: `?1` limit, `?2` offset.
pub(super) const LOG_PAGE: &str = "SELECT * FROM ( \
       SELECT r.id AS id, r.kind AS kind, r.verb AS verb, r.course_id AS course_id, \
              c.title AS course_title, r.element_id AS element_id, e.title AS element_title, \
              r.activity_id AS activity_id, r.summary AS summary, r.bytes AS bytes, \
              r.recorded_at AS recorded_at, r.event_json AS body, 0 AS unit, r.seq AS seq \
       FROM learning_records r \
       LEFT JOIN courses c ON c.id = r.course_id \
       LEFT JOIN course_elements e ON e.id = r.element_id \
     UNION ALL \
       SELECT s.id, 'interaction', '', en.course_id, c.title, a.element_id, e.title, \
              s.activity_id, 0, length(s.statement_json), s.stored_at, s.statement_json, 1, s.seq \
       FROM cmi5_statements s \
       JOIN package_attempts a ON a.id = s.attempt_id \
       JOIN enrollments en ON en.id = a.enrollment_id \
       LEFT JOIN courses c ON c.id = en.course_id \
       LEFT JOIN course_elements e ON e.id = a.element_id \
       WHERE s.verb IS NULL \
   ) ORDER BY recorded_at DESC, unit DESC, seq DESC LIMIT ?1 OFFSET ?2";

/// Fills in what a unit statement's entry reads from the statement itself:
/// the verb's short name, the activity and the scaled score.
pub(super) fn complete_entry(entry: &mut LearningRecordEntry, statement: &Value) {
    let verb_id = statement["verb"]["id"].as_str().unwrap_or_default();
    entry.verb = verb_id
        .rsplit(['/', '#'])
        .next()
        .unwrap_or(verb_id)
        .to_string();
    if let Some(object) = statement["object"]["id"].as_str() {
        entry.activity_id = object.to_string();
    }
    entry.score = statement["result"]["score"]["scaled"].as_f64();
}

/// Total size and count of the unit statements.
pub(super) fn usage(conn: &Connection) -> Result<(u64, u64), RecordError> {
    let (bytes, count): (i64, i64) = conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(length(statement_json)), 0), COUNT(*) \
             FROM cmi5_statements WHERE {UNIT_WRITTEN}"
        ),
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((
        u64::try_from(bytes).unwrap_or(0),
        u64::try_from(count).unwrap_or(0),
    ))
}

/// Removes the oldest unit statements of closed sessions until `total` is
/// within `limit_bytes`; returns the new total and how many went.
pub(super) fn trim(
    conn: &Connection,
    mut total: u64,
    limit_bytes: u64,
) -> Result<(u64, usize), RecordError> {
    let oldest: Vec<(i64, i64)> = {
        let mut statement = conn.prepare(&format!(
            "SELECT seq, length(statement_json) FROM cmi5_statements \
             WHERE {UNIT_WRITTEN} AND {SESSION_CLOSED} ORDER BY seq"
        ))?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut removed = 0;
    for (seq, bytes) in oldest {
        if total <= limit_bytes {
            break;
        }
        conn.execute("DELETE FROM cmi5_statements WHERE seq = ?1", params![seq])?;
        total = total.saturating_sub(u64::try_from(bytes).unwrap_or(0));
        removed += 1;
    }
    Ok((total, removed))
}

/// Deletes the unit statement with this id, if it is one. Returns 0 or 1.
pub(super) fn delete(conn: &Connection, id: &str) -> Result<usize, RecordError> {
    Ok(conn.execute(
        &format!("DELETE FROM cmi5_statements WHERE id = ?1 AND {UNIT_WRITTEN}"),
        params![id],
    )?)
}

/// Deletes every unit statement of a closed session, and the documents of
/// every package with no session open. Returns how many statements went.
pub(super) fn delete_all(conn: &Connection) -> Result<usize, RecordError> {
    let removed = conn.execute(
        &format!("DELETE FROM cmi5_statements WHERE {UNIT_WRITTEN} AND {SESSION_CLOSED}"),
        [],
    )?;
    conn.execute(
        "DELETE FROM xapi_documents WHERE package_hash NOT IN ( \
           SELECT a.package_hash FROM cmi5_sessions x \
           JOIN package_attempts a ON a.id = x.attempt_id WHERE x.status = 'open')",
        [],
    )?;
    Ok(removed)
}

/// Every unit statement, oldest first, rewritten for `recipient`.
pub(super) fn export(conn: &Connection, recipient: &Recipient) -> Result<Vec<Value>, RecordError> {
    let rows: Vec<(String, String)> = {
        let mut statement = conn.prepare(&format!(
            "SELECT attempt_id, statement_json FROM cmi5_statements \
             WHERE {UNIT_WRITTEN} ORDER BY seq"
        ))?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    rows.into_iter()
        .map(|(attempt_id, json)| {
            let mut statement: Value = serde_json::from_str(&json)?;
            for_recipient(&mut statement, recipient, &attempt_id)?;
            Ok(statement)
        })
        .collect()
}

/// Rewrites a unit statement for a recipient: its actor, its id and its
/// registration become the recipient's, as every built record's are. The
/// pseudonymous identifier the unit was given does not leave in it.
pub(super) fn for_recipient(
    statement: &mut Value,
    recipient: &Recipient,
    attempt_id: &str,
) -> Result<(), RecordError> {
    statement["actor"] = serde_json::to_value(Agent::Agent {
        account: recipient.account().clone(),
    })?;
    if let Some(id) = statement["id"].as_str() {
        let id = recipient.statement_id(&format!("cmi5-unit|{id}"));
        statement["id"] = Value::String(id);
    }
    if let Some(context) = statement.get_mut("context").and_then(Value::as_object_mut) {
        if context.contains_key("registration") {
            context.insert(
                "registration".into(),
                Value::String(recipient.registration(attempt_id)),
            );
        }
    }
    if let Some(object) = statement.as_object_mut() {
        // Set by the endpoint for the unit; meaningless to anyone else.
        object.remove("authority");
        object.remove("stored");
    }
    Ok(())
}
