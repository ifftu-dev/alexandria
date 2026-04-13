//! Derived skill state aggregation per Alexandria protocol §14.
//!
//! Not to be confused with `evidence::aggregator` — the old
//! `skill_proofs`-based aggregator is retired in PR 6; this new
//! module implements the weighted-mean + saturating-confidence model
//! and emits `DerivedSkillState` objects.

pub mod antigaming;
pub mod config;
pub mod independence;
pub mod level;
pub mod weights;

use serde::{Deserialize, Serialize};

use crate::crypto::did::Did;

pub use config::AggregationConfig;

/// Explainable derived-state output (§16). Every field answers a
/// specific question the consumer might ask: how strong, how sure,
/// how much evidence, how diverse, what sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedSkillState {
    pub subject: Did,
    pub skill_id: String,
    /// Q: weighted mean of evidence scores ∈ [0,1] (§14.10).
    pub raw_score: f64,
    /// C: confidence ∈ [0,1] saturating in evidence mass + issuer diversity (§14.12).
    pub confidence: f64,
    /// T = Q · C (§14.13).
    pub trust_score: f64,
    /// Discrete level 1–5 (§14.14).
    pub level: u8,
    /// M: total weighted evidence mass (§14.11).
    pub evidence_mass: f64,
    /// U: number of independent issuer clusters (§14.12).
    pub unique_issuer_clusters: u32,
    pub active_evidence_count: u32,
    pub calculation_version: String,
    pub sources: Vec<String>,
    pub computed_at: String,
}

/// A single piece of evidence fed into aggregation. Built from an
/// accepted VC (not a raw `evidence_records` row) so only valid,
/// unrevoked, unexpired credentials contribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationInput {
    pub credential_id: String,
    pub issuer: Did,
    pub credential_type: crate::domain::vc::CredentialType,
    pub raw_score: f64,
    pub issuance_time: String,
    pub expiration_time: Option<String>,
    pub rubric_completeness: f64,
    pub proctoring_reliability: f64,
    pub evidence_traceability: f64,
}

/// Run the aggregation pipeline (§22.2) and return an explainable
/// `DerivedSkillState`.
pub fn aggregate_skill_state(
    subject: &Did,
    skill_id: &str,
    evidence: &[AggregationInput],
    verification_time: &str,
    config: &AggregationConfig,
) -> DerivedSkillState {
    use weights::{
        freshness_weight, independence_weight, issuer_weight, quality_weight, type_weight,
    };

    let computed_at = verification_time.to_string();

    if evidence.is_empty() {
        return DerivedSkillState {
            subject: subject.clone(),
            skill_id: skill_id.to_string(),
            raw_score: 0.0,
            confidence: 0.0,
            trust_score: 0.0,
            level: level::map_level(0.0),
            evidence_mass: 0.0,
            unique_issuer_clusters: 0,
            active_evidence_count: 0,
            calculation_version: config.version.clone(),
            sources: vec![],
            computed_at,
        };
    }

    // Per-item weights (§14.3).
    let mut weights_sum = 0.0_f64;
    let mut weighted_score_sum = 0.0_f64;
    let mut sources = Vec::with_capacity(evidence.len());

    for e in evidence {
        let w_issuer = issuer_weight(&e.issuer, config);
        let w_type = type_weight(e.credential_type, config);
        let w_fresh = freshness_weight(e, skill_id, verification_time, config);
        let w_quality = quality_weight(e, config);
        let w_ind = independence_weight(e, evidence, config);
        let w = w_issuer * w_type * w_fresh * w_quality * w_ind;
        weighted_score_sum += w * e.raw_score.clamp(0.0, 1.0);
        weights_sum += w;
        sources.push(e.credential_id.clone());
    }

    // Q: weighted mean (§14.10). If Σw = 0 no active score exists.
    let raw_score = if weights_sum > 0.0 {
        weighted_score_sum / weights_sum
    } else {
        0.0
    };
    let evidence_mass = weights_sum;

    // U: distinct issuer DIDs. PR 7's clustering replaces this with
    // effective cluster count.
    let unique_issuer_clusters = {
        let mut set = std::collections::HashSet::new();
        for e in evidence {
            set.insert(e.issuer.as_str().to_string());
        }
        set.len() as u32
    };

    // C = (1 - e^-β·M)(1 - e^-γ·U) (§14.12).
    let confidence = (1.0 - (-config.beta * evidence_mass).exp())
        * (1.0 - (-config.gamma * unique_issuer_clusters as f64).exp());

    let trust_score = raw_score * confidence;
    let lvl = level::map_level(raw_score);

    DerivedSkillState {
        subject: subject.clone(),
        skill_id: skill_id.to_string(),
        raw_score,
        confidence,
        trust_score,
        level: lvl,
        evidence_mass,
        unique_issuer_clusters,
        active_evidence_count: evidence.len() as u32,
        calculation_version: config.version.clone(),
        sources,
        computed_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::vc::CredentialType;

    const TEST_NOW: &str = "2026-04-13T00:00:00Z";

    fn ev(credential_id: &str, issuer: &str, t: CredentialType, q: f64) -> AggregationInput {
        AggregationInput {
            credential_id: credential_id.into(),
            issuer: Did(format!("did:key:z{issuer}")),
            credential_type: t,
            raw_score: q,
            issuance_time: TEST_NOW.into(),
            expiration_time: None,
            rubric_completeness: 0.9,
            proctoring_reliability: 0.8,
            evidence_traceability: 0.9,
        }
    }

    #[test]
    fn empty_evidence_produces_no_score_state() {
        // Spec §14.10: if Σw = 0 then no active score exists. The
        // explainable output still needs to be well-formed — zero
        // mass, zero confidence, level 1 (floor).
        let cfg = AggregationConfig::default();
        let state = aggregate_skill_state(
            &Did("did:key:zAlice".into()),
            "skill_x",
            &[],
            TEST_NOW,
            &cfg,
        );
        assert_eq!(state.active_evidence_count, 0);
        assert!((state.evidence_mass).abs() < 1e-9);
        assert!((state.confidence).abs() < 1e-9);
        assert!((state.trust_score).abs() < 1e-9);
    }

    #[test]
    fn trust_score_equals_raw_times_confidence() {
        // §14.13: T = Q · C, always.
        let cfg = AggregationConfig::default();
        let state = aggregate_skill_state(
            &Did("did:key:zAlice".into()),
            "skill_x",
            &[
                ev("e1", "Uni", CredentialType::FormalCredential, 0.8),
                ev("e2", "Bootcamp", CredentialType::AssessmentCredential, 0.7),
            ],
            TEST_NOW,
            &cfg,
        );
        let expected = state.raw_score * state.confidence;
        assert!((state.trust_score - expected).abs() < 1e-6);
    }

    #[test]
    fn output_stamps_calculation_version_and_sources() {
        // Spec §16 output MUST be explainable: the version and the
        // source credential IDs are preserved so a verifier can tell
        // which formula produced this state and from which evidence.
        let cfg = AggregationConfig::default();
        let state = aggregate_skill_state(
            &Did("did:key:zAlice".into()),
            "skill_x",
            &[ev("cred-a", "Uni", CredentialType::FormalCredential, 0.9)],
            TEST_NOW,
            &cfg,
        );
        assert_eq!(state.calculation_version, cfg.version);
        assert!(state.sources.iter().any(|s| s == "cred-a"));
    }

    #[test]
    fn more_diverse_issuers_raise_confidence() {
        // §14.12: confidence increases in U_{s,k} — three distinct
        // clusters should beat three endorsements from one cluster,
        // all else equal.
        let cfg = AggregationConfig::default();
        let diverse = aggregate_skill_state(
            &Did("did:key:zAlice".into()),
            "skill_x",
            &[
                ev("e1", "Uni", CredentialType::FormalCredential, 0.8),
                ev("e2", "Bootcamp", CredentialType::FormalCredential, 0.8),
                ev("e3", "DAO", CredentialType::FormalCredential, 0.8),
            ],
            TEST_NOW,
            &cfg,
        );
        let monolithic = aggregate_skill_state(
            &Did("did:key:zAlice".into()),
            "skill_x",
            &[
                ev("e1", "Uni", CredentialType::FormalCredential, 0.8),
                ev("e2", "Uni", CredentialType::FormalCredential, 0.8),
                ev("e3", "Uni", CredentialType::FormalCredential, 0.8),
            ],
            TEST_NOW,
            &cfg,
        );
        assert!(diverse.confidence > monolithic.confidence);
    }
}
