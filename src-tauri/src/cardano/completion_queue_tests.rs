use super::*;

fn wallet() -> Wallet {
    crate::crypto::wallet::wallet_from_mnemonic(
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
    ).unwrap()
}

fn context() -> CompletionContext {
    let wallet = wallet();
    CompletionContext {
        version: 1,
        policy_id: super::super::script_refs::COMPLETION_MINTING_SCRIPT_HASH.to_owned(),
        course_id: "course".into(),
        subject_pubkey: *wallet.signing_key.verifying_key().as_bytes(),
        payment_key_hash: wallet.payment_key_hash,
        leaves: vec![[9; 32]],
        root: [9; 32],
        mean_score: 0.8,
        timestamp_ms: 1_714_000_000_000,
    }
}

fn claim(conn: &Connection, id: &str) {
    conn.execute("INSERT INTO completion_claims (id, subject_did, course_id, completion_root, credential_ids_json)
        VALUES (?1, 'subject', 'course', ?1, '[]')", [id]).unwrap();
}

fn test_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().unwrap();
    db
}

fn checkpoint(conn: &Connection, context: &CompletionContext, status: SubmissionStatus) {
    let slot = matches!(
        status,
        SubmissionStatus::Confirmed | SubmissionStatus::FailedOnChain
    )
    .then_some(42);
    conn.execute(
        "INSERT INTO chain_submissions (network, operation_kind, operation_id, tx_hash, signed_cbor, context_json, status, confirmed_slot)
         VALUES ('cardano-preprod', ?1, ?2, ?3, X'00', ?4, ?5, ?6)",
        params![KIND, context.operation_id(), "a".repeat(64), serde_json::to_string(context).unwrap(),
            serde_json::to_value(status).unwrap().as_str().unwrap(), slot],
    ).unwrap();
}

#[test]
fn unsigned_request_survives_reopen_and_freezes_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("completion.db");
    let db = Database::open(&path).unwrap();
    db.run_migrations().unwrap();
    let original = context();
    claim(db.conn(), "first");
    enqueue(db.conn(), "first", &original).unwrap();
    assert_eq!(
        state(db.conn(), "first").unwrap().status,
        WitnessStatus::Pending
    );
    assert!(state(db.conn(), "first").unwrap().tx_hash.is_none());
    drop(db);
    let db = Database::open(&path).unwrap();
    db.run_migrations().unwrap();
    let mut retry = original.clone();
    retry.timestamp_ms += 1234;
    enqueue(db.conn(), "first", &retry).unwrap();
    let (id, json) = next_request(db.conn()).unwrap().unwrap();
    assert_eq!(id, original.operation_id());
    assert_eq!(
        serde_json::from_str::<CompletionContext>(&json).unwrap(),
        original
    );
    let mut different = original.clone();
    different.root = [10; 32];
    different.leaves = vec![[10; 32]];
    claim(db.conn(), "changed-evidence");
    enqueue(db.conn(), "changed-evidence", &different).unwrap();
    let status = state(db.conn(), "changed-evidence").unwrap();
    assert_eq!(status.status, WitnessStatus::Unavailable);
    assert!(status.tx_hash.is_none());
    assert_eq!(next_request(db.conn()).unwrap().unwrap().1, json);
}

#[tokio::test]
async fn every_signed_status_is_excluded_from_building_or_resubmission() {
    let bf = BlockfrostClient::with_base_url("test".into(), "http://127.0.0.1:1".into()).unwrap();
    for (saved_status, expected) in [
        (
            SubmissionStatus::OutcomeUnknown,
            WitnessStatus::OutcomeUnknown,
        ),
        (SubmissionStatus::Submitted, WitnessStatus::Submitted),
        (SubmissionStatus::Confirmed, WitnessStatus::Confirmed),
        (
            SubmissionStatus::FailedOnChain,
            WitnessStatus::FailedOnChain,
        ),
    ] {
        let db = test_db();
        let context = context();
        claim(db.conn(), "claim");
        checkpoint(db.conn(), &context, saved_status);
        enqueue(db.conn(), "claim", &context).unwrap();
        assert!(next_request(db.conn()).unwrap().is_none());
        assert_eq!(state(db.conn(), "claim").unwrap().status, expected);
        let db = Arc::new(Mutex::new(Some(db)));
        tick(&db, &bf, &wallet()).await.unwrap();
        submission::with_database(&db, |conn| {
            let attempts: i64 = conn
                .query_row(
                    "SELECT attempts FROM completion_witness_requests",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                attempts, 0,
                "no build or provider call for a signed operation"
            );
            Ok(())
        })
        .unwrap();
    }
}

#[tokio::test]
async fn wrong_wallet_request_is_blocked_before_io() {
    let db = test_db();
    claim(db.conn(), "claim");
    let mut context = context();
    context.subject_pubkey = [3; 32];
    enqueue(db.conn(), "claim", &context).unwrap();
    let db = Arc::new(Mutex::new(Some(db)));
    let bf = BlockfrostClient::with_base_url("test".into(), "http://127.0.0.1:1".into()).unwrap();
    tick(&db, &bf, &wallet()).await.unwrap();
    submission::with_database(&db, |conn| {
        assert_eq!(
            state(conn, "claim").unwrap().status,
            WitnessStatus::Unavailable
        );
        assert!(next_request(conn)?.is_none());
        Ok(())
    })
    .unwrap();
}

#[test]
fn old_signed_evidence_cannot_be_attached_to_different_local_claim() {
    let db = test_db();
    let original = context();
    checkpoint(db.conn(), &original, SubmissionStatus::Submitted);
    claim(db.conn(), "claim");
    let mut different = original.clone();
    different.root = [11; 32];
    different.leaves = vec![[11; 32]];
    enqueue(db.conn(), "claim", &different).unwrap();
    assert_eq!(
        state(db.conn(), "claim").unwrap().status,
        WitnessStatus::Unavailable
    );
    assert!(state(db.conn(), "claim").unwrap().tx_hash.is_none());
    assert!(next_request(db.conn()).unwrap().is_none());
}

#[test]
fn configuration_alone_does_not_request_witnesses_for_local_claims() {
    let db = test_db();
    claim(db.conn(), "local");
    assert_eq!(
        state(db.conn(), "local").unwrap().status,
        WitnessStatus::NotRequested
    );
    assert!(next_request(db.conn()).unwrap().is_none());
    assert!(state(db.conn(), "missing").is_err());
}

#[test]
fn journal_handoff_atomically_removes_intent_from_dispatch_index() {
    let db = test_db();
    let context = context();
    claim(db.conn(), "claim");
    enqueue(db.conn(), "claim", &context).unwrap();
    db.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
    checkpoint(db.conn(), &context, SubmissionStatus::OutcomeUnknown);
    assert!(next_request(db.conn()).unwrap().is_none());
    let blocked: bool = db
        .conn()
        .query_row(
            "SELECT blocked FROM completion_witness_requests",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(blocked);
    db.conn().execute_batch("ROLLBACK").unwrap();
    assert!(next_request(db.conn()).unwrap().is_some());
    checkpoint(db.conn(), &context, SubmissionStatus::OutcomeUnknown);
    assert!(next_request(db.conn()).unwrap().is_none());
    assert_eq!(
        state(db.conn(), "claim").unwrap().status,
        WitnessStatus::OutcomeUnknown
    );
}

#[tokio::test]
async fn cancelled_unsigned_build_preserves_intent_and_restart_backoff() {
    use tokio::io::AsyncReadExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 1024];
        assert!(socket.read(&mut bytes).await.unwrap() > 0);
        started_tx.send(()).unwrap();
        // Keep this provider request unresolved until the test aborts it.
        std::future::pending::<()>().await;
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cancelled-completion.db");
    let db = Database::open(&path).unwrap();
    db.run_migrations().unwrap();
    let context = context();
    claim(db.conn(), "claim");
    enqueue(db.conn(), "claim", &context).unwrap();
    let shared = Arc::new(Mutex::new(Some(db)));
    let job_db = shared.clone();
    let job = tokio::spawn(async move {
        let bf =
            BlockfrostClient::with_base_url("test".into(), format!("http://{address}")).unwrap();
        tick(&job_db, &bf, &wallet()).await
    });
    let started = tokio::time::timeout(std::time::Duration::from_secs(5), started_rx).await;
    job.abort();
    server.abort();
    assert!(job.await.unwrap_err().is_cancelled());
    assert!(server.await.unwrap_err().is_cancelled());
    started.expect("worker did not reach the provider").unwrap();
    drop(shared);
    let db = Database::open(&path).unwrap();
    db.run_migrations().unwrap();
    assert_eq!(
        state(db.conn(), "claim").unwrap().status,
        WitnessStatus::Pending
    );
    assert!(
        next_request(db.conn()).unwrap().is_none(),
        "restart must respect backoff"
    );
    let saved: String = db
        .conn()
        .query_row(
            "SELECT context_json FROM completion_witness_requests",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<CompletionContext>(&saved).unwrap(),
        context
    );
    assert!(submission::lookup(
        db.conn(),
        Operation {
            kind: KIND,
            id: &context.operation_id()
        }
    )
    .unwrap()
    .is_none());
    db.conn()
        .execute(
            "UPDATE completion_witness_requests SET next_attempt_at = 0",
            [],
        )
        .unwrap();
    assert!(
        next_request(db.conn()).unwrap().is_some(),
        "unsigned intent may resume"
    );
}
