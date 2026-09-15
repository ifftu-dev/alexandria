use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::blockfrost::BlockfrostClient;
use super::completion_recovery::{self, CompletionContext, KIND};
use super::submission::{self, Operation, SubmissionStatus};
use crate::crypto::wallet::Wallet;
use crate::db::Database;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WitnessStatus {
    NotRequested,
    Pending,
    Submitted,
    OutcomeUnknown,
    Confirmed,
    FailedOnChain,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessState {
    pub status: WitnessStatus,
    pub tx_hash: Option<String>,
}

// Called inside the local claim transaction. Only an explicitly configured
// completion requests a witness; later provider configuration does not enqueue
// old local-only claims. The first request freezes the evidence and timestamp.
pub fn enqueue(
    conn: &Connection,
    claim_id: &str,
    requested: &CompletionContext,
) -> Result<(), String> {
    requested.validate()?;
    let id = requested.operation_id();
    let existing_claim: Option<String> = conn
        .query_row(
            "SELECT claim_id FROM completion_witness_requests WHERE operation_id = ?1",
            [&id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(existing) = existing_claim {
        if existing != claim_id {
            conn.execute(
                "UPDATE completion_claims SET witness_unavailable = 1 WHERE id = ?1",
                [claim_id],
            )
            .map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let operation = Operation {
        kind: KIND,
        id: &id,
    };
    let original = match submission::lookup(conn, operation)? {
        Some(saved) => requested.original_for_retry(&saved),
        None => completion_recovery::ensure_unobserved(conn, requested).map(|()| requested.clone()),
    };
    let context = match original {
        Ok(context) => context,
        Err(_) => {
            conn.execute(
                "UPDATE completion_claims SET witness_unavailable = 1 WHERE id = ?1",
                [claim_id],
            )
            .map_err(|e| e.to_string())?;
            return Ok(());
        }
    };
    conn.execute(
        "INSERT INTO completion_witness_requests (operation_id, claim_id, context_json) VALUES (?1, ?2, ?3)",
        params![id, claim_id, serde_json::to_string(&context).map_err(|e| e.to_string())?],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn state(conn: &Connection, claim_id: &str) -> Result<WitnessState, String> {
    let unavailable: bool = conn
        .query_row(
            "SELECT witness_unavailable FROM completion_claims WHERE id = ?1",
            [claim_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let request: Option<(String, bool, String)> = conn
        .query_row(
            "SELECT operation_id, blocked, context_json FROM completion_witness_requests WHERE claim_id = ?1",
            [claim_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((id, blocked, json)) = request else {
        return Ok(WitnessState {
            status: if unavailable {
                WitnessStatus::Unavailable
            } else {
                WitnessStatus::NotRequested
            },
            tx_hash: None,
        });
    };
    let saved = submission::lookup(
        conn,
        Operation {
            kind: KIND,
            id: &id,
        },
    )?;
    if let Some(saved) = saved.as_ref() {
        let matching = serde_json::from_str::<CompletionContext>(&json)
            .map_err(|e| e.to_string())
            .and_then(|context| context.original_for_retry(saved));
        if matching.is_err() {
            return Ok(WitnessState {
                status: WitnessStatus::Unavailable,
                tx_hash: None,
            });
        }
    }
    Ok(match saved {
        Some(saved) => WitnessState {
            status: match saved.status {
                SubmissionStatus::OutcomeUnknown => WitnessStatus::OutcomeUnknown,
                SubmissionStatus::Submitted => WitnessStatus::Submitted,
                SubmissionStatus::Confirmed if saved.confirmed_slot.is_some() => {
                    WitnessStatus::Confirmed
                }
                SubmissionStatus::FailedOnChain if saved.confirmed_slot.is_some() => {
                    WitnessStatus::FailedOnChain
                }
                _ => WitnessStatus::OutcomeUnknown,
            },
            tx_hash: Some(saved.tx_hash),
        },
        None => WitnessState {
            status: if blocked {
                WitnessStatus::Unavailable
            } else {
                WitnessStatus::Pending
            },
            tx_hash: None,
        },
    })
}

fn next_request(conn: &Connection) -> Result<Option<(String, String)>, String> {
    conn.query_row(
        "SELECT operation_id, context_json FROM completion_witness_requests r
         WHERE blocked = 0 AND next_attempt_at <= unixepoch()
           AND NOT EXISTS (SELECT 1 FROM chain_submissions s
             WHERE s.network = 'cardano-preprod' AND s.operation_kind = ?1 AND s.operation_id = r.operation_id)
         ORDER BY next_attempt_at, operation_id LIMIT 1",
        [KIND], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(|e| e.to_string())
}

fn validate_owner(context: &CompletionContext, id: &str, wallet: &Wallet) -> Result<(), String> {
    context.validate()?;
    if context.operation_id() != id
        || context.subject_pubkey != *wallet.signing_key.verifying_key().as_bytes()
        || context.payment_key_hash != wallet.payment_key_hash
    {
        return Err("completion request does not belong to this wallet".into());
    }
    Ok(())
}

// The caller must retain its profile lease throughout this pass. At most one
// unsigned intent is built per pass. Signed operations are exclusively handled
// by receipt recovery: neither absence nor an error authorizes a replacement.
pub async fn tick(
    db: &Arc<Mutex<Option<Database>>>,
    bf: &BlockfrostClient,
    wallet: &Wallet,
) -> Result<(), String> {
    let Some((id, json)) = submission::with_database(db, next_request)? else {
        return Ok(());
    };
    let context = serde_json::from_str::<CompletionContext>(&json)
        .map_err(|e| e.to_string())
        .and_then(|context| {
            validate_owner(&context, &id, wallet)?;
            Ok(context)
        });
    let context = match context {
        Ok(context) => context,
        Err(error) => {
            submission::with_database(db, |conn| {
                conn.execute("UPDATE completion_witness_requests SET blocked = 1, last_error = ?2 WHERE operation_id = ?1", params![id, error])
                    .map_err(|e| e.to_string())?;
                Ok(())
            })?;
            return Ok(());
        }
    };
    // Reserve the next attempt before any await, also spacing restart retries.
    let observed = submission::with_database(db, |conn| {
        let asset = hex::encode(super::completion_tx_builder::completion_asset_name(
            &context.payment_key_hash,
            context.course_id.as_bytes(),
        ));
        let observed = super::completion::observation_exists(conn, &context.policy_id, &asset)
            .map_err(|e| e.to_string())?;
        if observed {
            conn.execute("UPDATE completion_witness_requests SET blocked = 1,
                last_error = 'An existing observation prevents a new witness' WHERE operation_id = ?1", [&id])
                .map_err(|e| e.to_string())?;
            return Ok(true);
        }
        conn.execute(
            "UPDATE completion_witness_requests SET attempts = attempts + 1,
             next_attempt_at = unixepoch() + MIN(300, 30 * (attempts + 1)) WHERE operation_id = ?1",
            [&id],
        )
        .map_err(|e| e.to_string())?;
        Ok(false)
    })?;
    if observed {
        return Ok(());
    }
    let operation = Operation {
        kind: KIND,
        id: &id,
    };
    // Re-check before building; the durable submission layer arbitrates a
    // racing journal insert before POST as well.
    if submission::with_database(db, |conn| submission::lookup(conn, operation))?.is_some() {
        return Ok(());
    }
    let result = async {
        let treasury = super::treasury::TreasuryPayer::from_env();
        let built = super::completion_tx_builder::build_completion_mint_tx(
            bf,
            &wallet.payment_address,
            &wallet.payment_key_hash,
            &wallet.payment_key_extended,
            &context.subject_pubkey,
            context.course_id.as_bytes(),
            &context.leaves,
            &context.root,
            context.timestamp_ms,
            treasury.as_ref(),
        )
        .await
        .map_err(|e| e.to_string())?;
        submission::submit_once(db, bf, operation, &built.tx_cbor, &json).await
    }
    .await;
    if let Err(error) = result {
        submission::with_database(db, |conn| {
            conn.execute(
                "UPDATE completion_witness_requests SET last_error = ?2 WHERE operation_id = ?1",
                params![id, error],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "completion_queue_tests.rs"]
mod tests;
