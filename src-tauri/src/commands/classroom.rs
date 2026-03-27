//! Tauri commands for the Classrooms feature.
//!
//! Classrooms are persistent group spaces (like Discord servers) with
//! text channels, message history, join-request gating, and live A/V
//! calls (via iroh-live, delegated to TutoringManager).

use rusqlite::{params, OptionalExtension};
use tauri::{AppHandle, State};

use crate::classroom::gossip as classroom_gossip;
use crate::classroom::types::{
    classroom_message_topic, classroom_meta_topic, ClassroomMessagePayload, ClassroomMetaEvent,
};
use crate::crypto::hash::entity_id;
use crate::crypto::wallet;
use crate::domain::classroom::{
    Classroom, ClassroomCall, ClassroomChannel, ClassroomMember, ClassroomMessage, JoinRequest,
};
use crate::AppState;

// ── Helper: derive wallet + signing key from keystore ─────────────

async fn get_wallet(state: &AppState) -> Result<crate::crypto::wallet::Wallet, String> {
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("wallet is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    let mnemonic = mnemonic.clone();
    drop(ks_guard);
    wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())
}

fn classroom_role(
    db: &crate::db::Database,
    classroom_id: &str,
    stake_address: &str,
) -> Result<Option<String>, String> {
    db.conn()
        .query_row(
            "SELECT role FROM classroom_members WHERE classroom_id = ?1 AND stake_address = ?2",
            params![classroom_id, stake_address],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())
}

fn require_classroom_member(
    db: &crate::db::Database,
    classroom_id: &str,
    stake_address: &str,
) -> Result<String, String> {
    classroom_role(db, classroom_id, stake_address)?
        .ok_or_else(|| "you must be a classroom member to perform this action".to_string())
}

// ── Classroom CRUD ─────────────────────────────────────────────────

/// Create a new classroom and make the local user its owner.
#[tauri::command]
pub async fn classroom_create(
    name: String,
    description: Option<String>,
    icon_emoji: Option<String>,
    state: State<'_, AppState>,
) -> Result<Classroom, String> {
    let w = get_wallet(&state).await?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string();

    let id = entity_id(&[&w.stake_address.clone(), &name, &now_ms]);

    // Generate a random 8-char invite code
    let invite_code: String = {
        use std::time::SystemTime;
        let seed = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        format!("{:08X}", seed)
    };

    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    db.conn()
        .execute(
            "INSERT INTO classrooms (id, name, description, icon_emoji, owner_address, invite_code) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, name, description, icon_emoji, w.stake_address.clone(), invite_code],
        )
        .map_err(|e| e.to_string())?;

    // Owner is automatically a member
    db.conn()
        .execute(
            "INSERT INTO classroom_members (classroom_id, stake_address, role, display_name) \
             VALUES (?1, ?2, 'owner', ?3)",
            params![
                id,
                w.stake_address.clone(),
                /* display_name */ Option::<String>::None
            ],
        )
        .map_err(|e| e.to_string())?;

    // Create default #general channel
    let channel_id = entity_id(&[&id, "general"]);
    db.conn()
        .execute(
            "INSERT INTO classroom_channels (id, classroom_id, name, channel_type, position) \
             VALUES (?1, ?2, 'general', 'text', 0)",
            params![channel_id, id],
        )
        .map_err(|e| e.to_string())?;

    db.conn()
        .query_row(
            "SELECT c.id, c.name, c.description, c.icon_emoji, c.owner_address, \
                    c.invite_code, c.status, c.created_at, c.updated_at, \
                    COUNT(m.stake_address) AS member_count, \
                    MAX(CASE WHEN m.stake_address = ?2 THEN m.role END) AS my_role
             FROM classrooms c
             LEFT JOIN classroom_members m ON m.classroom_id = c.id
             WHERE c.id = ?1
             GROUP BY c.id",
            params![id, w.stake_address],
            map_classroom_row,
        )
        .map_err(|e| e.to_string())
}

/// List all classrooms the local user is a member of.
#[tauri::command]
pub async fn classroom_list(state: State<'_, AppState>) -> Result<Vec<Classroom>, String> {
    let w = get_wallet(&state).await?;
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT c.id, c.name, c.description, c.icon_emoji, c.owner_address, \
                c.invite_code, c.status, c.created_at, c.updated_at, \
                COUNT(m2.stake_address) AS member_count, \
                me.role AS my_role
         FROM classrooms c
         JOIN classroom_members me ON me.classroom_id = c.id AND me.stake_address = ?1
         LEFT JOIN classroom_members m2 ON m2.classroom_id = c.id
         WHERE c.status = 'active'
         GROUP BY c.id
         ORDER BY c.name",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![w.stake_address], |row| {
            Ok(Classroom {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                icon_emoji: row.get(3)?,
                owner_address: row.get(4)?,
                invite_code: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                member_count: row.get(9)?,
                my_role: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Get a single classroom by ID.
#[tauri::command]
pub async fn classroom_get(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<Classroom, String> {
    let w = get_wallet(&state).await?;
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    db.conn()
        .query_row(
            "SELECT c.id, c.name, c.description, c.icon_emoji, c.owner_address, \
                    c.invite_code, c.status, c.created_at, c.updated_at, \
                    COUNT(m.stake_address) AS member_count, \
                    MAX(CASE WHEN m.stake_address = ?2 THEN m.role END) AS my_role
             FROM classrooms c
             LEFT JOIN classroom_members m ON m.classroom_id = c.id
             WHERE c.id = ?1
             GROUP BY c.id",
            params![classroom_id, w.stake_address],
            map_classroom_row,
        )
        .map_err(|e| e.to_string())
}

/// Archive (soft-delete) a classroom. Owner only.
#[tauri::command]
pub async fn classroom_archive(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let rows = db
        .conn()
        .execute(
            "UPDATE classrooms SET status = 'archived', updated_at = datetime('now') \
             WHERE id = ?1 AND owner_address = ?2",
            params![classroom_id, w.stake_address],
        )
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err("classroom not found or you are not the owner".to_string());
    }
    Ok(())
}

fn map_classroom_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Classroom> {
    Ok(Classroom {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        icon_emoji: row.get(3)?,
        owner_address: row.get(4)?,
        invite_code: row.get(5)?,
        status: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        member_count: row.get(9)?,
        my_role: row.get(10)?,
    })
}

// ── Membership ─────────────────────────────────────────────────────

/// List members of a classroom.
#[tauri::command]
pub async fn classroom_list_members(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ClassroomMember>, String> {
    let w = get_wallet(&state).await?;
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let _ = require_classroom_member(db, &classroom_id, &w.stake_address)?;

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT classroom_id, stake_address, role, display_name, joined_at \
             FROM classroom_members WHERE classroom_id = ?1 ORDER BY role, display_name",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![classroom_id], |row| {
            Ok(ClassroomMember {
                classroom_id: row.get(0)?,
                stake_address: row.get(1)?,
                role: row.get(2)?,
                display_name: row.get(3)?,
                joined_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Submit a request to join a classroom (broadcasts via P2P).
#[tauri::command]
pub async fn classroom_request_join(
    classroom_id: String,
    message: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;

    // Generate a request_id
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string();
    let request_id = entity_id(&[&classroom_id, &w.stake_address.clone(), &now_ms]);

    // Persist locally as pending
    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO classroom_join_requests \
                 (id, classroom_id, stake_address, message, status, requested_at) \
                 VALUES (?1, ?2, ?3, ?4, 'pending', datetime('now'))",
                params![request_id, classroom_id, w.stake_address.clone(), message],
            )
            .map_err(|e| e.to_string())?;
    }

    // Subscribe to classroom topics so we can receive approval
    {
        let node_lock = state.p2p_node.lock().await;
        if let Some(node) = node_lock.as_ref() {
            let _ = node
                .subscribe_topic(&classroom_message_topic(&classroom_id))
                .await;
            let _ = node
                .subscribe_topic(&classroom_meta_topic(&classroom_id))
                .await;
        }
    }

    // Broadcast the join request
    {
        let node_lock = state.p2p_node.lock().await;
        if let Some(node) = node_lock.as_ref() {
            let event = ClassroomMetaEvent::JoinRequest {
                classroom_id: classroom_id.clone(),
                request_id,
                display_name: None,
                message,
            };
            let _ = classroom_gossip::publish_meta(
                node,
                &classroom_id,
                &event,
                &w.signing_key,
                &w.stake_address.clone(),
            )
            .await;
        }
    }

    Ok(())
}

/// Approve a pending join request (owner/moderator only). Broadcasts via P2P.
#[tauri::command]
pub async fn classroom_approve_member(
    classroom_id: String,
    stake_address: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;

    // Verify local user is moderator/owner
    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let role: Option<String> = db
            .conn()
            .query_row(
                "SELECT role FROM classroom_members \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                params![classroom_id, w.stake_address],
                |row| row.get(0),
            )
            .ok();
        match role.as_deref() {
            Some("owner") | Some("moderator") => {}
            _ => return Err("you must be a moderator or owner to approve members".to_string()),
        }

        // Upsert the member
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO classroom_members \
                 (classroom_id, stake_address, role, joined_at) \
                 VALUES (?1, ?2, 'member', datetime('now'))",
                params![classroom_id, stake_address],
            )
            .map_err(|e| e.to_string())?;

        // Update request status
        db.conn()
            .execute(
                "UPDATE classroom_join_requests \
                 SET status = 'approved', reviewed_by = ?3, reviewed_at = datetime('now') \
                 WHERE classroom_id = ?1 AND stake_address = ?2 AND status = 'pending'",
                params![classroom_id, stake_address, w.stake_address],
            )
            .map_err(|e| e.to_string())?;
    }

    // Broadcast approval
    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        let event = ClassroomMetaEvent::MemberApproved {
            classroom_id: classroom_id.clone(),
            stake_address,
            display_name: None,
        };
        let _ = classroom_gossip::publish_meta(
            node,
            &classroom_id,
            &event,
            &w.signing_key,
            &w.stake_address.clone(),
        )
        .await;
    }

    Ok(())
}

/// Deny a pending join request (owner/moderator only). Broadcasts via P2P.
#[tauri::command]
pub async fn classroom_deny_member(
    classroom_id: String,
    stake_address: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;

    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let role: Option<String> = db
            .conn()
            .query_row(
                "SELECT role FROM classroom_members \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                params![classroom_id, w.stake_address],
                |row| row.get(0),
            )
            .ok();
        match role.as_deref() {
            Some("owner") | Some("moderator") => {}
            _ => return Err("you must be a moderator or owner to deny members".to_string()),
        }

        db.conn()
            .execute(
                "UPDATE classroom_join_requests \
                 SET status = 'denied', reviewed_by = ?3, reviewed_at = datetime('now') \
                 WHERE classroom_id = ?1 AND stake_address = ?2 AND status = 'pending'",
                params![classroom_id, stake_address, w.stake_address],
            )
            .map_err(|e| e.to_string())?;
    }

    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        let event = ClassroomMetaEvent::MemberDenied {
            classroom_id: classroom_id.clone(),
            stake_address,
        };
        let _ = classroom_gossip::publish_meta(
            node,
            &classroom_id,
            &event,
            &w.signing_key,
            &w.stake_address.clone(),
        )
        .await;
    }

    Ok(())
}

/// Leave a classroom. Broadcasts a MemberLeft event and unsubscribes.
#[tauri::command]
pub async fn classroom_leave(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;

    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;

        // Owners must transfer ownership or archive before leaving
        let is_owner: bool = db
            .conn()
            .query_row(
                "SELECT role FROM classroom_members WHERE classroom_id = ?1 AND stake_address = ?2",
                params![classroom_id, w.stake_address],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .map(|r| r == "owner")
            .unwrap_or(false);

        if is_owner {
            return Err(
                "owners cannot leave — archive the classroom or transfer ownership first"
                    .to_string(),
            );
        }

        db.conn()
            .execute(
                "DELETE FROM classroom_members WHERE classroom_id = ?1 AND stake_address = ?2",
                params![classroom_id, w.stake_address],
            )
            .map_err(|e| e.to_string())?;
    }

    // Broadcast leave and unsubscribe
    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        let event = ClassroomMetaEvent::MemberLeft {
            classroom_id: classroom_id.clone(),
            stake_address: w.stake_address.clone(),
        };
        let _ = classroom_gossip::publish_meta(
            node,
            &classroom_id,
            &event,
            &w.signing_key,
            &w.stake_address.clone(),
        )
        .await;
        let _ = node
            .unsubscribe_topic(&classroom_message_topic(&classroom_id))
            .await;
        let _ = node
            .unsubscribe_topic(&classroom_meta_topic(&classroom_id))
            .await;
    }

    state.classroom.mark_unsubscribed(&classroom_id).await;
    Ok(())
}

/// Kick a member from a classroom (moderator/owner only).
#[tauri::command]
pub async fn classroom_kick_member(
    classroom_id: String,
    stake_address: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;

    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let role: Option<String> = db
            .conn()
            .query_row(
                "SELECT role FROM classroom_members \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                params![classroom_id, w.stake_address],
                |row| row.get(0),
            )
            .ok();
        match role.as_deref() {
            Some("owner") | Some("moderator") => {}
            _ => return Err("you must be a moderator or owner to kick members".to_string()),
        }

        db.conn()
            .execute(
                "DELETE FROM classroom_members WHERE classroom_id = ?1 AND stake_address = ?2",
                params![classroom_id, stake_address],
            )
            .map_err(|e| e.to_string())?;
    }

    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        let event = ClassroomMetaEvent::MemberKicked {
            classroom_id: classroom_id.clone(),
            stake_address,
        };
        let _ = classroom_gossip::publish_meta(
            node,
            &classroom_id,
            &event,
            &w.signing_key,
            &w.stake_address.clone(),
        )
        .await;
    }

    Ok(())
}

/// Set a member's role (owner only).
#[tauri::command]
pub async fn classroom_set_role(
    classroom_id: String,
    stake_address: String,
    role: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if !matches!(role.as_str(), "owner" | "moderator" | "member") {
        return Err("invalid role — must be 'owner', 'moderator', or 'member'".to_string());
    }

    let w = get_wallet(&state).await?;

    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let local_role: Option<String> = db
            .conn()
            .query_row(
                "SELECT role FROM classroom_members \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                params![classroom_id, w.stake_address],
                |row| row.get(0),
            )
            .ok();
        if local_role.as_deref() != Some("owner") {
            return Err("only the owner can change roles".to_string());
        }

        db.conn()
            .execute(
                "UPDATE classroom_members SET role = ?3 \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                params![classroom_id, stake_address, role],
            )
            .map_err(|e| e.to_string())?;
    }

    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        let event = ClassroomMetaEvent::RoleChanged {
            classroom_id: classroom_id.clone(),
            stake_address,
            new_role: role,
        };
        let _ = classroom_gossip::publish_meta(
            node,
            &classroom_id,
            &event,
            &w.signing_key,
            &w.stake_address.clone(),
        )
        .await;
    }

    Ok(())
}

/// List pending join requests for a classroom (moderator/owner only).
#[tauri::command]
pub async fn classroom_list_join_requests(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<JoinRequest>, String> {
    let w = get_wallet(&state).await?;
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    // Verify access
    let role: Option<String> = db
        .conn()
        .query_row(
            "SELECT role FROM classroom_members WHERE classroom_id = ?1 AND stake_address = ?2",
            params![classroom_id, w.stake_address],
            |row| row.get(0),
        )
        .ok();
    match role.as_deref() {
        Some("owner") | Some("moderator") => {}
        _ => return Err("you must be a moderator or owner to view join requests".to_string()),
    }

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, classroom_id, stake_address, display_name, message, \
                    status, reviewed_by, requested_at, reviewed_at \
             FROM classroom_join_requests \
             WHERE classroom_id = ?1 AND status = 'pending' \
             ORDER BY requested_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![classroom_id], |row| {
            Ok(JoinRequest {
                id: row.get(0)?,
                classroom_id: row.get(1)?,
                stake_address: row.get(2)?,
                display_name: row.get(3)?,
                message: row.get(4)?,
                status: row.get(5)?,
                reviewed_by: row.get(6)?,
                requested_at: row.get(7)?,
                reviewed_at: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

// ── Channels ───────────────────────────────────────────────────────

/// List channels in a classroom.
#[tauri::command]
pub async fn classroom_list_channels(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ClassroomChannel>, String> {
    let w = get_wallet(&state).await?;
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let _ = require_classroom_member(db, &classroom_id, &w.stake_address)?;

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, classroom_id, name, description, channel_type, position, created_at \
             FROM classroom_channels WHERE classroom_id = ?1 ORDER BY position, name",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![classroom_id], |row| {
            Ok(ClassroomChannel {
                id: row.get(0)?,
                classroom_id: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                channel_type: row.get(4)?,
                position: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Create a new channel (moderator/owner only).
#[tauri::command]
pub async fn classroom_create_channel(
    classroom_id: String,
    name: String,
    description: Option<String>,
    channel_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<ClassroomChannel, String> {
    let w = get_wallet(&state).await?;
    let channel_type = channel_type.unwrap_or_else(|| "text".to_string());

    if !matches!(channel_type.as_str(), "text" | "announcement") {
        return Err("channel_type must be 'text' or 'announcement'".to_string());
    }

    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let role: Option<String> = db
        .conn()
        .query_row(
            "SELECT role FROM classroom_members WHERE classroom_id = ?1 AND stake_address = ?2",
            params![classroom_id, w.stake_address],
            |row| row.get(0),
        )
        .ok();
    match role.as_deref() {
        Some("owner") | Some("moderator") => {}
        _ => return Err("only moderators and owners can create channels".to_string()),
    }

    let id = entity_id(&[&classroom_id, &name]);

    let position: i64 = db
        .conn()
        .query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM classroom_channels WHERE classroom_id = ?1",
            params![classroom_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    db.conn()
        .execute(
            "INSERT OR IGNORE INTO classroom_channels \
             (id, classroom_id, name, description, channel_type, position) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, classroom_id, name, description, channel_type, position],
        )
        .map_err(|e| e.to_string())?;

    db.conn()
        .query_row(
            "SELECT id, classroom_id, name, description, channel_type, position, created_at \
             FROM classroom_channels WHERE id = ?1",
            params![id],
            |row| {
                Ok(ClassroomChannel {
                    id: row.get(0)?,
                    classroom_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    channel_type: row.get(4)?,
                    position: row.get(5)?,
                    created_at: row.get(6)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}

/// Delete a channel (owner only).
#[tauri::command]
pub async fn classroom_delete_channel(
    channel_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    // Only owner can delete channels (cascades to messages)
    let rows = db
        .conn()
        .execute(
            "DELETE FROM classroom_channels WHERE id = ?1 AND classroom_id IN \
             (SELECT id FROM classrooms WHERE owner_address = ?2)",
            params![channel_id, w.stake_address],
        )
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err("channel not found or you are not the owner".to_string());
    }
    Ok(())
}

// ── Messages ───────────────────────────────────────────────────────

/// Get message history for a channel (paginated).
#[tauri::command]
pub async fn classroom_get_messages(
    channel_id: String,
    before_id: Option<String>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<ClassroomMessage>, String> {
    let w = get_wallet(&state).await?;
    let limit = limit.unwrap_or(50).min(200);
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let classroom_id: String = db
        .conn()
        .query_row(
            "SELECT classroom_id FROM classroom_channels WHERE id = ?1",
            params![channel_id],
            |row| row.get(0),
        )
        .map_err(|_| "channel not found".to_string())?;
    let _ = require_classroom_member(db, &classroom_id, &w.stake_address)?;

    let rows = if let Some(ref bid) = before_id {
        // Cursor-based pagination: messages sent before the given message
        let cursor_sent_at: Option<String> = db
            .conn()
            .query_row(
                "SELECT sent_at FROM classroom_messages WHERE id = ?1",
                params![bid],
                |row| row.get(0),
            )
            .ok();

        if let Some(cursor) = cursor_sent_at {
            let mut stmt = db
                .conn()
                .prepare(
                    "SELECT id, channel_id, classroom_id, sender_address, sender_name, \
                            content, deleted, edited_at, sent_at, received_at \
                     FROM classroom_messages \
                     WHERE channel_id = ?1 AND sent_at < ?2 AND deleted = 0 \
                     ORDER BY sent_at DESC LIMIT ?3",
                )
                .map_err(|e| e.to_string())?;

            // Collect immediately so the borrow of `stmt` ends before it's dropped.
            let raw: Vec<rusqlite::Result<ClassroomMessage>> = stmt
                .query_map(params![channel_id, cursor, limit], map_message_row)
                .map_err(|e| e.to_string())?
                .collect();
            raw.into_iter()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        } else {
            vec![]
        }
    } else {
        let mut stmt = db
            .conn()
            .prepare(
                "SELECT id, channel_id, classroom_id, sender_address, sender_name, \
                        content, deleted, edited_at, sent_at, received_at \
                 FROM classroom_messages \
                 WHERE channel_id = ?1 AND deleted = 0 \
                 ORDER BY sent_at DESC LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let raw: Vec<rusqlite::Result<ClassroomMessage>> = stmt
            .query_map(params![channel_id, limit], map_message_row)
            .map_err(|e| e.to_string())?
            .collect();
        raw.into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };

    // Return in chronological order
    let mut messages = rows;
    messages.reverse();
    Ok(messages)
}

fn map_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClassroomMessage> {
    Ok(ClassroomMessage {
        id: row.get(0)?,
        channel_id: row.get(1)?,
        classroom_id: row.get(2)?,
        sender_address: row.get(3)?,
        sender_name: row.get(4)?,
        content: row.get(5)?,
        deleted: row.get::<_, i64>(6)? != 0,
        edited_at: row.get(7)?,
        sent_at: row.get(8)?,
        received_at: row.get(9)?,
    })
}

/// Send a text message to a classroom channel (broadcasts via P2P).
#[tauri::command]
pub async fn classroom_send_message(
    channel_id: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<ClassroomMessage, String> {
    if content.trim().is_empty() {
        return Err("message content cannot be empty".to_string());
    }
    if content.len() > 4000 {
        return Err("message content too long (max 4000 characters)".to_string());
    }

    let w = get_wallet(&state).await?;

    let classroom_id = {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;

        let (cid, ctype): (String, String) = db
            .conn()
            .query_row(
                "SELECT classroom_id, channel_type FROM classroom_channels WHERE id = ?1",
                params![channel_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "channel not found".to_string())?;

        let role = require_classroom_member(db, &cid, &w.stake_address)?;

        // For announcement channels, only moderators/owners can post
        if ctype == "announcement" {
            match role.as_str() {
                "owner" | "moderator" => {}
                _ => {
                    return Err(
                        "only moderators and owners can post in announcement channels".to_string(),
                    )
                }
            }
        }

        cid
    };

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let id = entity_id(&[&channel_id, &w.stake_address.clone(), &now_ms.to_string()]);
    let sent_at = chrono::DateTime::from_timestamp_millis(now_ms as i64)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    // Persist locally
    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .execute(
                "INSERT INTO classroom_messages \
                 (id, channel_id, classroom_id, sender_address, content, sent_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    channel_id,
                    classroom_id,
                    w.stake_address.clone(),
                    content,
                    sent_at
                ],
            )
            .map_err(|e| e.to_string())?;
    }

    // Broadcast via P2P
    {
        let node_lock = state.p2p_node.lock().await;
        if let Some(node) = node_lock.as_ref() {
            // Encrypt the message content if a group key is available.
            // Get the content key first (async), then lock DB briefly (sync).
            let content_key = state.content_node.content_key().await;
            let (msg_content, is_encrypted, kv) = {
                let encrypted_gk = {
                    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
                    db_guard.as_ref().and_then(|db| {
                        db.conn().query_row(
                            "SELECT group_key_enc, key_version FROM classroom_group_keys WHERE classroom_id = ?1",
                            rusqlite::params![classroom_id],
                            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i32>(1)?)),
                        ).ok()
                    })
                }; // db_guard dropped

                match (encrypted_gk, content_key) {
                    (Some((enc_key, version)), Some(ck)) => {
                        match crate::crypto::content_crypto::decrypt(&ck, &enc_key) {
                            Ok(Some(gk_bytes)) if gk_bytes.len() == 32 => {
                                let mut gk = [0u8; 32];
                                gk.copy_from_slice(&gk_bytes);
                                match crate::crypto::group_key::encrypt_message(
                                    &gk,
                                    content.as_bytes(),
                                ) {
                                    Ok(ct) => {
                                        use base64::Engine;
                                        let encoded =
                                            base64::engine::general_purpose::STANDARD.encode(&ct);
                                        (encoded, true, version as u32)
                                    }
                                    Err(_) => (content.clone(), false, 0),
                                }
                            }
                            _ => (content.clone(), false, 0),
                        }
                    }
                    _ => (content.clone(), false, 0),
                }
            };

            let payload = ClassroomMessagePayload {
                id: id.clone(),
                classroom_id: classroom_id.clone(),
                channel_id: channel_id.clone(),
                content: msg_content,
                sender_name: None,
                sent_at: now_ms as u64,
                is_delete: false,
                encrypted: is_encrypted,
                key_version: kv,
            };
            let _ = classroom_gossip::publish_message(
                node,
                &classroom_id,
                &payload,
                &w.signing_key,
                &w.stake_address.clone(),
            )
            .await;
        }
    }

    Ok(ClassroomMessage {
        id,
        channel_id,
        classroom_id,
        sender_address: w.stake_address.clone(),
        sender_name: None,
        content,
        deleted: false,
        edited_at: None,
        sent_at,
        received_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Soft-delete a message (sender or moderator/owner).
#[tauri::command]
pub async fn classroom_delete_message(
    message_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;

    let (channel_id, classroom_id, sender) = {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT channel_id, classroom_id, sender_address \
                 FROM classroom_messages WHERE id = ?1",
                params![message_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(|_| "message not found".to_string())?
    };

    // Sender can always delete their own; moderators/owners can delete any
    if sender != w.stake_address {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let role: Option<String> = db
            .conn()
            .query_row(
                "SELECT role FROM classroom_members \
                 WHERE classroom_id = ?1 AND stake_address = ?2",
                params![classroom_id, w.stake_address],
                |row| row.get(0),
            )
            .ok();
        match role.as_deref() {
            Some("owner") | Some("moderator") => {}
            _ => return Err("you can only delete your own messages".to_string()),
        }
    }

    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .execute(
                "UPDATE classroom_messages SET deleted = 1 WHERE id = ?1",
                params![message_id],
            )
            .map_err(|e| e.to_string())?;
    }

    // Broadcast tombstone
    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        let payload = ClassroomMessagePayload {
            id: message_id,
            classroom_id: classroom_id.clone(),
            channel_id,
            content: String::new(),
            sender_name: None,
            sent_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            is_delete: true,
            encrypted: false,
            key_version: 0,
        };
        let _ = classroom_gossip::publish_message(
            node,
            &classroom_id,
            &payload,
            &w.signing_key,
            &w.stake_address.clone(),
        )
        .await;
    }

    Ok(())
}

// ── P2P Subscription ───────────────────────────────────────────────

/// Subscribe to P2P gossip topics for a classroom.
///
/// Must be called after joining / entering a classroom to receive
/// real-time messages and membership events.
#[tauri::command]
pub async fn classroom_subscribe(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;
    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let _ = require_classroom_member(db, &classroom_id, &w.stake_address)?;
    }

    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        node.subscribe_topic(&classroom_message_topic(&classroom_id))
            .await
            .map_err(|e| e.to_string())?;
        node.subscribe_topic(&classroom_meta_topic(&classroom_id))
            .await
            .map_err(|e| e.to_string())?;
    }
    state.classroom.mark_subscribed(&classroom_id).await;
    Ok(())
}

/// Unsubscribe from P2P gossip topics for a classroom.
#[tauri::command]
pub async fn classroom_unsubscribe(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        let _ = node
            .unsubscribe_topic(&classroom_message_topic(&classroom_id))
            .await;
        let _ = node
            .unsubscribe_topic(&classroom_meta_topic(&classroom_id))
            .await;
    }
    state.classroom.mark_unsubscribed(&classroom_id).await;
    Ok(())
}

// ── Live Calls ─────────────────────────────────────────────────────

/// Start a voice/video call in a classroom (desktop only).
///
/// Creates an iroh-live room and broadcasts the ticket to all members
/// via the meta gossip topic so they can join.
#[tauri::command]
#[cfg(desktop)]
#[allow(clippy::too_many_arguments)]
pub async fn classroom_start_call(
    classroom_id: String,
    channel_id: Option<String>,
    display_name: Option<String>,
    camera_id: Option<String>,
    mic_id: Option<String>,
    speaker_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ClassroomCall, String> {
    let w = get_wallet(&state).await?;
    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let _ = require_classroom_member(db, &classroom_id, &w.stake_address)?;
    }

    let content_node = &state.content_node;
    let endpoint = content_node
        .endpoint()
        .await
        .ok_or("iroh node not running")?;
    let gossip = content_node.gossip().await.ok_or("gossip not available")?;
    let live = content_node.live().await.ok_or("live not available")?;

    let call_id = uuid::Uuid::new_v4().to_string();
    let title = format!("{} — Voice Call", classroom_id);
    let name = display_name.unwrap_or_else(|| w.stake_address.clone());

    let devices = crate::tutoring::manager::DeviceSelection {
        camera_index: camera_id,
        mic_device_id: mic_id,
        speaker_device_id: speaker_id,
    };

    let ticket = state
        .tutoring
        .create_room(
            call_id.clone(),
            title.clone(),
            name,
            &endpoint,
            gossip,
            live,
            app,
            devices,
        )
        .await
        .map_err(|e| e.to_string())?;

    // Persist the call record
    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .execute(
                "INSERT INTO classroom_calls \
                 (id, classroom_id, channel_id, title, ticket, started_by, status, started_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', datetime('now'))",
                params![
                    call_id,
                    classroom_id,
                    channel_id,
                    title,
                    ticket,
                    w.stake_address
                ],
            )
            .map_err(|e| e.to_string())?;
    }

    // Broadcast call started event
    {
        let node_lock = state.p2p_node.lock().await;
        if let Some(node) = node_lock.as_ref() {
            let event = ClassroomMetaEvent::CallStarted {
                classroom_id: classroom_id.clone(),
                call_id: call_id.clone(),
                ticket: ticket.clone(),
                started_by: w.stake_address.clone(),
            };
            let _ = classroom_gossip::publish_meta(
                node,
                &classroom_id,
                &event,
                &w.signing_key,
                &w.stake_address.clone(),
            )
            .await;
        }
    }

    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    db.conn()
        .query_row(
            "SELECT id, classroom_id, channel_id, title, ticket, started_by, status, started_at, ended_at \
             FROM classroom_calls WHERE id = ?1",
            params![call_id],
            map_call_row,
        )
        .map_err(|e| e.to_string())
}

/// Join a classroom call.
#[tauri::command]
#[cfg(desktop)]
pub async fn classroom_join_call(
    call_id: String,
    display_name: Option<String>,
    camera_id: Option<String>,
    mic_id: Option<String>,
    speaker_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;

    let ticket: String = {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let (classroom_id, ticket): (String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT classroom_id, ticket FROM classroom_calls WHERE id = ?1 AND status = 'active'",
                params![call_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "call not found or already ended".to_string())?;
        let _ = require_classroom_member(db, &classroom_id, &w.stake_address)?;
        ticket.ok_or("call has no ticket")?
    };

    let content_node = &state.content_node;
    let endpoint = content_node
        .endpoint()
        .await
        .ok_or("iroh node not running")?;
    let gossip = content_node.gossip().await.ok_or("gossip not available")?;
    let live = content_node.live().await.ok_or("live not available")?;

    let name = display_name.unwrap_or_else(|| w.stake_address.clone());
    let devices = crate::tutoring::manager::DeviceSelection {
        camera_index: camera_id,
        mic_device_id: mic_id,
        speaker_device_id: speaker_id,
    };

    let title = format!("Voice Call ({})", call_id);
    state
        .tutoring
        .join_room(
            call_id, title, name, &ticket, &endpoint, gossip, live, app, devices,
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// End a classroom call (started_by or moderator/owner).
#[tauri::command]
pub async fn classroom_end_call(call_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let w = get_wallet(&state).await?;

    let classroom_id: String = {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let (classroom_id, started_by): (String, String) = db
            .conn()
            .query_row(
                "SELECT classroom_id, started_by FROM classroom_calls WHERE id = ?1 AND status = 'active'",
                params![call_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "call not found or already ended".to_string())?;

        if started_by != w.stake_address {
            match classroom_role(db, &classroom_id, &w.stake_address)?.as_deref() {
                Some("owner") | Some("moderator") => {}
                _ => {
                    return Err(
                        "only the call starter, moderators, or owners can end a call".to_string(),
                    )
                }
            }
        }

        db.conn()
            .execute(
                "UPDATE classroom_calls SET status = 'ended', ended_at = datetime('now') \
                 WHERE id = ?1",
                params![call_id],
            )
            .map_err(|e| e.to_string())?;

        classroom_id
    };

    // Leave the iroh-live room if we're in it
    let _ = state.tutoring.leave_room().await;

    // Broadcast call ended
    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        let event = ClassroomMetaEvent::CallEnded {
            classroom_id: classroom_id.clone(),
            call_id,
        };
        let _ = classroom_gossip::publish_meta(
            node,
            &classroom_id,
            &event,
            &w.signing_key,
            &w.stake_address.clone(),
        )
        .await;
    }

    Ok(())
}

/// Get the currently active call for a classroom (if any).
#[tauri::command]
pub async fn classroom_get_active_call(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<Option<ClassroomCall>, String> {
    let w = get_wallet(&state).await?;
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let _ = require_classroom_member(db, &classroom_id, &w.stake_address)?;

    db.conn()
        .query_row(
            "SELECT id, classroom_id, channel_id, title, ticket, started_by, status, started_at, ended_at \
             FROM classroom_calls WHERE classroom_id = ?1 AND status = 'active' LIMIT 1",
            params![classroom_id],
            map_call_row,
        )
        .optional()
        .map_err(|e| e.to_string())
}

fn map_call_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClassroomCall> {
    Ok(ClassroomCall {
        id: row.get(0)?,
        classroom_id: row.get(1)?,
        channel_id: row.get(2)?,
        title: row.get(3)?,
        ticket: row.get(4)?,
        started_by: row.get(5)?,
        status: row.get(6)?,
        started_at: row.get(7)?,
        ended_at: row.get(8)?,
    })
}

// ── Non-desktop stubs for call commands ───────────────────────────

#[tauri::command]
#[cfg(not(desktop))]
pub async fn classroom_start_call(
    _classroom_id: String,
    _channel_id: Option<String>,
    _display_name: Option<String>,
    _camera_id: Option<String>,
    _mic_id: Option<String>,
    _speaker_id: Option<String>,
    _app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<ClassroomCall, String> {
    Err("voice/video calls are not yet supported on this platform".to_string())
}

#[tauri::command]
#[cfg(not(desktop))]
pub async fn classroom_join_call(
    _call_id: String,
    _display_name: Option<String>,
    _camera_id: Option<String>,
    _mic_id: Option<String>,
    _speaker_id: Option<String>,
    _app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<(), String> {
    Err("voice/video calls are not yet supported on this platform".to_string())
}
