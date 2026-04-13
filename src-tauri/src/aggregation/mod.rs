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
/// `DerivedSkillState`. Implementation in PR 6.
pub fn aggregate_skill_state(
    _subject: &Did,
    _skill_id: &str,
    _evidence: &[AggregationInput],
    _verification_time: &str,
    _config: &AggregationConfig,
) -> DerivedSkillState {
    unimplemented!("PR 6 — aggregation engine")
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
    #[ignore = "pending PR 6 — aggregation engine"]
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
    #[ignore = "pending PR 6 — aggregation engine"]
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
    #[ignore = "pending PR 6 — aggregation engine"]
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
    #[ignore = "pending PR 6 — aggregation engine"]
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
