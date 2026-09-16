//! Tauri commands for the Classrooms feature.
//!
//! Classrooms are persistent group spaces (like Discord servers) with
//! text channels, message history, join-request gating, and live A/V
//! calls (via the live crate, delegated to TutoringManager).

use crate::profile::scope::ProfileState as State;
use rusqlite::{params, OptionalExtension};
use tauri::AppHandle;

use crate::classroom::gossip as classroom_gossip;
use crate::classroom::types::{
    classroom_message_topic, classroom_meta_topic, ClassroomMessagePayload, ClassroomMetaEvent,
};
use crate::crypto::hash::entity_id;
use crate::crypto::wallet;
use crate::db::executor::DatabaseWorkload;
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

async fn classroom_db<T, F>(
    state: &State<'_, AppState>,
    label: &'static str,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&crate::db::Database) -> Result<T, String> + Send + 'static,
{
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            label,
            operation,
        )
        .await
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

fn can_manage_classroom(
    conn: &rusqlite::Connection,
    classroom_id: &str,
    actor_address: &str,
) -> Result<bool, String> {
    conn.query_row(
        "SELECT c.owner_address, m.role FROM classrooms c \
         LEFT JOIN classroom_members m \
           ON m.classroom_id = c.id AND m.stake_address = ?2 \
         WHERE c.id = ?1",
        params![classroom_id, actor_address],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .optional()
    .map_err(|e| e.to_string())
    .map(|authority| {
        authority.is_some_and(|(owner_address, role)| {
            owner_address == actor_address || role.as_deref() == Some("moderator")
        })
    })
}

fn classroom_owner_address(
    conn: &rusqlite::Connection,
    classroom_id: &str,
) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT owner_address FROM classrooms WHERE id = ?1",
        params![classroom_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn persist_member_approval(
    db: &crate::db::Database,
    classroom_id: &str,
    approved_address: &str,
    reviewer_address: &str,
) -> Result<(), String> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    if !can_manage_classroom(&tx, classroom_id, reviewer_address)? {
        return Err("you must be a moderator or owner to approve members".to_string());
    }

    let updated = tx
        .execute(
            "UPDATE classroom_join_requests \
             SET status = 'approved', reviewed_by = ?3, reviewed_at = datetime('now') \
             WHERE classroom_id = ?1 AND stake_address = ?2 AND status = 'pending'",
            params![classroom_id, approved_address, reviewer_address],
        )
        .map_err(|e| e.to_string())?;
    if updated != 1 {
        return Err("pending join request not found".to_string());
    }

    tx.execute(
        "INSERT OR IGNORE INTO classroom_members \
         (classroom_id, stake_address, role, joined_at) \
         VALUES (?1, ?2, 'member', datetime('now'))",
        params![classroom_id, approved_address],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

fn persist_member_denial(
    db: &crate::db::Database,
    classroom_id: &str,
    denied_address: &str,
    reviewer_address: &str,
) -> Result<(), String> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    if !can_manage_classroom(&tx, classroom_id, reviewer_address)? {
        return Err("you must be a moderator or owner to deny members".to_string());
    }

    let updated = tx
        .execute(
            "UPDATE classroom_join_requests \
             SET status = 'denied', reviewed_by = ?3, reviewed_at = datetime('now') \
             WHERE classroom_id = ?1 AND stake_address = ?2 AND status = 'pending'",
            params![classroom_id, denied_address, reviewer_address],
        )
        .map_err(|e| e.to_string())?;
    if updated != 1 {
        return Err("pending join request not found".to_string());
    }
    tx.commit().map_err(|e| e.to_string())
}

fn persist_member_kick(
    db: &crate::db::Database,
    classroom_id: &str,
    kicked_address: &str,
    moderator_address: &str,
) -> Result<(), String> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    if !can_manage_classroom(&tx, classroom_id, moderator_address)? {
        return Err("you must be a moderator or owner to kick members".to_string());
    }

    let owner_address = classroom_owner_address(&tx, classroom_id)?
        .ok_or_else(|| "classroom not found".to_string())?;
    if kicked_address == owner_address {
        return Err("the classroom owner cannot be kicked".to_string());
    }
    let kicked_role = tx
        .query_row(
            "SELECT role FROM classroom_members \
             WHERE classroom_id = ?1 AND stake_address = ?2",
            params![classroom_id, kicked_address],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match kicked_role.as_deref() {
        Some(_) => {}
        None => return Err("classroom member not found".to_string()),
    }

    let removed = tx
        .execute(
            "DELETE FROM classroom_members WHERE classroom_id = ?1 AND stake_address = ?2",
            params![classroom_id, kicked_address],
        )
        .map_err(|e| e.to_string())?;
    if removed != 1 {
        return Err("classroom membership changed before it could be removed".to_string());
    }
    tx.commit().map_err(|e| e.to_string())
}

fn persist_member_role_change(
    db: &crate::db::Database,
    classroom_id: &str,
    member_address: &str,
    new_role: &str,
    actor_address: &str,
) -> Result<(), String> {
    if !matches!(new_role, "moderator" | "member") {
        return Err(
            "invalid role — generic role changes allow only 'moderator' or 'member'".to_string(),
        );
    }

    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    let canonical_owner = classroom_owner_address(&tx, classroom_id)?
        .ok_or_else(|| "classroom not found".to_string())?;
    if actor_address != canonical_owner {
        return Err("only the owner can change roles".to_string());
    }
    if member_address == canonical_owner {
        return Err("classroom ownership requires the dedicated transfer operation".to_string());
    }

    let updated = tx
        .execute(
            "UPDATE classroom_members SET role = ?3 \
             WHERE classroom_id = ?1 AND stake_address = ?2",
            params![classroom_id, member_address, new_role],
        )
        .map_err(|e| e.to_string())?;
    if updated != 1 {
        return Err("classroom member not found".to_string());
    }
    tx.commit().map_err(|e| e.to_string())
}

fn classroom_page_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(50).clamp(1, 200)
}

fn current_time_millis() -> Result<u64, String> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).map_err(|_| "current time is outside the supported range".to_string())
}

fn prepare_message_payload(
    content: &str,
    encrypted_group_key: Option<(Vec<u8>, i32)>,
    content_key: Option<[u8; 32]>,
) -> Result<(String, bool, u32), String> {
    let Some((encrypted_key, version)) = encrypted_group_key else {
        return Ok((content.to_string(), false, 0));
    };
    let content_key = content_key
        .ok_or("classroom group key cannot be opened while the content key is unavailable")?;
    let key_bytes = crate::crypto::content_crypto::decrypt(&content_key, &encrypted_key)
        .map_err(|e| format!("classroom group key would not open: {e}"))?
        .ok_or("classroom group key is stored unencrypted — refusing to send")?;
    if key_bytes.len() != 32 {
        return Err(format!(
            "classroom group key is {} bytes, expected 32 — refusing to send",
            key_bytes.len()
        ));
    }
    let mut group_key = [0u8; 32];
    group_key.copy_from_slice(&key_bytes);
    let ciphertext = crate::crypto::group_key::encrypt_message(&group_key, content.as_bytes())
        .map_err(|e| format!("classroom message encryption failed: {e}"))?;
    let key_version = u32::try_from(version)
        .map_err(|_| "classroom group key version cannot be negative".to_string())?;
    use base64::Engine;
    Ok((
        base64::engine::general_purpose::STANDARD.encode(ciphertext),
        true,
        key_version,
    ))
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

    let channel_id = entity_id(&[&id, "general"]);
    let owner_address = w.stake_address.clone();
    classroom_db(&state, "classroom.create", move |db| {
        let tx = rusqlite::Transaction::new_unchecked(
            db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO classrooms (id, name, description, icon_emoji, owner_address, invite_code) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, name, description, icon_emoji, owner_address, invite_code],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO classroom_members (classroom_id, stake_address, role, display_name) \
             VALUES (?1, ?2, 'owner', ?3)",
            params![id, owner_address, Option::<String>::None],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO classroom_channels (id, classroom_id, name, channel_type, position) \
             VALUES (?1, ?2, 'general', 'text', 0)",
            params![channel_id, id],
        )
        .map_err(|e| e.to_string())?;
        let classroom = tx
            .query_row(
                "SELECT c.id, c.name, c.description, c.icon_emoji, c.owner_address, \
                        c.invite_code, c.status, c.created_at, c.updated_at, \
                        COUNT(m.stake_address) AS member_count, \
                        CASE WHEN c.owner_address = ?2 THEN 'owner' \
                             ELSE MAX(CASE WHEN m.stake_address = ?2 \
                                          THEN CASE WHEN m.role = 'owner' THEN 'member' ELSE m.role END END) \
                        END AS my_role
                 FROM classrooms c
                 LEFT JOIN classroom_members m ON m.classroom_id = c.id
                 WHERE c.id = ?1
                 GROUP BY c.id",
                params![id, owner_address],
                map_classroom_row,
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(classroom)
    })
    .await
}

/// List all classrooms the local user is a member of.
#[tauri::command]
pub async fn classroom_list(state: State<'_, AppState>) -> Result<Vec<Classroom>, String> {
    let w = get_wallet(&state).await?;
    classroom_db(&state, "classroom.list", move |db| {
        let mut stmt = db
            .conn()
            .prepare(
                "SELECT c.id, c.name, c.description, c.icon_emoji, c.owner_address, \
                    c.invite_code, c.status, c.created_at, c.updated_at, \
                    COUNT(m2.stake_address) AS member_count, \
                    CASE WHEN c.owner_address = ?1 THEN 'owner' \
                         WHEN me.role = 'owner' THEN 'member' ELSE me.role END AS my_role
             FROM classrooms c
             JOIN classroom_members me ON me.classroom_id = c.id AND me.stake_address = ?1
             LEFT JOIN classroom_members m2 ON m2.classroom_id = c.id
             WHERE c.status = 'active'
             GROUP BY c.id
             ORDER BY c.name",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![w.stake_address], map_classroom_row)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    })
    .await
}

/// Get a single classroom by ID.
#[tauri::command]
pub async fn classroom_get(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<Classroom, String> {
    let w = get_wallet(&state).await?;
    classroom_db(&state, "classroom.get", move |db| {
        db.conn()
            .query_row(
                "SELECT c.id, c.name, c.description, c.icon_emoji, c.owner_address, \
                        c.invite_code, c.status, c.created_at, c.updated_at, \
                        COUNT(m.stake_address) AS member_count, \
                        CASE WHEN c.owner_address = ?2 THEN 'owner' \
                             ELSE MAX(CASE WHEN m.stake_address = ?2 \
                                          THEN CASE WHEN m.role = 'owner' THEN 'member' ELSE m.role END END) \
                        END AS my_role
                 FROM classrooms c
                 LEFT JOIN classroom_members m ON m.classroom_id = c.id
                 WHERE c.id = ?1
                 GROUP BY c.id",
                params![classroom_id, w.stake_address],
                map_classroom_row,
            )
            .map_err(|e| e.to_string())
    })
    .await
}

/// Archive (soft-delete) a classroom. Owner only.
#[tauri::command]
pub async fn classroom_archive(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;
    classroom_db(&state, "classroom.archive", move |db| {
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
    })
    .await
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
    classroom_db(&state, "classroom.list-members", move |db| {
        let _ = require_classroom_member(db, &classroom_id, &w.stake_address)?;
        let mut stmt = db
            .conn()
            .prepare(
                "SELECT m.classroom_id, m.stake_address, \
                        CASE WHEN m.stake_address = c.owner_address THEN 'owner' \
                             WHEN m.role = 'owner' THEN 'member' ELSE m.role END AS effective_role, \
                        m.display_name, m.joined_at \
                 FROM classroom_members m JOIN classrooms c ON c.id = m.classroom_id \
                 WHERE m.classroom_id = ?1 ORDER BY effective_role, m.display_name",
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
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    })
    .await
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
    let persisted_request_id = request_id.clone();
    let persisted_classroom_id = classroom_id.clone();
    let requester_address = w.stake_address.clone();
    let persisted_message = message.clone();
    classroom_db(&state, "classroom.request-join", move |db| {
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO classroom_join_requests \
                 (id, classroom_id, stake_address, message, status, requested_at) \
                 VALUES (?1, ?2, ?3, ?4, 'pending', datetime('now'))",
                params![
                    persisted_request_id,
                    persisted_classroom_id,
                    requester_address,
                    persisted_message
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await?;

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

    // Verify local user and update membership + request atomically.
    let persisted_classroom_id = classroom_id.clone();
    let approved_address = stake_address.clone();
    let reviewer_address = w.stake_address.clone();
    classroom_db(&state, "classroom.approve-member", move |db| {
        persist_member_approval(
            db,
            &persisted_classroom_id,
            &approved_address,
            &reviewer_address,
        )
    })
    .await?;

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

    let persisted_classroom_id = classroom_id.clone();
    let denied_address = stake_address.clone();
    let reviewer_address = w.stake_address.clone();
    classroom_db(&state, "classroom.deny-member", move |db| {
        persist_member_denial(
            db,
            &persisted_classroom_id,
            &denied_address,
            &reviewer_address,
        )
    })
    .await?;

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

    let persisted_classroom_id = classroom_id.clone();
    let member_address = w.stake_address.clone();
    classroom_db(&state, "classroom.leave", move |db| {
        if classroom_owner_address(db.conn(), &persisted_classroom_id)?.as_deref()
            == Some(member_address.as_str())
        {
            return Err(
                "owners cannot leave — archive the classroom or transfer ownership first"
                    .to_string(),
            );
        }

        let removed = db
            .conn()
            .execute(
                "DELETE FROM classroom_members WHERE classroom_id = ?1 AND stake_address = ?2",
                params![persisted_classroom_id, member_address],
            )
            .map_err(|e| e.to_string())?;
        if removed != 1 {
            return Err("classroom member not found".to_string());
        }
        Ok(())
    })
    .await?;

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

    let persisted_classroom_id = classroom_id.clone();
    let kicked_address = stake_address.clone();
    let moderator_address = w.stake_address.clone();
    classroom_db(&state, "classroom.kick-member", move |db| {
        persist_member_kick(
            db,
            &persisted_classroom_id,
            &kicked_address,
            &moderator_address,
        )
    })
    .await?;

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
    if !matches!(role.as_str(), "moderator" | "member") {
        return Err(
            "invalid role — generic role changes allow only 'moderator' or 'member'".to_string(),
        );
    }

    let w = get_wallet(&state).await?;

    let persisted_classroom_id = classroom_id.clone();
    let member_address = stake_address.clone();
    let persisted_role = role.clone();
    let owner_address = w.stake_address.clone();
    classroom_db(&state, "classroom.set-role", move |db| {
        persist_member_role_change(
            db,
            &persisted_classroom_id,
            &member_address,
            &persisted_role,
            &owner_address,
        )
    })
    .await?;

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
    let actor_address = w.stake_address.clone();
    classroom_db(&state, "classroom.list-join-requests", move |db| {
        if !can_manage_classroom(db.conn(), &classroom_id, &actor_address)? {
            return Err("you must be a moderator or owner to view join requests".to_string());
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
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    })
    .await
}

// ── Channels ───────────────────────────────────────────────────────

/// List channels in a classroom.
#[tauri::command]
pub async fn classroom_list_channels(
    classroom_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ClassroomChannel>, String> {
    let w = get_wallet(&state).await?;
    classroom_db(&state, "classroom.list-channels", move |db| {
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
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    })
    .await
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

    let id = entity_id(&[&classroom_id, &name]);
    classroom_db(&state, "classroom.create-channel", move |db| {
        let tx = rusqlite::Transaction::new_unchecked(
            db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(|e| e.to_string())?;
        if !can_manage_classroom(&tx, &classroom_id, &w.stake_address)? {
            return Err("only moderators and owners can create channels".to_string());
        }
        let position: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM classroom_channels WHERE classroom_id = ?1",
                params![classroom_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT OR IGNORE INTO classroom_channels \
             (id, classroom_id, name, description, channel_type, position) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, classroom_id, name, description, channel_type, position],
        )
        .map_err(|e| e.to_string())?;
        let channel = tx
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
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(channel)
    })
    .await
}

/// Delete a channel (owner only).
#[tauri::command]
pub async fn classroom_delete_channel(
    channel_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let w = get_wallet(&state).await?;
    classroom_db(&state, "classroom.delete-channel", move |db| {
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
    })
    .await
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
    let limit = classroom_page_limit(limit);
    classroom_db(&state, "classroom.get-messages", move |db| {
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
        let mut messages = rows;
        messages.reverse();
        Ok(messages)
    })
    .await
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
    let content_key = state.content_node.content_key().await;
    let context_channel_id = channel_id.clone();
    let sender_address = w.stake_address.clone();
    let (classroom_id, encrypted_group_key) = classroom_db(
        &state,
        "classroom.message-context",
        move |db| {
            let (classroom_id, channel_type): (String, String) = db
                .conn()
                .query_row(
                    "SELECT classroom_id, channel_type FROM classroom_channels WHERE id = ?1",
                    params![context_channel_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| "channel not found".to_string())?;
            let _ = require_classroom_member(db, &classroom_id, &sender_address)?;
            if channel_type == "announcement"
                && !can_manage_classroom(db.conn(), &classroom_id, &sender_address)?
            {
                return Err(
                    "only moderators and owners can post in announcement channels".to_string(),
                );
            }
            let encrypted_group_key = db
                .conn()
                .query_row(
                    "SELECT group_key_enc, key_version FROM classroom_group_keys WHERE classroom_id = ?1",
                    params![classroom_id],
                    |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i32>(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            Ok((classroom_id, encrypted_group_key))
        },
    )
    .await?;
    let (message_payload, is_encrypted, key_version) =
        prepare_message_payload(&content, encrypted_group_key, content_key)?;

    let now_ms = current_time_millis()?;
    let id = entity_id(&[&channel_id, &w.stake_address.clone(), &now_ms.to_string()]);
    let signed_now_ms = i64::try_from(now_ms)
        .map_err(|_| "current time is outside the supported database range".to_string())?;
    let sent_at = chrono::DateTime::from_timestamp_millis(signed_now_ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let persisted_id = id.clone();
    let persisted_channel_id = channel_id.clone();
    let persisted_classroom_id = classroom_id.clone();
    let persisted_sender = w.stake_address.clone();
    let persisted_content = content.clone();
    let persisted_sent_at = sent_at.clone();
    classroom_db(&state, "classroom.persist-message", move |db| {
        db.conn()
            .execute(
                "INSERT INTO classroom_messages \
                 (id, channel_id, classroom_id, sender_address, content, sent_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    persisted_id,
                    persisted_channel_id,
                    persisted_classroom_id,
                    persisted_sender,
                    persisted_content,
                    persisted_sent_at
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await?;

    // Broadcast via P2P
    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        let payload = ClassroomMessagePayload {
            id: id.clone(),
            classroom_id: classroom_id.clone(),
            channel_id: channel_id.clone(),
            content: message_payload,
            sender_name: None,
            sent_at: now_ms,
            is_delete: false,
            encrypted: is_encrypted,
            key_version,
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
    let persisted_message_id = message_id.clone();
    let actor_address = w.stake_address.clone();
    let (channel_id, classroom_id) = classroom_db(&state, "classroom.delete-message", move |db| {
        let tx = rusqlite::Transaction::new_unchecked(
            db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(|e| e.to_string())?;
        let (channel_id, classroom_id, sender) = tx
            .query_row(
                "SELECT channel_id, classroom_id, sender_address \
                 FROM classroom_messages WHERE id = ?1",
                params![persisted_message_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(|_| "message not found".to_string())?;
        if sender != actor_address && !can_manage_classroom(&tx, &classroom_id, &actor_address)? {
            return Err("you can only delete your own messages".to_string());
        }
        tx.execute(
            "UPDATE classroom_messages SET deleted = 1 WHERE id = ?1",
            params![persisted_message_id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok((channel_id, classroom_id))
    })
    .await?;

    // Broadcast tombstone
    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        let payload = ClassroomMessagePayload {
            id: message_id,
            classroom_id: classroom_id.clone(),
            channel_id,
            content: String::new(),
            sender_name: None,
            sent_at: current_time_millis()?,
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
    let authorized_classroom_id = classroom_id.clone();
    classroom_db(&state, "classroom.subscribe.authorize", move |db| {
        let _ = require_classroom_member(db, &classroom_id, &w.stake_address)?;
        Ok(())
    })
    .await?;

    let node_lock = state.p2p_node.lock().await;
    if let Some(node) = node_lock.as_ref() {
        node.subscribe_topic(&classroom_message_topic(&authorized_classroom_id))
            .await
            .map_err(|e| e.to_string())?;
        node.subscribe_topic(&classroom_meta_topic(&authorized_classroom_id))
            .await
            .map_err(|e| e.to_string())?;
    }
    state
        .classroom
        .mark_subscribed(&authorized_classroom_id)
        .await;
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
/// Creates a live-crate room and broadcasts the ticket to all members
/// via the meta gossip topic so they can join.
#[tauri::command]
#[cfg(desktop)]
#[allow(
    clippy::too_many_arguments,
    reason = "Tauri exposes these established call options as named IPC arguments"
)]
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
    let authorized_classroom_id = classroom_id.clone();
    let actor_address = w.stake_address.clone();
    classroom_db(&state, "classroom.start-call.authorize", move |db| {
        let _ = require_classroom_member(db, &authorized_classroom_id, &actor_address)?;
        Ok(())
    })
    .await?;

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

    let persisted_call_id = call_id.clone();
    let persisted_classroom_id = classroom_id.clone();
    let persisted_channel_id = channel_id.clone();
    let persisted_title = title.clone();
    let persisted_ticket = ticket.clone();
    let persisted_starter = w.stake_address.clone();
    let call = classroom_db(&state, "classroom.start-call.persist", move |db| {
        let tx = rusqlite::Transaction::new_unchecked(
            db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO classroom_calls \
             (id, classroom_id, channel_id, title, ticket, started_by, status, started_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', datetime('now'))",
            params![
                persisted_call_id,
                persisted_classroom_id,
                persisted_channel_id,
                persisted_title,
                persisted_ticket,
                persisted_starter
            ],
        )
        .map_err(|e| e.to_string())?;
        let call = tx
            .query_row(
                "SELECT id, classroom_id, channel_id, title, ticket, started_by, status, started_at, ended_at \
                 FROM classroom_calls WHERE id = ?1",
                params![persisted_call_id],
                map_call_row,
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(call)
    })
    .await;
    let call = match call {
        Ok(call) => call,
        Err(error) => {
            if let Err(cleanup_error) = state.tutoring.leave_room().await {
                log::error!(
                    "classroom_start_call: failed to clean up unpersisted room: {cleanup_error}"
                );
            }
            return Err(error);
        }
    };

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

    Ok(call)
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
    let requested_call_id = call_id.clone();
    let actor_address = w.stake_address.clone();
    let ticket = classroom_db(&state, "classroom.join-call.authorize", move |db| {
        let (classroom_id, ticket): (String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT classroom_id, ticket FROM classroom_calls WHERE id = ?1 AND status = 'active'",
                params![requested_call_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "call not found or already ended".to_string())?;
        let _ = require_classroom_member(db, &classroom_id, &actor_address)?;
        ticket.ok_or_else(|| "call has no ticket".to_string())
    })
    .await?;

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
    let persisted_call_id = call_id.clone();
    let actor_address = w.stake_address.clone();
    let classroom_id = classroom_db(&state, "classroom.end-call", move |db| {
        let tx = rusqlite::Transaction::new_unchecked(
            db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(|e| e.to_string())?;
        let (classroom_id, started_by): (String, String) = tx
            .query_row(
                "SELECT classroom_id, started_by FROM classroom_calls WHERE id = ?1 AND status = 'active'",
                params![persisted_call_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "call not found or already ended".to_string())?;

        if started_by != actor_address
            && !can_manage_classroom(&tx, &classroom_id, &actor_address)?
        {
            return Err(
                "only the call starter, moderators, or owners can end a call".to_string(),
            );
        }

        tx.execute(
                "UPDATE classroom_calls SET status = 'ended', ended_at = datetime('now') \
                 WHERE id = ?1",
                params![persisted_call_id],
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(classroom_id)
    })
    .await?;

    // Leave the live-crate room if we're in it
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
    classroom_db(&state, "classroom.get-active-call", move |db| {
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
    })
    .await
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
#[allow(
    clippy::too_many_arguments,
    reason = "the fail-closed platform stub must preserve the desktop IPC signature"
)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use base64::Engine;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open in-memory database");
        db.run_migrations().expect("run migrations");
        db.conn()
            .execute(
                "INSERT INTO classrooms (id, name, owner_address) \
                 VALUES ('classroom-1', 'Test classroom', 'owner')",
                [],
            )
            .expect("insert classroom");
        db.conn()
            .execute(
                "INSERT INTO classroom_members (classroom_id, stake_address, role) \
                 VALUES ('classroom-1', 'owner', 'owner')",
                [],
            )
            .expect("insert owner");
        db
    }

    fn insert_join_request(db: &Database, address: &str) {
        db.conn()
            .execute(
                "INSERT INTO classroom_join_requests (id, classroom_id, stake_address) \
                 VALUES (?1, 'classroom-1', ?2)",
                params![format!("request-{address}"), address],
            )
            .expect("insert join request");
    }

    #[test]
    fn message_page_limit_is_always_bounded() {
        assert_eq!(classroom_page_limit(None), 50);
        assert_eq!(classroom_page_limit(Some(-1)), 1);
        assert_eq!(classroom_page_limit(Some(0)), 1);
        assert_eq!(classroom_page_limit(Some(75)), 75);
        assert_eq!(classroom_page_limit(Some(500)), 200);
    }

    #[test]
    fn classroom_with_no_group_key_uses_plaintext_payload() {
        assert_eq!(
            prepare_message_payload("hello", None, None).unwrap(),
            ("hello".to_string(), false, 0)
        );
    }

    #[test]
    fn encrypted_group_key_never_falls_back_to_plaintext() {
        let error = prepare_message_payload("secret", Some((vec![1, 2, 3], 1)), None)
            .expect_err("a missing content key must fail closed");
        assert!(error.contains("content key is unavailable"));

        let error = prepare_message_payload("secret", Some((vec![1, 2, 3], -1)), Some([7; 32]))
            .expect_err("a corrupt encrypted key must fail closed");
        assert!(error.contains("would not open") || error.contains("stored unencrypted"));

        let content_key = [7; 32];
        let encrypted_group_key =
            crate::crypto::content_crypto::encrypt(&content_key, &[9; 32]).unwrap();
        let error =
            prepare_message_payload("secret", Some((encrypted_group_key, -1)), Some(content_key))
                .expect_err("a negative key version must be rejected");
        assert!(error.contains("cannot be negative"));
    }

    #[test]
    fn valid_group_key_produces_decryptable_versioned_payload() {
        let content_key = [7; 32];
        let group_key = [9; 32];
        let encrypted_group_key =
            crate::crypto::content_crypto::encrypt(&content_key, &group_key).unwrap();
        let (payload, encrypted, version) = prepare_message_payload(
            "private lesson",
            Some((encrypted_group_key, 3)),
            Some(content_key),
        )
        .unwrap();

        assert!(encrypted);
        assert_eq!(version, 3);
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .unwrap();
        assert_eq!(
            crate::crypto::group_key::decrypt_message(&group_key, &ciphertext).unwrap(),
            b"private lesson"
        );
    }

    #[test]
    fn approval_requires_and_consumes_one_pending_request() {
        let db = test_db();
        let error = persist_member_approval(&db, "classroom-1", "learner", "owner")
            .expect_err("membership must not be created without a request");
        assert_eq!(error, "pending join request not found");
        let member_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM classroom_members WHERE stake_address = 'learner'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(member_count, 0);

        insert_join_request(&db, "learner");
        persist_member_approval(&db, "classroom-1", "learner", "owner").unwrap();
        let (request_status, member_role): (String, String) = db
            .conn()
            .query_row(
                "SELECT r.status, m.role \
                 FROM classroom_join_requests r \
                 JOIN classroom_members m \
                   ON m.classroom_id = r.classroom_id AND m.stake_address = r.stake_address \
                 WHERE r.classroom_id = 'classroom-1' AND r.stake_address = 'learner'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(request_status, "approved");
        assert_eq!(member_role, "member");
    }

    #[test]
    fn unauthorized_review_leaves_request_pending() {
        let db = test_db();
        insert_join_request(&db, "learner");
        let error = persist_member_denial(&db, "classroom-1", "learner", "outsider")
            .expect_err("an outsider cannot review join requests");
        assert!(error.contains("moderator or owner"));
        let status: String = db
            .conn()
            .query_row(
                "SELECT status FROM classroom_join_requests WHERE stake_address = 'learner'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn owner_cannot_be_kicked_but_an_existing_member_can() {
        let db = test_db();
        db.conn()
            .execute(
                "INSERT INTO classroom_members (classroom_id, stake_address, role) \
                 VALUES ('classroom-1', 'moderator', 'moderator'), \
                        ('classroom-1', 'learner', 'member')",
                [],
            )
            .unwrap();

        let error = persist_member_kick(&db, "classroom-1", "owner", "moderator")
            .expect_err("the canonical owner must not be removed");
        assert_eq!(error, "the classroom owner cannot be kicked");
        persist_member_kick(&db, "classroom-1", "learner", "moderator").unwrap();
        let member_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM classroom_members WHERE stake_address = 'learner'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(member_count, 0);
    }

    #[test]
    fn generic_role_changes_cannot_create_or_replace_an_owner() {
        let db = test_db();
        db.conn()
            .execute(
                "INSERT INTO classroom_members (classroom_id, stake_address, role) \
                 VALUES ('classroom-1', 'learner', 'member'), \
                        ('classroom-1', 'forged-owner', 'owner')",
                [],
            )
            .unwrap();

        let error =
            persist_member_role_change(&db, "classroom-1", "learner", "moderator", "forged-owner")
                .expect_err("a forged owner membership must not grant authority");
        assert_eq!(error, "only the owner can change roles");

        insert_join_request(&db, "second-learner");
        let error = persist_member_approval(&db, "classroom-1", "second-learner", "forged-owner")
            .expect_err("a forged owner membership must not approve members");
        assert!(error.contains("moderator or owner"));
        let error = persist_member_kick(&db, "classroom-1", "learner", "forged-owner")
            .expect_err("a forged owner membership must not kick members");
        assert!(error.contains("moderator or owner"));

        let error = persist_member_role_change(&db, "classroom-1", "learner", "owner", "owner")
            .expect_err("the generic command must not transfer ownership");
        assert!(error.contains("generic role changes"));

        let error = persist_member_role_change(&db, "classroom-1", "owner", "member", "owner")
            .expect_err("the canonical owner cannot be demoted in isolation");
        assert!(error.contains("dedicated transfer"));

        persist_member_role_change(&db, "classroom-1", "learner", "moderator", "owner").unwrap();
        let role: String = db
            .conn()
            .query_row(
                "SELECT role FROM classroom_members \
                 WHERE classroom_id = 'classroom-1' AND stake_address = 'learner'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(role, "moderator");

        db.conn()
            .execute(
                "UPDATE classroom_members SET role = 'member' \
                 WHERE classroom_id = 'classroom-1' AND stake_address = 'owner'",
                [],
            )
            .unwrap();
        persist_member_approval(&db, "classroom-1", "second-learner", "owner")
            .expect("canonical ownership must not depend on the membership label");
    }
}
