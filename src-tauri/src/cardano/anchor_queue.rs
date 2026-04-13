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
pub async fn tick(
    _db: &Arc<Mutex<Option<Database>>>,
    _blockfrost: &Option<crate::cardano::blockfrost::BlockfrostClient>,
    _wallet: &Option<crate::crypto::wallet::Wallet>,
) -> Result<u32, String> {
    unimplemented!("PR 8 — anchor queue processor")
}

/// Enqueue a credential for anchoring. Idempotent: no-op if the
/// credential is already pending, submitted, or confirmed.
pub fn enqueue(_db: &rusqlite::Connection, _credential_id: &str) -> Result<(), String> {
    unimplemented!("PR 8 — enqueue credential anchor")
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

    #[test]
    #[ignore = "pending PR 8 — anchor queue processor"]
    fn enqueue_inserts_pending_row() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
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
    #[ignore = "pending PR 8 — anchor queue processor"]
    fn enqueue_is_idempotent_for_same_credential_id() {
        // Protocol §12.3 + queue convention: multiple enqueue calls for
        // the same credential MUST NOT create duplicate rows or flip
        // `confirmed` → `pending`.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
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
    #[ignore = "pending PR 8 — anchor queue processor"]
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
