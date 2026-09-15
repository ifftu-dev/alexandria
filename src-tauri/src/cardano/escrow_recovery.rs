//! Recovery of challenge stake operations without rebuilding transactions.

use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::blockfrost::BlockfrostClient;
use super::submission::{self, Journal, Operation, Submission, SubmissionStatus};
use crate::evidence::challenge;

pub const LOCK_KIND: &str = "challenge_lock";
pub const SETTLE_KIND: &str = "challenge_settle";

#[cfg(test)]
#[path = "escrow_recovery_tests.rs"]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscrowContext {
    pub version: u32,
    pub challenge_id: String,
    pub stake_lovelace: u64,
    pub action: EscrowAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EscrowAction {
    Lock {
        challenger_pkh: String,
        treasury_pkh: String,
        authority_pkh: String,
    },
    Settle {
        escrow_tx_hash: String,
        escrow_index: u64,
        recipient_pkh: String,
        refund: bool,
    },
}

/// Return the original operation before callers derive keys or build a tx.
pub fn existing_response(
    conn: &Connection,
    operation: Operation<'_>,
) -> Result<Option<String>, String> {
    let Some(existing) = submission::lookup(conn, operation)? else {
        return Ok(None);
    };
    project(conn, operation, &existing)?;
    response(existing).map(Some)
}

pub fn response(submitted: Submission) -> Result<String, String> {
    match submitted.status {
        SubmissionStatus::Confirmed | SubmissionStatus::Submitted => Ok(submitted.tx_hash),
        SubmissionStatus::OutcomeUnknown => Err(format!(
            "transaction outcome unknown ({}); preserved for reconciliation, no replacement will be created",
            submitted.tx_hash)),
        SubmissionStatus::FailedOnChain => Err(format!(
            "transaction script failed on chain ({}); no automatic replacement will be created",
            submitted.tx_hash)),
    }
}

pub fn project(
    conn: &Connection,
    operation: Operation<'_>,
    submitted: &Submission,
) -> Result<(), String> {
    if !matches!(
        submitted.status,
        SubmissionStatus::Confirmed | SubmissionStatus::FailedOnChain
    ) {
        return Ok(());
    }
    let context: EscrowContext =
        serde_json::from_str(&submitted.context_json).map_err(|e| e.to_string())?;
    if context.version != 1 || context.challenge_id != operation.id {
        return Err("invalid escrow recovery context".into());
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    if submitted.status == SubmissionStatus::Confirmed {
        let stake = challenge::get_stake_info(&tx, operation.id)?;
        if u64::try_from(stake.stake_lovelace).ok() != Some(context.stake_lovelace) {
            return Err("escrow recovery amount does not match challenge".into());
        }
        match context.action {
            EscrowAction::Lock {
                challenger_pkh,
                treasury_pkh,
                ..
            } if operation.kind == LOCK_KIND => {
                if stake.stake_status == "none" {
                    challenge::set_stake_locked(
                        &tx,
                        operation.id,
                        &submitted.tx_hash,
                        &challenger_pkh,
                        &treasury_pkh,
                    )?;
                } else if stake.lock_tx_hash.as_deref() != Some(submitted.tx_hash.as_str()) {
                    return Err("challenge is bound to another lock transaction".into());
                }
                // A repeated lock projection must not undo later settlement.
            }
            EscrowAction::Settle {
                escrow_tx_hash,
                escrow_index,
                recipient_pkh,
                refund,
            } if operation.kind == SETTLE_KIND => {
                if escrow_index != 0
                    || stake.lock_tx_hash.as_deref() != Some(escrow_tx_hash.as_str())
                {
                    return Err(
                        "settlement recovery does not match the recorded escrow output".into(),
                    );
                }
                let expected_recipient = if refund {
                    &stake.escrow_challenger_pkh
                } else {
                    &stake.escrow_treasury_pkh
                };
                if expected_recipient.as_deref() != Some(recipient_pkh.as_str()) {
                    return Err("settlement recovery recipient does not match escrow".into());
                }
                if stake.stake_status == "locked" {
                    // The signed transaction fixes the outcome; a later local
                    // resolution change cannot rewrite what the ledger did.
                    challenge::set_stake_settled(&tx, operation.id, &submitted.tx_hash, refund)?;
                } else {
                    let previous: Option<String> = tx
                        .query_row(
                            "SELECT settle_tx_hash FROM credential_challenges WHERE id = ?1",
                            [operation.id],
                            |row| row.get(0),
                        )
                        .map_err(|e| e.to_string())?;
                    let expected_status = if refund { "returned" } else { "forfeited" };
                    if previous.as_deref() != Some(submitted.tx_hash.as_str())
                        || stake.stake_status != expected_status
                    {
                        return Err("challenge has conflicting settlement state".into());
                    }
                }
            }
            _ => return Err("escrow recovery operation kind mismatch".into()),
        }
    }
    submission::mark_applied(&tx, operation)?;
    tx.commit().map_err(|e| e.to_string())
}

/// Run after unlock as part of the profile-leased chain worker. Querying an
/// existing transaction requires a provider, not a wallet or authority key.
pub(crate) async fn tick(journal: &Journal, bf: &BlockfrostClient) -> Result<(), String> {
    // Always recover locks first, so confirmed settlements can find them.
    for kind in [LOCK_KIND, SETTLE_KIND] {
        let ids = journal
            .run("escrow_recovery.scan", move |db| {
                submission::unapplied_operations(db.conn(), kind, 10)
            })
            .await?;
        for id in ids {
            let operation = Operation { kind, id: &id };
            match submission::reconcile(journal, bf, operation).await {
                Ok(Some(submitted)) => {
                    let id = id.clone();
                    if let Err(error) = journal
                        .run("escrow_recovery.project", move |db| {
                            project(db.conn(), Operation { kind, id: &id }, &submitted)
                        })
                        .await
                    {
                        log::error!("escrow recovery projection pending: {error}");
                    }
                }
                Ok(None) => return Err("escrow submission checkpoint missing".into()),
                Err(error) => log::debug!("escrow recovery pending: {error}"),
            }
        }
    }
    Ok(())
}
