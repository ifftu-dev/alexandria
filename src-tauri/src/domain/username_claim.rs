//! Signed username claims for the DHT username registry.
//!
//! A claim binds `@username → DID`, signed by the DID's own key —
//! `did:key` embeds the public key, so any node verifies a claim
//! offline with no PKI. Claims live in the Kademlia DHT under
//! `SHA256("alexandria:username:v1:" + username)` and are cached
//! locally in `username_claims`.
//!
//! Conflicting claims order deterministically (strongest first):
//!   1. tier — anchored (Cardano tx) > receipted (relay countersig) > bare
//!   2. time — anchor slot / receipt time / self-asserted `claimed_at`
//!   3. lexicographic DID as the final tiebreak
//!
//! Phase 1 produces bare claims only; `receipt` and `anchor` are
//! carried in the format now so later phases need no migration.

use std::cmp::Ordering;

use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crypto::did::{parse_did_key, resolve_did_key, Did};

pub const CLAIM_VERSION: u32 = 1;

/// Relay countersignature (Phase 2). The relay attests it first saw
/// this claim at `received_at`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelayReceipt {
    pub relay_peer_id: String,
    pub received_at: i64,
    pub sig: String,
}

/// Cardano anchor (Phase 3). The claim hash appears in a metadata tx;
/// the slot is a trustless timestamp.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CardanoAnchor {
    pub tx_hash: String,
    pub slot: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsernameClaim {
    pub version: u32,
    /// Normalized handle: lowercase `[a-z0-9_]{3,32}`.
    pub username: String,
    pub did: String,
    /// Self-asserted claim time (unix seconds). Trust tier: weakest.
    pub claimed_at: i64,
    /// Ed25519 signature (hex) by the DID key over [`canonical_bytes`].
    pub sig: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<RelayReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<CardanoAnchor>,
}

/// The DHT record key for a username.
pub fn dht_key(username: &str) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(b"alexandria:username:v1:");
    h.update(username.as_bytes());
    h.finalize().to_vec()
}

/// Bytes covered by the owner signature. The receipt/anchor are *not*
/// covered — they attest the claim from outside and arrive later.
fn canonical_bytes(username: &str, did: &str, claimed_at: i64) -> Vec<u8> {
    format!("alexandria-username-claim-v1|{username}|{did}|{claimed_at}").into_bytes()
}

impl UsernameClaim {
    /// Create + sign a bare claim with the DID's signing key.
    pub fn create(username: &str, did: &Did, claimed_at: i64, key: &SigningKey) -> Self {
        let sig = key.sign(&canonical_bytes(username, did.as_str(), claimed_at));
        UsernameClaim {
            version: CLAIM_VERSION,
            username: username.to_string(),
            did: did.as_str().to_string(),
            claimed_at,
            sig: hex::encode(sig.to_bytes()),
            receipt: None,
            anchor: None,
        }
    }

    /// Verify the owner signature against the key embedded in the DID.
    pub fn verify(&self) -> Result<(), String> {
        if self.version != CLAIM_VERSION {
            return Err(format!("unsupported claim version {}", self.version));
        }
        let did = parse_did_key(&self.did).map_err(|e| format!("bad did: {e:?}"))?;
        let vk = resolve_did_key(&did).map_err(|e| format!("bad did key: {e:?}"))?;
        let sig_bytes: [u8; 64] = hex::decode(&self.sig)
            .map_err(|e| format!("bad sig hex: {e}"))?
            .try_into()
            .map_err(|_| "bad sig length".to_string())?;
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        vk.verify_strict(
            &canonical_bytes(&self.username, &self.did, self.claimed_at),
            &sig,
        )
        .map_err(|_| "signature verification failed".to_string())
    }

    /// Trust tier: 2 = anchored, 1 = receipted, 0 = bare.
    pub fn tier(&self) -> u8 {
        if self.anchor.is_some() {
            2
        } else if self.receipt.is_some() {
            1
        } else {
            0
        }
    }

    /// The timestamp that counts for ordering, per tier.
    fn effective_time(&self) -> i64 {
        match (self.anchor.as_ref(), self.receipt.as_ref()) {
            (Some(a), _) => a.slot as i64,
            (None, Some(r)) => r.received_at,
            (None, None) => self.claimed_at,
        }
    }

    /// `true` if `self` beats `other` for the same username.
    /// Deterministic across all nodes: tier desc, time asc, DID asc.
    pub fn beats(&self, other: &UsernameClaim) -> bool {
        match self.tier().cmp(&other.tier()) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => match self.effective_time().cmp(&other.effective_time()) {
                Ordering::Less => true,
                Ordering::Greater => false,
                Ordering::Equal => self.did < other.did,
            },
        }
    }
}

/// Pick the winning claim among verified candidates.
pub fn best_claim(claims: Vec<UsernameClaim>) -> Option<UsernameClaim> {
    let mut best: Option<UsernameClaim> = None;
    for c in claims {
        if c.verify().is_err() {
            continue;
        }
        best = match best {
            None => Some(c),
            Some(b) => {
                if c.beats(&b) {
                    Some(c)
                } else {
                    Some(b)
                }
            }
        };
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::derive_did_key;

    fn keypair(seed: u8) -> (SigningKey, Did) {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let did = derive_did_key(&key);
        (key, did)
    }

    #[test]
    fn claim_signs_and_verifies() {
        let (key, did) = keypair(7);
        let c = UsernameClaim::create("ada_99", &did, 1000, &key);
        assert!(c.verify().is_ok());
    }

    #[test]
    fn tampered_claim_fails_verification() {
        let (key, did) = keypair(7);
        let mut c = UsernameClaim::create("ada_99", &did, 1000, &key);
        c.username = "eve_99".into();
        assert!(c.verify().is_err());
        // Binding to a different DID also fails.
        let (_, other_did) = keypair(9);
        let mut c2 = UsernameClaim::create("ada_99", &did, 1000, &key);
        c2.did = other_did.as_str().to_string();
        assert!(c2.verify().is_err());
    }

    #[test]
    fn ordering_earlier_bare_claim_wins() {
        let (k1, d1) = keypair(1);
        let (k2, d2) = keypair(2);
        let a = UsernameClaim::create("x_name", &d1, 100, &k1);
        let b = UsernameClaim::create("x_name", &d2, 200, &k2);
        assert!(a.beats(&b));
        assert!(!b.beats(&a));
        assert_eq!(best_claim(vec![b, a.clone()]), Some(a));
    }

    #[test]
    fn ordering_anchor_beats_receipt_beats_bare() {
        let (k1, d1) = keypair(1);
        let (k2, d2) = keypair(2);
        // Later anchored claim still beats earlier bare claim.
        let mut anchored = UsernameClaim::create("x_name", &d2, 900, &k2);
        anchored.anchor = Some(CardanoAnchor {
            tx_hash: "tx".into(),
            slot: 5000,
        });
        let bare = UsernameClaim::create("x_name", &d1, 100, &k1);
        assert!(anchored.beats(&bare));

        let mut receipted = UsernameClaim::create("x_name", &d1, 100, &k1);
        receipted.receipt = Some(RelayReceipt {
            relay_peer_id: "12D3".into(),
            received_at: 150,
            sig: String::new(),
        });
        assert!(anchored.beats(&receipted));
        assert!(receipted.beats(&bare));
    }

    #[test]
    fn equal_time_tiebreaks_on_did() {
        let (k1, d1) = keypair(1);
        let (k2, d2) = keypair(2);
        let a = UsernameClaim::create("x_name", &d1, 100, &k1);
        let b = UsernameClaim::create("x_name", &d2, 100, &k2);
        let (lo, hi) = if d1.as_str() < d2.as_str() {
            (a, b)
        } else {
            (b, a)
        };
        assert!(lo.beats(&hi));
    }

    #[test]
    fn best_claim_skips_invalid() {
        let (k1, d1) = keypair(1);
        let (k2, d2) = keypair(2);
        let good = UsernameClaim::create("x_name", &d2, 500, &k2);
        // Forged earlier claim: signed by k1 but bound to d2's name slot
        // with a fudged timestamp and broken sig.
        let mut forged = UsernameClaim::create("x_name", &d1, 1, &k1);
        forged.sig = "00".repeat(64);
        assert_eq!(best_claim(vec![forged, good.clone()]), Some(good));
    }
}
