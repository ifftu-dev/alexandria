//! Explicit device-pairing primitives for cross-device sync.
//!
//! Two devices belonging to the same user establish a 32-byte
//! symmetric `shared_key` by exchanging a **pairing code**. The
//! device initiating the pair generates the key and emits a code
//! carrying its dial information plus the key; the accepting device
//! decodes it, stores the key, and dials back to complete a two-way
//! handshake.
//!
//! Every subsequent sync payload between the pair is sealed with
//! AES-256-GCM under `shared_key` (see [`crate::crypto::content_crypto`]),
//! so the pairing secret — not merely a shared stake address — is what
//! authorises data exchange. The code is transported out-of-band
//! (shown on-screen / scanned as a QR), so it never traverses the
//! network in the clear.
//!
//! The code itself is `base64url(json(PairingCode))`. It embeds the
//! `shared_key`, so anyone who sees the code can pair — treat it like
//! a one-time secret and keep the validity window short.

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::crypto::hash::blake2b_256;

/// Everything the accepting device needs to pair with and dial the
/// initiating device. Serialised into the pairing code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingCode {
    /// Initiator's libp2p PeerId (base58).
    pub peer_id: String,
    /// Initiator's dialable multiaddresses.
    pub addresses: Vec<String>,
    /// The 32-byte symmetric key both devices will seal sync with.
    pub shared_key: [u8; 32],
    /// Owning identity's stake address — the acceptor refuses the code
    /// unless it matches its own (defence against pairing across users).
    pub stake_address: String,
    /// Initiator's stable device id (`devices.id`).
    pub device_id: String,
    /// Initiator's device label, for display on the acceptor.
    pub device_name: Option<String>,
    /// Initiator's platform (`macos` / `windows` / ...).
    pub platform: String,
}

/// Generate a fresh 32-byte pairing/sync key from the OS CSPRNG.
pub fn generate_shared_key() -> [u8; 32] {
    use rand::RngCore;
    let mut key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    key
}

/// Encode a [`PairingCode`] into its transportable string form.
pub fn encode(code: &PairingCode) -> Result<String, String> {
    let json = serde_json::to_vec(code).map_err(|e| format!("encode pairing code: {e}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json))
}

/// Decode a pairing code string back into a [`PairingCode`].
pub fn decode(code: &str) -> Result<PairingCode, String> {
    let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(code.trim())
        .map_err(|e| format!("decode pairing code: {e}"))?;
    serde_json::from_slice(&json).map_err(|e| format!("parse pairing code: {e}"))
}

/// Stable hash of a pairing code string. Stored instead of the raw
/// code so the secret never lands in the database in the clear.
pub fn code_hash(code: &str) -> String {
    hex::encode(blake2b_256(code.trim().as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PairingCode {
        PairingCode {
            peer_id: "12D3KooWExample".into(),
            addresses: vec!["/ip4/192.168.1.2/tcp/4001".into()],
            shared_key: [7u8; 32],
            stake_address: "stake_test1uxyz".into(),
            device_id: "dev-123".into(),
            device_name: Some("My Mac".into()),
            platform: "macos".into(),
        }
    }

    #[test]
    fn encode_decode_round_trip() {
        let code = sample();
        let s = encode(&code).unwrap();
        let back = decode(&s).unwrap();
        assert_eq!(code, back);
    }

    #[test]
    fn generated_keys_are_distinct() {
        let a = generate_shared_key();
        let b = generate_shared_key();
        assert_ne!(a, b, "two fresh keys must differ");
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn code_hash_is_stable_and_ignores_whitespace() {
        let s = encode(&sample()).unwrap();
        assert_eq!(code_hash(&s), code_hash(&format!("  {s}\n")));
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode("!!!not-base64!!!").is_err());
        assert!(decode("YWJj").is_err()); // valid base64, not a PairingCode
    }
}
