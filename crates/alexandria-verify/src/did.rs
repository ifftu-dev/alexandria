//! `did:key` identities for the VC-first credential model.
//!
//! Implements `did:key` per the W3C draft
//! (<https://w3c-ccg.github.io/did-method-key/>) for Ed25519 keys:
//!
//! ```text
//! did:key:z<base58btc(multicodec-varint(0xed) ++ raw_public_key)>
//! ```
//!
//! Ed25519 uses multicodec 0xed → varint-encoded as `[0xed, 0x01]` — a
//! 32-byte public key therefore produces a 34-byte codec payload, which
//! yields the canonical 48-character `z6Mk…`-prefixed base58btc string.
//!
//! Spec references: Alexandria Credential & Reputation Protocol v1
//! §4.1 (Subject identified by DID), §5.1 (MUST support at least one
//! DID method — we ship `did:key`), §5.3 (key rotation).

use ed25519_dalek::{SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

/// A decentralized identifier. For v1, always a `did:key:z...`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Did(pub String);

impl Did {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reference to a specific verification method inside a DID document
/// (e.g. `did:key:z...#key-1`). Used by VC `proof.verificationMethod`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationMethodRef(pub String);

/// A row in the key registry. Captures the (did, verifying_key) binding
/// along with a validity window so historical signatures can still be
/// verified after rotation (spec §5.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRegistryEntry {
    pub did: Did,
    pub key_id: String,
    pub public_key_bytes: Vec<u8>,
    pub valid_from: String,
    pub valid_until: Option<String>,
    pub rotated_by: Option<String>,
}

/// A minimal DID document. Full DID Core compliance is deferred.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DidDoc {
    pub id: Did,
    pub verification_methods: Vec<KeyRegistryEntry>,
}

/// Ed25519 public-key multicodec varint: 0xed encoded as `[0xed, 0x01]`.
/// See <https://github.com/multiformats/multicodec/blob/master/table.csv>.
const ED25519_MULTICODEC: [u8; 2] = [0xed, 0x01];

/// The `did:key:` scheme prefix.
const DID_KEY_PREFIX: &str = "did:key:";

/// Multibase base58btc prefix (lowercase 'z').
const MULTIBASE_BASE58BTC: char = 'z';

/// Derive a `did:key:z...` identifier from an Ed25519 signing key.
///
/// Deterministic: the same key always yields the same DID string.
pub fn derive_did_key(signing_key: &SigningKey) -> Did {
    did_from_verifying_key(&signing_key.verifying_key())
}

/// Derive a `did:key:z...` identifier from an Ed25519 verifying key.
pub fn did_from_verifying_key(vk: &VerifyingKey) -> Did {
    let mut payload = Vec::with_capacity(ED25519_MULTICODEC.len() + 32);
    payload.extend_from_slice(&ED25519_MULTICODEC);
    payload.extend_from_slice(vk.as_bytes());
    let mut out = String::with_capacity(DID_KEY_PREFIX.len() + 1 + 48);
    out.push_str(DID_KEY_PREFIX);
    out.push(MULTIBASE_BASE58BTC);
    out.push_str(&bs58::encode(&payload).into_string());
    Did(out)
}

/// Deterministic "course authority" signing key for a course's author.
///
/// A learner completing a course doesn't hold the instructor's key, so the
/// instructor [`AttestationCredential`](crate::vc::CredentialType)
/// issued on completion is signed with this stable per-author keypair,
/// derived from the course `author_address`. Domain-separated so it can't
/// collide with any other derived key; deterministic so the same author
/// always maps to the same issuer DID (repeated completions don't spawn
/// fresh issuer clusters in the aggregator).
pub fn course_authority_key(author_address: &str) -> SigningKey {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"alexandria:course-authority:v1\x00");
    hasher.update(author_address.as_bytes());
    let seed: [u8; 32] = *hasher.finalize().as_bytes();
    SigningKey::from_bytes(&seed)
}

/// The `did:key` of a course's authority — the issuer of its instructor
/// attestations. See [`course_authority_key`].
pub fn course_authority_did(author_address: &str) -> Did {
    derive_did_key(&course_authority_key(author_address))
}

/// Parse a `did:key:...` string into a `Did`, validating the method,
/// multibase prefix, multicodec header and key length.
pub fn parse_did_key(s: &str) -> Result<Did, DidError> {
    // Method check first — `did:<method>:<id>`.
    let rest = s
        .strip_prefix("did:")
        .ok_or_else(|| DidError::InvalidFormat(format!("not a DID: {s}")))?;
    let (method, id) = rest
        .split_once(':')
        .ok_or_else(|| DidError::InvalidFormat(format!("no method identifier in {s}")))?;
    if method != "key" {
        return Err(DidError::UnsupportedMethod);
    }
    let _ = decode_did_key_id(id)?;
    Ok(Did(s.to_string()))
}

/// Resolve a `Did` to the Ed25519 verifying key embedded in the
/// identifier. Does not touch the database — `did:key` is
/// self-resolving by construction.
pub fn resolve_did_key(did: &Did) -> Result<VerifyingKey, DidError> {
    let rest = did
        .as_str()
        .strip_prefix(DID_KEY_PREFIX)
        .ok_or(DidError::UnsupportedMethod)?;
    let pk_bytes = decode_did_key_id(rest)?;
    VerifyingKey::from_bytes(&pk_bytes)
        .map_err(|e| DidError::InvalidFormat(format!("bad ed25519 key: {e}")))
}

/// Decode the method-specific identifier portion of a `did:key` string
/// (the part after `did:key:`) into its raw 32-byte Ed25519 public key.
fn decode_did_key_id(id: &str) -> Result<[u8; 32], DidError> {
    let mut chars = id.chars();
    let prefix = chars
        .next()
        .ok_or_else(|| DidError::InvalidFormat("did:key identifier is empty".into()))?;
    if prefix != MULTIBASE_BASE58BTC {
        return Err(DidError::InvalidFormat(format!(
            "unsupported multibase prefix '{prefix}' — only base58btc ('z') is supported"
        )));
    }
    let bytes = bs58::decode(chars.as_str())
        .into_vec()
        .map_err(|e| DidError::InvalidFormat(format!("bs58 decode failed: {e}")))?;
    if bytes.len() != ED25519_MULTICODEC.len() + 32 {
        return Err(DidError::InvalidFormat(format!(
            "expected {} decoded bytes, got {}",
            ED25519_MULTICODEC.len() + 32,
            bytes.len()
        )));
    }
    if bytes[..ED25519_MULTICODEC.len()] != ED25519_MULTICODEC {
        return Err(DidError::InvalidFormat(
            "multicodec header is not ed25519-pub (0xed 0x01)".into(),
        ));
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&bytes[ED25519_MULTICODEC.len()..]);
    Ok(pk)
}

#[derive(Debug, thiserror::Error)]
pub enum DidError {
    #[error("invalid did:key format: {0}")]
    InvalidFormat(String),
    #[error("unsupported DID method")]
    UnsupportedMethod,
}

// ---------------------------------------------------------------------------
// Unit tests.
//
// Complements `tests/e2e_vc/did.rs` by pinning function-level behaviour of
// each primitive in isolation — determinism, multibase prefix shape, codec
// header, registry row structure — whereas the e2e suite covers whole user
// journeys across DB + verification.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer;

    /// Deterministic signing key derived from a role label. Mirrors
    /// `tests/e2e_vc/common.rs::test_key` but duplicated here because
    /// integration-test helpers aren't visible to `#[cfg(test)]` code in
    /// `src/`.
    fn key(role: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let b = role.as_bytes();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = b[i % b.len().max(1)];
        }
        SigningKey::from_bytes(&bytes)
    }

    #[test]
    fn derive_did_key_produces_did_key_multibase_prefix() {
        // did:key with Ed25519 + multicodec 0xed is always encoded as
        // multibase base58btc, which starts with `z`. Anything else
        // signals a codec/encoding mismatch.
        let did = derive_did_key(&key("alice"));
        assert!(did.as_str().starts_with("did:key:z"));
    }

    #[test]
    fn derive_did_key_is_deterministic() {
        let k = key("alice");
        assert_eq!(derive_did_key(&k), derive_did_key(&k));
    }

    #[test]
    fn derive_did_key_differs_per_signing_key() {
        assert_ne!(derive_did_key(&key("alice")), derive_did_key(&key("bob")));
    }

    #[test]
    fn parse_did_key_accepts_derived_identifier() {
        let did = derive_did_key(&key("alice"));
        let parsed = parse_did_key(did.as_str()).expect("round-trip");
        assert_eq!(parsed, did);
    }

    #[test]
    fn parse_did_key_rejects_unsupported_method() {
        match parse_did_key("did:ethr:0xabc") {
            Err(DidError::UnsupportedMethod) => {}
            other => panic!("expected UnsupportedMethod, got {:?}", other),
        }
    }

    #[test]
    fn parse_did_key_rejects_malformed_input() {
        assert!(matches!(
            parse_did_key("not-a-did"),
            Err(DidError::InvalidFormat(_))
        ));
    }

    #[test]
    fn parse_did_key_rejects_non_z_multibase_prefix() {
        // We only support base58btc ('z'). A different multibase prefix
        // indicates an incompatible encoding even if the raw bytes
        // would decode — reject eagerly so verifiers don't accept a
        // differently-encoded key as "the same" DID.
        assert!(matches!(
            parse_did_key("did:key:xABC"),
            Err(DidError::InvalidFormat(_))
        ));
    }

    #[test]
    fn parse_did_key_rejects_wrong_multicodec() {
        // Fabricate a `did:key:z...` that base58-decodes to a valid but
        // wrong-codec payload (e.g. secp256k1 prefix 0xe7 0x01 + 32
        // arbitrary bytes). Must be rejected as invalid format.
        let mut payload = vec![0xe7, 0x01];
        payload.extend_from_slice(&[7u8; 32]);
        let s = format!("did:key:z{}", bs58::encode(&payload).into_string());
        assert!(matches!(parse_did_key(&s), Err(DidError::InvalidFormat(_))));
    }

    #[test]
    fn resolve_did_key_round_trips_with_sign_verify() {
        // did:key is self-resolving: the public key is embedded in the
        // identifier, so a signature made with `key` must verify under
        // the `VerifyingKey` recovered from `resolve_did_key`.
        let k = key("carol");
        let did = derive_did_key(&k);
        let vk = resolve_did_key(&did).expect("resolve");
        let sig = k.sign(b"payload");
        assert!(vk.verify_strict(b"payload", &sig).is_ok());
    }
}
