//! Credential-hash integrity anchor queue.
//!
//! Mirrors `cardano::onchain_queue` but for credential hashes. Each row
//! in `credential_anchors` points to a `credentials` row; the processor
//! builds a metadata-only Cardano tx (no mint) via
//! `anchor_tx::build_anchor_metadata_tx`, submits via Blockfrost, and
//! records the resulting tx hash on success.

use std::sync::{Arc, Mutex};

use crate::cardano::anchor_tx;
use crate::cardano::submission::{self, Operation, Submission, SubmissionStatus};
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
    OutcomeUnknown,
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
        let operation = Operation {
            kind: "credential_anchor",
            id: &row.credential_id,
        };
        let existing = {
            let guard = db.lock().map_err(|_| "db lock poisoned")?;
            submission::lookup(guard.as_ref().ok_or("database closed")?.conn(), operation)?
        };
        if let Some(existing) = existing {
            // A crash may have left the queue row pending even though the
            // exact transaction was durably journaled. Never rebuild it.
            let recovered = match submission::reconcile(db, bf, operation).await {
                Ok(Some(recovered)) => recovered,
                Ok(None) => return Err("credential submission checkpoint missing".into()),
                Err(error) => {
                    log::debug!("credential anchor reconciliation: {error}");
                    existing
                }
            };
            let guard = db.lock().map_err(|_| "db lock poisoned")?;
            project_submission(
                guard.as_ref().ok_or("database closed")?.conn(),
                &row.credential_id,
                &recovered,
                &now,
            )?;
            processed += 1;
            continue;
        }
        if row.anchor_status != AnchorStatus::Pending {
            // Legacy submitted rows have no recoverable signed bytes. Keep
            // them out of the builder; absence of a journal is not a retry.
            continue;
        }
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
            Ok(submitted) => {
                let guard = db.lock().map_err(|_| "db lock poisoned".to_string())?;
                let db_ref = guard.as_ref().ok_or("database closed")?;
                project_submission(db_ref.conn(), &row.credential_id, &submitted, &now)?;
                log::info!(
                    "anchor_queue: {} → {:?} ({})",
                    row.credential_id,
                    submitted.status,
                    submitted.tx_hash
                );
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
) -> Result<Submission, String> {
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
    let context = serde_json::json!({"version": 1, "credential_id": credential_id,
        "integrity_hash": integrity_hash})
    .to_string();
    submission::submit_once(
        db,
        blockfrost,
        Operation {
            kind: "credential_anchor",
            id: credential_id,
        },
        &anchor.signed_cbor,
        &context,
    )
    .await
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
             WHERE (anchor_status = 'pending' OR \
               (anchor_status IN ('outcome_unknown', 'submitted') AND EXISTS \
                (SELECT 1 FROM chain_submissions s WHERE s.operation_kind = 'credential_anchor' \
                 AND s.operation_id = credential_id AND s.network = 'cardano-preprod'))) \
             AND \
               (next_attempt_at IS NULL OR julianday(next_attempt_at) <= julianday(?1)) \
             ORDER BY COALESCE(julianday(next_attempt_at), julianday(enqueued_at)), credential_id \
             LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![&now, TICK_BATCH], |r| {
            let status: String = r.get(2)?;
            Ok(CredentialAnchor {
                credential_id: r.get(0)?,
                anchor_tx_hash: r.get(1)?,
                anchor_status: serde_json::from_value(serde_json::Value::String(status))
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                attempts: r.get(3)?,
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

fn project_submission(
    conn: &rusqlite::Connection,
    credential_id: &str,
    submission: &Submission,
    now: &str,
) -> Result<(), String> {
    if matches!(
        submission.status,
        SubmissionStatus::Confirmed | SubmissionStatus::FailedOnChain
    ) && submission.confirmed_slot.is_none()
    {
        return Err("credential submission requires a ledger receipt before projection".into());
    }
    let status = match submission.status {
        SubmissionStatus::OutcomeUnknown => "outcome_unknown",
        SubmissionStatus::Submitted => "submitted",
        SubmissionStatus::Confirmed => "confirmed",
        SubmissionStatus::FailedOnChain => "failed",
    };
    conn.execute(
        "UPDATE credential_anchors \
         SET anchor_status = ?4, anchor_tx_hash = ?2, \
             attempts = CASE WHEN anchor_status = 'pending' THEN attempts + 1 ELSE attempts END, \
             last_error = ?5, next_attempt_at = ?3, \
             confirmed_at = CASE WHEN ?4 = 'confirmed' THEN COALESCE(confirmed_at, ?3) ELSE NULL END \
         WHERE credential_id = ?1 AND anchor_status != 'confirmed'",
        rusqlite::params![credential_id, submission.tx_hash, now, status, submission.last_error],
    )
    .map_err(|e| format!("project credential submission: {e}"))?;
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

/// Make an unsigned credential anchor eligible for another background attempt.
/// A row with durable signed bytes is never reset: it must reconcile that exact
/// transaction through the journal instead of creating a replacement.
pub fn enqueue_or_retry(db: &rusqlite::Connection, credential_id: &str) -> Result<(), String> {
    enqueue(db, credential_id)?;
    db.execute(
        "UPDATE credential_anchors
         SET anchor_status = 'pending', attempts = 0, last_error = NULL,
             next_attempt_at = NULL
         WHERE credential_id = ?1 AND anchor_status = 'failed'
           AND NOT EXISTS (
             SELECT 1 FROM chain_submissions
             WHERE network = 'cardano-preprod'
               AND operation_kind = 'credential_anchor'
               AND operation_id = ?1
           )",
        rusqlite::params![credential_id],
    )
    .map_err(|e| format!("retry credential anchor: {e}"))?;
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

    #[test]
    fn retry_resets_only_failures_without_signed_bytes() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "unsigned");
        seed_credential(db.conn(), "signed");
        for id in ["unsigned", "signed"] {
            enqueue(db.conn(), id).unwrap();
            db.conn()
                .execute(
                    "UPDATE credential_anchors SET anchor_status = 'failed', attempts = 5,
                        last_error = 'failed' WHERE credential_id = ?1",
                    [id],
                )
                .unwrap();
        }
        db.conn()
            .execute(
                "INSERT INTO chain_submissions
                 (network, operation_kind, operation_id, tx_hash, signed_cbor, context_json)
                 VALUES ('cardano-preprod', 'credential_anchor', 'signed', ?1, X'00', '{}')",
                ["a".repeat(64)],
            )
            .unwrap();

        enqueue_or_retry(db.conn(), "unsigned").unwrap();
        enqueue_or_retry(db.conn(), "signed").unwrap();

        let unsigned: (String, i64, Option<String>) = db
            .conn()
            .query_row(
                "SELECT anchor_status, attempts, last_error FROM credential_anchors
                 WHERE credential_id = 'unsigned'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(unsigned, ("pending".into(), 0, None));
        let signed: (String, i64, Option<String>) = db
            .conn()
            .query_row(
                "SELECT anchor_status, attempts, last_error FROM credential_anchors
                 WHERE credential_id = 'signed'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(signed, ("failed".into(), 5, Some("failed".into())));
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

    #[test]
    fn uncertain_projection_is_retry_visible_without_being_a_new_submission() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "cred");
        enqueue(db.conn(), "cred").unwrap();
        db.conn()
            .execute(
                "INSERT INTO chain_submissions
             (network, operation_kind, operation_id, tx_hash, signed_cbor, context_json)
             VALUES ('cardano-preprod', 'credential_anchor', 'cred', ?1, X'00', '{}')",
                ["a".repeat(64)],
            )
            .unwrap();
        let mut submission = Submission {
            tx_hash: "a".repeat(64),
            status: SubmissionStatus::OutcomeUnknown,
            context_json: "{}".into(),
            last_error: Some("response lost".into()),
            confirmed_slot: None,
        };
        let now = "2026-01-01T00:00:00Z";
        project_submission(db.conn(), "cred", &submission, now).unwrap();
        project_submission(db.conn(), "cred", &submission, now).unwrap();
        let rows = load_pending(db.conn()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].anchor_status, AnchorStatus::OutcomeUnknown);
        assert_eq!(
            rows[0].attempts, 1,
            "polling must not consume submission attempts"
        );
        let confirmed_at: Option<String> = db
            .conn()
            .query_row(
                "SELECT confirmed_at FROM credential_anchors WHERE credential_id = 'cred'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(confirmed_at, None);
        submission.status = SubmissionStatus::Confirmed;
        assert!(project_submission(db.conn(), "cred", &submission, now).is_err());
        assert_eq!(load_pending(db.conn()).unwrap().len(), 1);
        submission.confirmed_slot = Some(42);
        project_submission(db.conn(), "cred", &submission, now).unwrap();
        submission.status = SubmissionStatus::OutcomeUnknown;
        project_submission(db.conn(), "cred", &submission, now).unwrap();
        assert!(
            load_pending(db.conn()).unwrap().is_empty(),
            "late unknown must not downgrade confirmation"
        );
    }
}
