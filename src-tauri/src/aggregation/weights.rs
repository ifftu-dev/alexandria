//! Per-evidence weight components (§14.3–§14.8). Stubs — PR 6.

use crate::crypto::did::Did;
use crate::domain::vc::CredentialType;

use super::{AggregationConfig, AggregationInput};

pub fn issuer_weight(_issuer: &Did, _config: &AggregationConfig) -> f64 {
    unimplemented!("PR 6")
}

pub fn type_weight(_credential_type: CredentialType, _config: &AggregationConfig) -> f64 {
    unimplemented!("PR 6")
}

pub fn freshness_weight(
    _evidence: &AggregationInput,
    _skill_id: &str,
    _verification_time: &str,
    _config: &AggregationConfig,
) -> f64 {
    unimplemented!("PR 6")
}

pub fn quality_weight(_evidence: &AggregationInput, _config: &AggregationConfig) -> f64 {
    unimplemented!("PR 6")
}

pub fn independence_weight(
    _evidence: &AggregationInput,
    _peers: &[AggregationInput],
    _config: &AggregationConfig,
) -> f64 {
    unimplemented!("PR 6")
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
    #[ignore = "pending PR 6 — aggregation engine"]
    fn type_weight_orders_formal_over_attestation() {
        // Spec §25.1: FormalCredential > AttestationCredential.
        let cfg = AggregationConfig::default();
        let wf = type_weight(CredentialType::FormalCredential, &cfg);
        let wa = type_weight(CredentialType::AttestationCredential, &cfg);
        assert!(wf > wa);
    }

    #[test]
    #[ignore = "pending PR 6 — aggregation engine"]
    fn type_weight_formal_credential_is_one() {
        let cfg = AggregationConfig::default();
        assert!((type_weight(CredentialType::FormalCredential, &cfg) - 1.0).abs() < 1e-9);
    }

    #[test]
    #[ignore = "pending PR 6 — aggregation engine"]
    fn freshness_weight_is_one_at_issuance() {
        // Δt = 0 ⇒ e^{-λ·0} = 1.
        let cfg = AggregationConfig::default();
        let ev = sample(CredentialType::FormalCredential, TEST_NOW, 1.0, 1.0, 1.0);
        let w = freshness_weight(&ev, "skill_x", TEST_NOW, &cfg);
        assert!((w - 1.0).abs() < 1e-9);
    }

    #[test]
    #[ignore = "pending PR 6 — aggregation engine"]
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
    #[ignore = "pending PR 6 — aggregation engine"]
    fn quality_weight_uses_spec_14_7_mixture() {
        // Spec §14.7: w_quality = α_r·r + α_a·a + α_x·x.
        // With defaults (0.4, 0.3, 0.3) and r=a=x=1.0 ⇒ 1.0.
        let cfg = AggregationConfig::default();
        let ev = sample(CredentialType::FormalCredential, TEST_NOW, 1.0, 1.0, 1.0);
        let w = quality_weight(&ev, &cfg);
        assert!((w - 1.0).abs() < 1e-9);
    }

    #[test]
    #[ignore = "pending PR 6 — aggregation engine"]
    fn quality_weight_is_zero_for_no_evidence() {
        let cfg = AggregationConfig::default();
        let ev = sample(CredentialType::FormalCredential, TEST_NOW, 0.0, 0.0, 0.0);
        let w = quality_weight(&ev, &cfg);
        assert!(w.abs() < 1e-9);
    }

    #[test]
    #[ignore = "pending PR 6 — aggregation engine"]
    fn independence_weight_is_one_with_no_peers() {
        // Σρ = 0 ⇒ 1/(1+0) = 1.
        let cfg = AggregationConfig::default();
        let ev = sample(CredentialType::FormalCredential, TEST_NOW, 1.0, 1.0, 1.0);
        let w = independence_weight(&ev, &[], &cfg);
        assert!((w - 1.0).abs() < 1e-9);
    }

    #[test]
    #[ignore = "pending PR 6 — aggregation engine"]
    fn independence_weight_shrinks_with_correlated_peers() {
        // Adding correlated peers must monotonically reduce the weight.
        let cfg = AggregationConfig::default();
        let ev = sample(CredentialType::FormalCredential, TEST_NOW, 1.0, 1.0, 1.0);
        let alone = independence_weight(&ev, &[], &cfg);
        let crowd = independence_weight(&ev, &[ev.clone(), ev.clone()], &cfg);
        assert!(crowd < alone);
    }

    #[test]
    #[ignore = "pending PR 6 — aggregation engine"]
    fn issuer_weight_is_in_unit_interval() {
        let cfg = AggregationConfig::default();
        let w = issuer_weight(&Did("did:key:zSomeIssuer".into()), &cfg);
        assert!((0.0..=1.0).contains(&w));
    }
}
