//! Local, durable transaction identity across cancellation and profile locking.
//!
//! The first signed transaction binds an operation permanently. Commit its
//! exact bytes before sending; even a crash before the POST leaves an uncertain
//! outcome. Recovery only queries that identity, never constructs or submits a
//! replacement. Callers must check `lookup` before invoking builders or
//! applying local side effects.
//!
//! Every database phase runs as a short synchronous job on the profile-fenced
//! [`DatabaseExecutor`] through a [`Journal`]; network I/O happens only between
//! those jobs. The journal holds its profile lease for the whole operation.

use std::fmt;
use std::future::Future;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{
    blockfrost::{BlockfrostClient, TransactionReceipt},
    tx_builder,
};
use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::db::Database;
use crate::profile::scope::ProfileLease;

// The production Blockfrost client and transaction builders only support
// preprod. Network identity is not a project/API key or a particular provider.
const NETWORK: &str = "cardano-preprod";
const MAX_ERROR_CHARS: usize = 2048;

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

/// Why a submission was not handed to the network. No request was sent in
/// either case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitError {
    /// The profile database did not run the pre-send checkpoint (overloaded,
    /// locking, closed, or shutting down). Nothing was journaled; retry the
    /// same operation later without treating this as a failed attempt.
    Retryable(String),
    /// The operation was invalid, or its checkpoint was refused or failed to
    /// commit.
    Failed(String),
}

impl SubmitError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }
}

impl fmt::Display for SubmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retryable(message) | Self::Failed(message) => f.write_str(message),
        }
    }
}

impl From<SubmitError> for String {
    fn from(error: SubmitError) -> Self {
        error.to_string()
    }
}

/// Profile-fenced access to the submission journal and its domain projections.
///
/// Each phase is a short synchronous job on the profile's database executor,
/// in the lane of the work that initiated it; no job awaits network I/O. The
/// journal owns a clone of the profile lease, so profile resources cannot be
/// replaced while an operation still holds it, and queued phases of a locked
/// profile are refused before they start.
#[derive(Clone)]
pub(crate) struct Journal {
    executor: DatabaseExecutor,
    lease: ProfileLease,
    workload: DatabaseWorkload,
}

impl Journal {
    pub(crate) fn new(
        executor: DatabaseExecutor,
        lease: ProfileLease,
        workload: DatabaseWorkload,
    ) -> Self {
        Self {
            executor,
            lease,
            workload,
        }
    }

    /// Run one local phase. The outer error means the executor did not run the
    /// phase to completion (busy, profile changed, database closed, shutting
    /// down, or a panic that was rolled back); the inner result is the phase's.
    pub(crate) async fn try_run<T, F>(
        &self,
        label: &'static str,
        work: F,
    ) -> Result<Result<T, String>, String>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        self.executor
            .execute(self.workload, self.lease.clone(), label, move |database| {
                Ok(work(database))
            })
            .await
    }

    pub(crate) async fn run<T, F>(&self, label: &'static str, work: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        self.try_run(label, work).await?
    }

    /// Map a phase that precedes sending onto [`SubmitError`].
    pub(crate) async fn before_send<T, F>(
        &self,
        label: &'static str,
        work: F,
    ) -> Result<T, SubmitError>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        self.try_run(label, work)
            .await
            .map_err(SubmitError::Retryable)?
            .map_err(SubmitError::Failed)
    }
}

#[derive(Clone)]
struct OwnedOperation {
    kind: String,
    id: String,
}

impl OwnedOperation {
    fn new(operation: Operation<'_>) -> Self {
        Self {
            kind: operation.kind.to_owned(),
            id: operation.id.to_owned(),
        }
    }

    fn get(&self) -> Operation<'_> {
        Operation {
            kind: &self.kind,
            id: &self.id,
        }
    }
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

/// Direct access for tests that arrange or inspect state outside executor jobs.
#[cfg(test)]
pub(crate) fn with_database<T>(
    db: &std::sync::Arc<std::sync::Mutex<Option<Database>>>,
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
pub(crate) async fn submit_once(
    journal: &Journal,
    blockfrost: &BlockfrostClient,
    operation: Operation<'_>,
    signed_cbor: &[u8],
    context_json: &str,
) -> Result<Submission, SubmitError> {
    submit_with(
        journal,
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

pub(crate) async fn submit_once_with_members(
    journal: &Journal,
    blockfrost: &BlockfrostClient,
    operation: Operation<'_>,
    signed_cbor: &[u8],
    context_json: &str,
    members: &[Member<'_>],
) -> Result<Submission, SubmitError> {
    submit_members_with(
        journal,
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
    journal: &Journal,
    operation: Operation<'_>,
    signed_cbor: &[u8],
    context_json: &str,
    send: F,
) -> Result<Submission, SubmitError>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    submit_members_with(journal, operation, signed_cbor, context_json, &[], send).await
}

/// Durable pre-send checkpoint: one committed IMMEDIATE transaction on a
/// FULL-synchronous connection. Returns whether this call created the binding.
fn checkpoint(
    conn: &Connection,
    operation: Operation<'_>,
    tx_hash: &str,
    signed_cbor: &[u8],
    context_json: &str,
    members: &[Member<'_>],
) -> Result<(bool, Submission), String> {
    // A surrounding uncommitted transaction cannot serve as a durable
    // pre-send checkpoint. Refuse it, rather than silently using a savepoint.
    if !conn.is_autocommit() {
        return Err("submission requires a committed database boundary".into());
    }
    let synchronous: i64 = conn
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if synchronous < 2 {
        return Err("submission checkpoint requires SQLite synchronous FULL or stronger".into());
    }
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
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
}

async fn submit_members_with<F, Fut>(
    journal: &Journal,
    operation: Operation<'_>,
    signed_cbor: &[u8],
    context_json: &str,
    members: &[Member<'_>],
    send: F,
) -> Result<Submission, SubmitError>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    // Validate producer output before persisting it; the hash is derived
    // locally and never accepted on the authority of an HTTP response.
    let tx_hash =
        tx_builder::compute_tx_hash(signed_cbor).map_err(|e| SubmitError::Failed(e.to_string()))?;
    serde_json::from_str::<serde_json::Value>(context_json)
        .map_err(|e| SubmitError::Failed(e.to_string()))?;
    let owned = OwnedOperation::new(operation);
    let checkpoint_job = {
        let operation = owned.clone();
        let tx_hash = tx_hash.clone();
        let signed_cbor = signed_cbor.to_vec();
        let context_json = context_json.to_owned();
        let members: Vec<(String, String)> = members
            .iter()
            .map(|member| (member.kind.to_owned(), member.id.to_owned()))
            .collect();
        move |database: &Database| {
            let members: Vec<Member<'_>> = members
                .iter()
                .map(|(kind, id)| Member { kind, id })
                .collect();
            checkpoint(
                database.conn(),
                operation.get(),
                &tx_hash,
                &signed_cbor,
                &context_json,
                &members,
            )
        }
    };
    // Only a committed checkpoint authorizes sending. An executor refusal is
    // retryable and a failed commit is an error; neither is an uncertain outcome.
    let (inserted, original) = journal
        .before_send("submission.checkpoint", checkpoint_job)
        .await?;
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
        Err(error) => (
            false,
            Some(error.chars().take(MAX_ERROR_CHARS).collect::<String>()),
        ),
    };
    let recorded_error = error.clone();
    let recorded = journal
        .run("submission.acknowledge", move |database| {
            let conn = database.conn();
            let operation = owned.get();
            conn.execute(
                "UPDATE chain_submissions SET
                 status = CASE WHEN status = 'outcome_unknown' AND ?4
                               THEN 'submitted' ELSE status END,
                 last_error = CASE WHEN status IN ('confirmed', 'failed_on_chain') THEN last_error ELSE ?5 END,
                 updated_at = datetime('now')
             WHERE network = ?1 AND operation_kind = ?2 AND operation_id = ?3",
                params![
                    NETWORK,
                    operation.kind,
                    operation.id,
                    acknowledged,
                    recorded_error
                ],
            )
            .map_err(|e| e.to_string())?;
            lookup(conn, operation)?.ok_or_else(|| "submission checkpoint missing".into())
        })
        .await;
    Ok(recorded.unwrap_or_else(|failure| {
        // The request may have reached the network. The committed checkpoint
        // still records outcome_unknown, which recovery reconciles.
        let reason = match error {
            Some(error) => format!("{error}; response not recorded: {failure}"),
            None => format!("response not recorded: {failure}"),
        };
        Submission {
            status: SubmissionStatus::OutcomeUnknown,
            last_error: Some(reason.chars().take(MAX_ERROR_CHARS).collect()),
            ..original
        }
    }))
}

/// Reconcile only the journaled identity. Not found, unavailable, or timed out
/// is not proof of rejection and never makes the operation eligible to rebuild.
pub(crate) async fn reconcile(
    journal: &Journal,
    blockfrost: &BlockfrostClient,
    operation: Operation<'_>,
) -> Result<Option<Submission>, String> {
    reconcile_with(journal, operation, |hash| async move {
        blockfrost
            .get_transaction_receipt(&hash)
            .await
            .map_err(|e| e.to_string())
    })
    .await
}

fn is_final(submission: &Submission) -> bool {
    matches!(
        submission.status,
        SubmissionStatus::Confirmed | SubmissionStatus::FailedOnChain
    ) && submission.confirmed_slot.is_some()
}

async fn reconcile_with<F, Fut>(
    journal: &Journal,
    operation: Operation<'_>,
    check: F,
) -> Result<Option<Submission>, String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Option<TransactionReceipt>, String>>,
{
    let owned = OwnedOperation::new(operation);
    let read = owned.clone();
    let original = journal
        .run("submission.reconcile-read", move |database| {
            let conn = database.conn();
            let operation = read.get();
            let Some(original) = lookup(conn, operation)? else {
                return Ok(None);
            };
            if !is_final(&original) {
                // Rotate the fair recovery scan before the provider query.
                conn.execute(
                    "UPDATE chain_submissions SET updated_at = datetime('now')
                     WHERE network = ?1 AND operation_kind = ?2 AND operation_id = ?3",
                    params![NETWORK, operation.kind, operation.id],
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(Some(original))
        })
        .await?;
    let Some(original) = original else {
        return Ok(None);
    };
    if is_final(&original) {
        return Ok(Some(original));
    }
    let Some(receipt) = check(original.tx_hash.clone()).await? else {
        return journal
            .run("submission.reconcile-lookup", move |database| {
                lookup(database.conn(), owned.get())
            })
            .await;
    };
    if receipt.hash != original.tx_hash {
        return Err("transaction receipt hash mismatch".into());
    }
    let slot = i64::try_from(receipt.slot).map_err(|_| "transaction slot exceeds storage range")?;
    journal
        .run("submission.reconcile-receipt", move |database| {
            let conn = database.conn();
            let operation = owned.get();
            conn.execute(
                "UPDATE chain_submissions SET status = ?4, last_error = ?5,
                     confirmed_slot = ?6, updated_at = datetime('now')
                 WHERE network = ?1 AND operation_kind = ?2 AND operation_id = ?3 AND confirmed_slot IS NULL",
                params![NETWORK, operation.kind, operation.id,
                    if receipt.valid_contract { "confirmed" } else { "failed_on_chain" },
                    if receipt.valid_contract { None } else { Some("transaction included but script execution failed") }, slot],
            )
            .map_err(|e| e.to_string())?;
            lookup(conn, operation)
        })
        .await
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use super::*;
    use crate::cardano::test_chain::{self, FakeChain, Reply, TestProfile};
    use pallas_addresses::Address;
    use pallas_crypto::{hash::Hash, key::ed25519::SecretKey};
    use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
    use pallas_wallet::PrivateKey;

    const OPERATION: Operation<'static> = Operation {
        kind: "test",
        id: "operation-1",
    };
    const CONTEXT: &str = r#"{"version":1}"#;

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

    fn saved(profile: &TestProfile, operation: Operation<'_>) -> Option<Submission> {
        with_database(&profile.db, |conn| lookup(conn, operation)).unwrap()
    }

    async fn uncertain(journal: &Journal) -> Submission {
        submit_with(
            journal,
            OPERATION,
            &signed_transaction(200_000),
            CONTEXT,
            |_| async { Err("response timed out".into()) },
        )
        .await
        .unwrap()
    }

    /// Occupy the single database thread until the returned sender fires.
    async fn block_database(
        profile: &TestProfile,
    ) -> (
        std::sync::mpsc::Sender<()>,
        tokio::task::JoinHandle<Result<(), String>>,
    ) {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let journal = profile.journal(DatabaseWorkload::Learner);
        let blocker = tokio::spawn(async move {
            journal
                .run("test.block", move |_| {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                })
                .await
        });
        started_rx.await.unwrap();
        (release_tx, blocker)
    }

    fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }

    #[tokio::test]
    async fn cancellation_preserves_exact_bytes_after_encrypted_database_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile.db");
        let key = [9; 32];
        let database = Database::open_encrypted(&path, &key).unwrap();
        database.run_migrations().unwrap();
        let profile = TestProfile::new(database);
        let journal = profile.background();
        let bytes = signed_transaction(200_000);
        let expected_hash = tx_builder::compute_tx_hash(&bytes).unwrap();
        let (started, receiving) = tokio::sync::oneshot::channel();
        let task_journal = journal.clone();
        let task_bytes = bytes.clone();
        let task = tokio::spawn(async move {
            submit_with(
                &task_journal,
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
        let stored: Vec<u8> = reader
            .conn()
            .query_row("SELECT signed_cbor FROM chain_submissions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(stored, bytes);
        drop(reader);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        *profile.db.lock().unwrap() = None;
        *profile.db.lock().unwrap() = Some(Database::open_encrypted(&path, &key).unwrap());
        let restored = saved(&profile, OPERATION).unwrap();
        assert_eq!(restored.status, SubmissionStatus::OutcomeUnknown);
        assert_eq!(restored.tx_hash, expected_hash);
        let confirmed = reconcile_with(&journal, OPERATION, |hash| async move {
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
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let original = uncertain(&journal).await;
        let replacement = signed_transaction(300_000);
        assert_ne!(
            original.tx_hash,
            tx_builder::compute_tx_hash(&replacement).unwrap()
        );
        let repeated = submit_with(&journal, OPERATION, &replacement, CONTEXT, |_| async {
            panic!("must not POST again")
        })
        .await
        .unwrap();
        assert_eq!(original, repeated);
        let stored = with_database(&profile.db, |conn| {
            conn.query_row("SELECT signed_cbor FROM chain_submissions", [], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .map_err(|e| e.to_string())
        })
        .unwrap();
        assert_eq!(stored, signed_transaction(200_000));
    }

    #[tokio::test]
    async fn not_found_and_provider_failure_do_not_authorize_a_replacement() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let original = uncertain(&journal).await;
        assert_eq!(
            reconcile_with(&journal, OPERATION, |_| async { Ok(None) })
                .await
                .unwrap(),
            Some(original.clone())
        );
        assert!(reconcile_with(&journal, OPERATION, |_| async {
            Err("provider unavailable".into())
        })
        .await
        .is_err());
        assert_eq!(saved(&profile, OPERATION), Some(original));
    }

    #[tokio::test]
    async fn acknowledgement_must_match_locally_derived_hash_and_is_not_confirmation() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let bytes = signed_transaction(200_000);
        let expected = tx_builder::compute_tx_hash(&bytes).unwrap();
        let acknowledged = submit_with(&journal, OPERATION, &bytes, CONTEXT, |_| async move {
            Ok(expected)
        })
        .await
        .unwrap();
        assert_eq!(acknowledged.status, SubmissionStatus::Submitted);
        let other = Operation {
            id: "operation-2",
            ..OPERATION
        };
        let mismatched = submit_with(&journal, other, &bytes, CONTEXT, |_| async {
            Ok("0".repeat(64))
        })
        .await
        .unwrap();
        assert_eq!(mismatched.status, SubmissionStatus::OutcomeUnknown);
        assert!(mismatched.last_error.unwrap().contains("hash mismatch"));
    }

    #[tokio::test]
    async fn concurrent_retry_and_late_error_do_not_resend_or_downgrade_confirmation() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let (started, receiving) = tokio::sync::oneshot::channel();
        let (finish, response) = tokio::sync::oneshot::channel();
        let task_journal = journal.clone();
        let task = tokio::spawn(async move {
            submit_with(
                &task_journal,
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
            &journal,
            OPERATION,
            &signed_transaction(300_000),
            CONTEXT,
            |_| async { panic!("duplicate POST") },
        )
        .await
        .unwrap();
        assert_eq!(repeated.status, SubmissionStatus::OutcomeUnknown);
        reconcile_with(&journal, OPERATION, |hash| async {
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
        let profile = TestProfile::migrated();
        let journal = profile.background();
        with_database(&profile.db, |conn| {
            conn.execute_batch("BEGIN IMMEDIATE")
                .map_err(|e| e.to_string())
        })
        .unwrap();
        let result = submit_with(
            &journal,
            OPERATION,
            &signed_transaction(200_000),
            CONTEXT,
            |_| async { panic!("uncommitted checkpoint must not send") },
        )
        .await;
        let error = result.unwrap_err();
        assert!(!error.is_retryable());
        assert!(error.to_string().contains("committed database boundary"));
        with_database(&profile.db, |conn| {
            conn.execute_batch(
                "ROLLBACK; CREATE TRIGGER reject_checkpoint BEFORE INSERT ON chain_submissions
             BEGIN SELECT RAISE(ABORT, 'injected disk failure'); END;",
            )
            .map_err(|e| e.to_string())
        })
        .unwrap();
        let result = submit_with(
            &journal,
            OPERATION,
            &signed_transaction(200_000),
            CONTEXT,
            |_| async { panic!("failed checkpoint must not send") },
        )
        .await;
        let error = result.unwrap_err();
        assert!(
            matches!(&error, SubmitError::Failed(message) if message.contains("injected disk failure"))
        );
        assert!(saved(&profile, OPERATION).is_none());
    }

    #[tokio::test]
    async fn executor_overload_prevents_sending_and_is_retryable() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let (release, blocker) = block_database(&profile).await;
        // Fill the background lane; each first poll enqueues its job.
        let mut queued = Vec::new();
        let busy = loop {
            let mut filler = Box::pin(journal.run("test.fill", |_| Ok(())));
            match poll_once(filler.as_mut()) {
                Poll::Pending => queued.push(filler),
                Poll::Ready(result) => break result,
            }
            assert!(queued.len() <= 64, "background lane never filled");
        };
        assert_eq!(busy, Err("database is busy; retry the operation".into()));
        let error = submit_with(
            &journal,
            OPERATION,
            &signed_transaction(200_000),
            CONTEXT,
            |_| async { panic!("an overloaded checkpoint must not send") },
        )
        .await
        .unwrap_err();
        assert!(error.is_retryable(), "{error}");
        assert!(error.to_string().contains("busy"));
        release.send(()).unwrap();
        blocker.await.unwrap().unwrap();
        for filler in queued {
            filler.await.unwrap();
        }
        assert!(saved(&profile, OPERATION).is_none());
        // Once the pressure clears, the same operation is checkpointed and sent.
        let sent = uncertain(&journal).await;
        assert_eq!(sent.status, SubmissionStatus::OutcomeUnknown);
    }

    #[tokio::test]
    async fn stale_profile_lease_cancels_queued_checkpoint_before_send() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let (release, blocker) = block_database(&profile).await;
        let bytes = signed_transaction(200_000);
        let mut submit = Box::pin(submit_with(
            &journal,
            OPERATION,
            &bytes,
            CONTEXT,
            |_| async { panic!("a locked profile must not send") },
        ));
        // The first poll queues the checkpoint behind the blocked job.
        assert!(poll_once(submit.as_mut()).is_pending());
        let _ = profile.admission.close();
        release.send(()).unwrap();
        let error = submit.await.unwrap_err();
        assert!(error.is_retryable(), "{error}");
        assert!(error.to_string().contains("profile session changed"));
        blocker.await.unwrap().unwrap();
        assert!(saved(&profile, OPERATION).is_none());
    }

    #[tokio::test]
    async fn acknowledgement_after_profile_lock_is_not_recorded_and_stays_uncertain() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let bytes = signed_transaction(200_000);
        let hash = tx_builder::compute_tx_hash(&bytes).unwrap();
        let admission = profile.admission.clone();
        let result = submit_with(&journal, OPERATION, &bytes, CONTEXT, |_| async move {
            let _ = admission.close();
            Ok(hash)
        })
        .await
        .unwrap();
        assert_eq!(result.status, SubmissionStatus::OutcomeUnknown);
        assert!(result
            .last_error
            .unwrap()
            .contains("profile session changed"));
        let stored = saved(&profile, OPERATION).unwrap();
        assert_eq!(stored.status, SubmissionStatus::OutcomeUnknown);
        assert_eq!(stored.last_error, None);
    }

    #[tokio::test]
    async fn client_timeout_after_post_is_uncertain_and_recovery_never_reposts() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let bytes = signed_transaction(200_000);
        let expected = tx_builder::compute_tx_hash(&bytes).unwrap();
        let known = expected.clone();
        let chain = FakeChain::start(move |method, path, _| {
            if (method, path) == ("POST", "/tx/submit") {
                Reply::Stall
            } else if method == "GET" && path == format!("/txs/{known}") {
                Reply::Json(
                    200,
                    serde_json::json!({"hash": known, "slot": 42, "valid_contract": true})
                        .to_string(),
                )
            } else {
                Reply::Json(404, "{}".into())
            }
        })
        .await;
        let client = chain.client(test_chain::SHORT_LIMITS);
        let uncertain = submit_once(&journal, &client, OPERATION, &bytes, CONTEXT)
            .await
            .unwrap();
        assert_eq!(uncertain.status, SubmissionStatus::OutcomeUnknown);
        assert_eq!(uncertain.tx_hash, expected);
        assert!(uncertain.last_error.is_some());
        let retry = submit_once(
            &journal,
            &client,
            OPERATION,
            &signed_transaction(300_000),
            CONTEXT,
        )
        .await
        .unwrap();
        assert_eq!(retry, uncertain);
        let recovered = reconcile(&journal, &client, OPERATION)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recovered.status, SubmissionStatus::Confirmed);
        assert_eq!(recovered.confirmed_slot, Some(42));
        assert_eq!(
            chain.requests(),
            vec![
                "POST /tx/submit".to_string(),
                format!("GET /txs/{expected}")
            ]
        );
    }

    #[tokio::test]
    async fn conflicting_recovery_context_does_not_replace_original() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let original = uncertain(&journal).await;
        let result = submit_with(
            &journal,
            OPERATION,
            &signed_transaction(200_000),
            r#"{"version":2}"#,
            |_| async { panic!("conflicting intent must not send") },
        )
        .await;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("different recovery context"));
        assert_eq!(saved(&profile, OPERATION), Some(original));
        assert!(!crate::domain::sync::SYNCABLE_TABLES.contains(&"chain_submissions"));
    }

    #[tokio::test]
    async fn disabled_durable_commits_refuse_submission_before_transport() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        with_database(&profile.db, |conn| {
            conn.pragma_update(None, "synchronous", "NORMAL")
                .map_err(|e| e.to_string())
        })
        .unwrap();
        let result = submit_with(
            &journal,
            OPERATION,
            &signed_transaction(200_000),
            CONTEXT,
            |_| async { panic!("weak checkpoint must not send") },
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("synchronous FULL"));
        assert!(saved(&profile, OPERATION).is_none());
    }

    #[tokio::test]
    async fn overlapping_batch_rolls_back_checkpoint_and_new_members_before_sending() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let bytes = signed_transaction(200_000);
        let (started, receiving) = tokio::sync::oneshot::channel();
        let task_journal = journal.clone();
        let task_bytes = bytes.clone();
        let first = tokio::spawn(async move {
            submit_members_with(
                &task_journal,
                OPERATION,
                &task_bytes,
                CONTEXT,
                &[Member {
                    kind: "claim",
                    id: "shared",
                }],
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
            &journal,
            other,
            &bytes,
            CONTEXT,
            &[
                Member {
                    kind: "claim",
                    id: "fresh",
                },
                Member {
                    kind: "claim",
                    id: "shared",
                },
            ],
            |_| async { panic!("overlapping batch must not POST") },
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("already reserved"));
        assert!(saved(&profile, other).is_none());
        let members: i64 = with_database(&profile.db, |conn| {
            conn.query_row("SELECT COUNT(*) FROM chain_submission_members", [], |row| {
                row.get(0)
            })
            .map_err(|e| e.to_string())
        })
        .unwrap();
        assert_eq!(members, 1, "the fresh reservation must roll back too");
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        assert!(saved(&profile, OPERATION).is_some());
    }

    #[tokio::test]
    async fn repeated_batch_cannot_change_its_member_set() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let bytes = signed_transaction(200_000);
        let member = Member {
            kind: "claim",
            id: "original",
        };
        let first =
            submit_members_with(&journal, OPERATION, &bytes, CONTEXT, &[member], |_| async {
                Err("response lost".into())
            })
            .await
            .unwrap();
        let retry =
            submit_members_with(&journal, OPERATION, &bytes, CONTEXT, &[member], |_| async {
                panic!("duplicate send")
            })
            .await
            .unwrap();
        assert_eq!(retry, first);
        assert!(submit_members_with(
            &journal,
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
        .to_string()
        .contains("different members"));
        assert!(!crate::domain::sync::SYNCABLE_TABLES.contains(&"chain_submission_members"));
    }

    #[tokio::test]
    async fn failed_script_receipt_is_terminal_but_never_successful_execution() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        uncertain(&journal).await;
        let failed = reconcile_with(&journal, OPERATION, |hash| async {
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
            &journal,
            OPERATION,
            &signed_transaction(300_000),
            CONTEXT,
            |_| async { panic!("failed execution must not authorize automatic replacement") },
        )
        .await
        .unwrap();
        assert_eq!(repeated, failed);
        let repeated = reconcile_with(&journal, OPERATION, |_| async {
            panic!("terminal receipt is already saved")
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(repeated, failed);
    }

    #[tokio::test]
    async fn legacy_confirmation_without_receipt_requires_revalidation() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        uncertain(&journal).await;
        with_database(&profile.db, |conn| {
            conn.execute("UPDATE chain_submissions SET status = 'confirmed'", [])
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .unwrap();
        let failed = reconcile_with(&journal, OPERATION, |hash| async {
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
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let original = uncertain(&journal).await;
        let result = reconcile_with(&journal, OPERATION, |_| async {
            Ok(Some(TransactionReceipt {
                hash: "0".repeat(64),
                slot: 42,
                valid_contract: true,
            }))
        })
        .await;
        assert!(result.unwrap_err().contains("hash mismatch"));
        assert_eq!(saved(&profile, OPERATION), Some(original));
    }

    #[tokio::test]
    async fn interrupted_http_post_recovers_with_get_of_original_hash_only() {
        let profile = TestProfile::migrated();
        let journal = profile.background();
        let bytes = signed_transaction(200_000);
        let expected = tx_builder::compute_tx_hash(&bytes).unwrap();
        let (posted, receiving) = tokio::sync::oneshot::channel();
        let posted = std::sync::Mutex::new(Some(posted));
        let known = expected.clone();
        let chain = FakeChain::start(move |method, path, body| {
            if (method, path) == ("POST", "/tx/submit") {
                // Keep the first connection open without an acknowledgement.
                if let Some(posted) = posted.lock().unwrap().take() {
                    posted.send(body.to_vec()).unwrap();
                }
                Reply::Stall
            } else if method == "GET" && path == format!("/txs/{known}") {
                Reply::Json(
                    200,
                    serde_json::json!({"hash": known, "slot": 42, "valid_contract": true})
                        .to_string(),
                )
            } else {
                Reply::Json(404, "{}".into())
            }
        })
        .await;
        let client = chain.client(crate::cardano::blockfrost::RequestLimits::PRODUCTION);
        let task_journal = journal.clone();
        let task_client = client.clone();
        let task_bytes = bytes.clone();
        let submit = tokio::spawn(async move {
            submit_once(&task_journal, &task_client, OPERATION, &task_bytes, CONTEXT).await
        });
        let received = tokio::time::timeout(std::time::Duration::from_secs(5), receiving)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, bytes);
        submit.abort();
        assert!(submit.await.unwrap_err().is_cancelled());
        let stored = saved(&profile, OPERATION).unwrap();
        assert_eq!(stored.status, SubmissionStatus::OutcomeUnknown);
        assert_eq!(stored.tx_hash, expected);
        let recovered = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            reconcile(&journal, &client, OPERATION),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        assert_eq!(recovered.status, SubmissionStatus::Confirmed);
        assert_eq!(
            chain.requests(),
            vec![
                "POST /tx/submit".to_string(),
                format!("GET /txs/{expected}")
            ]
        );
    }
}
