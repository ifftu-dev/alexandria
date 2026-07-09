//! Versioned aggregation config. Defaults per §25.
//! Changing any of these requires a new `calculation_version`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::domain::vc::{CredentialType, ProvenanceTier};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationConfig {
    pub version: String,
    pub type_weights: HashMap<CredentialType, f64>,
    /// Quality-factor triples `(rubric, proctoring, traceability)` keyed by
    /// evidence provenance tier (§14.7 inputs). A claim with no provenance
    /// (`None`) uses `(1.0, 1.0, 1.0)` — the original behavior — so existing
    /// credentials score unchanged.
    pub provenance_quality: HashMap<ProvenanceTier, (f64, f64, f64)>,
    /// Per-skill decay constant λ for freshness (§14.6).
    pub skill_decay: HashMap<String, f64>,
    pub default_decay: f64,
    /// Per-issuer trust priors (§14.4). Values ∈ [0, 1]. Missing
    /// issuers fall back to `default_issuer_weight`.
    pub issuer_weights: HashMap<String, f64>,
    pub default_issuer_weight: f64,
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
        // Spec §25 defaults. Type weights per §25.1.
        let mut type_weights = HashMap::new();
        type_weights.insert(CredentialType::FormalCredential, 1.00);
        type_weights.insert(CredentialType::AssessmentCredential, 0.90);
        type_weights.insert(CredentialType::RoleCredential, 0.60);
        type_weights.insert(CredentialType::AttestationCredential, 0.35);
        type_weights.insert(CredentialType::SelfAssertion, 0.25);
        // DerivedCredential is a computed artifact (§6.5) — it MUST
        // NOT be confused with source-issued evidence, so it gets 0
        // by default and must be explicitly weighted by a verifier
        // policy that wants to consume derived states as inputs.
        type_weights.insert(CredentialType::DerivedCredential, 0.00);

        // Provenance quality triples (rubric, proctoring, traceability).
        // Ascending strength: a bare self-declared claim carries little
        // quality signal; an uploaded resume a bit more; an accredited
        // institution's document markedly more; a claim inside an
        // issuer-signed VC is full quality (its issuer/type weight already
        // dominates). All components stay in [0, 1] so the quality weight
        // (α_r·r + α_a·a + α_x·x, α's sum to 1) stays in [0, 1].
        let mut provenance_quality = HashMap::new();
        provenance_quality.insert(ProvenanceTier::SelfDeclared, (0.25, 0.25, 0.25));
        provenance_quality.insert(ProvenanceTier::DocumentBacked, (0.55, 0.40, 0.60));
        provenance_quality.insert(ProvenanceTier::AccreditedDocument, (0.85, 0.60, 0.90));
        provenance_quality.insert(ProvenanceTier::IssuerSigned, (1.00, 1.00, 1.00));

        Self {
            version: "1.1".into(),
            type_weights,
            provenance_quality,
            skill_decay: HashMap::new(),
            default_decay: 0.08,
            issuer_weights: HashMap::new(),
            // 0.8 is a reasonable prior for an unknown issuer in v1 —
            // high enough that a new issuer's evidence isn't dismissed,
            // low enough that verifiers can differentiate recognized
            // institutions upward (e.g., 0.95) via configuration.
            default_issuer_weight: 0.8,
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

impl AggregationConfig {
    /// Resolve the `(rubric, proctoring, traceability)` quality triple for a
    /// claim's provenance tier. `None` (a pre-provenance credential) yields
    /// `(1.0, 1.0, 1.0)` — the original behavior — so existing evidence keeps
    /// its exact score. An unmapped tier also falls back to full quality.
    pub fn quality_triple(&self, provenance: Option<ProvenanceTier>) -> (f64, f64, f64) {
        match provenance {
            None => (1.0, 1.0, 1.0),
            Some(tier) => self
                .provenance_quality
                .get(&tier)
                .copied()
                .unwrap_or((1.0, 1.0, 1.0)),
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
    fn quality_triple_none_reproduces_original_full_quality() {
        // A pre-provenance credential (None) MUST resolve to (1,1,1) so its
        // aggregated score is byte-identical to before provenance existed.
        let cfg = AggregationConfig::default();
        assert_eq!(cfg.quality_triple(None), (1.0, 1.0, 1.0));
    }

    #[test]
    fn quality_weight_is_monotonic_in_provenance_tier() {
        // Stronger provenance ⇒ strictly higher quality weight, so an
        // accredited transcript outranks a bare resume for the same score.
        let cfg = AggregationConfig::default();
        let qw = |t: ProvenanceTier| {
            let (r, a, x) = cfg.quality_triple(Some(t));
            cfg.quality_rubric_alpha * r
                + cfg.quality_proctoring_alpha * a
                + cfg.quality_traceability_alpha * x
        };
        let self_declared = qw(ProvenanceTier::SelfDeclared);
        let doc = qw(ProvenanceTier::DocumentBacked);
        let accredited = qw(ProvenanceTier::AccreditedDocument);
        let issuer = qw(ProvenanceTier::IssuerSigned);
        assert!(
            self_declared < doc && doc < accredited && accredited <= issuer,
            "expected strictly increasing quality: {self_declared} < {doc} < {accredited} <= {issuer}"
        );
        // IssuerSigned = full quality (its issuer/type weight already dominates).
        assert!((issuer - 1.0).abs() < 1e-9);
    }

    #[test]
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
