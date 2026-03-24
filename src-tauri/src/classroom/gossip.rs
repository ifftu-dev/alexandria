use ed25519_dalek::SigningKey;

use crate::p2p::network::{NetworkError, P2pNode};
use crate::p2p::signing::sign_gossip_message;

use super::types::{
    classroom_message_topic, classroom_meta_topic, ClassroomMessagePayload, ClassroomMetaEvent,
};

/// Publish a classroom text message to the per-classroom gossip topic.
pub async fn publish_message(
    node: &P2pNode,
    classroom_id: &str,
    payload: &ClassroomMessagePayload,
    signing_key: &SigningKey,
    stake_address: &str,
) -> Result<(), NetworkError> {
    let topic = classroom_message_topic(classroom_id);
    let payload_bytes = serde_json::to_vec(payload)
        .map_err(|e| NetworkError::Publish(format!("serialize message: {e}")))?;
    let envelope = sign_gossip_message(&topic, payload_bytes, signing_key, stake_address);
    let data = serde_json::to_vec(&envelope)
        .map_err(|e| NetworkError::Publish(format!("serialize envelope: {e}")))?;
    node.publish(&topic, data).await
}

/// Publish a classroom meta/control event to the per-classroom meta topic.
pub async fn publish_meta(
    node: &P2pNode,
    classroom_id: &str,
    event: &ClassroomMetaEvent,
    signing_key: &SigningKey,
    stake_address: &str,
) -> Result<(), NetworkError> {
    let topic = classroom_meta_topic(classroom_id);
    let payload_bytes = serde_json::to_vec(event)
        .map_err(|e| NetworkError::Publish(format!("serialize meta event: {e}")))?;
    let envelope = sign_gossip_message(&topic, payload_bytes, signing_key, stake_address);
    let data = serde_json::to_vec(&envelope)
        .map_err(|e| NetworkError::Publish(format!("serialize envelope: {e}")))?;
    node.publish(&topic, data).await
}
