//! Local PinBoard commitment management — declare, revoke, observe.
//! Stub — implementation in PR 10.

use crate::crypto::did::Did;
use crate::p2p::pinboard::PinboardCommitment;

pub fn declare_commitment(
    _db: &rusqlite::Connection,
    _pinner_did: &Did,
    _subject_did: &Did,
    _scope: &[String],
) -> Result<PinboardCommitment, String> {
    unimplemented!("PR 10 — declare pinboard commitment")
}

pub fn revoke_commitment(_db: &rusqlite::Connection, _commitment_id: &str) -> Result<(), String> {
    unimplemented!("PR 10 — revoke pinboard commitment")
}

pub fn record_observation(
    _db: &rusqlite::Connection,
    _commitment: &PinboardCommitment,
) -> Result<(), String> {
    unimplemented!("PR 10 — record remote pinboard commitment")
}

pub fn list_pinners_for(
    _db: &rusqlite::Connection,
    _subject: &Did,
) -> Result<Vec<PinboardCommitment>, String> {
    unimplemented!("PR 10 — list pinners for subject")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    #[ignore = "pending PR 10 — declare pinboard commitment"]
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
    #[ignore = "pending PR 10 — revoke pinboard commitment"]
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
    #[ignore = "pending PR 10 — list pinners for subject"]
    fn list_pinners_returns_empty_for_no_commitments() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let rows = list_pinners_for(db.conn(), &Did("did:key:zUnseen".into())).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    #[ignore = "pending PR 10 — record remote pinboard commitment"]
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
