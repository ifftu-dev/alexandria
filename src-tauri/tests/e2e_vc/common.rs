//! Shared test harness for the e2e VC scenarios. Stubs — PR 1 scaffolds
//! these; the real implementations land as dependency PRs arrive.

use app_lib::crypto::did::Did;
use app_lib::db::Database;
use ed25519_dalek::SigningKey;
use std::path::PathBuf;

/// A deterministic signing key per role for reproducible test fixtures.
pub fn test_key(role: &str) -> SigningKey {
    let mut bytes = [0u8; 32];
    let b = role.as_bytes();
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = b[i % b.len().max(1)];
    }
    SigningKey::from_bytes(&bytes)
}

/// Spin up a fresh database in a unique temp path with all migrations
/// applied. Integration tests don't see `#[cfg(test)]` items in the
/// library (`Database::open_in_memory`), so we use a tempfile.
pub fn new_test_db() -> Database {
    let dir = std::env::temp_dir().join(format!("alexandria-vc-e2e-{}", uuid_like()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path: PathBuf = dir.join("test.db");
    let db = Database::open(&path).expect("open db");
    db.run_migrations().expect("apply migrations");
    db
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos}-{}", std::process::id())
}

/// Convenience: derive a `did:key` from a role-keyed signing key.
/// Panics if invoked before PR 3 lands `derive_did_key`.
pub fn test_did(role: &str) -> Did {
    let key = test_key(role);
    app_lib::crypto::did::derive_did_key(&key)
}

/// Deterministic ISO-8601 timestamp for snapshot tests.
pub const TEST_NOW: &str = "2026-04-13T00:00:00Z";
