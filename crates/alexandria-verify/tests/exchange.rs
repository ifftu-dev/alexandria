//! The credential exchange over W3C Verifiable Presentations: the holder's
//! presentation must be bound to the exact request, short-lived, signed by
//! the requested candidate, and carry a credential that verifies on its own.
use alexandria_verify::did::derive_did_key;
use alexandria_verify::exchange::*;
use alexandria_verify::vc::presentation::{verify_presentation_proof, VerifiablePresentation};
use alexandria_verify::vc::{
    sign::{sign_credential, UnsignedCredential},
    AcceptanceDecision, VerifiableCredential,
};
use ed25519_dalek::SigningKey;
use serde_json::json;

const NOW: i64 = 1_780_272_000; // 2026-06-01T00:00:00Z
const NOW_ISO: &str = "2026-06-01T00:00:00Z";

struct Fixture {
    key: SigningKey,
    request: CredentialRequest,
    credential: VerifiableCredential,
    issuer_state: Option<IssuerState>,
}

fn fixture() -> Fixture {
    let key = SigningKey::from_bytes(&[91; 32]);
    let did = derive_did_key(&key);
    let mut raw: serde_json::Value =
        serde_json::from_str(include_str!("vectors/01-valid.json")).unwrap();
    raw["credential"]["credentialSubject"]["id"] = json!(did.as_str());
    raw["credential"]["credentialStatus"] = json!({"id":"urn:status#0","type":"BitstringStatusListEntry","statusPurpose":"revocation","statusListIndex":"0","statusListCredential":"urn:status"});
    let vc: VerifiableCredential = serde_json::from_value(raw["credential"].clone()).unwrap();
    let credential = sign_credential(UnsignedCredential { credential: vc }, &key, &did).unwrap();
    Fixture {
        key,
        request: CredentialRequest {
            id: "test-request".into(),
            audience: "urn:organization:a".into(),
            nonce: "test-nonce".into(),
            organization: "Example".into(),
            subject_did: did.0,
            skill_id: "skill_vector".into(),
            network_id: "test".into(),
            taxonomy_digest: "digest".into(),
            purpose: "Review my credential".into(),
            role_label: "Engineer".into(),
            require_new_assessment: false,
            created_at: NOW - 100,
            expires_at: NOW + 1000,
        },
        credential,
        issuer_state: Some(IssuerState {
            revoked: false,
            suspended: false,
            suspended_until: None,
            superseded: false,
        }),
    }
}

fn present(f: &Fixture) -> VerifiablePresentation {
    present_credential(
        f.request.clone(),
        f.credential.clone(),
        f.issuer_state.clone(),
        NOW,
        &f.key,
    )
    .unwrap()
}

fn decision(vp: &VerifiablePresentation, request: &CredentialRequest) -> AcceptanceDecision {
    verify_share(vp, request, NOW + 10, NOW_ISO)
        .unwrap()
        .acceptance_decision
}

#[test]
fn the_share_is_a_verifiable_presentation_bound_to_the_request() {
    let f = fixture();
    let vp = present(&f);
    let json = serde_json::to_value(&vp).unwrap();
    assert_eq!(
        json["@context"],
        json!(["https://www.w3.org/ns/credentials/v2"])
    );
    assert_eq!(json["type"], json!(["VerifiablePresentation"]));
    assert_eq!(json["holder"], f.request.subject_did);
    assert_eq!(
        json["verifiableCredential"][0]["id"],
        json!(f.credential.id)
    );
    assert_eq!(json["request"]["nonce"], "test-nonce");
    assert_eq!(json["proof"]["type"], "DataIntegrityProof");
    assert_eq!(json["proof"]["cryptosuite"], "eddsa-jcs-2022");
    assert_eq!(json["proof"]["proofPurpose"], "authentication");
    assert_eq!(json["proof"]["challenge"], "test-nonce");
    assert_eq!(json["proof"]["domain"], "urn:organization:a");
    assert_eq!(json["proof"]["created"], NOW_ISO);
    assert_eq!(json["proof"]["expires"], "2026-06-01T00:05:00Z");
    assert!(json["proof"].get("jws").is_none());
    let proof = verify_presentation_proof(&vp, NOW + 10).unwrap();
    assert_eq!(proof.created, NOW);
    assert_eq!(proof.expires, NOW + 300);
}

#[test]
fn signed_self_issued_status_accepts_but_absent_status_stays_pending() {
    let mut f = fixture();
    assert_eq!(
        decision(&present(&f), &f.request),
        AcceptanceDecision::Accept
    );
    f.issuer_state = None;
    assert_eq!(
        decision(&present(&f), &f.request),
        AcceptanceDecision::Pending
    );
}

#[test]
fn rejects_tampering_and_other_request_recipient_or_nonce() {
    let f = fixture();
    let vp = present(&f);
    let mut tampered = vp.clone();
    tampered.verifiable_credential[0]["id"] = json!("urn:tampered");
    assert!(verify_share(&tampered, &f.request, NOW + 10, NOW_ISO).is_err());
    let mut rebound = vp.clone();
    rebound.proof.domain = Some("urn:organization:b".into());
    assert!(verify_share(&rebound, &f.request, NOW + 10, NOW_ISO).is_err());
    for field in [
        "id",
        "audience",
        "nonce",
        "taxonomy_digest",
        "network_id",
        "subject_did",
    ] {
        let mut expected = serde_json::to_value(&f.request).unwrap();
        expected[field] = json!("other");
        assert!(
            verify_share(
                &vp,
                &serde_json::from_value(expected).unwrap(),
                NOW + 10,
                NOW_ISO
            )
            .is_err(),
            "{field}"
        );
    }
    assert!(
        verify_share(&vp, &f.request, NOW + 301, NOW_ISO).is_err(),
        "a presentation lives five minutes"
    );
    assert!(
        verify_share(&vp, &f.request, NOW - 60, NOW_ISO).is_err(),
        "a presentation from the future is refused"
    );
    let other = SigningKey::from_bytes(&[92; 32]);
    assert!(
        present_credential(f.request.clone(), f.credential.clone(), None, NOW, &other).is_err(),
        "only the requested candidate can present"
    );
}

#[test]
fn signed_revocation_suspension_and_supersession_are_rejected() {
    for flag in ["revoked", "suspended", "superseded"] {
        let mut f = fixture();
        let mut state = serde_json::to_value(&f.issuer_state).unwrap();
        state[flag] = json!(true);
        f.issuer_state = Some(serde_json::from_value(state).unwrap());
        assert_eq!(
            decision(&present(&f), &f.request),
            AcceptanceDecision::Reject,
            "{flag}"
        );
    }
}

#[test]
fn existing_credential_cannot_fulfill_new_assessment() {
    let mut f = fixture();
    f.request.require_new_assessment = true;
    assert!(verify_share(&present(&f), &f.request, NOW + 10, NOW_ISO).is_err());
}

#[test]
fn holder_cannot_assert_another_issuers_status_or_hide_bad_vc_signature() {
    let mut f = fixture();
    f.credential.proof.proof_value.push('x');
    assert_eq!(
        decision(&present(&f), &f.request),
        AcceptanceDecision::Reject
    );
    let mut f = fixture();
    f.credential.issuer = derive_did_key(&SigningKey::from_bytes(&[92; 32]));
    assert!(verify_share(&present(&f), &f.request, NOW + 10, NOW_ISO).is_err());
}

#[test]
fn issuer_revocation_is_respected_even_without_a_status_list_pointer() {
    let mut f = fixture();
    f.credential.credential_status = None;
    f.credential = sign_credential(
        UnsignedCredential {
            credential: f.credential,
        },
        &f.key,
        &derive_did_key(&f.key),
    )
    .unwrap();
    f.issuer_state.as_mut().unwrap().revoked = true;
    assert_eq!(
        decision(&present(&f), &f.request),
        AcceptanceDecision::Reject
    );
}

#[test]
fn a_presentation_with_two_credentials_or_no_request_is_refused() {
    let f = fixture();
    let mut two = present(&f);
    two.verifiable_credential
        .push(two.verifiable_credential[0].clone());
    assert!(verify_share(&two, &f.request, NOW + 10, NOW_ISO).is_err());
    let mut none = present(&f);
    none.properties.remove(REQUEST_PROPERTY);
    assert!(verify_share(&none, &f.request, NOW + 10, NOW_ISO).is_err());
}
