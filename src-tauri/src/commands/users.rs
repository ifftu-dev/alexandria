//! User-profile commands: fetch another user's public profile over
//! the `/alexandria/profile-fetch/1.0` protocol (by DID or @username),
//! and resolve cached usernames/display names for UI rendering.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::crypto::did::Did;
use crate::p2p::profile_fetch::{
    build_own_profile, cache_peer_profile, ProfileFetchRequest, ProfileFetchResponse, PublicProfile,
};
use crate::settings::{registry::keys, SettingsStore};
use crate::AppState;

/// Username + display name for one DID, for name rendering in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedName {
    pub username: Option<String>,
    pub display_name: Option<String>,
}

/// Resolve usernames/display names for a batch of DIDs from local
/// knowledge only (own identity + the `peer_profiles` cache). No
/// network traffic — surfaces improve as the cache fills.
#[tauri::command]
pub async fn resolve_profiles(
    state: State<'_, AppState>,
    dids: Vec<String>,
) -> Result<HashMap<String, ResolvedName>, String> {
    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let mut out = HashMap::new();
    let local_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);

    for did in dids {
        if !local_did.is_empty() && did == local_did {
            if let Some(p) = build_own_profile(conn) {
                out.insert(
                    did,
                    ResolvedName {
                        username: p.username,
                        display_name: p.display_name,
                    },
                );
            }
            continue;
        }
        let row = conn
            .query_row(
                "SELECT username, display_name FROM peer_profiles WHERE did = ?1",
                [&did],
                |r| {
                    Ok(ResolvedName {
                        username: r.get(0)?,
                        display_name: r.get(1)?,
                    })
                },
            )
            .ok();
        if let Some(r) = row {
            out.insert(did, r);
        }
    }
    Ok(out)
}

/// Fetch a user's public profile by DID or @username.
///
/// Resolution order:
///   1. Own identity (loopback).
///   2. The `peer_profiles` cache (skipped when `force` is true).
///   3. Broadcast over `/alexandria/profile-fetch/1.0` to known peers;
///      the owner's node answers. Successful fetches refresh the cache.
#[tauri::command]
pub async fn fetch_user_profile(
    state: State<'_, AppState>,
    did: Option<String>,
    username: Option<String>,
    force: Option<bool>,
) -> Result<PublicProfile, String> {
    let force = force.unwrap_or(false);
    let username = username.map(|u| u.trim().trim_start_matches('@').to_lowercase());
    if did.is_none() && username.is_none() {
        return Err("provide a DID or a username".to_string());
    }

    // 1 + 2: local answers, releasing the std lock before any await.
    let requestor_did = {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();

        if let Some(own) = build_own_profile(conn) {
            let own_username_match = match (&username, &own.username) {
                (Some(q), Some(u)) => q == u,
                _ => false,
            };
            if did.as_deref() == Some(own.did.as_str()) || own_username_match {
                return Ok(own);
            }
        }

        if !force {
            let cached = if let Some(ref d) = did {
                lookup_cache(conn, "did = ?1", d)
            } else if let Some(ref u) = username {
                lookup_cache(conn, "username = ?1", u)
            } else {
                None
            };
            if let Some(p) = cached {
                return Ok(p);
            }
        }

        SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID)
    };

    // 3: network broadcast.
    let requestor = Did(if requestor_did.is_empty() {
        "did:key:unknown".to_string()
    } else {
        requestor_did
    });
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_default();

    let node_guard = state.p2p_node.lock().await;
    let node = node_guard.as_ref().ok_or("P2P node not running")?;
    let peers = node
        .known_peers()
        .await
        .map_err(|e| format!("failed to list peers: {e}"))?;
    if peers.is_empty() {
        return Err("no known peers to fetch profile from".to_string());
    }

    let (mut private, mut not_owner, mut unreachable) = (0u32, 0u32, 0u32);
    for peer_str in peers {
        let Ok(peer) = peer_str.parse::<libp2p::PeerId>() else {
            continue;
        };
        let req = ProfileFetchRequest {
            subject_did: did.clone(),
            username: username.clone(),
            requestor: requestor.clone(),
            nonce: nonce.clone(),
        };
        match node.fetch_profile(peer, req).await {
            Ok(ProfileFetchResponse::Ok(profile)) => {
                let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
                if let Some(db) = guard.as_ref() {
                    let _ = cache_peer_profile(db.conn(), &profile);
                }
                return Ok(*profile);
            }
            Ok(ProfileFetchResponse::Private) => private += 1,
            Ok(ProfileFetchResponse::NotOwner) => not_owner += 1,
            Err(_) => unreachable += 1,
        }
    }

    if private > 0 {
        return Err("this profile is private".to_string());
    }
    Err(format!(
        "profile not found: {not_owner} peer(s) answered not-owner, {unreachable} unreachable"
    ))
}

fn lookup_cache(
    conn: &rusqlite::Connection,
    where_clause: &str,
    value: &str,
) -> Option<PublicProfile> {
    conn.query_row(
        &format!(
            "SELECT did, username, display_name, bio, avatar_cid
             FROM peer_profiles WHERE {where_clause}"
        ),
        [value],
        |r| {
            Ok(PublicProfile {
                did: r.get(0)?,
                username: r.get(1)?,
                display_name: r.get(2)?,
                bio: r.get(3)?,
                avatar_cid: r.get(4)?,
            })
        },
    )
    .ok()
}
