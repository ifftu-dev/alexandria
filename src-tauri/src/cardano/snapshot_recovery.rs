use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::blockfrost::BlockfrostClient;
use super::submission::{self, Operation, Submission, SubmissionStatus};
use super::{script_refs, snapshot};
use crate::crypto::wallet::Wallet;
use crate::db::Database;
use crate::domain::reputation::{OnChainSkillScore, ReputationRole, SnapshotRecord};

pub const KIND: &str = "reputation_snapshot";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotContext {
    pub version: u32,
    pub snapshot_id: String,
    pub actor_address: String,
    pub actor_did: String,
    pub owner_key_hash: [u8; 28],
    pub subject_id: String,
    pub role: ReputationRole,
    pub skills: Vec<OnChainSkillScore>,
    pub window_start_ms: i64,
    pub window_end_ms: i64,
    pub snapshot_at: String,
}

impl SnapshotContext {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.snapshot_id.is_empty()
            || self.subject_id.is_empty()
            || self.actor_address.is_empty()
            || !self.actor_did.starts_with("did:key:")
            || !matches!(
                self.role,
                ReputationRole::Learner | ReputationRole::Instructor
            )
            || self.window_start_ms > self.window_end_ms
            || chrono::DateTime::from_timestamp_millis(self.window_start_ms).is_none()
            || chrono::DateTime::from_timestamp_millis(self.window_end_ms).is_none()
            || chrono::DateTime::parse_from_rfc3339(&self.snapshot_at).is_err()
        {
            return Err("invalid frozen snapshot context".into());
        }
        for skill in &self.skills {
            let bytes = hex::decode(&skill.skill_id_bytes).map_err(|e| e.to_string())?;
            if bytes.is_empty()
                || bytes.len() > 16
                || skill.proficiency > 5
                || !(0..=1_000_000).contains(&skill.impact_score)
                || !(0..=10_000).contains(&skill.confidence)
                || skill.evidence_count < 0
            {
                return Err("invalid frozen snapshot skill score".into());
            }
        }
        Ok(())
    }

    pub fn validate_wallet(&self, wallet: &Wallet) -> Result<(), String> {
        self.validate()?;
        if self.actor_address != wallet.stake_address
            || self.owner_key_hash != wallet.payment_key_hash
            || self.actor_did
                != crate::crypto::did::did_from_verifying_key(&wallet.signing_key.verifying_key())
                    .as_str()
        {
            return Err("snapshot does not belong to the unlocked wallet".into());
        }
        Ok(())
    }

    fn asset_names(&self) -> (String, String) {
        let base = snapshot::reputation_base_name(&self.subject_id, &self.role);
        (
            hex::encode(snapshot::reference_asset_name(&base)),
            hex::encode(snapshot::user_asset_name(&base)),
        )
    }

    fn matches_record(&self, record: &SnapshotRecord) -> bool {
        let (reference, user) = self.asset_names();
        record.id == self.snapshot_id
            && record.actor_address == self.actor_address
            && record.subject_id == self.subject_id
            && record.role == self.role.as_str()
            && usize::try_from(record.skill_count).ok() == Some(self.skills.len())
            && record.ref_asset_name.as_deref() == Some(&reference)
            && record.user_asset_name.as_deref() == Some(&user)
            && record.snapshot_at == self.snapshot_at
    }
}

pub fn record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotRecord> {
    Ok(SnapshotRecord {
        id: row.get(0)?,
        actor_address: row.get(1)?,
        subject_id: row.get(2)?,
        role: row.get(3)?,
        skill_count: row.get(4)?,
        tx_status: row.get(5)?,
        tx_hash: row.get(6)?,
        policy_id: row.get(7)?,
        ref_asset_name: row.get(8)?,
        user_asset_name: row.get(9)?,
        error_message: row.get(10)?,
        snapshot_at: row.get(11)?,
        confirmed_at: row.get(12)?,
        snapshot_format: row.get(13)?,
        snapshot_scope: row.get(14)?,
        computation_spec: row.get(15)?,
        credential_id: row.get(16)?,
    })
}

pub fn record(conn: &Connection, id: &str) -> Result<SnapshotRecord, String> {
    conn.query_row(
        "SELECT rs.id, rs.actor_address, rs.subject_id, rs.role, rs.skill_count,
            CASE WHEN rs.credential_id IS NULL THEN rs.tx_status ELSE ca.anchor_status END,
            CASE WHEN rs.credential_id IS NULL THEN rs.tx_hash ELSE ca.anchor_tx_hash END,
            rs.policy_id, rs.ref_asset_name, rs.user_asset_name,
            CASE WHEN rs.credential_id IS NULL THEN rs.error_message ELSE ca.last_error END,
            rs.snapshot_at,
            CASE WHEN rs.credential_id IS NULL THEN rs.confirmed_at ELSE ca.confirmed_at END,
            rs.snapshot_format, rs.snapshot_scope, rs.computation_spec, rs.credential_id
         FROM reputation_snapshots rs
         LEFT JOIN credential_anchors ca ON ca.credential_id = rs.credential_id
         WHERE rs.id = ?1",
        [id],
        record_from_row,
    )
    .map_err(|e| format!("snapshot not found: {e}"))
}

pub fn freeze(conn: &Connection, context: &SnapshotContext) -> Result<(), String> {
    context.validate()?;
    let (reference, user) = context.asset_names();
    crate::db::with_transaction(conn, || {
        conn.execute("INSERT INTO reputation_snapshots (id, actor_address, subject_id, role,
            skill_count, ref_asset_name, user_asset_name, snapshot_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![context.snapshot_id, context.actor_address, context.subject_id, context.role.as_str(),
                context.skills.len() as i64, reference, user, context.snapshot_at]).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO reputation_snapshot_inputs (snapshot_id, context_json) VALUES (?1, ?2)",
            params![
                context.snapshot_id,
                serde_json::to_string(context).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

pub fn load(conn: &Connection, id: &str) -> Result<SnapshotContext, String> {
    let json: Option<String> = conn
        .query_row(
            "SELECT context_json FROM reputation_snapshot_inputs WHERE snapshot_id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let context: SnapshotContext = serde_json::from_str(&json.ok_or(
        "legacy snapshot has no frozen inputs; it is preserved but cannot be rebuilt or resubmitted",
    )?).map_err(|e| e.to_string())?;
    context.validate()?;
    if context.snapshot_id != id || !context.matches_record(&record(conn, id)?) {
        return Err("snapshot record differs from its preserved inputs".into());
    }
    Ok(context)
}

pub fn project(
    conn: &Connection,
    operation: Operation<'_>,
    observed: &Submission,
) -> Result<(), String> {
    if operation.kind != KIND {
        return Err("wrong snapshot operation kind".into());
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    // Re-read the journal inside this transaction: a late acknowledgement must
    // never downgrade a receipt already applied by another recovery caller.
    let saved = submission::lookup(&tx, operation)?.ok_or("snapshot journal missing")?;
    if saved.tx_hash != observed.tx_hash || saved.context_json != observed.context_json {
        return Err("snapshot recovery does not match its journal".into());
    }
    let context = load(&tx, operation.id)?;
    if serde_json::to_string(&context).map_err(|e| e.to_string())? != saved.context_json {
        return Err("snapshot journal differs from its frozen inputs".into());
    }
    let current = record(&tx, operation.id)?;
    if current
        .tx_hash
        .as_deref()
        .is_some_and(|hash| hash != saved.tx_hash)
        || current
            .policy_id
            .as_deref()
            .is_some_and(|policy| policy != script_refs::REPUTATION_MINTING_SCRIPT_HASH)
    {
        return Err("snapshot is bound to another transaction or policy".into());
    }
    let terminal = matches!(
        saved.status,
        SubmissionStatus::Confirmed | SubmissionStatus::FailedOnChain
    );
    if terminal && saved.confirmed_slot.is_none() {
        return Err("snapshot requires a ledger receipt".into());
    }
    let status = match saved.status {
        SubmissionStatus::OutcomeUnknown => "outcome_unknown",
        SubmissionStatus::Submitted => "submitted",
        SubmissionStatus::Confirmed => "confirmed",
        SubmissionStatus::FailedOnChain => "failed_on_chain",
    };
    tx.execute("UPDATE reputation_snapshots SET tx_status = ?2, tx_hash = ?3, policy_id = ?4,
        error_message = ?5, confirmed_at = CASE WHEN ?2 = 'confirmed' THEN COALESCE(confirmed_at, datetime('now')) ELSE NULL END
        WHERE id = ?1", params![operation.id, status, saved.tx_hash, script_refs::REPUTATION_MINTING_SCRIPT_HASH, saved.last_error])
        .map_err(|e| e.to_string())?;
    if terminal {
        submission::mark_applied(&tx, operation)?;
    }
    tx.commit().map_err(|e| e.to_string())
}

pub async fn tick(db: &Arc<Mutex<Option<Database>>>, bf: &BlockfrostClient) -> Result<(), String> {
    let ids =
        submission::with_database(db, |conn| submission::unapplied_operations(conn, KIND, 10))?;
    for id in ids {
        let operation = Operation {
            kind: KIND,
            id: &id,
        };
        match submission::reconcile(db, bf, operation).await {
            Ok(Some(saved)) => {
                if let Err(error) =
                    submission::with_database(db, |conn| project(conn, operation, &saved))
                {
                    log::warn!("snapshot projection remains pending: {error}");
                }
            }
            Ok(None) => {}
            Err(error) => log::debug!("snapshot receipt remains pending: {error}"),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "snapshot_recovery_tests.rs"]
mod tests;
