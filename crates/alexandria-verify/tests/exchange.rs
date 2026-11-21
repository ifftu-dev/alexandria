//! The credential exchange over W3C Verifiable Presentations: the holder's
//! presentation must be bound to the exact request, short-lived, signed by
//! the requested candidate, and carry a credential that verifies on its own.
use alexandria_verify::did::derive_did_key;
use alexandria_verify::exchange::*;
use alexandria_verify::integrity::{TrustProfile, VerificationRung};
use alexandria_verify::policy::{IssuancePolicy, ProbePolicy};
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
            issuance_policy: None,
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

/// A self-issued credential as issued today: a revocation entry and a
/// suspension entry, each into its own list.
fn with_both_status_entries(mut f: Fixture) -> Fixture {
    let mut raw = serde_json::to_value(&f.credential).unwrap();
    raw["credentialStatus"] = json!([
        {"id":"urn:status:1#0","type":"BitstringStatusListEntry","statusPurpose":"revocation","statusListIndex":"0","statusListCredential":"urn:status:1"},
        {"id":"urn:status:2#0","type":"BitstringStatusListEntry","statusPurpose":"suspension","statusListIndex":"0","statusListCredential":"urn:status:2"}
    ]);
    let vc: VerifiableCredential = serde_json::from_value(raw).unwrap();
    let did = derive_did_key(&f.key);
    f.credential = sign_credential(UnsignedCredential { credential: vc }, &f.key, &did).unwrap();
    f
}

#[test]
fn a_holder_issuers_suspension_reads_through_the_suspension_entry() {
    let mut f = with_both_status_entries(fixture());
    let state = f.issuer_state.as_mut().unwrap();
    state.suspended = true;
    let result = verify_share(&present(&f), &f.request, NOW + 10, NOW_ISO).unwrap();
    assert!(result.suspended);
    assert!(!result.revoked);
    assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);

    // A timed suspension whose time has passed no longer holds.
    f.issuer_state.as_mut().unwrap().suspended_until = Some("2026-05-01T00:00:00Z".into());
    let result = verify_share(&present(&f), &f.request, NOW + 10, NOW_ISO).unwrap();
    assert!(!result.suspended);
    assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);

    // Without the holder's word on status, both lists are unavailable.
    f.issuer_state = None;
    let result = verify_share(&present(&f), &f.request, NOW + 10, NOW_ISO).unwrap();
    assert_eq!(result.acceptance_decision, AcceptanceDecision::Pending);
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
    f.credential.issuer = derive_did_key(&SigningKey::from_bytes(&[92; 32])).into();
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

fn camera_policy() -> IssuancePolicy {
    IssuancePolicy {
        min_integrity: Some(0.8),
        max_critical: Some(0),
        max_warning: Some(2),
        require_clean: true,
        required_assurance_level: Some("local".into()),
        probe_policy: Some(ProbePolicy::Reseed),
        probe_connection_id: None,
        min_trust_profile: Some(TrustProfile::Camera),
        min_verification_rung: Some(VerificationRung::Process),
        required_answer_custody: None,
    }
}

#[test]
fn a_request_without_a_policy_serialises_as_0_6_0_did() {
    let f = fixture();
    // The exact bytes 0.6.0 wrote for this request: the policy field adds
    // nothing when absent, so org proofs and presentations made before it
    // existed still verify.
    let expected = format!(
        concat!(
            r#"{{"id":"test-request","audience":"urn:organization:a","nonce":"test-nonce","#,
            r#""organization":"Example","subject_did":"{}","skill_id":"skill_vector","#,
            r#""network_id":"test","taxonomy_digest":"digest","purpose":"Review my credential","#,
            r#""role_label":"Engineer","require_new_assessment":false,"created_at":{},"expires_at":{}}}"#
        ),
        f.request.subject_did,
        NOW - 100,
        NOW + 1000
    );
    assert_eq!(serde_json::to_string(&f.request).unwrap(), expected);
    let parsed: CredentialRequest = serde_json::from_str(&expected).unwrap();
    assert_eq!(parsed, f.request);
}

#[test]
fn a_request_with_a_policy_round_trips_and_its_presentation_verifies() {
    let mut f = fixture();
    f.request.issuance_policy = Some(camera_policy());
    let json = serde_json::to_string(&f.request).unwrap();
    assert!(json.contains(r#""issuance_policy":{"min_integrity":0.8,"#));
    assert!(!json.contains("probe_connection_id"));
    let parsed: CredentialRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, f.request);

    let vp = present(&f);
    let carried = &serde_json::to_value(&vp).unwrap()["request"]["issuance_policy"];
    assert_eq!(carried["min_trust_profile"], "camera");
    assert_eq!(decision(&vp, &f.request), AcceptanceDecision::Accept);
}

#[test]
fn a_share_whose_request_policy_differs_from_the_expected_one_is_refused() {
    let mut f = fixture();
    f.request.issuance_policy = Some(camera_policy());
    let vp = present(&f);

    let mut weaker = f.request.clone();
    weaker.issuance_policy.as_mut().unwrap().min_trust_profile = Some(TrustProfile::Core);
    assert!(verify_share(&vp, &weaker, NOW + 10, NOW_ISO).is_err());

    let mut dropped = f.request.clone();
    dropped.issuance_policy = None;
    assert!(verify_share(&vp, &dropped, NOW + 10, NOW_ISO).is_err());

    // And the other way: a share made without the policy does not answer a
    // request that carries one.
    let mut plain = fixture();
    plain.request.issuance_policy = None;
    let vp = present(&plain);
    let mut expected = plain.request.clone();
    expected.issuance_policy = Some(camera_policy());
    assert!(verify_share(&vp, &expected, NOW + 10, NOW_ISO).is_err());
}

#[test]
fn a_request_naming_a_model_connection_is_refused_by_holder_and_verifier() {
    let mut f = fixture();
    let mut policy = camera_policy();
    policy.probe_policy = Some(ProbePolicy::Llm);
    policy.probe_connection_id = Some("conn-1".into());
    f.request.issuance_policy = Some(policy.clone());
    assert!(f.request.check().is_err());
    assert!(
        present_credential(f.request.clone(), f.credential.clone(), None, NOW, &f.key).is_err(),
        "the holder does not sign it"
    );

    // Built the documented way, the connection is gone and the rest stays.
    f.request.issuance_policy = Some(policy.for_request());
    assert!(f.request.check().is_ok());
    assert_eq!(
        f.request.issuance_policy.as_ref().unwrap().probe_policy,
        Some(ProbePolicy::Llm)
    );
    let vp = present(&f);
    let mut expected = f.request.clone();
    expected.issuance_policy = Some(policy);
    assert!(
        verify_share(&vp, &expected, NOW + 10, NOW_ISO).is_err(),
        "the verifier refuses an expected request that names one"
    );
}
