//! Integrity snapshot commitment chain.
//!
//! Each snapshot is folded into a running hash, so the persisted flag
//! stream is order-fixed and tamper-evident on the device. The chain is
//! local evidence only: anchoring and committee co-signing have no
//! verified path, so a credential's achieved assurance stays `local`.

use crate::crypto::hash::blake2b_256;

/// Domain separation tag — bumped if the fold format changes.
const COMMIT_TAG: &str = "alexandria-integrity-commit-v1";

/// Fold one snapshot into the running commitment chain.
///
/// `prev` is the previous commitment hex (`""` for the genesis snapshot);
/// `snapshot_canonical` is the deterministic bytes of the snapshot the
/// caller already sends to the backend (flags + scores). Returns the new
/// running hash as hex. Chaining makes the stream order-fixed and
/// tamper-evident: changing or reordering any snapshot changes the root.
pub fn fold_commitment(prev: &str, snapshot_canonical: &[u8]) -> String {
    let mut buf =
        Vec::with_capacity(COMMIT_TAG.len() + 1 + prev.len() + 1 + snapshot_canonical.len());
    buf.extend_from_slice(COMMIT_TAG.as_bytes());
    buf.push(b'|');
    buf.extend_from_slice(prev.as_bytes());
    buf.push(b'|');
    buf.extend_from_slice(snapshot_canonical);
    hex::encode(blake2b_256(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commitment_chain_is_order_sensitive() {
        let a = fold_commitment("", b"snap-a");
        let ab = fold_commitment(&a, b"snap-b");
        let b = fold_commitment("", b"snap-b");
        let ba = fold_commitment(&b, b"snap-a");
        assert_ne!(ab, ba, "reordering snapshots must change the root");
        // Deterministic.
        assert_eq!(
            ab,
            fold_commitment(&fold_commitment("", b"snap-a"), b"snap-b")
        );
    }
}
