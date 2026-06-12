//! Pull-based public profile fetch over `/alexandria/profile-fetch/1.0`.
//!
//! A node serves *its own owner's* profile (username, display name,
//! bio, avatar). Requests address the owner either by DID or by
//! username — the latter powers @username lookup across the network.
//! A node answers `Ok` only if it owns the requested identity,
//! `Private` if it owns it but the profile is private, and `NotOwner`
//! otherwise so a broadcast caller can move on to the next peer.
//!
//! Privacy notes:
//!   - A private profile queried **by DID** answers `Private` (the DID
//!     is already known to the caller; only the fields are withheld).
//!   - A private profile queried **by username** answers `NotOwner` —
//!     answering `Private` would leak the username→DID binding.
//!
//! Mirrors the request-response wiring of [`super::graph_fetch`].

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::crypto::did::Did;
use crate::settings::{registry::keys, SettingsStore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileFetchRequest {
    /// DID whose profile is requested (exact match), or…
    pub subject_did: Option<String>,
    /// …username to look up (case-insensitive). Exactly one should be set.
    pub username: Option<String>,
    /// DID of the requesting node.
    pub requestor: Did,
    /// Replay-protection nonce.
    pub nonce: String,
}

/// The public view of a user profile, as served over the wire and
/// cached in `peer_profiles`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublicProfile {
    pub did: String,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_cid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProfileFetchResponse {
    Ok(Box<PublicProfile>),
    /// We own the requested DID but the profile is private.
    Private,
    /// This node does not own the requested identity.
    NotOwner,
}

struct OwnerRow {
    username: Option<String>,
    display_name: Option<String>,
    bio: Option<String>,
    avatar_cid: Option<String>,
    visibility: Option<String>,
}

fn read_owner_row(conn: &Connection) -> Option<OwnerRow> {
    conn.query_row(
        "SELECT username, display_name, bio, avatar_cid, visibility
         FROM local_identity WHERE id = 1",
        [],
        |row| {
            Ok(OwnerRow {
                username: row.get(0)?,
                display_name: row.get(1)?,
                bio: row.get(2)?,
                avatar_cid: row.get(3)?,
                visibility: row.get(4)?,
            })
        },
    )
    .ok()
}

/// Build the owner's public profile (no visibility filtering — the
/// caller decides based on context, e.g. the owner's own settings UI).
pub fn build_own_profile(conn: &Connection) -> Option<PublicProfile> {
    let local_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
    if local_did.is_empty() {
        return None;
    }
    let row = read_owner_row(conn)?;
    Some(PublicProfile {
        did: local_did,
        username: row.username,
        display_name: row.display_name,
        bio: row.bio,
        avatar_cid: row.avatar_cid,
    })
}

/// Handle an inbound profile-fetch request against the local DB.
pub fn handle_profile_fetch_request(
    conn: &Connection,
    req: &ProfileFetchRequest,
) -> Result<ProfileFetchResponse, String> {
    let local_did = SettingsStore::get(conn, keys::IDENTITY_LOCAL_DID);
    if local_did.is_empty() {
        return Ok(ProfileFetchResponse::NotOwner);
    }
    let Some(row) = read_owner_row(conn) else {
        return Ok(ProfileFetchResponse::NotOwner);
    };
    let is_private = row.visibility.as_deref() == Some("private");

    let matched_by_did = req.subject_did.as_deref() == Some(local_did.as_str());
    let matched_by_username = match (&req.username, &row.username) {
        (Some(q), Some(u)) => q.trim().to_lowercase() == *u,
        _ => false,
    };

    if matched_by_did {
        if is_private {
            return Ok(ProfileFetchResponse::Private);
        }
    } else if matched_by_username {
        if is_private {
            // Don't leak the username→DID binding for private profiles.
            return Ok(ProfileFetchResponse::NotOwner);
        }
    } else {
        return Ok(ProfileFetchResponse::NotOwner);
    }

    Ok(ProfileFetchResponse::Ok(Box::new(PublicProfile {
        did: local_did,
        username: row.username,
        display_name: row.display_name,
        bio: row.bio,
        avatar_cid: row.avatar_cid,
    })))
}

/// Upsert a fetched peer profile into the local cache.
pub fn cache_peer_profile(conn: &Connection, p: &PublicProfile) -> Result<(), String> {
    conn.execute(
        "INSERT INTO peer_profiles (did, username, display_name, bio, avatar_cid, visibility, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'public', datetime('now'))
         ON CONFLICT(did) DO UPDATE SET
             username = excluded.username,
             display_name = excluded.display_name,
             bio = excluded.bio,
             avatar_cid = excluded.avatar_cid,
             updated_at = excluded.updated_at",
        rusqlite::params![p.did, p.username, p.display_name, p.bio, p.avatar_cid],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(visibility: &str) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL,
                 updated_at TEXT, scope TEXT NOT NULL DEFAULT 'sync');
             CREATE TABLE local_identity (id INTEGER PRIMARY KEY, stake_address TEXT,
                 payment_address TEXT, username TEXT, display_name TEXT, bio TEXT,
                 avatar_cid TEXT, visibility TEXT);
             CREATE TABLE peer_profiles (did TEXT PRIMARY KEY, username TEXT,
                 display_name TEXT, bio TEXT, avatar_cid TEXT,
                 visibility TEXT NOT NULL DEFAULT 'public',
                 updated_at TEXT NOT NULL DEFAULT (datetime('now')));",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address, username, display_name, bio, visibility)
             VALUES (1, 'stake1', 'addr1', 'ada_99', 'Ada Lovelace', 'First programmer', ?1)",
            [visibility],
        )
        .unwrap();
        SettingsStore::set(&conn, keys::IDENTITY_LOCAL_DID, "did:key:alice".to_string()).unwrap();
        conn
    }

    fn req_did(did: &str) -> ProfileFetchRequest {
        ProfileFetchRequest {
            subject_did: Some(did.into()),
            username: None,
            requestor: Did("did:key:carol".into()),
            nonce: "n".into(),
        }
    }

    fn req_username(u: &str) -> ProfileFetchRequest {
        ProfileFetchRequest {
            subject_did: None,
            username: Some(u.into()),
            requestor: Did("did:key:carol".into()),
            nonce: "n".into(),
        }
    }

    #[test]
    fn serves_public_profile_by_did_and_username() {
        let conn = setup("public");
        match handle_profile_fetch_request(&conn, &req_did("did:key:alice")).unwrap() {
            ProfileFetchResponse::Ok(p) => {
                assert_eq!(p.username.as_deref(), Some("ada_99"));
                assert_eq!(p.display_name.as_deref(), Some("Ada Lovelace"));
            }
            other => panic!("expected Ok, got {other:?}"),
        }
        // Username lookup is case-insensitive.
        assert!(matches!(
            handle_profile_fetch_request(&conn, &req_username("ADA_99")).unwrap(),
            ProfileFetchResponse::Ok(_)
        ));
    }

    #[test]
    fn private_profile_answers_private_by_did_but_not_owner_by_username() {
        let conn = setup("private");
        assert!(matches!(
            handle_profile_fetch_request(&conn, &req_did("did:key:alice")).unwrap(),
            ProfileFetchResponse::Private
        ));
        // Username query must not leak the binding.
        assert!(matches!(
            handle_profile_fetch_request(&conn, &req_username("ada_99")).unwrap(),
            ProfileFetchResponse::NotOwner
        ));
    }

    #[test]
    fn rejects_non_owner() {
        let conn = setup("public");
        assert!(matches!(
            handle_profile_fetch_request(&conn, &req_did("did:key:bob")).unwrap(),
            ProfileFetchResponse::NotOwner
        ));
        assert!(matches!(
            handle_profile_fetch_request(&conn, &req_username("someone_else")).unwrap(),
            ProfileFetchResponse::NotOwner
        ));
    }

    #[test]
    fn cache_upserts() {
        let conn = setup("public");
        let p = PublicProfile {
            did: "did:key:bob".into(),
            username: Some("bob".into()),
            display_name: Some("Bob".into()),
            bio: None,
            avatar_cid: None,
        };
        cache_peer_profile(&conn, &p).unwrap();
        let p2 = PublicProfile {
            display_name: Some("Bobby".into()),
            ..p.clone()
        };
        cache_peer_profile(&conn, &p2).unwrap();
        let name: String = conn
            .query_row(
                "SELECT display_name FROM peer_profiles WHERE did = 'did:key:bob'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name, "Bobby");
    }
}
