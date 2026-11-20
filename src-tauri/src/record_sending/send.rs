//! Draining one recipient's outbox, and voiding what it was sent.

use alexandria_studio::xapi::lrs::LrsError;
use alexandria_studio::xapi::{self, BuildOptions, LearningEvent, Recipient, Statement};
use chrono::{DateTime, Utc};
use serde::Serialize;

use super::access_log::{self, AccessAction, NewAccessEntry};
use super::{outbox, timestamp, RecipientEntry, RecordDb, Transport};
use crate::learning_records;

/// Records per request. Stores accept far larger batches; this keeps one
/// refused statement from holding back many others for long.
pub const BATCH_SIZE: usize = 50;

/// Batches one pass sends to one recipient before giving the others a turn.
const MAX_BATCHES_PER_PASS: usize = 20;

/// What one pass over a recipient's outbox did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct BatchOutcome {
    pub sent: u32,
    /// Refused by the recipient, or not buildable; retried later.
    pub failed: u32,
    /// Kept queued because the recipient could not be reached.
    pub waiting: u32,
}

/// Every course is public until the course access spec adds private
/// courses. Records from a private course whose owner has not allowed its
/// title to be disclosed must then be built with `Names::Omit`.
fn build_options() -> BuildOptions {
    BuildOptions::default()
}

/// Whether a delivery error means the request went out (so the attempt is a
/// send the access log records). Only an unreachable recipient keeps records
/// waiting rather than failed; an address that may no longer be used fails
/// without anything being sent.
fn reached_recipient(error: &LrsError) -> bool {
    matches!(
        error,
        LrsError::Refused { .. }
            | LrsError::Unavailable { .. }
            | LrsError::Timeout
            | LrsError::InvalidResponse(_)
    )
}

/// Queues what is newly in scope for `recipient`, then sends what is due,
/// in batches, oldest first. Statements are built for the recipient at send
/// with its own identifiers. Stops at the first batch that reaches nobody.
#[allow(clippy::too_many_arguments)]
pub async fn send_due<D: RecordDb>(
    db: &D,
    recipient: &RecipientEntry,
    recipient_name: &str,
    transport: &dyn Transport,
    learner_secret: &[u8; 32],
    learner_did: &str,
    now: DateTime<Utc>,
) -> Result<BatchOutcome, String> {
    let built_for = recipient
        .recipient(learner_secret, learner_did)
        .map_err(|e| e.to_string())?;
    let mut outcome = BatchOutcome::default();
    for _ in 0..MAX_BATCHES_PER_PASS {
        let entry = recipient.clone();
        let records = db
            .run("records.outbox.due", move |db| {
                outbox::queue(db.conn(), &entry, now)
                    .and_then(|_| outbox::due(db.conn(), &entry.id, now, BATCH_SIZE))
                    .map_err(|e| e.to_string())
            })
            .await?;
        if records.is_empty() {
            break;
        }

        let mut outside = Vec::new();
        let mut unbuildable = Vec::new();
        let mut ids = Vec::new();
        let mut statements = Vec::new();
        for record in records {
            if !recipient.scope.admits(&record) {
                outside.push(record.id);
                continue;
            }
            match learning_records::statement(&record, &built_for, &build_options()) {
                Ok(statement) => {
                    ids.push(record.id);
                    statements.push(statement);
                }
                Err(error) => unbuildable.push((record.id, error.to_string())),
            }
        }
        outcome.failed += unbuildable.len() as u32;
        let entry = recipient.clone();
        db.run("records.outbox.set-aside", move |db| {
            outbox::drop_queued(db.conn(), &entry.id, &outside).map_err(|e| e.to_string())?;
            for (id, error) in unbuildable {
                outbox::mark_unsent(db.conn(), &entry.id, &[id], &error, true, now)
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
        .await?;
        if statements.is_empty() {
            continue;
        }

        let deliveries = deliver(transport, ids, statements).await;
        let mut offline = false;
        for (ids, result) in &deliveries {
            match result {
                Ok(()) => outcome.sent += ids.len() as u32,
                Err(error) if error.is_unreachable() => {
                    offline = true;
                    outcome.waiting += ids.len() as u32;
                }
                Err(_) => outcome.failed += ids.len() as u32,
            }
        }
        let entry = recipient.clone();
        let name = recipient_name.to_string();
        db.run("records.outbox.record", move |db| {
            for (ids, result) in deliveries {
                record_delivery(db.conn(), &entry, &name, &ids, result, now)
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
        .await?;
        if offline {
            break;
        }
    }
    Ok(outcome)
}

/// Sends a batch. A store refuses a whole batch for one bad statement, so a
/// refused batch is retried one statement at a time to let the rest through.
async fn deliver(
    transport: &dyn Transport,
    ids: Vec<String>,
    statements: Vec<Statement>,
) -> Vec<(Vec<String>, Result<(), LrsError>)> {
    match transport.deliver(&statements).await {
        Err(LrsError::Refused { status: 400, .. }) if statements.len() > 1 => {
            let mut results = Vec::with_capacity(ids.len());
            for (id, statement) in ids.into_iter().zip(statements) {
                let result = transport.deliver(std::slice::from_ref(&statement)).await;
                results.push((vec![id], result));
            }
            results
        }
        result => vec![(ids, result)],
    }
}

fn record_delivery(
    conn: &rusqlite::Connection,
    recipient: &RecipientEntry,
    recipient_name: &str,
    ids: &[String],
    result: Result<(), LrsError>,
    now: DateTime<Utc>,
) -> Result<(), super::SendError> {
    let refusal = match &result {
        Ok(()) => {
            outbox::mark_sent(conn, &recipient.id, ids, now)?;
            None
        }
        Err(error) => {
            let failed = !error.is_unreachable();
            outbox::mark_unsent(conn, &recipient.id, ids, &error.to_string(), failed, now)?;
            if !reached_recipient(error) {
                return Ok(());
            }
            Some(error.to_string())
        }
    };
    access_log::append(
        conn,
        &NewAccessEntry {
            at: &timestamp(now),
            recipient_id: &recipient.id,
            recipient_kind: recipient.kind,
            recipient_name,
            action: AccessAction::Send,
            scope: &recipient.scope,
            record_ids: ids,
            identifier: &recipient.actor_name,
            refusal: refusal.as_deref(),
        },
    )
}

/// Sends voiding statements for `record_ids` (local ids of records the
/// recipient accepted), in batches, and logs each batch. Best effort: stops
/// at the first batch that fails and returns why. Returns how many were
/// voided.
#[allow(clippy::too_many_arguments)]
pub async fn void_sent<D: RecordDb>(
    db: &D,
    recipient: &RecipientEntry,
    recipient_name: &str,
    transport: &dyn Transport,
    learner_secret: &[u8; 32],
    learner_did: &str,
    record_ids: Vec<String>,
    now: DateTime<Utc>,
) -> Result<usize, String> {
    let built_for: Recipient = recipient
        .recipient(learner_secret, learner_did)
        .map_err(|e| e.to_string())?;
    let mut voided = 0;
    for chunk in record_ids.chunks(BATCH_SIZE) {
        let statements = chunk
            .iter()
            .map(|id| {
                xapi::build_statement(
                    &LearningEvent::voiding(id, now),
                    &built_for,
                    &BuildOptions::default(),
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let result = transport.deliver(&statements).await;
        let refusal = match &result {
            Ok(()) => None,
            Err(error) if reached_recipient(error) => Some(error.to_string()),
            Err(error) => return Err(error.to_string()),
        };
        let entry = recipient.clone();
        let name = recipient_name.to_string();
        let ids = chunk.to_vec();
        let logged_refusal = refusal.clone();
        db.run("records.access-log.void", move |db| {
            access_log::append(
                db.conn(),
                &NewAccessEntry {
                    at: &timestamp(now),
                    recipient_id: &entry.id,
                    recipient_kind: entry.kind,
                    recipient_name: &name,
                    action: AccessAction::Void,
                    scope: &entry.scope,
                    record_ids: &ids,
                    identifier: &entry.actor_name,
                    refusal: logged_refusal.as_deref(),
                },
            )
            .map_err(|e| e.to_string())
        })
        .await?;
        if let Some(refusal) = refusal {
            return Err(refusal);
        }
        voided += chunk.len();
    }
    Ok(voided)
}
