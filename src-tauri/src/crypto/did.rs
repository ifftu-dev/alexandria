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
