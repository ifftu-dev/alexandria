use serde::{Deserialize, Serialize};

/// Returns the gossip topic for text messages in a classroom.
pub fn classroom_message_topic(classroom_id: &str) -> String {
    format!("/alexandria/classroom/{}/1.0", classroom_id)
}

/// Returns the gossip topic for meta/control events in a classroom.
pub fn classroom_meta_topic(classroom_id: &str) -> String {
    format!("/alexandria/classroom/{}/meta/1.0", classroom_id)
}

/// Returns true if the topic is a classroom message or meta topic.
pub fn is_classroom_topic(topic: &str) -> bool {
    topic.starts_with("/alexandria/classroom/")
}

/// Payload for a text message broadcast on the classroom message topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassroomMessagePayload {
    pub id: String,
    pub classroom_id: String,
    pub channel_id: String,
    pub content: String,
    pub sender_name: Option<String>,
    /// Unix timestamp in milliseconds.
    pub sent_at: u64,
    /// If true, this is a tombstone (soft-delete) for the message with this `id`.
    pub is_delete: bool,
    /// If true, `content` is base64-encoded ciphertext (AES-256-GCM with group key).
    #[serde(default)]
    pub encrypted: bool,
    /// The group key version used for encryption (for key rotation).
    #[serde(default)]
    pub key_version: u32,
}

/// A meta/control event broadcast on the classroom meta topic.
///
/// Used for membership management and call lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ClassroomMetaEvent {
    JoinRequest {
        classroom_id: String,
        request_id: String,
        display_name: Option<String>,
        message: Option<String>,
    },
    MemberApproved {
        classroom_id: String,
        stake_address: String,
        display_name: Option<String>,
    },
    MemberDenied {
        classroom_id: String,
        stake_address: String,
    },
    MemberLeft {
        classroom_id: String,
        stake_address: String,
    },
    MemberKicked {
        classroom_id: String,
        stake_address: String,
    },
    RoleChanged {
        classroom_id: String,
        stake_address: String,
        new_role: String,
    },
    CallStarted {
        classroom_id: String,
        call_id: String,
        ticket: String,
        started_by: String,
    },
    CallEnded {
        classroom_id: String,
        call_id: String,
    },
    /// Distribute an encrypted group key to a specific member.
    ///
    /// Sent by the owner/moderator when a member is approved or when
    /// the group key is rotated. The `encrypted_group_key` is encrypted
    /// via X25519 ECDH for the target member's public key.
    KeyDistribution {
        classroom_id: String,
        stake_address: String,
        /// Base64-encoded encrypted group key (nonce || ciphertext).
        encrypted_group_key: String,
        key_version: u32,
    },
}

impl ClassroomMetaEvent {
    /// Extract the classroom_id from any variant.
    pub fn classroom_id(&self) -> &str {
        match self {
            Self::JoinRequest { classroom_id, .. } => classroom_id,
            Self::MemberApproved { classroom_id, .. } => classroom_id,
            Self::MemberDenied { classroom_id, .. } => classroom_id,
            Self::MemberLeft { classroom_id, .. } => classroom_id,
            Self::MemberKicked { classroom_id, .. } => classroom_id,
            Self::RoleChanged { classroom_id, .. } => classroom_id,
            Self::CallStarted { classroom_id, .. } => classroom_id,
            Self::CallEnded { classroom_id, .. } => classroom_id,
            Self::KeyDistribution { classroom_id, .. } => classroom_id,
        }
    }

    /// Return the event type name as a string.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::JoinRequest { .. } => "JoinRequest",
            Self::MemberApproved { .. } => "MemberApproved",
            Self::MemberDenied { .. } => "MemberDenied",
            Self::MemberLeft { .. } => "MemberLeft",
            Self::MemberKicked { .. } => "MemberKicked",
            Self::RoleChanged { .. } => "RoleChanged",
            Self::CallStarted { .. } => "CallStarted",
            Self::CallEnded { .. } => "CallEnded",
            Self::KeyDistribution { .. } => "KeyDistribution",
        }
    }
}

// ── Tauri event payloads emitted to the webview ────────────────────

/// Emitted as `classroom:message` when a new message arrives via P2P.
#[derive(Debug, Clone, Serialize)]
pub struct ClassroomMessageEvent {
    pub classroom_id: String,
    pub channel_id: String,
    pub message: ClassroomMessageInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassroomMessageInfo {
    pub id: String,
    pub channel_id: String,
    pub classroom_id: String,
    pub sender_address: String,
    pub sender_name: Option<String>,
    pub content: String,
    pub sent_at: String,
}

/// Emitted as `classroom:meta` when a meta/control event arrives via P2P.
#[derive(Debug, Clone, Serialize)]
pub struct ClassroomMetaTauriEvent {
    pub classroom_id: String,
    pub event_type: String,
    pub data: serde_json::Value,
}
