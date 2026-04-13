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
