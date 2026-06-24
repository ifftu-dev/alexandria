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
    // Governance admin/committee txs are operator-signed (3-A). Only an
    // operator-configured node builds + submits them; everyone else skips
    // the queue entirely. The user `wallet` is no longer used here (votes
    // and nominations are off-chain gossip, not queued).
    let _ = wallet;
    let operator = match super::operator::load_operator_key() {
        Some(op) => op,
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
        match build_and_submit(&item.action_type, item, bf, db, &operator).await {
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

/// Build, sign (operator key) and submit the on-chain tx for a queued
/// governance action under the lean model. Only three actions touch the
/// chain — `finalize_election` (publish the finalized-election UTxO),
/// `install_committee` (spend + recreate the DAO state UTxO), and
/// `resolve_proposal` (anchor the outcome). Everything else is off-chain
/// (votes/nominations gossip; open/submit/approve are DB-only) and any
/// legacy queue rows for them are dropped.
async fn build_and_submit(
    action_type: &str,
    item: &QueueItem,
    blockfrost: &super::blockfrost::BlockfrostClient,
    db: &std::sync::Arc<std::sync::Mutex<Option<crate::db::Database>>>,
    operator: &super::operator::OperatorKey,
) -> Result<String, String> {
    use super::gov_onchain;

    let signed: Vec<u8> = match action_type {
        "finalize_election" => {
            let f = read_finalize_data(db, &item.target_id)?;
            gov_onchain::publish_finalized_election(
                blockfrost,
                operator,
                &f.dao_policy,
                &f.dao_token_name,
                &f.reputation_policy,
                f.seats,
                f.nomination_end_ms,
                f.voting_end_ms,
            )
            .await?
        }
        "install_committee" => {
            let mut d = read_install_data(db, &item.target_id)?;
            // No registry-resolvable winners → install the operator as the
            // sole committee so the DAO stays governable.
            if d.committee_vkhs.is_empty() {
                d.committee_vkhs = vec![operator.payment_key_hash()];
            }
            let params = gov_onchain::InstallParams {
                dao_state_utxo: (&d.dao_state_tx, d.dao_state_idx),
                dao_state_lovelace: d.dao_state_lovelace,
                dao_policy: d.dao_policy,
                dao_token_name: d.dao_token_name.clone(),
                scope_type: d.scope_type.clone(),
                scope_id: d.scope_id.clone(),
                reputation_policy: d.reputation_policy,
                committee_size: d.committee_size,
                election_interval_ms: d.election_interval_ms,
                election_ref: (&d.election_ref_tx, d.election_ref_idx),
                committee_vkhs: d.committee_vkhs.clone(),
                term_start_ms: chrono::Utc::now().timestamp_millis(),
            };
            gov_onchain::install_committee(blockfrost, operator, &params).await?
        }
        "resolve_proposal" => {
            let o = read_proposal_outcome(db, &item.target_id)?;
            let metadata = gov_onchain::proposal_outcome_metadata(
                &item.target_id,
                &o.status,
                o.votes_for,
                o.votes_against,
                o.vote_merkle_root_hex,
            );
            gov_onchain::build_governance_anchor(blockfrost, operator, metadata).await?
        }
        "open_election" | "cast_election_vote" | "cast_proposal_vote" | "submit_proposal"
        | "approve_proposal" => {
            return Err(format!(
                "action '{action_type}' is off-chain under the lean governance model"
            ));
        }
        other => return Err(format!("unknown governance action type: {other}")),
    };

    let tx_hash = blockfrost
        .submit_tx(&signed)
        .await
        .map_err(|e| format!("tx submission failed: {e}"))?;

    // Committee install moves the DAO state token to a new UTxO; advance
    // the pointer so the next spend finds it.
    if action_type == "install_committee" {
        update_dao_state_pointer(db, &item.target_id, &tx_hash)?;
    }
    Ok(tx_hash)
}

/// Row data for `finalize_election`, read under a brief DB lock.
struct FinalizeData {
    dao_policy: [u8; 28],
    dao_token_name: Vec<u8>,
    reputation_policy: [u8; 28],
    seats: i64,
    nomination_end_ms: i64,
    voting_end_ms: i64,
}

/// Row data for `install_committee`.
struct InstallData {
    dao_state_tx: String,
    dao_state_idx: u64,
    dao_state_lovelace: u64,
    dao_policy: [u8; 28],
    dao_token_name: Vec<u8>,
    scope_type: String,
    scope_id: Vec<u8>,
    reputation_policy: [u8; 28],
    committee_size: i64,
    election_interval_ms: i64,
    election_ref_tx: String,
    election_ref_idx: u64,
    committee_vkhs: Vec<[u8; 28]>,
}

fn hash28(hex_str: &str) -> Result<[u8; 28], String> {
    super::gov_tx_builder::hash_from_hex_pub(hex_str).map_err(|e| e.to_string())
}

/// Parse a `"txhash#index"` UTxO pointer.
fn parse_utxo_ref(s: &str) -> Result<(String, u64), String> {
    let (h, i) = s
        .split_once('#')
        .ok_or("malformed utxo ref (want txhash#index)")?;
    Ok((h.to_string(), i.parse().map_err(|_| "bad utxo index")?))
}

/// ISO-8601 → POSIX ms, or 0 when absent/unparseable.
fn iso_to_ms(s: Option<String>) -> i64 {
    s.and_then(|d| chrono::DateTime::parse_from_rfc3339(&d).ok())
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

fn read_finalize_data(
    db: &std::sync::Arc<std::sync::Mutex<Option<crate::db::Database>>>,
    election_id: &str,
) -> Result<FinalizeData, String> {
    let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
    let dbref = guard.as_ref().ok_or("database not initialized")?;
    let (seats, nom, vot, pol, name_hex, rep): (
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = dbref
        .conn()
        .query_row(
            "SELECT e.seats, e.nomination_end, e.voting_end, \
             d.state_token_policy, d.state_token_name, d.reputation_policy \
             FROM governance_elections e JOIN governance_daos d ON d.id = e.dao_id \
             WHERE e.id = ?1",
            params![election_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .map_err(|e| format!("election/DAO not found: {e}"))?;
    let pol = pol.ok_or("DAO has no on-chain state-token policy (not created on-chain)")?;
    let name_hex = name_hex.ok_or("DAO has no on-chain state-token name")?;
    let rep = rep.ok_or("DAO has no reputation policy")?;
    Ok(FinalizeData {
        dao_policy: hash28(&pol)?,
        dao_token_name: hex::decode(&name_hex).map_err(|e| format!("token name hex: {e}"))?,
        reputation_policy: hash28(&rep)?,
        seats,
        nomination_end_ms: iso_to_ms(nom),
        voting_end_ms: iso_to_ms(vot),
    })
}

#[allow(clippy::type_complexity)]
fn read_install_data(
    db: &std::sync::Arc<std::sync::Mutex<Option<crate::db::Database>>>,
    election_id: &str,
) -> Result<InstallData, String> {
    let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
    let dbref = guard.as_ref().ok_or("database not initialized")?;
    let conn = dbref.conn();
    let now_secs = chrono::Utc::now().timestamp();

    let (
        _dao_id,
        elec_ref,
        dao_utxo,
        pol,
        name_hex,
        scope_type,
        scope_id,
        rep,
        csize,
        interval_days,
    ): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
        Option<String>,
        i64,
        i64,
    ) = conn
        .query_row(
            "SELECT e.dao_id, e.on_chain_tx, d.dao_state_utxo, d.state_token_policy, \
             d.state_token_name, d.scope_type, d.scope_id, d.reputation_policy, \
             d.committee_size, d.election_interval_days \
             FROM governance_elections e JOIN governance_daos d ON d.id = e.dao_id \
             WHERE e.id = ?1",
            params![election_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                ))
            },
        )
        .map_err(|e| format!("election/DAO not found: {e}"))?;

    let elec_ref = elec_ref.ok_or("election has no finalized on-chain tx (finalize first)")?;
    let dao_utxo = dao_utxo.ok_or("DAO has no on-chain state UTxO")?;
    let (dao_state_tx, dao_state_idx) = parse_utxo_ref(&dao_utxo)?;
    let pol = pol.ok_or("DAO has no state-token policy")?;
    let name_hex = name_hex.ok_or("DAO has no state-token name")?;
    let rep = rep.ok_or("DAO has no reputation policy")?;

    // Winners → on-chain VKHs via the stake-pubkey registry.
    let mut committee_vkhs = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT stake_address FROM governance_election_nominees \
                 WHERE election_id = ?1 AND is_winner = 1",
            )
            .map_err(|e| e.to_string())?;
        let winners: Vec<String> = stmt
            .query_map(params![election_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        for w in winners {
            if let Some(vkh) = super::gov_onchain::stake_to_vkh(conn, &w, now_secs) {
                committee_vkhs.push(vkh);
            }
        }
    }

    Ok(InstallData {
        dao_state_tx,
        dao_state_idx,
        dao_state_lovelace: MIN_SCRIPT_DAO_LOVELACE,
        dao_policy: hash28(&pol)?,
        dao_token_name: hex::decode(&name_hex).map_err(|e| format!("token name hex: {e}"))?,
        scope_type,
        scope_id: scope_id.into_bytes(),
        reputation_policy: hash28(&rep)?,
        committee_size: csize,
        election_interval_ms: interval_days * 86_400_000,
        election_ref_tx: elec_ref,
        election_ref_idx: 0,
        committee_vkhs,
    })
}

/// DAO state UTxOs are created with this min-ADA (see create_dao).
const MIN_SCRIPT_DAO_LOVELACE: u64 = 3_000_000;

/// Resolved-proposal data for the outcome anchor.
struct ProposalOutcome {
    status: String,
    votes_for: i64,
    votes_against: i64,
    /// Merkle root (hex) over the signed off-chain votes, or `None` if
    /// no signed votes were recorded locally.
    vote_merkle_root_hex: Option<String>,
}

fn read_proposal_outcome(
    db: &std::sync::Arc<std::sync::Mutex<Option<crate::db::Database>>>,
    proposal_id: &str,
) -> Result<ProposalOutcome, String> {
    let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
    let dbref = guard.as_ref().ok_or("database not initialized")?;
    let conn = dbref.conn();

    let (status, votes_for, votes_against): (String, i64, i64) = conn
        .query_row(
            "SELECT status, votes_for, votes_against FROM governance_proposals WHERE id = ?1",
            params![proposal_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| format!("proposal not found: {e}"))?;

    // Merkle-commit the signed votes (auditable tally). Leaf binds the
    // voter, choice, and their signature.
    let mut leaves: Vec<[u8; 32]> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT voter, in_favor, signature FROM governance_proposal_votes \
                 WHERE proposal_id = ?1 AND signature IS NOT NULL ORDER BY id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![proposal_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (voter, in_favor, sig) = row.map_err(|e| e.to_string())?;
            let canonical = format!("{voter}:{in_favor}:{sig}");
            leaves.push(crate::crypto::hash::blake2b_256(canonical.as_bytes()));
        }
    }
    let vote_merkle_root_hex = if leaves.is_empty() {
        None
    } else {
        Some(hex::encode(crate::domain::completion::merkle_root(&leaves)))
    };

    Ok(ProposalOutcome {
        status,
        votes_for,
        votes_against,
        vote_merkle_root_hex,
    })
}

fn update_dao_state_pointer(
    db: &std::sync::Arc<std::sync::Mutex<Option<crate::db::Database>>>,
    election_id: &str,
    install_tx: &str,
) -> Result<(), String> {
    let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
    let dbref = guard.as_ref().ok_or("database not initialized")?;
    let new_ptr = format!("{install_tx}#0");
    dbref
        .conn()
        .execute(
            "UPDATE governance_daos SET dao_state_utxo = ?1 \
             WHERE id = (SELECT dao_id FROM governance_elections WHERE id = ?2)",
            params![new_ptr, election_id],
        )
        .map_err(|e| format!("failed to advance DAO state pointer: {e}"))?;
    Ok(())
}
