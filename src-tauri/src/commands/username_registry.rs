//! DHT username registry commands (Phase 1: bare signed claims).
//!
//! `claim_username` publishes the active profile's signed
//! `@username → DID` claim to the Kademlia DHT; `resolve_username` and
//! `check_username_availability` read claims back, verify signatures,
//! apply the deterministic conflict ordering, and cache the winner in
//! `username_claims`. Relay receipts (P2) and Cardano anchoring (P3 —
//! batched, ~0.011 ADA/user) strengthen the same record format later.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::crypto::wallet;
use crate::domain::username_claim::{best_claim, dht_key, UsernameClaim};
use crate::AppState;

/// Reserved handles that signup must refuse.
const RESERVED: &[&str] = &[
    "alexandria",
    "admin",
    "administrator",
    "root",
    "support",
    "system",
    "moderator",
    "official",
];

pub fn is_reserved(username: &str) -> bool {
    RESERVED.contains(&username)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailabilityResult {
    pub username: String,
    /// `true` when no verified claim by another DID exists.
    pub available: bool,
    /// DID holding the winning claim, when taken.
    pub taken_by: Option<String>,
    /// `false` when the DHT was unreachable — availability is then a
    /// local-cache-only answer and signup should warn, not block.
    pub authoritative: bool,
}

fn cache_claim(conn: &Connection, claim: &UsernameClaim) -> Result<(), String> {
    let json = serde_json::to_string(claim).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO username_claims (username, did, claimed_at, tier, claim_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
         ON CONFLICT(username) DO UPDATE SET
             did = excluded.did,
             claimed_at = excluded.claimed_at,
             tier = excluded.tier,
             claim_json = excluded.claim_json,
             updated_at = excluded.updated_at",
        rusqlite::params![
            claim.username,
            claim.did,
            claim.claimed_at,
            claim.tier(),
            json
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn cached_claim(conn: &Connection, username: &str) -> Option<UsernameClaim> {
    conn.query_row(
        "SELECT claim_json FROM username_claims WHERE username = ?1",
        [username],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|json| serde_json::from_str(&json).ok())
}

/// Gather every claim visible for a username: DHT records (when the
/// node is up) plus the local cache, verified + deterministically
/// ordered. Returns `(winner, dht_reachable)`.
pub(crate) async fn resolve_claims(
    state: &State<'_, AppState>,
    username: &str,
) -> Result<(Option<UsernameClaim>, bool), String> {
    let mut candidates: Vec<UsernameClaim> = Vec::new();
    {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        if let Some(db) = guard.as_ref() {
            if let Some(c) = cached_claim(db.conn(), username) {
                candidates.push(c);
            }
        }
    }

    let mut dht_reachable = false;
    {
        let node_guard = state.p2p_node.lock().await;
        if let Some(node) = node_guard.as_ref() {
            if let Ok(records) = node.get_dht_records(dht_key(username)).await {
                dht_reachable = true;
                for raw in records {
                    if let Ok(c) = serde_json::from_slice::<UsernameClaim>(&raw) {
                        if c.username == username {
                            // Strip receipts that aren't from a trusted
                            // relay — they must not inflate the tier.
                            candidates.push(crate::p2p::username_reg::sanitize_claim(c));
                        }
                    }
                }
            }
        }
    }

    let winner = best_claim(candidates);
    if let Some(ref w) = winner {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        if let Some(db) = guard.as_ref() {
            let _ = cache_claim(db.conn(), w);
        }
    }
    Ok((winner, dht_reachable))
}

/// Check whether a username is free to claim.
#[tauri::command]
pub async fn check_username_availability(
    state: State<'_, AppState>,
    username: String,
) -> Result<AvailabilityResult, String> {
    let username = crate::domain::identity::validate_username(&username)?;
    if is_reserved(&username) {
        return Ok(AvailabilityResult {
            username,
            available: false,
            taken_by: Some("reserved".to_string()),
            authoritative: true,
        });
    }
    let (winner, dht_reachable) = resolve_claims(&state, &username).await?;
    Ok(AvailabilityResult {
        username,
        available: winner.is_none(),
        taken_by: winner.map(|c| c.did),
        authoritative: dht_reachable,
    })
}

/// Resolve a username to the DID holding the winning claim.
#[tauri::command]
pub async fn resolve_username(
    state: State<'_, AppState>,
    username: String,
) -> Result<Option<UsernameClaim>, String> {
    let username = crate::domain::identity::validate_username(&username)?;
    let (winner, _) = resolve_claims(&state, &username).await?;
    Ok(winner)
}

/// Publish the active profile's signed username claim to the DHT and
/// cache it locally. Idempotent — re-publishing refreshes the record
/// (kad records expire, so the frontend calls this on unlock too).
#[tauri::command]
pub async fn claim_username(state: State<'_, AppState>) -> Result<UsernameClaim, String> {
    // Username from the identity row.
    let username: String = {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT username FROM local_identity WHERE id = 1",
                [],
                |r| r.get::<_, Option<String>>(0),
            )
            .map_err(|e| e.to_string())?
            .ok_or("no username set on this profile")?
    };
    let username = crate::domain::identity::validate_username(&username)?;
    if is_reserved(&username) {
        return Err("this username is reserved".to_string());
    }

    // Signing key from the unlocked wallet.
    let signing_key = {
        let ks_guard = state.keystore.lock().await;
        let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        drop(ks_guard);
        let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
        w.signing_key.clone()
    };
    let did = crate::crypto::did::derive_did_key(&signing_key);

    // Refuse to claim over a verified earlier claim by someone else.
    let (existing, _) = resolve_claims(&state, &username).await?;
    if let Some(ref e) = existing {
        if e.did != did.as_str() {
            return Err(format!("@{username} is already claimed by another user"));
        }
    }

    let claimed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Re-publishing keeps the ORIGINAL claim time — refreshing a
    // record must not reset your priority.
    let claim = match existing {
        Some(e) if e.did == did.as_str() => e,
        _ => UsernameClaim::create(&username, &did, claimed_at, &signing_key),
    };

    // Cache locally first; DHT publish is best-effort (offline nodes
    // claim locally and the unlock-time republish wins the race later).
    {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        cache_claim(db.conn(), &claim)?;
    }

    // Upgrade to tier 1: ask a trusted relay to countersign with its
    // first-seen timestamp. Best-effort — bare claims still publish.
    let mut claim = claim;
    if claim.receipt.is_none() {
        let node_guard = state.p2p_node.lock().await;
        if let Some(node) = node_guard.as_ref() {
            for relay in crate::p2p::discovery::relay_peer_ids() {
                let req = crate::p2p::username_reg::ReceiptRequest {
                    claim: claim.clone(),
                };
                match node.request_username_receipt(relay, req).await {
                    Ok(crate::p2p::username_reg::ReceiptResponse::Granted(receipt)) => {
                        if crate::p2p::username_reg::verify_receipt(&claim.sig, &receipt) {
                            claim.receipt = Some(receipt);
                            break;
                        }
                        log::warn!("relay receipt failed verification — ignoring");
                    }
                    Ok(crate::p2p::username_reg::ReceiptResponse::Refused {
                        reason,
                        existing_did,
                        ..
                    }) => {
                        // A refusal for another DID means we lost the
                        // first-seen race at the relay.
                        if let Some(other) = existing_did {
                            if other != claim.did {
                                return Err(format!(
                                    "@{username} is already registered to another user ({reason})"
                                ));
                            }
                        }
                        log::warn!("relay refused receipt: {reason}");
                    }
                    Err(e) => {
                        log::debug!("relay receipt request failed: {e}");
                    }
                }
            }
        }
    }

    // Re-cache with the receipt attached (tier 1) and publish.
    {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        cache_claim(db.conn(), &claim)?;
    }
    let payload = serde_json::to_vec(&claim).map_err(|e| e.to_string())?;
    {
        let node_guard = state.p2p_node.lock().await;
        if let Some(node) = node_guard.as_ref() {
            if let Err(e) = node.put_dht_record(dht_key(&username), payload).await {
                log::warn!("username claim DHT publish failed (will retry on unlock): {e}");
            }
        } else {
            log::info!("username claim cached; DHT publish deferred until P2P starts");
        }
    }

    Ok(claim)
}
