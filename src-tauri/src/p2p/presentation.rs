//! `TOPIC_VC_PRESENTATION` — subject opts in to broadcast a
//! selectively-disclosed presentation of a credential.
//!
//! PR 9 lands the parse + accept path. Persistence is a no-op for
//! now — until PR 11's full presentation layer ships, there's no
//! `presentations` table to insert into; the gossip path validates
//! the envelope is parseable so peers don't silently drop valid
//! traffic, but doesn't surface anything to the UI yet.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

#[derive(serde::Deserialize)]
struct PresentationGossip {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    audience: String,
    #[allow(dead_code)]
    nonce: String,
    #[allow(dead_code)]
    payload_json: String,
    #[allow(dead_code)]
    proof: String,
}

pub fn handle_presentation_message(
    _db: &Database,
    message: &SignedGossipMessage,
) -> Result<(), String> {
    serde_json::from_slice::<PresentationGossip>(&message.payload)
        .map(|_| ())
        .map_err(|e| format!("malformed presentation payload: {e}"))
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
    fn valid_presentation_message_is_accepted() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg(
            br#"{"id":"pres-1","audience":"audit","nonce":"n","payload_json":"{}","proof":"sig"}"#,
        );
        handle_presentation_message(&db, &msg).unwrap();
    }

    #[test]
    fn malformed_presentation_payload_errors() {
        // Gossip handler still returns Err on malformed-but-topic-right
        // payloads so the pump records the drop via its error path.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let msg = stub_msg(b"not-json");
        assert!(handle_presentation_message(&db, &msg).is_err());
    }
}
