//! Gossip message validation pipeline.
//!
//! Every incoming gossip message passes through a 5-step validation
//! pipeline before being forwarded to the application layer:
//!
//! 1. **Signature**: Ed25519 signature over the payload is valid.
//! 2. **Identity binding** (privileged topics only): the
//!    `(stake_address, public_key)` pair is registered in
//!    `stake_pubkey_registry` for the current timestamp. See
//!    `docs/stake-pubkey-registry.md` and [`crate::p2p::registry`].
//! 3. **Freshness**: Timestamp is within ±5 minutes of local time.
//! 4. **Deduplication**: Blake2b-256 hash of payload not in seen cache.
//! 5. **Schema**: Payload is valid JSON (topic-specific schema validation
//!    is deferred to the domain handlers in later PRs).
//! 6. **Authority**: For taxonomy updates, verify the signer is a DAO
//!    committee member (the domain handler does the heavy check; this
//!    step is a lightweight gate).
//!
//! Per spec (§7.3): "Invalid messages are dropped silently. Peers that
//! repeatedly send invalid messages are scored down by GossipSub's
//! peer scoring mechanism, eventually disconnected."

use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use lru::LruCache;
use thiserror::Error;

use crate::crypto::hash::blake2b_256;
use crate::db::Database;

use super::registry;
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
    #[error("identity not in registry: stake_address '{stake_address}' has no current binding for the signing public key (see docs/stake-pubkey-registry.md)")]
    IdentityMismatch { stake_address: String },
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
    #[error("internal error: {0}")]
    Internal(String),
}

/// Validation result with the rejection reason (for logging/metrics).
pub type ValidationResult = Result<(), ValidationError>;

/// The message validator.
///
/// Holds an LRU dedup cache and an optional `Database` handle. The
/// handle is used by [`check_identity_binding`](MessageValidator::check_identity_binding)
/// to look up `(stake_address, public_key)` bindings in
/// `stake_pubkey_registry` for privileged topics. Validators without a
/// DB handle (legacy `start_node` entry, unit tests) fail-open on the
/// identity check — they're intended for non-privileged paths only.
///
/// Thread-safe via interior mutability (`Mutex`) so it can be shared
/// across the async swarm event loop.
pub struct MessageValidator {
    /// LRU cache of Blake2b-256 hashes (hex) of previously seen payloads.
    /// When full, the least-recently-used entry is evicted (no replay
    /// window, unlike the previous full-clear strategy).
    seen: Mutex<LruCache<String, ()>>,
    /// Database handle for `stake_pubkey_registry` lookups. `None` when
    /// the validator is constructed via [`MessageValidator::new`] (legacy
    /// path, tests). Production wires this via
    /// [`MessageValidator::with_db`].
    db: Option<Arc<Mutex<Option<Database>>>>,
}

impl MessageValidator {
    /// Create a validator with no DB handle. Privileged-topic messages
    /// will fail-open on the identity check; intended for the legacy
    /// `start_node` path and tests that only exercise non-privileged
    /// topics.
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(LruCache::new(NonZeroUsize::new(DEDUP_CACHE_MAX).unwrap())),
            db: None,
        }
    }

    /// Create a validator wired to the active profile's database so the
    /// identity binding step can consult `stake_pubkey_registry`.
    pub fn with_db(db: Arc<Mutex<Option<Database>>>) -> Self {
        Self {
            seen: Mutex::new(LruCache::new(NonZeroUsize::new(DEDUP_CACHE_MAX).unwrap())),
            db: Some(db),
        }
    }

    /// Run the full validation pipeline on an incoming gossip message.
    ///
    /// Returns `Ok(())` if the message passes all checks, or the first
    /// `ValidationError` encountered. Checks run in order:
    /// signature → identity → freshness → dedup → schema → authority.
    pub fn validate(&self, message: &SignedGossipMessage) -> ValidationResult {
        self.check_signature(message)?;
        self.check_identity_binding(message)?;
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

    /// Step 1.5: identity binding via the persistent stake-pubkey
    /// registry.
    ///
    /// For **privileged** topics (taxonomy, governance, Sentinel
    /// priors, plugin DAO attestations) the
    /// `(stake_address, public_key)` pair MUST appear in
    /// `stake_pubkey_registry` within a window covering the current
    /// time. Non-privileged topics skip the check so arbitrary peers
    /// can still gossip catalog/profile/opinion traffic.
    ///
    /// If the validator was constructed without a DB handle
    /// ([`MessageValidator::new`]) the check fails-open — privileged
    /// topics aren't expected on that path.
    fn check_identity_binding(&self, message: &SignedGossipMessage) -> ValidationResult {
        if !registry::is_privileged_topic(&message.topic) {
            return Ok(());
        }
        let Some(db_handle) = &self.db else {
            // Fail-open: this validator wasn't given a DB. Production
            // callers always use `with_db`; tests construct via
            // `new`. A privileged-topic message reaching this branch
            // in production means startup wiring is broken — log a
            // WARN per message so the dormancy is loud rather than
            // silent. The `start_node_with_db` no-DB warning fires
            // once at boot; this one fires per offending message so
            // it survives log rotation.
            log::warn!(
                "validator: privileged-topic '{}' accepted WITHOUT registry check \
                 (no DB handle) — wiring bug or test fixture",
                message.topic
            );
            return Ok(());
        };
        let guard = db_handle
            .lock()
            .map_err(|_| ValidationError::Internal("db handle lock poisoned".into()))?;
        let Some(db) = guard.as_ref() else {
            // DB not open yet (profile not active) — refuse rather
            // than fail-open. Privileged messages received while the
            // registry is unavailable must wait for the profile to
            // come online.
            return Err(ValidationError::Internal(
                "no active profile DB; cannot consult stake-pubkey registry".into(),
            ));
        };
        registry::check_message(db.conn(), message).map_err(|e| {
            log::warn!("registry rejection on {}: {e}", message.topic);
            ValidationError::IdentityMismatch {
                stake_address: message.stake_address.clone(),
            }
        })
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
    /// Otherwise, the hash is added to the LRU cache. When the cache is
    /// full, the least-recently-used entry is evicted — no full-clear
    /// replay window.
    fn check_dedup(&self, message: &SignedGossipMessage) -> ValidationResult {
        let hash = hex::encode(blake2b_256(&message.payload));

        let mut seen = self
            .seen
            .lock()
            .map_err(|_| ValidationError::Internal("dedup cache lock poisoned".into()))?;

        if seen.contains(&hash) {
            return Err(ValidationError::Duplicate { hash });
        }

        // put() returns the evicted entry (if any) when the cache is full
        seen.put(hash, ());

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

    // -- Identity binding tests --
    //
    // The old TOFU model is gone (see docs/stake-pubkey-registry.md);
    // for non-privileged topics every signed message passes the
    // identity step. The registry-backed checks for privileged topics
    // are exercised below in
    // `privileged_topic_*` tests using a real in-memory DB.

    #[test]
    fn non_privileged_topic_accepts_any_consistent_key() {
        let key = test_key();
        let validator = MessageValidator::new();

        let msg1 = valid_message(&key, "/alexandria/catalog/1.0");
        assert!(validator.validate(&msg1).is_ok());

        // Different payload, same key + address — still passes (no
        // registry check on catalog).
        let msg2 = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"second\":true}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );
        assert!(validator.validate(&msg2).is_ok());
    }

    #[test]
    fn non_privileged_topic_accepts_different_keys_same_address() {
        // Pre-launch, no TOFU: anyone can sign catalog/profile/opinion
        // traffic. Authority is enforced at the registry layer for
        // privileged topics only.
        let key1 = test_key();
        let key2 = test_key();
        let validator = MessageValidator::new();
        let stake_address = "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4";

        let msg1 = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"first\":true}".to_vec(),
            &key1,
            stake_address,
        );
        assert!(validator.validate(&msg1).is_ok());

        let msg2 = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"second\":true}".to_vec(),
            &key2,
            stake_address,
        );
        // Catalog is non-privileged — passes despite different key.
        assert!(validator.validate(&msg2).is_ok());
    }

    #[test]
    fn different_stake_addresses_can_use_different_keys() {
        let key1 = test_key();
        let key2 = test_key();
        let validator = MessageValidator::new();

        let msg1 = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"user1\":true}".to_vec(),
            &key1,
            "stake_test1user1",
        );
        assert!(validator.validate(&msg1).is_ok());

        let msg2 = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"user2\":true}".to_vec(),
            &key2,
            "stake_test1user2",
        );
        assert!(validator.validate(&msg2).is_ok());
    }

    // -- Registry-backed privileged-topic tests --

    fn db_handle() -> Arc<Mutex<Option<Database>>> {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        Arc::new(Mutex::new(Some(db)))
    }

    #[test]
    fn privileged_topic_rejects_unregistered_key() {
        let key = test_key();
        let validator = MessageValidator::with_db(db_handle());
        let msg = sign_gossip_message(
            super::TOPIC_TAXONOMY,
            b"{\"version\":1}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );
        let res = validator.validate(&msg);
        assert!(
            matches!(res, Err(ValidationError::IdentityMismatch { .. })),
            "unregistered key on a privileged topic must be rejected: {res:?}"
        );
    }

    #[test]
    fn privileged_topic_accepts_registered_key() {
        use crate::p2p::registry;
        let key = test_key();
        let handle = db_handle();
        // Seed registry with a snapshot entry binding the stake address
        // to this key for an open-ended window.
        {
            let guard = handle.lock().unwrap();
            let db = guard.as_ref().unwrap();
            let pubkey_hex = hex::encode(key.verifying_key().to_bytes());
            registry::upsert_snapshot_entry(
                db.conn(),
                &registry::SnapshotEntry {
                    stake_address:
                        "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4".into(),
                    public_key_hex: pubkey_hex,
                    valid_from: 0,
                    valid_until: None,
                    on_chain_tx: None,
                },
                None,
            )
            .unwrap();
        }
        let validator = MessageValidator::with_db(handle);
        let msg = sign_gossip_message(
            super::TOPIC_TAXONOMY,
            b"{\"version\":1}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );
        assert!(validator.validate(&msg).is_ok());
    }

    #[test]
    fn privileged_topic_without_db_fails_open() {
        // `MessageValidator::new()` produces a validator without a DB
        // handle (legacy path / unit tests). Privileged-topic messages
        // pass the identity check there because the caller has opted
        // out — production wires a real DB via `with_db` and
        // `start_node_with_db` emits a startup WARN if anyone forgets.
        let key = test_key();
        let validator = MessageValidator::new();
        let msg = sign_gossip_message(
            super::TOPIC_TAXONOMY,
            b"{\"version\":1}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );
        assert!(validator.validate(&msg).is_ok());
    }

    #[test]
    fn privileged_topic_with_unlocked_profile_fails_closed() {
        // `with_db(Arc<Mutex<Option<Database>>>)` is the production
        // constructor. When the inner `Option` is `None` (profile
        // hasn't been unlocked yet) we MUST refuse privileged-topic
        // traffic — silently dropping it would be one mistake, but
        // silently *accepting* it without a registry check is a
        // worse one. The validator returns `Internal(...)` so the
        // event loop reports the message as Reject upstream.
        let key = test_key();
        let handle: Arc<Mutex<Option<Database>>> = Arc::new(Mutex::new(None));
        let validator = MessageValidator::with_db(handle);
        let msg = sign_gossip_message(
            super::TOPIC_TAXONOMY,
            b"{\"version\":1}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );
        let res = validator.validate(&msg);
        assert!(
            matches!(res, Err(ValidationError::Internal(_))),
            "privileged topic w/ no active profile must fail closed, got {res:?}"
        );
    }

    #[test]
    fn non_privileged_topic_with_unlocked_profile_passes() {
        // Catalog + the other open topics MUST keep working before a
        // profile is unlocked — the registry check is privileged-
        // only by design. This is the mirror of the test above and
        // documents why the gate lives where it does.
        let key = test_key();
        let handle: Arc<Mutex<Option<Database>>> = Arc::new(Mutex::new(None));
        let validator = MessageValidator::with_db(handle);
        let msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"id\":1}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );
        assert!(validator.validate(&msg).is_ok());
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
