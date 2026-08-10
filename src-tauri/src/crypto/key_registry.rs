//! The `key_registry` table: reading a DID's key at a point in time, and
//! rotating it.
//!
//! These are the database half of `alexandria_verify::did`. They live here
//! rather than in that crate because it is deliberately I/O-free — see
//! `crates/alexandria-verify/src/lib.rs`. `resolve_key_at` is what
//! [`SqliteVerificationStore::key_at`](crate::domain::vc::SqliteVerificationStore)
//! is implemented over.

use alexandria_verify::did::{did_from_verifying_key, resolve_did_key, Did, KeyRegistryEntry};
use ed25519_dalek::SigningKey;
use rusqlite::OptionalExtension;

/// Failures reading or writing the key registry.
#[derive(Debug, thiserror::Error)]
pub enum KeyRegistryError {
    #[error("db error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("malformed registry row: {0}")]
    Malformed(String),
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
) -> Result<Option<KeyRegistryEntry>, KeyRegistryError> {
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
        .map_err(|e| KeyRegistryError::Malformed(format!("registry pk hex: {e}")))?;
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
) -> Result<KeyRegistryEntry, KeyRegistryError> {
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

// ---------------------------------------------------------------------------
// Unit tests. Moved here with the functions they cover when the pure DID
// primitives were extracted into `alexandria-verify`.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use alexandria_verify::did::derive_did_key;

    /// Deterministic signing key derived from a role label.
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
