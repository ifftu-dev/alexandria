//! Per-evidence weight components (§14.3–§14.8).

use crate::crypto::did::Did;
use crate::domain::vc::CredentialType;

use super::{AggregationConfig, AggregationInput};

/// Issuer trust prior (§14.4). Looks up `config.issuer_weights[did]`,
/// falls back to `default_issuer_weight`. Clamped to `[0, 1]`.
pub fn issuer_weight(issuer: &Did, config: &AggregationConfig) -> f64 {
    let w = config
        .issuer_weights
        .get(issuer.as_str())
        .copied()
        .unwrap_or(config.default_issuer_weight);
    w.clamp(0.0, 1.0)
}

/// Credential-class weight (§14.5, §25.1). Returns 0 for any variant
/// not in the config — a verifier policy that wants to consume a
/// novel type must supply a weight.
pub fn type_weight(credential_type: CredentialType, config: &AggregationConfig) -> f64 {
    config
        .type_weights
        .get(&credential_type)
        .copied()
        .unwrap_or(0.0)
        .max(0.0)
}

/// Freshness (§14.6): `w_fresh = e^{-λ · Δt_years}`.
///
/// `Δt` is the positive difference between `verification_time` and
/// `evidence.issuance_time`, in years. If either timestamp fails to
/// parse we return 1.0 (treat as fresh) — a verifier policy layer
/// can choose stricter parsing, but at this layer we'd rather not
/// silently discount evidence over a clock-format quibble.
pub fn freshness_weight(
    evidence: &AggregationInput,
    skill_id: &str,
    verification_time: &str,
    config: &AggregationConfig,
) -> f64 {
    let delta_years = years_between(&evidence.issuance_time, verification_time).unwrap_or(0.0);
    // Negative delta (credential dated in the future) ⇒ treat as 0 —
    // we're not going to give future-dated credentials >1.0 weight.
    let delta = delta_years.max(0.0);
    let lambda = config
        .skill_decay
        .get(skill_id)
        .copied()
        .unwrap_or(config.default_decay);
    (-lambda * delta).exp()
}

/// Quality (§14.7): linear combination of rubric, proctoring,
/// traceability with α_r + α_a + α_x = 1 enforced by config.
pub fn quality_weight(evidence: &AggregationInput, config: &AggregationConfig) -> f64 {
    let r = evidence.rubric_completeness.clamp(0.0, 1.0);
    let a = evidence.proctoring_reliability.clamp(0.0, 1.0);
    let x = evidence.evidence_traceability.clamp(0.0, 1.0);
    (config.quality_rubric_alpha * r
        + config.quality_proctoring_alpha * a
        + config.quality_traceability_alpha * x)
        .clamp(0.0, 1.0)
}

/// Independence (§14.8): `w_ind = 1 / (1 + Σ ρ_ij)`.
///
/// For v1 we use a simple dependence heuristic: `ρ_ij = 1.0` if
/// evidence items `i` and `j` share the same issuer DID, else 0.
/// PR 7's `aggregation::independence` module replaces this with a
/// cluster-aware `pairwise_dependence` fed from the DB.
pub fn independence_weight(
    evidence: &AggregationInput,
    peers: &[AggregationInput],
    _config: &AggregationConfig,
) -> f64 {
    // Count co-issuer peers. We skip self via pointer equality (so
    // when the caller passes the full evidence slice including the
    // current item, we don't count it against itself) — callers that
    // already exclude self are unaffected.
    let co_issuer_count = peers
        .iter()
        .filter(|p| !std::ptr::eq(*p, evidence) && p.issuer == evidence.issuer)
        .count() as f64;
    1.0 / (1.0 + co_issuer_count)
}

/// Parse two ISO 8601 strings and return `(to - from)` in fractional
/// years. Returns `None` if either fails to parse.
fn years_between(from: &str, to: &str) -> Option<f64> {
    let a = chrono::DateTime::parse_from_rfc3339(from).ok()?;
    let b = chrono::DateTime::parse_from_rfc3339(to).ok()?;
    let secs = (b - a).num_seconds() as f64;
    Some(secs / (365.25 * 86_400.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_NOW: &str = "2026-04-13T00:00:00Z";

    fn sample(
        credential_type: CredentialType,
        issuance: &str,
        rubric: f64,
        proctor: f64,
        trace: f64,
    ) -> AggregationInput {
        AggregationInput {
            credential_id: "ev".into(),
            issuer: Did("did:key:zPeer".into()),
            credential_type,
            raw_score: 0.5,
            issuance_time: issuance.into(),
            expiration_time: None,
            rubric_completeness: rubric,
            proctoring_reliability: proctor,
            evidence_traceability: trace,
        }
    }

    #[test]
    fn type_weight_orders_formal_over_attestation() {
        // Spec §25.1: FormalCredential > AttestationCredential.
        let cfg = AggregationConfig::default();
        let wf = type_weight(CredentialType::FormalCredential, &cfg);
        let wa = type_weight(CredentialType::AttestationCredential, &cfg);
        assert!(wf > wa);
    }

    #[test]
    fn type_weight_formal_credential_is_one() {
        let cfg = AggregationConfig::default();
        assert!((type_weight(CredentialType::FormalCredential, &cfg) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn freshness_weight_is_one_at_issuance() {
        // Δt = 0 ⇒ e^{-λ·0} = 1.
        let cfg = AggregationConfig::default();
        let ev = sample(CredentialType::FormalCredential, TEST_NOW, 1.0, 1.0, 1.0);
        let w = freshness_weight(&ev, "skill_x", TEST_NOW, &cfg);
        assert!((w - 1.0).abs() < 1e-9);
    }

    #[test]
    fn freshness_weight_decays_with_age() {
        // Older issuance ⇒ lower weight, monotonically.
        let cfg = AggregationConfig::default();
        let young = sample(
            CredentialType::FormalCredential,
            "2026-01-01T00:00:00Z",
            1.0,
            1.0,
            1.0,
        );
        let old = sample(
            CredentialType::FormalCredential,
            "2020-01-01T00:00:00Z",
            1.0,
            1.0,
            1.0,
        );
        let wy = freshness_weight(&young, "skill_x", TEST_NOW, &cfg);
        let wo = freshness_weight(&old, "skill_x", TEST_NOW, &cfg);
        assert!(wo < wy);
    }

    #[test]
    fn quality_weight_uses_spec_14_7_mixture() {
        // Spec §14.7: w_quality = α_r·r + α_a·a + α_x·x.
        // With defaults (0.4, 0.3, 0.3) and r=a=x=1.0 ⇒ 1.0.
        let cfg = AggregationConfig::default();
        let ev = sample(CredentialType::FormalCredential, TEST_NOW, 1.0, 1.0, 1.0);
        let w = quality_weight(&ev, &cfg);
        assert!((w - 1.0).abs() < 1e-9);
    }

    #[test]
    fn quality_weight_is_zero_for_no_evidence() {
        let cfg = AggregationConfig::default();
        let ev = sample(CredentialType::FormalCredential, TEST_NOW, 0.0, 0.0, 0.0);
        let w = quality_weight(&ev, &cfg);
        assert!(w.abs() < 1e-9);
    }

    #[test]
    fn independence_weight_is_one_with_no_peers() {
        // Σρ = 0 ⇒ 1/(1+0) = 1.
        let cfg = AggregationConfig::default();
        let ev = sample(CredentialType::FormalCredential, TEST_NOW, 1.0, 1.0, 1.0);
        let w = independence_weight(&ev, &[], &cfg);
        assert!((w - 1.0).abs() < 1e-9);
    }

    #[test]
    fn independence_weight_shrinks_with_correlated_peers() {
        // Adding correlated peers must monotonically reduce the weight.
        let cfg = AggregationConfig::default();
        let ev = sample(CredentialType::FormalCredential, TEST_NOW, 1.0, 1.0, 1.0);
        let alone = independence_weight(&ev, &[], &cfg);
        let crowd = independence_weight(&ev, &[ev.clone(), ev.clone()], &cfg);
        assert!(crowd < alone);
    }

    #[test]
    fn issuer_weight_is_in_unit_interval() {
        let cfg = AggregationConfig::default();
        let w = issuer_weight(&Did("did:key:zSomeIssuer".into()), &cfg);
        assert!((0.0..=1.0).contains(&w));
    }
}
