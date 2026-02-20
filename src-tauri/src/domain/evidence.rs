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
