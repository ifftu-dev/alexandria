use super::network::{NetworkError, P2pNode};
use super::types::{
    SignedGossipMessage, TOPIC_CATALOG, TOPIC_EVIDENCE, TOPIC_GOVERNANCE, TOPIC_PROFILES,
    TOPIC_TAXONOMY,
};

/// High-level gossip operations for publishing typed messages.
///
/// These functions wrap the raw `P2pNode::publish()` with topic-specific
/// serialization. Message signing and validation is handled in a
/// subsequent PR — for now, messages are published as raw JSON.
impl P2pNode {
    /// Publish a course announcement to the catalog topic.
    pub async fn publish_catalog(&self, message: &SignedGossipMessage) -> Result<(), NetworkError> {
        let data = serde_json::to_vec(message)
            .map_err(|e| NetworkError::Publish(format!("serialize: {e}")))?;
        self.publish(TOPIC_CATALOG, data).await
    }

    /// Publish an evidence record to the evidence topic.
    pub async fn publish_evidence(
        &self,
        message: &SignedGossipMessage,
    ) -> Result<(), NetworkError> {
        let data = serde_json::to_vec(message)
            .map_err(|e| NetworkError::Publish(format!("serialize: {e}")))?;
        self.publish(TOPIC_EVIDENCE, data).await
    }

    /// Publish a taxonomy update to the taxonomy topic.
    pub async fn publish_taxonomy(
        &self,
        message: &SignedGossipMessage,
    ) -> Result<(), NetworkError> {
        let data = serde_json::to_vec(message)
            .map_err(|e| NetworkError::Publish(format!("serialize: {e}")))?;
        self.publish(TOPIC_TAXONOMY, data).await
    }

    /// Publish a governance announcement to the governance topic.
    pub async fn publish_governance(
        &self,
        message: &SignedGossipMessage,
    ) -> Result<(), NetworkError> {
        let data = serde_json::to_vec(message)
            .map_err(|e| NetworkError::Publish(format!("serialize: {e}")))?;
        self.publish(TOPIC_GOVERNANCE, data).await
    }

    /// Publish a profile CID announcement to the profiles topic.
    pub async fn publish_profile(
        &self,
        message: &SignedGossipMessage,
    ) -> Result<(), NetworkError> {
        let data = serde_json::to_vec(message)
            .map_err(|e| NetworkError::Publish(format!("serialize: {e}")))?;
        self.publish(TOPIC_PROFILES, data).await
    }
}
