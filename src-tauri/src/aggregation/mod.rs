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
