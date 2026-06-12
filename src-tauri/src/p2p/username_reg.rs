//! Client side of `/alexandria/username-reg/1.0` — requesting relay
//! receipts for username claims and verifying receipts on resolution.
//!
//! A receipt is the relay's countersignature over (claim sig,
//! first-seen time, relay peer id). It lifts a claim from tier 0
//! (bare, self-asserted time) to tier 1 (relay-attested time) in the
//! deterministic conflict ordering. Receipts are only trusted from
//! the configured relay set ([`super::discovery::relay_peer_ids`]) —
//! the relay's ed25519 PeerId embeds its public key, so verification
//! needs no extra key distribution.

use libp2p::PeerId;
use serde::{Deserialize, Serialize};

use crate::domain::username_claim::{RelayReceipt, UsernameClaim};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptRequest {
    pub claim: UsernameClaim,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReceiptResponse {
    Granted(RelayReceipt),
    Refused {
        reason: String,
        existing_did: Option<String>,
        existing_received_at: Option<i64>,
    },
}

/// Canonical bytes the relay signs — must match the relay's encoding.
fn canonical_receipt_bytes(claim_sig: &str, received_at: i64, relay_peer_id: &str) -> Vec<u8> {
    format!("alexandria-username-receipt-v1|{claim_sig}|{received_at}|{relay_peer_id}").into_bytes()
}

/// Verify a receipt against the trusted relay set. Returns `false`
/// for unknown relays, undecodable peer ids, or bad signatures.
pub fn verify_receipt(claim_sig: &str, receipt: &RelayReceipt) -> bool {
    let Ok(peer_id) = receipt.relay_peer_id.parse::<PeerId>() else {
        return false;
    };
    if !super::discovery::relay_peer_ids().contains(&peer_id) {
        return false;
    }
    // Ed25519 peer ids use an identity multihash — the public key is
    // embedded in the PeerId itself.
    let Ok(public_key) =
        libp2p::identity::PublicKey::try_decode_protobuf(peer_id.as_ref().digest())
    else {
        return false;
    };
    let Ok(sig) = hex::decode(&receipt.sig) else {
        return false;
    };
    public_key.verify(
        &canonical_receipt_bytes(claim_sig, receipt.received_at, &receipt.relay_peer_id),
        &sig,
    )
}

/// Drop an untrusted/invalid receipt from a claim so it cannot
/// inflate the claim's tier during conflict ordering. (Anchors are
/// verified in the Cardano observer instead — phase 3.)
pub fn sanitize_claim(mut claim: UsernameClaim) -> UsernameClaim {
    if let Some(ref receipt) = claim.receipt {
        if !verify_receipt(&claim.sig, receipt) {
            claim.receipt = None;
        }
    }
    claim
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_relay_receipt_is_stripped() {
        let kp = libp2p::identity::Keypair::generate_ed25519();
        let pid = kp.public().to_peer_id().to_string();
        let key = ed25519_dalek::SigningKey::from_bytes(&[1; 32]);
        let did = crate::crypto::did::derive_did_key(&key);
        let mut claim =
            crate::domain::username_claim::UsernameClaim::create("ada_99", &did, 100, &key);
        let bytes = canonical_receipt_bytes(&claim.sig, 200, &pid);
        claim.receipt = Some(RelayReceipt {
            relay_peer_id: pid,
            received_at: 200,
            sig: hex::encode(kp.sign(&bytes).unwrap()),
        });
        // Valid signature, but the relay isn't in the trusted set.
        let sanitized = sanitize_claim(claim);
        assert!(sanitized.receipt.is_none());
    }

    #[test]
    fn garbage_receipt_is_stripped() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[1; 32]);
        let did = crate::crypto::did::derive_did_key(&key);
        let mut claim =
            crate::domain::username_claim::UsernameClaim::create("ada_99", &did, 100, &key);
        claim.receipt = Some(RelayReceipt {
            relay_peer_id: "not-a-peer-id".into(),
            received_at: 200,
            sig: "zz".into(),
        });
        assert!(sanitize_claim(claim).receipt.is_none());
    }
}
