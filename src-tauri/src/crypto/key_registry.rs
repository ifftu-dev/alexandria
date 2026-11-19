//! The `key_registry` table: the key bound to an identifier that is not
//! self-resolving.
//!
//! A `did:key` is its key and never consults this table: no row can change
//! how one resolves. The registry is only the lookup for DID methods that do
//! not embed their key. It is the database half of `alexandria_verify::did`,
//! kept here because that crate is deliberately I/O-free — see
//! `crates/alexandria-verify/src/lib.rs`. [`registered_key`] is what
//! [`SqliteVerificationStore::registered_key`](crate::domain::vc::SqliteVerificationStore)
//! is implemented over.

use alexandria_verify::did::{Did, KeyRegistryEntry};
use rusqlite::OptionalExtension;

/// Failures reading the key registry.
#[derive(Debug, thiserror::Error)]
pub enum KeyRegistryError {
    #[error("db error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("malformed registry row: {0}")]
    Malformed(String),
}

/// The key registered for `did`, if the registry holds one. `Ok(None)` when
/// it holds none.
pub fn registered_key(
    db: &rusqlite::Connection,
    did: &Did,
) -> Result<Option<KeyRegistryEntry>, KeyRegistryError> {
    let row = db
        .query_row(
            "SELECT key_id, public_key_hex FROM key_registry \
             WHERE did = ?1 ORDER BY key_id LIMIT 1",
            rusqlite::params![did.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((key_id, pk_hex)) = row else {
        return Ok(None);
    };
    let public_key_bytes = hex::decode(&pk_hex)
        .map_err(|e| KeyRegistryError::Malformed(format!("registry pk hex: {e}")))?;
    Ok(Some(KeyRegistryEntry {
        did: did.clone(),
        key_id,
        public_key_bytes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrate");
        db
    }

    const KEY_HEX: &str = "b7ebf265e5f9104e83588f2b4584e336f69470351004bb160f033e84feced80a";

    #[test]
    fn a_registered_key_is_returned_for_its_did() {
        let db = test_db();
        let did = Did("did:web:issuer.example".into());
        db.conn()
            .execute(
                "INSERT INTO key_registry (did, key_id, public_key_hex) VALUES (?1, 'key-1', ?2)",
                rusqlite::params![did.as_str(), KEY_HEX],
            )
            .unwrap();
        let entry = registered_key(db.conn(), &did)
            .unwrap()
            .expect("registered");
        assert_eq!(entry.did, did);
        assert_eq!(entry.public_key_bytes, hex::decode(KEY_HEX).unwrap());
    }

    #[test]
    fn an_unknown_did_has_no_registered_key() {
        let db = test_db();
        let missing = Did("did:web:unknown.example".into());
        assert!(registered_key(db.conn(), &missing).unwrap().is_none());
    }

    /// The retired rotation handler recorded rows with an empty key. A row
    /// must carry a 32-byte Ed25519 key or it cannot be written.
    #[test]
    fn a_row_without_a_key_cannot_be_written() {
        let db = test_db();
        for key in ["", "abcd"] {
            let refused = db.conn().execute(
                "INSERT INTO key_registry (did, key_id, public_key_hex) \
                 VALUES ('did:web:issuer.example', 'key-1', ?1)",
                [key],
            );
            assert!(refused.is_err(), "a row with key {key:?} was written");
        }
    }
}
