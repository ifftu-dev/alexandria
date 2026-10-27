//! Shared test fixtures: a fixed time, deterministic keys, and a migrated
//! database with an issuer and a subject.

use ed25519_dalek::SigningKey;

use crate::crypto::did::{derive_did_key, Did};
use crate::db::Database;

pub(crate) const NOW: &str = "2026-04-13T00:00:00Z";

pub(crate) fn test_key(role: &str) -> SigningKey {
    let mut bytes = [0u8; 32];
    let b = role.as_bytes();
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = b[i % b.len().max(1)];
    }
    SigningKey::from_bytes(&bytes)
}

pub(crate) fn setup() -> (Database, SigningKey, Did, Did) {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().unwrap();
    let issuer_key = test_key("issuer");
    let issuer = derive_did_key(&issuer_key);
    let subject = derive_did_key(&test_key("subject"));
    (db, issuer_key, issuer, subject)
}
