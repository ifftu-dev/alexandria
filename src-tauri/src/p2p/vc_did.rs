//! `TOPIC_VC_DID` handler — issuers broadcast their DID document and
//! key rotations. Stub — implementation in PR 9.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DidIngest {
    Stored,
    UpdatedRegistry,
    Ignored,
}

pub fn handle_did_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<DidIngest, String> {
    unimplemented!("PR 9 — DID doc propagation")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_msg(topic: &str, payload: &[u8]) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: topic.into(),
            payload: payload.to_vec(),
            signature: vec![0u8; 64],
            public_key: vec![0u8; 32],
            stake_address: "stake_test1...".into(),
            timestamp: 1_712_880_000,
            encrypted: false,
            key_id: None,
        }
    }

    #[test]
    #[ignore = "pending PR 9 — DID doc propagation"]
    fn first_observation_of_unknown_did_is_stored() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg("/alexandria/vc-did/1.0", br#"{"did":"did:key:zNew"}"#);
        let outcome = handle_did_message(&db, &msg).unwrap();
        assert_eq!(outcome, DidIngest::Stored);
    }

    #[test]
    #[ignore = "pending PR 9 — DID doc propagation"]
    fn second_observation_updates_registry_for_rotation() {
        // A later message that carries a rotation record must transition
        // from `Stored` on first sight to `UpdatedRegistry` on the key
        // rotation — this is what lets historical verification (§5.3)
        // survive across peers.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let first = stub_msg("/alexandria/vc-did/1.0", br#"{"did":"did:key:zA"}"#);
        let _ = handle_did_message(&db, &first).unwrap();
        let rotation = stub_msg(
            "/alexandria/vc-did/1.0",
            br#"{"did":"did:key:zA","rotated_to":"did:key:zA2"}"#,
        );
        let outcome = handle_did_message(&db, &rotation).unwrap();
        assert_eq!(outcome, DidIngest::UpdatedRegistry);
    }

    #[test]
    #[ignore = "pending PR 9 — DID doc propagation"]
    fn malformed_payload_is_ignored_not_errored() {
        // Gossip is noisy. Garbage payloads should return `Ignored`
        // rather than propagating up as errors — an error stops the
        // whole gossip pump for that peer.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg("/alexandria/vc-did/1.0", b"garbage");
        let outcome = handle_did_message(&db, &msg).unwrap();
        assert_eq!(outcome, DidIngest::Ignored);
    }
}
