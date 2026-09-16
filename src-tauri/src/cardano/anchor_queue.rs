//! Credential-hash integrity anchor queue.
//!
//! Each row in `credential_anchors` points to a `credentials` row; the processor
//! builds a metadata-only Cardano tx (no mint) via
//! `anchor_tx::build_anchor_metadata_tx`, submits via Blockfrost, and
//! records the resulting tx hash on success.

use crate::cardano::anchor_tx;
use crate::cardano::blockfrost::BlockfrostClient;
use crate::cardano::submission::{
    self, Journal, Operation, Submission, SubmissionStatus, SubmitError,
};
use crate::crypto::did::Did;
use crate::crypto::wallet::Wallet;

const OPERATION_KIND: &str = "credential_anchor";

/// Maximum number of rows processed per `tick` call. Caps work
/// per scheduler invocation so an idle node returning to a large
/// backlog doesn't block other tasks.
const TICK_BATCH: i64 = 10;

/// Maximum number of submission attempts before a row is marked permanently
/// failed.
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
pub(crate) async fn tick(
    journal: &Journal,
    blockfrost: &Option<BlockfrostClient>,
    wallet: &Option<Wallet>,
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

    // Every local phase is a short executor job; the Blockfrost calls
    // below run between jobs and never hold the database.
    let batch = journal
        .run("anchor_queue.load", |db| load_pending(db.conn()))
        .await?;
    if batch.is_empty() {
        return Ok(0);
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut processed = 0u32;

    for row in batch {
        let credential_id = row.credential_id.clone();
        let operation = Operation {
            kind: OPERATION_KIND,
            id: &credential_id,
        };
        let lookup_id = credential_id.clone();
        let existing = journal
            .run("anchor_queue.lookup", move |db| {
                submission::lookup(
                    db.conn(),
                    Operation {
                        kind: OPERATION_KIND,
                        id: &lookup_id,
                    },
                )
            })
            .await?;
        if let Some(existing) = existing {
            // A crash may have left the queue row pending even though the
            // exact transaction was durably journaled. Never rebuild it.
            let recovered = match submission::reconcile(journal, bf, operation).await {
                Ok(Some(recovered)) => recovered,
                Ok(None) => return Err("credential submission checkpoint missing".into()),
                Err(error) => {
                    log::debug!("credential anchor reconciliation: {error}");
                    existing
                }
            };
            project_anchor(journal, &credential_id, recovered, &now).await?;
            processed += 1;
            continue;
        }
        if row.anchor_status != AnchorStatus::Pending {
            // Legacy submitted rows have no recoverable signed bytes. Keep
            // them out of the builder; absence of a journal is not a retry.
            continue;
        }
        // Hit max attempts before this run? Mark permanently failed
        // and move on.
        if row.attempts >= MAX_ATTEMPTS {
            let (id, failed_at) = (credential_id.clone(), now.clone());
            journal
                .run("anchor_queue.fail-permanent", move |db| {
                    mark_failed_permanent(db.conn(), &id, &failed_at)
                })
                .await?;
            processed += 1;
            continue;
        }

        log::info!(
            "anchor_queue: processing {} (attempt {})",
            credential_id,
            row.attempts + 1
        );

        match build_and_submit(&credential_id, bf, w, journal).await {
            Ok(submitted) => {
                log::info!(
                    "anchor_queue: {} → {:?} ({})",
                    credential_id,
                    submitted.status,
                    submitted.tx_hash
                );
                project_anchor(journal, &credential_id, submitted, &now).await?;
            }
            Err(SubmitError::Retryable(error)) => {
                // Nothing was journaled or sent, so no attempt is consumed.
                // The profile database is unavailable; end this pass.
                log::debug!("anchor_queue: {credential_id} deferred: {error}");
                return Ok(processed);
            }
            Err(SubmitError::Failed(error)) => {
                log::warn!("anchor_queue: {} failed: {}", credential_id, error);
                let (id, failed_at) = (credential_id.clone(), now.clone());
                journal
                    .run("anchor_queue.fail-retryable", move |db| {
                        mark_failed_retryable(db.conn(), &id, &error, &failed_at)
                    })
                    .await?;
            }
        }
        processed += 1;
    }

    Ok(processed)
}

async fn project_anchor(
    journal: &Journal,
    credential_id: &str,
    submission: Submission,
    now: &str,
) -> Result<(), String> {
    let (id, now) = (credential_id.to_owned(), now.to_owned());
    journal
        .run("anchor_queue.project", move |db| {
            project_submission(db.conn(), &id, &submission, &now)
        })
        .await
}

/// Fetch the credential's hash + issuer + issuance_date, build the
/// anchor tx, and submit. Pulled out so `tick` stays readable.
async fn build_and_submit(
    credential_id: &str,
    blockfrost: &BlockfrostClient,
    wallet: &Wallet,
    journal: &Journal,
) -> Result<Submission, SubmitError> {
    let id = credential_id.to_owned();
    let (integrity_hash, issuer_did, issuance_date) = journal
        .before_send("anchor_queue.credential", move |db| {
            db.conn()
                .query_row(
                    "SELECT integrity_hash, issuer_did, issuance_date FROM credentials \
                     WHERE id = ?1",
                    rusqlite::params![id],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    },
                )
                .map_err(|e| format!("load credential: {e}"))
        })
        .await?;

    let issuer = Did(issuer_did);
    let anchor = anchor_tx::build_anchor_metadata_tx(
        &integrity_hash,
        &issuer,
        &issuance_date,
        wallet,
        blockfrost,
    )
    .await
    .map_err(SubmitError::Failed)?;
    let context = serde_json::json!({"version": 1, "credential_id": credential_id,
        "integrity_hash": integrity_hash})
    .to_string();
    submission::submit_once(
        journal,
        blockfrost,
        Operation {
            kind: OPERATION_KIND,
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
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::cardano::test_chain::{self, FakeChain, TestProfile, SHORT_LIMITS};
    use crate::db::Database;

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
        let profile = TestProfile::new(Database::open_in_memory().unwrap());
        let processed = tick(&profile.background(), &None, &None)
            .await
            .expect("tick ok");
        assert_eq!(processed, 0);
    }

    fn anchor_row(profile: &TestProfile, id: &str) -> (String, i64, Option<String>) {
        profile.with_conn(|conn| {
            conn.query_row(
                "SELECT anchor_status, attempts, anchor_tx_hash FROM credential_anchors
                 WHERE credential_id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
        })
    }

    #[tokio::test]
    async fn confirmed_anchor_is_never_reprocessed_with_live_credentials() {
        let profile = TestProfile::migrated();
        profile.with_conn(|conn| {
            seed_credential(conn, "cred-confirmed");
            conn.execute(
                "INSERT INTO credential_anchors (credential_id, anchor_status, anchor_tx_hash, attempts)
                 VALUES ('cred-confirmed', 'confirmed', 'tx_abc', 1)",
                [],
            )
            .unwrap();
        });
        let (chain, _) = FakeChain::stalled_submit(42).await;
        let processed = tick(
            &profile.background(),
            &Some(chain.client(SHORT_LIMITS)),
            &Some(test_chain::wallet()),
        )
        .await
        .unwrap();
        assert_eq!(processed, 0);
        assert!(chain.requests().is_empty(), "no build, submit or query");
        assert_eq!(
            anchor_row(&profile, "cred-confirmed"),
            ("confirmed".into(), 1, Some("tx_abc".into()))
        );
    }

    #[tokio::test]
    async fn timed_out_anchor_is_reconciled_without_rebuild_or_resubmission() {
        let profile = TestProfile::migrated();
        profile.with_conn(|conn| {
            seed_credential(conn, "cred");
            enqueue(conn, "cred").unwrap();
        });
        let (chain, included) = FakeChain::stalled_submit(42).await;
        let client = Some(chain.client(SHORT_LIMITS));
        let wallet = Some(test_chain::wallet());
        let journal = profile.background();

        // The POST reaches the provider, then the client deadline expires.
        assert_eq!(tick(&journal, &client, &wallet).await.unwrap(), 1);
        let (status, attempts, hash) = anchor_row(&profile, "cred");
        assert_eq!((status.as_str(), attempts), ("outcome_unknown", 1));
        let hash = hash.unwrap();
        let journaled = profile.with_conn(|conn| {
            submission::lookup(
                conn,
                Operation {
                    kind: OPERATION_KIND,
                    id: "cred",
                },
            )
            .unwrap()
            .unwrap()
        });
        assert_eq!(journaled.tx_hash, hash);
        assert!(journaled.last_error.is_some());

        // "Not found" is not a rejection and never authorizes a rebuild.
        tick(&journal, &client, &wallet).await.unwrap();
        assert_eq!(
            anchor_row(&profile, "cred"),
            ("outcome_unknown".into(), 1, Some(hash.clone()))
        );

        included.store(true, Ordering::Release);
        tick(&journal, &client, &wallet).await.unwrap();
        assert_eq!(
            anchor_row(&profile, "cred"),
            ("confirmed".into(), 1, Some(hash.clone()))
        );
        let address = &wallet.as_ref().unwrap().payment_address;
        assert_eq!(chain.count("POST /tx/submit"), 1);
        assert_eq!(chain.count(&format!("GET /addresses/{address}/utxos")), 1);
        assert_eq!(chain.count(&format!("GET /txs/{hash}")), 2);
        assert_eq!(chain.requests().len(), 6);
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
