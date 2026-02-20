//! Gossip message signing.
//!
//! Constructs and signs `SignedGossipMessage` envelopes using the
//! sender's Cardano Ed25519 signing key. Every message published
//! on the P2P network is wrapped in a signed envelope so receivers
//! can verify authenticity and link the sender to an on-chain identity.

use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;

use crate::crypto::signing as core_signing;

use super::types::SignedGossipMessage;

/// Create a signed gossip message envelope.
///
/// Signs the raw `payload` bytes with the sender's Cardano signing key
/// and populates all envelope fields (signature, public key, stake address,
/// timestamp).
///
/// The payload should already be JSON-serialized topic-specific data
/// (e.g., a course announcement, evidence record, etc.).
pub fn sign_gossip_message(
    topic: &str,
    payload: Vec<u8>,
    signing_key: &SigningKey,
    stake_address: &str,
) -> SignedGossipMessage {
    let signed = core_signing::sign(&payload, signing_key);

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    SignedGossipMessage {
        topic: topic.to_string(),
        payload,
        signature: signed.signature,
        public_key: signed.public_key,
        stake_address: stake_address.to_string(),
        timestamp,
    }
}

/// Verify the Ed25519 signature on a gossip message envelope.
///
/// Returns `Ok(())` if the signature is valid, or an error describing
/// why verification failed. This checks ONLY the cryptographic
/// signature — freshness, dedup, schema, and authority checks are
/// handled by the validation pipeline.
pub fn verify_gossip_signature(
    message: &SignedGossipMessage,
) -> Result<(), core_signing::SigningError> {
    let signed_msg = core_signing::SignedMessage {
        payload: message.payload.clone(),
        signature: message.signature.clone(),
        public_key: message.public_key.clone(),
    };
    core_signing::verify(&signed_msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> SigningKey {
        let mut rng = rand::thread_rng();
        SigningKey::generate(&mut rng)
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let key = test_key();
        let payload = b"{\"title\":\"Test Course\"}".to_vec();

        let msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            payload.clone(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        assert_eq!(msg.topic, "/alexandria/catalog/1.0");
        assert_eq!(msg.payload, payload);
        assert_eq!(msg.public_key.len(), 32);
        assert_eq!(msg.signature.len(), 64);
        assert!(msg.timestamp > 0);
        assert!(msg.stake_address.starts_with("stake_test1"));

        assert!(verify_gossip_signature(&msg).is_ok());
    }

    #[test]
    fn tampered_payload_fails_verification() {
        let key = test_key();
        let msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"original".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        let mut tampered = msg;
        tampered.payload = b"tampered".to_vec();

        assert!(verify_gossip_signature(&tampered).is_err());
    }

    #[test]
    fn wrong_public_key_fails_verification() {
        let key1 = test_key();
        let key2 = test_key();

        let mut msg = sign_gossip_message(
            "/alexandria/evidence/1.0",
            b"payload".to_vec(),
            &key1,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        // Replace public key with a different one
        msg.public_key = key2.verifying_key().to_bytes().to_vec();

        assert!(verify_gossip_signature(&msg).is_err());
    }

    #[test]
    fn empty_signature_fails_verification() {
        let key = test_key();
        let mut msg = sign_gossip_message(
            "/alexandria/profiles/1.0",
            b"payload".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        msg.signature = vec![];

        assert!(verify_gossip_signature(&msg).is_err());
    }

    #[test]
    fn timestamp_is_recent() {
        let key = test_key();
        let msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"payload".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Timestamp should be within 1 second of now
        assert!(msg.timestamp <= now);
        assert!(msg.timestamp >= now - 1);
    }
}
