//! Local, durable transaction identity across cancellation and profile locking.
//!
//! The first signed transaction binds an operation permanently. Commit its
//! exact bytes before sending; even a crash before the POST leaves an uncertain
//! outcome. Recovery only queries that identity, never constructs or submits a
//! replacement. Callers must retain their profile lease throughout this work
//! and check `lookup` before invoking builders or applying local side effects.

use std::future::Future;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{
    blockfrost::{BlockfrostClient, TransactionReceipt},
    tx_builder,
};
use crate::db::Database;

// The production Blockfrost client and transaction builders only support
// preprod. Network identity is not a project/API key or a particular provider.
const NETWORK: &str = "cardano-preprod";
type SharedDatabase = Arc<Mutex<Option<Database>>>;

#[derive(Debug, Clone, Copy)]
pub struct Operation<'a> {
    pub kind: &'a str,
    pub id: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Member<'a> {
    pub kind: &'a str,
    pub id: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionStatus {
    OutcomeUnknown,
    Submitted,
    Confirmed,
    FailedOnChain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    pub tx_hash: String,
    pub status: SubmissionStatus,
    /// Caller-owned, versioned recovery context; never replaced on retry.
    pub context_json: String,
    pub last_error: Option<String>,
    pub confirmed_slot: Option<u64>,
}

pub fn lookup(conn: &Connection, operation: Operation<'_>) -> Result<Option<Submission>, String> {
    conn.query_row(
        "SELECT tx_hash, status, context_json, last_error, confirmed_slot FROM chain_submissions
         WHERE network = ?1 AND operation_kind = ?2 AND operation_id = ?3",
        params![NETWORK, operation.kind, operation.id],
        |row| {
            let status: String = row.get(1)?;
            let status = match status.as_str() {
                "outcome_unknown" => SubmissionStatus::OutcomeUnknown,
                "submitted" => SubmissionStatus::Submitted,
                "confirmed" => SubmissionStatus::Confirmed,
                "failed_on_chain" => SubmissionStatus::FailedOnChain,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(Submission {
                tx_hash: row.get(0)?,
                status,
                context_json: row.get(2)?,
                last_error: row.get(3)?,
                confirmed_slot: row
                    .get::<_, Option<i64>>(4)?
                    .map(|slot| {
                        u64::try_from(slot)
                            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(4, slot))
                    })
                    .transpose()?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub(crate) fn with_database<T>(
    db: &SharedDatabase,
    work: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    let guard = db.lock().map_err(|_| "database lock poisoned")?;
    let database = guard.as_ref().ok_or("database closed")?;
    work(database.conn())
}

/// Bounded, fair recovery scan. Final results remain here until their domain
/// projection and `mark_applied` commit together.
pub fn unapplied_operations(
    conn: &Connection,
    kind: &str,
    limit: u32,
) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "SELECT operation_id FROM chain_submissions WHERE network = ?1
         AND operation_kind = ?2 AND applied_at IS NULL ORDER BY updated_at, operation_id LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(params![NETWORK, kind, limit], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    rows
}

pub fn mark_applied(
    tx: &rusqlite::Transaction<'_>,
    operation: Operation<'_>,
) -> Result<(), String> {
    if tx
        .execute(
            "UPDATE chain_submissions SET applied_at = COALESCE(applied_at, datetime('now'))
         WHERE network = ?1 AND operation_kind = ?2 AND operation_id = ?3
           AND status IN ('confirmed', 'failed_on_chain') AND confirmed_slot IS NOT NULL",
            params![NETWORK, operation.kind, operation.id],
        )
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("cannot finish projection before a verified ledger receipt".into());
    }
    Ok(())
}

/// Submit only if this operation has never been journaled. A repeated call
/// returns the original identity, even if its caller built different bytes.
/// Different recovery context for the same operation is an error.
pub async fn submit_once(
    db: &SharedDatabase,
    blockfrost: &BlockfrostClient,
    operation: Operation<'_>,
    signed_cbor: &[u8],
    context_json: &str,
) -> Result<Submission, String> {
    submit_with(
        db,
        operation,
        signed_cbor,
        context_json,
        |bytes| async move {
            blockfrost
                .submit_tx(&bytes)
                .await
                .map_err(|e| e.to_string())
        },
    )
    .await
}

pub async fn submit_once_with_members(
    db: &SharedDatabase,
    blockfrost: &BlockfrostClient,
    operation: Operation<'_>,
    signed_cbor: &[u8],
    context_json: &str,
    members: &[Member<'_>],
) -> Result<Submission, String> {
    submit_members_with(
        db,
        operation,
        signed_cbor,
        context_json,
        members,
        |bytes| async move {
            blockfrost
                .submit_tx(&bytes)
                .await
                .map_err(|e| e.to_string())
        },
    )
    .await
}

async fn submit_with<F, Fut>(
    db: &SharedDatabase,
    operation: Operation<'_>,
    signed_cbor: &[u8],
    context_json: &str,
    send: F,
) -> Result<Submission, String>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    submit_members_with(db, operation, signed_cbor, context_json, &[], send).await
}

async fn submit_members_with<F, Fut>(
    db: &SharedDatabase,
    operation: Operation<'_>,
    signed_cbor: &[u8],
    context_json: &str,
    members: &[Member<'_>],
    send: F,
) -> Result<Submission, String>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    // Validate producer output before persisting it; the hash is derived
    // locally and never accepted on the authority of an HTTP response.
    let tx_hash = tx_builder::compute_tx_hash(signed_cbor).map_err(|e| e.to_string())?;
    serde_json::from_str::<serde_json::Value>(context_json).map_err(|e| e.to_string())?;
    let (inserted, original) = with_database(db, |conn| {
        // A surrounding uncommitted transaction cannot serve as a durable
        // pre-send checkpoint. Refuse it, rather than silently using a savepoint.
        if !conn.is_autocommit() {
            return Err("submission requires a committed database boundary".into());
        }
        let synchronous: i64 = conn
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if synchronous < 2 {
            return Err(
                "submission checkpoint requires SQLite synchronous FULL or stronger".into(),
            );
        }
        let tx =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(|e| e.to_string())?;
        let inserted = tx
            .execute(
                "INSERT INTO chain_submissions
             (network, operation_kind, operation_id, tx_hash, signed_cbor, context_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(network, operation_kind, operation_id) DO NOTHING",
                params![
                    NETWORK,
                    operation.kind,
                    operation.id,
                    tx_hash,
                    signed_cbor,
                    context_json
                ],
            )
            .map_err(|e| e.to_string())?
            == 1;
        let original = lookup(&tx, operation)?.ok_or("submission checkpoint missing")?;
        if original.context_json != context_json {
            return Err("operation is already bound to different recovery context".into());
        }
        let mut expected: Vec<(&str, &str)> = members.iter().map(|m| (m.kind, m.id)).collect();
        expected.sort_unstable();
        if expected.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("duplicate submission member".into());
        }
        if inserted {
            for member in members {
                tx.execute(
                    "INSERT INTO chain_submission_members
                     (network, member_kind, member_id, operation_kind, operation_id)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        NETWORK,
                        member.kind,
                        member.id,
                        operation.kind,
                        operation.id
                    ],
                )
                .map_err(|e| format!("submission member already reserved or invalid: {e}"))?;
            }
        } else {
            let mut statement = tx
                .prepare(
                    "SELECT member_kind, member_id FROM chain_submission_members WHERE network = ?1
                 AND operation_kind = ?2 AND operation_id = ?3 ORDER BY member_kind, member_id",
                )
                .map_err(|e| e.to_string())?;
            let actual = statement
                .query_map(params![NETWORK, operation.kind, operation.id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            if actual
                .iter()
                .map(|(kind, id)| (kind.as_str(), id.as_str()))
                .collect::<Vec<_>>()
                != expected
            {
                return Err("operation is already bound to different members".into());
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok((inserted, original))
    })?;
    if !inserted {
        return Ok(original);
    }

    // Cancellation anywhere after the durable INSERT leaves outcome_unknown.
    let response = send(signed_cbor.to_vec()).await;
    let (acknowledged, error) = match response {
        Ok(hash) if hash == tx_hash => (true, None),
        Ok(_) => (
            false,
            Some("submission response transaction hash mismatch".to_owned()),
        ),
        Err(error) => (false, Some(error.chars().take(2048).collect::<String>())),
    };
    with_database(db, |conn| {
        conn.execute(
            "UPDATE chain_submissions SET
                 status = CASE WHEN status = 'outcome_unknown' AND ?4
                               THEN 'submitted' ELSE status END,
                 last_error = CASE WHEN status IN ('confirmed', 'failed_on_chain') THEN last_error ELSE ?5 END,
                 updated_at = datetime('now')
             WHERE network = ?1 AND operation_kind = ?2 AND operation_id = ?3",
            params![NETWORK, operation.kind, operation.id, acknowledged, error],
        )
        .map_err(|e| e.to_string())?;
        lookup(conn, operation)?.ok_or_else(|| "submission checkpoint missing".into())
    })
}

/// Reconcile only the journaled identity. Not found, unavailable, or timed out
/// is not proof of rejection and never makes the operation eligible to rebuild.
pub async fn reconcile(
    db: &SharedDatabase,
    blockfrost: &BlockfrostClient,
    operation: Operation<'_>,
) -> Result<Option<Submission>, String> {
    reconcile_with(db, operation, |hash| async move {
        blockfrost
            .get_transaction_receipt(&hash)
            .await
            .map_err(|e| e.to_string())
    })
    .await
}

async fn reconcile_with<F, Fut>(
    db: &SharedDatabase,
    operation: Operation<'_>,
    check: F,
) -> Result<Option<Submission>, String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Option<TransactionReceipt>, String>>,
{
    let Some(original) = with_database(db, |conn| lookup(conn, operation))? else {
        return Ok(None);
    };
    if matches!(
        original.status,
        SubmissionStatus::Confirmed | SubmissionStatus::FailedOnChain
    ) && original.confirmed_slot.is_some()
    {
        return Ok(Some(original));
    }
    with_database(db, |conn| {
        conn.execute(
            "UPDATE chain_submissions SET updated_at = datetime('now')
             WHERE network = ?1 AND operation_kind = ?2 AND operation_id = ?3",
            params![NETWORK, operation.kind, operation.id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })?;
    if let Some(receipt) = check(original.tx_hash.clone()).await? {
        if receipt.hash != original.tx_hash {
            return Err("transaction receipt hash mismatch".into());
        }
        let slot =
            i64::try_from(receipt.slot).map_err(|_| "transaction slot exceeds storage range")?;
        with_database(db, |conn| {
            conn.execute(
                "UPDATE chain_submissions SET status = ?4, last_error = ?5,
                     confirmed_slot = ?6, updated_at = datetime('now')
                 WHERE network = ?1 AND operation_kind = ?2 AND operation_id = ?3 AND confirmed_slot IS NULL",
                params![NETWORK, operation.kind, operation.id,
                    if receipt.valid_contract { "confirmed" } else { "failed_on_chain" },
                    if receipt.valid_contract { None } else { Some("transaction included but script execution failed") }, slot],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })?;
    }
    with_database(db, |conn| lookup(conn, operation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pallas_addresses::Address;
    use pallas_crypto::{hash::Hash, key::ed25519::SecretKey};
    use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
    use pallas_wallet::PrivateKey;

    const OPERATION: Operation<'static> = Operation {
        kind: "test",
        id: "operation-1",
    };
    const CONTEXT: &str = r#"{"version":1}"#;

    fn test_db() -> SharedDatabase {
        let database = Database::open_in_memory().unwrap();
        database.run_migrations().unwrap();
        Arc::new(Mutex::new(Some(database)))
    }

    fn signed_transaction(fee: u64) -> Vec<u8> {
        let built = StagingTransaction::new()
            .input(Input::new(Hash::from([0x42; 32]), 0))
            .output(Output::new(Address::from_bech32(
                "addr_test1qz2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zex2k2xt3uqxgjqnnj83ws8lhrn648jjxtwq2ytjqp"
            ).unwrap(), 2_000_000))
            .fee(fee).network_id(0).build_conway_raw().unwrap();
        tx_builder::sign_raw_tx(
            &built.tx_bytes.0,
            &PrivateKey::Normal(SecretKey::from([7; 32])),
        )
        .unwrap()
    }

    async fn uncertain(db: &SharedDatabase) -> Submission {
        submit_with(
            db,
            OPERATION,
            &signed_transaction(200_000),
            CONTEXT,
            |_| async { Err("response timed out".into()) },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn cancellation_preserves_exact_bytes_after_encrypted_database_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile.db");
        let key = [9; 32];
        let database = Database::open_encrypted(&path, &key).unwrap();
        database.run_migrations().unwrap();
        let db = Arc::new(Mutex::new(Some(database)));
        let bytes = signed_transaction(200_000);
        let expected_hash = tx_builder::compute_tx_hash(&bytes).unwrap();
        let (started, receiving) = tokio::sync::oneshot::channel();
        let task_db = db.clone();
        let task_bytes = bytes.clone();
        let task = tokio::spawn(async move {
            submit_with(
                &task_db,
                OPERATION,
                &task_bytes,
                CONTEXT,
                |sent| async move {
                    started.send(sent).unwrap();
                    std::future::pending::<Result<String, String>>().await
                },
            )
            .await
        });
        assert_eq!(receiving.await.unwrap(), bytes);
        // A separate connection already sees committed data while the
        // response is still outstanding, not just after dropping the task.
        let reader = Database::open_encrypted(&path, &key).unwrap();
        let saved: Vec<u8> = reader
            .conn()
            .query_row("SELECT signed_cbor FROM chain_submissions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(saved, bytes);
        drop(reader);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        *db.lock().unwrap() = None;
        *db.lock().unwrap() = Some(Database::open_encrypted(&path, &key).unwrap());
        let restored = with_database(&db, |conn| lookup(conn, OPERATION))
            .unwrap()
            .unwrap();
        assert_eq!(restored.status, SubmissionStatus::OutcomeUnknown);
        assert_eq!(restored.tx_hash, expected_hash);
        let confirmed = reconcile_with(&db, OPERATION, |hash| async move {
            assert_eq!(hash, expected_hash);
            Ok(Some(TransactionReceipt {
                hash,
                slot: 42,
                valid_contract: true,
            }))
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(confirmed.status, SubmissionStatus::Confirmed);
    }

    #[tokio::test]
    async fn repeated_operation_never_sends_or_replaces_original_transaction() {
        let db = test_db();
        let original = uncertain(&db).await;
        let replacement = signed_transaction(300_000);
        assert_ne!(
            original.tx_hash,
            tx_builder::compute_tx_hash(&replacement).unwrap()
        );
        let repeated = submit_with(&db, OPERATION, &replacement, CONTEXT, |_| async {
            panic!("must not POST again")
        })
        .await
        .unwrap();
        assert_eq!(original, repeated);
        let saved = with_database(&db, |conn| {
            conn.query_row("SELECT signed_cbor FROM chain_submissions", [], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .map_err(|e| e.to_string())
        })
        .unwrap();
        assert_eq!(saved, signed_transaction(200_000));
    }

    #[tokio::test]
    async fn not_found_and_provider_failure_do_not_authorize_a_replacement() {
        let db = test_db();
        let original = uncertain(&db).await;
        assert_eq!(
            reconcile_with(&db, OPERATION, |_| async { Ok(None) })
                .await
                .unwrap(),
            Some(original.clone())
        );
        assert!(reconcile_with(&db, OPERATION, |_| async {
            Err("provider unavailable".into())
        })
        .await
        .is_err());
        assert_eq!(
            with_database(&db, |conn| lookup(conn, OPERATION)).unwrap(),
            Some(original)
        );
    }

    #[tokio::test]
    async fn acknowledgement_must_match_locally_derived_hash_and_is_not_confirmation() {
        let db = test_db();
        let bytes = signed_transaction(200_000);
        let expected = tx_builder::compute_tx_hash(&bytes).unwrap();
        let acknowledged = submit_with(
            &db,
            OPERATION,
            &bytes,
            CONTEXT,
            |_| async move { Ok(expected) },
        )
        .await
        .unwrap();
        assert_eq!(acknowledged.status, SubmissionStatus::Submitted);
        let other = Operation {
            id: "operation-2",
            ..OPERATION
        };
        let mismatched = submit_with(&db, other, &bytes, CONTEXT, |_| async {
            Ok("0".repeat(64))
        })
        .await
        .unwrap();
        assert_eq!(mismatched.status, SubmissionStatus::OutcomeUnknown);
        assert!(mismatched.last_error.unwrap().contains("hash mismatch"));
    }

    #[tokio::test]
    async fn concurrent_retry_and_late_error_do_not_resend_or_downgrade_confirmation() {
        let db = test_db();
        let (started, receiving) = tokio::sync::oneshot::channel();
        let (finish, response) = tokio::sync::oneshot::channel();
        let task_db = db.clone();
        let task = tokio::spawn(async move {
            submit_with(
                &task_db,
                OPERATION,
                &signed_transaction(200_000),
                CONTEXT,
                |_| async move {
                    started.send(()).unwrap();
                    response.await.unwrap()
                },
            )
            .await
        });
        receiving.await.unwrap();
        let repeated = submit_with(
            &db,
            OPERATION,
            &signed_transaction(300_000),
            CONTEXT,
            |_| async { panic!("duplicate POST") },
        )
        .await
        .unwrap();
        assert_eq!(repeated.status, SubmissionStatus::OutcomeUnknown);
        reconcile_with(&db, OPERATION, |hash| async {
            Ok(Some(TransactionReceipt {
                hash,
                slot: 42,
                valid_contract: true,
            }))
        })
        .await
        .unwrap();
        finish.send(Err("late transport error".into())).unwrap();
        let terminal = task.await.unwrap().unwrap();
        assert_eq!(terminal.status, SubmissionStatus::Confirmed);
        assert_eq!(terminal.last_error, None);
    }

    #[tokio::test]
    async fn uncommitted_or_failed_checkpoint_never_calls_transport() {
        let db = test_db();
        with_database(&db, |conn| {
            conn.execute_batch("BEGIN IMMEDIATE")
                .map_err(|e| e.to_string())
        })
        .unwrap();
        let result = submit_with(
            &db,
            OPERATION,
            &signed_transaction(200_000),
            CONTEXT,
            |_| async { panic!("uncommitted checkpoint must not send") },
        )
        .await;
        assert!(result.unwrap_err().contains("committed database boundary"));
        with_database(&db, |conn| {
            conn.execute_batch(
                "ROLLBACK; CREATE TRIGGER reject_checkpoint BEFORE INSERT ON chain_submissions
             BEGIN SELECT RAISE(ABORT, 'injected disk failure'); END;",
            )
            .map_err(|e| e.to_string())
        })
        .unwrap();
        let result = submit_with(
            &db,
            OPERATION,
            &signed_transaction(200_000),
            CONTEXT,
            |_| async { panic!("failed checkpoint must not send") },
        )
        .await;
        assert!(result.unwrap_err().contains("injected disk failure"));
        assert!(with_database(&db, |conn| lookup(conn, OPERATION))
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn conflicting_recovery_context_does_not_replace_original() {
        let db = test_db();
        let original = uncertain(&db).await;
        let result = submit_with(
            &db,
            OPERATION,
            &signed_transaction(200_000),
            r#"{"version":2}"#,
            |_| async { panic!("conflicting intent must not send") },
        )
        .await;
        assert!(result.unwrap_err().contains("different recovery context"));
        assert_eq!(
            with_database(&db, |conn| lookup(conn, OPERATION)).unwrap(),
            Some(original)
        );
        assert!(!crate::domain::sync::SYNCABLE_TABLES.contains(&"chain_submissions"));
    }

    #[tokio::test]
    async fn disabled_durable_commits_refuse_submission_before_transport() {
        let db = test_db();
        with_database(&db, |conn| {
            conn.pragma_update(None, "synchronous", "NORMAL")
                .map_err(|e| e.to_string())
        })
        .unwrap();
        let result = submit_with(
            &db,
            OPERATION,
            &signed_transaction(200_000),
            CONTEXT,
            |_| async { panic!("weak checkpoint must not send") },
        )
        .await;
        assert!(result.unwrap_err().contains("synchronous FULL"));
        assert!(with_database(&db, |conn| lookup(conn, OPERATION))
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn overlapping_batch_rolls_back_checkpoint_and_new_members_before_sending() {
        let db = test_db();
        let bytes = signed_transaction(200_000);
        let shared = Member {
            kind: "claim",
            id: "shared",
        };
        let (started, receiving) = tokio::sync::oneshot::channel();
        let task_db = db.clone();
        let task_bytes = bytes.clone();
        let first = tokio::spawn(async move {
            submit_members_with(
                &task_db,
                OPERATION,
                &task_bytes,
                CONTEXT,
                &[shared],
                |_| async {
                    started.send(()).unwrap();
                    std::future::pending::<Result<String, String>>().await
                },
            )
            .await
        });
        receiving.await.unwrap();
        let other = Operation {
            id: "overlapping-batch",
            ..OPERATION
        };
        let result = submit_members_with(
            &db,
            other,
            &bytes,
            CONTEXT,
            &[
                Member {
                    kind: "claim",
                    id: "fresh",
                },
                shared,
            ],
            |_| async { panic!("overlapping batch must not POST") },
        )
        .await;
        assert!(result.unwrap_err().contains("already reserved"));
        assert!(with_database(&db, |conn| lookup(conn, other))
            .unwrap()
            .is_none());
        let members: i64 = with_database(&db, |conn| {
            conn.query_row("SELECT COUNT(*) FROM chain_submission_members", [], |row| {
                row.get(0)
            })
            .map_err(|e| e.to_string())
        })
        .unwrap();
        assert_eq!(members, 1, "the fresh reservation must roll back too");
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        assert!(with_database(&db, |conn| lookup(conn, OPERATION))
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn repeated_batch_cannot_change_its_member_set() {
        let db = test_db();
        let bytes = signed_transaction(200_000);
        let member = Member {
            kind: "claim",
            id: "original",
        };
        let first = submit_members_with(&db, OPERATION, &bytes, CONTEXT, &[member], |_| async {
            Err("response lost".into())
        })
        .await
        .unwrap();
        let retry = submit_members_with(&db, OPERATION, &bytes, CONTEXT, &[member], |_| async {
            panic!("duplicate send")
        })
        .await
        .unwrap();
        assert_eq!(retry, first);
        assert!(submit_members_with(
            &db,
            OPERATION,
            &bytes,
            CONTEXT,
            &[Member {
                kind: "claim",
                id: "different"
            }],
            |_| async { panic!("different members must not send") }
        )
        .await
        .unwrap_err()
        .contains("different members"));
        assert!(!crate::domain::sync::SYNCABLE_TABLES.contains(&"chain_submission_members"));
    }

    #[tokio::test]
    async fn failed_script_receipt_is_terminal_but_never_successful_execution() {
        let db = test_db();
        uncertain(&db).await;
        let failed = reconcile_with(&db, OPERATION, |hash| async {
            Ok(Some(TransactionReceipt {
                hash,
                slot: 42,
                valid_contract: false,
            }))
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(failed.status, SubmissionStatus::FailedOnChain);
        assert_eq!(failed.confirmed_slot, Some(42));
        assert!(failed
            .last_error
            .as_ref()
            .unwrap()
            .contains("script execution failed"));
        let repeated = submit_with(
            &db,
            OPERATION,
            &signed_transaction(300_000),
            CONTEXT,
            |_| async { panic!("failed execution must not authorize automatic replacement") },
        )
        .await
        .unwrap();
        assert_eq!(repeated, failed);
        let repeated = reconcile_with(&db, OPERATION, |_| async {
            panic!("terminal receipt is already saved")
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(repeated, failed);
    }

    #[tokio::test]
    async fn legacy_confirmation_without_receipt_requires_revalidation() {
        let db = test_db();
        uncertain(&db).await;
        with_database(&db, |conn| {
            conn.execute("UPDATE chain_submissions SET status = 'confirmed'", [])
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .unwrap();
        let failed = reconcile_with(&db, OPERATION, |hash| async {
            Ok(Some(TransactionReceipt {
                hash,
                slot: 42,
                valid_contract: false,
            }))
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(failed.status, SubmissionStatus::FailedOnChain);
    }

    #[tokio::test]
    async fn unrelated_receipt_cannot_certify_original_transaction() {
        let db = test_db();
        let original = uncertain(&db).await;
        let result = reconcile_with(&db, OPERATION, |_| async {
            Ok(Some(TransactionReceipt {
                hash: "0".repeat(64),
                slot: 42,
                valid_contract: true,
            }))
        })
        .await;
        assert!(result.unwrap_err().contains("hash mismatch"));
        assert_eq!(
            with_database(&db, |conn| lookup(conn, OPERATION)).unwrap(),
            Some(original)
        );
    }

    #[tokio::test]
    async fn interrupted_http_post_recovers_with_get_of_original_hash_only() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        async fn read_request(stream: &mut tokio::net::TcpStream) -> (String, Vec<u8>) {
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 1024];
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0, "request closed before completion");
                bytes.extend_from_slice(&chunk[..count]);
                assert!(bytes.len() < 64 * 1024, "unexpected request size");
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
                    let length = header
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        return (header, bytes[end + 4..end + 4 + length].to_vec());
                    }
                }
            }
        }

        let db = test_db();
        let bytes = signed_transaction(200_000);
        let expected = tx_builder::compute_tx_hash(&bytes).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = BlockfrostClient::with_base_url(
            "test-project".into(),
            format!("http://{}", listener.local_addr().unwrap()),
        )
        .unwrap();
        let (posted, receiving) = tokio::sync::oneshot::channel();
        let server_expected = expected.clone();
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let (header, body) = read_request(&mut first).await;
            assert!(header.starts_with("POST /tx/submit HTTP/1.1"));
            posted.send(body).unwrap();
            // Keep the first connection open without an acknowledgement.
            let (mut second, _) = listener.accept().await.unwrap();
            let (header, body) = read_request(&mut second).await;
            assert!(header.starts_with(&format!("GET /txs/{server_expected} HTTP/1.1")));
            assert!(body.is_empty());
            let body =
                serde_json::json!({"hash": server_expected, "slot": 42, "valid_contract": true})
                    .to_string();
            second
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            drop(first);
        });
        let task_db = db.clone();
        let task_client = client.clone();
        let task_bytes = bytes.clone();
        let submit = tokio::spawn(async move {
            submit_once(&task_db, &task_client, OPERATION, &task_bytes, CONTEXT).await
        });
        let received = tokio::time::timeout(std::time::Duration::from_secs(5), receiving)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, bytes);
        submit.abort();
        assert!(submit.await.unwrap_err().is_cancelled());
        let saved = with_database(&db, |conn| lookup(conn, OPERATION))
            .unwrap()
            .unwrap();
        assert_eq!(saved.status, SubmissionStatus::OutcomeUnknown);
        assert_eq!(saved.tx_hash, expected);
        let recovered = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            reconcile(&db, &client, OPERATION),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        assert_eq!(recovered.status, SubmissionStatus::Confirmed);
        server.await.unwrap();
    }
}
