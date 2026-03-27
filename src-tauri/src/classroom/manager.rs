use std::collections::HashSet;

use rusqlite::OptionalExtension;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

use super::types::{
    ClassroomMessageEvent, ClassroomMessageInfo, ClassroomMessagePayload, ClassroomMetaEvent,
    ClassroomMetaTauriEvent,
};

/// Manages the set of classroom topics the local node is subscribed to.
///
/// Does not own any background tasks — it is a lightweight registry.
/// All media/call delegation is handled by `TutoringManager`.
pub struct ClassroomManager {
    subscriptions: Mutex<HashSet<String>>,
}

impl Default for ClassroomManager {
    fn default() -> Self {
        Self {
            subscriptions: Mutex::new(HashSet::new()),
        }
    }
}

impl ClassroomManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn mark_subscribed(&self, classroom_id: &str) {
        self.subscriptions
            .lock()
            .await
            .insert(classroom_id.to_string());
    }

    pub async fn mark_unsubscribed(&self, classroom_id: &str) {
        self.subscriptions.lock().await.remove(classroom_id);
    }

    pub async fn is_subscribed(&self, classroom_id: &str) -> bool {
        self.subscriptions.lock().await.contains(classroom_id)
    }
}

fn classroom_role(db: &Database, classroom_id: &str, stake_address: &str) -> Option<String> {
    db.conn()
        .query_row(
            "SELECT role FROM classroom_members WHERE classroom_id = ?1 AND stake_address = ?2",
            rusqlite::params![classroom_id, stake_address],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
}

/// Handle an incoming gossip message on a classroom text channel topic.
///
/// Called from the P2P event consumer loop (DB lock is held by the caller).
/// Validates membership, persists the message, and emits a Tauri event.
pub fn handle_classroom_message(db: &Database, signed_msg: &SignedGossipMessage, app: &AppHandle) {
    let payload: ClassroomMessagePayload = match serde_json::from_slice(&signed_msg.payload) {
        Ok(p) => p,
        Err(e) => {
            log::debug!("[classroom] Invalid message payload: {e}");
            return;
        }
    };

    // Verify sender is a classroom member (authz gate)
    let is_member = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM classroom_members \
             WHERE classroom_id = ?1 AND stake_address = ?2",
            rusqlite::params![payload.classroom_id, signed_msg.stake_address],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if !is_member {
        log::debug!(
            "[classroom] Message from non-member {} for {}, dropping",
            signed_msg.stake_address,
            payload.classroom_id
        );
        return;
    }

    let local_address: Option<String> = db
        .conn()
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok();
    let local_is_member = local_address
        .as_deref()
        .and_then(|local| classroom_role(db, &payload.classroom_id, local))
        .is_some();

    if !local_is_member {
        log::debug!(
            "[classroom] Local node is not a member of {}, ignoring message",
            payload.classroom_id
        );
        return;
    }

    if payload.is_delete {
        let _ = db.conn().execute(
            "UPDATE classroom_messages SET deleted = 1 \
             WHERE id = ?1 AND classroom_id = ?2",
            rusqlite::params![payload.id, payload.classroom_id],
        );
        return;
    }

    // Convert Unix ms timestamp to ISO 8601
    let sent_at = chrono::DateTime::from_timestamp_millis(payload.sent_at as i64)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let result = db.conn().execute(
        "INSERT OR IGNORE INTO classroom_messages \
         (id, channel_id, classroom_id, sender_address, sender_name, content, sent_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            payload.id,
            payload.channel_id,
            payload.classroom_id,
            signed_msg.stake_address,
            payload.sender_name,
            payload.content,
            sent_at,
        ],
    );

    if let Err(e) = result {
        log::error!("[classroom] Failed to persist message: {e}");
        return;
    }

    let _ = app.emit(
        "classroom:message",
        ClassroomMessageEvent {
            classroom_id: payload.classroom_id.clone(),
            channel_id: payload.channel_id.clone(),
            message: ClassroomMessageInfo {
                id: payload.id,
                channel_id: payload.channel_id,
                classroom_id: payload.classroom_id,
                sender_address: signed_msg.stake_address.clone(),
                sender_name: payload.sender_name,
                content: payload.content,
                sent_at,
            },
        },
    );
}

/// Handle an incoming gossip message on a classroom meta topic.
///
/// Called from the P2P event consumer loop (DB lock is held by the caller).
/// Applies the membership/call state change and emits a Tauri event.
pub fn handle_classroom_meta(db: &Database, signed_msg: &SignedGossipMessage, app: &AppHandle) {
    let event: ClassroomMetaEvent = match serde_json::from_slice(&signed_msg.payload) {
        Ok(e) => e,
        Err(e) => {
            log::debug!("[classroom] Invalid meta payload: {e}");
            return;
        }
    };

    // Get local stake address for authz checks
    let local_address: Option<String> = db
        .conn()
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok();
    let classroom_id = event.classroom_id().to_string();
    let local_role = local_address
        .as_deref()
        .and_then(|local| classroom_role(db, &classroom_id, local));
    let local_is_member = local_role.is_some();
    let local_is_moderator = matches!(local_role.as_deref(), Some("owner") | Some("moderator"));
    let sender_role = classroom_role(db, &classroom_id, &signed_msg.stake_address);
    let sender_is_member = sender_role.is_some();
    let sender_is_moderator = matches!(sender_role.as_deref(), Some("owner") | Some("moderator"));
    let sender_is_owner = matches!(sender_role.as_deref(), Some("owner"));

    match &event {
        ClassroomMetaEvent::JoinRequest {
            classroom_id,
            request_id,
            display_name,
            message: request_message,
        } => {
            // Only persist if local user is a moderator/owner
            if local_is_moderator {
                let _ = db.conn().execute(
                    "INSERT OR IGNORE INTO classroom_join_requests \
                     (id, classroom_id, stake_address, display_name, message, status, requested_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, 'pending', datetime('now'))",
                    rusqlite::params![
                        request_id,
                        classroom_id,
                        signed_msg.stake_address,
                        display_name,
                        request_message,
                    ],
                );
            }
        }

        ClassroomMetaEvent::MemberApproved {
            classroom_id,
            stake_address,
            display_name,
        } => {
            let should_apply = sender_is_moderator
                && (local_is_member || local_address.as_deref() == Some(stake_address.as_str()));
            if !should_apply {
                return;
            }
            let _ = db.conn().execute(
                "INSERT OR IGNORE INTO classroom_members \
                 (classroom_id, stake_address, display_name, role, joined_at) \
                 VALUES (?1, ?2, ?3, 'member', datetime('now'))",
                rusqlite::params![classroom_id, stake_address, display_name],
            );
            let _ = db.conn().execute(
                "UPDATE classroom_join_requests \
                 SET status = 'approved', reviewed_at = datetime('now') \
                 WHERE classroom_id = ?1 AND stake_address = ?2 AND status = 'pending'",
                rusqlite::params![classroom_id, stake_address],
            );
        }

        ClassroomMetaEvent::MemberDenied {
            classroom_id,
            stake_address,
        } => {
            let should_apply = sender_is_moderator
                && (local_is_member || local_address.as_deref() == Some(stake_address.as_str()));
            if !should_apply {
                return;
            }
            let _ = db.conn().execute(
                "UPDATE classroom_join_requests \
                 SET status = 'denied', reviewed_at = datetime('now') \
                 WHERE classroom_id = ?1 AND stake_address = ?2 AND status = 'pending'",
                rusqlite::params![classroom_id, stake_address],
            );
        }

        ClassroomMetaEvent::MemberLeft {
            classroom_id,
            stake_address,
        } => {
            if signed_msg.stake_address != *stake_address
                || !(local_is_member || local_address.as_deref() == Some(stake_address.as_str()))
            {
                return;
            }
            let _ = db.conn().execute(
                "DELETE FROM classroom_members \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                rusqlite::params![classroom_id, stake_address],
            );
        }

        ClassroomMetaEvent::MemberKicked {
            classroom_id,
            stake_address,
        } => {
            if !sender_is_moderator
                || !(local_is_member || local_address.as_deref() == Some(stake_address.as_str()))
            {
                return;
            }
            let _ = db.conn().execute(
                "DELETE FROM classroom_members \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                rusqlite::params![classroom_id, stake_address],
            );
        }

        ClassroomMetaEvent::RoleChanged {
            classroom_id,
            stake_address,
            new_role,
        } => {
            if !sender_is_owner
                || !(local_is_member || local_address.as_deref() == Some(stake_address.as_str()))
            {
                return;
            }
            let _ = db.conn().execute(
                "UPDATE classroom_members SET role = ?3 \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                rusqlite::params![classroom_id, stake_address, new_role],
            );
        }

        ClassroomMetaEvent::CallStarted {
            classroom_id,
            call_id,
            ticket,
            started_by,
        } => {
            if !sender_is_member || !local_is_member {
                return;
            }
            let _ = db.conn().execute(
                "INSERT OR IGNORE INTO classroom_calls \
                 (id, classroom_id, title, ticket, started_by, status, started_at) \
                 VALUES (?1, ?2, 'Voice Call', ?3, ?4, 'active', datetime('now'))",
                rusqlite::params![call_id, classroom_id, ticket, started_by],
            );
        }

        ClassroomMetaEvent::CallEnded { call_id, .. } => {
            if !sender_is_member || !local_is_member {
                return;
            }
            let _ = db.conn().execute(
                "UPDATE classroom_calls SET status = 'ended', ended_at = datetime('now') \
                 WHERE id = ?1",
                rusqlite::params![call_id],
            );
        }

        ClassroomMetaEvent::KeyDistribution {
            classroom_id,
            stake_address,
            encrypted_group_key,
            key_version,
        } => {
            // Only process if this key distribution is for us
            if local_address.as_deref() != Some(stake_address.as_str()) {
                return;
            }
            // Store the encrypted group key locally
            let _ = db.conn().execute(
                "INSERT OR REPLACE INTO classroom_group_keys \
                 (classroom_id, group_key_enc, key_version, updated_at) \
                 VALUES (?1, ?2, ?3, datetime('now'))",
                rusqlite::params![classroom_id, encrypted_group_key.as_bytes(), key_version,],
            );
            log::info!("[classroom] Received group key v{key_version} for {classroom_id}");
        }
    }

    let event_type = event.event_type().to_string();
    let data = serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);

    let _ = app.emit(
        "classroom:meta",
        ClassroomMetaTauriEvent {
            classroom_id,
            event_type,
            data,
        },
    );
}
