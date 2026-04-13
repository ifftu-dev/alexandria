//! `did:key` identities for the VC-first credential model.
//!
//! Stub scaffolding introduced in the TDD phase of the VC migration.
//! Every function `unimplemented!()` until PR 3 (DID layer) lands it.
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

/// Derive a `did:key:z...` identifier from an Ed25519 signing key.
///
/// The DID embeds the public key via multibase-encoded multicodec
/// (multicodec 0xed for Ed25519), following the `did:key` spec.
/// Implementation lands in PR 3.
pub fn derive_did_key(_signing_key: &SigningKey) -> Did {
    unimplemented!("PR 3 — DID layer")
}

/// Parse a `did:key:...` string into a `Did`. Returns an error if the
/// string isn't a valid `did:key` identifier.
pub fn parse_did_key(_s: &str) -> Result<Did, DidError> {
    unimplemented!("PR 3 — DID layer")
}

/// Resolve a `Did` to the Ed25519 verifying key embedded in the
/// identifier. Does not touch the database — `did:key` is
/// self-resolving by construction.
pub fn resolve_did_key(_did: &Did) -> Result<VerifyingKey, DidError> {
    unimplemented!("PR 3 — DID layer")
}

/// Look up the `KeyRegistryEntry` valid at a specific point in time.
/// Used by verification when a credential was signed under a key that
/// has since been rotated — we still need to verify it under the
/// historical key.
pub fn resolve_key_at(
    _db: &rusqlite::Connection,
    _did: &Did,
    _at: &str,
) -> Result<Option<KeyRegistryEntry>, DidError> {
    unimplemented!("PR 3 — DID layer")
}

/// Rotate the signer's current key. Records a new `KeyRegistryEntry`
/// with `valid_from = now`, closes the previous entry's `valid_until`.
pub fn rotate_key(
    _db: &rusqlite::Connection,
    _current: &Did,
    _new_signing_key: &SigningKey,
) -> Result<KeyRegistryEntry, DidError> {
    unimplemented!("PR 3 — DID layer")
}

#[derive(Debug, thiserror::Error)]
pub enum DidError {
    #[error("invalid did:key format: {0}")]
    InvalidFormat(String),
    #[error("unsupported DID method")]
    UnsupportedMethod,
    #[error("db error: {0}")]
    Db(#[from] rusqlite::Error),
}

// ---------------------------------------------------------------------------
// Unit tests (VC migration — PR 2 scaffolding).
//
// Every test here is `#[ignore]`'d with a pointer to the implementation PR
// that will un-ignore it. These complement `tests/e2e_vc/did.rs` by pinning
// function-level behaviour of each primitive in isolation — determinism,
// multibase prefix shape, registry row structure, idempotency — whereas the
// e2e suite covers whole user journeys.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
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

    const TEST_NOW: &str = "2026-04-13T00:00:00Z";

    #[test]
    #[ignore = "pending PR 3 — DID layer"]
    fn derive_did_key_produces_did_key_multibase_prefix() {
        // did:key with Ed25519 + multicodec 0xed is always encoded as
        // multibase base58btc, which starts with `z`. Anything else
        // signals a codec/encoding mismatch.
        let did = derive_did_key(&key("alice"));
        assert!(did.as_str().starts_with("did:key:z"));
    }

    #[test]
    #[ignore = "pending PR 3 — DID layer"]
    fn derive_did_key_is_deterministic() {
        let k = key("alice");
        assert_eq!(derive_did_key(&k), derive_did_key(&k));
    }

    #[test]
    #[ignore = "pending PR 3 — DID layer"]
    fn derive_did_key_differs_per_signing_key() {
        assert_ne!(derive_did_key(&key("alice")), derive_did_key(&key("bob")));
    }

    #[test]
    #[ignore = "pending PR 3 — DID layer"]
    fn parse_did_key_accepts_derived_identifier() {
        let did = derive_did_key(&key("alice"));
        let parsed = parse_did_key(did.as_str()).expect("round-trip");
        assert_eq!(parsed, did);
    }

    #[test]
    #[ignore = "pending PR 3 — DID layer"]
    fn parse_did_key_rejects_unsupported_method() {
        match parse_did_key("did:ethr:0xabc") {
            Err(DidError::UnsupportedMethod) => {}
            other => panic!("expected UnsupportedMethod, got {:?}", other),
        }
    }

    #[test]
    #[ignore = "pending PR 3 — DID layer"]
    fn parse_did_key_rejects_malformed_input() {
        assert!(matches!(
            parse_did_key("not-a-did"),
            Err(DidError::InvalidFormat(_))
        ));
    }

    #[test]
    #[ignore = "pending PR 3 — DID layer"]
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

    #[test]
    #[ignore = "pending PR 3 — DID layer"]
    fn rotate_key_writes_new_registry_entry() {
        // Rotation inserts a new `KeyRegistryEntry` with `valid_from =
        // now` and closes out the previous entry's `valid_until`.
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrate");
        let did = derive_did_key(&key("issuer"));
        let entry = rotate_key(db.conn(), &did, &key("issuer-v2")).expect("rotate");
        assert_eq!(entry.did, did);
        assert!(entry.valid_until.is_none());
    }

    #[test]
    #[ignore = "pending PR 3 — DID layer"]
    fn resolve_key_at_returns_historical_key_after_rotation() {
        // Spec §5.3: a VC signed under key_v1 must still verify at t_v
        // even after the issuer has rotated to key_v2.
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrate");
        let did = derive_did_key(&key("issuer"));
        rotate_key(db.conn(), &did, &key("issuer-v2")).expect("rotate");
        let historical = resolve_key_at(db.conn(), &did, TEST_NOW)
            .expect("lookup")
            .expect("expected historical entry");
        assert_eq!(historical.did, did);
    }

    #[test]
    #[ignore = "pending PR 3 — DID layer"]
    fn resolve_key_at_unknown_did_returns_none() {
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrate");
        let missing = Did("did:key:zUnknown".into());
        assert!(resolve_key_at(db.conn(), &missing, TEST_NOW)
            .expect("lookup")
            .is_none());
    }
}
