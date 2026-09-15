use super::*;

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
         tx_hash, signed_cbor, context_json, status, confirmed_slot)
         VALUES ('cardano-preprod', ?1, ?2, ?3, X'00', ?4, ?5, ?6)",
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
