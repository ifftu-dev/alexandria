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
use alexandria_verify::vc::anchor::IntegrityAnchor;
use rusqlite::{Connection, OptionalExtension};

/// [`VerificationStore`] backed by the local encrypted database.
///
/// Missing rows and failed reads remain distinct. The shared verifier can then
/// return a pending decision instead of treating unavailable status as clean.
pub struct SqliteVerificationStore<'a>(pub &'a Connection);

impl VerificationStore for SqliteVerificationStore<'_> {
    fn key_at(&self, did: &Did, at: &str) -> StoreLookup<KeyRegistryEntry> {
        match crate::crypto::key_registry::resolve_key_at(self.0, did, at) {
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

    /// This node's own anchors: a `credential_anchors` row is `confirmed` only
    /// once the submission journal holds a ledger receipt for the transaction,
    /// which was built from the stored `integrity_hash` and `issuer_did`.
    fn integrity_anchor(&self, credential_hash: &str) -> StoreLookup<IntegrityAnchor> {
        match self
            .0
            .query_row(
                "SELECT c.integrity_hash, c.issuer_did, a.anchor_tx_hash, a.confirmed_at \
                 FROM credential_anchors a JOIN credentials c ON c.id = a.credential_id \
                 WHERE c.integrity_hash = ?1 AND a.anchor_status = 'confirmed' \
                   AND a.anchor_tx_hash IS NOT NULL \
                 ORDER BY a.confirmed_at LIMIT 1",
                rusqlite::params![credential_hash],
                |r| {
                    Ok(IntegrityAnchor {
                        credential_hash: r.get(0)?,
                        issuer_did: r.get(1)?,
                        tx_hash: r.get(2)?,
                        anchored_at: r.get(3)?,
                    })
                },
            )
            .optional()
        {
            Ok(Some(anchor)) => StoreLookup::Found(anchor),
            Ok(None) => StoreLookup::Missing,
            Err(_) => StoreLookup::Unavailable,
        }
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
            unavailable.key_at(&did, "2026-01-01T00:00:00Z"),
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
            available.key_at(&did, "2026-01-01T00:00:00Z"),
            StoreLookup::Missing
        ));
        assert_eq!(available.status_list_bits("missing"), StoreLookup::Missing);
        assert_eq!(available.suspension("missing"), StoreLookup::Missing);
        assert_eq!(
            available.is_superseded("missing"),
            StoreLookup::Found(false)
        );
    }

    #[test]
    fn a_credential_is_anchored_once_its_own_anchor_is_confirmed() {
        use crate::credentials::{issue::IssueCredentialRequest, Issuer};
        use ed25519_dalek::SigningKey;

        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let issuer = Issuer::from_key(SigningKey::from_bytes(&[7; 32]));
        let subject = alexandria_verify::did::derive_did_key(&SigningKey::from_bytes(&[9; 32]));
        let req = IssueCredentialRequest {
            credential_type: CredentialType::FormalCredential,
            subject,
            claim: Claim::Skill(SkillClaim {
                skill_id: "skill_anchor".into(),
                level: 2,
                score: 0.5,
                evidence_refs: vec![],
                rubric_version: None,
                assessment_method: None,
                provenance: None,
            }),
            evidence_refs: vec![],
            expiration_date: None,
            supersedes: None,
            integrity_session_id: None,
            integrity_policy: None,
        };
        let now = "2026-11-09T00:00:00Z";
        let vc = crate::credentials::issue(db.conn(), &issuer, &req, now).unwrap();
        let id = vc.id.clone().unwrap();
        let policy = VerificationPolicy::default();

        // Issuance queues the anchor; a queued anchor is not an anchor.
        assert!(!verify_credential_db(db.conn(), &vc, now, &policy).integrity_anchored);
        db.conn()
            .execute(
                "UPDATE credential_anchors SET anchor_status = 'submitted', \
                 anchor_tx_hash = ?2 WHERE credential_id = ?1",
                rusqlite::params![id, "ab".repeat(32)],
            )
            .unwrap();
        assert!(!verify_credential_db(db.conn(), &vc, now, &policy).integrity_anchored);

        db.conn()
            .execute(
                "UPDATE credential_anchors SET anchor_status = 'confirmed', confirmed_at = ?2 \
                 WHERE credential_id = ?1",
                rusqlite::params![id, now],
            )
            .unwrap();
        assert!(verify_credential_db(db.conn(), &vc, now, &policy).integrity_anchored);

        // The anchor names the stored content: an altered copy is not anchored.
        let mut altered = vc.clone();
        altered.valid_from = "2026-11-10T00:00:00Z".into();
        assert!(!verify_credential_db(db.conn(), &altered, now, &policy).integrity_anchored);
    }
}
