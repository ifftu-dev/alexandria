//! Issuer clustering + independence matrix (§14.8, §15.1).
//!
//! MVP clustering: each issuer DID is its own cluster, so
//! independence collapses to "same DID ⇒ correlated, different DID
//! ⇒ independent". A richer implementation can replace
//! `cluster_issuers` with DAO-membership / stake-prefix / transitive-
//! delegation analysis without changing the function surface —
//! `pairwise_dependence` always returns a value in `[0, 1]`,
//! symmetric, with self-pair = 0.

use std::collections::HashMap;

use crate::crypto::did::Did;

/// Group issuers into independence clusters. In v1 the cluster id
/// IS the issuer DID — every issuer is its own cluster. When PR 9's
/// governance propagation lands, this can fold DAO membership /
/// stake-prefix signals in without touching callers.
pub fn cluster_issuers(_db: &rusqlite::Connection, issuers: &[Did]) -> HashMap<Did, String> {
    let mut out = HashMap::with_capacity(issuers.len());
    for did in issuers {
        out.insert(did.clone(), did.as_str().to_string());
    }
    out
}

/// Pairwise dependence estimate ρ_ij ∈ [0, 1] for two issuers.
/// Self-pair is defined as 0 so §14.8's
/// `w_ind = 1/(1 + Σ_{j≠i} ρ_ij)` collapses to 1 for a solitary
/// issuer.
pub fn pairwise_dependence(a: &Did, b: &Did, db: &rusqlite::Connection) -> f64 {
    if a == b {
        return 0.0;
    }
    let map = cluster_issuers(db, &[a.clone(), b.clone()]);
    match (map.get(a), map.get(b)) {
        (Some(ca), Some(cb)) if ca == cb => 1.0,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn pairwise_dependence_is_zero_for_self() {
        // By convention the self-pair is 0 — the independence formula
        // (§14.8) sums over j ≠ i, so a self-correlation would break
        // the base case w_independence = 1/(1+0) = 1.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let a = Did("did:key:zA".into());
        let rho = pairwise_dependence(&a, &a, db.conn());
        assert!(rho.abs() < 1e-9);
    }

    #[test]
    fn pairwise_dependence_is_in_unit_interval() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let rho = pairwise_dependence(
            &Did("did:key:zA".into()),
            &Did("did:key:zB".into()),
            db.conn(),
        );
        assert!((0.0..=1.0).contains(&rho));
    }

    #[test]
    fn pairwise_dependence_is_symmetric() {
        // ρ_ab = ρ_ba is required for the off-diagonal sum to be
        // well-defined regardless of enumeration order.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let a = Did("did:key:zA".into());
        let b = Did("did:key:zB".into());
        let ab = pairwise_dependence(&a, &b, db.conn());
        let ba = pairwise_dependence(&b, &a, db.conn());
        assert!((ab - ba).abs() < 1e-9);
    }

    #[test]
    fn cluster_issuers_assigns_every_input() {
        // Each input DID must be represented in the output map; the
        // independence matrix loop can't tolerate missing keys.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let dids = vec![
            Did("did:key:zA".into()),
            Did("did:key:zB".into()),
            Did("did:key:zC".into()),
        ];
        let map = cluster_issuers(db.conn(), &dids);
        for d in &dids {
            assert!(map.contains_key(d), "missing {:?}", d);
        }
    }
}
