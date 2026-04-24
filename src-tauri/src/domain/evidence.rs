//! Proficiency-level taxonomy and reputation types.
//!
//! Post-migration 040 this module no longer holds the legacy evidence
//! pipeline (SkillAssessment / EvidenceRecord / SkillProof). Those were
//! retired when VCs became the sole credential artifact. What stays:
//!
//! * `ProficiencyLevel` — Bloom's taxonomy enum, still used as the
//!   proficiency level on VC claims (`credentials.skill_id` +
//!   claim-embedded `proficiency_level`).
//! * `ReputationAssertion` — computed reputation row, kept because the
//!   reputation engine survives; only its *input* changes (credentials
//!   instead of skill_proofs).

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
}
