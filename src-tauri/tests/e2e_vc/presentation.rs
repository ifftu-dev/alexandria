//! §18 — Selective disclosure: field subset + audience + nonce.

use super::common::{new_test_db, test_did, test_key, TEST_NOW};
use app_lib::commands::credentials::{issue_credential_impl, IssueCredentialRequest};
use app_lib::commands::presentation::{
    create_presentation_impl, verify_presentation_impl, CreatePresentationRequest,
    PresentationVerification,
};
use app_lib::crypto::did::derive_did_key;
use app_lib::domain::vc::{Claim, CredentialType, SkillClaim};

/// Helper: issue a single skill credential to a subject and return
/// `(db, subject_signing_key, credential_id)`.
fn issue_for_subject() -> (
    app_lib::db::Database,
    ed25519_dalek::SigningKey,
    app_lib::crypto::did::Did,
    String,
) {
    let db = new_test_db();
    let issuer_key = test_key("issuer");
    let issuer = derive_did_key(&issuer_key);
    let subject_key = test_key("subject");
    let subject = derive_did_key(&subject_key);
    let req = IssueCredentialRequest {
        credential_type: CredentialType::FormalCredential,
        subject: subject.clone(),
        claim: Claim::Skill(SkillClaim {
            skill_id: "skill_pres".into(),
            level: 4,
            score: 0.92,
            evidence_refs: vec!["urn:uuid:e1".into()],
            rubric_version: Some("v1".into()),
            assessment_method: Some("exam".into()),
        }),
        evidence_refs: vec!["urn:uuid:e1".into()],
        expiration_date: None,
    };
    let vc = issue_credential_impl(db.conn(), &issuer_key, &issuer, &req, TEST_NOW)
        .expect("issue credential");
    (db, subject_key, subject, vc.id)
}

#[tokio::test]
async fn presentation_reveals_only_requested_fields() {
    // Create presentation with reveal = ["credential_subject.claim.level"].
    // Output must not include the raw score or evidence_refs.
    let (db, subject_key, subject, cred_id) = issue_for_subject();
    let req = CreatePresentationRequest {
        credential_ids: vec![cred_id],
        reveal: vec!["credential_subject.claim.level".into()],
        audience: "did:web:hirer.example".into(),
        nonce: "n-fields".into(),
    };
    let env = create_presentation_impl(db.conn(), &subject_key, &subject, &req).expect("create");
    assert!(
        !env.payload_json.contains("\"score\""),
        "score field leaked: {}",
        env.payload_json
    );
    assert!(
        !env.payload_json.contains("evidence_refs"),
        "evidence_refs leaked: {}",
        env.payload_json
    );
    assert!(env.payload_json.contains("\"level\""), "level missing");
    // The verifier can still validate the envelope end-to-end.
    let outcome = verify_presentation_impl(db.conn(), &env, "did:web:hirer.example").unwrap();
    assert_eq!(outcome, PresentationVerification::Accepted);
    // Unused fixture refs:
    let _ = test_did("subject");
}

#[tokio::test]
async fn nonce_reuse_is_rejected_on_verification_side() {
    // Replay protection: same (audience, nonce) seen twice → reject the 2nd.
    let (db, subject_key, subject, cred_id) = issue_for_subject();
    let req = CreatePresentationRequest {
        credential_ids: vec![cred_id],
        reveal: vec!["credential_subject.claim.level".into()],
        audience: "did:web:hirer.example".into(),
        nonce: "n-replay-e2e".into(),
    };
    let env = create_presentation_impl(db.conn(), &subject_key, &subject, &req).expect("create");
    assert_eq!(
        verify_presentation_impl(db.conn(), &env, "did:web:hirer.example").unwrap(),
        PresentationVerification::Accepted
    );
    assert_eq!(
        verify_presentation_impl(db.conn(), &env, "did:web:hirer.example").unwrap(),
        PresentationVerification::Replayed
    );
}

#[tokio::test]
async fn audience_mismatch_rejected() {
    // Presentation bound to audience X, verified at audience Y → reject.
    let (db, subject_key, subject, cred_id) = issue_for_subject();
    let req = CreatePresentationRequest {
        credential_ids: vec![cred_id],
        reveal: vec!["credential_subject.claim.level".into()],
        audience: "did:web:hirer-A".into(),
        nonce: "n-aud-e2e".into(),
    };
    let env = create_presentation_impl(db.conn(), &subject_key, &subject, &req).expect("create");
    let outcome = verify_presentation_impl(db.conn(), &env, "did:web:hirer-B").unwrap();
    assert_eq!(outcome, PresentationVerification::AudienceMismatch);
}
