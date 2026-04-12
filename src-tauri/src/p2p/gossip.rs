use ed25519_dalek::SigningKey;

use super::network::{NetworkError, P2pNode};
use super::signing::sign_gossip_message;
use super::types::{
    SignedGossipMessage, TOPIC_CATALOG, TOPIC_EVIDENCE, TOPIC_GOVERNANCE, TOPIC_OPINIONS,
    TOPIC_PROFILES, TOPIC_TAXONOMY,
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

    /// Publish an evidence record to the evidence topic.
    pub async fn publish_evidence(
        &self,
        payload: Vec<u8>,
        signing_key: &SigningKey,
        stake_address: &str,
    ) -> Result<(), NetworkError> {
        self.sign_and_publish(TOPIC_EVIDENCE, payload, signing_key, stake_address)
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
