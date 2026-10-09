use alexandria_verify::did::derive_did_key;
use alexandria_verify::exchange::*;
use alexandria_verify::vc::{
    sign::{sign_credential, UnsignedCredential},
    AcceptanceDecision, VerifiableCredential,
};
use ed25519_dalek::SigningKey;
use serde_json::json;

fn fixture() -> (SigningKey, CredentialShare) {
    let key = SigningKey::from_bytes(&[91; 32]);
    let did = derive_did_key(&key);
    let mut raw: serde_json::Value =
        serde_json::from_str(include_str!("vectors/01-valid.json")).unwrap();
    raw["credential"]["credentialSubject"]["id"] = json!(did.as_str());
    raw["credential"]["credentialStatus"] = json!({"id":"urn:status#0","type":"BitstringStatusListEntry","statusPurpose":"revocation","statusListIndex":"0","statusListCredential":"urn:status"});
    let vc: VerifiableCredential = serde_json::from_value(raw["credential"].clone()).unwrap();
    let credential = sign_credential(UnsignedCredential { credential: vc }, &key, &did).unwrap();
    (
        key,
        CredentialShare {
            format: FORMAT.into(),
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
                created_at: 100,
                expires_at: 1000,
            },
            issued_at: 200,
            expires_at: 500,
            credential,
            issuer_state: Some(IssuerState {
                revoked: false,
                suspended: false,
                suspended_until: None,
                superseded: false,
            }),
        },
    )
}

fn decision(signed: &SignedCredentialShare) -> AcceptanceDecision {
    verify_share(signed, &signed.share.request, 210, "2026-06-01T00:00:00Z")
        .unwrap()
        .acceptance_decision
}

#[test]
fn signed_self_issued_status_accepts_but_absent_status_stays_pending() {
    let (key, mut share) = fixture();
    assert_eq!(
        decision(&sign_share(share.clone(), &key).unwrap()),
        AcceptanceDecision::Accept
    );
    share.issuer_state = None;
    assert_eq!(
        decision(&sign_share(share, &key).unwrap()),
        AcceptanceDecision::Pending
    );
}

#[test]
fn rejects_tampering_and_other_request_recipient_or_nonce() {
    let (key, share) = fixture();
    let signed = sign_share(share, &key).unwrap();
    let mut tampered = signed.clone();
    tampered.share.credential.id = Some("urn:tampered".into());
    assert!(verify_share(
        &tampered,
        &signed.share.request,
        210,
        "2026-06-01T00:00:00Z"
    )
    .is_err());
    for field in [
        "id",
        "audience",
        "nonce",
        "taxonomy_digest",
        "network_id",
        "subject_did",
    ] {
        let mut expected = serde_json::to_value(&signed.share.request).unwrap();
        expected[field] = json!("other");
        assert!(
            verify_share(
                &signed,
                &serde_json::from_value(expected).unwrap(),
                210,
                "2026-06-01T00:00:00Z"
            )
            .is_err(),
            "{field}"
        );
    }
    assert!(verify_share(&signed, &signed.share.request, 500, "2026-06-01T00:00:00Z").is_err());
}

#[test]
fn signed_revocation_suspension_and_supersession_are_rejected() {
    for flag in ["revoked", "suspended", "superseded"] {
        let (key, mut share) = fixture();
        let mut state = serde_json::to_value(&share.issuer_state).unwrap();
        state[flag] = json!(true);
        share.issuer_state = Some(serde_json::from_value(state).unwrap());
        assert_eq!(
            decision(&sign_share(share, &key).unwrap()),
            AcceptanceDecision::Reject,
            "{flag}"
        );
    }
}

#[test]
fn existing_credential_cannot_fulfill_new_assessment() {
    let (key, mut share) = fixture();
    share.request.require_new_assessment = true;
    let signed = sign_share(share, &key).unwrap();
    assert!(verify_share(&signed, &signed.share.request, 210, "2026-06-01T00:00:00Z").is_err());
}

#[test]
fn holder_cannot_assert_another_issuers_status_or_hide_bad_vc_signature() {
    let (key, mut share) = fixture();
    share.credential.proof.proof_value.push('x');
    assert_eq!(
        decision(&sign_share(share, &key).unwrap()),
        AcceptanceDecision::Reject
    );
    let (key, mut share) = fixture();
    share.credential.issuer = derive_did_key(&SigningKey::from_bytes(&[92; 32]));
    let signed = sign_share(share, &key).unwrap();
    assert!(verify_share(&signed, &signed.share.request, 210, "2026-06-01T00:00:00Z").is_err());
}

#[test]
fn issuer_revocation_is_respected_even_without_a_status_list_pointer() {
    let (key, mut share) = fixture();
    share.credential.credential_status = None;
    share.credential = sign_credential(
        UnsignedCredential {
            credential: share.credential,
        },
        &key,
        &derive_did_key(&key),
    )
    .unwrap();
    share.issuer_state.as_mut().unwrap().revoked = true;
    assert_eq!(
        decision(&sign_share(share, &key).unwrap()),
        AcceptanceDecision::Reject
    );
}
