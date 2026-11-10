//! Batched credential integrity anchoring (protocol spec §11.3.1).
//!
//! Issuing a credential queues it here. The issuer's node collects its own
//! queued credentials and, once the oldest has waited a batch window or the
//! batch is full, anchors them all with one transaction carrying the Merkle
//! root of their integrity hashes (`anchor_tx`). The anchor sponsor (the
//! Alexandria treasury, a script) pays when it can, the issuer's wallet when it
//! cannot.
//!
//! Every pass of [`tick`]:
//!
//! 1. projects any journaled batch whose projection a crash interrupted;
//! 2. reconciles open batches against the ledger. A confirmed batch stores a
//!    receipt per credential and the observed anchor; a batch that failed on
//!    chain, or can provably never land, releases its credentials for the next
//!    batch;
//! 3. flushes at most one new batch;
//! 4. looks up imported receipts' transactions on the ledger.
//!
//! The submission journal binds each batch to its exact signed bytes before
//! anything is sent, so a batch is never rebuilt or resent as a different
//! transaction. A credential belongs to one open batch at a time.

use alexandria_verify::vc::anchor::{
    anchor_matches, merkle_path, merkle_root, AnchorEvidence, AnchorReceipt, IntegrityAnchor,
    RECEIPT_VERSION,
};
use alexandria_verify::StoreLookup;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::cardano::anchor_tx::{self, AnchorRecord, Funding};
use crate::cardano::ledger::{self, Ledger};
use crate::cardano::submission::{
    self, Journal, Operation, Submission, SubmissionStatus, SubmitError,
};
use crate::crypto::did::{derive_did_key, Did};
use crate::crypto::wallet::Wallet;
use crate::domain::vc::VerifiableCredential;
use crate::network_profile::AnchorSponsorProfile;

pub use crate::domain::credential_anchor::{AnchorStatus, CredentialAnchorState};

const BATCH_KIND: &str = "credential_anchor_batch";
const NETWORK: &str = "cardano-preprod";

/// Failed batch attempts before a credential is marked failed. A failure
/// needs the user's retry after that.
pub const MAX_ATTEMPTS: u32 = 5;

/// Slots past a batch's validity interval before a transaction that was
/// never seen counts as unable to land. Leaves room for the provider to
/// catch up and for short rollbacks.
pub const EXPIRY_MARGIN_SLOTS: u64 = 1_800;

/// Wait before trying again to build a batch that could not be built or paid
/// for.
const UNBUILT_RETRY_MINUTES: i64 = 30;

/// Imported receipts whose transaction is looked up per pass.
const OBSERVATIONS_PER_TICK: i64 = 8;

/// When a batch flushes.
#[derive(Debug, Clone)]
pub struct BatchPolicy {
    /// Flush once the oldest queued credential was issued this long ago.
    pub window_secs: i64,
    /// Flush as soon as this many credentials are queued; never anchor more
    /// in one batch.
    pub cap: usize,
}

impl Default for BatchPolicy {
    fn default() -> Self {
        Self {
            window_secs: 6 * 60 * 60,
            cap: 256,
        }
    }
}

/// What the journal keeps alongside a batch's signed bytes: enough to
/// project the batch and compute every receipt after a crash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct BatchContext {
    version: u32,
    issuer_did: String,
    root: String,
    leaf_count: u64,
    funding: String,
    invalid_from_slot: u64,
    members: Vec<BatchMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct BatchMember {
    credential_id: String,
    credential_hash: String,
}

/// One anchoring pass. A node with no ledger does nothing; a node without an
/// unlocked wallet still reconciles and observes but anchors nothing new.
/// Returns how many batches or observations changed.
pub(crate) async fn tick<L: Ledger>(
    journal: &Journal,
    ledger: Option<&L>,
    wallet: Option<&Wallet>,
    sponsor: Option<&AnchorSponsorProfile>,
    policy: &BatchPolicy,
    now: &str,
) -> Result<u32, String> {
    let Some(ledger) = ledger else {
        log::debug!("anchor_queue::tick: no ledger, skipping");
        return Ok(0);
    };
    let mut changed = journal
        .run("anchor_queue.recover", |db| recover(db.conn()))
        .await?;
    changed += reconcile_batches(journal, ledger, now).await?;
    if let Some(wallet) = wallet {
        changed += flush(journal, ledger, wallet, sponsor, policy, now).await?;
    }
    changed += observe_receipts(journal, ledger, now).await?;
    Ok(changed)
}

// -- 1. recovery --------------------------------------------------------------

/// Project journaled batches that have no `anchor_batches` row yet: the
/// process stopped between the journal checkpoint and the projection.
fn recover(conn: &Connection) -> Result<u32, String> {
    let mut stmt = conn
        .prepare(
            "SELECT s.operation_id, s.context_json, s.status FROM chain_submissions s
             WHERE s.network = ?1 AND s.operation_kind = ?2
               AND NOT EXISTS (SELECT 1 FROM anchor_batches b WHERE b.id = s.operation_id)",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![NETWORK, BATCH_KIND], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut recovered = 0;
    for (tx_hash, context, status) in rows {
        let context: BatchContext =
            serde_json::from_str(&context).map_err(|e| format!("batch context: {e}"))?;
        // Final outcomes are finished by reconciliation, which also stores
        // receipts; until then the batch is open.
        let open = if status == "outcome_unknown" {
            "outcome_unknown"
        } else {
            "submitted"
        };
        project_new_batch(conn, &tx_hash, &context, open)?;
        recovered += 1;
    }
    Ok(recovered)
}

fn project_new_batch(
    conn: &Connection,
    tx_hash: &str,
    context: &BatchContext,
    status: &str,
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT OR IGNORE INTO anchor_batches
         (id, issuer_did, root, leaf_count, funding, status, tx_hash, invalid_from_slot)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?1, ?7)",
        params![
            tx_hash,
            context.issuer_did,
            context.root,
            context.leaf_count as i64,
            context.funding,
            status,
            context.invalid_from_slot as i64
        ],
    )
    .map_err(|e| format!("project batch: {e}"))?;
    for (index, member) in context.members.iter().enumerate() {
        tx.execute(
            "UPDATE credential_anchors SET batch_id = ?2, leaf_index = ?3, anchor_tx_hash = ?2,
                 anchor_status = ?4, last_error = NULL, next_attempt_at = NULL
             WHERE credential_id = ?1",
            params![member.credential_id, tx_hash, index as i64, status],
        )
        .map_err(|e| format!("project batch member: {e}"))?;
    }
    tx.commit().map_err(|e| e.to_string())
}

// -- 2. reconciliation --------------------------------------------------------

struct OpenBatch {
    id: String,
    invalid_from_slot: u64,
}

async fn reconcile_batches<L: Ledger>(
    journal: &Journal,
    ledger: &L,
    now: &str,
) -> Result<u32, String> {
    let open = journal
        .run("anchor_queue.open-batches", |db| {
            let mut stmt = db
                .conn()
                .prepare(
                    "SELECT id, invalid_from_slot FROM anchor_batches
                     WHERE status IN ('outcome_unknown', 'submitted') ORDER BY created_at, id",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(OpenBatch {
                        id: r.get(0)?,
                        invalid_from_slot: r.get::<_, i64>(1)? as u64,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string());
            rows
        })
        .await?;
    if open.is_empty() {
        return Ok(0);
    }
    let tip = ledger.tip_slot().await.ok();
    let mut changed = 0;
    for batch in open {
        let operation = Operation {
            kind: BATCH_KIND,
            id: &batch.id,
        };
        let submission = match submission::reconcile(journal, ledger, operation).await {
            Ok(Some(submission)) => submission,
            Ok(None) => return Err(format!("anchor batch {} has no journal entry", batch.id)),
            Err(error) => {
                log::debug!("anchor batch {} reconciliation: {error}", batch.id);
                continue;
            }
        };
        let expired = tip.is_some_and(|tip| tip > batch.invalid_from_slot + EXPIRY_MARGIN_SLOTS);
        let (id, now) = (batch.id.clone(), now.to_owned());
        let outcome = match submission.status {
            SubmissionStatus::Confirmed if submission.confirmed_slot.is_some() => {
                journal
                    .run("anchor_queue.confirm", move |db| {
                        confirm_batch(db.conn(), &id, &submission, &now)
                    })
                    .await
            }
            SubmissionStatus::FailedOnChain if submission.confirmed_slot.is_some() => {
                journal
                    .run("anchor_queue.failed", move |db| {
                        release_batch(
                            db.conn(),
                            &id,
                            "failed",
                            "the anchor transaction failed on chain",
                            &now,
                            Some(&submission),
                        )
                    })
                    .await
            }
            SubmissionStatus::OutcomeUnknown | SubmissionStatus::Submitted if expired => {
                journal
                    .run("anchor_queue.expire", move |db| {
                        release_batch(
                            db.conn(),
                            &id,
                            "expired",
                            "the anchor transaction expired before it was included",
                            &now,
                            None,
                        )
                    })
                    .await
            }
            SubmissionStatus::Submitted => {
                journal
                    .run("anchor_queue.submitted", move |db| {
                        set_open_status(db.conn(), &id, "submitted")
                    })
                    .await
            }
            _ => continue,
        };
        outcome?;
        changed += 1;
    }
    Ok(changed)
}

fn batch_context(conn: &Connection, batch_id: &str) -> Result<BatchContext, String> {
    let submission = submission::lookup(
        conn,
        Operation {
            kind: BATCH_KIND,
            id: batch_id,
        },
    )?
    .ok_or("anchor batch journal entry missing")?;
    serde_json::from_str(&submission.context_json).map_err(|e| format!("batch context: {e}"))
}

fn set_open_status(conn: &Connection, batch_id: &str, status: &str) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE anchor_batches SET status = ?2 WHERE id = ?1
           AND status IN ('outcome_unknown', 'submitted')",
        params![batch_id, status],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE credential_anchors SET anchor_status = ?2 WHERE batch_id = ?1
           AND anchor_status IN ('outcome_unknown', 'submitted')",
        params![batch_id, status],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

/// The batch landed: store each credential's receipt and the observed anchor,
/// read from the exact journaled bytes the ledger confirmed.
fn confirm_batch(
    conn: &Connection,
    batch_id: &str,
    submission: &Submission,
    now: &str,
) -> Result<(), String> {
    let context = batch_context(conn, batch_id)?;
    let signed: Vec<u8> = conn
        .query_row(
            "SELECT signed_cbor FROM chain_submissions
             WHERE network = ?1 AND operation_kind = ?2 AND operation_id = ?3",
            params![NETWORK, BATCH_KIND, batch_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    // Confirmed no later than now; the receipt carries no block time.
    let observed = ledger::parse_anchor_tx(&signed, &submission.tx_hash, Some(now.to_owned()))
        .ok_or("the journaled anchor transaction carries no anchor record")?;
    let leaves = context
        .members
        .iter()
        .map(|m| decode_hash(&m.credential_hash))
        .collect::<Result<Vec<_>, _>>()?;
    let confirmed_at = observed
        .anchored_at
        .clone()
        .unwrap_or_else(|| now.to_owned());

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for (index, member) in context.members.iter().enumerate() {
        let receipt = AnchorReceipt {
            v: RECEIPT_VERSION,
            leaf_index: index as u64,
            leaf_count: context.leaf_count,
            path: merkle_path(&leaves, index)
                .iter()
                .map(hex::encode)
                .collect(),
            root: context.root.clone(),
            tx_hash: submission.tx_hash.clone(),
            issuer_did: context.issuer_did.clone(),
        };
        store_receipt(&tx, &member.credential_hash, &receipt, "local")?;
    }
    store_observation(&tx, &observed)?;
    tx.execute(
        "UPDATE anchor_batches SET status = 'confirmed', confirmed_at = ?2 WHERE id = ?1",
        params![batch_id, confirmed_at],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE credential_anchors SET anchor_status = 'confirmed', confirmed_at = ?2,
             last_error = NULL, next_attempt_at = NULL
         WHERE batch_id = ?1",
        params![batch_id, confirmed_at],
    )
    .map_err(|e| e.to_string())?;
    submission::mark_applied(
        &tx,
        Operation {
            kind: BATCH_KIND,
            id: batch_id,
        },
    )?;
    tx.commit().map_err(|e| e.to_string())
}

/// The batch will never anchor its credentials: put them back in the queue
/// for a new batch, counting the attempt.
fn release_batch(
    conn: &Connection,
    batch_id: &str,
    batch_status: &str,
    reason: &str,
    now: &str,
    final_submission: Option<&Submission>,
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE anchor_batches SET status = ?2 WHERE id = ?1",
        params![batch_id, batch_status],
    )
    .map_err(|e| e.to_string())?;
    let members: Vec<String> = {
        let mut stmt = tx
            .prepare("SELECT credential_id FROM credential_anchors WHERE batch_id = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([batch_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    tx.execute(
        "UPDATE credential_anchors SET batch_id = NULL, leaf_index = NULL, anchor_tx_hash = NULL
         WHERE batch_id = ?1",
        params![batch_id],
    )
    .map_err(|e| e.to_string())?;
    for id in members {
        record_failure(&tx, &id, reason, now)?;
    }
    if final_submission.is_some() {
        submission::mark_applied(
            &tx,
            Operation {
                kind: BATCH_KIND,
                id: batch_id,
            },
        )?;
    }
    tx.commit().map_err(|e| e.to_string())
}

/// Count a failed attempt: back off exponentially (capped at an hour), and
/// mark the credential failed at [`MAX_ATTEMPTS`].
fn record_failure(
    conn: &Connection,
    credential_id: &str,
    reason: &str,
    now: &str,
) -> Result<(), String> {
    let attempts: u32 = conn
        .query_row(
            "SELECT attempts FROM credential_anchors WHERE credential_id = ?1",
            [credential_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("read attempts: {e}"))?;
    let attempts = attempts + 1;
    let status = if attempts >= MAX_ATTEMPTS {
        "failed"
    } else {
        "pending"
    };
    let backoff_minutes = (1u32 << attempts.min(6)).min(60);
    conn.execute(
        "UPDATE credential_anchors SET attempts = ?2, last_error = ?3, anchor_status = ?4,
             next_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ', ?5, '+' || ?6 || ' minutes')
         WHERE credential_id = ?1",
        params![
            credential_id,
            attempts,
            reason,
            status,
            now,
            backoff_minutes
        ],
    )
    .map_err(|e| format!("record anchor failure: {e}"))?;
    Ok(())
}

// -- 3. flush -----------------------------------------------------------------

struct Queued {
    credential_id: String,
    credential_hash: String,
    issued_at: String,
}

/// This issuer's queued credentials that are due, oldest first, and whether
/// the batch should flush now.
fn load_queued(
    conn: &Connection,
    issuer: &Did,
    policy: &BatchPolicy,
    now: &str,
) -> Result<(Vec<Queued>, bool), String> {
    let mut stmt = conn
        .prepare(
            "SELECT a.credential_id, c.integrity_hash, c.issuance_date
             FROM credential_anchors a JOIN credentials c ON c.id = a.credential_id
             WHERE a.anchor_status = 'pending' AND a.batch_id IS NULL
               AND c.issuer_did = ?1
               AND (a.next_attempt_at IS NULL OR julianday(a.next_attempt_at) <= julianday(?2))
             ORDER BY julianday(c.issuance_date), a.credential_id
             LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let queued = stmt
        .query_map(params![issuer.as_str(), now, policy.cap as i64], |r| {
            Ok(Queued {
                credential_id: r.get(0)?,
                credential_hash: r.get(1)?,
                issued_at: r.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let Some(oldest) = queued.first() else {
        return Ok((queued, false));
    };
    let window_passed: bool = conn
        .query_row(
            "SELECT julianday(?1) <= julianday(?2) - (?3 / 86400.0)",
            params![oldest.issued_at, now, policy.window_secs],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let due = window_passed || queued.len() >= policy.cap;
    Ok((queued, due))
}

async fn flush<L: Ledger>(
    journal: &Journal,
    ledger: &L,
    wallet: &Wallet,
    sponsor: Option<&AnchorSponsorProfile>,
    policy: &BatchPolicy,
    now: &str,
) -> Result<u32, String> {
    let issuer = derive_did_key(&wallet.signing_key);
    let (queued, due) = {
        let (issuer, policy, now) = (issuer.clone(), policy.clone(), now.to_owned());
        journal
            .run("anchor_queue.load", move |db| {
                load_queued(db.conn(), &issuer, &policy, &now)
            })
            .await?
    };
    if !due {
        return Ok(0);
    }
    let ids: Vec<String> = queued.iter().map(|q| q.credential_id.clone()).collect();
    match build_and_submit(journal, ledger, wallet, &issuer, sponsor, &queued).await {
        Ok(()) => Ok(1),
        Err(SubmitError::Retryable(error)) => {
            // Nothing was journaled or sent; try again next pass.
            log::debug!("anchor batch deferred: {error}");
            Ok(0)
        }
        Err(SubmitError::Failed(error)) => {
            // Nothing was journaled: the batch could not be built or paid
            // for. The credentials stay queued with the reason, and an
            // attempt is not spent; only a batch that reached the ledger
            // and failed there counts toward `failed`.
            log::warn!("anchor batch for {} credentials waits: {error}", ids.len());
            let now = now.to_owned();
            journal
                .run("anchor_queue.wait", move |db| {
                    let tx = db
                        .conn()
                        .unchecked_transaction()
                        .map_err(|e| e.to_string())?;
                    for id in &ids {
                        tx.execute(
                            "UPDATE credential_anchors SET last_error = ?2,
                                 next_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ', ?3, '+' || ?4 || ' minutes')
                             WHERE credential_id = ?1",
                            params![id, error, now, UNBUILT_RETRY_MINUTES],
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    tx.commit().map_err(|e| e.to_string())
                })
                .await?;
            Ok(1)
        }
    }
}

async fn build_and_submit<L: Ledger>(
    journal: &Journal,
    ledger: &L,
    wallet: &Wallet,
    issuer: &Did,
    sponsor: Option<&AnchorSponsorProfile>,
    queued: &[Queued],
) -> Result<(), SubmitError> {
    let leaves = queued
        .iter()
        .map(|q| decode_hash(&q.credential_hash))
        .collect::<Result<Vec<_>, _>>()
        .map_err(SubmitError::Failed)?;
    let record = AnchorRecord {
        root: hex::encode(merkle_root(&leaves)),
        leaf_count: leaves.len() as u64,
        issuer_did: issuer.clone(),
        anchored_from: queued[0].issued_at.clone(),
    };

    let mut sponsor_error = None;
    let mut built = None;
    if let Some(profile) = sponsor {
        match sponsored(ledger, profile, &record, wallet).await {
            Ok(Some((tx, funding))) => built = Some((tx, funding)),
            Ok(None) => {}
            Err(error) => {
                log::warn!("sponsored anchor not built, trying the issuer's wallet: {error}");
                sponsor_error = Some(error);
            }
        }
    }
    let (anchor, funding) = match built {
        Some(built) => built,
        None => match anchor_tx::build_anchor_tx(&record, Funding::Issuer, wallet, ledger).await {
            Ok(tx) => (tx, Funding::Issuer.label()),
            Err(error) => {
                return Err(SubmitError::Failed(match sponsor_error {
                    Some(sponsor) => format!("{sponsor}; {error}"),
                    None => error,
                }))
            }
        },
    };

    let context = BatchContext {
        version: 1,
        issuer_did: issuer.as_str().to_owned(),
        root: record.root.clone(),
        leaf_count: record.leaf_count,
        funding: funding.to_owned(),
        invalid_from_slot: anchor.invalid_from_slot,
        members: queued
            .iter()
            .map(|q| BatchMember {
                credential_id: q.credential_id.clone(),
                credential_hash: q.credential_hash.clone(),
            })
            .collect(),
    };
    let context_json =
        serde_json::to_string(&context).map_err(|e| SubmitError::Failed(e.to_string()))?;
    // A rebuild identical to an earlier batch (same inputs, same tip) would
    // only find that batch's outcome in the journal. Wait for the chain to
    // move instead.
    let earlier = {
        let id = anchor.tx_hash.clone();
        journal
            .before_send("anchor_queue.earlier", move |db| {
                submission::lookup(
                    db.conn(),
                    Operation {
                        kind: BATCH_KIND,
                        id: &id,
                    },
                )
            })
            .await?
    };
    if earlier.is_some() {
        return Err(SubmitError::Failed(
            "the rebuilt batch is identical to an earlier transaction; waiting for the chain to move"
                .into(),
        ));
    }
    let submitted = submission::submit_once(
        journal,
        ledger,
        Operation {
            kind: BATCH_KIND,
            id: &anchor.tx_hash,
        },
        &anchor.signed_cbor,
        &context_json,
    )
    .await?;
    let status = match submitted.status {
        SubmissionStatus::OutcomeUnknown => "outcome_unknown",
        _ => "submitted",
    };
    let tx_hash = submitted.tx_hash.clone();
    journal
        .run("anchor_queue.project", move |db| {
            project_new_batch(db.conn(), &tx_hash, &context, status)
        })
        .await
        .map_err(SubmitError::Failed)?;
    log::info!(
        "anchor batch {} ({} credentials, {funding}-funded): {:?}",
        submitted.tx_hash,
        record.leaf_count,
        submitted.status
    );
    Ok(())
}

/// Build a sponsored anchor when a sponsor UTxO is spendable now, with its
/// funding label. `Ok(None)` when none is.
async fn sponsored<L: Ledger>(
    ledger: &L,
    profile: &AnchorSponsorProfile,
    record: &AnchorRecord,
    wallet: &Wallet,
) -> Result<Option<(anchor_tx::AnchorTx, &'static str)>, String> {
    let (utxos, tip) = tokio::join!(ledger.utxos(&profile.address), ledger.tip_slot());
    let (utxos, tip) = (utxos?, tip?);
    let Some(utxo) = anchor_tx::select_sponsor_utxo(profile, &utxos, tip, &record.issuer_did)
    else {
        return Ok(None);
    };
    let funding = Funding::Sponsor { profile, utxo };
    let label = funding.label();
    anchor_tx::build_anchor_tx(record, funding, wallet, ledger)
        .await
        .map(|tx| Some((tx, label)))
}

fn decode_hash(hex_hash: &str) -> Result<[u8; 32], String> {
    hex::decode(hex_hash)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| format!("integrity hash {hex_hash:?} is not 32 bytes of hex"))
}

// -- 4. observation -----------------------------------------------------------

/// Wait before looking up a receipt's transaction again after the ledger did
/// not show it.
const OBSERVE_RETRY_MINUTES: i64 = 60;

/// Look up the transactions of receipts this node holds but has never seen
/// on the ledger (imported with a bundle, typically). A transaction the
/// ledger does not show is retried later, so it cannot hold up the rest.
async fn observe_receipts<L: Ledger>(
    journal: &Journal,
    ledger: &L,
    now: &str,
) -> Result<u32, String> {
    let unobserved: Vec<String> = {
        let now = now.to_owned();
        journal
            .run("anchor_queue.unobserved", move |db| {
                let mut stmt = db
                    .conn()
                    .prepare(
                        "SELECT r.tx_hash FROM anchor_receipts r
                         WHERE NOT EXISTS
                             (SELECT 1 FROM anchor_observations o WHERE o.tx_hash = r.tx_hash)
                         GROUP BY r.tx_hash
                         HAVING MAX(r.next_observe_at IS NULL
                                    OR julianday(r.next_observe_at) <= julianday(?1))
                         ORDER BY MIN(r.stored_at), r.tx_hash LIMIT ?2",
                    )
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map(params![now, OBSERVATIONS_PER_TICK], |r| r.get(0))
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string());
                rows
            })
            .await?
    };
    let mut observed = 0;
    for tx_hash in unobserved {
        let found = match ledger::observe_anchor(ledger, &tx_hash).await {
            Ok(found) => found,
            Err(error) => {
                log::debug!("anchor observation {tx_hash}: {error}");
                continue;
            }
        };
        let now = now.to_owned();
        journal
            .run("anchor_queue.observe", move |db| match found {
                Some(anchor) => store_observation(db.conn(), &anchor),
                None => db
                    .conn()
                    .execute(
                        "UPDATE anchor_receipts SET next_observe_at =
                             strftime('%Y-%m-%dT%H:%M:%SZ', ?2, '+' || ?3 || ' minutes')
                         WHERE tx_hash = ?1",
                        params![tx_hash, now, OBSERVE_RETRY_MINUTES],
                    )
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            })
            .await?;
        observed += 1;
    }
    Ok(observed)
}

fn store_receipt(
    conn: &Connection,
    credential_hash: &str,
    receipt: &AnchorReceipt,
    source: &str,
) -> Result<(), String> {
    let json = serde_json::to_string(receipt).map_err(|e| e.to_string())?;
    // The same receipt arriving again changes nothing, except that this
    // node's own confirmation marks it local.
    conn.execute(
        "INSERT INTO anchor_receipts (credential_hash, tx_hash, root, receipt_json, source)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (credential_hash, tx_hash) DO UPDATE SET
             source = CASE WHEN excluded.source = 'local' THEN 'local' ELSE source END,
             receipt_json = CASE WHEN excluded.source = 'local'
                                 THEN excluded.receipt_json ELSE receipt_json END,
             root = CASE WHEN excluded.source = 'local' THEN excluded.root ELSE root END",
        params![
            credential_hash,
            receipt.tx_hash.to_ascii_lowercase(),
            receipt.root.to_ascii_lowercase(),
            json,
            source
        ],
    )
    .map_err(|e| format!("store anchor receipt: {e}"))?;
    refresh_verified(conn, Some(credential_hash), None)
}

fn store_observation(conn: &Connection, anchor: &IntegrityAnchor) -> Result<(), String> {
    let json = serde_json::to_string(anchor).map_err(|e| e.to_string())?;
    let tx_hash = anchor.tx_hash.to_ascii_lowercase();
    conn.execute(
        "INSERT OR REPLACE INTO anchor_observations (tx_hash, root, anchor_json)
         VALUES (?1, ?2, ?3)",
        params![tx_hash, anchor.root.to_ascii_lowercase(), json],
    )
    .map_err(|e| format!("store anchor observation: {e}"))?;
    refresh_verified(conn, None, Some(&tx_hash))
}

/// Recompute the cached `verified` flag of the receipts for one credential
/// hash or one transaction: whether the receipt anchors a stored credential
/// with that content, by the verifier's own rule.
fn refresh_verified(
    conn: &Connection,
    credential_hash: Option<&str>,
    tx_hash: Option<&str>,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT r.credential_hash, r.tx_hash, r.receipt_json, o.anchor_json,
                    (SELECT c.signed_vc_json FROM credentials c
                     WHERE c.integrity_hash = r.credential_hash LIMIT 1)
             FROM anchor_receipts r
             LEFT JOIN anchor_observations o ON o.tx_hash = r.tx_hash
             WHERE (?1 IS NULL OR r.credential_hash = ?1) AND (?2 IS NULL OR r.tx_hash = ?2)",
        )
        .map_err(|e| e.to_string())?;
    type Row = (String, String, String, Option<String>, Option<String>);
    let rows: Vec<Row> = stmt
        .query_map(params![credential_hash, tx_hash], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    for (hash, tx, receipt, observed, credential) in rows {
        let verified = match (observed, credential) {
            (Some(observed), Some(credential)) => {
                let evidence = serde_json::from_str(&receipt)
                    .ok()
                    .zip(serde_json::from_str(&observed).ok())
                    .map(|(receipt, observed)| AnchorEvidence { receipt, observed });
                let credential = serde_json::from_str::<VerifiableCredential>(&credential).ok();
                match (evidence, credential) {
                    (Some(evidence), Some(credential)) => {
                        crate::domain::vc::integrity_hash_of(&credential).is_ok_and(|own| {
                            own == hash && anchor_matches(&evidence, &credential, &own)
                        })
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        conn.execute(
            "UPDATE anchor_receipts SET verified = ?3 WHERE credential_hash = ?1 AND tx_hash = ?2",
            params![hash, tx, verified],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// -- receipts in and out --------------------------------------------------------

/// The best receipt for a credential hash: this node's own first, then one
/// whose root the ledger shows, then any.
const BEST_RECEIPT: &str = "SELECT r.receipt_json, o.anchor_json FROM anchor_receipts r
     LEFT JOIN anchor_observations o ON o.tx_hash = r.tx_hash AND o.root = r.root
     WHERE r.credential_hash = ?1
     ORDER BY r.verified DESC, r.source = 'local' DESC, o.tx_hash IS NOT NULL DESC, r.stored_at
     LIMIT 1";

/// The best stored receipt for this credential's content, if any.
pub fn receipt_for(
    conn: &Connection,
    credential: &VerifiableCredential,
) -> Result<Option<AnchorReceipt>, String> {
    let hash = crate::domain::vc::integrity_hash_of(credential)?;
    receipt_by_hash(conn, &hash)
}

pub fn receipt_by_hash(
    conn: &Connection,
    credential_hash: &str,
) -> Result<Option<AnchorReceipt>, String> {
    let json: Option<String> = conn
        .query_row(BEST_RECEIPT, [credential_hash], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    json.map(|j| serde_json::from_str(&j).map_err(|e| format!("stored receipt: {e}")))
        .transpose()
}

/// Keep a receipt that arrived with `credential` (from a bundle). It counts
/// for nothing until the ledger shows its transaction and the verifier folds
/// it to that root. Receipts never displace one another: a wrong one only
/// waits beside the right one.
pub fn import_receipt(
    conn: &Connection,
    credential: &VerifiableCredential,
    receipt: &AnchorReceipt,
) -> Result<(), String> {
    let hash = crate::domain::vc::integrity_hash_of(credential)?;
    let hex64 = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    if !hex64(&receipt.tx_hash) || !hex64(&receipt.root) {
        return Err("receipt names no transaction or root".into());
    }
    store_receipt(conn, &hash, receipt, "imported")
}

/// The evidence the verifier needs for `credential_hash`: `Missing` without a
/// receipt, `Unavailable` while no receipt's transaction has been observed.
pub fn evidence(conn: &Connection, credential_hash: &str) -> StoreLookup<AnchorEvidence> {
    let best: Result<Option<(String, Option<String>)>, _> = conn
        .query_row(BEST_RECEIPT, [credential_hash], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional();
    match best {
        Ok(None) => StoreLookup::Missing,
        Ok(Some((receipt, Some(observed)))) => {
            match (
                serde_json::from_str(&receipt),
                serde_json::from_str(&observed),
            ) {
                (Ok(receipt), Ok(observed)) => {
                    StoreLookup::Found(AnchorEvidence { receipt, observed })
                }
                _ => StoreLookup::Unavailable,
            }
        }
        Ok(Some((_, None))) | Err(_) => StoreLookup::Unavailable,
    }
}

// -- state for the UI -----------------------------------------------------------

const STATE_COLUMNS: &str = "credential_id, anchor_status, anchor_tx_hash, attempts, \
     last_error, enqueued_at, confirmed_at";

fn state_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CredentialAnchorState> {
    let status: String = r.get(1)?;
    let last_error: Option<String> = r.get(4)?;
    Ok(CredentialAnchorState {
        credential_id: r.get(0)?,
        status: AnchorStatus::parse(&status).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                format!("unknown anchor status {status:?}").into(),
            )
        })?,
        tx_hash: r.get(2)?,
        attempts: r.get(3)?,
        needs_funds: last_error
            .as_deref()
            .is_some_and(|e| e.contains(anchor_tx::NEEDS_FUNDS)),
        last_error,
        enqueued_at: r.get(5)?,
        confirmed_at: r.get(6)?,
    })
}

/// Received credentials (never queued here) whose receipt verified, by the
/// cached flag. `credential_id` narrows to one.
fn received_states(
    conn: &Connection,
    credential_id: Option<&str>,
) -> Result<Vec<CredentialAnchorState>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT c.id, o.anchor_json FROM credentials c
             JOIN anchor_receipts r ON r.credential_hash = c.integrity_hash AND r.verified = 1
             JOIN anchor_observations o ON o.tx_hash = r.tx_hash
             WHERE (?1 IS NULL OR c.id = ?1)
               AND NOT EXISTS (SELECT 1 FROM credential_anchors a WHERE a.credential_id = c.id)
             GROUP BY c.id ORDER BY c.id",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(String, String)> = stmt
        .query_map(params![credential_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(id, observed)| {
            let observed: IntegrityAnchor =
                serde_json::from_str(&observed).map_err(|e| format!("observation: {e}"))?;
            Ok(CredentialAnchorState {
                credential_id: id,
                status: AnchorStatus::Confirmed,
                tx_hash: Some(observed.tx_hash),
                attempts: 0,
                last_error: None,
                needs_funds: false,
                enqueued_at: observed.anchored_at.clone().unwrap_or_default(),
                confirmed_at: observed.anchored_at,
            })
        })
        .collect()
}

/// The anchor state of every credential this node queued, plus received
/// credentials whose receipt verifies, by credential id.
pub fn list_states(conn: &Connection) -> Result<Vec<CredentialAnchorState>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {STATE_COLUMNS} FROM credential_anchors ORDER BY credential_id"
        ))
        .map_err(|e| format!("list credential anchors: {e}"))?;
    let mut states: Vec<CredentialAnchorState> = stmt
        .query_map([], state_row)
        .map_err(|e| format!("list credential anchors: {e}"))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("list credential anchors: {e}"))?;
    states.extend(received_states(conn, None)?);
    states.sort_by(|a, b| a.credential_id.cmp(&b.credential_id));
    Ok(states)
}

/// One credential's anchor state; `None` when it was never queued here and
/// carries no verified receipt.
pub fn state(
    conn: &Connection,
    credential_id: &str,
) -> Result<Option<CredentialAnchorState>, String> {
    let queued = conn
        .query_row(
            &format!("SELECT {STATE_COLUMNS} FROM credential_anchors WHERE credential_id = ?1"),
            params![credential_id],
            state_row,
        )
        .optional()
        .map_err(|e| format!("read credential anchor: {e}"))?;
    match queued {
        Some(state) => Ok(Some(state)),
        None => Ok(received_states(conn, Some(credential_id))?
            .into_iter()
            .next()),
    }
}

/// Whether this node queued the credential for anchoring (it issued it).
pub fn is_queued(conn: &Connection, credential_id: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM credential_anchors WHERE credential_id = ?1)",
        [credential_id],
        |r| r.get(0),
    )
    .map_err(|e| format!("read credential anchor: {e}"))
}

/// Queue a credential for anchoring. Idempotent: a no-op if it is already
/// queued, in a batch, or anchored.
pub fn enqueue(db: &Connection, credential_id: &str) -> Result<(), String> {
    db.execute(
        "INSERT OR IGNORE INTO credential_anchors (credential_id, anchor_status) VALUES (?1, 'pending')",
        params![credential_id],
    )
    .map_err(|e| format!("enqueue credential anchor: {e}"))?;
    Ok(())
}

/// Queue a credential, or give a failed one a fresh set of attempts. A
/// credential in an open batch is never touched: that batch must resolve
/// through the journal first.
pub fn enqueue_or_retry(db: &Connection, credential_id: &str) -> Result<(), String> {
    enqueue(db, credential_id)?;
    db.execute(
        "UPDATE credential_anchors
         SET anchor_status = 'pending', attempts = 0, last_error = NULL, next_attempt_at = NULL
         WHERE credential_id = ?1 AND anchor_status = 'failed' AND batch_id IS NULL",
        params![credential_id],
    )
    .map_err(|e| format!("retry credential anchor: {e}"))?;
    Ok(())
}

#[cfg(test)]
#[path = "anchor_queue_tests.rs"]
mod tests;
