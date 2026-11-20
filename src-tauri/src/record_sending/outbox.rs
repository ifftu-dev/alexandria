//! The per-recipient outbox (`learning_record_outbox`).
//!
//! - [`queue`] adds every record in a recipient's scope that is not queued
//!   for it yet, including records written before it was approved.
//! - [`due`] hands out the oldest records waiting to be sent whose retry
//!   time has come.
//! - [`mark_sent`] and [`mark_unsent`] record how a try went. A try that
//!   reached nobody (offline) leaves the records `queued`; a refusal makes
//!   them `failed`. Both are retried after [`backoff`] and keep the reason
//!   for the learner to read.

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::{timestamp, RecipientEntry, SendError};
pub use crate::domain::record_sending::OutboxStatus;
use crate::learning_records::{self, LearningRecord};

/// First retry delay; doubled per failed try up to [`MAX_BACKOFF_SECS`].
pub const FIRST_BACKOFF_SECS: i64 = 30;
pub const MAX_BACKOFF_SECS: i64 = 6 * 60 * 60;

/// How long to wait after `attempts` failed tries.
pub fn backoff(attempts: u32) -> Duration {
    let doublings = attempts.saturating_sub(1).min(20);
    Duration::seconds((FIRST_BACKOFF_SECS << doublings).min(MAX_BACKOFF_SECS))
}

/// Queues every record in `recipient`'s scope not yet queued for it.
/// Returns how many were added.
pub fn queue(
    conn: &Connection,
    recipient: &RecipientEntry,
    now: DateTime<Utc>,
) -> Result<usize, SendError> {
    let now = timestamp(now);
    Ok(conn.execute(
        "INSERT OR IGNORE INTO learning_record_outbox (recipient_id, record_id, created_at, updated_at) \
         SELECT ?1, id, ?2, ?2 FROM learning_records \
         WHERE course_id IN (SELECT value FROM json_each(?3)) \
           AND (kind = 'completion' OR ?4 = 1) \
         ORDER BY seq",
        params![
            recipient.id,
            now,
            serde_json::to_string(&recipient.scope.course_ids)?,
            recipient.scope.include_interactions,
        ],
    )?)
}

/// Up to `limit` records waiting for `recipient_id` whose retry time has
/// come, oldest first.
pub fn due(
    conn: &Connection,
    recipient_id: &str,
    now: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<LearningRecord>, SendError> {
    let mut statement = conn.prepare(
        "SELECT o.record_id FROM learning_record_outbox o \
         JOIN learning_records r ON r.id = o.record_id \
         WHERE o.recipient_id = ?1 AND o.state <> 'sent' \
           AND (o.next_attempt_at IS NULL OR o.next_attempt_at <= ?2) \
         ORDER BY r.seq LIMIT ?3",
    )?;
    let ids = statement
        .query_map(params![recipient_id, timestamp(now), limit as i64], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut records = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(record) = learning_records::get(conn, &id)? {
            records.push(record);
        }
    }
    Ok(records)
}

/// Drops records from a recipient's queue without sending them (they fell
/// outside its scope).
pub fn drop_queued(conn: &Connection, recipient_id: &str, ids: &[String]) -> Result<(), SendError> {
    for id in ids {
        conn.execute(
            "DELETE FROM learning_record_outbox \
             WHERE recipient_id = ?1 AND record_id = ?2 AND state <> 'sent'",
            params![recipient_id, id],
        )?;
    }
    Ok(())
}

pub fn mark_sent(
    conn: &Connection,
    recipient_id: &str,
    ids: &[String],
    now: DateTime<Utc>,
) -> Result<(), SendError> {
    let now = timestamp(now);
    for id in ids {
        conn.execute(
            "UPDATE learning_record_outbox SET state = 'sent', attempts = attempts + 1, \
             last_error = NULL, next_attempt_at = NULL, sent_at = ?3, updated_at = ?3 \
             WHERE recipient_id = ?1 AND record_id = ?2",
            params![recipient_id, id, now],
        )?;
    }
    Ok(())
}

/// Records a try that did not get the records accepted. `failed` is false
/// when nothing reached the recipient (the records stay queued, waiting for
/// a connection) and true when it answered with a refusal.
pub fn mark_unsent(
    conn: &Connection,
    recipient_id: &str,
    ids: &[String],
    error: &str,
    failed: bool,
    now: DateTime<Utc>,
) -> Result<(), SendError> {
    let state = if failed { "failed" } else { "queued" };
    for id in ids {
        let attempts: Option<u32> = conn
            .query_row(
                "SELECT attempts FROM learning_record_outbox \
                 WHERE recipient_id = ?1 AND record_id = ?2",
                params![recipient_id, id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(attempts) = attempts else { continue };
        let attempts = attempts.saturating_add(1);
        conn.execute(
            "UPDATE learning_record_outbox SET state = ?3, attempts = ?4, last_error = ?5, \
             next_attempt_at = ?6, updated_at = ?7 \
             WHERE recipient_id = ?1 AND record_id = ?2",
            params![
                recipient_id,
                id,
                state,
                attempts,
                error,
                timestamp(now + backoff(attempts)),
                timestamp(now),
            ],
        )?;
    }
    Ok(())
}

pub fn status(conn: &Connection, recipient_id: &str) -> Result<OutboxStatus, SendError> {
    let (queued, failed, sent, next_attempt_at, last_sent_at) = conn.query_row(
        "SELECT \
           COALESCE(SUM(state = 'queued'), 0), COALESCE(SUM(state = 'failed'), 0), \
           COALESCE(SUM(state = 'sent'), 0), \
           MIN(CASE WHEN state <> 'sent' THEN next_attempt_at END), MAX(sent_at) \
         FROM learning_record_outbox WHERE recipient_id = ?1",
        params![recipient_id],
        |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        },
    )?;
    let last_error = conn
        .query_row(
            "SELECT last_error FROM learning_record_outbox \
             WHERE recipient_id = ?1 AND state <> 'sent' AND last_error IS NOT NULL \
             ORDER BY updated_at DESC LIMIT 1",
            params![recipient_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(OutboxStatus {
        queued,
        failed,
        sent,
        last_error,
        next_attempt_at,
        last_sent_at,
    })
}

/// A record a recipient refused, for the activity log to show beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SendFailure {
    pub recipient_id: String,
    pub record_id: String,
    pub attempts: u32,
    pub last_error: String,
    pub next_attempt_at: Option<String>,
}

/// Every record whose last try was refused, across recipients.
pub fn failures(conn: &Connection) -> Result<Vec<SendFailure>, SendError> {
    let mut statement = conn.prepare(
        "SELECT recipient_id, record_id, attempts, COALESCE(last_error, ''), next_attempt_at \
         FROM learning_record_outbox WHERE state = 'failed' ORDER BY updated_at DESC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(SendFailure {
            recipient_id: row.get(0)?,
            record_id: row.get(1)?,
            attempts: row.get(2)?,
            last_error: row.get(3)?,
            next_attempt_at: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// The earliest time any waiting record is due again, if one is waiting
/// for a retry.
pub fn next_retry(conn: &Connection) -> Result<Option<DateTime<Utc>>, SendError> {
    let next: Option<String> = conn.query_row(
        "SELECT MIN(next_attempt_at) FROM learning_record_outbox \
         WHERE state <> 'sent' AND next_attempt_at IS NOT NULL",
        [],
        |row| row.get(0),
    )?;
    Ok(next
        .and_then(|at| DateTime::parse_from_rfc3339(&at).ok())
        .map(|at| at.with_timezone(&Utc)))
}
