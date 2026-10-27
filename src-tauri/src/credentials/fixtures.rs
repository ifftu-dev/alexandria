//! Shared test fixtures: a fixed time, deterministic keys, a migrated
//! database with an issuer and a subject, and a sample skill credential
//! request.

use ed25519_dalek::SigningKey;

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
