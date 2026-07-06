//! Guardian invite codes (cross-device parental oversight).
//!
//! A gated minor generates an invite carrying their dial info plus a
//! fresh 32-byte shared key — the same shape as device pairing
//! ([`super::pairing`]) but deliberately CROSS-USER: the accepting
//! parent has a different identity, so there is no stake-address
//! match. Possession of the (out-of-band) code is what authorises the
//! link; every subsequent guardian exchange is sealed under the key.
//!
//! Invites live longer than pairing codes (days, not minutes): the
//! parent may be remote and offline — the *code* waits, not a
//! connection. It is still a one-time secret; the child side consumes
//! it on first use.

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::crypto::hash::blake2b_256;

/// Everything the parent's device needs to dial the child and complete
/// the guardian link. Serialised into the invite code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardianInvite {
    /// Child's did:key.
    pub child_did: String,
    /// Child's stake address (for display / record only — no match check).
    pub child_stake_address: String,
    /// Child's libp2p PeerId (base58).
    pub child_peer_id: String,
    /// Child's dialable multiaddresses.
    pub addresses: Vec<String>,
    /// The 32-byte symmetric key sealing every guardian exchange.
    pub shared_key: [u8; 32],
    /// Child's display name, for the parent's confirmation screen.
    pub display_name: Option<String>,
}

/// Encode a [`GuardianInvite`] into its transportable string form.
pub fn encode(invite: &GuardianInvite) -> Result<String, String> {
    let json = serde_json::to_vec(invite).map_err(|e| format!("encode guardian invite: {e}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json))
}

/// Decode an invite code string back into a [`GuardianInvite`].
pub fn decode(code: &str) -> Result<GuardianInvite, String> {
    let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(code.trim())
        .map_err(|e| format!("decode guardian invite: {e}"))?;
    serde_json::from_slice(&json).map_err(|e| format!("parse guardian invite: {e}"))
}

/// Stable hash of an invite code string — stored instead of the raw
/// code so the secret never lands in the database in the clear.
pub fn code_hash(code: &str) -> String {
    hex::encode(blake2b_256(code.trim().as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> GuardianInvite {
        GuardianInvite {
            child_did: "did:key:zChild".into(),
            child_stake_address: "stake_test1uchild".into(),
            child_peer_id: "12D3KooWChild".into(),
            addresses: vec!["/ip4/192.168.1.9/tcp/4001".into()],
            shared_key: [11u8; 32],
            display_name: Some("Ada".into()),
        }
    }

    #[test]
    fn encode_decode_round_trip() {
        let invite = sample();
        let s = encode(&invite).unwrap();
        assert_eq!(decode(&s).unwrap(), invite);
    }

    #[test]
    fn code_hash_is_stable_and_ignores_whitespace() {
        let s = encode(&sample()).unwrap();
        assert_eq!(code_hash(&s), code_hash(&format!(" {s}\n")));
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode("!!!nope!!!").is_err());
        assert!(decode("YWJj").is_err());
    }
}
