//! Persistent on-chain governance transaction queue.
//!
//! Governance commands write to local SQLite instantly, then enqueue an
//! on-chain Plutus transaction for async submission. A background task
//! processes the queue, building and submitting transactions via Blockfrost.
//!
//! Queue states: pending → outcome_unknown → submitted → confirmed.
//! Build failures may be retried; journaled transactions are never rebuilt.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::blockfrost::BlockfrostClient;
use super::submission::{self, Journal, Operation, Submission, SubmissionStatus, SubmitError};
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
    // Repeating the command must not allocate a fresh queue identity that
    // bypasses an uncertain transaction's durable binding.
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM onchain_governance_queue
         WHERE action_type = ?1 AND target_table = ?2 AND target_id = ?3
         ORDER BY created_at, id LIMIT 1",
            params![action_type, target_table, target_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(id) = existing {
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(id);
    }
    let id = crate::crypto::hash::entity_id(&[action_type, target_table, target_id]);
    tx.execute(
        "INSERT INTO onchain_governance_queue \
             (id, action_type, payload_json, target_table, target_id) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, action_type, payload_json, target_table, target_id],
    )
    .map_err(|e| format!("failed to enqueue on-chain tx: {e}"))?;
    tx.commit().map_err(|e| e.to_string())?;

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
             WHERE status = 'pending' AND NOT EXISTS \
               (SELECT 1 FROM chain_submissions s WHERE s.network = 'cardano-preprod' \
                AND s.operation_kind = 'governance_queue' AND s.operation_id = onchain_governance_queue.id) \
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

/// Project the durable journal atomically into the queue and chain pointers.
/// Confirmed pointers describe observed chain state, not an uncertain POST.
fn project_submission(
    db: &Database,
    item: &QueueItem,
    submission: &Submission,
) -> Result<(), String> {
    if matches!(
        submission.status,
        SubmissionStatus::Confirmed | SubmissionStatus::FailedOnChain
    ) && submission.confirmed_slot.is_none()
    {
        return Err("governance submission requires a ledger receipt before projection".into());
    }
    let status = match submission.status {
        SubmissionStatus::OutcomeUnknown => "outcome_unknown",
        SubmissionStatus::Submitted => "submitted",
        SubmissionStatus::Confirmed => "confirmed",
        SubmissionStatus::FailedOnChain => "failed",
    };
    let tx = db
        .conn()
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let changed = tx
        .execute(
            "UPDATE onchain_governance_queue SET status = ?2, tx_hash = ?3,
             attempts = CASE WHEN status = 'pending' THEN attempts + 1 ELSE attempts END,
             last_error = ?4, updated_at = datetime('now')
         WHERE id = ?1 AND status != 'confirmed'",
            params![item.id, status, submission.tx_hash, submission.last_error],
        )
        .map_err(|e| e.to_string())?;
    if changed == 1 && submission.status == SubmissionStatus::Confirmed {
        let sql = match item.target_table.as_str() {
            "governance_elections" => {
                "UPDATE governance_elections SET on_chain_tx = ?1 WHERE id = ?2"
            }
            "governance_proposals" => {
                "UPDATE governance_proposals SET on_chain_tx = ?1 WHERE id = ?2"
            }
            _ => return Err("unsupported on-chain queue target".into()),
        };
        if tx
            .execute(sql, params![submission.tx_hash, item.target_id])
            .map_err(|e| e.to_string())?
            != 1
        {
            return Err("on-chain queue target missing".into());
        }
        if item.action_type == "install_committee"
            && tx
                .execute(
                    "UPDATE governance_daos SET dao_state_utxo = ?1
                 WHERE id = (SELECT dao_id FROM governance_elections WHERE id = ?2)",
                    params![format!("{}#0", submission.tx_hash), item.target_id],
                )
                .map_err(|e| e.to_string())?
                != 1
        {
            return Err("on-chain DAO target missing".into());
        }
    }
    tx.commit().map_err(|e| e.to_string())
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
             WHERE id = ?2 AND NOT EXISTS \
               (SELECT 1 FROM chain_submissions s WHERE s.network = 'cardano-preprod' \
                AND s.operation_kind = 'governance_queue' AND s.operation_id = ?2)",
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
             WHERE id = ?1 AND status = 'failed' AND NOT EXISTS \
               (SELECT 1 FROM chain_submissions s WHERE s.network = 'cardano-preprod' \
                AND s.operation_kind = 'governance_queue' AND s.operation_id = ?1)",
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
pub(crate) async fn process_queue(
    journal: &Journal,
    blockfrost: &Option<BlockfrostClient>,
    wallet: &Option<crate::crypto::wallet::Wallet>,
) -> Result<usize, String> {
    // Skip if no Blockfrost client or wallet available
    let bf = match blockfrost {
        Some(ref client) => client,
        None => return Ok(0),
    };
    // Recover before checking builder credentials or pending work. In
    // particular, an empty pending queue must not suppress confirmation.
    let recovered = reconcile_journaled_items(journal, bf).await?;
    let confirmed = confirm_submitted_items(journal, bf).await?;
    if !super::gov_tx_builder::validators_deployed() {
        return Ok(recovered + confirmed);
    }
    // Legacy governance admin/committee txs are operator-signed (3-A).
    // Only an operator-configured node builds new ones; reconciliation
    // above needs no signing key. The user `wallet` is unused here (votes
    // and nominations are off-chain gossip, not queued).
    let _ = wallet;
    // Every queued action (finalized-election publication, committee
    // install, proposal-outcome anchor) carries the legacy local/operator
    // governance authority, so production builds none of them. Journaled
    // items above still reconcile and confirm.
    if !crate::domain::governance::legacy_local_governance_enabled() {
        return Ok(recovered + confirmed);
    }
    let operator = match super::operator::load_operator_key() {
        Some(op) => op,
        None => return Ok(recovered + confirmed),
    };

    // Get pending items
    let items = journal.run("governance_queue.pending", get_pending).await?;

    if items.is_empty() {
        return Ok(recovered + confirmed);
    }

    let mut processed = recovered + confirmed;

    for item in items {
        // Skip items that have been attempted too many times
        if item.attempts >= 5 {
            let id = item.id.clone();
            journal
                .run("governance_queue.max-attempts", move |db| {
                    mark_failed(db, &id, "max attempts (5) reached")
                })
                .await?;
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
        match build_and_submit(&item.action_type, &item, bf, journal, &operator).await {
            Ok(submitted) => {
                log::info!(
                    "On-chain tx: {} -> {:?} ({})",
                    item.action_type,
                    submitted.status,
                    submitted.tx_hash
                );
                journal
                    .run("governance_queue.project", move |db| {
                        project_submission(db, &item, &submitted)
                    })
                    .await?;
            }
            Err(SubmitError::Retryable(error)) => {
                // Nothing was journaled or sent; the item stays pending
                // without consuming an attempt.
                log::debug!("On-chain tx deferred for {}: {error}", item.action_type);
                return Ok(processed);
            }
            Err(SubmitError::Failed(e)) => {
                log::warn!("On-chain tx failed for {}: {}", item.action_type, e);
                let id = item.id.clone();
                journal
                    .run("governance_queue.fail", move |db| mark_failed(db, &id, &e))
                    .await?;
            }
        }

        processed += 1;
    }

    Ok(processed)
}

async fn reconcile_journaled_items(
    journal: &Journal,
    blockfrost: &BlockfrostClient,
) -> Result<usize, String> {
    let items = journal
        .run("governance_queue.journaled", |database| {
            let mut statement = database
                .conn()
                .prepare(
                    "SELECT q.id FROM onchain_governance_queue q JOIN chain_submissions s
             ON s.network = 'cardano-preprod' AND s.operation_kind = 'governance_queue'
                AND s.operation_id = q.id
             WHERE q.status != 'confirmed' ORDER BY q.updated_at, q.id LIMIT 20",
                )
                .map_err(|e| e.to_string())?;
            let ids = statement
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            let items = ids
                .iter()
                .map(|id| get_item(database, id)?.ok_or_else(|| "queue item missing".into()))
                .collect::<Result<Vec<_>, String>>();
            items
        })
        .await?;
    let count = items.len();
    for item in items {
        let operation = Operation {
            kind: "governance_queue",
            id: &item.id,
        };
        // Provider failure still projects the persisted uncertain state;
        // it cannot turn this operation back into buildable work.
        if let Err(error) = submission::reconcile(journal, blockfrost, operation).await {
            log::debug!("governance submission reconciliation: {error}");
        }
        journal
            .run("governance_queue.project-journaled", move |database| {
                let operation = Operation {
                    kind: "governance_queue",
                    id: &item.id,
                };
                let original =
                    submission::lookup(database.conn(), operation)?.ok_or("submission missing")?;
                project_submission(database, &item, &original)
            })
            .await?;
    }
    Ok(count)
}

/// Poll submitted queue items for on-chain confirmation.
///
/// Queries items with status='submitted' and checks each via Blockfrost.
/// Items confirmed on-chain transition to 'confirmed'.
async fn confirm_submitted_items(
    journal: &Journal,
    blockfrost: &BlockfrostClient,
) -> Result<usize, String> {
    let items = journal
        .run("governance_queue.submitted", get_submitted)
        .await?;

    let mut confirmed = 0;
    for item in &items {
        if let Some(ref tx_hash) = item.tx_hash {
            match blockfrost.is_tx_confirmed(tx_hash).await {
                Ok(true) => {
                    let id = item.id.clone();
                    journal
                        .run("governance_queue.confirm-legacy", move |db| {
                            mark_confirmed(db, &id)
                        })
                        .await?;
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
             WHERE status = 'submitted' AND tx_hash IS NOT NULL AND NOT EXISTS \
               (SELECT 1 FROM chain_submissions s WHERE s.network = 'cardano-preprod' \
                AND s.operation_kind = 'governance_queue' AND s.operation_id = onchain_governance_queue.id) \
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
    blockfrost: &BlockfrostClient,
    journal: &Journal,
    operator: &super::operator::OperatorKey,
) -> Result<Submission, SubmitError> {
    use super::gov_onchain;

    let failed = |error: String| SubmitError::Failed(error);
    let target_id = item.target_id.clone();
    let signed: Vec<u8> = match action_type {
        "finalize_election" => {
            let f = journal
                .before_send("governance_queue.finalize-data", move |db| {
                    read_finalize_data(db, &target_id)
                })
                .await?;
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
            .await
            .map_err(|e| failed(e.to_string()))?
        }
        "install_committee" => {
            // The operator only pays for and signs the transaction; it is
            // never substituted for an unresolvable elected committee.
            let d = journal
                .before_send("governance_queue.install-data", move |db| {
                    read_install_data(db, &target_id)
                })
                .await?;
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
            gov_onchain::install_committee(blockfrost, operator, &params)
                .await
                .map_err(|e| failed(e.to_string()))?
        }
        "resolve_proposal" => {
            let o = journal
                .before_send("governance_queue.proposal-outcome", move |db| {
                    read_proposal_outcome(db, &target_id)
                })
                .await?;
            let metadata = gov_onchain::proposal_outcome_metadata(
                &item.target_id,
                &o.status,
                o.votes_for,
                o.votes_against,
                o.vote_merkle_root_hex,
            );
            gov_onchain::build_governance_anchor(blockfrost, operator, metadata)
                .await
                .map_err(|e| failed(e.to_string()))?
        }
        "open_election" | "cast_election_vote" | "cast_proposal_vote" | "submit_proposal"
        | "approve_proposal" => {
            return Err(failed(format!(
                "action '{action_type}' is off-chain under the lean governance model"
            )));
        }
        other => return Err(failed(format!("unknown governance action type: {other}"))),
    };

    let context = serde_json::json!({"version": 1, "action_type": action_type,
        "target_table": item.target_table, "target_id": item.target_id,
        "payload_json": item.payload_json})
    .to_string();
    submission::submit_once(
        journal,
        blockfrost,
        Operation {
            kind: "governance_queue",
            id: &item.id,
        },
        &signed,
        &context,
    )
    .await
}

/// Row data for `finalize_election`, read in one short database job.
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

#[allow(clippy::type_complexity)]
fn read_finalize_data(dbref: &Database, election_id: &str) -> Result<FinalizeData, String> {
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
fn read_install_data(dbref: &Database, election_id: &str) -> Result<InstallData, String> {
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

    let committee_vkhs = certified_committee_vkhs(conn, election_id, now_secs)?;

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

/// Resolve every election winner to an on-chain VKH via the stake-pubkey
/// registry. Fails when there are no winners or any winner is unregistered:
/// installing a partial committee, or the operator in its place, would grant
/// authority nobody elected.
fn certified_committee_vkhs(
    conn: &rusqlite::Connection,
    election_id: &str,
    now_secs: i64,
) -> Result<Vec<[u8; 28]>, String> {
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
    if winners.is_empty() {
        return Err("cannot install committee: election has no winners".into());
    }
    winners
        .iter()
        .map(|winner| {
            super::gov_onchain::stake_to_vkh(conn, winner, now_secs).ok_or_else(|| {
                format!("cannot install committee: winner '{winner}' has no registered key")
            })
        })
        .collect()
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

fn read_proposal_outcome(dbref: &Database, proposal_id: &str) -> Result<ProposalOutcome, String> {
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::cardano::test_chain::{FakeChain, TestProfile, SHORT_LIMITS};

    fn test_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn().execute_batch(
            "INSERT INTO governance_daos (id, name, scope_type, scope_id, dao_state_utxo)
             VALUES ('dao', 'Test', 'subject', 'subject', 'old#0');
             INSERT INTO governance_elections (id, dao_id, title) VALUES ('election', 'dao', 'Test');"
        ).unwrap();
        db
    }

    fn queued(db: &Database) -> QueueItem {
        let id = enqueue(
            db,
            "install_committee",
            "{}",
            "governance_elections",
            "election",
        )
        .unwrap();
        get_item(db, &id).unwrap().unwrap()
    }

    fn submission(status: SubmissionStatus) -> Submission {
        Submission {
            tx_hash: "a".repeat(64),
            status,
            context_json: "{}".into(),
            last_error: None,
            confirmed_slot: (status == SubmissionStatus::Confirmed).then_some(42),
        }
    }

    #[cfg(not(all(debug_assertions, feature = "legacy-local-governance")))]
    #[test]
    fn production_queue_builds_no_governance_actions() {
        assert!(!crate::domain::governance::legacy_local_governance_enabled());
    }

    #[test]
    fn committee_install_never_falls_back_to_the_operator() {
        let db = test_db();
        db.conn()
            .execute_batch(&format!(
                "UPDATE governance_elections SET on_chain_tx = '{tx}' WHERE id = 'election';
                 UPDATE governance_daos SET state_token_policy = '{hash}',
                     state_token_name = '64616f', reputation_policy = '{hash}' WHERE id = 'dao';",
                tx = "b".repeat(64),
                hash = "00".repeat(28),
            ))
            .unwrap();
        let install =
            |db: &Database| read_install_data(db, "election").map(|data| data.committee_vkhs);

        let error = install(&db).expect_err("no winners must not install");
        assert!(error.contains("no winners"), "{error}");

        let execute = |sql: &str| db.conn().execute_batch(sql).unwrap();
        execute(
            "INSERT INTO governance_election_nominees
                 (id, election_id, stake_address, accepted, is_winner)
             VALUES ('winner', 'election', 'stake_winner', 1, 1),
                    ('unregistered', 'election', 'stake_unregistered', 1, 1);
             INSERT INTO stake_pubkey_registry
                 (stake_address, public_key_hex, valid_from, valid_until, source)
             VALUES ('stake_winner', '11111111111111111111111111111111', 0, NULL, 'snapshot');",
        );
        let error = install(&db)
            .expect_err("an unregistered winner must not be dropped from the committee");
        assert!(error.contains("stake_unregistered"), "{error}");

        execute("DELETE FROM governance_election_nominees WHERE id = 'unregistered'");
        assert_eq!(install(&db).unwrap().len(), 1);
    }

    #[test]
    fn enqueue_reuses_the_original_operation_even_after_outcome_unknown() {
        let db = test_db();
        let item = queued(&db);
        project_submission(&db, &item, &submission(SubmissionStatus::OutcomeUnknown)).unwrap();
        let duplicate = queued(&db);
        assert_eq!(duplicate.id, item.id);
        assert_eq!(duplicate.status, "outcome_unknown");
        assert_eq!(get_all(&db).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn journaled_item_reconciles_by_hash_without_operator_build_or_post() {
        let db = test_db();
        let item = queued(&db);
        let hash = "a".repeat(64);
        db.conn()
            .execute(
                "INSERT INTO chain_submissions
             (network, operation_kind, operation_id, tx_hash, signed_cbor, context_json)
             VALUES ('cardano-preprod', 'governance_queue', ?1, ?2, X'00', '{}')",
                params![item.id, hash],
            )
            .unwrap();
        let profile = TestProfile::new(db);
        let (chain, included) = FakeChain::stalled_submit(42).await;
        let client = Some(chain.client(SHORT_LIMITS));
        let journal = profile.background();
        let status =
            |profile: &TestProfile| profile.with_conn(|conn| get_item_status(conn, &item.id));

        process_queue(&journal, &client, &None).await.unwrap();
        assert_eq!(status(&profile), "outcome_unknown");
        included.store(true, Ordering::Release);
        process_queue(&journal, &client, &None).await.unwrap();
        assert_eq!(status(&profile), "confirmed");
        let pointer: String = profile.with_conn(|conn| {
            conn.query_row(
                "SELECT dao_state_utxo FROM governance_daos WHERE id = 'dao'",
                [],
                |row| row.get(0),
            )
            .unwrap()
        });
        assert_eq!(pointer, format!("{hash}#0"));
        let query = format!("GET /txs/{hash}");
        assert_eq!(chain.requests(), vec![query.clone(), query]);
    }

    fn get_item_status(conn: &rusqlite::Connection, id: &str) -> String {
        conn.query_row(
            "SELECT status FROM onchain_governance_queue WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn journaled_pending_or_failed_item_cannot_be_rebuilt() {
        let db = test_db();
        let item = queued(&db);
        assert_eq!(get_pending(&db).unwrap().len(), 1);
        db.conn()
            .execute(
                "INSERT INTO chain_submissions
             (network, operation_kind, operation_id, tx_hash, signed_cbor, context_json)
             VALUES ('cardano-preprod', 'governance_queue', ?1, ?2, X'00', '{}')",
                params![item.id, "a".repeat(64)],
            )
            .unwrap();
        assert!(get_pending(&db).unwrap().is_empty());
        mark_failed(&db, &item.id, "lost response").unwrap();
        assert_eq!(get_item(&db, &item.id).unwrap().unwrap().status, "pending");
        db.conn()
            .execute(
                "UPDATE onchain_governance_queue SET status = 'failed' WHERE id = ?1",
                [&item.id],
            )
            .unwrap();
        retry_item(&db, &item.id).unwrap();
        assert_eq!(get_item(&db, &item.id).unwrap().unwrap().status, "failed");
        assert!(get_pending(&db).unwrap().is_empty());
    }

    #[test]
    fn confirmation_and_chain_pointers_roll_back_together_then_recover_idempotently() {
        let db = test_db();
        let item = queued(&db);
        let unknown = submission(SubmissionStatus::OutcomeUnknown);
        project_submission(&db, &item, &unknown).unwrap();
        let pointer = || {
            db.conn()
                .query_row(
                    "SELECT dao_state_utxo FROM governance_daos WHERE id = 'dao'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap()
        };
        assert_eq!(pointer(), "old#0");
        project_submission(&db, &item, &submission(SubmissionStatus::Submitted)).unwrap();
        assert_eq!(pointer(), "old#0", "acknowledged is not confirmed");
        let mut legacy_confirmation = submission(SubmissionStatus::Confirmed);
        legacy_confirmation.confirmed_slot = None;
        assert!(project_submission(&db, &item, &legacy_confirmation).is_err());
        assert_eq!(
            get_item(&db, &item.id).unwrap().unwrap().status,
            "submitted"
        );
        assert_eq!(pointer(), "old#0");
        db.conn()
            .execute_batch(
                "CREATE TRIGGER fail_pointer BEFORE UPDATE OF dao_state_utxo ON governance_daos
             BEGIN SELECT RAISE(ABORT, 'injected pointer failure'); END;",
            )
            .unwrap();
        let confirmed = submission(SubmissionStatus::Confirmed);
        assert!(project_submission(&db, &item, &confirmed)
            .unwrap_err()
            .contains("injected pointer failure"));
        assert_eq!(
            get_item(&db, &item.id).unwrap().unwrap().status,
            "submitted"
        );
        let election_tx: Option<String> = db
            .conn()
            .query_row(
                "SELECT on_chain_tx FROM governance_elections WHERE id = 'election'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(election_tx, None);
        assert_eq!(pointer(), "old#0");
        db.conn()
            .execute_batch("DROP TRIGGER fail_pointer")
            .unwrap();
        project_submission(&db, &item, &confirmed).unwrap();
        project_submission(&db, &item, &unknown).unwrap();
        assert_eq!(
            get_item(&db, &item.id).unwrap().unwrap().status,
            "confirmed"
        );
        assert_eq!(pointer(), format!("{}#0", confirmed.tx_hash));
    }
}
