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
