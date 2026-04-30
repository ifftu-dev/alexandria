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

/// Strongly-typed view over a `credentialSubject`'s skill properties.
/// The on-disk shape is W3C VC v2 — the subject carries these fields
/// directly, not nested under a `claim` discriminator. Use
/// `SkillClaim::extract` to read one out of a [`CredentialSubject`],
/// and `into_properties` when constructing one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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

impl SkillClaim {
    /// Read a SkillClaim out of a subject's free-form properties.
    /// Returns None if the subject doesn't carry the marker `skillId`.
    pub fn extract(subject: &CredentialSubject) -> Option<Self> {
        if !subject.properties.contains_key("skillId") {
            return None;
        }
        let v = serde_json::Value::Object(subject.properties.clone());
        serde_json::from_value(v).ok()
    }

    /// Render this claim as a property map for embedding in a
    /// [`CredentialSubject`]. The keys are camelCase JSON.
    pub fn into_properties(self) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::to_value(self).expect("SkillClaim serializes") {
            serde_json::Value::Object(m) => m,
            _ => unreachable!("SkillClaim always serializes to a JSON object"),
        }
    }
}

/// Strongly-typed view over a `credentialSubject`'s role properties.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoleClaim {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl RoleClaim {
    pub fn extract(subject: &CredentialSubject) -> Option<Self> {
        if !subject.properties.contains_key("role") {
            return None;
        }
        let v = serde_json::Value::Object(subject.properties.clone());
        serde_json::from_value(v).ok()
    }

    pub fn into_properties(self) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::to_value(self).expect("RoleClaim serializes") {
            serde_json::Value::Object(m) => m,
            _ => unreachable!("RoleClaim always serializes to a JSON object"),
        }
    }
}

/// Free-form custom claim — any properties the issuer wants to attach
/// to the subject that aren't covered by [`SkillClaim`] / [`RoleClaim`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomClaim {
    #[serde(flatten)]
    pub properties: serde_json::Map<String, serde_json::Value>,
}

impl CustomClaim {
    pub fn into_properties(self) -> serde_json::Map<String, serde_json::Value> {
        self.properties
    }
}

/// Request-side enum for VC issuance. Carries enough information to
/// build a [`CredentialSubject`] and the legacy `claim_kind` DB column.
/// Frontends submit this as `{"kind":"skill"|"role"|"custom", ...}`.
/// It is **not** part of the on-disk VC shape — that's handled by
/// [`CredentialSubject`] directly per W3C VC v2.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Claim {
    Skill(SkillClaim),
    Role(RoleClaim),
    Custom(CustomClaim),
}

impl Claim {
    /// Discriminator for the legacy `credentials.claim_kind` column.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Claim::Skill(_) => "skill",
            Claim::Role(_) => "role",
            Claim::Custom(_) => "custom",
        }
    }

    /// Skill id when this is a skill claim, for the
    /// `credentials.skill_id` index column.
    pub fn skill_id(&self) -> Option<&str> {
        match self {
            Claim::Skill(s) => Some(&s.skill_id),
            _ => None,
        }
    }

    /// Build a [`CredentialSubject`] for `subject_did` from this claim.
    pub fn into_subject(self, subject_did: Did) -> CredentialSubject {
        let properties = match self {
            Claim::Skill(s) => s.into_properties(),
            Claim::Role(r) => r.into_properties(),
            Claim::Custom(c) => c.into_properties(),
        };
        CredentialSubject {
            id: subject_did,
            properties,
        }
    }
}

/// `credentialSubject` per W3C VC v2 §4.4: a `id` plus an open-ended
/// property bag. Typed views (`SkillClaim::extract`, `RoleClaim::extract`)
/// read structured claims back out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSubject {
    pub id: Did,
    /// All other properties on the subject, serialized inline.
    #[serde(flatten)]
    pub properties: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatus {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub status_purpose: String,
    pub status_list_index: String,
    pub status_list_credential: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct Witness {
    /// Confirmed Cardano tx hash that locked at the validator.
    pub tx_hash: String,
    /// Hex-encoded script hash of the authorising validator.
    pub validator_script_hash: String,
    /// Human-readable validator name (e.g. `"completion.ak"`).
    pub validator_name: String,
}

/// The signed credential envelope. Serialises to W3C VC v2 JSON-LD.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiableCredential {
    #[serde(rename = "@context")]
    pub context: Vec<String>,
    /// Optional per W3C VC v2 §4.3 — locally-issued credentials always
    /// populate this, but external VCs without an `id` are still valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub type_: Vec<String>,
    pub issuer: Did,
    pub valid_from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    /// Echoes the verified credential's envelope `id`. Empty string
    /// when the credential carries no `id` (W3C VC v2 allows this).
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
    fn claim_request_tag_is_snake_case() {
        // The IPC request enum keeps the kind discriminator so the
        // frontend can submit `{"kind":"skill", ...}`. The tag does
        // NOT appear on the on-disk subject — it's request-shape only.
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

    #[test]
    fn skill_claim_round_trips_through_subject() {
        // Constructing a subject from a SkillClaim and reading it
        // back via `extract` MUST be lossless — this is the contract
        // issuance + aggregation depend on.
        let original = SkillClaim {
            skill_id: "skill_y".into(),
            level: 4,
            score: 0.92,
            evidence_refs: vec!["urn:uuid:e1".into()],
            rubric_version: Some("v2".into()),
            assessment_method: Some("project".into()),
        };
        let subject = CredentialSubject {
            id: Did("did:key:zSubject".into()),
            properties: original.clone().into_properties(),
        };
        let recovered = SkillClaim::extract(&subject).expect("skill claim present");
        assert_eq!(original, recovered);
    }

    #[test]
    fn skill_claim_serializes_camel_case() {
        // On the wire and on disk we conform to W3C VC v2 / JS
        // convention: `skillId`, `evidenceRefs`, `rubricVersion`,
        // `assessmentMethod`. The Rust field names stay snake_case.
        let s = SkillClaim {
            skill_id: "skill_x".into(),
            level: 1,
            score: 0.5,
            evidence_refs: vec![],
            rubric_version: None,
            assessment_method: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("skillId").is_some(), "got {v}");
        assert!(v.get("skill_id").is_none());
    }

    #[test]
    fn role_claim_extract_returns_none_for_skill_subject() {
        // A subject carrying skill properties must not be misread as
        // a role claim — `extract` keys off the marker property.
        let subject = CredentialSubject {
            id: Did("did:key:zS".into()),
            properties: SkillClaim {
                skill_id: "x".into(),
                level: 0,
                score: 0.0,
                evidence_refs: vec![],
                rubric_version: None,
                assessment_method: None,
            }
            .into_properties(),
        };
        assert!(RoleClaim::extract(&subject).is_none());
    }
}
