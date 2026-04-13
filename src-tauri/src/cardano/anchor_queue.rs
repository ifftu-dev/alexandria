//! Credential-hash integrity anchor queue.
//!
//! Mirrors `cardano::onchain_queue` but for credential hashes. Each row
//! in `credential_anchors` points to a `credentials` row; the processor
//! builds a metadata-only Cardano tx (no mint) via
//! `anchor_tx::build_anchor_metadata_tx`, submits via Blockfrost, and
//! records the resulting tx hash on success.

use std::sync::{Arc, Mutex};

use crate::cardano::anchor_tx;
use crate::crypto::did::Did;
use crate::db::Database;

/// Maximum number of rows processed per `tick` call. Caps work
/// per scheduler invocation so an idle node returning to a large
/// backlog doesn't block other tasks.
const TICK_BATCH: i64 = 10;

/// Maximum number of submission attempts before giving up. Mirrors
/// `onchain_queue::process_queue` (line ~270).
const MAX_ATTEMPTS: u32 = 5;

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
/// Returns the number of rows whose state changed (submitted, failed,
/// or marked permanently failed at MAX_ATTEMPTS).
pub async fn tick(
    db: &Arc<Mutex<Option<Database>>>,
    blockfrost: &Option<crate::cardano::blockfrost::BlockfrostClient>,
    wallet: &Option<crate::crypto::wallet::Wallet>,
) -> Result<u32, String> {
    // Idle-node contract: no chain credentials ⇒ no work, no error.
    let bf = match blockfrost {
        Some(c) => c,
        None => {
            log::debug!("anchor_queue::tick: blockfrost unavailable, skipping");
            return Ok(0);
        }
    };
    let w = match wallet {
        Some(w) => w,
        None => {
            log::debug!("anchor_queue::tick: wallet unavailable, skipping");
            return Ok(0);
        }
    };

    // Pull the pending batch. We hold the DB lock only for the SELECT
    // so the long-running Blockfrost calls below don't block other
    // commands.
    let batch = {
        let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
        let db_ref = match guard.as_ref() {
            Some(d) => d,
            None => return Ok(0),
        };
        load_pending(db_ref.conn())?
    };
    if batch.is_empty() {
        return Ok(0);
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut processed = 0u32;

    for row in &batch {
        // Hit max attempts before this run? Mark permanently failed
        // and move on. Mirror the onchain_queue convention.
        if row.attempts >= MAX_ATTEMPTS {
            let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
            let db_ref = guard.as_ref().ok_or("database closed")?;
            mark_failed_permanent(db_ref.conn(), &row.credential_id, &now)?;
            processed += 1;
            continue;
        }

        log::info!(
            "anchor_queue: processing {} (attempt {})",
            row.credential_id,
            row.attempts + 1
        );

        match build_and_submit(&row.credential_id, bf, w, db).await {
            Ok(tx_hash) => {
                let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
                let db_ref = guard.as_ref().ok_or("database closed")?;
                mark_submitted(db_ref.conn(), &row.credential_id, &tx_hash, &now)?;
                log::info!("anchor_queue: {} → {}", row.credential_id, tx_hash);
            }
            Err(e) => {
                let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
                let db_ref = guard.as_ref().ok_or("database closed")?;
                mark_failed_retryable(db_ref.conn(), &row.credential_id, &e, &now)?;
                log::warn!("anchor_queue: {} failed: {}", row.credential_id, e);
            }
        }
        processed += 1;
    }

    Ok(processed)
}

/// Fetch the credential's hash + issuer + issuance_date, build the
/// anchor tx, and submit. Pulled out so `tick` stays readable.
async fn build_and_submit(
    credential_id: &str,
    blockfrost: &crate::cardano::blockfrost::BlockfrostClient,
    wallet: &crate::crypto::wallet::Wallet,
    db: &Arc<Mutex<Option<Database>>>,
) -> Result<String, String> {
    let (integrity_hash, issuer_did, issuance_date) = {
        let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
        let db_ref = guard.as_ref().ok_or("database closed")?;
        db_ref
            .conn()
            .query_row(
                "SELECT integrity_hash, issuer_did, issuance_date FROM credentials \
                 WHERE id = ?1",
                rusqlite::params![credential_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(|e| format!("load credential: {e}"))?
    };

    let issuer = Did(issuer_did);
    let anchor = anchor_tx::build_anchor_metadata_tx(
        &integrity_hash,
        &issuer,
        &issuance_date,
        wallet,
        blockfrost,
    )
    .await?;
    blockfrost
        .submit_tx(&anchor.signed_cbor)
        .await
        .map_err(|e| format!("submit_tx: {e}"))?;
    Ok(anchor.tx_hash)
}

/// Pending rows ready to be processed *now* — `next_attempt_at` is
/// either NULL (never tried, or first attempt) or in the past.
fn load_pending(conn: &rusqlite::Connection) -> Result<Vec<CredentialAnchor>, String> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut stmt = conn
        .prepare(
            "SELECT credential_id, anchor_tx_hash, anchor_status, attempts, \
                    last_error, next_attempt_at \
             FROM credential_anchors \
             WHERE anchor_status = 'pending' \
               AND (next_attempt_at IS NULL OR next_attempt_at <= ?1) \
             ORDER BY enqueued_at \
             LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![&now, TICK_BATCH], |r| {
            let status: String = r.get(2)?;
            Ok(CredentialAnchor {
                credential_id: r.get(0)?,
                anchor_tx_hash: r.get(1)?,
                anchor_status: serde_json::from_str(&format!("\"{status}\""))
                    .unwrap_or(AnchorStatus::Pending),
                attempts: r.get::<_, i64>(3)? as u32,
                last_error: r.get(4)?,
                next_attempt_at: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn mark_submitted(
    conn: &rusqlite::Connection,
    credential_id: &str,
    tx_hash: &str,
    now: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE credential_anchors \
         SET anchor_status = 'submitted', anchor_tx_hash = ?2, \
             attempts = attempts + 1, last_error = NULL, \
             next_attempt_at = NULL, confirmed_at = ?3 \
         WHERE credential_id = ?1",
        rusqlite::params![credential_id, tx_hash, now],
    )
    .map_err(|e| format!("mark_submitted: {e}"))?;
    Ok(())
}

fn mark_failed_retryable(
    conn: &rusqlite::Connection,
    credential_id: &str,
    error: &str,
    now: &str,
) -> Result<(), String> {
    // Exponential backoff: next_attempt_at = now + 2^attempts minutes,
    // capped at 60 minutes so a long-broken row still gets retried
    // hourly. SQLite datetime arithmetic is delegated to the
    // `datetime(?, '+N minutes')` builtin so we don't have to format
    // timestamps in Rust.
    let attempts: i64 = conn
        .query_row(
            "SELECT attempts FROM credential_anchors WHERE credential_id = ?1",
            rusqlite::params![credential_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("read attempts: {e}"))?;
    let next_attempts = attempts + 1;
    let backoff_min = (1u32 << next_attempts.min(6) as u32).min(60);
    conn.execute(
        "UPDATE credential_anchors \
         SET attempts = ?2, last_error = ?3, \
             next_attempt_at = datetime(?4, '+' || ?5 || ' minutes') \
         WHERE credential_id = ?1",
        rusqlite::params![credential_id, next_attempts, error, now, backoff_min as i64],
    )
    .map_err(|e| format!("mark_failed_retryable: {e}"))?;
    Ok(())
}

fn mark_failed_permanent(
    conn: &rusqlite::Connection,
    credential_id: &str,
    now: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE credential_anchors \
         SET anchor_status = 'failed', \
             last_error = COALESCE(last_error, '') || ' (max attempts)', \
             next_attempt_at = ?2 \
         WHERE credential_id = ?1",
        rusqlite::params![credential_id, now],
    )
    .map_err(|e| format!("mark_failed_permanent: {e}"))?;
    Ok(())
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
