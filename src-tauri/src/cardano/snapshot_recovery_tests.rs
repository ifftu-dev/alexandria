use super::*;

fn test_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().unwrap();
    db
}

fn wallet() -> Wallet {
    crate::crypto::wallet::wallet_from_mnemonic(
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
    ).unwrap()
}

fn context() -> SnapshotContext {
    let wallet = wallet();
    SnapshotContext {
        version: 1,
        snapshot_id: "snap".into(),
        actor_address: wallet.stake_address.clone(),
        actor_did: crate::crypto::did::did_from_verifying_key(&wallet.signing_key.verifying_key())
            .as_str()
            .to_owned(),
        owner_key_hash: wallet.payment_key_hash,
        subject_id: "subject".into(),
        role: ReputationRole::Learner,
        skills: vec![OnChainSkillScore {
            skill_id_bytes: hex::encode("skill"),
            proficiency: 2,
            impact_score: 800_000,
            confidence: 8000,
            evidence_count: 2,
        }],
        window_start_ms: 0,
        window_end_ms: 1_714_000_000_000,
        snapshot_at: "2024-04-24T00:00:00+00:00".into(),
    }
}

fn checkpoint(
    conn: &Connection,
    context: &SnapshotContext,
    status: SubmissionStatus,
) -> Submission {
    let slot = matches!(
        status,
        SubmissionStatus::Confirmed | SubmissionStatus::FailedOnChain
    )
    .then_some(42);
    let saved = Submission {
        tx_hash: "a".repeat(64),
        status,
        context_json: serde_json::to_string(context).unwrap(),
        last_error: None,
        confirmed_slot: slot,
    };
    conn.execute("INSERT INTO chain_submissions (network, operation_kind, operation_id, tx_hash, signed_cbor, context_json, status, confirmed_slot)
        VALUES ('cardano-preprod', ?1, ?2, ?3, X'00', ?4, ?5, ?6)", params![KIND, context.snapshot_id,
            saved.tx_hash, saved.context_json, serde_json::to_value(status).unwrap().as_str().unwrap(), slot.map(|s| s as i64)]).unwrap();
    saved
}

#[test]
fn frozen_snapshot_and_uncertain_checkpoint_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("snapshots.db");
    let db = Database::open(&path).unwrap();
    db.run_migrations().unwrap();
    let context = context();
    freeze(db.conn(), &context).unwrap();
    let saved = checkpoint(db.conn(), &context, SubmissionStatus::OutcomeUnknown);
    assert_eq!(
        record(db.conn(), "snap").unwrap().tx_status,
        "outcome_unknown"
    );
    drop(db);
    let db = Database::open(&path).unwrap();
    db.run_migrations().unwrap();
    let loaded = load(db.conn(), "snap").unwrap();
    assert_eq!(serde_json::to_string(&loaded).unwrap(), saved.context_json);
    assert_eq!(
        record(db.conn(), "snap").unwrap().tx_hash,
        Some(saved.tx_hash)
    );
    assert!(
        freeze(db.conn(), &context).is_err(),
        "frozen inputs must not be replaced"
    );
}

#[test]
fn frozen_input_failure_rolls_back_snapshot_record() {
    let db = test_db();
    db.conn()
        .execute_batch(
            "CREATE TRIGGER fail_inputs BEFORE INSERT ON reputation_snapshot_inputs
        BEGIN SELECT RAISE(ABORT, 'injected input failure'); END;",
        )
        .unwrap();
    assert!(freeze(db.conn(), &context())
        .unwrap_err()
        .contains("injected input failure"));
    assert!(record(db.conn(), "snap").is_err());
    assert!(db.conn().is_autocommit());
    db.conn().execute_batch("DROP TRIGGER fail_inputs").unwrap();
    freeze(db.conn(), &context()).unwrap();
}

#[test]
fn each_submission_state_is_projected_without_fabricating_confirmation() {
    for (status, expected) in [
        (SubmissionStatus::OutcomeUnknown, "outcome_unknown"),
        (SubmissionStatus::Submitted, "submitted"),
        (SubmissionStatus::Confirmed, "confirmed"),
        (SubmissionStatus::FailedOnChain, "failed_on_chain"),
    ] {
        let db = test_db();
        let context = context();
        freeze(db.conn(), &context).unwrap();
        let saved = checkpoint(db.conn(), &context, status);
        let operation = Operation {
            kind: KIND,
            id: "snap",
        };
        project(db.conn(), operation, &saved).unwrap();
        project(db.conn(), operation, &saved).unwrap();
        let record = record(db.conn(), "snap").unwrap();
        assert_eq!(record.tx_status, expected);
        assert_eq!(
            record.confirmed_at.is_some(),
            status == SubmissionStatus::Confirmed
        );
        assert_eq!(
            submission::unapplied_operations(db.conn(), KIND, 10)
                .unwrap()
                .is_empty(),
            saved.confirmed_slot.is_some()
        );
    }
}

#[test]
fn stale_acknowledgement_cannot_downgrade_a_confirmed_snapshot() {
    let db = test_db();
    let context = context();
    freeze(db.conn(), &context).unwrap();
    let stale = checkpoint(db.conn(), &context, SubmissionStatus::Submitted);
    db.conn()
        .execute(
            "UPDATE chain_submissions SET status = 'confirmed', confirmed_slot = 42",
            [],
        )
        .unwrap();
    project(
        db.conn(),
        Operation {
            kind: KIND,
            id: "snap",
        },
        &stale,
    )
    .unwrap();
    assert_eq!(record(db.conn(), "snap").unwrap().tx_status, "confirmed");
}

#[test]
fn failed_projection_rolls_back_domain_state_and_remains_retryable() {
    let db = test_db();
    let context = context();
    freeze(db.conn(), &context).unwrap();
    let saved = checkpoint(db.conn(), &context, SubmissionStatus::Confirmed);
    db.conn()
        .execute_batch(
            "CREATE TRIGGER fail_applied BEFORE UPDATE OF applied_at ON chain_submissions
        BEGIN SELECT RAISE(ABORT, 'injected projection failure'); END;",
        )
        .unwrap();
    let operation = Operation {
        kind: KIND,
        id: "snap",
    };
    assert!(project(db.conn(), operation, &saved)
        .unwrap_err()
        .contains("injected projection failure"));
    assert_eq!(
        record(db.conn(), "snap").unwrap().tx_status,
        "outcome_unknown"
    );
    assert_eq!(
        submission::unapplied_operations(db.conn(), KIND, 10).unwrap(),
        vec!["snap"]
    );
    db.conn()
        .execute_batch("DROP TRIGGER fail_applied")
        .unwrap();
    project(db.conn(), operation, &saved).unwrap();
    assert_eq!(record(db.conn(), "snap").unwrap().tx_status, "confirmed");
}

#[test]
fn wallet_record_and_frozen_evidence_must_match() {
    let db = test_db();
    let mut context = context();
    context.validate_wallet(&wallet()).unwrap();
    context.owner_key_hash = [0; 28];
    assert!(context.validate_wallet(&wallet()).is_err());
    freeze(db.conn(), &context).unwrap();
    let saved = checkpoint(db.conn(), &context, SubmissionStatus::Submitted);
    db.conn()
        .execute("UPDATE reputation_snapshots SET subject_id = 'other'", [])
        .unwrap();
    assert!(project(
        db.conn(),
        Operation {
            kind: KIND,
            id: "snap"
        },
        &saved
    )
    .is_err());
    db.conn()
        .execute("UPDATE reputation_snapshots SET subject_id = 'subject'", [])
        .unwrap();
    context.skills[0].impact_score = 900_000;
    db.conn()
        .execute(
            "UPDATE reputation_snapshot_inputs SET context_json = ?1",
            [serde_json::to_string(&context).unwrap()],
        )
        .unwrap();
    assert!(project(
        db.conn(),
        Operation {
            kind: KIND,
            id: "snap"
        },
        &saved
    )
    .is_err());
}

#[test]
fn legacy_records_are_preserved_and_cannot_be_reconstructed() {
    let db = test_db();
    db.conn().execute("INSERT INTO reputation_snapshots (id, actor_address, subject_id, role, tx_status, tx_hash)
        VALUES ('legacy', 'owner', 'subject', 'learner', 'failed', 'original')", []).unwrap();
    assert!(load(db.conn(), "legacy")
        .unwrap_err()
        .contains("legacy snapshot"));
    assert_eq!(
        record(db.conn(), "legacy").unwrap().tx_hash.as_deref(),
        Some("original")
    );
    assert_eq!(record(db.conn(), "legacy").unwrap().tx_status, "failed");
}

#[test]
fn checkpoint_rejects_changed_inputs_or_a_preexisting_transaction() {
    let db = test_db();
    let context = context();
    freeze(db.conn(), &context).unwrap();
    let insert = |json: &str| {
        db.conn().execute(
        "INSERT INTO chain_submissions (network, operation_kind, operation_id, tx_hash, signed_cbor, context_json)
         VALUES ('cardano-preprod', ?1, 'snap', ?2, X'00', ?3)", params![KIND, "a".repeat(64), json],
    )
    };
    let mut changed = context.clone();
    changed.skills[0].impact_score = 700_000;
    assert!(insert(&serde_json::to_string(&changed).unwrap()).is_err());
    assert!(submission::lookup(
        db.conn(),
        Operation {
            kind: KIND,
            id: "snap"
        }
    )
    .unwrap()
    .is_none());
    assert_eq!(record(db.conn(), "snap").unwrap().tx_status, "pending");
    db.conn()
        .execute(
            "UPDATE reputation_snapshots SET tx_hash = 'preserved-original'",
            [],
        )
        .unwrap();
    assert!(insert(&serde_json::to_string(&context).unwrap()).is_err());
    assert_eq!(
        record(db.conn(), "snap").unwrap().tx_hash.as_deref(),
        Some("preserved-original")
    );
    assert!(submission::lookup(
        db.conn(),
        Operation {
            kind: KIND,
            id: "snap"
        }
    )
    .unwrap()
    .is_none());
}

#[test]
fn terminal_status_without_receipt_cannot_confirm_snapshot() {
    let db = test_db();
    let context = context();
    freeze(db.conn(), &context).unwrap();
    let mut saved = checkpoint(db.conn(), &context, SubmissionStatus::Confirmed);
    db.conn()
        .execute("UPDATE chain_submissions SET confirmed_slot = NULL", [])
        .unwrap();
    saved.confirmed_slot = None;
    assert!(project(
        db.conn(),
        Operation {
            kind: KIND,
            id: "snap"
        },
        &saved
    )
    .is_err());
    assert_eq!(
        record(db.conn(), "snap").unwrap().tx_status,
        "outcome_unknown"
    );
    assert!(record(db.conn(), "snap").unwrap().confirmed_at.is_none());
}
