use std::sync::atomic::Ordering;

use super::*;
use crate::cardano::fake_ledger::{ada, FakeLedger};
use crate::cardano::test_chain::{self, FakeChain, TestProfile, SHORT_LIMITS};
use crate::db::Database;

fn context() -> CompletionContext {
    CompletionContext {
        version: 1,
        policy_id: script_refs::COMPLETION_MINTING_SCRIPT_HASH.to_owned(),
        course_id: "course".into(),
        subject_pubkey: [7; 32],
        payment_key_hash: [8; 28],
        leaves: vec![[9; 32]],
        root: [9; 32],
        mean_score: 0.8,
        timestamp_ms: 1_714_000_000_000,
    }
}

fn checkpoint(db: &Database, context: &CompletionContext, status: SubmissionStatus) -> Submission {
    let slot = matches!(
        status,
        SubmissionStatus::Confirmed | SubmissionStatus::FailedOnChain
    )
    .then_some(42);
    let submitted = Submission {
        tx_hash: "a".repeat(64),
        status,
        context_json: serde_json::to_string(context).unwrap(),
        last_error: None,
        confirmed_slot: slot,
    };
    db.conn()
        .execute(
            "INSERT INTO chain_submissions (network, operation_kind, operation_id,
         tx_hash, signed_cbor, invalid_from_slot, context_json, status, confirmed_slot)
         VALUES ('cardano-preprod', ?1, ?2, ?3, X'00', 0, ?4, ?5, ?6)",
            rusqlite::params![
                KIND,
                context.operation_id(),
                submitted.tx_hash,
                submitted.context_json,
                serde_json::to_value(status).unwrap().as_str().unwrap(),
                slot.map(|value| value as i64)
            ],
        )
        .unwrap();
    submitted
}

fn test_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().unwrap();
    db
}

#[test]
fn retry_preserves_original_time_and_rejects_changed_evidence_or_identity() {
    let db = test_db();
    let original = context();
    let saved = checkpoint(&db, &original, SubmissionStatus::OutcomeUnknown);
    let mut retry = original.clone();
    retry.timestamp_ms += 10_000;
    assert_eq!(retry.original_for_retry(&saved).unwrap(), original);
    retry.leaves = vec![[10; 32]];
    retry.root = [10; 32];
    assert_eq!(retry.operation_id(), original.operation_id());
    assert!(retry.original_for_retry(&saved).is_err());
    retry = original.clone();
    retry.subject_pubkey = [11; 32];
    assert!(retry.original_for_retry(&saved).is_err());
    retry = original.clone();
    retry.mean_score = 0.9;
    assert!(retry.original_for_retry(&saved).is_err());
    retry = original.clone();
    retry.root = [11; 32];
    assert!(retry.validate().is_err());
    retry = original.clone();
    retry.mean_score = f64::NAN;
    assert!(retry.validate().is_err());
}

#[test]
fn only_successful_receipt_creates_a_completion_observation() {
    for status in [
        SubmissionStatus::OutcomeUnknown,
        SubmissionStatus::Submitted,
        SubmissionStatus::FailedOnChain,
        SubmissionStatus::Confirmed,
    ] {
        let db = test_db();
        let context = context();
        let saved = checkpoint(&db, &context, status);
        let id = context.operation_id();
        let operation = Operation {
            kind: KIND,
            id: &id,
        };
        project(db.conn(), operation, &saved).unwrap();
        project(db.conn(), operation, &saved).unwrap();
        let pending = completion::pending_observations(db.conn()).unwrap();
        assert_eq!(
            pending.len(),
            usize::from(status == SubmissionStatus::Confirmed)
        );
        if let Some(obs) = pending.first() {
            assert_eq!(obs.tx_hash, saved.tx_hash);
            assert_eq!(obs.completion_root, hex::encode(context.root));
            assert_eq!(obs.subject_pubkey, hex::encode(context.subject_pubkey));
            assert_eq!(
                obs.completion_time,
                context.observation(&saved.tx_hash).unwrap().completion_time
            );
            assert!(ensure_unobserved(db.conn(), &context).is_err());
        }
        let unapplied = submission::unapplied_operations(db.conn(), KIND, 10).unwrap();
        assert_eq!(unapplied.is_empty(), saved.confirmed_slot.is_some());
    }
}

#[test]
fn interrupted_projection_rolls_back_observation_and_retries_original_evidence() {
    let db = test_db();
    let context = context();
    let mut saved = checkpoint(&db, &context, SubmissionStatus::Confirmed);
    let id = context.operation_id();
    let operation = Operation {
        kind: KIND,
        id: &id,
    };
    saved.confirmed_slot = None;
    assert!(project(db.conn(), operation, &saved).is_err());
    assert!(completion::pending_observations(db.conn())
        .unwrap()
        .is_empty());
    saved.confirmed_slot = Some(42);
    db.conn()
        .execute_batch(
            "CREATE TRIGGER fail_application BEFORE UPDATE OF applied_at
        ON chain_submissions BEGIN SELECT RAISE(ABORT, 'injected application failure'); END;",
        )
        .unwrap();
    assert!(project(db.conn(), operation, &saved)
        .unwrap_err()
        .contains("injected application failure"));
    assert!(completion::pending_observations(db.conn())
        .unwrap()
        .is_empty());
    assert_eq!(
        submission::unapplied_operations(db.conn(), KIND, 10).unwrap(),
        vec![id.clone()]
    );
    db.conn()
        .execute_batch("DROP TRIGGER fail_application")
        .unwrap();
    project(db.conn(), operation, &saved).unwrap();
    assert_eq!(
        completion::pending_observations(db.conn()).unwrap().len(),
        1
    );
    assert!(submission::unapplied_operations(db.conn(), KIND, 10)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn uncertain_witness_recovers_by_querying_its_original_transaction_only() {
    let db = test_db();
    let context = context();
    let saved = checkpoint(&db, &context, SubmissionStatus::OutcomeUnknown);
    let profile = TestProfile::new(db);
    let (chain, included) = FakeChain::stalled_submit(42).await;
    let client = chain.client(SHORT_LIMITS);
    let journal = profile.background();
    tick(&journal, &client).await.unwrap();
    assert!(profile.with_conn(|conn| completion::pending_observations(conn).unwrap().is_empty()));
    included.store(true, Ordering::Release);
    tick(&journal, &client).await.unwrap();
    let pending = profile.with_conn(|conn| completion::pending_observations(conn).unwrap());
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].tx_hash, saved.tx_hash);
    // Each pass reads the tip (expiry needs it), then asks only for the
    // original transaction.
    let tip = "GET /blocks/latest".to_owned();
    let query = format!("GET /txs/{}", saved.tx_hash);
    assert_eq!(
        chain.requests(),
        vec![tip.clone(), query.clone(), tip, query]
    );
}

#[test]
fn conflicting_observation_is_not_overwritten_and_issued_observation_stays_issued() {
    let db = test_db();
    let context = context();
    let saved = checkpoint(&db, &context, SubmissionStatus::Confirmed);
    let id = context.operation_id();
    let operation = Operation {
        kind: KIND,
        id: &id,
    };
    let mut obs = context.observation(&saved.tx_hash).unwrap();
    obs.completion_root = "b".repeat(64);
    completion::record_observation(db.conn(), &obs).unwrap();
    assert!(project(db.conn(), operation, &saved).is_err());
    assert_eq!(
        completion::pending_observations(db.conn()).unwrap()[0].completion_root,
        obs.completion_root
    );
    db.conn()
        .execute(
            "UPDATE completion_observations SET completion_root = ?1, credential_id = 'issued'",
            [hex::encode(context.root)],
        )
        .unwrap();
    project(db.conn(), operation, &saved).unwrap();
    assert!(completion::pending_observations(db.conn())
        .unwrap()
        .is_empty());
    assert_eq!(
        completion::find_by_asset(db.conn(), &context.policy_id, &context.asset_name_hex())
            .unwrap()
            .unwrap()
            .credential_id
            .as_deref(),
        Some("issued")
    );
}

#[tokio::test]
async fn tick_confirms_a_queued_mint_once_the_ledger_includes_it() {
    let wallet = test_chain::wallet();
    let context = CompletionContext {
        subject_pubkey: *wallet.signing_key.verifying_key().as_bytes(),
        payment_key_hash: wallet.payment_key_hash,
        ..context()
    };
    let db = test_db();
    db.conn()
        .execute(
            "INSERT INTO completion_claims (id, subject_did, course_id, completion_root, credential_ids_json)
             VALUES ('claim', 'subject', 'course', 'root', '[]')",
            [],
        )
        .unwrap();
    super::super::completion_queue::enqueue(db.conn(), "claim", &context).unwrap();
    let profile = TestProfile::new(db);
    let journal = profile.background();
    let ledger = FakeLedger::at_tip(120_000_000);
    ledger.set_utxos(
        &wallet.payment_address,
        vec![ada(0x11, 0, 20_000_000), ada(0x22, 0, 6_000_000)],
    );
    super::super::completion_queue::tick(&journal, &ledger, &wallet)
        .await
        .unwrap();
    let (tx_hash, _) = ledger.submissions().pop().unwrap();

    tick(&journal, &ledger).await.unwrap();
    assert!(profile.with_conn(|conn| completion::pending_observations(conn).unwrap().is_empty()));

    ledger.include(&tx_hash, 120_000_042);
    tick(&journal, &ledger).await.unwrap();
    let pending = profile.with_conn(|conn| completion::pending_observations(conn).unwrap());
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].tx_hash, tx_hash);
    let witness =
        profile.with_conn(|conn| super::super::completion_queue::state(conn, "claim").unwrap());
    assert_eq!(
        witness.status,
        super::super::completion_queue::WitnessStatus::Confirmed
    );
}

use super::super::completion_queue::{self as queue, WitnessStatus};
use super::super::reservations::ReservedOutpoints;
use super::super::tx_builder::TTL_OFFSET;
use pallas_traverse::MultiEraTx;

const TIP: u64 = 120_000_000;
/// First slot at which a transaction built at [`TIP`] can be expired.
const PAST_WINDOW: u64 = TIP + TTL_OFFSET + submission::EXPIRY_MARGIN_SLOTS + 1;

fn wallet_context(course: &str) -> CompletionContext {
    let wallet = test_chain::wallet();
    CompletionContext {
        course_id: course.into(),
        subject_pubkey: *wallet.signing_key.verifying_key().as_bytes(),
        payment_key_hash: wallet.payment_key_hash,
        ..context()
    }
}

/// A profile with one queued completion request for `context`.
fn queued(context: &CompletionContext) -> TestProfile {
    let db = test_db();
    db.conn()
        .execute(
            "INSERT INTO completion_claims (id, subject_did, course_id, completion_root, credential_ids_json)
             VALUES ('claim', 'subject', 'course', 'root', '[]')",
            [],
        )
        .unwrap();
    queue::enqueue(db.conn(), "claim", context).unwrap();
    TestProfile::new(db)
}

/// A transaction from this wallet that never went through this node's
/// journal (another node, or one from before reservations), spending the
/// wallet's largest UTxO.
async fn foreign_spend(ledger: &FakeLedger) -> String {
    let wallet = test_chain::wallet();
    let other = wallet_context("elsewhere");
    let built = completion_tx_builder::build_completion_mint_tx(
        ledger,
        &wallet.payment_address,
        &wallet.payment_key_hash,
        &wallet.payment_key_extended,
        &other.subject_pubkey,
        other.course_id.as_bytes(),
        &other.leaves,
        &other.root,
        other.timestamp_ms,
        TIP,
        &ReservedOutpoints::default(),
        None,
    )
    .await
    .unwrap();
    ledger.submit(&built.tx_cbor).await.unwrap()
}

/// What a completion mint attests: the token output with its inline datum,
/// and the redeemer carrying the leaves.
fn evidence(bytes: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let tx = MultiEraTx::decode(bytes).unwrap();
    let redeemers = tx
        .redeemers()
        .iter()
        .map(|redeemer| pallas_codec::minicbor::to_vec(redeemer.data()).unwrap())
        .collect();
    (tx.outputs()[0].encode(), redeemers)
}

fn inputs(bytes: &[u8]) -> Vec<String> {
    MultiEraTx::decode(bytes)
        .unwrap()
        .inputs()
        .iter()
        .map(|input| format!("{}#{}", input.hash(), input.index()))
        .collect()
}

fn witness(profile: &TestProfile) -> queue::WitnessState {
    profile.with_conn(|conn| queue::state(conn, "claim").unwrap())
}

#[tokio::test]
async fn double_spent_mint_expires_and_is_replaced_with_identical_evidence() {
    let wallet = test_chain::wallet();
    let context = wallet_context("course");
    let profile = queued(&context);
    let journal = profile.background();
    let ledger = FakeLedger::at_tip(TIP);
    ledger.set_utxos(
        &wallet.payment_address,
        vec![
            ada(0x11, 0, 40_000_000),
            ada(0x22, 0, 30_000_000),
            ada(0x33, 0, 20_000_000),
        ],
    );
    let foreign = foreign_spend(&ledger).await;

    queue::tick(&journal, &ledger, &wallet).await.unwrap();
    let (rejected_hash, rejected_bytes) = ledger.submissions().pop().unwrap();
    assert_ne!(rejected_hash, foreign, "the mint itself was sent");
    assert_eq!(witness(&profile).status, WitnessStatus::OutcomeUnknown);
    ledger.include(&foreign, TIP + 5);
    ledger.catch_up();

    // Inside the window, no receipt proves nothing.
    ledger.set_tip(PAST_WINDOW - 1);
    tick(&journal, &ledger).await.unwrap();
    assert_eq!(witness(&profile).status, WitnessStatus::OutcomeUnknown);

    ledger.set_tip(PAST_WINDOW);
    tick(&journal, &ledger).await.unwrap();
    let waiting = witness(&profile);
    assert_eq!(waiting.status, WitnessStatus::Pending);
    assert!(waiting.tx_hash.is_none());

    queue::tick(&journal, &ledger, &wallet).await.unwrap();
    let submissions = ledger.submissions();
    assert_eq!(submissions.len(), 3, "one replacement");
    let (replacement_hash, replacement_bytes) = submissions.last().unwrap().clone();
    assert_ne!(replacement_hash, rejected_hash);
    assert_eq!(evidence(&replacement_bytes), evidence(&rejected_bytes));
    assert_eq!(
        inputs(&rejected_bytes),
        vec![format!("{}#0", "11".repeat(32))]
    );
    assert_eq!(inputs(&replacement_bytes), vec![format!("{foreign}#1")]);
    let replaced = witness(&profile);
    assert_eq!(replaced.status, WitnessStatus::Submitted);
    assert_eq!(replaced.tx_hash.as_deref(), Some(replacement_hash.as_str()));
    let id = context.operation_id();
    let operation = Operation {
        kind: KIND,
        id: &id,
    };
    let history = profile.with_conn(|conn| submission::history(conn, operation).unwrap());
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].tx_hash, rejected_hash);
    assert_eq!(history[0].signed_cbor, rejected_bytes);
    let journaled = profile.with_conn(|conn| submission::lookup(conn, operation).unwrap().unwrap());
    assert_eq!(
        serde_json::from_str::<CompletionContext>(&journaled.context_json).unwrap(),
        context
    );

    ledger.include(&replacement_hash, PAST_WINDOW + 10);
    tick(&journal, &ledger).await.unwrap();
    assert_eq!(witness(&profile).status, WitnessStatus::Confirmed);
    let observed = profile.with_conn(|conn| completion::pending_observations(conn).unwrap());
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].tx_hash, replacement_hash);
}

/// A queued mint sent once and rejected as a double spend, with the tip past
/// its window. Returns the profile, the ledger and the rejected hash.
async fn rejected_mint(context: &CompletionContext) -> (TestProfile, FakeLedger, String) {
    let wallet = test_chain::wallet();
    let profile = queued(context);
    let ledger = FakeLedger::at_tip(TIP);
    ledger.set_utxos(
        &wallet.payment_address,
        vec![ada(0x11, 0, 40_000_000), ada(0x22, 0, 30_000_000)],
    );
    let foreign = foreign_spend(&ledger).await;
    queue::tick(&profile.background(), &ledger, &wallet)
        .await
        .unwrap();
    let (rejected, _) = ledger.submissions().pop().unwrap();
    assert_ne!(rejected, foreign);
    ledger.include(&foreign, TIP + 5);
    ledger.catch_up();
    ledger.set_tip(PAST_WINDOW);
    (profile, ledger, rejected)
}

#[tokio::test]
async fn expired_mint_is_not_replaced_with_different_evidence() {
    let wallet = test_chain::wallet();
    let context = wallet_context("course");
    let (profile, ledger, rejected) = rejected_mint(&context).await;
    let journal = profile.background();
    tick(&journal, &ledger).await.unwrap();
    assert_eq!(witness(&profile).status, WitnessStatus::Pending);

    // Same operation, different leaves: not what the original attested.
    let changed = CompletionContext {
        leaves: vec![[10; 32]],
        root: [10; 32],
        ..context.clone()
    };
    assert_eq!(changed.operation_id(), context.operation_id());
    profile.with_conn(|conn| {
        conn.execute(
            "UPDATE completion_witness_requests SET context_json = ?1",
            [serde_json::to_string(&changed).unwrap()],
        )
        .unwrap()
    });
    let sent = ledger.submissions().len();
    queue::tick(&journal, &ledger, &wallet).await.unwrap();

    assert_eq!(ledger.submissions().len(), sent, "no replacement is sent");
    let refused = witness(&profile);
    assert_eq!(refused.status, WitnessStatus::Unavailable);
    assert!(refused.tx_hash.is_none());
    let id = context.operation_id();
    let operation = Operation {
        kind: KIND,
        id: &id,
    };
    let journaled = profile.with_conn(|conn| submission::lookup(conn, operation).unwrap().unwrap());
    assert_eq!(journaled.status, SubmissionStatus::Expired);
    assert_eq!(journaled.tx_hash, rejected);
    assert!(profile.with_conn(|conn| submission::history(conn, operation).unwrap().is_empty()));
}

/// The fake ledger with its tip unavailable.
struct NoTip<'a>(&'a FakeLedger);

impl Ledger for NoTip<'_> {
    async fn utxos(&self, address: &str) -> Result<Vec<super::super::types::UTxO>, String> {
        self.0.utxos(address).await
    }
    async fn protocol_params(&self) -> Result<super::super::types::ProtocolParameters, String> {
        self.0.protocol_params().await
    }
    async fn tip_slot(&self) -> Result<u64, String> {
        Err("tip unavailable".into())
    }
    async fn evaluate(&self, tx: &[u8]) -> Result<Vec<(u64, u64)>, String> {
        self.0.evaluate(tx).await
    }
    async fn submit(&self, tx: &[u8]) -> Result<String, String> {
        self.0.submit(tx).await
    }
    async fn receipt(
        &self,
        tx_hash: &str,
    ) -> Result<Option<super::super::blockfrost::TransactionReceipt>, String> {
        self.0.receipt(tx_hash).await
    }
    async fn tx_cbor(&self, tx_hash: &str) -> Result<Option<Vec<u8>>, String> {
        self.0.tx_cbor(tx_hash).await
    }
}

#[tokio::test]
async fn mint_past_its_window_is_not_expired_while_the_tip_is_unavailable() {
    let context = wallet_context("course");
    let (profile, ledger, rejected) = rejected_mint(&context).await;
    let journal = profile.background();

    tick(&journal, &NoTip(&ledger)).await.unwrap();
    let waiting = witness(&profile);
    assert_eq!(waiting.status, WitnessStatus::OutcomeUnknown);
    assert_eq!(waiting.tx_hash.as_deref(), Some(rejected.as_str()));

    tick(&journal, &ledger).await.unwrap();
    assert_eq!(witness(&profile).status, WitnessStatus::Pending);
}
