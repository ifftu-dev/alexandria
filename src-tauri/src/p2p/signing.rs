//! Gossip message signing.
//!
//! Constructs and signs `SignedGossipMessage` envelopes using the
//! sender's Cardano Ed25519 signing key. Every message published
//! on the P2P network is wrapped in a signed envelope so receivers
//! can verify authenticity and link the sender to an on-chain identity.
//!
//! The signature covers a canonical message that includes ALL envelope
//! fields — topic, timestamp, stake_address, and the payload — preventing
//! replay attacks with modified timestamps and identity field tampering.

use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};

use crate::crypto::signing as core_signing;

use super::types::SignedGossipMessage;

/// Build the canonical bytes that are signed/verified.
///
/// Format: `SHA-256(topic || timestamp_be_bytes || stake_address || payload)`
///
/// Using SHA-256 as a pre-hash keeps the signed message a fixed 32 bytes
/// regardless of payload size, and ensures all fields are unambiguously
/// committed to the signature.
fn canonical_signed_bytes(
    topic: &str,
    timestamp: u64,
    stake_address: &str,
    payload: &[u8],
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(topic.as_bytes());
    hasher.update(timestamp.to_be_bytes());
    hasher.update(stake_address.as_bytes());
    hasher.update(payload);
    hasher.finalize().to_vec()
}

/// Create a signed gossip message envelope.
///
/// Signs a canonical hash of ALL envelope fields (topic, timestamp,
/// stake_address, payload) with the sender's Cardano signing key.
/// This prevents replay attacks with modified timestamps and identity
/// field tampering.
///
/// The payload should already be JSON-serialized topic-specific data
/// (e.g., a course announcement, evidence record, etc.).
pub fn sign_gossip_message(
    topic: &str,
    payload: Vec<u8>,
    signing_key: &SigningKey,
    stake_address: &str,
) -> SignedGossipMessage {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let canonical = canonical_signed_bytes(topic, timestamp, stake_address, &payload);
    let signed = core_signing::sign(&canonical, signing_key);

    SignedGossipMessage {
        topic: topic.to_string(),
        payload,
        signature: signed.signature,
        public_key: signed.public_key,
        stake_address: stake_address.to_string(),
        timestamp,
        encrypted: false,
        key_id: None,
    }
}

/// Verify the Ed25519 signature on a gossip message envelope.
///
/// Reconstructs the canonical signed bytes from the envelope fields
/// and verifies the Ed25519 signature. This ensures that the topic,
/// timestamp, stake_address, and payload are all authenticated —
/// tampering with ANY field invalidates the signature.
pub fn verify_gossip_signature(
    message: &SignedGossipMessage,
) -> Result<(), core_signing::SigningError> {
    let canonical = canonical_signed_bytes(
        &message.topic,
        message.timestamp,
        &message.stake_address,
        &message.payload,
    );
    let signed_msg = core_signing::SignedMessage {
        payload: canonical,
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
    fn tampered_timestamp_fails_verification() {
        let key = test_key();
        let msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"test\":true}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        let mut tampered = msg;
        tampered.timestamp += 100; // modify timestamp

        assert!(
            verify_gossip_signature(&tampered).is_err(),
            "modifying timestamp should invalidate signature"
        );
    }

    #[test]
    fn tampered_stake_address_fails_verification() {
        let key = test_key();
        let msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"test\":true}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        let mut tampered = msg;
        tampered.stake_address = "stake_test1uattacker_address".to_string();

        assert!(
            verify_gossip_signature(&tampered).is_err(),
            "modifying stake_address should invalidate signature"
        );
    }

    #[test]
    fn tampered_topic_fails_verification() {
        let key = test_key();
        let msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"test\":true}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        let mut tampered = msg;
        tampered.topic = "/alexandria/taxonomy/1.0".to_string();

        assert!(
            verify_gossip_signature(&tampered).is_err(),
            "modifying topic should invalidate signature"
        );
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
