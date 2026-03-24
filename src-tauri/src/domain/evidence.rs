//! Evidence pipeline domain models.
//!
//! Types for assessments, evidence records, skill proofs, and
//! reputation assertions. These map directly to the SQLite schema.

use serde::{Deserialize, Serialize};

/// Bloom's taxonomy proficiency levels, ordered lowest to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProficiencyLevel {
    Remember,
    Understand,
    Apply,
    Analyze,
    Evaluate,
    Create,
}

impl ProficiencyLevel {
    /// All levels in ascending order.
    pub const ALL: &[ProficiencyLevel] = &[
        ProficiencyLevel::Remember,
        ProficiencyLevel::Understand,
        ProficiencyLevel::Apply,
        ProficiencyLevel::Analyze,
        ProficiencyLevel::Evaluate,
        ProficiencyLevel::Create,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            ProficiencyLevel::Remember => "remember",
            ProficiencyLevel::Understand => "understand",
            ProficiencyLevel::Apply => "apply",
            ProficiencyLevel::Analyze => "analyze",
            ProficiencyLevel::Evaluate => "evaluate",
            ProficiencyLevel::Create => "create",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<ProficiencyLevel> {
        match s {
            "remember" => Some(ProficiencyLevel::Remember),
            "understand" => Some(ProficiencyLevel::Understand),
            "apply" => Some(ProficiencyLevel::Apply),
            "analyze" => Some(ProficiencyLevel::Analyze),
            "evaluate" => Some(ProficiencyLevel::Evaluate),
            "create" => Some(ProficiencyLevel::Create),
            _ => None,
        }
    }
}

/// A skill assessment definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAssessment {
    pub id: String,
    pub skill_id: String,
    pub course_id: Option<String>,
    pub source_element_id: Option<String>,
    pub assessment_type: String,
    pub proficiency_level: String,
    pub difficulty: f64,
    pub weight: f64,
    pub trust_factor: f64,
    pub created_at: String,
}

/// An evidence record — a single scored attempt at a skill assessment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: String,
    pub skill_assessment_id: String,
    pub skill_id: String,
    pub proficiency_level: String,
    pub score: f64,
    pub difficulty: f64,
    pub trust_factor: f64,
    pub course_id: Option<String>,
    pub instructor_address: Option<String>,
    pub created_at: String,
}

/// A skill proof — aggregated evidence demonstrating proficiency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillProof {
    pub id: String,
    pub skill_id: String,
    pub proficiency_level: String,
    pub confidence: f64,
    pub evidence_count: i64,
    pub computed_at: String,
    pub updated_at: String,
}

/// A reputation assertion — computed impact score for an actor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationAssertion {
    pub id: String,
    pub actor_address: String,
    pub role: String,
    pub skill_id: Option<String>,
    pub proficiency_level: Option<String>,
    pub score: f64,
    pub evidence_count: i64,
    pub computation_spec: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proficiency_level_roundtrip_all() {
        for level in ProficiencyLevel::ALL {
            let s = level.as_str();
            let parsed =
                ProficiencyLevel::from_str(s).unwrap_or_else(|| panic!("failed to parse '{s}'"));
            assert_eq!(*level, parsed);
        }
    }

    #[test]
    fn proficiency_level_ordering() {
        assert!(ProficiencyLevel::Remember < ProficiencyLevel::Understand);
        assert!(ProficiencyLevel::Understand < ProficiencyLevel::Apply);
        assert!(ProficiencyLevel::Apply < ProficiencyLevel::Analyze);
        assert!(ProficiencyLevel::Analyze < ProficiencyLevel::Evaluate);
        assert!(ProficiencyLevel::Evaluate < ProficiencyLevel::Create);
    }

    #[test]
    fn proficiency_level_all_has_six() {
        assert_eq!(ProficiencyLevel::ALL.len(), 6);
    }

    #[test]
    fn proficiency_level_from_str_invalid() {
        assert!(ProficiencyLevel::from_str("").is_none());
        assert!(ProficiencyLevel::from_str("Remember").is_none()); // case-sensitive
        assert!(ProficiencyLevel::from_str("master").is_none());
    }

    #[test]
    fn proficiency_level_serde_json_roundtrip() {
        for level in ProficiencyLevel::ALL {
            let json = serde_json::to_string(level).unwrap();
            let parsed: ProficiencyLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(*level, parsed);
        }
    }

    #[test]
    fn proficiency_level_serde_uses_snake_case() {
        let json = serde_json::to_string(&ProficiencyLevel::Remember).unwrap();
        assert_eq!(json, "\"remember\"");
        let json = serde_json::to_string(&ProficiencyLevel::Create).unwrap();
        assert_eq!(json, "\"create\"");
    }

    #[test]
    fn evidence_record_serde_roundtrip() {
        let record = EvidenceRecord {
            id: "ev1".into(),
            skill_assessment_id: "sa1".into(),
            skill_id: "sk1".into(),
            proficiency_level: "apply".into(),
            score: 0.85,
            difficulty: 0.7,
            trust_factor: 1.0,
            course_id: Some("c1".into()),
            instructor_address: None,
            created_at: "2025-01-01 00:00:00".into(),
        };
        let json = serde_json::to_string(&record).unwrap();
        let parsed: EvidenceRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "ev1");
        assert_eq!(parsed.score, 0.85);
        assert!(parsed.instructor_address.is_none());
        assert_eq!(parsed.course_id.unwrap(), "c1");
    }

    #[test]
    fn skill_proof_serde_roundtrip() {
        let proof = SkillProof {
            id: "sp1".into(),
            skill_id: "sk1".into(),
            proficiency_level: "analyze".into(),
            confidence: 0.92,
            evidence_count: 12,
            computed_at: "2025-01-01".into(),
            updated_at: "2025-01-02".into(),
        };
        let json = serde_json::to_string(&proof).unwrap();
        let parsed: SkillProof = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.confidence, 0.92);
        assert_eq!(parsed.evidence_count, 12);
    }

    #[test]
    fn evidence_announcement_serde_roundtrip() {
        let ann = EvidenceAnnouncement {
            evidence_id: "ev1".into(),
            learner_address: "stake_test1u123".into(),
            skill_id: "sk1".into(),
            proficiency_level: "apply".into(),
            assessment_id: "sa1".into(),
            score: 0.75,
            difficulty: 0.6,
            trust_factor: 1.0,
            course_id: None,
            instructor_address: Some("stake_test1uinst".into()),
            created_at: 1700000000,
        };
        let json = serde_json::to_string(&ann).unwrap();
        let parsed: EvidenceAnnouncement = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.created_at, 1700000000);
        assert!(parsed.course_id.is_none());
        assert_eq!(parsed.instructor_address.unwrap(), "stake_test1uinst");
    }
}

/// An evidence announcement broadcast on `/alexandria/evidence/1.0`.
///
/// This is the gossip payload for sharing evidence records across the
/// P2P network. Other nodes store these for reputation computation.
/// Matches the spec §10.1 evidence record structure.
///
/// **Important**: Received evidence does NOT trigger local aggregation.
/// Only the learner's own node aggregates proofs — peers store evidence
/// solely for reputation inputs (instructor impact, verification).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceAnnouncement {
    /// Deterministic evidence ID: `blake2b(learner + assessment + timestamp + skill)`.
    pub evidence_id: String,
    /// Learner's Cardano stake address (bech32).
    pub learner_address: String,
    /// Skill ID this evidence applies to.
    pub skill_id: String,
    /// Bloom's taxonomy proficiency level being assessed.
    pub proficiency_level: String,
    /// Assessment ID this evidence was generated from.
    pub assessment_id: String,
    /// Score achieved (0.0 to 1.0).
    pub score: f64,
    /// Assessment difficulty (0.0 to 1.0).
    pub difficulty: f64,
    /// Assessment trust factor (default 1.0).
    pub trust_factor: f64,
    /// Course ID this evidence was generated from (if any).
    pub course_id: Option<String>,
    /// Instructor's Cardano stake address (if applicable).
    pub instructor_address: Option<String>,
    /// Unix timestamp of evidence creation.
    pub created_at: i64,
}
