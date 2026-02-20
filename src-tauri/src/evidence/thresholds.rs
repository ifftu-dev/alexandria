//! Bloom's taxonomy proficiency thresholds for skill proof aggregation.
//!
//! Each level has minimum evidence count, minimum confidence, and
//! optional type requirements. Levels are evaluated lowest-to-highest;
//! the first level that fails breaks the chain.

use crate::domain::evidence::ProficiencyLevel;

/// Threshold requirements for achieving a proficiency level.
#[derive(Debug, Clone)]
pub struct LevelThreshold {
    pub level: ProficiencyLevel,
    /// Minimum number of evidence records required.
    pub min_evidence: usize,
    /// Minimum weighted confidence score (0.0 to 1.0).
    pub min_confidence: f64,
    /// If set, at least one evidence record must come from an
    /// assessment of this type (e.g., "project" for Create level).
    pub requires_type: Option<&'static str>,
}

/// Default thresholds per the Alexandria whitepaper.
pub const THRESHOLDS: &[LevelThreshold] = &[
    LevelThreshold {
        level: ProficiencyLevel::Remember,
        min_evidence: 1,
        min_confidence: 0.60,
        requires_type: None,
    },
    LevelThreshold {
        level: ProficiencyLevel::Understand,
        min_evidence: 2,
        min_confidence: 0.65,
        requires_type: None,
    },
    LevelThreshold {
        level: ProficiencyLevel::Apply,
        min_evidence: 2,
        min_confidence: 0.70,
        requires_type: None,
    },
    LevelThreshold {
        level: ProficiencyLevel::Analyze,
        min_evidence: 3,
        min_confidence: 0.75,
        requires_type: None,
    },
    LevelThreshold {
        level: ProficiencyLevel::Evaluate,
        min_evidence: 3,
        min_confidence: 0.80,
        requires_type: None,
    },
    LevelThreshold {
        level: ProficiencyLevel::Create,
        min_evidence: 1,
        min_confidence: 0.80,
        requires_type: Some("project"),
    },
];

/// Get the threshold for a specific proficiency level.
pub fn threshold_for(level: ProficiencyLevel) -> Option<&'static LevelThreshold> {
    THRESHOLDS.iter().find(|t| t.level == level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_cover_all_levels() {
        for level in ProficiencyLevel::ALL {
            assert!(
                threshold_for(*level).is_some(),
                "missing threshold for {:?}",
                level
            );
        }
    }

    #[test]
    fn thresholds_are_in_ascending_order() {
        for pair in THRESHOLDS.windows(2) {
            assert!(
                pair[0].level < pair[1].level,
                "thresholds not in ascending order: {:?} >= {:?}",
                pair[0].level,
                pair[1].level
            );
        }
    }

    #[test]
    fn confidence_increases_with_level() {
        for pair in THRESHOLDS.windows(2) {
            assert!(
                pair[0].min_confidence <= pair[1].min_confidence,
                "confidence should increase: {} > {} for {:?} -> {:?}",
                pair[0].min_confidence,
                pair[1].min_confidence,
                pair[0].level,
                pair[1].level
            );
        }
    }

    #[test]
    fn create_requires_project() {
        let create = threshold_for(ProficiencyLevel::Create).unwrap();
        assert_eq!(create.requires_type, Some("project"));
    }
}
