//! §15 — Sybil / inflation / self-assertion / re-issuance controls.

use super::common::{new_test_db, test_did};
use app_lib::aggregation::{
    antigaming::{apply_cluster_cap, inflation_penalty, inflation_z_score},
    AggregationConfig,
};

#[tokio::test]
#[ignore = "pending PR 7 — anti-gaming"]
async fn cluster_cap_prevents_linear_scaling() {
    // Sum of 10 same-cluster weights must not exceed kappa_cluster.
    let cfg = AggregationConfig::default();
    let capped = apply_cluster_cap(100.0, &cfg);
    assert!(capped <= cfg.kappa_cluster + 1e-6);
}

#[tokio::test]
#[ignore = "pending PR 7 — anti-gaming"]
async fn z_score_below_threshold_applies_no_penalty() {
    let cfg = AggregationConfig::default();
    let p = inflation_penalty(1.0, &cfg); // z below z_max=1.5
    assert!((p - 1.0).abs() < 1e-9);
}

#[tokio::test]
#[ignore = "pending PR 7 — anti-gaming"]
async fn z_score_above_threshold_penalises_monotonically() {
    let cfg = AggregationConfig::default();
    let p1 = inflation_penalty(2.0, &cfg);
    let p2 = inflation_penalty(3.0, &cfg);
    assert!(p2 < p1, "higher z must give stricter penalty");
    assert!(p1 < 1.0, "any z > z_max reduces the weight");
}

#[tokio::test]
#[ignore = "pending PR 7 — anti-gaming"]
async fn inflation_z_score_uses_global_stats() {
    let db = new_test_db();
    let z = inflation_z_score(&test_did("generous-issuer"), db.conn(), "quiz");
    // Shape check only — substantive behaviour lands with PR 7
    assert!(z.is_finite());
}
