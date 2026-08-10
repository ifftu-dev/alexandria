//! Verifiable Credentials.
//!
//! The credential types, canonicalization, signing and verification all live
//! in the `alexandria-verify` crate, which is deliberately I/O-free so that a
//! server — or a third party writing their own verifier — can link it without
//! linking this application. This module re-exports that surface so call sites
//! here are unchanged, and supplies the piece the crate cannot: an
//! implementation of [`VerificationStore`] over the app's SQLite database.

pub use alexandria_verify::vc::*;
pub use alexandria_verify::{NullStore, VerificationStore};

use alexandria_verify::did::{Did, KeyRegistryEntry};
use rusqlite::Connection;

/// [`VerificationStore`] backed by the local encrypted database.
///
/// Every method swallows its errors into `None`/`false`, which is the contract
/// the trait defines: verification cannot distinguish "no" from "don't know"
/// and must not reject on the difference. A failed read here means a credential
/// is treated as not-revoked and not-suspended, which is the conservative
/// direction — the same behaviour these lookups had when they were inline SQL
/// inside `verify_credential`.
pub struct SqliteVerificationStore<'a>(pub &'a Connection);

impl VerificationStore for SqliteVerificationStore<'_> {
    fn key_at(&self, did: &Did, at: &str) -> Option<KeyRegistryEntry> {
        crate::crypto::key_registry::resolve_key_at(self.0, did, at)
            .ok()
            .flatten()
    }

    fn status_list_bits(&self, list_id: &str) -> Option<Vec<u8>> {
        self.0
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE list_id = ?1",
                rusqlite::params![list_id],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .ok()
    }

    fn suspension(&self, credential_id: &str) -> Option<(bool, Option<String>)> {
        self.0
            .query_row(
                "SELECT suspended, suspended_until FROM credentials WHERE id = ?1",
                rusqlite::params![credential_id],
                |r| Ok((r.get::<_, i64>(0)? != 0, r.get::<_, Option<String>>(1)?)),
            )
            .ok()
    }

    fn is_superseded(&self, credential_id: &str) -> bool {
        let count: i64 = self
            .0
            .query_row(
                "SELECT COUNT(*) FROM credentials WHERE supersedes = ?1",
                rusqlite::params![credential_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        count > 0
    }
}

/// Verify a credential against the local database.
///
/// Thin convenience over [`alexandria_verify::vc::verify::verify_credential`]
/// for the many call sites that already hold a `&Connection`.
pub fn verify_credential_db(
    db: &Connection,
    credential: &VerifiableCredential,
    verification_time: &str,
    policy: &VerificationPolicy,
) -> VerificationResult {
    verify::verify_credential(
        &SqliteVerificationStore(db),
        credential,
        verification_time,
        policy,
    )
}
