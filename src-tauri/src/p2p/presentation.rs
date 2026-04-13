//! `TOPIC_VC_PRESENTATION` — subject opts in to broadcast a
//! selectively-disclosed presentation of a credential. Stub — PR 9.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

pub fn handle_presentation_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<(), String> {
    unimplemented!("PR 9 — presentation gossip")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_msg(payload: &[u8]) -> SignedGossipMessage {
        SignedGossipMessage {
            topic: "/alexandria/vc-presentation/1.0".into(),
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
    #[ignore = "pending PR 9 — presentation gossip"]
    fn valid_presentation_message_is_accepted() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg(
            br#"{"id":"pres-1","audience":"audit","nonce":"n","payload_json":"{}","proof":"sig"}"#,
        );
        handle_presentation_message(&db, &msg).unwrap();
    }

    #[test]
    #[ignore = "pending PR 9 — presentation gossip"]
    fn malformed_presentation_payload_errors() {
        // Gossip handler still returns Err on malformed-but-topic-right
        // payloads so the pump records the drop via its error path.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg(b"not-json");
        assert!(handle_presentation_message(&db, &msg).is_err());
    }
}
