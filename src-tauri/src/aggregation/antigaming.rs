//! Sybil / inflation / self-assertion / re-issuance controls (§15).

use crate::crypto::did::Did;

use super::AggregationConfig;

/// Compute the issuer-level inflation z-score against a global mean
/// for a given credential_type ("FormalCredential", etc.).
///
/// ```text
///   z_I = (μ_I − μ_G) / σ_G
/// ```
///
/// We read observations from the `credentials` table:
///   - μ_I — mean raw score across this issuer's credentials of the
///     given type (pulled from the stored `signed_vc_json` payload)
///   - μ_G, σ_G — mean and population standard deviation across all
///     issuers' credentials of the same type
///
/// Returns 0.0 (no penalty) when:
///   - fewer than 2 observations for the issuer
///   - fewer than 2 total observations globally
///   - σ_G is zero (no variance, can't z-score)
///
/// These degenerate cases are handled defensively so the penalty
/// function is always total — no `NaN` leakage into weights.
pub fn inflation_z_score(issuer: &Did, db: &rusqlite::Connection, credential_type: &str) -> f64 {
    let issuer_mean = match issuer_stats(db, issuer, credential_type) {
        Ok((n, m)) if n >= 2 => m,
        _ => return 0.0,
    };
    let (global_mean, global_stddev) = match global_stats(db, credential_type) {
        Ok((n, m, s)) if n >= 2 && s > 0.0 => (m, s),
        _ => return 0.0,
    };
    (issuer_mean - global_mean) / global_stddev
}

/// Apply the inflation penalty `p_I` (§15.3).
///
/// ```text
///   p_I = 1                           if z ≤ z_max
///         e^{-η (z − z_max)}          if z >  z_max
/// ```
pub fn inflation_penalty(z: f64, config: &AggregationConfig) -> f64 {
    if !z.is_finite() || z <= config.z_max {
        1.0
    } else {
        (-config.eta * (z - config.z_max)).exp()
    }
}

/// Cap the contribution of a cluster at `κ_cluster` (§15.1).
///
/// ```text
///   W_cluster = min(Σw_i, κ_cluster)
/// ```
pub fn apply_cluster_cap(cluster_weight_sum: f64, config: &AggregationConfig) -> f64 {
    cluster_weight_sum.min(config.kappa_cluster)
}

// ---- internal helpers ---------------------------------------------------

fn issuer_stats(
    db: &rusqlite::Connection,
    issuer: &Did,
    credential_type: &str,
) -> Result<(i64, f64), rusqlite::Error> {
    let rows = collect_scores(
        db,
        "SELECT signed_vc_json FROM credentials \
         WHERE issuer_did = ?1 AND credential_type = ?2",
        rusqlite::params![issuer.as_str(), credential_type],
    )?;
    let n = rows.len() as i64;
    if n == 0 {
        return Ok((0, 0.0));
    }
    let mean = rows.iter().sum::<f64>() / rows.len() as f64;
    Ok((n, mean))
}

fn global_stats(
    db: &rusqlite::Connection,
    credential_type: &str,
) -> Result<(i64, f64, f64), rusqlite::Error> {
    let rows = collect_scores(
        db,
        "SELECT signed_vc_json FROM credentials WHERE credential_type = ?1",
        rusqlite::params![credential_type],
    )?;
    let n = rows.len() as i64;
    if n == 0 {
        return Ok((0, 0.0, 0.0));
    }
    let mean = rows.iter().sum::<f64>() / rows.len() as f64;
    let variance = rows.iter().map(|q| (q - mean).powi(2)).sum::<f64>() / rows.len() as f64;
    Ok((n, mean, variance.sqrt()))
}

fn collect_scores(
    db: &rusqlite::Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<f64>, rusqlite::Error> {
    let mut stmt = db.prepare(sql)?;
    let rows = stmt.query_map(params, |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        let json = r?;
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
            if let Some(s) = v
                .get("credentialSubject")
                .and_then(|cs| cs.get("claim"))
                .and_then(|c| c.get("score"))
                .and_then(|x| x.as_f64())
            {
                out.push(s);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn cluster_cap_is_identity_below_threshold() {
        let cfg = AggregationConfig::default();
        let w = 0.5 * cfg.kappa_cluster;
        let out = apply_cluster_cap(w, &cfg);
        assert!((out - w).abs() < 1e-9);
    }

    #[test]
    fn cluster_cap_saturates_at_kappa() {
        // §15.1: W_cluster = min(Σw_i, κ_cluster).
        let cfg = AggregationConfig::default();
        let capped = apply_cluster_cap(1000.0, &cfg);
        assert!(capped <= cfg.kappa_cluster + 1e-9);
        assert!((capped - cfg.kappa_cluster).abs() < 1e-9);
    }

    #[test]
    fn inflation_penalty_is_one_at_or_below_zmax() {
        let cfg = AggregationConfig::default();
        assert!((inflation_penalty(cfg.z_max, &cfg) - 1.0).abs() < 1e-9);
        assert!((inflation_penalty(0.0, &cfg) - 1.0).abs() < 1e-9);
        assert!((inflation_penalty(-3.0, &cfg) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn inflation_penalty_decreases_above_zmax() {
        let cfg = AggregationConfig::default();
        let p1 = inflation_penalty(cfg.z_max + 0.5, &cfg);
        let p2 = inflation_penalty(cfg.z_max + 1.0, &cfg);
        let p3 = inflation_penalty(cfg.z_max + 2.0, &cfg);
        assert!(p1 < 1.0);
        assert!(p2 < p1);
        assert!(p3 < p2);
        assert!(p3 > 0.0);
    }

    #[test]
    fn inflation_z_score_for_unknown_issuer_is_finite() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let z = inflation_z_score(&Did("did:key:zUnseen".into()), db.conn(), "exam");
        assert!(z.is_finite());
        assert_eq!(z, 0.0);
    }

    #[test]
    fn inflation_z_score_is_zero_when_no_variance() {
        // Single score in the population ⇒ σ_G = 0, so z can't be
        // computed. Return 0 rather than NaN.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        insert_score_row(db.conn(), "c-1", "did:key:zA", "AssessmentCredential", 0.8);
        insert_score_row(db.conn(), "c-2", "did:key:zA", "AssessmentCredential", 0.8);
        let z = inflation_z_score(&Did("did:key:zA".into()), db.conn(), "AssessmentCredential");
        assert_eq!(z, 0.0);
    }

    #[test]
    fn inflation_z_score_flags_consistent_over_scorer() {
        // Issuer A scores consistently high vs peers — z > 0.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        for i in 0..5 {
            insert_score_row(
                db.conn(),
                &format!("a-{i}"),
                "did:key:zA",
                "AssessmentCredential",
                0.95,
            );
        }
        for (i, s) in [0.55, 0.60, 0.65, 0.70, 0.75]
            .into_iter()
            .cycle()
            .take(10)
            .enumerate()
        {
            insert_score_row(
                db.conn(),
                &format!("b-{i}"),
                "did:key:zB",
                "AssessmentCredential",
                s,
            );
        }
        let z = inflation_z_score(&Did("did:key:zA".into()), db.conn(), "AssessmentCredential");
        assert!(z > 1.0, "expected inflation z > 1, got {z}");
    }

    fn insert_score_row(
        conn: &rusqlite::Connection,
        id: &str,
        issuer: &str,
        credential_type: &str,
        score: f64,
    ) {
        let json = serde_json::json!({
            "credentialSubject": {
                "id": "did:key:zSubject",
                "claim": { "kind": "skill", "skillId": "s", "level": 3, "score": score }
            }
        })
        .to_string();
        conn.execute(
            "INSERT INTO credentials \
             (id, issuer_did, subject_did, credential_type, claim_kind, issuance_date, \
              signed_vc_json, integrity_hash) \
             VALUES (?1, ?2, 'did:key:zSubject', ?3, 'skill', '2026-04-13T00:00:00Z', ?4, 'h')",
            rusqlite::params![id, issuer, credential_type, json],
        )
        .unwrap();
    }
}
