//! The learner-readable access log of sends (`record_access_log`).
//!
//! One row per send that reached a recipient (accepted or refused) and per
//! voiding on removal. A try that reached nobody is not a send and is not
//! logged; it shows as waiting in the outbox instead. Reads of the learner's
//! records by an organisation are known only to the hosted service and are
//! fetched from there.

use super::{RecipientKind, Scope, SendError};
pub use crate::domain::record_sending::{AccessAction, RecordAccessEntry as AccessLogEntry};
use rusqlite::{params, Connection};

impl AccessAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Void => "void",
        }
    }
}

/// A row to append.
#[derive(Debug, Clone)]
pub struct NewAccessEntry<'a> {
    pub at: &'a str,
    pub recipient_id: &'a str,
    pub recipient_kind: RecipientKind,
    pub recipient_name: &'a str,
    pub action: AccessAction,
    pub scope: &'a Scope,
    pub record_ids: &'a [String],
    pub identifier: &'a str,
    /// `None` when accepted, else why not.
    pub refusal: Option<&'a str>,
}

pub fn append(conn: &Connection, entry: &NewAccessEntry<'_>) -> Result<(), SendError> {
    conn.execute(
        "INSERT INTO record_access_log \
         (at, recipient_id, recipient_kind, recipient_name, action, course_ids, \
          include_interactions, record_count, record_ids, identifier, outcome, detail) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            entry.at,
            entry.recipient_id,
            entry.recipient_kind.as_str(),
            entry.recipient_name,
            entry.action.as_str(),
            serde_json::to_string(&entry.scope.course_ids)?,
            entry.scope.include_interactions,
            entry.record_ids.len() as i64,
            serde_json::to_string(entry.record_ids)?,
            entry.identifier,
            if entry.refusal.is_none() {
                "sent"
            } else {
                "failed"
            },
            entry.refusal,
        ],
    )?;
    Ok(())
}

/// Every row, newest first.
pub fn list(conn: &Connection) -> Result<Vec<AccessLogEntry>, SendError> {
    let mut statement = conn.prepare(
        "SELECT seq, at, recipient_id, recipient_kind, recipient_name, action, course_ids, \
                include_interactions, record_count, identifier, outcome, detail \
         FROM record_access_log ORDER BY seq DESC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, bool>(7)?,
            row.get::<_, u32>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, Option<String>>(11)?,
        ))
    })?;
    let mut entries = Vec::new();
    for row in rows {
        let (
            seq,
            at,
            recipient_id,
            kind,
            name,
            action,
            course_ids,
            interactions,
            count,
            id,
            outcome,
            detail,
        ) = row?;
        entries.push(AccessLogEntry {
            seq,
            at,
            recipient_id,
            recipient_kind: RecipientKind::parse(&kind)?,
            recipient_name: name,
            action: if action == "void" {
                AccessAction::Void
            } else {
                AccessAction::Send
            },
            scope: Scope {
                course_ids: serde_json::from_str(&course_ids)?,
                include_interactions: interactions,
            },
            record_count: count,
            identifier: id,
            accepted: outcome == "sent",
            detail,
        });
    }
    Ok(entries)
}

/// The local ids of every record `recipient_id` accepted, oldest first,
/// each once. Kept here rather than read from the outbox so a record the
/// learner deleted locally can still be voided.
pub fn accepted_record_ids(
    conn: &Connection,
    recipient_id: &str,
) -> Result<Vec<String>, SendError> {
    let mut statement = conn.prepare(
        "SELECT record_ids FROM record_access_log \
         WHERE recipient_id = ?1 AND action = 'send' AND outcome = 'sent' ORDER BY seq",
    )?;
    let batches = statement
        .query_map(params![recipient_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = std::collections::HashSet::new();
    let mut ids = Vec::new();
    for batch in batches {
        for id in serde_json::from_str::<Vec<String>>(&batch)? {
            if seen.insert(id.clone()) {
                ids.push(id);
            }
        }
    }
    Ok(ids)
}
