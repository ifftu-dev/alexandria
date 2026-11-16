//! §4.1 / §5.1 — DID derivation, and a `did:key` resolving to its own key.

use super::common::{new_test_db, test_did, test_key, TEST_NOW};
use app_lib::crypto::did::{derive_did_key, resolve_did_key};
use app_lib::domain::vc::{
    sign::{sign_credential, UnsignedCredential},
    verify_credential_db, AcceptanceDecision, Claim, Proof, SkillClaim, VerifiableCredential,
    VerificationPolicy,
};
use ed25519_dalek::Signer;

#[tokio::test]
async fn derive_did_key_is_deterministic() {
    let k = test_key("alice");
    let d1 = derive_did_key(&k);
    let d2 = derive_did_key(&k);
    assert_eq!(d1, d2);
    assert!(d1.as_str().starts_with("did:key:z"));
}

#[tokio::test]
async fn did_key_resolves_to_original_public_key() {
    let k = test_key("bob");
    let did = derive_did_key(&k);
    let vk = resolve_did_key(&did).expect("resolve");
    // Sign with original, verify with resolved: must match.
    let sig = k.sign(b"test");
    assert!(vk.verify_strict(b"test", &sig).is_ok());
}

#[tokio::test]
async fn a_registry_row_cannot_change_how_a_did_key_issuer_verifies() {
    // A did:key is its key. A key_registry row for the issuer's DID naming
    // another key — stale, or left from the retired rotation feature — must
    // not change which key the credential is checked against.
    let db = new_test_db();
    let issuer_key = test_key("issuer");
    let issuer = derive_did_key(&issuer_key);
    db.conn()
        .execute(
            "INSERT INTO key_registry (did, key_id, public_key_hex) VALUES (?1, 'key-2', ?2)",
            rusqlite::params![
                issuer.as_str(),
                hex::encode(test_key("issuer-other").verifying_key().as_bytes())
            ],
        )
        .expect("a well-formed registry row");

    let unsigned = UnsignedCredential {
        credential: VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some("urn:uuid:did-key-is-its-key".into()),
            type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
            issuer: issuer.clone(),
            valid_from: TEST_NOW.into(),
            valid_until: None,
            credential_subject: Claim::Skill(SkillClaim {
                skill_id: "skill_did_key".into(),
                level: 3,
                score: 0.8,
                evidence_refs: vec![],
                rubric_version: None,
                assessment_method: None,
                provenance: None,
            })
            .into_subject(test_did("subject")),
            credential_status: None,
            terms_of_use: None,
            witness: None,
            integrity: None,
            evidence: Vec::new(),
            related_resource: Vec::new(),
            proof: Proof::unsigned(TEST_NOW),
        },
    };
    let signed = sign_credential(unsigned, &issuer_key, &issuer).expect("sign");
    let result = verify_credential_db(db.conn(), &signed, TEST_NOW, &VerificationPolicy::default());
    assert!(result.issuer_resolved);
    assert!(result.valid_signature);
    assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
}
