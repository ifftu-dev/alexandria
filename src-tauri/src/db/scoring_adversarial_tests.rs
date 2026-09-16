//! Adversarial scoring fixtures.
//!
//! Genuinely signed credentials are tampered with, re-indexed, revoked or
//! removed, and storage writes are made to fail. Every cached projection must
//! agree with a direct recomputation from verified inputs and must never
//! present a stale or inflated score.

use ed25519_dalek::SigningKey;

use super::Database;
use crate::commands::aggregation::{get_derived_skill_state_impl, list_derived_states_impl};
use crate::crypto::did::{derive_did_key, Did};
use crate::db::opinion_eligibility::test_support::store_scored_credential;
use crate::evidence::reputation::{on_credential_accepted, recompute_for_subject, revalidate_rows};

const NOW: &str = "2026-09-15T00:00:00Z";

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn taxonomy(db: &Database) {
    db.conn()
        .execute_batch(
            "INSERT INTO subject_fields (id, name) VALUES ('sf', 'Field');
             INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub', 'Subject', 'sf');
             INSERT INTO skills (id, name, subject_id) VALUES ('skill', 'Skill', 'sub');",
        )
        .unwrap();
}

fn test_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().unwrap();
    taxonomy(&db);
    db
}

/// Store a signed skill credential and accept it into reputation.
fn issue(db: &Database, id: &str, issuer: &SigningKey, subject: &Did, score: f64) {
    store_scored_credential(db, id, issuer, subject, "skill", 2, score, None);
    on_credential_accepted(db.conn(), id).unwrap();
}

/// The reputation rows presented for `actor` after revalidation, as
/// `(role, score, evidence_count)`.
fn presented(db: &Database, actor: &Did) -> Vec<(String, f64, i64)> {
    revalidate_rows(db.conn(), Some(actor.as_str())).unwrap();
    db.conn()
        .prepare(
            "SELECT role, score, evidence_count FROM current_reputation_assertions \
             WHERE actor_address = ?1 ORDER BY role",
        )
        .unwrap()
        .query_map([actor.as_str()], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn learner_row(score: f64, evidence_count: i64) -> Vec<(String, f64, i64)> {
    vec![("learner".to_string(), score, evidence_count)]
}

#[test]
fn a_tampered_signed_body_cannot_inflate_any_projection() {
    let db = test_db();
    let learner = derive_did_key(&key(1));
    let instructor = key(2);
    issue(&db, "issued", &instructor, &learner, 0.3);
    let state = get_derived_skill_state_impl(db.conn(), &learner, "skill", NOW)
        .unwrap()
        .unwrap();
    assert!(state.raw_score < 0.5);
    assert_eq!(presented(&db, &learner), learner_row(0.3, 1));

    db.conn()
        .execute(
            "UPDATE credentials SET signed_vc_json = \
             json_set(signed_vc_json, '$.credentialSubject.score', 1.0)",
            [],
        )
        .unwrap();

    assert!(
        get_derived_skill_state_impl(db.conn(), &learner, "skill", NOW)
            .unwrap()
            .is_none()
    );
    assert!(presented(&db, &learner).is_empty());
    assert!(presented(&db, &derive_did_key(&instructor)).is_empty());
}

#[test]
fn unsigned_index_metadata_cannot_redirect_credit() {
    let db = test_db();
    let learner = derive_did_key(&key(1));
    let instructor = derive_did_key(&key(2));
    let impostor = derive_did_key(&key(3));
    issue(&db, "issued", &key(2), &learner, 0.8);

    db.conn()
        .execute(
            "UPDATE credentials SET issuer_did = ?1, subject_did = ?1",
            [impostor.as_str()],
        )
        .unwrap();
    recompute_for_subject(db.conn(), impostor.as_str()).unwrap();

    assert!(presented(&db, &impostor).is_empty());
    assert!(
        get_derived_skill_state_impl(db.conn(), &impostor, "skill", NOW)
            .unwrap()
            .is_none()
    );
    // Projections score only rows whose index and signature agree, so the
    // re-indexed row no longer backs the genuine parties' rows either.
    assert!(presented(&db, &learner).is_empty());
    assert!(presented(&db, &instructor).is_empty());
}

#[test]
fn revoking_the_last_input_clears_every_reader_until_reinstated() {
    let db = test_db();
    let learner = derive_did_key(&key(1));
    let instructor = derive_did_key(&key(2));
    issue(&db, "issued", &key(2), &learner, 0.8);
    assert!(
        get_derived_skill_state_impl(db.conn(), &learner, "skill", NOW)
            .unwrap()
            .is_some()
    );
    assert_eq!(presented(&db, &learner), learner_row(0.8, 1));
    assert_eq!(presented(&db, &instructor).len(), 1);

    db.conn()
        .execute("UPDATE credentials SET revoked = 1 WHERE id = 'issued'", [])
        .unwrap();

    assert!(list_derived_states_impl(db.conn(), Some(learner.as_str()))
        .unwrap()
        .is_empty());
    assert!(
        get_derived_skill_state_impl(db.conn(), &learner, "skill", NOW)
            .unwrap()
            .is_none()
    );
    assert!(presented(&db, &learner).is_empty());
    assert!(presented(&db, &instructor).is_empty());
    let stored: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM reputation_assertions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored, 2, "rows are kept as excluded, not deleted");

    db.conn()
        .execute("UPDATE credentials SET revoked = 0 WHERE id = 'issued'", [])
        .unwrap();
    assert_eq!(presented(&db, &learner), learner_row(0.8, 1));
}

#[test]
fn failed_recomputation_never_presents_a_stale_score() {
    let db = test_db();
    let learner = derive_did_key(&key(1));
    issue(&db, "first", &key(2), &learner, 0.9);
    issue(&db, "second", &key(3), &learner, 0.4);
    assert_eq!(presented(&db, &learner), learner_row(0.9, 2));
    get_derived_skill_state_impl(db.conn(), &learner, "skill", NOW).unwrap();

    db.conn()
        .execute("UPDATE credentials SET revoked = 1 WHERE id = 'first'", [])
        .unwrap();
    db.conn()
        .execute_batch(
            "CREATE TEMP TRIGGER fail_reputation_insert BEFORE INSERT ON reputation_assertions
             BEGIN SELECT RAISE(ABORT, 'injected reputation write failure'); END;
             CREATE TEMP TRIGGER fail_reputation_update BEFORE UPDATE ON reputation_assertions
             BEGIN SELECT RAISE(ABORT, 'injected reputation write failure'); END;
             CREATE TEMP TRIGGER fail_history BEFORE INSERT ON derived_skill_state_history
             BEGIN SELECT RAISE(ABORT, 'injected history write failure'); END;",
        )
        .unwrap();

    assert!(revalidate_rows(db.conn(), Some(learner.as_str())).is_err());
    assert!(get_derived_skill_state_impl(db.conn(), &learner, "skill", NOW).is_err());

    db.conn()
        .execute_batch(
            "DROP TRIGGER fail_reputation_insert;
             DROP TRIGGER fail_reputation_update;
             DROP TRIGGER fail_history;",
        )
        .unwrap();
    assert_eq!(presented(&db, &learner), learner_row(0.4, 1));
    assert_eq!(
        get_derived_skill_state_impl(db.conn(), &learner, "skill", NOW)
            .unwrap()
            .unwrap()
            .active_evidence_count,
        1
    );
}

#[test]
fn revalidation_does_not_commit_its_callers_transaction() {
    let db = test_db();
    let learner = derive_did_key(&key(1));
    issue(&db, "issued", &key(2), &learner, 0.8);
    db.conn()
        .execute("UPDATE credentials SET revoked = 1", [])
        .unwrap();
    {
        let _caller_transaction = db.conn().unchecked_transaction().unwrap();
        revalidate_rows(db.conn(), None).unwrap();
        assert!(
            get_derived_skill_state_impl(db.conn(), &learner, "skill", NOW)
                .unwrap()
                .is_none()
        );
        assert!(!db.conn().is_autocommit());
    }
    let valid: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM current_reputation_assertions",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(valid, 2, "the caller's rollback restores the excluded rows");
}

#[test]
fn fingerprints_survive_reopen_and_unchanged_rows_are_not_rewritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scoring.db");
    let learner = derive_did_key(&key(1));
    {
        let db = Database::open(&path).unwrap();
        db.run_migrations().unwrap();
        taxonomy(&db);
        issue(&db, "issued", &key(2), &learner, 0.8);
        db.conn()
            .execute(
                "UPDATE reputation_assertions SET updated_at = 'untouched'",
                [],
            )
            .unwrap();
    }

    let reopened = Database::open(&path).unwrap();
    reopened.run_migrations().unwrap();
    revalidate_rows(reopened.conn(), None).unwrap();
    let untouched: i64 = reopened
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM current_reputation_assertions WHERE updated_at = 'untouched'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(untouched, 2, "unchanged inputs keep both rows as computed");

    reopened
        .conn()
        .execute("DELETE FROM credentials WHERE id = 'issued'", [])
        .unwrap();
    revalidate_rows(reopened.conn(), None).unwrap();
    let counts: (i64, i64) = reopened
        .conn()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM current_reputation_assertions), \
                    (SELECT COUNT(*) FROM reputation_assertions)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        counts,
        (0, 2),
        "removing the last input excludes rows without deleting them"
    );
}
