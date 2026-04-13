//! Verifiable Credential domain types (VC-first credential model).
//!
//! Per the Alexandria Credential & Reputation Protocol v1, the
//! **canonical credential is a signed W3C VC**, not a Cardano NFT.
//! This module defines the types; sub-modules handle canonicalization,
//! signing, and verification. All function bodies stubbed until the
//! corresponding implementation PRs land.

pub mod canonicalize;
pub mod context;
pub mod sign;
pub mod verify;

use serde::{Deserialize, Serialize};

use crate::crypto::did::{Did, VerificationMethodRef};

/// High-level credential classes (spec §6). The `type` field on the
/// JSON-LD credential is always `["VerifiableCredential", <class>]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum CredentialType {
    FormalCredential,
    AssessmentCredential,
    AttestationCredential,
    RoleCredential,
    DerivedCredential,
    SelfAssertion,
}

/// A single claim payload. The spec (§7) uses a permissive JSON shape;
/// we strongly-type the common cases and fall back to free JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Claim {
    Skill(SkillClaim),
    Role(RoleClaim),
    Custom(serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillClaim {
    pub skill_id: String,
    pub level: u8,
    pub score: f64,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub rubric_version: Option<String>,
    #[serde(default)]
    pub assessment_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleClaim {
    pub role: String,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSubject {
    pub id: Did,
    pub claim: Claim,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialStatus {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub status_purpose: String,
    pub status_list_index: String,
    pub status_list_credential: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermsOfUse {
    pub policy_version: String,
    pub usage: String,
}

/// The signed credential envelope. Serialises to JSON-LD per §7.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiableCredential {
    #[serde(rename = "@context")]
    pub context: Vec<String>,
    pub id: String,
    #[serde(rename = "type")]
    pub type_: Vec<String>,
    pub issuer: Did,
    pub issuance_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiration_date: Option<String>,
    pub credential_subject: CredentialSubject,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_status: Option<CredentialStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms_of_use: Option<TermsOfUse>,
    pub proof: Proof,
}

/// Ed25519Signature2020 proof block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proof {
    #[serde(rename = "type")]
    pub type_: String,
    pub created: String,
    pub verification_method: VerificationMethodRef,
    pub proof_purpose: String,
    pub jws: String,
}

/// Output of the verification algorithm (§13.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub credential_id: String,
    pub valid_signature: bool,
    pub issuer_resolved: bool,
    pub revoked: bool,
    pub expired: bool,
    pub subject_bound: bool,
    pub integrity_anchored: bool,
    pub verification_time: String,
    pub acceptance_decision: AcceptanceDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AcceptanceDecision {
    Accept,
    Reject,
}

/// Verifier policy — configurable thresholds + acceptance rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationPolicy {
    pub reject_expired: bool,
    pub require_integrity_anchor: bool,
    pub allowed_types: Vec<CredentialType>,
}

impl Default for VerificationPolicy {
    fn default() -> Self {
        Self {
            reject_expired: true,
            require_integrity_anchor: false,
            allowed_types: vec![
                CredentialType::FormalCredential,
                CredentialType::AssessmentCredential,
                CredentialType::AttestationCredential,
                CredentialType::RoleCredential,
                CredentialType::DerivedCredential,
                CredentialType::SelfAssertion,
            ],
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VcError {
    #[error("canonicalization failed: {0}")]
    Canonicalize(String),
    #[error("signature error: {0}")]
    Signature(String),
    #[error("invalid credential: {0}")]
    InvalidCredential(String),
    #[error("did error: {0}")]
    Did(#[from] crate::crypto::did::DidError),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}
