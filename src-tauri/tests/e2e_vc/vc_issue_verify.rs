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
#[ignore = "pending PR 4 — VC sign/verify"]
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
#[ignore = "pending PR 4 — VC sign/verify"]
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
#[ignore = "pending PR 4 — VC sign/verify"]
async fn wrong_subject_binding_is_rejected() {
    // Presenter is not the subject → verifier rejects per §10.
    unimplemented!("construct presentation from non-subject key and verify")
}

#[tokio::test]
#[ignore = "pending PR 4 — VC sign/verify"]
async fn expired_credential_is_rejected_under_strict_policy() {
    // expirationDate in the past → rejected under `reject_expired = true`.
    unimplemented!("set expirationDate < verification_time and assert expired=true")
}
