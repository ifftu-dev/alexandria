//! §20.4 — exported credentials verify without any Alexandria services.
//!
//! The bundle export is the long-tail durability story: a subject's
//! signed VCs, the historical key registry, and the status lists,
//! packaged into a single JSON-LD document that an external verifier
//! can re-check with no Alexandria infrastructure.
//!
//! These tests exercise the in-process offline verifier (a fresh,
//! state-free in-memory DB), which is the same shape an external
//! tool like `digitalbazaar/vc-js` would have to take. PR 12 keeps
//! the actual subprocess shell-out as a follow-up; the offline
//! verifier here proves the bundle is self-contained.

use super::common::{new_test_db, test_did, test_key, TEST_NOW};
use app_lib::commands::credentials::{
    export_bundle_impl, issue_credential_impl, revoke_credential_impl, verify_bundle_offline_impl,
    IssueCredentialRequest,
};
use app_lib::crypto::did::derive_did_key;
use app_lib::domain::vc::{Claim, CredentialType, SkillClaim};

fn issue_one(db: &app_lib::db::Database, skill: &str) -> (app_lib::crypto::did::Did, String) {
    let issuer_key = test_key("issuer-survival");
    let issuer = derive_did_key(&issuer_key);
    let subject = test_did("subject-survival");
    let req = IssueCredentialRequest {
        credential_type: CredentialType::FormalCredential,
        subject: subject.clone(),
        claim: Claim::Skill(SkillClaim {
            skill_id: skill.into(),
            level: 4,
            score: 0.85,
            evidence_refs: vec![],
            rubric_version: Some("v1".into()),
            assessment_method: Some("exam".into()),
            provenance: None,
        }),
        evidence_refs: vec![],
        expiration_date: None,
        supersedes: None,
        integrity_session_id: None,
        integrity_policy: None,
    };
    let vc = issue_credential_impl(db.conn(), &issuer_key, &issuer, &req, TEST_NOW).expect("issue");
    (issuer, vc.id.expect("issued VC always has an envelope id"))
}

#[tokio::test]
async fn exported_bundle_verifies_with_offline_tooling() {
    // The offline verifier is the in-process analogue of "shell out
    // to digitalbazaar/vc-js" — same offline contract (fresh DB, no
    // shared state), no Alexandria services running.
    let db = new_test_db();
    let _ = issue_one(&db, "skill_survival_offline");
    let bundle = export_bundle_impl(db.conn()).expect("export");
    let (accepted, total) = verify_bundle_offline_impl(&bundle, TEST_NOW).expect("verify");
    assert_eq!(total, 1);
    assert_eq!(
        accepted, 1,
        "every signed credential in the bundle must verify offline"
    );
}

#[tokio::test]
async fn exported_bundle_propagates_revocation_to_offline_verifier() {
    // The status list inside the bundle carries the revocation bit,
    // so the offline verifier sees the same Reject as the local one.
    let db = new_test_db();
    let (_issuer, cred_id) = issue_one(&db, "skill_survival_revoked");
    revoke_credential_impl(db.conn(), &cred_id, "test", TEST_NOW).expect("revoke");
    let bundle = export_bundle_impl(db.conn()).expect("export");
    let (accepted, total) = verify_bundle_offline_impl(&bundle, TEST_NOW).expect("verify");
    assert_eq!(total, 1);
    assert_eq!(accepted, 0, "revoked credential MUST NOT verify offline");
}

#[tokio::test]
async fn export_bundle_is_deterministic() {
    // §20.4: same credential set + same fixed clock ⇒ byte-identical
    // bundle. Needed so bundles round-trip through archival storage
    // (content-addressed by the bundle bytes).
    let db = new_test_db();
    let _ = issue_one(&db, "skill_survival_determinism");
    let a = export_bundle_impl(db.conn()).expect("first export");
    let b = export_bundle_impl(db.conn()).expect("second export");
    assert_eq!(
        a, b,
        "bundle MUST be byte-identical across repeated exports"
    );
}

#[tokio::test]
async fn exported_bundle_verifies_offline_under_a_rotated_issuer_key() {
    // §5.3 survivability, through the bundle path — and the only scenario in
    // which the bundle's key registry is load-bearing.
    //
    // `did:key` is self-resolving, so a credential signed under the key the DID
    // embeds always verifies without consulting a registry at all. The registry
    // matters in the other direction: after rotating, the issuer keeps their DID
    // and signs *new* credentials with key_v2. Self-resolution then yields v1 and
    // the signature fails; only the registry entry for the post-rotation window
    // produces the right key.
    //
    // This is what covers `BundleStore::key_at`. Verified by neutering that
    // method and confirming this test — and only this test — fails.
    let db = new_test_db();
    let issuer_key_v1 = test_key("issuer-survival");
    let issuer = derive_did_key(&issuer_key_v1);
    let issuer_key_v2 = test_key("issuer-rotated-v2");

    app_lib::crypto::key_registry::rotate_key(db.conn(), &issuer, &issuer_key_v2).expect("rotate");

    // Signed with v2 under the v1 DID, exactly as a post-rotation issuance is.
    let req = IssueCredentialRequest {
        credential_type: CredentialType::FormalCredential,
        subject: test_did("subject-survival"),
        claim: Claim::Skill(SkillClaim {
            skill_id: "skill_survival_rotated".into(),
            level: 4,
            score: 0.85,
            evidence_refs: vec![],
            rubric_version: Some("v1".into()),
            assessment_method: Some("exam".into()),
            provenance: None,
        }),
        evidence_refs: vec![],
        expiration_date: None,
        supersedes: None,
        integrity_session_id: None,
        integrity_policy: None,
    };
    issue_credential_impl(db.conn(), &issuer_key_v2, &issuer, &req, TEST_NOW).expect("issue");

    // Verify well after the rotation, so the open registry window is v2's.
    let after_rotation = "2099-01-01T00:00:00Z";
    let bundle = export_bundle_impl(db.conn()).expect("export");
    let (accepted, total) = verify_bundle_offline_impl(&bundle, after_rotation).expect("verify");
    assert_eq!(total, 1);
    assert_eq!(
        accepted, 1,
        "a credential signed under the rotated key must verify from the bundle \
         via the registry — did:key self-resolution yields the pre-rotation key"
    );
}
