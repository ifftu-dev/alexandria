//! IPC commands for guardian links (cross-device parental oversight).
//!
//! Child (ward) side: generate an invite while gated, push activity to
//! linked guardians, revoke once adult.
//! Parent (guardian) side: accept an invite (issues the guardianship
//! RoleCredential and dials the child), pull/receive activity, revoke.

use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::State;

use crate::commands::pairing::{DbHandle, NodeHandle};
use crate::crypto::guardian as invite_codec;
use crate::domain::vc::{Claim, CredentialType, RoleClaim};
use crate::p2p::guardian::{
    self as proto, GuardianActivityPayload, GuardianRequest, GuardianResponse,
};
use crate::AppState;

/// Guardian invites stay valid for a week — the parent may be remote
/// and offline; the code waits, not a connection.
const INVITE_TTL_SECS: i64 = 7 * 24 * 3600;

#[derive(Debug, Serialize)]
pub struct GuardianLinkInfo {
    pub id: String,
    pub side: String,
    pub peer_did: String,
    pub peer_display_name: Option<String>,
    pub status: String,
    pub child_birthdate: Option<String>,
    pub created_at: String,
    pub last_sync_at: Option<String>,
}

/// List this profile's guardian links (both sides).
#[tauri::command]
pub async fn guardian_list_links(
    state: State<'_, AppState>,
) -> Result<Vec<GuardianLinkInfo>, String> {
    let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, side, peer_did, peer_display_name, status, child_birthdate, \
             created_at, last_sync_at FROM guardian_links ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let links = stmt
        .query_map([], |row| {
            Ok(GuardianLinkInfo {
                id: row.get(0)?,
                side: row.get(1)?,
                peer_did: row.get(2)?,
                peer_display_name: row.get(3)?,
                status: row.get(4)?,
                child_birthdate: row.get(5)?,
                created_at: row.get(6)?,
                last_sync_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(links)
}

/// Child side: generate a one-time guardian invite code. Shown on the
/// gate screen (and in settings for adding another guardian later).
#[tauri::command]
pub async fn guardian_create_invite(state: State<'_, AppState>) -> Result<String, String> {
    // Live dial info from the running node (same approach as pairing).
    let status = {
        let node = state.p2p_node.lock().await;
        let node = node
            .as_ref()
            .ok_or("the network is still starting — try again in a moment")?;
        node.status().await.map_err(|e| e.to_string())?
    };
    let child_peer_id = status
        .peer_id
        .ok_or("P2P node has no PeerId yet — try again in a moment")?;
    let mut addresses: Vec<String> = status
        .relay_addresses
        .into_iter()
        .chain(status.listening_addresses)
        .filter(|a| !a.contains("/127.0.0.1/") && !a.contains("/::1/"))
        .collect();
    addresses.dedup();

    let shared_key = crate::crypto::pairing::generate_shared_key();

    let (child_did, child_stake_address, display_name) = {
        let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();
        let did: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'identity.local_did'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let (stake, name): (String, Option<String>) = conn
            .query_row(
                "SELECT stake_address, display_name FROM local_identity WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("no identity: {e}"))?;
        (did, stake, name)
    };

    let invite = invite_codec::GuardianInvite {
        child_did,
        child_stake_address,
        child_peer_id,
        addresses,
        shared_key,
        display_name,
    };
    let code = invite_codec::encode(&invite)?;

    {
        let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        proto::record_pending_invite(
            db.conn(),
            &invite_codec::code_hash(&code),
            &shared_key,
            INVITE_TTL_SECS,
        )?;
    }
    Ok(code)
}

/// Parent side: accept a child's invite. Issues the guardianship
/// credential, records the link, dials the child, and completes the
/// exchange. If the child is offline the link stays `pending` and
/// [`guardian_sync_now`] retries.
#[tauri::command]
pub async fn guardian_accept_invite(
    state: State<'_, AppState>,
    code: String,
) -> Result<GuardianLinkInfo, String> {
    let invite = invite_codec::decode(&code)?;
    let code_hash = invite_codec::code_hash(&code);
    if invite.child_did.is_empty() {
        return Err("invite carries no child DID".to_string());
    }

    // Issue the guardianship credential (RoleCredential role=guardian,
    // subject = child DID) with our signing key.
    let (signing_key, issuer_did) = crate::commands::credentials::load_issuer_key(&state).await?;
    let now = crate::commands::credentials::now_rfc3339();

    let (link_id, guardian_vc_json, guardian_did, guardian_stake, guardian_name) = {
        let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();

        let req = crate::commands::credentials::IssueCredentialRequest {
            credential_type: CredentialType::RoleCredential,
            subject: crate::crypto::did::Did(invite.child_did.clone()),
            claim: Claim::Role(RoleClaim {
                role: "guardian".to_string(),
                scope: Some(issuer_did.as_str().to_string()),
            }),
            evidence_refs: vec![],
            expiration_date: None,
            supersedes: None,
            integrity_session_id: None,
            integrity_policy: None,
        };
        let vc = crate::commands::credentials::issue_credential_impl(
            conn,
            &signing_key,
            &issuer_did,
            &req,
            &now,
        )?;
        let vc_json = serde_json::to_string(&vc).map_err(|e| e.to_string())?;
        let vc_id = vc.id.clone().unwrap_or_default();

        let link_id =
            crate::crypto::hash::entity_id(&[issuer_did.as_str(), &invite.child_did, &now]);
        let (stake, name): (String, Option<String>) = conn
            .query_row(
                "SELECT stake_address, display_name FROM local_identity WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("no identity: {e}"))?;

        conn.execute(
            "INSERT INTO guardian_links \
             (id, side, peer_did, peer_stake_address, peer_peer_id, peer_display_name, \
              shared_key, status, guardian_vc_id, invite_code_hash) \
             VALUES (?1, 'guardian', ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8)",
            rusqlite::params![
                link_id,
                invite.child_did,
                invite.child_stake_address,
                invite.child_peer_id,
                invite.display_name,
                &invite.shared_key[..],
                vc_id,
                code_hash
            ],
        )
        .map_err(|e| e.to_string())?;

        // Remember the child's dial addresses for later retries/pulls.
        if !invite.addresses.is_empty() {
            let addrs_json =
                serde_json::to_string(&invite.addresses).unwrap_or_else(|_| "[]".into());
            conn.execute(
                "INSERT INTO peers (peer_id, addresses, last_seen) \
                 VALUES (?1, ?2, datetime('now')) \
                 ON CONFLICT(peer_id) DO UPDATE SET addresses = ?2, last_seen = datetime('now')",
                rusqlite::params![invite.child_peer_id, addrs_json],
            )
            .map_err(|e| e.to_string())?;
        }

        (
            link_id,
            vc_json,
            issuer_did.as_str().to_string(),
            stake,
            name,
        )
    };
    let _ = guardian_stake;

    // Dial the child and attempt the Link exchange now. Offline child ⇒
    // the link stays pending; surface that honestly.
    let result = send_link_request(
        &state.db,
        &state.p2p_node,
        &link_id,
        &invite,
        &code_hash,
        &guardian_did,
        guardian_name,
        &guardian_vc_json,
    )
    .await;

    if let Err(e) = result {
        log::info!("guardian: link exchange deferred ({e}); child likely offline");
    }

    get_link(&state, &link_id)
}

#[allow(clippy::too_many_arguments)]
async fn send_link_request(
    db_handle: &DbHandle,
    node_handle: &NodeHandle,
    link_id: &str,
    invite: &invite_codec::GuardianInvite,
    code_hash: &str,
    guardian_did: &str,
    guardian_name: Option<String>,
    guardian_vc_json: &str,
) -> Result<(), String> {
    let guardian_stake = {
        let db_guard = db_handle.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT stake_address FROM local_identity WHERE id = 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?
    };

    let peer: libp2p::PeerId = invite
        .child_peer_id
        .parse()
        .map_err(|e| format!("bad peer id in invite: {e}"))?;
    let addrs: Vec<libp2p::Multiaddr> = invite
        .addresses
        .iter()
        .filter_map(|a| a.parse().ok())
        .collect();

    let request = GuardianRequest::Link {
        code_hash: code_hash.to_string(),
        link_id: link_id.to_string(),
        guardian_did: guardian_did.to_string(),
        guardian_stake_address: guardian_stake,
        guardian_display_name: guardian_name,
        guardian_vc_json: guardian_vc_json.to_string(),
    };

    let response = {
        let node = node_handle.lock().await;
        let node = node.as_ref().ok_or("P2P node not running")?;
        let _ = node.connect_peer(peer, addrs).await;
        node.guardian_request(peer, request)
            .await
            .map_err(|e| e.to_string())?
    };

    match response {
        GuardianResponse::Linked { sealed_snapshot } => {
            let db_guard = db_handle.lock().map_err(|_| "db lock poisoned")?;
            let db = db_guard.as_ref().ok_or("database not initialized")?;
            let conn = db.conn();
            let payload: GuardianActivityPayload =
                proto::open(&invite.shared_key, &sealed_snapshot)?;
            conn.execute(
                "UPDATE guardian_links SET status = 'active', updated_at = datetime('now') \
                 WHERE id = ?1",
                rusqlite::params![link_id],
            )
            .map_err(|e| e.to_string())?;
            proto::apply_activity_snapshot(conn, link_id, &payload)?;
            Ok(())
        }
        GuardianResponse::Unauthorized => {
            Err("the child's device rejected the invite (expired or already used)".to_string())
        }
        GuardianResponse::Error(e) => Err(format!("child device error: {e}")),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// One syncable link row: `(id, side, status, shared_key, peer_id, code_hash)`.
type SyncableLink = (
    String,
    String,
    String,
    Vec<u8>,
    Option<String>,
    Option<String>,
);

/// Push (ward) / pull (guardian) activity across every active link.
/// Also retries pending Link exchanges on the guardian side. Returns
/// rows merged locally (guardian side) or pushed (ward side).
#[tauri::command]
pub async fn guardian_sync_now(state: State<'_, AppState>) -> Result<i64, String> {
    guardian_sync_all(&state.db, &state.p2p_node).await
}

/// Background driver: run one guardian exchange across every link.
/// Called from the app's periodic queue loop (fire-and-forget) and by
/// the manual [`guardian_sync_now`] command. No-op without links.
pub(crate) async fn guardian_sync_all(
    db_handle: &DbHandle,
    node_handle: &NodeHandle,
) -> Result<i64, String> {
    let links: Vec<SyncableLink> = {
        let db_guard = db_handle.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let mut stmt = db
            .conn()
            .prepare(
                "SELECT id, side, status, shared_key, peer_peer_id, invite_code_hash \
                 FROM guardian_links WHERE status IN ('active','pending')",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    let mut total = 0i64;
    for (link_id, side, link_status, key_blob, peer_id, _code_hash) in links {
        if key_blob.len() != 32 {
            continue;
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_blob);
        let Some(peer_str) = peer_id else { continue };

        match (side.as_str(), link_status.as_str()) {
            ("ward", "active") => {
                if let Ok(rows) =
                    push_to_guardian(db_handle, node_handle, &link_id, &key, &peer_str).await
                {
                    total += rows;
                }
            }
            ("guardian", "active") => {
                if let Ok(rows) =
                    pull_from_ward(db_handle, node_handle, &link_id, &key, &peer_str).await
                {
                    total += rows;
                }
            }
            // Pending guardian-side links get retried by re-running the
            // full accept exchange from the stored state.
            ("guardian", "pending") => {
                if let Err(e) = retry_pending_link(db_handle, node_handle, &link_id).await {
                    log::debug!("guardian: pending link {link_id} retry skipped: {e}");
                }
            }
            _ => {}
        }
    }
    Ok(total)
}

async fn push_to_guardian(
    db_handle: &DbHandle,
    node_handle: &NodeHandle,
    link_id: &str,
    key: &[u8; 32],
    peer_str: &str,
) -> Result<i64, String> {
    let sealed = {
        let db_guard = db_handle.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let snapshot = proto::build_activity_snapshot(db.conn())?;
        proto::seal(key, &snapshot)?
    };
    let peer: libp2p::PeerId = peer_str.parse().map_err(|e| format!("bad peer id: {e}"))?;
    let response = {
        let node = node_handle.lock().await;
        let node = node.as_ref().ok_or("P2P node not running")?;
        node.guardian_request(
            peer,
            GuardianRequest::ActivityPush {
                link_id: link_id.to_string(),
                sealed,
            },
        )
        .await
        .map_err(|e| e.to_string())?
    };
    match response {
        GuardianResponse::Merged { rows } => {
            let db_guard = db_handle.lock().map_err(|_| "db lock poisoned")?;
            let db = db_guard.as_ref().ok_or("database not initialized")?;
            let _ = db.conn().execute(
                "UPDATE guardian_links SET last_sync_at = datetime('now') WHERE id = ?1",
                rusqlite::params![link_id],
            );
            Ok(rows)
        }
        GuardianResponse::Unauthorized => Err("guardian no longer recognises this link".into()),
        GuardianResponse::Error(e) => Err(e),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

async fn pull_from_ward(
    db_handle: &DbHandle,
    node_handle: &NodeHandle,
    link_id: &str,
    key: &[u8; 32],
    peer_str: &str,
) -> Result<i64, String> {
    let peer: libp2p::PeerId = peer_str.parse().map_err(|e| format!("bad peer id: {e}"))?;
    let response = {
        let node = node_handle.lock().await;
        let node = node.as_ref().ok_or("P2P node not running")?;
        node.guardian_request(
            peer,
            GuardianRequest::ActivityPull {
                link_id: link_id.to_string(),
            },
        )
        .await
        .map_err(|e| e.to_string())?
    };
    match response {
        GuardianResponse::Sealed { sealed } => {
            let payload: GuardianActivityPayload = proto::open(key, &sealed)?;
            let db_guard = db_handle.lock().map_err(|_| "db lock poisoned")?;
            let db = db_guard.as_ref().ok_or("database not initialized")?;
            proto::apply_activity_snapshot(db.conn(), link_id, &payload)
        }
        GuardianResponse::Unauthorized => Err("ward no longer recognises this link".into()),
        GuardianResponse::Error(e) => Err(e),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// Stored guardian-side link fields needed to rebuild a Link request:
/// `(peer_did, peer_stake, peer_peer_id, peer_name, shared_key, vc_id, code_hash)`.
type PendingLinkRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Vec<u8>,
    Option<String>,
    Option<String>,
);

/// Re-send the Link request for a guardian-side pending link, rebuilding
/// it from the stored link row + issued credential.
async fn retry_pending_link(
    db_handle: &DbHandle,
    node_handle: &NodeHandle,
    link_id: &str,
) -> Result<(), String> {
    let (invite, code_hash, guardian_vc_json, guardian_did, guardian_name) = {
        let db_guard = db_handle.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();
        let (peer_did, peer_stake, peer_peer_id, peer_name, key_blob, vc_id, code_hash): PendingLinkRow = conn
            .query_row(
                "SELECT peer_did, peer_stake_address, peer_peer_id, peer_display_name, \
                 shared_key, guardian_vc_id, invite_code_hash \
                 FROM guardian_links WHERE id = ?1 AND side = 'guardian'",
                rusqlite::params![link_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .map_err(|e| e.to_string())?;
        let code_hash = code_hash.ok_or("link has no stored invite hash")?;
        let peer_peer_id = peer_peer_id.ok_or("link has no peer id")?;
        if key_blob.len() != 32 {
            return Err("stored key malformed".into());
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_blob);

        let vc_json: String = conn
            .query_row(
                "SELECT signed_vc_json FROM credentials WHERE id = ?1",
                rusqlite::params![vc_id.unwrap_or_default()],
                |r| r.get(0),
            )
            .map_err(|e| format!("stored guardian VC missing: {e}"))?;
        let guardian_did: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'identity.local_did'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let guardian_name: Option<String> = conn
            .query_row(
                "SELECT display_name FROM local_identity WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        let addrs: Vec<String> = conn
            .query_row(
                "SELECT addresses FROM peers WHERE peer_id = ?1",
                rusqlite::params![peer_peer_id],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();

        let invite = invite_codec::GuardianInvite {
            child_did: peer_did,
            child_stake_address: peer_stake.unwrap_or_default(),
            child_peer_id: peer_peer_id,
            addresses: addrs,
            shared_key: key,
            display_name: peer_name,
        };
        (invite, code_hash, vc_json, guardian_did, guardian_name)
    };

    send_link_request(
        db_handle,
        node_handle,
        link_id,
        &invite,
        &code_hash,
        &guardian_did,
        guardian_name,
        &guardian_vc_json,
    )
    .await
}

/// Revoke a link. Wards can only revoke once adult; guardians always
/// can. The counterparty is notified best-effort (offline peers learn
/// on their next exchange attempt, which will be refused).
#[tauri::command]
pub async fn guardian_revoke_link(
    state: State<'_, AppState>,
    link_id: String,
) -> Result<(), String> {
    let (side, key_blob, peer_id): (String, Vec<u8>, Option<String>) = {
        let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();
        let row: (String, Vec<u8>, Option<String>) = conn
            .query_row(
                "SELECT side, shared_key, peer_peer_id FROM guardian_links WHERE id = ?1",
                rusqlite::params![link_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| "link not found".to_string())?;

        // A minor ward cannot remove their own oversight.
        let (birthdate, role): (Option<String>, String) = conn
            .query_row(
                "SELECT birthdate, account_role FROM local_identity WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let _ = role;
        let row_side: &str = &row.0;
        if row_side == "ward" {
            let is_minor = birthdate
                .as_deref()
                .map(|b| crate::domain::identity::is_minor(b, chrono::Utc::now().date_naive()))
                .unwrap_or(false);
            if is_minor {
                return Err(
                    "you can remove your guardian once you turn 18 — until then only your guardian can unlink"
                        .to_string(),
                );
            }
        }

        conn.execute(
            "UPDATE guardian_links SET status = 'revoked', updated_at = datetime('now') \
             WHERE id = ?1",
            rusqlite::params![link_id],
        )
        .map_err(|e| e.to_string())?;
        row
    };
    let _ = side;

    // Notify the counterparty, best-effort.
    if let (Some(peer_str), true) = (peer_id, key_blob.len() == 32) {
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_blob);
        if let Ok(peer) = peer_str.parse::<libp2p::PeerId>() {
            if let Ok(sealed_marker) = proto::seal(&key, &format!("revoke:{link_id}")) {
                let node = state.p2p_node.lock().await;
                if let Some(node) = node.as_ref() {
                    let _ = node
                        .guardian_request(
                            peer,
                            GuardianRequest::Revoke {
                                link_id: link_id.clone(),
                                sealed_marker,
                            },
                        )
                        .await;
                }
            }
        }
    }
    Ok(())
}

/// Read one child's mirrored activity rows for a table (guardian side).
#[tauri::command]
pub async fn guardian_get_child_activity(
    state: State<'_, AppState>,
    link_id: String,
    table: String,
) -> Result<Vec<serde_json::Value>, String> {
    if !proto::GUARDIAN_SYNC_TABLES.contains(&table.as_str()) {
        return Err(format!("'{table}' is not a guardian-synced table"));
    }
    let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT payload_json FROM guardian_activity_rows \
             WHERE link_id = ?1 AND table_name = ?2 ORDER BY updated_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![link_id, table], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|j| serde_json::from_str(&j).ok())
        .collect())
}

fn get_link(state: &State<'_, AppState>, link_id: &str) -> Result<GuardianLinkInfo, String> {
    let db_guard = state.db.lock().map_err(|_| "db lock poisoned")?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    db.conn()
        .query_row(
            "SELECT id, side, peer_did, peer_display_name, status, child_birthdate, \
             created_at, last_sync_at FROM guardian_links WHERE id = ?1",
            rusqlite::params![link_id],
            |row| {
                Ok(GuardianLinkInfo {
                    id: row.get(0)?,
                    side: row.get(1)?,
                    peer_did: row.get(2)?,
                    peer_display_name: row.get(3)?,
                    status: row.get(4)?,
                    child_birthdate: row.get(5)?,
                    created_at: row.get(6)?,
                    last_sync_at: row.get(7)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}
