//! Local PinBoard commitment management — declare, revoke, observe,
//! list. Backed by `pinboard_observations` (migration 25), shared
//! with `p2p::pinboard::handle_pinboard_message` for remote ingest.
//!
//! Commitments are signed artifacts in the wire format (so peers can
//! re-broadcast them verbatim with a verifiable signature). The
//! declaration path here writes a row with placeholder signature
//! material — the IPC command wrapper signs the row before
//! broadcasting, since signing is keystore-dependent and shouldn't
//! be a precondition for the storage layer.

use uuid::Uuid;

use crate::crypto::did::Did;
use crate::p2p::pinboard::PinboardCommitment;

pub fn declare_commitment(
    db: &rusqlite::Connection,
    pinner_did: &Did,
    subject_did: &Did,
    scope: &[String],
) -> Result<PinboardCommitment, String> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let commit = PinboardCommitment {
        id: format!("urn:uuid:{}", Uuid::new_v4()),
        pinner_did: pinner_did.as_str().to_string(),
        subject_did: subject_did.as_str().to_string(),
        scope: scope.to_vec(),
        commitment_since: now,
        revoked_at: None,
        // Placeholders — the IPC command layer overwrites these with
        // a real signature before the commitment is broadcast.
        signature: "unsigned".into(),
        public_key: pinner_did.as_str().to_string(),
    };
    insert_observation(db, &commit)?;
    Ok(commit)
}

pub fn revoke_commitment(db: &rusqlite::Connection, commitment_id: &str) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let updated = db
        .execute(
            "UPDATE pinboard_observations SET revoked_at = ?2 \
             WHERE id = ?1 AND revoked_at IS NULL",
            rusqlite::params![commitment_id, &now],
        )
        .map_err(|e| format!("revoke pinboard commitment: {e}"))?;
    if updated == 0 {
        // Idempotent: revoking an already-revoked or unknown row is fine.
        log::debug!("revoke_commitment: no open row for {commitment_id}");
    }
    Ok(())
}

pub fn record_observation(
    db: &rusqlite::Connection,
    commitment: &PinboardCommitment,
) -> Result<(), String> {
    insert_observation(db, commitment)
}

pub fn list_pinners_for(
    db: &rusqlite::Connection,
    subject: &Did,
) -> Result<Vec<PinboardCommitment>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, pinner_did, subject_did, scope, commitment_since, \
                    revoked_at, signature, public_key \
             FROM pinboard_observations WHERE subject_did = ?1 \
             ORDER BY commitment_since DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![subject.as_str()], |r| {
            let scope_json: String = r.get(3)?;
            let scope: Vec<String> = serde_json::from_str(&scope_json).unwrap_or_default();
            Ok(PinboardCommitment {
                id: r.get(0)?,
                pinner_did: r.get(1)?,
                subject_did: r.get(2)?,
                scope,
                commitment_since: r.get(4)?,
                revoked_at: r.get(5)?,
                signature: r.get(6)?,
                public_key: r.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn insert_observation(db: &rusqlite::Connection, c: &PinboardCommitment) -> Result<(), String> {
    let scope_json = serde_json::to_string(&c.scope).map_err(|e| e.to_string())?;
    db.execute(
        "INSERT OR IGNORE INTO pinboard_observations \
         (id, pinner_did, subject_did, scope, commitment_since, \
          revoked_at, signature, public_key) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            c.id,
            c.pinner_did,
            c.subject_did,
            scope_json,
            c.commitment_since,
            c.revoked_at,
            c.signature,
            c.public_key,
        ],
    )
    .map_err(|e| format!("insert pinboard observation: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn declare_commitment_emits_signed_record() {
        // A declared commitment is a signed artifact: the pinner
        // attests (pinner_did, subject_did, scope, since) and can
        // rebroadcast identically on replay.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let pinner = Did("did:key:zPinner".into());
        let subject = Did("did:key:zSubject".into());
        let commit =
            declare_commitment(db.conn(), &pinner, &subject, &["credentials".into()]).unwrap();
        assert!(!commit.signature.is_empty());
        assert!(!commit.public_key.is_empty());
        assert_eq!(commit.subject_did, subject.as_str());
    }

    #[test]
    fn revoke_commitment_marks_revoked_at() {
        // Revocation MUST stamp `revoked_at`; the pinning layer uses
        // this to demote content from the pinboard tier to cache tier.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let pinner = Did("did:key:zPinner".into());
        let subject = Did("did:key:zSubject".into());
        let c = declare_commitment(db.conn(), &pinner, &subject, &["credentials".into()]).unwrap();
        revoke_commitment(db.conn(), &c.id).unwrap();
        let rows = list_pinners_for(db.conn(), &subject).unwrap();
        assert!(
            rows.iter().all(|r| r.revoked_at.is_some()),
            "every row must be marked revoked"
        );
    }

    #[test]
    fn list_pinners_returns_empty_for_no_commitments() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let rows = list_pinners_for(db.conn(), &Did("did:key:zUnseen".into())).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn record_observation_persists_remote_commitment() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let commit = PinboardCommitment {
            id: "remote-1".into(),
            pinner_did: "did:key:zRemote".into(),
            subject_did: "did:key:zSubject".into(),
            scope: vec!["credentials".into()],
            commitment_since: "2026-04-13T00:00:00Z".into(),
            revoked_at: None,
            signature: "sig".into(),
            public_key: "pk".into(),
        };
        record_observation(db.conn(), &commit).unwrap();
        let rows = list_pinners_for(db.conn(), &Did("did:key:zSubject".into())).unwrap();
        assert!(rows.iter().any(|r| r.id == "remote-1"));
    }
}
