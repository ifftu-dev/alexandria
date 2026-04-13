//! Versioned aggregation config. Defaults per §25.
//! Changing any of these requires a new `calculation_version`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::domain::vc::CredentialType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationConfig {
    pub version: String,
    pub type_weights: HashMap<CredentialType, f64>,
    /// Per-skill decay constant λ for freshness (§14.6).
    pub skill_decay: HashMap<String, f64>,
    pub default_decay: f64,
    /// Confidence saturation parameters (§14.12, §25.3).
    pub beta: f64,
    pub gamma: f64,
    /// Quality weight mixture coefficients (§14.7).
    pub quality_rubric_alpha: f64,
    pub quality_proctoring_alpha: f64,
    pub quality_traceability_alpha: f64,
    /// Inflation penalty threshold + decay (§15.3, §25.4).
    pub z_max: f64,
    pub eta: f64,
    /// Cluster influence cap (§15.1).
    pub kappa_cluster: f64,
}

impl Default for AggregationConfig {
    fn default() -> Self {
        // Placeholder values — replaced with the §25 defaults in PR 6.
        Self {
            version: "1.0".into(),
            type_weights: HashMap::new(),
            skill_decay: HashMap::new(),
            default_decay: 0.08,
            beta: 0.6,
            gamma: 0.7,
            quality_rubric_alpha: 0.4,
            quality_proctoring_alpha: 0.3,
            quality_traceability_alpha: 0.3,
            z_max: 1.5,
            eta: 0.5,
            kappa_cluster: 3.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_spec_25_confidence_parameters() {
        // Spec §25.3: β = 0.6, γ = 0.7.
        let cfg = AggregationConfig::default();
        assert!((cfg.beta - 0.6).abs() < 1e-9);
        assert!((cfg.gamma - 0.7).abs() < 1e-9);
    }

    #[test]
    fn default_uses_spec_25_inflation_parameters() {
        // Spec §25.4: z_max = 1.5, η = 0.5.
        let cfg = AggregationConfig::default();
        assert!((cfg.z_max - 1.5).abs() < 1e-9);
        assert!((cfg.eta - 0.5).abs() < 1e-9);
    }

    #[test]
    fn default_quality_alphas_sum_to_one() {
        // Spec §14.7: α_r + α_a + α_x = 1. Otherwise the quality weight
        // can exceed 1.0 or collapse to <1.0, silently skewing every
        // aggregated score.
        let cfg = AggregationConfig::default();
        let sum = cfg.quality_rubric_alpha
            + cfg.quality_proctoring_alpha
            + cfg.quality_traceability_alpha;
        assert!((sum - 1.0).abs() < 1e-9, "got {}", sum);
    }

    #[test]
    fn default_version_is_semver_1_dot_x() {
        // Aggregation engine MUST be versioned (§28): historical derived
        // states keep the version that produced them, so bumping the
        // formula does not silently rewrite past interpretations.
        let cfg = AggregationConfig::default();
        assert!(
            cfg.version.starts_with("1."),
            "expected 1.x, got {}",
            cfg.version
        );
    }

    #[test]
    #[ignore = "pending PR 6 — aggregation engine"]
    fn default_populates_type_weights_per_spec_25_1() {
        // §25.1 defaults:
        //   Formal 1.00, Assessment 0.90, Role 0.60,
        //   Attestation 0.35, SelfAssertion 0.25.
        let cfg = AggregationConfig::default();
        assert_eq!(
            cfg.type_weights
                .get(&crate::domain::vc::CredentialType::FormalCredential),
            Some(&1.0)
        );
        assert_eq!(
            cfg.type_weights
                .get(&crate::domain::vc::CredentialType::AssessmentCredential),
            Some(&0.9)
        );
        assert_eq!(
            cfg.type_weights
                .get(&crate::domain::vc::CredentialType::AttestationCredential),
            Some(&0.35)
        );
    }
}
