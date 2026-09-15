use super::{schema::MIGRATIONS, Database};
use crate::commands::aggregation::{get_derived_skill_state_impl, list_derived_states_impl};
use crate::commands::credentials::{issue_credential_impl, IssueCredentialRequest};
use crate::crypto::did::{course_authority_did, derive_did_key, Did};
use crate::domain::vc::{Claim, CredentialType, SkillClaim};
use crate::evidence::reputation::refresh_invalidated;
use ed25519_dalek::SigningKey;
use rusqlite::params;

const AUTHOR: &str = "addr_public_author_for_exclusion_test";
const BEFORE: &str = "2026-09-13T00:00:00Z";
const AFTER: &str = "2026-09-14T00:00:00Z";

fn legacy_key() -> SigningKey {
    // Reproduce the vulnerability from public input; no privileged key access.
    let mut hash = blake3::Hasher::new();
    hash.update(b"alexandria:course-authority:v1\x00");
    hash.update(AUTHOR.as_bytes());
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

fn recognize(db: &Database) {
    db.conn()
        .execute(
            "INSERT INTO courses (id, title, author_address) VALUES ('course', 'Course', ?1)",
            [AUTHOR],
        )
        .unwrap();
}

fn issue(db: &Database, key: &SigningKey, subject: &Did, score: f64) {
    let issuer = derive_did_key(key);
    issue_credential_impl(
        db.conn(),
        key,
        &issuer,
        &IssueCredentialRequest {
            credential_type: if &issuer == subject {
                CredentialType::SelfAssertion
            } else {
                CredentialType::AttestationCredential
            },
            subject: subject.clone(),
            claim: Claim::Skill(SkillClaim {
                skill_id: "skill".into(),
                level: 2,
                score,
                evidence_refs: vec![],
                rubric_version: None,
                // Deliberately identical for genuine and public-derived issuers.
                assessment_method: Some("instructor_attestation".into()),
                provenance: None,
            }),
            evidence_refs: vec![],
            expiration_date: None,
            supersedes: None,
            integrity_session_id: None,
            integrity_policy: None,
        },
        BEFORE,
    )
    .unwrap();
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
fn exact_late_match_removes_only_public_derived_evidence_and_repairs_caches() {
    let db = test_db();
    let learner = SigningKey::from_bytes(&[13; 32]);
    let subject = derive_did_key(&learner);
    issue(&db, &learner, &subject, 0.4);
    issue(&db, &legacy_key(), &subject, 0.9);
    issue(&db, &SigningKey::from_bytes(&[14; 32]), &subject, 0.6);
    let original = original_credentials(&db);
    let before = get_derived_skill_state_impl(db.conn(), &subject, "skill", BEFORE)
        .unwrap()
        .unwrap();
    assert_eq!(before.unique_issuer_clusters, 3);
    recognize(&db);
    let hidden: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM current_reputation_assertions WHERE actor_address = ?1",
            [subject.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hidden, 0, "stale reputation is hidden even before repair");
    let after = get_derived_skill_state_impl(db.conn(), &subject, "skill", AFTER)
        .unwrap()
        .unwrap();
    assert_eq!(after.unique_issuer_clusters, 2);
    assert_eq!(after.active_evidence_count, 2);
    assert!(after.confidence < before.confidence);
    refresh_invalidated(db.conn()).unwrap();
    let (score, count): (f64, i64) = db
        .conn()
        .query_row(
            "SELECT score, evidence_count FROM current_reputation_assertions
         WHERE actor_address = ?1 AND role = 'learner'",
            [subject.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((score, count), (0.6, 2));
    let excluded: String = db
        .conn()
        .query_row(
            "SELECT input_policy_state FROM reputation_assertions WHERE actor_address = ?1",
            [course_authority_did(AUTHOR).as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        excluded, "excluded",
        "retain the original synthetic issuer's row"
    );
    let history: (f64, bool) = db
        .conn()
        .query_row(
            "SELECT confidence, input_policy_valid FROM derived_skill_state_history
         WHERE subject_did = ?1 AND snapshot_date = '2026-09-13'",
            [subject.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(history, (before.confidence, false));
    assert_eq!(original_credentials(&db), original);
}

#[test]
fn recognition_survives_course_edit_delete_and_legacy_credential_reimport() {
    let db = test_db();
    recognize(&db);
    db.conn()
        .execute(
            "UPDATE courses SET author_address = 'another_public_author'",
            [],
        )
        .unwrap();
    db.conn()
        .execute("DELETE FROM courses WHERE id = 'course'", [])
        .unwrap();
    let subject = derive_did_key(&SigningKey::from_bytes(&[15; 32]));
    issue(&db, &legacy_key(), &subject, 1.0);
    assert!(
        get_derived_skill_state_impl(db.conn(), &subject, "skill", AFTER)
            .unwrap()
            .is_none()
    );
    assert_eq!(original_credentials(&db).len(), 1);
    let count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM current_reputation_assertions",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    assert!(
        db.conn()
            .execute(
                "INSERT INTO public_derived_issuers (issuer_did, author_address) VALUES (?1, ?2)",
                params![subject.as_str(), "unrelated-public-input"],
            )
            .is_err(),
        "an unmatched key cannot be classified by declaration"
    );
}

#[test]
fn unmatched_attestation_and_cache_are_unchanged_despite_matching_label() {
    let db = test_db();
    let subject = derive_did_key(&SigningKey::from_bytes(&[16; 32]));
    issue(&db, &SigningKey::from_bytes(&[17; 32]), &subject, 0.8);
    let before = get_derived_skill_state_impl(db.conn(), &subject, "skill", BEFORE)
        .unwrap()
        .unwrap();
    let original = original_credentials(&db);
    recognize(&db);
    let listed = list_derived_states_impl(db.conn(), Some(subject.as_str())).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        serde_json::to_value(&listed[0]).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(original_credentials(&db), original);
}

#[test]
fn failed_cache_repair_keeps_refresh_work_and_never_restores_stale_score() {
    let db = test_db();
    let learner = SigningKey::from_bytes(&[18; 32]);
    let subject = derive_did_key(&learner);
    issue(&db, &learner, &subject, 0.4);
    issue(&db, &legacy_key(), &subject, 0.9);
    get_derived_skill_state_impl(db.conn(), &subject, "skill", BEFORE).unwrap();
    recognize(&db);
    db.conn()
        .execute_batch(
            "CREATE TEMP TRIGGER fail_history BEFORE INSERT ON derived_skill_state_history
        BEGIN SELECT RAISE(ABORT, 'injected history write failure'); END;",
        )
        .unwrap();
    assert!(get_derived_skill_state_impl(db.conn(), &subject, "skill", AFTER).is_err());
    let counts: (i64, i64) = db.conn().query_row(
        "SELECT (SELECT COUNT(*) FROM derived_skill_states), (SELECT COUNT(*) FROM derived_skill_refresh_queue)",
        [], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    assert_eq!(counts, (0, 1));
    db.conn()
        .execute_batch("DROP TRIGGER fail_history")
        .unwrap();
    assert_eq!(
        get_derived_skill_state_impl(db.conn(), &subject, "skill", AFTER)
            .unwrap()
            .unwrap()
            .unique_issuer_clusters,
        1
    );
}

#[test]
fn migration_backfills_exact_matches_without_rewriting_credentials() {
    let db = Database::open_in_memory().unwrap();
    for (_, _, sql) in MIGRATIONS.iter().filter(|(v, _, _)| *v < 85) {
        db.conn().execute_batch(sql).unwrap();
    }
    taxonomy(&db);
    recognize(&db);
    // Storage migration fixture; cryptographic issuance is covered above.
    let did = course_authority_did(AUTHOR);
    let payload = serde_json::json!({"issuer": did.as_str(), "credentialSubject": {"id":"learner", "skillId":"skill"}}).to_string();
    db.conn().execute(
        "INSERT INTO credentials (id, issuer_did, subject_did, credential_type, claim_kind,
         skill_id, issuance_date, signed_vc_json, integrity_hash)
         VALUES ('legacy', ?1, 'learner', 'AttestationCredential', 'skill', 'skill', ?2, ?3, 'original')",
        params![did.as_str(), BEFORE, payload],
    ).unwrap();
    let original = original_credentials(&db);
    let sql = MIGRATIONS.iter().find(|(v, _, _)| *v == 85).unwrap().2;
    let tx = db.conn().unchecked_transaction().unwrap();
    tx.execute_batch(sql).unwrap();
    tx.commit().unwrap();
    assert_eq!(original_credentials(&db), original);
    let counts: (i64, i64) = db.conn().query_row(
        "SELECT (SELECT COUNT(*) FROM scoring_credentials), (SELECT COUNT(*) FROM derived_skill_refresh_queue)",
        [], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    assert_eq!(counts, (0, 1));
    assert!(!db
        .conn()
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
}

#[test]
fn pending_refresh_and_recognition_survive_database_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("issuer-policy.db");
    let subject = derive_did_key(&SigningKey::from_bytes(&[19; 32]));
    let original = {
        let db = Database::open(&path).unwrap();
        db.run_migrations().unwrap();
        taxonomy(&db);
        issue(&db, &legacy_key(), &subject, 0.9);
        get_derived_skill_state_impl(db.conn(), &subject, "skill", BEFORE).unwrap();
        recognize(&db);
        db.conn()
            .execute("DELETE FROM courses WHERE id = 'course'", [])
            .unwrap();
        original_credentials(&db)
    };
    let reopened = Database::open(&path).unwrap();
    reopened.run_migrations().unwrap();
    assert!(
        list_derived_states_impl(reopened.conn(), Some(subject.as_str()))
            .unwrap()
            .is_empty()
    );
    refresh_invalidated(reopened.conn()).unwrap();
    let count: i64 = reopened
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM current_reputation_assertions",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(original_credentials(&reopened), original);
}

#[test]
fn scoring_classifier_uses_payload_issuer_not_denormalized_metadata() {
    let db = test_db();
    let subject = derive_did_key(&SigningKey::from_bytes(&[20; 32]));
    let genuine = derive_did_key(&SigningKey::from_bytes(&[21; 32]));
    issue(&db, &legacy_key(), &subject, 0.9);
    issue(&db, &SigningKey::from_bytes(&[21; 32]), &subject, 0.6);
    // Swap only unsigned query metadata. Neither edit changes who signed.
    db.conn()
        .execute(
            "UPDATE credentials SET issuer_did = CASE
        WHEN issuer_did = ?1 THEN ?2 ELSE ?1 END",
            params![genuine.as_str(), course_authority_did(AUTHOR).as_str()],
        )
        .unwrap();
    recognize(&db);
    let state = get_derived_skill_state_impl(db.conn(), &subject, "skill", AFTER)
        .unwrap()
        .unwrap();
    assert_eq!(state.active_evidence_count, 1);
    assert_eq!(state.raw_score, 0.6);
    assert_eq!(original_credentials(&db).len(), 2);
}

#[test]
fn derived_refresh_does_not_commit_its_callers_transaction() {
    let db = test_db();
    let learner = SigningKey::from_bytes(&[22; 32]);
    let subject = derive_did_key(&learner);
    issue(&db, &learner, &subject, 0.4);
    issue(&db, &legacy_key(), &subject, 0.9);
    recognize(&db);
    {
        let _caller_transaction = db.conn().unchecked_transaction().unwrap();
        get_derived_skill_state_impl(db.conn(), &subject, "skill", AFTER).unwrap();
        refresh_invalidated(db.conn()).unwrap();
        assert!(!db.conn().is_autocommit());
    }
    let counts: (i64, i64) = db.conn().query_row(
        "SELECT (SELECT COUNT(*) FROM derived_skill_states), (SELECT COUNT(*) FROM derived_skill_refresh_queue)",
        [], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    assert_eq!(
        counts,
        (0, 1),
        "the caller's rollback restores pending work"
    );
}

#[test]
fn issuer_recognition_and_pending_reputation_use_targeted_indexes() {
    let db = test_db();
    for (sql, expected_index) in [
        ("EXPLAIN QUERY PLAN SELECT subject_did, skill_id FROM credentials WHERE
          CASE WHEN json_valid(signed_vc_json) THEN json_extract(signed_vc_json, '$.issuer') END = ?1",
          "idx_credentials_scoring_issuer"),
        ("EXPLAIN QUERY PLAN SELECT id FROM reputation_assertions
          WHERE input_policy_state = 'needs_refresh' AND ?1 IS NOT NULL",
          "idx_reputation_needs_refresh"),
    ] {
        let details = db.conn().prepare(sql).unwrap()
            .query_map([course_authority_did(AUTHOR).as_str()], |r| r.get::<_, String>(3)).unwrap()
            .collect::<Result<Vec<_>, _>>().unwrap().join("\n");
        assert!(details.contains(expected_index), "unexpected plan: {details}");
    }
}
