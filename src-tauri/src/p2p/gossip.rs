use ed25519_dalek::SigningKey;

use super::network::{NetworkError, P2pNode};
use super::signing::sign_gossip_message;
use super::types::{
    SignedGossipMessage, TOPIC_CATALOG, TOPIC_GOVERNANCE, TOPIC_OPINIONS, TOPIC_PINBOARD,
    TOPIC_PROFILES, TOPIC_SENTINEL_PRIORS, TOPIC_TAXONOMY, TOPIC_VC_DID, TOPIC_VC_PRESENTATION,
    TOPIC_VC_STATUS,
};

/// High-level gossip operations for publishing typed messages.
///
/// These functions construct signed envelopes from raw payloads, then
/// serialize and publish them. The sender's Cardano signing key provides
/// the Ed25519 signature, and the stake address links the message to
/// an on-chain identity.
///
/// Each `publish_*` method:
/// 1. Wraps the payload in a `SignedGossipMessage` envelope (signed)
/// 2. Serializes the envelope to JSON bytes
/// 3. Publishes via the GossipSub topic
impl P2pNode {
    /// Publish a course announcement to the catalog topic.
    pub async fn publish_catalog(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_CATALOG, payload, signing_key, stake_address)
            .await
    }

    /// Publish a taxonomy update to the taxonomy topic.
    pub async fn publish_taxonomy(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_TAXONOMY, payload, signing_key, stake_address)
            .await
    }

    /// Publish a governance announcement to the governance topic.
    pub async fn publish_governance(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_GOVERNANCE, payload, signing_key, stake_address)
            .await
    }

    /// Publish a profile CID announcement to the profiles topic.
    pub async fn publish_profile(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_PROFILES, payload, signing_key, stake_address)
            .await
    }

    /// Publish a Field Commentary opinion to the opinions topic.
    pub async fn publish_opinion(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_OPINIONS, payload, signing_key, stake_address)
            .await
    }

    // ---- VC-first migration (PRs 9, 10, 11) -----------------------------

    /// Publish a DID document announcement or key-rotation record
    /// (§5.3). Receivers reflect into their local `key_registry` so
    /// historical verification works across peers.
    pub async fn publish_vc_did(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_VC_DID, payload, signing_key, stake_address)
            .await
    }

    /// Publish a revocation status list snapshot or delta (§11.2).
    pub async fn publish_vc_status(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_VC_STATUS, payload, signing_key, stake_address)
            .await
    }

    /// Publish a selective-disclosure presentation envelope (§18).
    pub async fn publish_vc_presentation(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_VC_PRESENTATION, payload, signing_key, stake_address)
            .await
    }

    /// Publish a PinBoard pinning commitment (§12 + §20.4).
    pub async fn publish_pinboard(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_PINBOARD, payload, signing_key, stake_address)
            .await
    }

    /// Publish a ratified Sentinel adversarial-prior announcement.
    /// Carried on a dedicated topic so peers can subscribe just to the
    /// Sentinel library without needing the full governance firehose.
    pub async fn publish_sentinel_prior(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_SENTINEL_PRIORS, payload, signing_key, stake_address)
            .await
    }

    /// Sign a payload, wrap it in an envelope, and publish to the topic.
    async fn sign_and_publish(
        &self,
        topic: &str,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        let envelope = sign_gossip_message(topic, payload, signing_key, stake_address);
        let data = serde_json::to_vec(&envelope)
            .map_err(|e| NetworkError::Publish(format!("serialize envelope: {e}")))?;
        self.publish(topic, data).await
    }

    /// Publish a pre-signed envelope (for advanced use cases where
    /// the caller has already constructed and signed the message).
    pub async fn publish_signed(&self, message: &SignedGossipMessage) -> Result<(), NetworkError> {
        let data = serde_json::to_vec(message)
            .map_err(|e| NetworkError::Publish(format!("serialize: {e}")))?;
        self.publish(&message.topic, data).await
    }
}
