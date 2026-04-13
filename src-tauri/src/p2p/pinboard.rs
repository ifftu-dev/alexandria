//! `TOPIC_PINBOARD` handler — peers broadcast opt-in commitments to
//! pin specific subjects' content for community redundancy.
//! Stub — implementation in PR 10.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PinboardCommitment {
    pub id: String,
    pub pinner_did: String,
    pub subject_did: String,
    pub scope: Vec<String>,
    pub commitment_since: String,
    pub revoked_at: Option<String>,
    pub signature: String,
    pub public_key: String,
}

pub fn handle_pinboard_message(db: &Database, message: &SignedGossipMessage) -> Result<(), String> {
    let commit: PinboardCommitment = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("malformed pinboard payload: {e}"))?;
    crate::ipfs::pinboard::record_observation(db.conn(), &commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_commitment(revoked_at: Option<&str>) -> PinboardCommitment {
        PinboardCommitment {
            id: "commit-1".into(),
            pinner_did: "did:key:zPinner".into(),
            subject_did: "did:key:zSubject".into(),
            scope: vec!["credentials".into()],
            commitment_since: "2026-04-13T00:00:00Z".into(),
            revoked_at: revoked_at.map(Into::into),
            signature: "sig".into(),
            public_key: "pk".into(),
        }
    }

    #[test]
    fn pinboard_commitment_serde_round_trips() {
        // Gossip messages carry these as JSON payloads. Locking the
        // serde surface here prevents silent field-name drift between
        // the handler, the storage layer, and the IPC command.
        let c = stub_commitment(None);
        let s = serde_json::to_string(&c).unwrap();
        let back: PinboardCommitment = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, c.id);
        assert_eq!(back.scope, c.scope);
        assert!(back.revoked_at.is_none());
    }

    #[test]
    fn pinboard_commitment_revocation_round_trips() {
        let c = stub_commitment(Some("2026-05-01T00:00:00Z"));
        let s = serde_json::to_string(&c).unwrap();
        let back: PinboardCommitment = serde_json::from_str(&s).unwrap();
        assert_eq!(back.revoked_at.as_deref(), Some("2026-05-01T00:00:00Z"));
    }

    #[test]
    fn handle_pinboard_message_persists_observation() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let payload = serde_json::to_vec(&stub_commitment(None)).unwrap();
        let msg = SignedGossipMessage {
            topic: "/alexandria/pinboard/1.0".into(),
            payload,
            signature: vec![0u8; 64],
            public_key: vec![0u8; 32],
            stake_address: "stake_test1...".into(),
            timestamp: 1_712_880_000,
            encrypted: false,
            key_id: None,
        };
        handle_pinboard_message(&db, &msg).unwrap();
        // Round-trip: the observation must now be findable via the
        // local `list_pinners_for(subject)` query.
        let found = crate::ipfs::pinboard::list_pinners_for(
            db.conn(),
            &crate::crypto::did::Did("did:key:zSubject".into()),
        )
        .unwrap();
        assert!(found.iter().any(|c| c.id == "commit-1"));
    }
}
