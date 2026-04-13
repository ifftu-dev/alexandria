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
        }),
        evidence_refs: vec![],
        expiration_date: None,
    };
    let vc = issue_credential_impl(db.conn(), &issuer_key, &issuer, &req, TEST_NOW).expect("issue");
    (issuer, vc.id)
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
