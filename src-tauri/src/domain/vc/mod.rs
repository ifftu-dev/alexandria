//! Verifiable Credentials.
//!
//! The credential types, canonicalization, signing and verification all live
//! in the `alexandria-verify` crate, which is deliberately I/O-free so that a
//! server — or a third party writing their own verifier — can link it without
//! linking this application. This module re-exports that surface so call sites
//! here are unchanged, and supplies the piece the crate cannot: an
//! implementation of [`VerificationStore`] over the app's SQLite database.

pub use alexandria_verify::vc::*;
pub use alexandria_verify::{NullStore, StoreLookup, VerificationStore};

use alexandria_verify::did::{Did, KeyRegistryEntry};
use rusqlite::{Connection, OptionalExtension};

/// [`VerificationStore`] backed by the local encrypted database.
///
/// Missing rows and failed reads remain distinct. The shared verifier can then
/// return a pending decision instead of treating unavailable status as clean.
pub struct SqliteVerificationStore<'a>(pub &'a Connection);

impl VerificationStore for SqliteVerificationStore<'_> {
    fn registered_key(&self, did: &Did) -> StoreLookup<KeyRegistryEntry> {
        match crate::crypto::key_registry::registered_key(self.0, did) {
            Ok(Some(entry)) => StoreLookup::Found(entry),
            Ok(None) => StoreLookup::Missing,
            Err(_) => StoreLookup::Unavailable,
        }
    }

    fn status_list_bits(&self, list_id: &str) -> StoreLookup<Vec<u8>> {
        match self
            .0
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE list_id = ?1",
                rusqlite::params![list_id],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()
        {
            Ok(Some(bits)) => StoreLookup::Found(bits),
            Ok(None) => StoreLookup::Missing,
            Err(_) => StoreLookup::Unavailable,
        }
    }

    fn suspension(&self, credential_id: &str) -> StoreLookup<(bool, Option<String>)> {
        match self
            .0
            .query_row(
                "SELECT suspended, suspended_until FROM credentials WHERE id = ?1",
                rusqlite::params![credential_id],
                |r| Ok((r.get::<_, i64>(0)? != 0, r.get::<_, Option<String>>(1)?)),
            )
            .optional()
        {
            Ok(Some(state)) => StoreLookup::Found(state),
            Ok(None) => StoreLookup::Missing,
            Err(_) => StoreLookup::Unavailable,
        }
    }

    fn is_superseded(&self, credential_id: &str) -> StoreLookup<bool> {
        match self.0.query_row(
            "SELECT COUNT(*) FROM credentials WHERE supersedes = ?1",
            rusqlite::params![credential_id],
            |r| r.get::<_, i64>(0),
        ) {
            Ok(count) => StoreLookup::Found(count > 0),
            Err(_) => StoreLookup::Unavailable,
        }
    }

    /// A stored receipt for `credential_hash` (this node's own once its batch
    /// confirmed, or one imported with a bundle) together with the observed
    /// ledger anchor of the transaction it names.
    fn integrity_anchor(&self, credential_hash: &str) -> StoreLookup<anchor::AnchorEvidence> {
        crate::cardano::anchor_queue::evidence(self.0, credential_hash)
    }
}

/// Verify a credential against the local database, checking its device
/// attestation evidence against this build's trust inputs (Apple's root and
/// the network profile's Play Integrity keys).
///
/// Thin convenience over
/// [`alexandria_verify::vc::verify::verify_credential_with_attestation`] for
/// the many call sites that already hold a `&Connection`.
pub fn verify_credential_db(
    db: &Connection,
    credential: &VerifiableCredential,
    verification_time: &str,
    policy: &VerificationPolicy,
) -> VerificationResult {
    verify::verify_credential_with_attestation(
        &SqliteVerificationStore(db),
        credential,
        verification_time,
        policy,
        crate::sentinel::attestation::profile_trust_inputs(),
    )
}

/// The hash stored with a credential and recorded by its anchor:
/// [`anchor::integrity_hash`], BLAKE3 over its JCS-canonical JSON with the
/// proof value cleared.
pub fn integrity_hash_of(vc: &VerifiableCredential) -> Result<String, String> {
    anchor::integrity_hash(vc).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_store_distinguishes_missing_rows_from_failed_queries() {
        let did = Did("did:key:zMissing".into());
        let unavailable = Connection::open_in_memory().unwrap();
        let unavailable = SqliteVerificationStore(&unavailable);
        assert!(matches!(
            unavailable.registered_key(&did),
            StoreLookup::Unavailable
        ));
        assert_eq!(
            unavailable.status_list_bits("missing"),
            StoreLookup::Unavailable
        );
        assert_eq!(unavailable.suspension("missing"), StoreLookup::Unavailable);
        assert_eq!(
            unavailable.is_superseded("missing"),
            StoreLookup::Unavailable
        );

        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let available = SqliteVerificationStore(db.conn());
        assert!(matches!(
            available.registered_key(&did),
            StoreLookup::Missing
        ));
        assert_eq!(available.status_list_bits("missing"), StoreLookup::Missing);
        assert_eq!(available.suspension("missing"), StoreLookup::Missing);
        assert_eq!(
            available.is_superseded("missing"),
            StoreLookup::Found(false)
        );
    }
}
