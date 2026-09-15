//! §14 + §26 — weighted mean, saturating confidence, level mapping.
//! Reproduces the protocol's own worked example (§26): Q≈0.846, C≈0.514,
//! T≈0.435, L=5.

use super::common::{test_did, TEST_NOW};
use app_lib::aggregation::{aggregate_skill_state, AggregationConfig, AggregationInput};
use app_lib::domain::vc::CredentialType;

fn spec_26_evidence() -> Vec<AggregationInput> {
    let t = TEST_NOW.to_string();
    vec![
        AggregationInput {
            credential_id: "ev1".into(),
            issuer: test_did("uni"),
            credential_type: CredentialType::FormalCredential,
            raw_score: 0.90,
            issuance_time: t.clone(),
            expiration_time: None,
            rubric_completeness: 0.92,
            proctoring_reliability: 0.90,
            evidence_traceability: 0.95,
            self_issued: false,
        },
        AggregationInput {
            credential_id: "ev2".into(),
            issuer: test_did("bootcamp"),
            credential_type: CredentialType::AssessmentCredential,
            raw_score: 0.78,
            issuance_time: t.clone(),
            expiration_time: None,
            rubric_completeness: 0.88,
            proctoring_reliability: 0.85,
            evidence_traceability: 0.90,
            self_issued: false,
        },
        AggregationInput {
            credential_id: "ev3".into(),
            issuer: test_did("peer"),
            credential_type: CredentialType::AttestationCredential,
            raw_score: 0.80,
            issuance_time: t,
            expiration_time: None,
            rubric_completeness: 0.70,
            proctoring_reliability: 0.50,
            evidence_traceability: 0.80,
            self_issued: false,
        },
    ]
}

#[tokio::test]
async fn worked_example_26_reproduces_raw_score() {
    let state = aggregate_skill_state(
        &test_did("alice"),
        "skill:logistics.network_optimization",
        &spec_26_evidence(),
        TEST_NOW,
        &AggregationConfig::default(),
    );
    assert!(
        (state.raw_score - 0.846).abs() < 0.05,
        "Q ≈ 0.846, got {}",
        state.raw_score
    );
}

#[tokio::test]
async fn worked_example_26_reproduces_confidence() {
    let state = aggregate_skill_state(
        &test_did("alice"),
        "skill:logistics.network_optimization",
        &spec_26_evidence(),
        TEST_NOW,
        &AggregationConfig::default(),
    );
    assert!(
        (state.confidence - 0.514).abs() < 0.05,
        "C ≈ 0.514, got {}",
        state.confidence
    );
}

#[tokio::test]
async fn worked_example_26_yields_level_5() {
    let state = aggregate_skill_state(
        &test_did("alice"),
        "skill:logistics.network_optimization",
        &spec_26_evidence(),
        TEST_NOW,
        &AggregationConfig::default(),
    );
    assert_eq!(state.level, 5);
    // Config version 1.1 added provenance-weighted quality; 1.2 stopped
    // self-issued claims counting as independent issuer clusters (a rule
    // change requires a new calculation_version). The worked example is
    // unaffected: its evidence carries no provenance and no self-issued
    // claims, so quality resolves to (1,1,1) and the derived level holds.
    assert_eq!(state.calculation_version, "1.2");
}

#[tokio::test]
async fn trust_score_equals_q_times_c() {
    let state = aggregate_skill_state(
        &test_did("alice"),
        "skill:logistics.network_optimization",
        &spec_26_evidence(),
        TEST_NOW,
        &AggregationConfig::default(),
    );
    let expected = state.raw_score * state.confidence;
    assert!((state.trust_score - expected).abs() < 1e-6);
}
