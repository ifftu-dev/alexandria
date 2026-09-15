use super::*;

fn test_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().unwrap();
    db.conn().execute_batch(
        "INSERT INTO credentials (id, issuer_did, subject_did, credential_type, claim_kind,
         issuance_date, signed_vc_json, integrity_hash)
         VALUES ('credential', 'did:key:i', 'did:key:s', 'FormalCredential', 'skill', '2026-01-01', '{}', 'hash');
         INSERT INTO credential_challenges (id, challenger, credential_id, reason, stake_lovelace, dao_id, signature)
         VALUES ('challenge', 'challenger', 'credential', 'test', 5000000, 'dao', 'signature');"
    ).unwrap();
    db
}

fn lock_context() -> EscrowContext {
    EscrowContext {
        version: 1,
        challenge_id: "challenge".into(),
        stake_lovelace: 5_000_000,
        action: EscrowAction::Lock {
            challenger_pkh: "a".repeat(56),
            treasury_pkh: "b".repeat(56),
            authority_pkh: "c".repeat(56),
        },
    }
}

fn checkpoint(conn: &Connection, kind: &str, context: &EscrowContext, status: &str) -> Submission {
    conn.execute(
        "INSERT INTO chain_submissions
         (network, operation_kind, operation_id, tx_hash, signed_cbor, context_json, status, confirmed_slot)
         VALUES ('cardano-preprod', ?1, 'challenge', ?2, X'00', ?3, ?4, ?5)",
        rusqlite::params![kind, if kind == LOCK_KIND { "a".repeat(64) } else { "b".repeat(64) },
            serde_json::to_string(context).unwrap(), status, matches!(status, "confirmed" | "failed_on_chain").then_some(42)],
    ).unwrap();
    submission::lookup(
        conn,
        Operation {
            kind,
            id: "challenge",
        },
    )
    .unwrap()
    .unwrap()
}

#[test]
fn pending_and_acknowledged_lock_are_not_reported_as_escrowed() {
    for status in ["outcome_unknown", "submitted", "failed_on_chain"] {
        let db = test_db();
        let operation = Operation {
            kind: LOCK_KIND,
            id: "challenge",
        };
        let saved = checkpoint(db.conn(), LOCK_KIND, &lock_context(), status);
        project(db.conn(), operation, &saved).unwrap();
        let stake = challenge::get_stake_info(db.conn(), "challenge").unwrap();
        assert_eq!(stake.stake_status, "none");
        assert_eq!(stake.lock_tx_hash, None);
        let result = existing_response(db.conn(), operation);
        if status == "submitted" {
            assert_eq!(result.unwrap(), Some(saved.tx_hash));
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn failed_application_rolls_back_escrow_and_preserves_recovery() {
    let db = test_db();
    let operation = Operation {
        kind: LOCK_KIND,
        id: "challenge",
    };
    let saved = checkpoint(db.conn(), LOCK_KIND, &lock_context(), "confirmed");
    db.conn()
        .execute_batch(
            "CREATE TRIGGER fail_application BEFORE UPDATE OF applied_at ON chain_submissions
         BEGIN SELECT RAISE(ABORT, 'injected application failure'); END;",
        )
        .unwrap();
    assert!(project(db.conn(), operation, &saved)
        .unwrap_err()
        .contains("injected application failure"));
    assert_eq!(
        challenge::get_stake_info(db.conn(), "challenge")
            .unwrap()
            .stake_status,
        "none"
    );
    assert_eq!(
        submission::unapplied_operations(db.conn(), LOCK_KIND, 10)
            .unwrap()
            .len(),
        1
    );
    db.conn()
        .execute_batch("DROP TRIGGER fail_application")
        .unwrap();
    project(db.conn(), operation, &saved).unwrap();
    let stake = challenge::get_stake_info(db.conn(), "challenge").unwrap();
    assert_eq!(stake.stake_status, "locked");
    assert_eq!(stake.escrow_challenger_pkh, Some("a".repeat(56)));
    assert_eq!(stake.escrow_treasury_pkh, Some("b".repeat(56)));
    assert!(submission::unapplied_operations(db.conn(), LOCK_KIND, 10)
        .unwrap()
        .is_empty());
}

#[test]
fn recovery_preserves_signed_settlement_and_late_lock_does_not_relock() {
    let db = test_db();
    let lock_operation = Operation {
        kind: LOCK_KIND,
        id: "challenge",
    };
    let lock = checkpoint(db.conn(), LOCK_KIND, &lock_context(), "confirmed");
    project(db.conn(), lock_operation, &lock).unwrap();
    let context = EscrowContext {
        version: 1,
        challenge_id: "challenge".into(),
        stake_lovelace: 5_000_000,
        action: EscrowAction::Settle {
            escrow_tx_hash: lock.tx_hash.clone(),
            escrow_index: 0,
            recipient_pkh: "a".repeat(56),
            refund: true,
        },
    };
    let settle = checkpoint(db.conn(), SETTLE_KIND, &context, "confirmed");
    // A later local resolution must not rewrite the signed refund.
    db.conn()
        .execute("UPDATE credential_challenges SET status = 'rejected'", [])
        .unwrap();
    let settle_operation = Operation {
        kind: SETTLE_KIND,
        id: "challenge",
    };
    project(db.conn(), settle_operation, &settle).unwrap();
    project(db.conn(), lock_operation, &lock).unwrap();
    project(db.conn(), settle_operation, &settle).unwrap();
    assert_eq!(
        challenge::get_stake_info(db.conn(), "challenge")
            .unwrap()
            .stake_status,
        "returned"
    );
}

#[test]
fn mismatched_amount_blocks_projection_without_discarding_the_signed_record() {
    let db = test_db();
    let mut context = lock_context();
    context.stake_lovelace = 6_000_000;
    let saved = checkpoint(db.conn(), LOCK_KIND, &context, "confirmed");
    assert!(project(
        db.conn(),
        Operation {
            kind: LOCK_KIND,
            id: "challenge"
        },
        &saved
    )
    .unwrap_err()
    .contains("amount"));
    assert_eq!(
        challenge::get_stake_info(db.conn(), "challenge")
            .unwrap()
            .stake_status,
        "none"
    );
    assert_eq!(
        submission::unapplied_operations(db.conn(), LOCK_KIND, 10)
            .unwrap()
            .len(),
        1
    );
}
