//! Persistent on-chain governance transaction queue.
//!
//! Governance commands write to local SQLite instantly, then enqueue an
//! on-chain Plutus transaction for async submission. A background task
//! processes the queue, building and submitting transactions via Blockfrost.
//!
//! Queue states: pending → submitted → confirmed | failed

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::Database;

/// A queued on-chain governance transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: String,
    pub action_type: String,
    pub payload_json: String,
    pub target_table: String,
    pub target_id: String,
    pub status: String,
    pub tx_hash: Option<String>,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Enqueue a governance action for on-chain submission.
pub fn enqueue(
    db: &Database,
    action_type: &str,
    payload_json: &str,
    target_table: &str,
    target_id: &str,
) -> Result<String, String> {
    let id = crate::crypto::hash::entity_id(&[action_type, target_id, &chrono::Utc::now().to_rfc3339()]);

    db.conn()
        .execute(
            "INSERT INTO onchain_governance_queue \
             (id, action_type, payload_json, target_table, target_id) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, action_type, payload_json, target_table, target_id],
        )
        .map_err(|e| format!("failed to enqueue on-chain tx: {e}"))?;

    log::info!(
        "Enqueued on-chain governance tx: {} for {}.{}",
        action_type,
        target_table,
        target_id
    );

    Ok(id)
}

/// Get all pending queue items (for processing or display).
pub fn get_pending(db: &Database) -> Result<Vec<QueueItem>, String> {
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, action_type, payload_json, target_table, target_id, \
             status, tx_hash, attempts, last_error, created_at, updated_at \
             FROM onchain_governance_queue \
             WHERE status = 'pending' \
             ORDER BY created_at ASC LIMIT 20",
        )
        .map_err(|e| e.to_string())?;

    let items = stmt
        .query_map([], |row| {
            Ok(QueueItem {
                id: row.get(0)?,
                action_type: row.get(1)?,
                payload_json: row.get(2)?,
                target_table: row.get(3)?,
                target_id: row.get(4)?,
                status: row.get(5)?,
                tx_hash: row.get(6)?,
                attempts: row.get(7)?,
                last_error: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(items)
}

/// Get all queue items (for status display).
pub fn get_all(db: &Database) -> Result<Vec<QueueItem>, String> {
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, action_type, payload_json, target_table, target_id, \
             status, tx_hash, attempts, last_error, created_at, updated_at \
             FROM onchain_governance_queue \
             ORDER BY created_at DESC LIMIT 50",
        )
        .map_err(|e| e.to_string())?;

    let items = stmt
        .query_map([], |row| {
            Ok(QueueItem {
                id: row.get(0)?,
                action_type: row.get(1)?,
                payload_json: row.get(2)?,
                target_table: row.get(3)?,
                target_id: row.get(4)?,
                status: row.get(5)?,
                tx_hash: row.get(6)?,
                attempts: row.get(7)?,
                last_error: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(items)
}

/// Mark a queue item as submitted (tx built and sent to Blockfrost).
pub fn mark_submitted(db: &Database, queue_id: &str, tx_hash: &str) -> Result<(), String> {
    db.conn()
        .execute(
            "UPDATE onchain_governance_queue \
             SET status = 'submitted', tx_hash = ?1, \
                 attempts = attempts + 1, updated_at = datetime('now') \
             WHERE id = ?2",
            params![tx_hash, queue_id],
        )
        .map_err(|e| e.to_string())?;

    // Also update the target entity's on_chain_tx column
    // (the target_table is already validated via the schema allowlist)
    let item = get_item(db, queue_id)?;
    if let Some(item) = item {
        let sql = format!(
            "UPDATE {} SET on_chain_tx = ?1 WHERE id = ?2",
            item.target_table
        );
        let _ = db.conn().execute(&sql, params![tx_hash, item.target_id]);
    }

    Ok(())
}

/// Mark a queue item as confirmed (tx confirmed on chain).
pub fn mark_confirmed(db: &Database, queue_id: &str) -> Result<(), String> {
    db.conn()
        .execute(
            "UPDATE onchain_governance_queue \
             SET status = 'confirmed', updated_at = datetime('now') \
             WHERE id = ?1",
            params![queue_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Mark a queue item as failed with an error message.
pub fn mark_failed(db: &Database, queue_id: &str, error: &str) -> Result<(), String> {
    db.conn()
        .execute(
            "UPDATE onchain_governance_queue \
             SET status = 'failed', last_error = ?1, \
                 attempts = attempts + 1, updated_at = datetime('now') \
             WHERE id = ?2",
            params![error, queue_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Reset a failed item back to pending for retry.
pub fn retry_item(db: &Database, queue_id: &str) -> Result<(), String> {
    db.conn()
        .execute(
            "UPDATE onchain_governance_queue \
             SET status = 'pending', last_error = NULL, \
                 updated_at = datetime('now') \
             WHERE id = ?1 AND status = 'failed'",
            params![queue_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Get a single queue item by ID.
fn get_item(db: &Database, queue_id: &str) -> Result<Option<QueueItem>, String> {
    db.conn()
        .query_row(
            "SELECT id, action_type, payload_json, target_table, target_id, \
             status, tx_hash, attempts, last_error, created_at, updated_at \
             FROM onchain_governance_queue WHERE id = ?1",
            params![queue_id],
            |row| {
                Ok(QueueItem {
                    id: row.get(0)?,
                    action_type: row.get(1)?,
                    payload_json: row.get(2)?,
                    target_table: row.get(3)?,
                    target_id: row.get(4)?,
                    status: row.get(5)?,
                    tx_hash: row.get(6)?,
                    attempts: row.get(7)?,
                    last_error: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())
}
