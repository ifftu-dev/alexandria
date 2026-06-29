//! Automated integrity attestation — pure core (§ Integrity→VC bridge, P1).
//!
//! High-assurance integrity is *layered* and fully automated — no human
//! ever hand-signs a session:
//!
//!   * **Anchored (baseline)** — the client commits a running hash of
//!     the snapshot stream during the session. The chained commitment
//!     fixes the order and contents of the flags, and the final
//!     `commitment_root` is anchored (DHT/chain) so it is timestamped
//!     and cannot be rebuilt after the learner has seen the questions
//!     or result. Proves *timing + immutability*.
//!   * **High-assurance (upgrade)** — committee-operated attestor nodes
//!     auto-counter-sign the live commitment stream over P2P. Collecting
//!     a supermajority of committee co-signatures over the
//!     `commitment_root` proves an *independent party witnessed the
//!     session live*, so a compromised client cannot fabricate it.
//!
//! This module is pure: it computes the commitment chain, the canonical
//! attestation payload, the M-of-N committee threshold, and resolves the
//! assurance ladder. Persistence, the anchor submission, the P2P
//! co-sign protocol, and the attestor node live in later increments.

use std::collections::HashSet;

use crate::crypto::hash::blake2b_256;
use crate::crypto::signing::{verify, SignedMessage};

/// Domain separation tag — bumped if the payload format changes so old
/// signatures can never be replayed against a new schema.
const PAYLOAD_TAG: &str = "alexandria-integrity-attestation-v1";
const COMMIT_TAG: &str = "alexandria-integrity-commit-v1";

/// The automated assurance ladder embedded in a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssuranceLevel {
    /// Device-reported only. A determined attacker could suppress flags.
    Local,
    /// Snapshot commitment chain anchored (timestamp + immutability).
    Anchored,
    /// Anchored AND a committee supermajority co-signed the live stream.
    HighAssurance,
}

impl AssuranceLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            AssuranceLevel::Local => "local",
            AssuranceLevel::Anchored => "anchored",
            AssuranceLevel::HighAssurance => "high_assurance",
        }
    }

    pub fn from_label(s: &str) -> Self {
        match s {
            "anchored" => AssuranceLevel::Anchored,
            "high_assurance" => AssuranceLevel::HighAssurance,
            // Unknown / "local" → the safe floor. Never upgrade on skew.
            _ => AssuranceLevel::Local,
        }
    }

    /// Rank for monotonic comparison (e.g. policy "at least anchored").
    pub fn rank(self) -> u8 {
        match self {
            AssuranceLevel::Local => 0,
            AssuranceLevel::Anchored => 1,
            AssuranceLevel::HighAssurance => 2,
        }
    }
}

/// One committee co-signature over the attestation payload. `public_key`
/// and `signature` are hex; `attestor_address` is the signer's Cardano
/// stake address (committee membership is keyed on it).
#[derive(Debug, Clone)]
pub struct CoSignature {
    pub attestor_address: String,
    pub public_key_hex: String,
    pub signature_hex: String,
}

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

/// Canonical bytes an attestor (anchor or co-signer) commits to. Binds
/// the terminal session state to its commitment root so a signature
/// can't be lifted onto a different outcome. Single source of truth for
/// both producing and verifying signatures.
#[allow(clippy::too_many_arguments)]
pub fn attestation_payload(
    session_id: &str,
    status: &str,
    integrity_score: Option<f64>,
    critical_count: i64,
    warning_count: i64,
    commitment_root: &str,
    ended_at: &str,
) -> Vec<u8> {
    // Format the score deterministically — fixed precision, explicit
    // "none" so signer and verifier agree bit-for-bit.
    let score = match integrity_score {
        Some(s) => format!("{s:.6}"),
        None => "none".to_string(),
    };
    format!(
        "{PAYLOAD_TAG}|{session_id}|{status}|{score}|{critical_count}|{warning_count}|{commitment_root}|{ended_at}"
    )
    .into_bytes()
}

/// Supermajority threshold for a committee of `n`: ceil(2n/3), min 1.
/// Matches the protocol's 2/3 governance convention.
pub fn required_threshold(committee_size: usize) -> usize {
    if committee_size == 0 {
        return usize::MAX; // impossible to meet — no committee, no high-assurance
    }
    (2 * committee_size).div_ceil(3).max(1)
}

/// Count distinct committee members whose co-signature over `payload`
/// verifies. Non-committee signers, duplicate signers, malformed hex,
/// and bad signatures are all ignored.
pub fn count_valid_committee_cosigs(
    payload: &[u8],
    cosigs: &[CoSignature],
    committee_addresses: &HashSet<String>,
) -> usize {
    let mut counted: HashSet<&str> = HashSet::new();
    for c in cosigs {
        if !committee_addresses.contains(&c.attestor_address) {
            continue;
        }
        if counted.contains(c.attestor_address.as_str()) {
            continue;
        }
        let (Ok(public_key), Ok(signature)) = (
            hex::decode(&c.public_key_hex),
            hex::decode(&c.signature_hex),
        ) else {
            continue;
        };
        let signed = SignedMessage {
            payload: payload.to_vec(),
            signature,
            public_key,
        };
        if verify(&signed).is_ok() {
            counted.insert(c.attestor_address.as_str());
        }
    }
    counted.len()
}

/// Resolve the assurance ladder from the two layers. `anchored` is true
/// once the commitment root has a confirmed anchor; `valid_cosigs` is
/// the count from [`count_valid_committee_cosigs`].
///
/// A committee co-sign supermajority is the strong signal and yields
/// `HighAssurance` (it covers the commitment root regardless of a
/// separate anchor). An anchor alone yields `Anchored`. Otherwise
/// `Local`.
pub fn resolve_assurance(
    anchored: bool,
    valid_cosigs: usize,
    committee_size: usize,
) -> AssuranceLevel {
    if committee_size > 0 && valid_cosigs >= required_threshold(committee_size) {
        AssuranceLevel::HighAssurance
    } else if anchored {
        AssuranceLevel::Anchored
    } else {
        AssuranceLevel::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signing::sign;
    use ed25519_dalek::SigningKey;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn cosign(addr: &str, k: &SigningKey, payload: &[u8]) -> CoSignature {
        let s = sign(payload, k);
        CoSignature {
            attestor_address: addr.to_string(),
            public_key_hex: hex::encode(&s.public_key),
            signature_hex: hex::encode(&s.signature),
        }
    }

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

    #[test]
    fn threshold_is_two_thirds_ceil() {
        assert_eq!(required_threshold(1), 1);
        assert_eq!(required_threshold(3), 2);
        assert_eq!(required_threshold(4), 3);
        assert_eq!(required_threshold(6), 4);
        assert_eq!(required_threshold(0), usize::MAX);
    }

    #[test]
    fn counts_only_valid_committee_signers() {
        let payload = attestation_payload("s1", "completed", Some(0.9), 0, 1, "root", "t");
        let committee: HashSet<String> = ["addr_a", "addr_b", "addr_c"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let good_a = cosign("addr_a", &key(1), &payload);
        let good_b = cosign("addr_b", &key(2), &payload);
        let outsider = cosign("addr_x", &key(3), &payload); // not in committee
        let dup_a = cosign("addr_a", &key(1), &payload); // duplicate signer

        // Tampered: signature over a different payload.
        let other = attestation_payload("s1", "flagged", Some(0.1), 2, 0, "root", "t");
        let mut tampered = cosign("addr_c", &key(4), &other);
        tampered.attestor_address = "addr_c".into();

        let cosigs = vec![good_a, good_b, outsider, dup_a, tampered];
        let n = count_valid_committee_cosigs(&payload, &cosigs, &committee);
        assert_eq!(n, 2, "only addr_a + addr_b count once each");
    }

    #[test]
    fn resolve_assurance_ladder() {
        // committee of 3 → threshold 2
        let c: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(resolve_assurance(false, 0, c.len()), AssuranceLevel::Local);
        assert_eq!(
            resolve_assurance(true, 0, c.len()),
            AssuranceLevel::Anchored
        );
        assert_eq!(
            resolve_assurance(true, 1, c.len()),
            AssuranceLevel::Anchored
        );
        assert_eq!(
            resolve_assurance(true, 2, c.len()),
            AssuranceLevel::HighAssurance
        );
        assert_eq!(
            resolve_assurance(false, 3, c.len()),
            AssuranceLevel::HighAssurance
        );
        // no committee → never high-assurance
        assert_eq!(resolve_assurance(true, 5, 0), AssuranceLevel::Anchored);
    }

    #[test]
    fn level_string_roundtrip_and_floor() {
        for l in [
            AssuranceLevel::Local,
            AssuranceLevel::Anchored,
            AssuranceLevel::HighAssurance,
        ] {
            assert_eq!(AssuranceLevel::from_label(l.as_str()), l);
        }
        // Unknown skews to the safe floor.
        assert_eq!(AssuranceLevel::from_label("bogus"), AssuranceLevel::Local);
    }
}
