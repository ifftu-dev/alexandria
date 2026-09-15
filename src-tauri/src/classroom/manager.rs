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

    pub async fn clear_subscriptions(&self) {
        self.subscriptions.lock().await.clear();
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

fn classroom_owner(db: &Database, classroom_id: &str) -> Option<String> {
    db.conn()
        .query_row(
            "SELECT owner_address FROM classrooms WHERE id = ?1",
            rusqlite::params![classroom_id],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
}

fn is_canonical_owner(owner_address: Option<&str>, stake_address: &str) -> bool {
    owner_address == Some(stake_address)
}

fn can_moderate(role: Option<&str>, canonical_owner: bool) -> bool {
    canonical_owner || role == Some("moderator")
}

fn is_assignable_role(role: &str) -> bool {
    matches!(role, "moderator" | "member")
}

fn apply_classroom_message(
    db: &Database,
    payload: &ClassroomMessagePayload,
    sender_address: &str,
    local_address: Option<&str>,
) -> Result<Option<String>, String> {
    let owner_address = classroom_owner(db, &payload.classroom_id);
    let sender_role = classroom_role(db, &payload.classroom_id, sender_address);
    let sender_is_owner = is_canonical_owner(owner_address.as_deref(), sender_address);
    if !sender_is_owner && sender_role.is_none() {
        return Ok(None);
    }
    let local_is_member = local_address.is_some_and(|local| {
        is_canonical_owner(owner_address.as_deref(), local)
            || classroom_role(db, &payload.classroom_id, local).is_some()
    });
    if !local_is_member {
        return Ok(None);
    }

    let channel_exists = db
        .conn()
        .query_row(
            "SELECT 1 FROM classroom_channels WHERE id = ?1 AND classroom_id = ?2",
            rusqlite::params![payload.channel_id, payload.classroom_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .is_some();
    if !channel_exists {
        return Ok(None);
    }

    if payload.is_delete {
        let original_sender = db
            .conn()
            .query_row(
                "SELECT sender_address FROM classroom_messages \
                 WHERE id = ?1 AND classroom_id = ?2 AND channel_id = ?3 AND deleted = 0",
                rusqlite::params![payload.id, payload.classroom_id, payload.channel_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let sender_can_moderate = can_moderate(sender_role.as_deref(), sender_is_owner);
        if original_sender.as_deref() != Some(sender_address) && !sender_can_moderate {
            return Ok(None);
        }
        db.conn()
            .execute(
                "UPDATE classroom_messages SET deleted = 1 \
                 WHERE id = ?1 AND classroom_id = ?2 AND channel_id = ?3 AND deleted = 0",
                rusqlite::params![payload.id, payload.classroom_id, payload.channel_id],
            )
            .map_err(|error| error.to_string())?;
        return Ok(None);
    }

    if payload.content.trim().is_empty() || payload.content.len() > 4000 {
        return Ok(None);
    }
    let sent_at = i64::try_from(payload.sent_at)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|timestamp| timestamp.to_rfc3339())
        .ok_or_else(|| "classroom message timestamp is outside the supported range".to_string())?;
    let inserted = db
        .conn()
        .execute(
            "INSERT OR IGNORE INTO classroom_messages \
             (id, channel_id, classroom_id, sender_address, sender_name, content, sent_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                payload.id,
                payload.channel_id,
                payload.classroom_id,
                sender_address,
                payload.sender_name,
                payload.content,
                sent_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok((inserted == 1).then_some(sent_at))
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

    let local_address: Option<String> = db
        .conn()
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok();
    let sent_at = match apply_classroom_message(
        db,
        &payload,
        &signed_msg.stake_address,
        local_address.as_deref(),
    ) {
        Ok(Some(sent_at)) => sent_at,
        Ok(None) => return,
        Err(error) => {
            log::error!("[classroom] Failed to apply message: {error}");
            return;
        }
    };

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

fn apply_classroom_meta(
    db: &Database,
    event: &ClassroomMetaEvent,
    sender_address: &str,
    local_address: Option<&str>,
) -> Result<bool, String> {
    let classroom_id = event.classroom_id().to_string();
    let owner_address = classroom_owner(db, &classroom_id);
    let local_role = local_address.and_then(|local| classroom_role(db, &classroom_id, local));
    let local_is_owner =
        local_address.is_some_and(|local| is_canonical_owner(owner_address.as_deref(), local));
    let local_is_member = local_is_owner || local_role.is_some();
    let local_is_moderator = can_moderate(local_role.as_deref(), local_is_owner);
    let sender_role = classroom_role(db, &classroom_id, sender_address);
    let sender_is_owner = is_canonical_owner(owner_address.as_deref(), sender_address);
    let sender_is_member = sender_is_owner || sender_role.is_some();
    let sender_is_moderator = can_moderate(sender_role.as_deref(), sender_is_owner);

    match event {
        ClassroomMetaEvent::JoinRequest {
            classroom_id,
            request_id,
            display_name,
            message: request_message,
        } => {
            if !local_is_moderator {
                return Ok(false);
            }
            db.conn()
                .execute(
                    "INSERT OR IGNORE INTO classroom_join_requests \
                     (id, classroom_id, stake_address, display_name, message, status, requested_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, 'pending', datetime('now'))",
                    rusqlite::params![
                        request_id,
                        classroom_id,
                        sender_address,
                        display_name,
                        request_message,
                    ],
                )
                .map(|changed| changed == 1)
                .map_err(|error| error.to_string())
        }
        ClassroomMetaEvent::MemberApproved {
            classroom_id,
            stake_address,
            display_name,
        } => {
            let should_apply = sender_is_moderator
                && (local_is_member || local_address == Some(stake_address.as_str()));
            if !should_apply {
                return Ok(false);
            }
            let tx = rusqlite::Transaction::new_unchecked(
                db.conn(),
                rusqlite::TransactionBehavior::Immediate,
            )
            .map_err(|error| error.to_string())?;
            let inserted = tx
                .execute(
                    "INSERT OR IGNORE INTO classroom_members \
                 (classroom_id, stake_address, display_name, role, joined_at) \
                 VALUES (?1, ?2, ?3, 'member', datetime('now'))",
                    rusqlite::params![classroom_id, stake_address, display_name],
                )
                .map_err(|error| error.to_string())?;
            let reviewed = tx
                .execute(
                    "UPDATE classroom_join_requests \
                 SET status = 'approved', reviewed_at = datetime('now') \
                 WHERE classroom_id = ?1 AND stake_address = ?2 AND status = 'pending'",
                    rusqlite::params![classroom_id, stake_address],
                )
                .map_err(|error| error.to_string())?;
            tx.commit().map_err(|error| error.to_string())?;
            Ok(inserted + reviewed > 0)
        }
        ClassroomMetaEvent::MemberDenied {
            classroom_id,
            stake_address,
        } => {
            let should_apply = sender_is_moderator
                && (local_is_member || local_address == Some(stake_address.as_str()));
            if !should_apply {
                return Ok(false);
            }
            db.conn()
                .execute(
                    "UPDATE classroom_join_requests \
                 SET status = 'denied', reviewed_at = datetime('now') \
                 WHERE classroom_id = ?1 AND stake_address = ?2 AND status = 'pending'",
                    rusqlite::params![classroom_id, stake_address],
                )
                .map(|changed| changed == 1)
                .map_err(|error| error.to_string())
        }
        ClassroomMetaEvent::MemberLeft {
            classroom_id,
            stake_address,
        } => {
            if sender_address != stake_address
                || is_canonical_owner(owner_address.as_deref(), stake_address)
                || !(local_is_member || local_address == Some(stake_address.as_str()))
            {
                return Ok(false);
            }
            db.conn()
                .execute(
                    "DELETE FROM classroom_members \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                    rusqlite::params![classroom_id, stake_address],
                )
                .map(|changed| changed == 1)
                .map_err(|error| error.to_string())
        }
        ClassroomMetaEvent::MemberKicked {
            classroom_id,
            stake_address,
        } => {
            if !sender_is_moderator
                || is_canonical_owner(owner_address.as_deref(), stake_address)
                || !(local_is_member || local_address == Some(stake_address.as_str()))
            {
                return Ok(false);
            }
            db.conn()
                .execute(
                    "DELETE FROM classroom_members \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                    rusqlite::params![classroom_id, stake_address],
                )
                .map(|changed| changed == 1)
                .map_err(|error| error.to_string())
        }
        ClassroomMetaEvent::RoleChanged {
            classroom_id,
            stake_address,
            new_role,
        } => {
            if !sender_is_owner
                || !is_assignable_role(new_role)
                || is_canonical_owner(owner_address.as_deref(), stake_address)
                || !(local_is_member || local_address == Some(stake_address.as_str()))
            {
                return Ok(false);
            }
            db.conn()
                .execute(
                    "UPDATE classroom_members SET role = ?3 \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                    rusqlite::params![classroom_id, stake_address, new_role],
                )
                .map(|changed| changed == 1)
                .map_err(|error| error.to_string())
        }
        ClassroomMetaEvent::CallStarted {
            classroom_id,
            call_id,
            ticket,
            started_by,
        } => {
            if !sender_is_member || !local_is_member || sender_address != started_by {
                return Ok(false);
            }
            db.conn()
                .execute(
                    "INSERT OR IGNORE INTO classroom_calls \
                 (id, classroom_id, title, ticket, started_by, status, started_at) \
                 VALUES (?1, ?2, 'Voice Call', ?3, ?4, 'active', datetime('now'))",
                    rusqlite::params![call_id, classroom_id, ticket, started_by],
                )
                .map(|changed| changed == 1)
                .map_err(|error| error.to_string())
        }
        ClassroomMetaEvent::CallEnded {
            classroom_id,
            call_id,
        } => {
            if !sender_is_member || !local_is_member {
                return Ok(false);
            }
            let started_by = db
                .conn()
                .query_row(
                    "SELECT started_by FROM classroom_calls \
                     WHERE id = ?1 AND classroom_id = ?2 AND status = 'active'",
                    rusqlite::params![call_id, classroom_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            if started_by.as_deref() != Some(sender_address) && !sender_is_moderator {
                return Ok(false);
            }
            db.conn()
                .execute(
                    "UPDATE classroom_calls SET status = 'ended', ended_at = datetime('now') \
                     WHERE id = ?1 AND classroom_id = ?2 AND status = 'active'",
                    rusqlite::params![call_id, classroom_id],
                )
                .map(|changed| changed == 1)
                .map_err(|error| error.to_string())
        }
        ClassroomMetaEvent::KeyDistribution {
            classroom_id,
            stake_address,
            encrypted_group_key,
            key_version,
        } => {
            if local_address != Some(stake_address.as_str()) {
                return Ok(false);
            }
            if !sender_is_moderator {
                log::warn!(
                    "[classroom] ignoring KeyDistribution for {classroom_id} from non-moderator {}",
                    sender_address
                );
                return Ok(false);
            }
            db.conn()
                .execute(
                    "INSERT INTO classroom_group_keys \
                     (classroom_id, group_key_enc, key_version, updated_at) \
                     VALUES (?1, ?2, ?3, datetime('now')) \
                     ON CONFLICT(classroom_id) DO UPDATE SET \
                       group_key_enc = excluded.group_key_enc, \
                       key_version = excluded.key_version, \
                       updated_at = datetime('now') \
                     WHERE excluded.key_version > classroom_group_keys.key_version",
                    rusqlite::params![classroom_id, encrypted_group_key.as_bytes(), key_version],
                )
                .map(|changed| changed == 1)
                .map_err(|error| error.to_string())
        }
    }
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

    let local_address: Option<String> = db
        .conn()
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok();
    match apply_classroom_meta(
        db,
        &event,
        &signed_msg.stake_address,
        local_address.as_deref(),
    ) {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            log::error!("[classroom] Failed to apply meta event: {error}");
            return;
        }
    }

    let classroom_id = event.classroom_id().to_string();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO classrooms (id, name, owner_address) \
                 VALUES ('classroom-1', 'Test', 'owner');
                 INSERT INTO classroom_members (classroom_id, stake_address, role) VALUES \
                 ('classroom-1', 'owner', 'owner'), \
                 ('classroom-1', 'moderator', 'moderator'), \
                 ('classroom-1', 'member-a', 'member'), \
                 ('classroom-1', 'member-b', 'member');
                 INSERT INTO classroom_channels (id, classroom_id, name) \
                 VALUES ('channel-1', 'classroom-1', 'general');",
            )
            .unwrap();
        db
    }

    fn message(
        id: &str,
        channel_id: &str,
        content: &str,
        is_delete: bool,
    ) -> ClassroomMessagePayload {
        ClassroomMessagePayload {
            id: id.into(),
            classroom_id: "classroom-1".into(),
            channel_id: channel_id.into(),
            content: content.into(),
            sender_name: None,
            sent_at: 1_700_000_000_000,
            is_delete,
            encrypted: false,
            key_version: 0,
        }
    }

    #[test]
    fn only_the_canonical_owner_or_a_moderator_can_moderate() {
        assert!(can_moderate(Some("member"), true));
        assert!(can_moderate(Some("moderator"), false));
        assert!(!can_moderate(Some("owner"), false));
        assert!(!can_moderate(Some("member"), false));
        assert!(!can_moderate(None, false));
    }

    #[test]
    fn generic_role_events_cannot_assign_owner() {
        assert!(is_assignable_role("member"));
        assert!(is_assignable_role("moderator"));
        assert!(!is_assignable_role("owner"));
        assert!(!is_assignable_role("administrator"));
    }

    #[test]
    fn incoming_call_events_bind_the_starter_and_ender() {
        let db = test_db();
        let spoofed = ClassroomMetaEvent::CallStarted {
            classroom_id: "classroom-1".into(),
            call_id: "call-1".into(),
            ticket: "ticket".into(),
            started_by: "owner".into(),
        };
        assert!(!apply_classroom_meta(&db, &spoofed, "member-a", Some("owner")).unwrap());

        let started = ClassroomMetaEvent::CallStarted {
            classroom_id: "classroom-1".into(),
            call_id: "call-1".into(),
            ticket: "ticket".into(),
            started_by: "member-a".into(),
        };
        assert!(apply_classroom_meta(&db, &started, "member-a", Some("owner")).unwrap());

        let ended = ClassroomMetaEvent::CallEnded {
            classroom_id: "classroom-1".into(),
            call_id: "call-1".into(),
        };
        assert!(!apply_classroom_meta(&db, &ended, "member-b", Some("owner")).unwrap());
        assert!(apply_classroom_meta(&db, &ended, "moderator", Some("owner")).unwrap());
        let status: String = db
            .conn()
            .query_row(
                "SELECT status FROM classroom_calls WHERE id = 'call-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "ended");
        assert!(!apply_classroom_meta(&db, &ended, "moderator", Some("owner")).unwrap());
    }

    #[test]
    fn incoming_approval_is_atomic_and_retryable() {
        let db = test_db();
        db.conn()
            .execute(
                "INSERT INTO classroom_join_requests (id, classroom_id, stake_address) \
                 VALUES ('request-1', 'classroom-1', 'learner')",
                [],
            )
            .unwrap();
        db.conn()
            .execute_batch(
                "CREATE TEMP TRIGGER fail_approval BEFORE UPDATE ON classroom_join_requests \
                 BEGIN SELECT RAISE(ABORT, 'injected approval failure'); END;",
            )
            .unwrap();
        let event = ClassroomMetaEvent::MemberApproved {
            classroom_id: "classroom-1".into(),
            stake_address: "learner".into(),
            display_name: Some("Learner".into()),
        };
        assert!(apply_classroom_meta(&db, &event, "owner", Some("owner")).is_err());
        let members: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM classroom_members WHERE stake_address = 'learner'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(members, 0);

        db.conn()
            .execute_batch("DROP TRIGGER fail_approval")
            .unwrap();
        assert!(apply_classroom_meta(&db, &event, "owner", Some("owner")).unwrap());
        assert!(!apply_classroom_meta(&db, &event, "owner", Some("owner")).unwrap());
    }

    #[test]
    fn incoming_group_key_replay_cannot_downgrade_the_version() {
        let db = test_db();
        let version_two = ClassroomMetaEvent::KeyDistribution {
            classroom_id: "classroom-1".into(),
            stake_address: "member-a".into(),
            encrypted_group_key: "new-key".into(),
            key_version: 2,
        };
        assert!(apply_classroom_meta(&db, &version_two, "moderator", Some("member-a")).unwrap());

        let version_one = ClassroomMetaEvent::KeyDistribution {
            classroom_id: "classroom-1".into(),
            stake_address: "member-a".into(),
            encrypted_group_key: "old-key".into(),
            key_version: 1,
        };
        assert!(!apply_classroom_meta(&db, &version_one, "moderator", Some("member-a")).unwrap());
        let (key, version): (Vec<u8>, i64) = db
            .conn()
            .query_row(
                "SELECT group_key_enc, key_version FROM classroom_group_keys \
                 WHERE classroom_id = 'classroom-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(key, b"new-key");
        assert_eq!(version, 2);
    }

    #[test]
    fn incoming_messages_bind_channels_and_ignore_conflicting_replays() {
        let db = test_db();
        let original = message("message-1", "channel-1", "original", false);
        assert!(
            apply_classroom_message(&db, &original, "member-a", Some("owner"))
                .unwrap()
                .is_some()
        );

        let conflicting = message("message-1", "channel-1", "replacement", false);
        assert!(
            apply_classroom_message(&db, &conflicting, "member-a", Some("owner"))
                .unwrap()
                .is_none()
        );
        let content: String = db
            .conn()
            .query_row(
                "SELECT content FROM classroom_messages WHERE id = 'message-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(content, "original");

        let wrong_channel = message("message-2", "missing-channel", "cross-scope", false);
        assert!(
            apply_classroom_message(&db, &wrong_channel, "member-a", Some("owner"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn incoming_message_deletion_requires_sender_or_moderator() {
        let db = test_db();
        let original = message("message-1", "channel-1", "original", false);
        apply_classroom_message(&db, &original, "member-a", Some("owner")).unwrap();
        let deletion = message("message-1", "channel-1", "", true);

        apply_classroom_message(&db, &deletion, "member-b", Some("owner")).unwrap();
        let deleted: i64 = db
            .conn()
            .query_row(
                "SELECT deleted FROM classroom_messages WHERE id = 'message-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(deleted, 0);

        apply_classroom_message(&db, &deletion, "moderator", Some("owner")).unwrap();
        let deleted: i64 = db
            .conn()
            .query_row(
                "SELECT deleted FROM classroom_messages WHERE id = 'message-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(deleted, 1);
    }

    #[test]
    fn incoming_message_rejects_unrepresentable_timestamps() {
        let db = test_db();
        let mut payload = message("message-1", "channel-1", "content", false);
        payload.sent_at = u64::MAX;
        assert!(apply_classroom_message(&db, &payload, "member-a", Some("owner")).is_err());
        let messages: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM classroom_messages", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(messages, 0);
    }
}
