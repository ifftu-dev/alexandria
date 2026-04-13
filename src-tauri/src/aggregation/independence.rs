//! Issuer clustering + independence matrix (§14.8, §15.1). Stub — PR 7.

use crate::crypto::did::Did;

/// Group issuers into independence clusters. Two issuers are in the
/// same cluster if they share DAO membership, stake prefix, or
/// transitive delegation. Returns a mapping from issuer DID to cluster ID.
pub fn cluster_issuers(
    _db: &rusqlite::Connection,
    _issuers: &[Did],
) -> std::collections::HashMap<Did, String> {
    unimplemented!("PR 7")
}

/// Pairwise dependence estimate ρ_ij ∈ [0, 1] for two issuers.
pub fn pairwise_dependence(_a: &Did, _b: &Did, _db: &rusqlite::Connection) -> f64 {
    unimplemented!("PR 7")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    #[ignore = "pending PR 7 — issuer clustering"]
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
    #[ignore = "pending PR 7 — issuer clustering"]
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
    #[ignore = "pending PR 7 — issuer clustering"]
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
    #[ignore = "pending PR 7 — issuer clustering"]
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
