use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::blockfrost::BlockfrostClient;
use super::completion::{self, CompletionObservation};
use super::completion_tx_builder;
use super::script_refs;
use super::submission::{self, Journal, Operation, Submission, SubmissionStatus};
use crate::domain::completion::merkle_root;

pub const KIND: &str = "completion_witness";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompletionContext {
    pub version: u32,
    pub policy_id: String,
    pub course_id: String,
    pub subject_pubkey: [u8; 32],
    pub payment_key_hash: [u8; 28],
    pub leaves: Vec<[u8; 32]>,
    pub root: [u8; 32],
    pub mean_score: f64,
    pub timestamp_ms: i64,
}

impl CompletionContext {
    pub fn operation_id(&self) -> String {
        format!("{}.{}", self.policy_id, self.asset_name_hex())
    }

    fn asset_name_hex(&self) -> String {
        hex::encode(completion_tx_builder::completion_asset_name(
            &self.payment_key_hash,
            self.course_id.as_bytes(),
        ))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.policy_id != script_refs::COMPLETION_MINTING_SCRIPT_HASH
            || self.course_id.is_empty()
            || self.leaves.is_empty()
            || !self.mean_score.is_finite()
            || !(0.0..=1.0).contains(&self.mean_score)
            || chrono::DateTime::from_timestamp_millis(self.timestamp_ms).is_none()
        {
            return Err("invalid completion recovery context".into());
        }
        if merkle_root(&self.leaves) != self.root {
            return Err("completion recovery leaves do not match the original root".into());
        }
        Ok(())
    }

    pub fn original_for_retry(&self, submitted: &Submission) -> Result<Self, String> {
        self.validate()?;
        let original: Self = serde_json::from_str(&submitted.context_json)
            .map_err(|e| format!("invalid saved completion context: {e}"))?;
        original.validate()?;
        if original.policy_id != self.policy_id
            || original.course_id != self.course_id
            || original.subject_pubkey != self.subject_pubkey
            || original.payment_key_hash != self.payment_key_hash
            || original.leaves != self.leaves
            || original.root != self.root
            || original.mean_score != self.mean_score
        {
            return Err(format!(
                "completion evidence differs from preserved transaction {}; no replacement will be created",
                submitted.tx_hash
            ));
        }
        Ok(original)
    }

    fn observation(&self, tx_hash: &str) -> Result<CompletionObservation, String> {
        self.validate()?;
        let time = chrono::DateTime::from_timestamp_millis(self.timestamp_ms)
            .ok_or("invalid completion timestamp")?;
        Ok(CompletionObservation {
            policy_id: self.policy_id.clone(),
            asset_name_hex: self.asset_name_hex(),
            tx_hash: tx_hash.to_owned(),
            subject_pubkey: hex::encode(self.subject_pubkey),
            course_id: hex::encode(self.course_id.as_bytes()),
            completion_root: hex::encode(self.root),
            completion_time: time.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            credential_id: None,
            observed_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            issued_at: None,
        })
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
    if submitted.confirmed_slot.is_none() {
        return Err("completion requires a ledger receipt before projection".into());
    }
    let context: CompletionContext =
        serde_json::from_str(&submitted.context_json).map_err(|e| e.to_string())?;
    context.validate()?;
    if operation.kind != KIND || operation.id != context.operation_id() {
        return Err("completion recovery context does not match operation".into());
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    if submitted.status == SubmissionStatus::Confirmed {
        let obs = context.observation(&submitted.tx_hash)?;
        let existing = tx
            .query_row(
                "SELECT tx_hash, subject_pubkey, course_id, completion_root, completion_time
                 FROM completion_observations WHERE policy_id = ?1 AND asset_name_hex = ?2",
                rusqlite::params![obs.policy_id, obs.asset_name_hex],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(existing) = existing {
            if existing
                != (
                    obs.tx_hash,
                    obs.subject_pubkey,
                    obs.course_id,
                    obs.completion_root,
                    obs.completion_time,
                )
            {
                return Err(
                    "existing completion observation conflicts with preserved witness".into(),
                );
            }
        } else {
            completion::record_observation(&tx, &obs).map_err(|e| e.to_string())?;
        }
    }
    submission::mark_applied(&tx, operation)?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn ensure_unobserved(conn: &Connection, context: &CompletionContext) -> Result<(), String> {
    if completion::observation_exists(conn, &context.policy_id, &context.asset_name_hex())
        .map_err(|e| e.to_string())?
    {
        return Err("completion already has an observation; reconcile its original transaction before another mint".into());
    }
    Ok(())
}

pub(crate) async fn tick(journal: &Journal, bf: &BlockfrostClient) -> Result<(), String> {
    let ids = journal
        .run("completion_recovery.scan", |db| {
            submission::unapplied_operations(db.conn(), KIND, 10)
        })
        .await?;
    for id in ids {
        let operation = Operation {
            kind: KIND,
            id: &id,
        };
        match submission::reconcile(journal, bf, operation).await {
            Ok(Some(submitted)) => {
                let id = id.clone();
                if let Err(error) = journal
                    .run("completion_recovery.project", move |db| {
                        project(
                            db.conn(),
                            Operation {
                                kind: KIND,
                                id: &id,
                            },
                            &submitted,
                        )
                    })
                    .await
                {
                    log::warn!("completion recovery remains pending: {error}");
                }
            }
            Ok(None) => {}
            Err(error) => log::debug!("completion receipt remains pending: {error}"),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "completion_recovery_tests.rs"]
mod tests;
