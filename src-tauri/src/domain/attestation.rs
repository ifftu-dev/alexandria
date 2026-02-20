//! Multi-party attestation domain types.
//!
//! For high-stakes skills (governance-gated), evidence records require
//! assessor co-signatures before contributing to skill proof aggregation.
//! Assessors are DAO-elected members with role = 'assessor'.

use serde::{Deserialize, Serialize};

/// Attestation requirement for a skill at a specific proficiency level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationRequirement {
    pub skill_id: String,
    pub proficiency_level: String,
    pub required_attestors: i64,
    pub dao_id: String,
    pub set_by_proposal: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// An assessor's attestation on an evidence record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceAttestation {
    pub id: String,
    pub evidence_id: String,
    pub attestor_address: String,
    pub attestor_role: String,
    pub attestation_type: String,
    pub integrity_score: Option<f64>,
    pub session_cid: Option<String>,
    pub signature: String,
    pub created_at: String,
}

/// Attestor role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttestorRole {
    Assessor,
    Proctor,
}

impl AttestorRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            AttestorRole::Assessor => "assessor",
            AttestorRole::Proctor => "proctor",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "assessor" => Some(Self::Assessor),
            "proctor" => Some(Self::Proctor),
            _ => None,
        }
    }
}

/// Attestation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttestationType {
    CoSign,
    ProctorVerify,
    SkillVerify,
}

impl AttestationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AttestationType::CoSign => "co_sign",
            AttestationType::ProctorVerify => "proctor_verify",
            AttestationType::SkillVerify => "skill_verify",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "co_sign" => Some(Self::CoSign),
            "proctor_verify" => Some(Self::ProctorVerify),
            "skill_verify" => Some(Self::SkillVerify),
            _ => None,
        }
    }
}

/// Parameters for setting an attestation requirement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetRequirementParams {
    pub skill_id: String,
    pub proficiency_level: String,
    pub required_attestors: i64,
    pub dao_id: String,
    pub set_by_proposal: Option<String>,
}

/// Parameters for submitting an attestation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitAttestationParams {
    pub evidence_id: String,
    pub attestation_type: Option<String>,
    pub integrity_score: Option<f64>,
    pub session_cid: Option<String>,
}

/// Attestation status for an evidence record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationStatus {
    pub evidence_id: String,
    pub skill_id: String,
    pub proficiency_level: String,
    pub required_attestors: i64,
    pub current_attestors: i64,
    pub is_fully_attested: bool,
    pub attestations: Vec<EvidenceAttestation>,
}
