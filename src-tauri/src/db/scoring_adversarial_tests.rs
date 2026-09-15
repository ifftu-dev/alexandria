//! Adversarial scoring fixtures.
//!
//! Genuinely signed credentials are tampered with, re-indexed, revoked or
//! removed, and storage writes are made to fail. Every cached projection must
//! agree with a direct recomputation from verified inputs and must never
//! present a stale or inflated score.

use ed25519_dalek::SigningKey;
use rusqlite::params;

use super::{schema::MIGRATIONS, Database};
use crate::commands::aggregation::{get_derived_skill_state_impl, list_derived_states_impl};
use crate::crypto::did::{course_authority_did, derive_did_key, Did};
use crate::db::opinion_eligibility::test_support::store_scored_credential;
use crate::evidence::reputation::{on_credential_accepted, recompute_for_subject, revalidate_rows};

const NOW: &str = "2026-09-15T00:00:00Z";
const LEGACY_AUTHOR: &str = "addr_public_author_for_retirement_test";

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// Reproduces a historical course-authority key from its public author
/// address. Test-only; anyone can compute it.
fn legacy_key() -> SigningKey {
    let mut hash = blake3::Hasher::new();
    hash.update(b"alexandria:course-authority:v1\x00");
    hash.update(LEGACY_AUTHOR.as_bytes());
    SigningKey::from_bytes(hash.finalize().as_bytes())
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

fn original_credentials(db: &Database) -> Vec<(String, String, bool)> {
    db.conn()
        .prepare("SELECT id, signed_vc_json, revoked FROM credentials ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
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

#[test]
fn migration_retires_issuer_recognition_without_rewriting_credentials() {
    assert_eq!(
        derive_did_key(&legacy_key()),
        course_authority_did(LEGACY_AUTHOR)
    );
    let db = Database::open_in_memory().unwrap();
    let before_retirement = MIGRATIONS
        .iter()
        .copied()
        .filter(|(version, _, _)| *version < 94)
        .collect::<Vec<_>>();
    db.run_migrations_from(&before_retirement).unwrap();
    taxonomy(&db);
    let learner = derive_did_key(&key(1));
    store_scored_credential(&db, "issued", &key(2), &learner, "skill", 2, 0.8, None);
    store_scored_credential(
        &db,
        "legacy",
        &legacy_key(),
        &learner,
        "skill",
        2,
        0.9,
        None,
    );
    db.conn()
        .execute(
            "INSERT INTO reputation_assertions \
             (id, actor_address, role, skill_id, proficiency_level, score, evidence_count) \
             VALUES ('stale', ?1, 'learner', 'skill', 'apply', 0.9, 2)",
            [learner.as_str()],
        )
        .unwrap();
    db.conn()
        .execute(
            "INSERT INTO derived_skill_state_history \
             (subject_did, skill_id, snapshot_date, raw_score, confidence, trust_score, \
              level, evidence_mass, computed_at) \
             VALUES (?1, 'skill', '2026-09-14', 0.9, 0.5, 0.45, 2, 1.0, ?2)",
            params![learner.as_str(), NOW],
        )
        .unwrap();
    // Recognizing the author's reproducible key invalidated dependent state.
    db.conn()
        .execute(
            "INSERT INTO courses (id, title, author_address) VALUES ('course', 'Course', ?1)",
            [LEGACY_AUTHOR],
        )
        .unwrap();
    let invalidated: (i64, String, i64) = db
        .conn()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM derived_skill_refresh_queue), \
                    (SELECT input_policy_state FROM reputation_assertions WHERE id = 'stale'), \
                    (SELECT COUNT(*) FROM derived_skill_state_history \
                     WHERE input_policy_valid = 0)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(invalidated, (1, "needs_refresh".to_string(), 1));
    let original = original_credentials(&db);

    db.run_migrations().unwrap();

    assert_eq!(original_credentials(&db), original);
    let retired: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name IN ( \
                 'public_derived_issuers', 'derived_skill_refresh_queue', \
                 'scoring_credentials', 'course_authority_recognized_insert', \
                 'course_authority_recognized_update', 'public_derived_issuer_recognized', \
                 'idx_credentials_scoring_issuer', 'idx_reputation_needs_refresh')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retired, 0);
    let history: (i64, i64) = db
        .conn()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM derived_skill_state_history), \
                    (SELECT COUNT(*) FROM pragma_table_info('derived_skill_state_history') \
                     WHERE name = 'input_policy_valid')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(history, (0, 0), "invalidated history points are discarded");
    let stale: (String, Option<String>) = db
        .conn()
        .query_row(
            "SELECT input_policy_state, input_fingerprint FROM reputation_assertions \
             WHERE id = 'stale'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stale, ("excluded".to_string(), None));

    // The reproducible key's credential now scores like any other unaccepted
    // issuer's, and the stale row is recomputed in place on its next read.
    assert_eq!(presented(&db, &learner), learner_row(0.9, 2));
    db.conn()
        .execute(
            "INSERT INTO courses (id, title, author_address) \
             VALUES ('another', 'Another', 'another_public_author')",
            [],
        )
        .unwrap();
    assert!(!db
        .conn()
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
}
