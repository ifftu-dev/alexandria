//! §7 / §9 / §10 / §11 / §13 — VC issue → sign → verify → revoke cycle.

use super::common::{new_test_db, test_did, TEST_NOW};
use app_lib::domain::vc::{
    sign::{sign_credential, UnsignedCredential},
    verify::verify_credential,
    AcceptanceDecision, Claim, CredentialSubject, Proof, SkillClaim, VerifiableCredential,
    VerificationPolicy,
};

fn sample_unsigned(subject: app_lib::crypto::did::Did) -> UnsignedCredential {
    UnsignedCredential {
        credential: VerifiableCredential {
            context: vec!["https://www.w3.org/2018/credentials/v1".into()],
            id: "urn:uuid:test-credential".into(),
            type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
            issuer: test_did("issuer"),
            issuance_date: TEST_NOW.into(),
            expiration_date: None,
            credential_subject: CredentialSubject {
                id: subject,
                claim: Claim::Skill(SkillClaim {
                    skill_id: "skill_big_o".into(),
                    level: 4,
                    score: 0.82,
                    evidence_refs: vec![],
                    rubric_version: Some("v1".into()),
                    assessment_method: Some("exam".into()),
                }),
            },
            credential_status: None,
            terms_of_use: None,
            proof: Proof {
                type_: "Ed25519Signature2020".into(),
                created: TEST_NOW.into(),
                verification_method: app_lib::crypto::did::VerificationMethodRef(
                    "did:key:z...#key-1".into(),
                ),
                proof_purpose: "assertionMethod".into(),
                jws: String::new(), // populated by sign_credential
            },
        },
    }
}

#[tokio::test]
async fn sign_then_verify_roundtrip_accepts() {
    let db = new_test_db();
    let subject = test_did("alice");
    let issuer_key = super::common::test_key("issuer");
    let issuer_did = test_did("issuer");

    let signed =
        sign_credential(sample_unsigned(subject.clone()), &issuer_key, &issuer_did).expect("sign");
    let result = verify_credential(db.conn(), &signed, TEST_NOW, &VerificationPolicy::default());
    assert!(result.valid_signature);
    assert!(result.subject_bound);
    assert!(!result.revoked);
    assert!(!result.expired);
    assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
}

#[tokio::test]
async fn tampered_payload_fails_verification() {
    let db = new_test_db();
    let subject = test_did("alice");
    let issuer_key = super::common::test_key("issuer");
    let issuer_did = test_did("issuer");

    let mut signed =
        sign_credential(sample_unsigned(subject), &issuer_key, &issuer_did).expect("sign");
    // Tamper: raise the score after signing
    if let Claim::Skill(ref mut s) = signed.credential_subject.claim {
        s.score = 1.0;
    }
    let result = verify_credential(db.conn(), &signed, TEST_NOW, &VerificationPolicy::default());
    assert!(!result.valid_signature);
    assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
}

#[tokio::test]
#[ignore = "pending PR 5 — canonical storage + status list"]
async fn revoked_credential_is_rejected() {
    // Issue → publish a revocation in the status list → verify rejects.
    // Requires the credentials table + status_lists table from PR 5.
    unimplemented!("drive via commands::credentials::{{issue_credential, revoke_credential}}")
}

#[tokio::test]
async fn wrong_subject_binding_is_rejected() {
    // §10 semantic non-transferability: a subject.id that isn't a
    // well-formed DID can't be bound to a presenter, so the verifier
    // MUST reject. We don't need a separate "presenter" identity
    // here — the subject field itself fails the DID check.
    let db = new_test_db();
    let issuer_key = super::common::test_key("issuer");
    let issuer_did = test_did("issuer");
    let mut unsigned = sample_unsigned(test_did("alice"));
    // Replace the subject with a non-DID identifier.
    unsigned.credential.credential_subject.id = app_lib::crypto::did::Did("not-a-did".into());
    let signed = sign_credential(unsigned, &issuer_key, &issuer_did).expect("sign");
    let result = verify_credential(db.conn(), &signed, TEST_NOW, &VerificationPolicy::default());
    assert!(!result.subject_bound);
    assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
}

#[tokio::test]
async fn expired_credential_is_rejected_under_strict_policy() {
    // §11.1: default policy SHOULD treat expired formal credentials as
    // inactive. expirationDate < TEST_NOW ⇒ `expired=true` ⇒ Reject
    // under `reject_expired=true`.
    let db = new_test_db();
    let issuer_key = super::common::test_key("issuer");
    let issuer_did = test_did("issuer");
    let mut unsigned = sample_unsigned(test_did("alice"));
    unsigned.credential.expiration_date = Some("2026-01-01T00:00:00Z".into());
    let signed = sign_credential(unsigned, &issuer_key, &issuer_did).expect("sign");
    let result = verify_credential(db.conn(), &signed, TEST_NOW, &VerificationPolicy::default());
    assert!(result.expired);
    assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
}
