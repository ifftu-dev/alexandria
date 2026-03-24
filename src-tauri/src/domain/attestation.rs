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

    #[allow(clippy::should_implement_trait)]
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

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "co_sign" => Some(Self::CoSign),
            "proctor_verify" => Some(Self::ProctorVerify),
            "skill_verify" => Some(Self::SkillVerify),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attestor_role_roundtrip() {
        for (variant, expected) in [
            (AttestorRole::Assessor, "assessor"),
            (AttestorRole::Proctor, "proctor"),
        ] {
            assert_eq!(variant.as_str(), expected);
            let parsed = AttestorRole::from_str(expected).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn attestor_role_from_str_invalid() {
        assert!(AttestorRole::from_str("").is_none());
        assert!(AttestorRole::from_str("Assessor").is_none());
        assert!(AttestorRole::from_str("verifier").is_none());
    }

    #[test]
    fn attestation_type_roundtrip() {
        for (variant, expected) in [
            (AttestationType::CoSign, "co_sign"),
            (AttestationType::ProctorVerify, "proctor_verify"),
            (AttestationType::SkillVerify, "skill_verify"),
        ] {
            assert_eq!(variant.as_str(), expected);
            let parsed = AttestationType::from_str(expected).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn attestation_type_from_str_invalid() {
        assert!(AttestationType::from_str("").is_none());
        assert!(AttestationType::from_str("cosign").is_none());
        assert!(AttestationType::from_str("CoSign").is_none());
    }

    #[test]
    fn attestor_role_serde_roundtrip() {
        for variant in [AttestorRole::Assessor, AttestorRole::Proctor] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: AttestorRole = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn attestation_type_serde_roundtrip() {
        for variant in [
            AttestationType::CoSign,
            AttestationType::ProctorVerify,
            AttestationType::SkillVerify,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: AttestationType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn attestation_status_serde_roundtrip() {
        let status = AttestationStatus {
            evidence_id: "ev1".into(),
            skill_id: "sk1".into(),
            proficiency_level: "apply".into(),
            required_attestors: 3,
            current_attestors: 1,
            is_fully_attested: false,
            attestations: vec![EvidenceAttestation {
                id: "att1".into(),
                evidence_id: "ev1".into(),
                attestor_address: "stake_test1u123".into(),
                attestor_role: "assessor".into(),
                attestation_type: "co_sign".into(),
                integrity_score: Some(0.95),
                session_cid: None,
                signature: "sig".into(),
                created_at: "2025-01-01".into(),
            }],
        };
        let json = serde_json::to_string(&status).unwrap();
        let parsed: AttestationStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.required_attestors, 3);
        assert_eq!(parsed.current_attestors, 1);
        assert!(!parsed.is_fully_attested);
        assert_eq!(parsed.attestations.len(), 1);
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
