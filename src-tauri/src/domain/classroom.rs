use serde::{Deserialize, Serialize};

/// A classroom — a persistent group space with channels, messaging, and live calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classroom {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_emoji: Option<String>,
    pub owner_address: String,
    pub invite_code: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    /// Populated by JOIN in list queries.
    pub member_count: Option<i64>,
    /// The local user's role in this classroom.
    pub my_role: Option<String>,
}

/// A member of a classroom.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassroomMember {
    pub classroom_id: String,
    pub stake_address: String,
    pub role: String,
    pub display_name: Option<String>,
    pub joined_at: String,
}

/// A text or announcement channel within a classroom.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassroomChannel {
    pub id: String,
    pub classroom_id: String,
    pub name: String,
    pub description: Option<String>,
    pub channel_type: String,
    pub position: i64,
    pub created_at: String,
}

/// A message in a classroom channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassroomMessage {
    pub id: String,
    pub channel_id: String,
    pub classroom_id: String,
    pub sender_address: String,
    pub sender_name: Option<String>,
    pub content: String,
    pub deleted: bool,
    pub edited_at: Option<String>,
    pub sent_at: String,
    pub received_at: String,
}

/// A join request from a user wanting to enter a classroom.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRequest {
    pub id: String,
    pub classroom_id: String,
    pub stake_address: String,
    pub display_name: Option<String>,
    pub message: Option<String>,
    pub status: String,
    pub reviewed_by: Option<String>,
    pub requested_at: String,
    pub reviewed_at: Option<String>,
}

/// An active or ended voice/video call in a classroom.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassroomCall {
    pub id: String,
    pub classroom_id: String,
    pub channel_id: Option<String>,
    pub title: String,
    pub ticket: Option<String>,
    pub started_by: String,
    pub status: String,
    pub started_at: String,
    pub ended_at: Option<String>,
}

// ── Request types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateClassroomRequest {
    pub name: String,
    pub description: Option<String>,
    pub icon_emoji: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub description: Option<String>,
    pub channel_type: Option<String>,
}
