//! Shared test fixtures: a fixed time, deterministic keys, a migrated
//! database with an issuer and a subject, and a sample skill credential
//! request.

use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection};

use crate::credentials::issue::IssueCredentialRequest;
use crate::credentials::Issuer;
use crate::crypto::did::{derive_did_key, Did};
use crate::db::Database;
use crate::domain::vc::{Claim, CredentialType, SkillClaim};

pub(crate) const NOW: &str = "2026-04-13T00:00:00Z";

pub(crate) fn test_key(role: &str) -> SigningKey {
    let mut bytes = [0u8; 32];
    let b = role.as_bytes();
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = b[i % b.len().max(1)];
    }
    SigningKey::from_bytes(&bytes)
}

pub(crate) fn setup() -> (Database, Issuer, Did, Did) {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().unwrap();
    let issuer_key = Issuer::from_key(test_key("issuer"));
    let issuer = issuer_key.did().clone();
    let subject = derive_did_key(&test_key("subject"));
    (db, issuer_key, issuer, subject)
}

pub(crate) fn sample_request(subject: Did) -> IssueCredentialRequest {
    IssueCredentialRequest {
        credential_type: CredentialType::FormalCredential,
        subject,
        claim: Claim::Skill(SkillClaim {
            skill_id: "skill_test".into(),
            level: 4,
            score: 0.82,
            evidence_refs: vec![],
            rubric_version: Some("v1".into()),
            assessment_method: Some("exam".into()),
            provenance: None,
        }),
        evidence_refs: vec!["urn:uuid:e1".into()],
        expiration_date: None,
        supersedes: None,
        integrity_session_id: None,
        integrity_policy: None,
    }
}

/// A live session that selected `profile` (with the camera benchmark
/// passing), then judged `floor` at attempt start and ended clean.
pub(crate) fn session_on_profile(
    conn: &Connection,
    id: &str,
    attested: bool,
    camera: bool,
    floor: Option<crate::sentinel::scoring::TrustProfile>,
) {
    use crate::sentinel::trust_profile::{
        record_policy_floor, select_for_session, BenchmarkResult, Capabilities, ProfileRequest,
    };
    conn.execute(
        "INSERT INTO integrity_sessions (id, status, assurance_level, started_at)
         VALUES (?1, 'active', ?2, ?3)",
        params![id, if attested { "device_attested" } else { "local" }, NOW],
    )
    .unwrap();
    let caps = Capabilities {
        camera,
        ..Default::default()
    };
    let request = ProfileRequest {
        capabilities: caps,
        consent: caps,
        role_assessment_id: None,
        exchange_request_id: None,
    };
    let fast = BenchmarkResult {
        detect_ms: 9.0,
        liveness_ms: 7.0,
        track_ms: 1.0,
    };
    select_for_session(conn, id, &request, Some(&fast)).unwrap();
    record_policy_floor(conn, id, floor).unwrap();
    conn.execute(
        "UPDATE integrity_sessions SET status = 'completed', ended_at = ?2 WHERE id = ?1",
        params![id, NOW],
    )
    .unwrap();
}

/// A finished assessment session on a profile chosen from `camera`, whose
/// attempt served `served` items, `observed` of which the response
/// tracker saw the learner work through.
pub(crate) fn assessed(conn: &Connection, id: &str, camera: bool, served: usize, observed: usize) {
    session_on_profile(conn, id, false, camera, None);
    conn.execute(
        "INSERT OR IGNORE INTO question_banks (id, skill_id, label) VALUES ('bank', 'skill_test', 'Test')",
        [],
    )
    .unwrap();
    let ids: Vec<String> = (0..served).map(|i| format!("q{i}")).collect();
    conn.execute(
        "INSERT INTO assessment_attempts
             (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders,
              integrity_session_id, started_at)
         VALUES (?1, ?5, 'bank', 'skill_test', 1, ?2, '[]', ?3, ?4)",
        params![
            format!("att-{id}"),
            serde_json::to_string(&ids).unwrap(),
            id,
            NOW,
            format!("did:key:{id}")
        ],
    )
    .unwrap();
    for item in ids.iter().take(observed) {
        conn.execute(
            "INSERT INTO integrity_observed_items (session_id, item_id) VALUES (?1, ?2)",
            params![id, item],
        )
        .unwrap();
    }
}

/// Record a verification result in a finished session, as if it had
/// been recorded before monitoring ended. A probe's source is its policy's own:
/// `reseed` asks the same item again, `authored` a new question.
pub(crate) fn verified(conn: &Connection, session: &str, step: &str, policy: &str, outcome: &str) {
    use crate::sentinel::verification::{record_verification, NewVerification};
    let outcome = match outcome {
        "satisfied" => crate::sentinel::verification::VerificationOutcome::Satisfied,
        "failed" => crate::sentinel::verification::VerificationOutcome::Failed,
        _ => crate::sentinel::verification::VerificationOutcome::Skipped,
    };
    conn.execute(
        "UPDATE integrity_sessions SET ended_at = NULL WHERE id = ?1",
        [session],
    )
    .unwrap();
    record_verification(
        conn,
        &NewVerification {
            session_id: session,
            attempt_id: None,
            step,
            policy,
            outcome,
            reason: None,
            subject_ref: None,
            detail: serde_json::json!({ "source": policy }),
        },
    )
    .unwrap();
    conn.execute(
        "UPDATE integrity_sessions SET ended_at = ?2 WHERE id = ?1",
        params![session, NOW],
    )
    .unwrap();
}

/// The session's attempt scored `score`, after `prior` graded items on
/// the same skill scored `prior_score` a day before it started.
pub(crate) fn scored_with_history(
    conn: &Connection,
    session: &str,
    score: f64,
    prior: usize,
    prior_score: f64,
) {
    conn.execute(
        "UPDATE assessment_attempts SET score = ?2 WHERE integrity_session_id = ?1",
        params![session, score],
    )
    .unwrap();
    let subject: String = conn
        .query_row(
            "SELECT subject_did FROM assessment_attempts WHERE integrity_session_id = ?1",
            [session],
            |r| r.get(0),
        )
        .unwrap();
    let earlier = format!("prior-{session}");
    conn.execute(
        "INSERT INTO assessment_attempts
             (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, started_at)
         VALUES (?1, ?2, 'bank', 'skill_test', 2, '[]', '[]', '2026-04-11T00:00:00Z')",
        params![earlier, subject],
    )
    .unwrap();
    for i in 0..prior {
        conn.execute(
            "INSERT INTO attempt_items (attempt_id, ordinal, item_id, score, graded_at)
             VALUES (?1, ?2, ?3, ?4, '2026-04-12T00:00:00Z')",
            params![earlier, i as i64, format!("p{i}"), prior_score],
        )
        .unwrap();
    }
}
