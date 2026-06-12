//! DHT username registry commands (Phase 1: bare signed claims).
//!
//! `claim_username` publishes the active profile's signed
//! `@username → DID` claim to the Kademlia DHT; `resolve_username` and
//! `check_username_availability` read claims back, verify signatures,
//! apply the deterministic conflict ordering, and cache the winner in
//! `username_claims`. Relay receipts (P2) and Cardano anchoring (P3 —
//! batched, ~0.011 ADA/user) strengthen the same record format later.

use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::time::timeout;

/// Hard ceiling per network operation in the registry path. Kademlia
/// queries run up to 60 s on a sparse DHT — unacceptable behind a
/// button press. Registry ops are best-effort by design (local cache +
/// republish-on-start heal), so we cut them short and move on.
const DHT_OP_TIMEOUT: Duration = Duration::from_secs(8);

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
    .and_then(|json| serde_json::from_str::<UsernameClaim>(&json).ok())
    .map(UsernameClaim::normalize)
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
            if let Ok(Ok(records)) =
                timeout(DHT_OP_TIMEOUT, node.get_dht_records(dht_key(username))).await
            {
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
                // Anchors are only trusted once this node has verified
                // the digest on-chain (anchor_verified, set by the
                // username_anchor tick). An unverified anchor is
                // stripped so a forged tx_hash can't fake tier 2.
                let verified_sig: Option<String> = {
                    let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
                    guard.as_ref().and_then(|db| {
                        db.conn()
                            .query_row(
                                "SELECT claim_json FROM username_claims
                                 WHERE username = ?1 AND anchor_verified = 1",
                                [username],
                                |r| r.get::<_, String>(0),
                            )
                            .ok()
                            .and_then(|json| {
                                serde_json::from_str::<UsernameClaim>(&json)
                                    .ok()
                                    .map(|c| c.sig)
                            })
                    })
                };
                for c in candidates.iter_mut() {
                    if c.anchor.is_some() && verified_sig.as_deref() != Some(c.sig.as_str()) {
                        c.anchor = None;
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
    if dht_reachable || winner.is_some() {
        return Ok(AvailabilityResult {
            username,
            available: winner.is_none(),
            taken_by: winner.map(|c| c.did),
            authoritative: dht_reachable,
        });
    }

    // No P2P yet (e.g. signup runs before any profile/wallet exists) —
    // ask the relays' HTTP registry endpoints instead. Receipt stores
    // are per-relay (and one region may run ephemeral), so query ALL
    // relays: taken if any says taken; authoritative if any answered.
    let mut any_answered = false;
    for endpoint in crate::p2p::discovery::relay_http_endpoints() {
        let url = format!("{endpoint}/username/{username}");
        let resp = timeout(Duration::from_secs(5), async {
            reqwest::get(&url).await?.json::<serde_json::Value>().await
        })
        .await;
        if let Ok(Ok(body)) = resp {
            any_answered = true;
            let available = body
                .get("available")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if !available {
                return Ok(AvailabilityResult {
                    username,
                    available: false,
                    taken_by: body
                        .get("did")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    authoritative: true,
                });
            }
        }
    }

    Ok(AvailabilityResult {
        username,
        available: true,
        taken_by: None,
        authoritative: any_answered,
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
    // record must not reset your priority. Re-claiming your own
    // released name within the grace window undoes the release.
    let claim = match existing {
        Some(mut e) if e.did == did.as_str() => {
            e.release = None;
            e
        }
        _ => UsernameClaim::create(&username, &did, claimed_at, &signing_key),
    };

    // Cache locally first; DHT publish is best-effort (offline nodes
    // claim locally and the unlock-time republish wins the race later).
    {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        cache_claim(db.conn(), &claim)?;
    }

    // Upgrade to tier 1: gather countersignatures from EVERY trusted
    // relay (receipt diversity — ordering uses the median time, so one
    // relay can't move the clock). Best-effort: bare claims publish.
    let mut claim = claim.normalize();
    {
        let node_guard = state.p2p_node.lock().await;
        if let Some(node) = node_guard.as_ref() {
            let mut refused_for_other = 0u32;
            for relay in crate::p2p::discovery::relay_peer_ids() {
                if claim
                    .receipts
                    .iter()
                    .any(|r| r.relay_peer_id == relay.to_string())
                {
                    continue; // already hold this relay's receipt
                }
                let req = crate::p2p::username_reg::ReceiptRequest {
                    claim: claim.clone(),
                };
                let attempt =
                    timeout(DHT_OP_TIMEOUT, node.request_username_receipt(relay, req)).await;
                let Ok(attempt) = attempt else {
                    log::debug!("relay receipt request timed out");
                    continue;
                };
                match attempt {
                    Ok(crate::p2p::username_reg::ReceiptResponse::Granted(receipt)) => {
                        if crate::p2p::username_reg::verify_receipt(&claim.sig, &receipt) {
                            claim.add_receipt(receipt);
                        } else {
                            log::warn!("relay receipt failed verification — ignoring");
                        }
                    }
                    Ok(crate::p2p::username_reg::ReceiptResponse::Refused {
                        reason,
                        existing_did,
                        ..
                    }) => {
                        if existing_did.as_deref().is_some_and(|d| d != claim.did) {
                            refused_for_other += 1;
                        }
                        log::warn!("relay refused receipt: {reason}");
                    }
                    Err(e) => {
                        log::debug!("relay receipt request failed: {e}");
                    }
                }
            }
            // Only "someone else holds this" with zero receipts of our
            // own is a hard error — a single relay's view is no longer
            // authoritative under receipt diversity.
            if claim.receipts.is_empty() && refused_for_other > 0 {
                return Err(format!("@{username} is already registered to another user"));
            }
        }
    }

    // Re-cache with receipts attached (tier 1) and publish.
    {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        cache_claim(db.conn(), &claim)?;
    }
    let payload = serde_json::to_vec(&claim).map_err(|e| e.to_string())?;
    {
        let node_guard = state.p2p_node.lock().await;
        if let Some(node) = node_guard.as_ref() {
            match timeout(
                DHT_OP_TIMEOUT,
                node.put_dht_record(dht_key(&username), payload),
            )
            .await
            {
                Ok(Err(e)) => {
                    log::warn!("username claim DHT publish failed (will retry on unlock): {e}");
                }
                Err(_) => {
                    log::warn!("username claim DHT publish timed out (will retry on unlock)");
                }
                Ok(Ok(())) => {}
            }
        } else {
            log::info!("username claim cached; DHT publish deferred until P2P starts");
        }
    }

    Ok(claim)
}

/// Conflict status for the active profile's username: someone else's
/// claim deterministically beats ours.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsernameConflict {
    pub username: String,
    pub winner_did: String,
}

/// Check whether the active profile still holds its username. Returns
/// a conflict when the registry's winning claim belongs to another DID
/// — the UI prompts the deterministic loser to pick a new handle.
#[tauri::command]
pub async fn check_my_username_conflict(
    state: State<'_, AppState>,
) -> Result<Option<UsernameConflict>, String> {
    let (username, my_did) = {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        let username: Option<String> = db
            .conn()
            .query_row(
                "SELECT username FROM local_identity WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let my_did = crate::settings::SettingsStore::get(
            db.conn(),
            crate::settings::registry::keys::IDENTITY_LOCAL_DID,
        );
        (username, my_did)
    };
    let Some(username) = username else {
        return Ok(None);
    };
    if my_did.is_empty() {
        return Ok(None);
    }
    let (winner, _) = resolve_claims(&state, &username).await?;
    Ok(match winner {
        Some(c) if c.did != my_did => Some(UsernameConflict {
            username,
            winner_did: c.did,
        }),
        _ => None,
    })
}

/// Change the active profile's username (conflict recovery, or by
/// choice). Validates, checks availability, updates the identity row,
/// and publishes a fresh claim. The old claim is simply no longer
/// refreshed — it ages out of the DHT at record expiry.
#[tauri::command]
pub async fn set_username(
    state: State<'_, AppState>,
    username: String,
) -> Result<UsernameClaim, String> {
    let username = crate::domain::identity::validate_username(&username)?;
    if is_reserved(&username) {
        return Err("this username is reserved".to_string());
    }

    let my_did = {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        crate::settings::SettingsStore::get(
            db.conn(),
            crate::settings::registry::keys::IDENTITY_LOCAL_DID,
        )
    };
    let (winner, _) = resolve_claims(&state, &username).await?;
    if let Some(w) = winner {
        if w.did != my_did {
            return Err(format!("@{username} is already taken"));
        }
    }

    // Tombstone the old handle: a signed release frees it (at relays
    // and in ordering) after the grace window, instead of leaving it
    // squatted-by-absence forever.
    let old_released: Option<UsernameClaim> = {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        let old_username: Option<String> = db
            .conn()
            .query_row(
                "SELECT username FROM local_identity WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        old_username
            .filter(|old| *old != username)
            .and_then(|old| cached_claim(db.conn(), &old))
    };

    {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .execute(
                "UPDATE local_identity SET username = ?1, updated_at = datetime('now') WHERE id = 1",
                [&username],
            )
            .map_err(|e| e.to_string())?;
    }

    // Sign + cache the claim locally so the rename is durable and the
    // UI returns immediately. Receipt + DHT publish run in the
    // background — they're best-effort (republished on every p2p
    // start) and can take several network round-trips.
    let signing_key = {
        let ks_guard = state.keystore.lock().await;
        let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        drop(ks_guard);
        let w =
            crate::crypto::wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
        w.signing_key.clone()
    };
    let did = crate::crypto::did::derive_did_key(&signing_key);
    let claimed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let claim = UsernameClaim::create(&username, &did, claimed_at, &signing_key);
    let released_old = old_released.filter(|c| c.did == did.as_str()).map(|mut c| {
        c.release(claimed_at, &signing_key);
        c
    });
    {
        let guard = state.db.lock().map_err(|_| "database lock poisoned")?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        cache_claim(db.conn(), &claim)?;
        if let Some(ref old) = released_old {
            cache_claim(db.conn(), old)?;
        }
    }

    let db = state.db.clone();
    let node_handle = state.p2p_node.clone();
    let bg_claim = claim.clone();
    let bg_released = released_old;
    tauri::async_runtime::spawn(async move {
        let node_guard = node_handle.lock().await;
        let Some(node) = node_guard.as_ref() else {
            return;
        };
        // Receipts (tier 1) from every relay — best-effort.
        let mut enriched = bg_claim;
        for relay in crate::p2p::discovery::relay_peer_ids() {
            let req = crate::p2p::username_reg::ReceiptRequest {
                claim: enriched.clone(),
            };
            if let Ok(Ok(crate::p2p::username_reg::ReceiptResponse::Granted(receipt))) =
                timeout(DHT_OP_TIMEOUT, node.request_username_receipt(relay, req)).await
            {
                if crate::p2p::username_reg::verify_receipt(&enriched.sig, &receipt) {
                    enriched.add_receipt(receipt);
                }
            }
        }
        if !enriched.receipts.is_empty() {
            if let Ok(guard) = db.lock() {
                if let Some(database) = guard.as_ref() {
                    let _ = cache_claim(database.conn(), &enriched);
                }
            }
        }
        if let Ok(payload) = serde_json::to_vec(&enriched) {
            let _ = timeout(
                DHT_OP_TIMEOUT,
                node.put_dht_record(dht_key(&enriched.username), payload),
            )
            .await;
        }
        // Publish the old handle's tombstone: DHT record + a receipt
        // round to each relay so their first-seen stores learn the
        // release and free the name after grace.
        if let Some(old) = bg_released {
            if let Ok(payload) = serde_json::to_vec(&old) {
                let _ = timeout(
                    DHT_OP_TIMEOUT,
                    node.put_dht_record(dht_key(&old.username), payload),
                )
                .await;
            }
            for relay in crate::p2p::discovery::relay_peer_ids() {
                let req = crate::p2p::username_reg::ReceiptRequest { claim: old.clone() };
                let _ = timeout(DHT_OP_TIMEOUT, node.request_username_receipt(relay, req)).await;
            }
        }
    });

    Ok(claim)
}
