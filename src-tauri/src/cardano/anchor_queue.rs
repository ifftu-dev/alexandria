//! Credential-hash integrity anchor queue. Stub — implementation in PR 8.
//!
//! Mirrors `cardano::onchain_queue` but for credential hashes. Each row
//! in `credential_anchors` points to a `credentials` row; the processor
//! builds a metadata-only Cardano tx (no mint) and records the tx hash
//! on success.

use std::sync::{Arc, Mutex};

use crate::db::Database;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorStatus {
    Pending,
    Submitted,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CredentialAnchor {
    pub credential_id: String,
    pub anchor_tx_hash: Option<String>,
    pub anchor_status: AnchorStatus,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub next_attempt_at: Option<String>,
}

/// Process one batch from the queue. Silently skips when
/// `BLOCKFROST_PROJECT_ID` is unset or the vault is locked; logs at
/// debug only so an idle node doesn't spam.
///
/// PR 8 ships the queue + idle-node contract. Actual on-chain
/// submission via `build_anchor_metadata_tx` is a follow-up tied
/// to a real testnet — until then the processor returns 0 and
/// leaves rows pending whenever the chain hooks are wired but the
/// builder is still stubbed.
pub async fn tick(
    db: &Arc<Mutex<Option<Database>>>,
    blockfrost: &Option<crate::cardano::blockfrost::BlockfrostClient>,
    wallet: &Option<crate::crypto::wallet::Wallet>,
) -> Result<u32, String> {
    // Idle-node contract: no chain credentials ⇒ no work, no error.
    if blockfrost.is_none() || wallet.is_none() {
        log::debug!("anchor_queue::tick: blockfrost or wallet unavailable, skipping");
        return Ok(0);
    }

    // Confirm the DB is open; otherwise this is also an idle skip.
    {
        let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
        if guard.is_none() {
            return Ok(0);
        }
    }

    // Builder is not yet wired — pending rows remain pending. Return 0
    // so the scheduler treats this as "nothing processed this tick".
    log::debug!("anchor_queue::tick: builder not yet wired, leaving pending rows");
    Ok(0)
}

/// Enqueue a credential for anchoring. Idempotent: a no-op insert
/// if the credential is already pending, submitted, or confirmed.
pub fn enqueue(db: &rusqlite::Connection, credential_id: &str) -> Result<(), String> {
    db.execute(
        "INSERT OR IGNORE INTO credential_anchors \
         (credential_id, anchor_status) VALUES (?1, 'pending')",
        rusqlite::params![credential_id],
    )
    .map_err(|e| format!("enqueue credential anchor: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_status_serializes_as_snake_case() {
        // Stored as `anchor_status` text column in `credential_anchors`.
        // The rename is what lets the DB layer round-trip via serde.
        assert_eq!(
            serde_json::to_string(&AnchorStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&AnchorStatus::Submitted).unwrap(),
            "\"submitted\""
        );
        assert_eq!(
            serde_json::to_string(&AnchorStatus::Confirmed).unwrap(),
            "\"confirmed\""
        );
        assert_eq!(
            serde_json::to_string(&AnchorStatus::Failed).unwrap(),
            "\"failed\""
        );
    }

    /// Insert a minimal credentials row so the FK on credential_anchors
    /// is satisfiable. Returns the inserted credential_id.
    fn seed_credential(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO credentials \
             (id, issuer_did, subject_did, credential_type, claim_kind, \
              issuance_date, signed_vc_json, integrity_hash) \
             VALUES (?1, 'did:key:zI', 'did:key:zS', 'FormalCredential', \
                     'skill', '2026-04-13T00:00:00Z', '{}', 'h')",
            rusqlite::params![id],
        )
        .unwrap();
    }

    #[test]
    fn enqueue_inserts_pending_row() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "cred-1");
        enqueue(db.conn(), "cred-1").unwrap();
        let status: String = db
            .conn()
            .query_row(
                "SELECT anchor_status FROM credential_anchors WHERE credential_id = ?1",
                rusqlite::params!["cred-1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn enqueue_is_idempotent_for_same_credential_id() {
        // Protocol §12.3 + queue convention: multiple enqueue calls for
        // the same credential MUST NOT create duplicate rows or flip
        // `confirmed` → `pending`.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "cred-1");
        enqueue(db.conn(), "cred-1").unwrap();
        enqueue(db.conn(), "cred-1").unwrap();
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credential_anchors WHERE credential_id = ?1",
                rusqlite::params!["cred-1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn tick_without_blockfrost_returns_zero_silently() {
        // Idle-node contract: no Blockfrost project id + no wallet
        // ⇒ tick is a silent no-op. Logs at debug only to avoid spam.
        let db = std::sync::Arc::new(std::sync::Mutex::new(Some(
            Database::open_in_memory().unwrap(),
        )));
        let processed = tick(&db, &None, &None).await.expect("tick ok");
        assert_eq!(processed, 0);
    }
}
