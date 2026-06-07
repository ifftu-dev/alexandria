//! Identity / wallet IPC commands.
//!
//! All commands here operate on the *currently active profile*. They
//! return an error when no profile is unlocked. The lifecycle
//! commands (create / unlock / lock / delete) live in
//! [`commands::profile`].

use rusqlite::params;
use serde::Serialize;
use tauri::State;

use crate::crypto::wallet;
use crate::domain::identity::{Identity, ProfileUpdate, WalletInfo};
use crate::domain::profile::{ProfilePayload, PublishProfileResult, SignedProfile};
use crate::ipfs::profile as ipfs_profile;
use crate::AppState;

/// Combined response from unlock/create flows.
#[derive(Debug, Serialize)]
pub struct UnlockResponse {
    pub wallet: WalletInfo,
    pub profile: Option<Identity>,
}

/// Export the mnemonic phrase from the active vault. Requires the
/// vault to be unlocked **and** re-authentication via the vault
/// password. Rate-limited: 3 attempts per 5 minutes.
#[tauri::command]
pub async fn export_mnemonic(
    state: State<'_, AppState>,
    password: String,
) -> Result<String, String> {
    {
        let mut limiter = state.ipc_limiter.lock().map_err(|e| e.to_string())?;
        limiter.check("export_mnemonic")?;
    }
    if let Ok(mut ts) = state.last_activity.lock() {
        *ts = std::time::Instant::now();
    }
    let keystore = state.keystore.lock().await;
    let ks = keystore.as_ref().ok_or("vault is locked")?;
    ks.check_password(&password)
        .map_err(|_| "incorrect password".to_string())?;
    ks.retrieve_mnemonic().map_err(|e| e.to_string())
}

/// Whether the active vault is unlocked. The actual biometric check
/// happens in the frontend via `tauri-plugin-biometry`; this lets the
/// frontend know whether biometric enrollment is currently meaningful.
#[tauri::command]
pub async fn is_biometric_available(state: State<'_, AppState>) -> Result<bool, String> {
    let keystore = state.keystore.lock().await;
    Ok(keystore.is_some())
}

/// Get the active profile's wallet info (no secrets).
#[tauri::command]
pub async fn get_wallet_info(state: State<'_, AppState>) -> Result<Option<WalletInfo>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let Some(db) = db_guard.as_ref() else {
        return Ok(None);
    };

    let result = db.conn().query_row(
        "SELECT stake_address, payment_address FROM local_identity WHERE id = 1",
        [],
        |row| {
            Ok(WalletInfo {
                stake_address: row.get(0)?,
                payment_address: row.get(1)?,
                has_mnemonic_backup: true,
            })
        },
    );

    match result {
        Ok(info) => Ok(Some(info)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Derive the active profile's `did:key`. Returns `None` when the
/// vault is locked so the UI can render in locked-read-only mode.
#[tauri::command]
pub async fn get_local_did(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let ks_guard = state.keystore.lock().await;
    let Some(ks) = ks_guard.as_ref() else {
        return Ok(None);
    };
    let mnemonic = match ks.retrieve_mnemonic() {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    drop(ks_guard);
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    let did = crate::crypto::did::derive_did_key(&w.signing_key);
    Ok(Some(did.as_str().to_string()))
}

/// Resolve a batch of `did:key` strings to human display names.
///
/// DIDs are opaque to humans, so the UI shows names wherever possible.
/// We can only name DIDs we actually know:
///  - the active profile's own DID → its `display_name`
///  - any built-in plugin's author DID → "Alexandria" (first-party)
///
/// Unknown DIDs are simply omitted from the returned map; the frontend
/// falls back to a shortened DID for those. Best-effort + cheap: callers
/// pass every DID they want to render and cache the result.
#[tauri::command]
pub async fn resolve_display_names(
    state: State<'_, AppState>,
    dids: Vec<String>,
) -> Result<std::collections::HashMap<String, String>, String> {
    use std::collections::{HashMap, HashSet};

    let mut out: HashMap<String, String> = HashMap::new();
    if dids.is_empty() {
        return Ok(out);
    }
    let requested: HashSet<&str> = dids.iter().map(|s| s.as_str()).collect();

    // Own DID → own display name (best-effort; needs the vault unlocked).
    let local_did: Option<String> = {
        let ks_guard = state.keystore.lock().await;
        match ks_guard.as_ref().and_then(|ks| ks.retrieve_mnemonic().ok()) {
            Some(mnemonic) => {
                drop(ks_guard);
                wallet::wallet_from_mnemonic(&mnemonic).ok().map(|w| {
                    crate::crypto::did::derive_did_key(&w.signing_key)
                        .as_str()
                        .to_string()
                })
            }
            None => None,
        }
    };

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    if let Some(did) = local_did {
        if requested.contains(did.as_str()) {
            if let Some(profile) = read_profile(db.conn()) {
                if let Some(name) = profile.display_name {
                    if !name.trim().is_empty() {
                        out.insert(did, name);
                    }
                }
            }
        }
    }

    // Built-in plugin authors → first-party label.
    {
        let mut stmt = db
            .conn()
            .prepare("SELECT DISTINCT author_did FROM plugin_installed WHERE source = 'builtin'")
            .map_err(|e| e.to_string())?;
        let authors = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        for a in authors {
            if requested.contains(a.as_str()) && !out.contains_key(&a) {
                out.insert(a, "Alexandria".to_string());
            }
        }
    }

    Ok(out)
}

/// Get the active profile's Identity row from the local DB.
#[tauri::command]
pub async fn get_profile(state: State<'_, AppState>) -> Result<Option<Identity>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let Some(db) = db_guard.as_ref() else {
        return Ok(None);
    };
    Ok(read_profile(db.conn()))
}

fn read_profile(conn: &rusqlite::Connection) -> Option<Identity> {
    conn.query_row(
        "SELECT stake_address, payment_address, display_name, bio, avatar_cid, profile_hash, created_at, updated_at
         FROM local_identity WHERE id = 1",
        [],
        |row| {
            Ok(Identity {
                stake_address: row.get(0)?,
                payment_address: row.get(1)?,
                display_name: row.get(2)?,
                bio: row.get(3)?,
                avatar_cid: row.get(4)?,
                profile_hash: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    )
    .ok()
}

/// Update the active profile's Identity row.
#[tauri::command]
pub async fn update_profile(
    state: State<'_, AppState>,
    update: ProfileUpdate,
) -> Result<Identity, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let mut set_clauses = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref name) = update.display_name {
        // Usernames are mandatory — refuse to blank an existing name.
        if name.trim().is_empty() {
            return Err("Display name is required and cannot be empty.".into());
        }
        set_clauses.push("display_name = ?");
        values.push(Box::new(name.trim().to_string()));
    }
    if let Some(ref bio) = update.bio {
        set_clauses.push("bio = ?");
        values.push(Box::new(bio.clone()));
    }
    if let Some(ref avatar) = update.avatar_cid {
        set_clauses.push("avatar_cid = ?");
        values.push(Box::new(avatar.clone()));
    }

    if set_clauses.is_empty() {
        return Err("no fields to update".into());
    }

    set_clauses.push("updated_at = datetime('now')");

    let sql = format!(
        "UPDATE local_identity SET {} WHERE id = 1",
        set_clauses.join(", ")
    );

    let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();

    db.conn()
        .execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;

    db.conn()
        .query_row(
            "SELECT stake_address, payment_address, display_name, bio, avatar_cid, profile_hash, created_at, updated_at
             FROM local_identity WHERE id = 1",
            [],
            |row| {
                Ok(Identity {
                    stake_address: row.get(0)?,
                    payment_address: row.get(1)?,
                    display_name: row.get(2)?,
                    bio: row.get(3)?,
                    avatar_cid: row.get(4)?,
                    profile_hash: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}

/// Publish the active profile to iroh, signing with the wallet key.
#[tauri::command]
pub async fn publish_profile(state: State<'_, AppState>) -> Result<PublishProfileResult, String> {
    let keystore = state.keystore.lock().await;
    let ks = keystore.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(keystore);

    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;

    let (stake_address, display_name, bio, avatar_cid, created_at_str): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    ) = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT stake_address, display_name, bio, avatar_cid, created_at
                 FROM local_identity WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(|e| e.to_string())?
    };

    let created_at = parse_datetime_to_unix(&created_at_str);
    let updated_at = chrono::Utc::now().timestamp();

    let payload = ProfilePayload {
        version: 1,
        stake_address,
        name: display_name,
        bio,
        avatar_hash: avatar_cid,
        created_at,
        updated_at,
    };

    let signed = ipfs_profile::sign_profile(&payload, &w.signing_key).map_err(|e| e.to_string())?;

    let content_node = state.content_node_required().await?;
    let result = ipfs_profile::publish_profile(&content_node, &signed)
        .await
        .map_err(|e| e.to_string())?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    db.conn()
        .execute(
            "UPDATE local_identity SET profile_hash = ?1, updated_at = datetime('now') WHERE id = 1",
            params![result.profile_hash],
        )
        .map_err(|e| e.to_string())?;

    Ok(result)
}

/// Resolve a profile from iroh by BLAKE3 hash.
#[tauri::command]
pub async fn resolve_profile(
    state: State<'_, AppState>,
    hash: String,
) -> Result<SignedProfile, String> {
    let content_node = state.content_node_required().await?;
    ipfs_profile::resolve_profile(&content_node, &hash)
        .await
        .map_err(|e| e.to_string())
}

fn parse_datetime_to_unix(datetime_str: &str) -> i64 {
    chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or_else(|_| chrono::Utc::now().timestamp())
}
