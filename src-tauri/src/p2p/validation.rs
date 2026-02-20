//! Gossip message validation pipeline.
//!
//! Every incoming gossip message passes through a 5-step validation
//! pipeline before being forwarded to the application layer:
//!
//! 1. **Signature**: Ed25519 signature over the payload is valid.
//! 2. **Freshness**: Timestamp is within ±5 minutes of local time.
//! 3. **Deduplication**: Blake2b-256 hash of payload not in seen cache.
//! 4. **Schema**: Payload is valid JSON (topic-specific schema validation
//!    is deferred to the domain handlers in later PRs).
//! 5. **Authority**: For taxonomy updates, verify the signer is a DAO
//!    committee member (stubbed — requires on-chain lookup).
//!
//! Per spec (§7.3): "Invalid messages are dropped silently. Peers that
//! repeatedly send invalid messages are scored down by GossipSub's
//! peer scoring mechanism, eventually disconnected."

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::crypto::hash::blake2b_256;

use super::signing::verify_gossip_signature;
use super::types::{SignedGossipMessage, TOPIC_TAXONOMY};

/// Maximum age of a message in seconds (5 minutes per spec §7.3).
const FRESHNESS_WINDOW_SECS: u64 = 5 * 60;

/// Maximum number of entries in the dedup cache before pruning.
/// At 64 bytes per entry (hex-encoded Blake2b-256), 100k entries ≈ 6.4 MB.
const DEDUP_CACHE_MAX: usize = 100_000;

#[derive(Error, Debug, PartialEq)]
pub enum ValidationError {
    #[error("signature verification failed: {0}")]
    InvalidSignature(String),
    #[error("message too old: timestamp {timestamp} is {age_secs}s in the past (max {FRESHNESS_WINDOW_SECS}s)")]
    TooOld { timestamp: u64, age_secs: u64 },
    #[error("message from the future: timestamp {timestamp} is {ahead_secs}s ahead (max {FRESHNESS_WINDOW_SECS}s)")]
    FromFuture { timestamp: u64, ahead_secs: u64 },
    #[error("duplicate message: payload hash {hash}")]
    Duplicate { hash: String },
    #[error("invalid payload: {0}")]
    InvalidPayload(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
}

/// Validation result with the rejection reason (for logging/metrics).
pub type ValidationResult = Result<(), ValidationError>;

/// The message validator.
///
/// Maintains a dedup cache of seen payload hashes. Thread-safe via
/// interior mutability (`Mutex`) so it can be shared across the
/// async swarm event loop.
pub struct MessageValidator {
    /// Set of Blake2b-256 hashes (hex) of previously seen payloads.
    seen: Mutex<HashSet<String>>,
}

impl MessageValidator {
    /// Create a new validator with an empty dedup cache.
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
        }
    }

    /// Run the full validation pipeline on an incoming gossip message.
    ///
    /// Returns `Ok(())` if the message passes all checks, or the first
    /// `ValidationError` encountered. Checks run in order:
    /// signature → freshness → dedup → schema → authority.
    pub fn validate(&self, message: &SignedGossipMessage) -> ValidationResult {
        self.check_signature(message)?;
        self.check_freshness(message)?;
        self.check_dedup(message)?;
        self.check_schema(message)?;
        self.check_authority(message)?;
        Ok(())
    }

    /// Step 1: Verify the Ed25519 signature over the payload.
    fn check_signature(&self, message: &SignedGossipMessage) -> ValidationResult {
        verify_gossip_signature(message)
            .map_err(|e| ValidationError::InvalidSignature(e.to_string()))
    }

    /// Step 2: Check that the message timestamp is within ±5 minutes.
    pub(crate) fn check_freshness(&self, message: &SignedGossipMessage) -> ValidationResult {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if message.timestamp > now + FRESHNESS_WINDOW_SECS {
            return Err(ValidationError::FromFuture {
                timestamp: message.timestamp,
                ahead_secs: message.timestamp - now,
            });
        }

        if now > message.timestamp + FRESHNESS_WINDOW_SECS {
            return Err(ValidationError::TooOld {
                timestamp: message.timestamp,
                age_secs: now - message.timestamp,
            });
        }

        Ok(())
    }

    /// Step 3: Check for duplicate messages using Blake2b-256 of the payload.
    ///
    /// If the payload hash has been seen before, the message is rejected.
    /// Otherwise, the hash is added to the seen cache.
    fn check_dedup(&self, message: &SignedGossipMessage) -> ValidationResult {
        let hash = hex::encode(blake2b_256(&message.payload));

        let mut seen = self.seen.lock().unwrap();

        // Prune if cache is too large (simple strategy: clear all).
        // A more sophisticated approach (LRU, time-based expiry) can
        // be added later — the freshness check already limits the
        // window of relevant messages to ±5 minutes.
        if seen.len() >= DEDUP_CACHE_MAX {
            log::info!("Dedup cache reached {DEDUP_CACHE_MAX} entries, clearing");
            seen.clear();
        }

        if !seen.insert(hash.clone()) {
            return Err(ValidationError::Duplicate { hash });
        }

        Ok(())
    }

    /// Step 4: Validate that the payload is well-formed JSON.
    ///
    /// Topic-specific schema validation (e.g., verifying a catalog
    /// message has the required `course_id`, `title`, etc.) is deferred
    /// to the domain handlers in later PRs (catalog PR 3, evidence PR 4,
    /// taxonomy PR 5). This check only verifies syntactic validity.
    fn check_schema(&self, message: &SignedGossipMessage) -> ValidationResult {
        // Payload must be valid JSON
        serde_json::from_slice::<serde_json::Value>(&message.payload)
            .map_err(|e| ValidationError::InvalidPayload(format!("invalid JSON: {e}")))?;
        Ok(())
    }

    /// Step 5: Authority check for privileged topics.
    ///
    /// Per spec §7.3: "For taxonomy updates, verify the signer is a
    /// DAO committee member."
    ///
    /// The validation pipeline runs without DB access (it lives in the
    /// swarm event loop). Full authority verification — checking that
    /// the signer is a DAO committee member via `governance_dao_members`
    /// — is performed by the taxonomy domain handler (`p2p::taxonomy::
    /// handle_taxonomy_message`) which has DB access. This step does a
    /// lightweight topic-level check only.
    fn check_authority(&self, message: &SignedGossipMessage) -> ValidationResult {
        if message.topic == TOPIC_TAXONOMY {
            // Lightweight check: taxonomy messages must have a non-empty
            // stake address (the domain handler verifies committee membership).
            if message.stake_address.is_empty() {
                return Err(ValidationError::Unauthorized(
                    "taxonomy update missing stake_address".into(),
                ));
            }
            log::debug!(
                "Taxonomy message from {} — committee check deferred to domain handler",
                message.stake_address
            );
        }
        Ok(())
    }

    /// Get the current size of the dedup cache.
    #[cfg(test)]
    pub fn seen_count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl Default for MessageValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::signing::sign_gossip_message;
    use ed25519_dalek::SigningKey;

    fn test_key() -> SigningKey {
        let mut rng = rand::thread_rng();
        SigningKey::generate(&mut rng)
    }

    fn valid_message(key: &SigningKey, topic: &str) -> SignedGossipMessage {
        sign_gossip_message(
            topic,
            b"{\"test\":true}".to_vec(),
            key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        )
    }

    // -- Signature tests --

    #[test]
    fn valid_message_passes_all_checks() {
        let key = test_key();
        let msg = valid_message(&key, "/alexandria/catalog/1.0");
        let validator = MessageValidator::new();

        assert!(validator.validate(&msg).is_ok());
    }

    #[test]
    fn tampered_payload_fails_signature() {
        let key = test_key();
        let mut msg = valid_message(&key, "/alexandria/catalog/1.0");
        msg.payload = b"{\"tampered\":true}".to_vec();

        let validator = MessageValidator::new();
        let result = validator.validate(&msg);

        assert!(matches!(result, Err(ValidationError::InvalidSignature(_))));
    }

    #[test]
    fn empty_signature_fails() {
        let key = test_key();
        let mut msg = valid_message(&key, "/alexandria/catalog/1.0");
        msg.signature = vec![];

        let validator = MessageValidator::new();
        let result = validator.validate(&msg);

        assert!(matches!(result, Err(ValidationError::InvalidSignature(_))));
    }

    // -- Freshness tests --

    #[test]
    fn message_too_old_is_rejected() {
        let key = test_key();
        let mut msg = valid_message(&key, "/alexandria/catalog/1.0");
        // Re-sign with an old timestamp by directly setting it
        // (signature was over original payload, but we need to test
        // freshness independently — so we skip signature check by
        // testing freshness directly)
        msg.timestamp = 1_000_000; // Way in the past

        let validator = MessageValidator::new();
        // validate() checks signature first, which would pass (timestamp
        // isn't signed). But we want to test freshness, so call directly.
        let result = validator.check_freshness(&msg);

        assert!(matches!(result, Err(ValidationError::TooOld { .. })));
    }

    #[test]
    fn message_from_future_is_rejected() {
        let key = test_key();
        let mut msg = valid_message(&key, "/alexandria/catalog/1.0");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        msg.timestamp = now + 10 * 60; // 10 minutes in the future

        let validator = MessageValidator::new();
        let result = validator.check_freshness(&msg);

        assert!(matches!(result, Err(ValidationError::FromFuture { .. })));
    }

    #[test]
    fn message_within_window_passes_freshness() {
        let key = test_key();
        let msg = valid_message(&key, "/alexandria/catalog/1.0");

        let validator = MessageValidator::new();
        assert!(validator.check_freshness(&msg).is_ok());
    }

    // -- Dedup tests --

    #[test]
    fn duplicate_message_is_rejected() {
        let key = test_key();
        let msg = valid_message(&key, "/alexandria/catalog/1.0");

        let validator = MessageValidator::new();
        assert!(validator.validate(&msg).is_ok());

        // Same message again should be rejected as duplicate
        let result = validator.validate(&msg);
        assert!(matches!(result, Err(ValidationError::Duplicate { .. })));
    }

    #[test]
    fn different_payloads_are_not_duplicates() {
        let key = test_key();
        let msg1 = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"id\":1}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );
        let msg2 = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"id\":2}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        let validator = MessageValidator::new();
        assert!(validator.validate(&msg1).is_ok());
        assert!(validator.validate(&msg2).is_ok());
        assert_eq!(validator.seen_count(), 2);
    }

    // -- Schema tests --

    #[test]
    fn invalid_json_fails_schema() {
        let key = test_key();
        // Sign a message with invalid JSON payload
        let msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"not valid json{{{".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        let validator = MessageValidator::new();
        // Schema check is step 4, but we call it directly
        let result = validator.check_schema(&msg);
        assert!(matches!(result, Err(ValidationError::InvalidPayload(_))));
    }

    #[test]
    fn valid_json_passes_schema() {
        let key = test_key();
        let msg = valid_message(&key, "/alexandria/catalog/1.0");

        let validator = MessageValidator::new();
        assert!(validator.check_schema(&msg).is_ok());
    }

    // -- Authority tests --

    #[test]
    fn taxonomy_message_passes_authority_stub() {
        let key = test_key();
        let msg = valid_message(&key, "/alexandria/taxonomy/1.0");

        let validator = MessageValidator::new();
        // Authority is stubbed — should pass with a debug log
        assert!(validator.check_authority(&msg).is_ok());
    }

    #[test]
    fn non_taxonomy_message_passes_authority() {
        let key = test_key();
        let msg = valid_message(&key, "/alexandria/catalog/1.0");

        let validator = MessageValidator::new();
        assert!(validator.check_authority(&msg).is_ok());
    }
}
