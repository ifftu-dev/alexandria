//! Verifiable Credential domain types (VC-first credential model).
//!
//! Per the Alexandria Credential & Reputation Protocol v1, the
//! **canonical credential is a signed W3C VC**, not a Cardano NFT.
//! This module defines the types; sub-modules handle canonicalization,
//! signing, and verification for the local-first VC implementation.

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

/// Cardano on-chain witness for auto-earned credentials.
///
/// When a learner's element-completion tx is confirmed by the
/// `completion.ak` validator (or an equivalent authorised validator),
/// the observer auto-issues a self-signed VC referencing that tx.
/// The witness block is part of the signed envelope — the learner
/// asserts *"this credential is authorised by on-chain tx X under
/// validator script Y"* and the JWS covers that assertion.
///
/// Verifiers resolve the `tx_hash` on Cardano and check that it locks
/// at `validator_script_hash`; if it does not, the credential MUST be
/// rejected. A VC without a `Witness` block is a manually-issued
/// credential and relies on the issuer DID alone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Witness {
    /// Confirmed Cardano tx hash that locked at the validator.
    pub tx_hash: String,
    /// Hex-encoded script hash of the authorising validator.
    pub validator_script_hash: String,
    /// Human-readable validator name (e.g. `"completion.ak"`).
    pub validator_name: String,
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
    /// On-chain witness for auto-earned credentials (§14.7). Signed
    /// over by the JWS when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness: Option<Witness>,
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
///
/// `suspended` and `superseded` were added in the §11.3/§11.4
/// follow-up. `#[serde(default)]` is set so older persisted results
/// (e.g. ones hydrated from an earlier schema or an older peer's
/// gossip) round-trip without a migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub credential_id: String,
    pub valid_signature: bool,
    pub issuer_resolved: bool,
    pub revoked: bool,
    pub expired: bool,
    pub subject_bound: bool,
    pub integrity_anchored: bool,
    /// True if the credential is currently within an active
    /// suspension window (§11.3).
    #[serde(default)]
    pub suspended: bool,
    /// True if any newer credential claims to supersede this one
    /// via `supersedes` (§11.4).
    #[serde(default)]
    pub superseded: bool,
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
    /// §11.3 says suspended credentials MUST be excluded from
    /// positive active computations. Default true.
    #[serde(default = "default_true")]
    pub reject_suspended: bool,
    /// §11.4 says superseded credentials SHOULD NOT be treated as
    /// the current active state. Default true.
    #[serde(default = "default_true")]
    pub reject_superseded: bool,
}

fn default_true() -> bool {
    true
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
            reject_suspended: true,
            reject_superseded: true,
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

// ---------------------------------------------------------------------------
// Shape-level tests for the VC domain types. These don't depend on any
// stubbed function body; they lock in the serde surface (field names,
// snake_case vs camelCase, optional-field handling) against accidental
// regressions as the implementation fills in around them.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_verification_policy_rejects_expired() {
        // Spec §11.1: strict default MUST treat expired formal
        // credentials as inactive.
        let p = VerificationPolicy::default();
        assert!(p.reject_expired);
        assert!(!p.require_integrity_anchor);
    }

    #[test]
    fn default_policy_allows_all_credential_types() {
        // Out of the box, every enum variant is acceptable; policies
        // narrow this later (e.g., a hiring portal that only accepts
        // FormalCredential + AssessmentCredential).
        let p = VerificationPolicy::default();
        assert!(p.allowed_types.contains(&CredentialType::FormalCredential));
        assert!(p
            .allowed_types
            .contains(&CredentialType::AssessmentCredential));
        assert!(p
            .allowed_types
            .contains(&CredentialType::AttestationCredential));
    }

    #[test]
    fn credential_type_serializes_as_pascal_case() {
        // Must match the `type` field values in §7's canonical JSON.
        let json = serde_json::to_string(&CredentialType::FormalCredential).unwrap();
        assert_eq!(json, "\"FormalCredential\"");
    }

    #[test]
    fn acceptance_decision_serializes_as_lowercase() {
        // Spec §13.1 shows `"acceptanceDecision": "accept"`.
        let json = serde_json::to_string(&AcceptanceDecision::Accept).unwrap();
        assert_eq!(json, "\"accept\"");
    }

    #[test]
    fn claim_tag_is_snake_case() {
        // `Claim::Skill` must serialize with `"kind": "skill"` to
        // match the payload shape in §7.
        let claim = Claim::Skill(SkillClaim {
            skill_id: "skill_x".into(),
            level: 3,
            score: 0.5,
            evidence_refs: vec![],
            rubric_version: None,
            assessment_method: None,
        });
        let v = serde_json::to_value(&claim).unwrap();
        assert_eq!(v.get("kind").and_then(|x| x.as_str()), Some("skill"));
    }
}
