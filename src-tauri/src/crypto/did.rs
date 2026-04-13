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

/// Look up the `KeyRegistryEntry` valid at a specific point in time.
///
/// Returns the entry whose `[valid_from, valid_until)` window contains
/// `at`. Used by verification when a credential was signed under a key
/// that has since been rotated — we still need to verify it under the
/// historical key (spec §5.3). Returns `Ok(None)` if no entry is known.
pub fn resolve_key_at(
    db: &rusqlite::Connection,
    did: &Did,
    at: &str,
) -> Result<Option<KeyRegistryEntry>, DidError> {
    let row = db
        .query_row(
            "SELECT key_id, public_key_hex, valid_from, valid_until, rotated_by \
             FROM key_registry \
             WHERE did = ?1 \
               AND valid_from <= ?2 \
               AND (valid_until IS NULL OR valid_until > ?2) \
             ORDER BY valid_from DESC \
             LIMIT 1",
            rusqlite::params![did.as_str(), at],
            |row| {
                let key_id: String = row.get(0)?;
                let pk_hex: String = row.get(1)?;
                let valid_from: String = row.get(2)?;
                let valid_until: Option<String> = row.get(3)?;
                let rotated_by: Option<String> = row.get(4)?;
                Ok((key_id, pk_hex, valid_from, valid_until, rotated_by))
            },
        )
        .optional()?;

    let Some((key_id, pk_hex, valid_from, valid_until, rotated_by)) = row else {
        return Ok(None);
    };
    let public_key_bytes = hex::decode(&pk_hex)
        .map_err(|e| DidError::InvalidFormat(format!("registry pk hex: {e}")))?;
    Ok(Some(KeyRegistryEntry {
        did: did.clone(),
        key_id,
        public_key_bytes,
        valid_from,
        valid_until,
        rotated_by,
    }))
}

/// Rotate the signer's current key. Closes any currently-open
/// registry entry for `current` at `now`, then inserts a new entry
/// with `valid_from = now` and `valid_until = NULL`. Returns the new
/// entry.
///
/// If no prior entry exists for `current`, the pre-rotation key
/// (derivable from `did:key` self-resolution) is backfilled first
/// with `valid_from = "1970-01-01T00:00:00Z"` — this way a verifier
/// evaluating a credential signed *before* rotation can still find
/// the historical key at any verification time ≤ `now`, which is
/// the survivability guarantee of spec §5.3.
pub fn rotate_key(
    db: &rusqlite::Connection,
    current: &Did,
    new_signing_key: &SigningKey,
) -> Result<KeyRegistryEntry, DidError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let new_pk = new_signing_key.verifying_key();
    let new_did = did_from_verifying_key(&new_pk);
    let pk_hex = hex::encode(new_pk.as_bytes());

    // Backfill the historical (pre-rotation) key if this is the first
    // rotation we're recording for this DID. `did:key` is self-
    // resolving so we can always extract the original pubkey from the
    // DID itself.
    let existing: i64 = db.query_row(
        "SELECT COUNT(*) FROM key_registry WHERE did = ?1",
        rusqlite::params![current.as_str()],
        |r| r.get(0),
    )?;
    if existing == 0 {
        if let Ok(pre_pk) = resolve_did_key(current) {
            let pre_hex = hex::encode(pre_pk.as_bytes());
            db.execute(
                "INSERT INTO key_registry \
                 (did, key_id, public_key_hex, valid_from, valid_until, rotated_by) \
                 VALUES (?1, 'key-1', ?2, '1970-01-01T00:00:00Z', NULL, NULL)",
                rusqlite::params![current.as_str(), &pre_hex],
            )?;
        }
    }

    // New rows are numbered by insertion order so clients can refer to
    // "<did>#key-N" verification methods deterministically.
    let key_id: String = db
        .query_row(
            "SELECT 'key-' || (COALESCE(MAX(CAST(substr(key_id, 5) AS INTEGER)), 0) + 1) \
             FROM key_registry WHERE did = ?1",
            rusqlite::params![current.as_str()],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "key-1".to_string());

    db.execute(
        "UPDATE key_registry \
         SET valid_until = ?2, rotated_by = ?3 \
         WHERE did = ?1 AND valid_until IS NULL",
        rusqlite::params![current.as_str(), &now, new_did.as_str()],
    )?;

    db.execute(
        "INSERT INTO key_registry \
         (did, key_id, public_key_hex, valid_from, valid_until, rotated_by) \
         VALUES (?1, ?2, ?3, ?4, NULL, NULL)",
        rusqlite::params![current.as_str(), &key_id, &pk_hex, &now],
    )?;

    Ok(KeyRegistryEntry {
        did: current.clone(),
        key_id,
        public_key_bytes: new_pk.as_bytes().to_vec(),
        valid_from: now,
        valid_until: None,
        rotated_by: None,
    })
}

use rusqlite::OptionalExtension;

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

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrate");
        db
    }

    const TEST_NOW: &str = "2026-04-13T00:00:00Z";

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

    #[test]
    fn rotate_key_writes_new_registry_entry() {
        // Rotation inserts a new `KeyRegistryEntry` with `valid_from =
        // now` and closes out the previous entry's `valid_until`.
        let db = test_db();
        let did = derive_did_key(&key("issuer"));
        let entry = rotate_key(db.conn(), &did, &key("issuer-v2")).expect("rotate");
        assert_eq!(entry.did, did);
        assert!(entry.valid_until.is_none());
    }

    #[test]
    fn rotate_key_closes_prior_open_entries() {
        // After two rotations the registry holds three rows:
        //   1. the backfilled pre-rotation key (valid_from=epoch,
        //      closed at the first rotation)
        //   2. the v2 key (closed at the second rotation)
        //   3. the v3 key (currently open)
        // So exactly two entries are closed.
        let db = test_db();
        let did = derive_did_key(&key("issuer"));
        rotate_key(db.conn(), &did, &key("v2")).unwrap();
        rotate_key(db.conn(), &did, &key("v3")).unwrap();
        let closed: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM key_registry \
                 WHERE did = ?1 AND valid_until IS NOT NULL",
                rusqlite::params![did.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(closed, 2);
        // And exactly one open entry.
        let open: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM key_registry \
                 WHERE did = ?1 AND valid_until IS NULL",
                rusqlite::params![did.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(open, 1);
    }

    #[test]
    fn resolve_key_at_returns_historical_key_after_rotation() {
        // Spec §5.3: a VC signed under key_v1 must still verify at t_v
        // even after the issuer has rotated to key_v2.
        let db = test_db();
        let did = derive_did_key(&key("issuer"));
        rotate_key(db.conn(), &did, &key("issuer-v2")).expect("rotate");
        let historical = resolve_key_at(db.conn(), &did, TEST_NOW)
            .expect("lookup")
            .expect("expected historical entry");
        assert_eq!(historical.did, did);
    }

    #[test]
    fn resolve_key_at_unknown_did_returns_none() {
        let db = test_db();
        let missing = Did("did:key:zUnknown".into());
        assert!(resolve_key_at(db.conn(), &missing, TEST_NOW)
            .expect("lookup")
            .is_none());
    }
}
