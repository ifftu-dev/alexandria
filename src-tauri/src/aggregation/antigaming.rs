//! Sybil / inflation / self-assertion / re-issuance controls (§15). Stub — PR 7.

use crate::crypto::did::Did;

use super::AggregationConfig;

/// Compute the issuer-level inflation z-score against a global mean.
pub fn inflation_z_score(_issuer: &Did, _db: &rusqlite::Connection, _assessment_type: &str) -> f64 {
    unimplemented!("PR 7")
}

/// Apply the inflation penalty p_I (§15.3) given the issuer's z-score.
pub fn inflation_penalty(_z: f64, _config: &AggregationConfig) -> f64 {
    unimplemented!("PR 7")
}

/// Cap the contribution of a cluster at `kappa_cluster` (§15.1).
pub fn apply_cluster_cap(_cluster_weight_sum: f64, _config: &AggregationConfig) -> f64 {
    unimplemented!("PR 7")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    #[ignore = "pending PR 7 — anti-gaming"]
    fn cluster_cap_is_identity_below_threshold() {
        // Sums below κ_cluster must pass through unchanged.
        let cfg = AggregationConfig::default();
        let w = 0.5 * cfg.kappa_cluster;
        let out = apply_cluster_cap(w, &cfg);
        assert!((out - w).abs() < 1e-9);
    }

    #[test]
    #[ignore = "pending PR 7 — anti-gaming"]
    fn cluster_cap_saturates_at_kappa() {
        // Spec §15.1: W_cluster = min(Σw_i, κ_cluster). Huge sums must
        // clamp down, preventing Sybil clusters from linearly scaling.
        let cfg = AggregationConfig::default();
        let capped = apply_cluster_cap(1000.0, &cfg);
        assert!(capped <= cfg.kappa_cluster + 1e-9);
        assert!((capped - cfg.kappa_cluster).abs() < 1e-9);
    }

    #[test]
    #[ignore = "pending PR 7 — anti-gaming"]
    fn inflation_penalty_is_one_at_or_below_zmax() {
        // §15.3: z ≤ z_max ⇒ p_I = 1.
        let cfg = AggregationConfig::default();
        assert!((inflation_penalty(cfg.z_max, &cfg) - 1.0).abs() < 1e-9);
        assert!((inflation_penalty(0.0, &cfg) - 1.0).abs() < 1e-9);
        assert!((inflation_penalty(-3.0, &cfg) - 1.0).abs() < 1e-9);
    }

    #[test]
    #[ignore = "pending PR 7 — anti-gaming"]
    fn inflation_penalty_decreases_above_zmax() {
        // §15.3: z > z_max ⇒ p_I = e^{-η(z − z_max)}, strictly decreasing.
        let cfg = AggregationConfig::default();
        let p1 = inflation_penalty(cfg.z_max + 0.5, &cfg);
        let p2 = inflation_penalty(cfg.z_max + 1.0, &cfg);
        let p3 = inflation_penalty(cfg.z_max + 2.0, &cfg);
        assert!(p1 < 1.0);
        assert!(p2 < p1);
        assert!(p3 < p2);
        // Always bounded below by 0.
        assert!(p3 > 0.0);
    }

    #[test]
    #[ignore = "pending PR 7 — anti-gaming"]
    fn inflation_z_score_for_unknown_issuer_is_finite() {
        // No observations yet ⇒ implementations should return a finite
        // z (e.g., 0) rather than NaN so the penalty formula is total.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let z = inflation_z_score(&Did("did:key:zUnseen".into()), db.conn(), "exam");
        assert!(z.is_finite());
    }
}
