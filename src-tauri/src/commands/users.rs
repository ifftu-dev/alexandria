//! User-profile commands: fetch another user's public profile over
//! the `/alexandria/profile-fetch/1.0` protocol (by DID or @username),
//! and resolve cached usernames/display names for UI rendering.

use std::collections::HashMap;

use crate::profile::scope::ProfileState as State;
use serde::{Deserialize, Serialize};

use crate::crypto::did::Did;
use crate::db::executor::DatabaseWorkload;
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
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "users.resolve_profiles",
            move |db| Ok(resolve_profiles_db(db, dids)),
        )
        .await
}

fn resolve_profiles_db(
    db: &crate::db::Database,
    dids: Vec<String>,
) -> HashMap<String, ResolvedName> {
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
    out
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
    let mut did = did;
    let username = username.map(|u| u.trim().trim_start_matches('@').to_lowercase());
    if did.is_none() && username.is_none() {
        return Err("provide a DID or a username".to_string());
    }

    // Username lookups go through the DHT registry first: the winning
    // signed claim is the authoritative @username → DID binding, which
    // stops a malicious node answering profile-fetch for a handle it
    // doesn't hold. Falls back to broadcast-by-username when no claim
    // is resolvable (registry empty or DHT unreachable).
    if did.is_none() {
        if let Some(ref u) = username {
            if let Ok((Some(claim), _)) = super::username_registry::resolve_claims(&state, u).await
            {
                did = Some(claim.did);
            }
            // DHT reads can be slow/unreachable on mobile links. The
            // relays' HTTP registry maps @username → DID directly — a
            // last-resort binding source so profile lookup works even
            // when the signed claim isn't fetchable from the DHT in
            // time. (Authority still holds: profile-fetch only returns
            // a profile from the node that actually owns the DID.)
            if did.is_none() {
                did = super::username_registry::resolve_username_did_via_relay(u).await;
            }
        }
    }

    // 1 + 2: local answers, releasing the std lock before any await.
    let lookup_did = did.clone();
    let lookup_username = username.clone();
    let (local_profile, requestor_did) = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "users.fetch_profile.local",
            move |db| {
                Ok(lookup_local_profile(
                    db,
                    &lookup_did,
                    &lookup_username,
                    force,
                ))
            },
        )
        .await?;
    if let Some(profile) = local_profile {
        return Ok(profile);
    }

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
    // Discover + dial Alexandria peers we haven't met yet (the owner
    // of this DID may not be in our routing table), then broadcast.
    let _ = node.discover_peers(std::time::Duration::from_secs(4)).await;
    let peers = node
        .known_peers()
        .await
        .map_err(|e| format!("failed to list peers: {e}"))?;
    if peers.is_empty() {
        return Err("no known peers to fetch profile from".to_string());
    }

    // Broadcast to all known peers concurrently and return the first
    // owner that answers. A serial loop pays each unreachable peer's
    // timeout in sequence; fanning out bounds the total wait to the
    // slowest single response, capped per-request below so a dead or
    // unresponsive peer can't stall the whole fetch.
    use futures::stream::StreamExt;
    const PER_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    let mut inflight = futures::stream::FuturesUnordered::new();
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
        inflight.push(async move {
            tokio::time::timeout(PER_REQUEST_TIMEOUT, node.fetch_profile(peer, req)).await
        });
    }

    let (mut private, mut not_owner, mut unreachable) = (0u32, 0u32, 0u32);
    let mut fetched = None;
    while let Some(res) = inflight.next().await {
        match res {
            Ok(Ok(ProfileFetchResponse::Ok(profile))) => {
                fetched = Some(*profile);
                break;
            }
            Ok(Ok(ProfileFetchResponse::Private)) => private += 1,
            Ok(Ok(ProfileFetchResponse::NotOwner)) => not_owner += 1,
            Ok(Err(_)) => unreachable += 1, // network error
            Err(_) => unreachable += 1,     // per-request timeout
        }
    }
    drop(inflight);
    drop(node_guard);

    if let Some(profile) = fetched {
        let cached_profile = profile.clone();
        state
            .db_executor
            .execute(
                DatabaseWorkload::Background,
                state.profile_lease(),
                "users.fetch_profile.cache",
                move |db| {
                    let _ = cache_peer_profile(db.conn(), &cached_profile);
                    Ok(())
                },
            )
            .await?;
        return Ok(profile);
    }

    if private > 0 {
        return Err("this profile is private".to_string());
    }
    Err(format!(
        "profile not found: {not_owner} peer(s) answered not-owner, {unreachable} unreachable"
    ))
}

fn lookup_local_profile(
    db: &crate::db::Database,
    did: &Option<String>,
    username: &Option<String>,
    force: bool,
) -> (Option<PublicProfile>, String) {
    let conn = db.conn();
    if let Some(own) = build_own_profile(conn) {
        let own_username_match = match (username, &own.username) {
            (Some(query), Some(own_username)) => query == own_username,
            _ => false,
        };
        if did.as_deref() == Some(own.did.as_str()) || own_username_match {
            return (Some(own), String::new());
        }
    }

    if !force {
        let cached = if let Some(did) = did {
            lookup_cache(conn, "did = ?1", did)
        } else if let Some(username) = username {
            lookup_cache(conn, "username = ?1", username)
        } else {
            None
        };
        if cached.is_some() {
            return (cached, String::new());
        }
    }

    (None, SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> crate::db::Database {
        let db = crate::db::Database::open_in_memory().expect("in-memory database");
        db.run_migrations().expect("migrations");
        db.conn()
            .execute(
                "INSERT INTO local_identity \
                 (id, stake_address, payment_address, username, display_name, visibility) \
                 VALUES (1, 'stake_test_alice', 'addr_test_alice', 'alice', 'Alice', 'public')",
                [],
            )
            .expect("local identity");
        SettingsStore::set(
            db.conn(),
            keys::IDENTITY_LOCAL_DID,
            "did:key:alice".to_string(),
        )
        .expect("local DID");
        cache_peer_profile(
            db.conn(),
            &PublicProfile {
                did: "did:key:bob".into(),
                username: Some("bob".into()),
                display_name: Some("Bob".into()),
                bio: None,
                avatar_cid: None,
            },
        )
        .expect("cached peer");
        db
    }

    #[test]
    fn local_lookup_prefers_the_owner_and_returns_the_requestor_identity() {
        let db = database();
        let (own, requestor) =
            lookup_local_profile(&db, &Some("did:key:alice".into()), &None, false);
        assert_eq!(own.expect("owner").display_name.as_deref(), Some("Alice"));
        assert!(requestor.is_empty());

        let (missing, requestor) =
            lookup_local_profile(&db, &Some("did:key:unknown".into()), &None, false);
        assert!(missing.is_none());
        assert_eq!(requestor, "did:key:alice");
    }

    #[test]
    fn force_skips_the_cached_peer_without_losing_the_requestor_identity() {
        let db = database();
        let query = Some("did:key:bob".to_string());
        let (cached, _) = lookup_local_profile(&db, &query, &None, false);
        assert_eq!(
            cached.expect("cached peer").username.as_deref(),
            Some("bob")
        );

        let (forced, requestor) = lookup_local_profile(&db, &query, &None, true);
        assert!(forced.is_none());
        assert_eq!(requestor, "did:key:alice");
    }

    #[test]
    fn batch_resolution_returns_owned_and_cached_profiles_only() {
        let db = database();
        let profiles = resolve_profiles_db(
            &db,
            vec![
                "did:key:alice".into(),
                "did:key:bob".into(),
                "did:key:unknown".into(),
            ],
        );
        assert_eq!(profiles.len(), 2);
        assert_eq!(
            profiles
                .get("did:key:alice")
                .and_then(|profile| profile.display_name.as_deref()),
            Some("Alice")
        );
        assert_eq!(
            profiles
                .get("did:key:bob")
                .and_then(|profile| profile.username.as_deref()),
            Some("bob")
        );
    }
}
