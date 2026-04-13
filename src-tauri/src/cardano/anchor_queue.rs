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
