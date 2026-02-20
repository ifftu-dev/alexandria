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
