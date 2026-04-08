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
    let id =
        crate::crypto::hash::entity_id(&[action_type, target_id, &chrono::Utc::now().to_rfc3339()]);

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

/// Process pending queue items: attempt to build and submit on-chain transactions.
///
/// Called periodically from a background task. For each pending item:
/// 1. Check if validators are deployed (skip all if not)
/// 2. Attempt to build the Plutus transaction via gov_tx_builder
/// 3. Submit via Blockfrost
/// 4. Mark as submitted/failed accordingly
///
/// Returns the number of items processed.
pub async fn process_queue(
    db: &std::sync::Arc<std::sync::Mutex<Option<crate::db::Database>>>,
    blockfrost: &Option<super::blockfrost::BlockfrostClient>,
    wallet: &Option<crate::crypto::wallet::Wallet>,
) -> Result<usize, String> {
    // Skip if validators not deployed yet
    if !super::gov_tx_builder::validators_deployed() {
        return Ok(0);
    }

    // Skip if no Blockfrost client or wallet available
    let bf = match blockfrost {
        Some(ref client) => client,
        None => return Ok(0),
    };
    let w = match wallet {
        Some(ref w) => w,
        None => return Ok(0),
    };

    // Get pending items
    let items = {
        let db_guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
        let db_ref = db_guard.as_ref().ok_or("database not initialized")?;
        get_pending(db_ref)?
    };

    if items.is_empty() {
        return Ok(0);
    }

    let mut processed = 0;

    for item in &items {
        // Skip items that have been attempted too many times
        if item.attempts >= 5 {
            let db_guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
            let db_ref = db_guard.as_ref().ok_or("database not initialized")?;
            mark_failed(db_ref, &item.id, "max attempts (5) reached")?;
            processed += 1;
            continue;
        }

        log::info!(
            "Processing on-chain queue item: {} ({}) attempt {}",
            item.action_type,
            item.id,
            item.attempts + 1
        );

        // Attempt to build and submit the transaction
        match build_and_submit(&item.action_type, item, bf, w).await {
            Ok(tx_hash) => {
                let db_guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
                let db_ref = db_guard.as_ref().ok_or("database not initialized")?;
                mark_submitted(db_ref, &item.id, &tx_hash)?;
                log::info!("On-chain tx submitted: {} -> {}", item.action_type, tx_hash);
            }
            Err(e) => {
                let db_guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
                let db_ref = db_guard.as_ref().ok_or("database not initialized")?;
                mark_failed(db_ref, &item.id, &e)?;
                log::warn!("On-chain tx failed for {}: {}", item.action_type, e);
            }
        }

        processed += 1;
    }

    // After processing pending items, check submitted ones for confirmation
    let confirmed = confirm_submitted_items(db, bf).await?;
    if confirmed > 0 {
        log::info!("governance queue: confirmed {confirmed} transaction(s)");
    }

    Ok(processed + confirmed)
}

/// Poll submitted queue items for on-chain confirmation.
///
/// Queries items with status='submitted' and checks each via Blockfrost.
/// Items confirmed on-chain transition to 'confirmed'.
async fn confirm_submitted_items(
    db: &std::sync::Arc<std::sync::Mutex<Option<crate::db::Database>>>,
    blockfrost: &super::blockfrost::BlockfrostClient,
) -> Result<usize, String> {
    let items = {
        let db_guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
        let db_ref = db_guard.as_ref().ok_or("database not initialized")?;
        get_submitted(db_ref)?
    };

    let mut confirmed = 0;
    for item in &items {
        if let Some(ref tx_hash) = item.tx_hash {
            match blockfrost.is_tx_confirmed(tx_hash).await {
                Ok(true) => {
                    let db_guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
                    let db_ref = db_guard.as_ref().ok_or("database not initialized")?;
                    mark_confirmed(db_ref, &item.id)?;
                    log::info!("On-chain tx confirmed: {} ({})", item.action_type, tx_hash);
                    confirmed += 1;
                }
                Ok(false) => {
                    // Not yet confirmed, will check again next cycle
                }
                Err(e) => {
                    log::debug!("Failed to check tx {}: {e}", tx_hash);
                }
            }
        }
    }

    Ok(confirmed)
}

/// Get queue items that have been submitted but not yet confirmed.
fn get_submitted(db: &crate::db::Database) -> Result<Vec<QueueItem>, String> {
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, action_type, payload_json, target_table, target_id, \
             status, tx_hash, attempts, last_error, created_at, updated_at \
             FROM onchain_governance_queue \
             WHERE status = 'submitted' AND tx_hash IS NOT NULL \
             ORDER BY updated_at ASC LIMIT 20",
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

/// Build and submit an on-chain transaction for a specific governance action.
///
/// Dispatches to the appropriate gov_tx_builder function based on action_type.
/// Returns the transaction hash on success.
async fn build_and_submit(
    action_type: &str,
    item: &QueueItem,
    blockfrost: &super::blockfrost::BlockfrostClient,
    wallet: &crate::crypto::wallet::Wallet,
) -> Result<String, String> {
    let payment_address = &wallet.payment_address;
    let payment_key_hash = &wallet.payment_key_hash;
    let payment_key_extended = &wallet.payment_key_extended;

    let result = match action_type {
        "open_election" => {
            let params: serde_json::Value = serde_json::from_str(&item.payload_json)
                .map_err(|e| format!("invalid payload: {e}"))?;
            super::gov_tx_builder::build_open_election_tx(
                blockfrost,
                payment_address,
                payment_key_hash,
                payment_key_extended,
                &[0u8; 28],
                &[],
                params["election_id"].as_i64().unwrap_or(0),
                params["seats"].as_i64().unwrap_or(5),
                params["nominee_min_proficiency"].as_str().unwrap_or("apply"),
                params["voter_min_proficiency"].as_str().unwrap_or("remember"),
                params["nomination_end_ms"].as_i64().unwrap_or(0),
                params["voting_end_ms"].as_i64().unwrap_or(0),
            )
            .await
        }
        "cast_election_vote" => {
            super::gov_tx_builder::build_cast_vote_tx(
                blockfrost,
                payment_address,
                payment_key_hash,
                payment_key_extended,
                "election",
                None,
            )
            .await
        }
        "finalize_election" => {
            super::gov_tx_builder::build_finalize_election_tx(
                blockfrost,
                payment_address,
                payment_key_hash,
                payment_key_extended,
            )
            .await
        }
        "install_committee" => {
            let params: serde_json::Value = serde_json::from_str(&item.payload_json)
                .map_err(|e| format!("invalid payload: {e}"))?;
            let election_tx = params["election_tx_hash"].as_str().unwrap_or("");
            let election_tx_bytes = hex::decode(election_tx)
                .map_err(|e| format!("invalid election tx hash: {e}"))?;
            super::gov_tx_builder::build_install_committee_tx(
                blockfrost,
                payment_address,
                payment_key_hash,
                payment_key_extended,
                (&election_tx_bytes, 0),
            )
            .await
        }
        "submit_proposal" | "approve_proposal" => {
            // Both create/approve a proposal UTxO at the proposal script address
            super::gov_tx_builder::build_resolve_proposal_tx(
                blockfrost,
                payment_address,
                payment_key_hash,
                payment_key_extended,
            )
            .await
        }
        "cast_proposal_vote" => {
            let params: serde_json::Value = serde_json::from_str(&item.payload_json)
                .map_err(|e| format!("invalid payload: {e}"))?;
            let in_favor = params["in_favor"].as_bool();
            super::gov_tx_builder::build_cast_vote_tx(
                blockfrost,
                payment_address,
                payment_key_hash,
                payment_key_extended,
                "proposal",
                in_favor,
            )
            .await
        }
        "resolve_proposal" => {
            super::gov_tx_builder::build_resolve_proposal_tx(
                blockfrost,
                payment_address,
                payment_key_hash,
                payment_key_extended,
            )
            .await
        }
        other => return Err(format!("unknown governance action type: {other}")),
    };

    match result {
        Ok(gov_result) => {
            // Submit signed tx to Blockfrost
            let tx_hash = blockfrost
                .submit_tx(&gov_result.tx_cbor)
                .await
                .map_err(|e| format!("tx submission failed: {e}"))?;
            Ok(tx_hash)
        }
        Err(e) => Err(format!("tx build failed: {e}")),
    }
}
