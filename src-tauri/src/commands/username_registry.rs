//! DHT username registry commands (Phase 1: bare signed claims).
//!
//! `claim_username` publishes the active profile's signed
//! `@username → DID` claim to the Kademlia DHT; `resolve_username` and
//! `check_username_availability` read claims back, verify signatures,
//! apply the deterministic conflict ordering, and cache the winner in
//! `username_claims`. Relay receipts (P2) and Cardano anchoring (P3 —
//! batched, ~0.011 ADA/user) strengthen the same record format later.

use std::time::Duration;

use crate::profile::scope::ProfileState as State;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

/// Hard ceiling per network operation in the registry path. Kademlia
/// queries run up to 60 s on a sparse DHT — unacceptable behind a
/// button press. Registry ops are best-effort by design (local cache +
/// republish-on-start heal), so we cut them short and move on.
const DHT_OP_TIMEOUT: Duration = Duration::from_secs(8);

/// Generous ceiling for background publish/receipt round-trips. The
/// claim flow and the rename publish run off the UI thread (or at
/// p2p-start), and the path mobile→relay over a circuit can take far
/// longer than the interactive budget above. Still bounded so a dead
/// relay can't wedge the task forever.
const DHT_PUBLISH_TIMEOUT: Duration = Duration::from_secs(25);

use crate::crypto::wallet;
use crate::db::executor::DatabaseWorkload;
use crate::domain::username_claim::{best_claim, dht_key, UsernameClaim};
use crate::AppState;

async fn username_db<T, F>(
    state: &State<'_, AppState>,
    workload: DatabaseWorkload,
    label: &'static str,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&crate::db::Database) -> Result<T, String> + Send + 'static,
{
    state
        .db_executor
        .execute(workload, state.profile_lease(), label, operation)
        .await
}

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

fn commit_username_rename(
    db: &crate::db::Database,
    username: &str,
    claim: &UsernameClaim,
    released_old: Option<&UsernameClaim>,
) -> Result<(), String> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    let updated = tx
        .execute(
            "UPDATE local_identity SET username = ?1, updated_at = datetime('now') WHERE id = 1",
            [username],
        )
        .map_err(|e| e.to_string())?;
    if updated != 1 {
        return Err("local identity not found".to_string());
    }
    cache_claim(&tx, claim)?;
    if let Some(old) = released_old {
        cache_claim(&tx, old)?;
    }
    tx.commit().map_err(|e| e.to_string())
}

fn current_unix_seconds() -> Result<i64, String> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch".to_string())?
        .as_secs();
    i64::try_from(seconds).map_err(|_| "system time is outside the supported range".to_string())
}

/// Claims for `username` published to the DHT, when a node is running, with
/// receipts from untrusted relays stripped. The flag says whether the DHT
/// answered at all.
async fn dht_claims(state: &AppState, username: &str) -> (Vec<UsernameClaim>, bool) {
    let node_guard = state.p2p_node.lock().await;
    let Some(node) = node_guard.as_ref() else {
        return (Vec::new(), false);
    };
    let Ok(Ok(records)) = timeout(DHT_OP_TIMEOUT, node.get_dht_records(dht_key(username))).await
    else {
        return (Vec::new(), false);
    };
    let claims = records
        .into_iter()
        .filter_map(|raw| serde_json::from_slice::<UsernameClaim>(&raw).ok())
        .filter(|claim| claim.username == username)
        // Receipts that aren't from a trusted relay must not inflate the tier.
        .map(crate::p2p::username_reg::sanitize_claim)
        .collect();
    (claims, true)
}

/// Gather every claim visible for a username: DHT records (when the
/// node is up) plus the local cache, verified + deterministically
/// ordered. Returns `(winner, dht_reachable)`.
pub(crate) async fn resolve_claims(
    state: &State<'_, AppState>,
    username: &str,
) -> Result<(Option<UsernameClaim>, bool), String> {
    let mut candidates: Vec<UsernameClaim> = Vec::new();
    let cached_username = username.to_string();
    let (cached, verified_sig) = username_db(
        state,
        DatabaseWorkload::Learner,
        "username.resolve-cache",
        move |db| {
            let cached = cached_claim(db.conn(), &cached_username);
            let verified_sig = db
                .conn()
                .query_row(
                    "SELECT claim_json FROM username_claims
                     WHERE username = ?1 AND anchor_verified = 1",
                    [&cached_username],
                    |r| r.get::<_, String>(0),
                )
                .ok()
                .and_then(|json| {
                    serde_json::from_str::<UsernameClaim>(&json)
                        .ok()
                        .map(|claim| claim.sig)
                });
            Ok((cached, verified_sig))
        },
    )
    .await?;
    if let Some(cached) = cached {
        candidates.push(cached);
    }

    let (published, dht_reachable) = dht_claims(state, username).await;
    if dht_reachable {
        candidates.extend(published);
        // Anchors are only trusted once this node has verified the digest
        // on-chain (anchor_verified, set by the username_anchor tick). An
        // unverified anchor is stripped so a forged tx_hash can't fake tier 2.
        for c in candidates.iter_mut() {
            if c.anchor.is_some() && verified_sig.as_deref() != Some(c.sig.as_str()) {
                c.anchor = None;
            }
        }
    }

    let winner = best_claim(candidates);
    if let Some(winner_to_cache) = winner.clone() {
        username_db(
            state,
            DatabaseWorkload::Learner,
            "username.cache-winner",
            move |db| {
                let _ = cache_claim(db.conn(), &winner_to_cache);
                Ok(())
            },
        )
        .await?;
    }
    Ok((winner, dht_reachable))
}

/// Check whether a username is free to claim.
///
/// Unscoped: signup asks this before any profile exists, so there is no
/// profile session to admit it and no profile database to read. It consults the
/// network only — the DHT when a node is running, otherwise the relays'
/// registries. A scoped version was refused at dispatch during every signup,
/// and the page could only ever say availability was unknown.
#[tauri::command]
pub async fn check_username_availability(
    state: tauri::State<'_, AppState>,
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
    let (mut published, dht_reachable) = dht_claims(&state, &username).await;
    // Without a profile nothing on this device has verified an anchor, so
    // none is trusted. Tiers only decide between claims; any valid claim means
    // the name is taken.
    for claim in published.iter_mut() {
        claim.anchor = None;
    }
    let winner = best_claim(published);
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
    let username = username_db(
        &state,
        DatabaseWorkload::Learner,
        "username.claim-identity",
        |db| {
            db.conn()
                .query_row(
                    "SELECT username FROM local_identity WHERE id = 1",
                    [],
                    |r| r.get::<_, Option<String>>(0),
                )
                .map_err(|e| e.to_string())?
                .ok_or("no username set on this profile".to_string())
        },
    )
    .await?;
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

    let claimed_at = current_unix_seconds()?;
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
    let initial_claim = claim.clone();
    username_db(
        &state,
        DatabaseWorkload::Learner,
        "username.cache-claim",
        move |db| cache_claim(db.conn(), &initial_claim),
    )
    .await?;

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
                let attempt = timeout(
                    DHT_PUBLISH_TIMEOUT,
                    node.request_username_receipt(relay, req),
                )
                .await;
                let Ok(attempt) = attempt else {
                    log::warn!("username relay receipt request timed out");
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
    let receipted_claim = claim.clone();
    username_db(
        &state,
        DatabaseWorkload::Learner,
        "username.cache-receipted-claim",
        move |db| cache_claim(db.conn(), &receipted_claim),
    )
    .await?;
    let payload = serde_json::to_vec(&claim).map_err(|e| e.to_string())?;
    {
        let node_guard = state.p2p_node.lock().await;
        if let Some(node) = node_guard.as_ref() {
            match timeout(
                DHT_PUBLISH_TIMEOUT,
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
    let (username, my_did) = username_db(
        &state,
        DatabaseWorkload::Learner,
        "username.conflict-identity",
        |db| {
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
            Ok((username, my_did))
        },
    )
    .await?;
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

    let my_did = username_db(
        &state,
        DatabaseWorkload::Learner,
        "username.rename-did",
        |db| {
            Ok(crate::settings::SettingsStore::get(
                db.conn(),
                crate::settings::registry::keys::IDENTITY_LOCAL_DID,
            ))
        },
    )
    .await?;
    let (winner, _) = resolve_claims(&state, &username).await?;
    if let Some(w) = winner {
        if w.did != my_did {
            return Err(format!("@{username} is already taken"));
        }
    }

    // Tombstone the old handle: a signed release frees it (at relays
    // and in ordering) after the grace window, instead of leaving it
    // squatted-by-absence forever.
    let replacement_username = username.clone();
    let old_released = username_db(
        &state,
        DatabaseWorkload::Learner,
        "username.rename-current",
        move |db| {
            let old_username: Option<String> = db
                .conn()
                .query_row(
                    "SELECT username FROM local_identity WHERE id = 1",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            Ok(old_username
                .filter(|old| *old != replacement_username)
                .and_then(|old| cached_claim(db.conn(), &old)))
        },
    )
    .await?;

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
    let claimed_at = current_unix_seconds()?;
    let claim = UsernameClaim::create(&username, &did, claimed_at, &signing_key);
    let released_old = old_released.filter(|c| c.did == did.as_str()).map(|mut c| {
        c.release(claimed_at, &signing_key);
        c
    });
    let persisted_username = username.clone();
    let persisted_claim = claim.clone();
    let persisted_release = released_old.clone();
    username_db(
        &state,
        DatabaseWorkload::Learner,
        "username.rename-commit",
        move |db| {
            commit_username_rename(
                db,
                &persisted_username,
                &persisted_claim,
                persisted_release.as_ref(),
            )
        },
    )
    .await?;

    let db_executor = state.db_executor.clone();
    let background_lease = state.profile_lease();
    let node_handle = state.p2p_node.clone();
    let bg_claim = claim.clone();
    let bg_released = released_old;
    state
        .profile_operations
        .spawn_job(async move {
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
                    timeout(
                        DHT_PUBLISH_TIMEOUT,
                        node.request_username_receipt(relay, req),
                    )
                    .await
                {
                    if crate::p2p::username_reg::verify_receipt(&enriched.sig, &receipt) {
                        enriched.add_receipt(receipt);
                    }
                }
            }
            if let Ok(payload) = serde_json::to_vec(&enriched) {
                let _ = timeout(
                    DHT_PUBLISH_TIMEOUT,
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
                        DHT_PUBLISH_TIMEOUT,
                        node.put_dht_record(dht_key(&old.username), payload),
                    )
                    .await;
                }
                for relay in crate::p2p::discovery::relay_peer_ids() {
                    let req = crate::p2p::username_reg::ReceiptRequest { claim: old.clone() };
                    let _ = timeout(
                        DHT_PUBLISH_TIMEOUT,
                        node.request_username_receipt(relay, req),
                    )
                    .await;
                }
            }
            drop(node_guard);
            if !enriched.receipts.is_empty() {
                let _ = db_executor
                    .execute(
                        DatabaseWorkload::Background,
                        background_lease,
                        "username.cache-background-receipts",
                        move |db| cache_claim(db.conn(), &enriched),
                    )
                    .await;
            }
        })
        .await;

    Ok(claim)
}

/// Resolve `@username → DID` via the relays' HTTP registry endpoint.
/// Last-resort binding source when the signed claim isn't fetchable
/// from the DHT (slow mobile link, sparse DHT). Returns the DID the
/// first answering relay reports as the current holder.
pub async fn resolve_username_did_via_relay(username: &str) -> Option<String> {
    let username = username.trim().trim_start_matches('@').to_lowercase();
    for endpoint in crate::p2p::discovery::relay_http_endpoints() {
        let url = format!("{endpoint}/username/{username}");
        let resp = timeout(Duration::from_secs(5), async {
            reqwest::get(&url).await?.json::<serde_json::Value>().await
        })
        .await;
        if let Ok(Ok(body)) = resp {
            if let Some(did) = body.get("did").and_then(|v| v.as_str()) {
                return Some(did.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;

    use super::*;
    use crate::crypto::did::derive_did_key;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open database");
        db.run_migrations().expect("run migrations");
        db.conn()
            .execute(
                "INSERT INTO local_identity \
                 (id, stake_address, payment_address, username) \
                 VALUES (1, 'stake_test1owner', 'addr_test1owner', 'oldname')",
                [],
            )
            .expect("insert identity");
        db
    }

    fn claim(username: &str) -> UsernameClaim {
        let key = SigningKey::from_bytes(&[7; 32]);
        let did = derive_did_key(&key);
        UsernameClaim::create(username, &did, 1_700_000_000, &key)
    }

    #[test]
    fn username_rename_updates_identity_and_claim_together() {
        let db = test_db();
        let new_claim = claim("newname");
        commit_username_rename(&db, "newname", &new_claim, None).unwrap();

        let username: String = db
            .conn()
            .query_row(
                "SELECT username FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(username, "newname");
        assert_eq!(
            cached_claim(db.conn(), "newname").map(|stored| stored.sig),
            Some(new_claim.sig)
        );
    }

    #[test]
    fn username_rename_rolls_back_when_claim_persistence_fails() {
        let db = test_db();
        db.conn()
            .execute_batch(
                "CREATE TRIGGER fail_username_claim \
                 BEFORE INSERT ON username_claims \
                 BEGIN SELECT RAISE(FAIL, 'injected claim failure'); END;",
            )
            .unwrap();

        let error = commit_username_rename(&db, "newname", &claim("newname"), None)
            .expect_err("claim failure must abort the identity update");
        assert!(error.contains("injected claim failure"));
        let username: String = db
            .conn()
            .query_row(
                "SELECT username FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(username, "oldname");
        assert!(cached_claim(db.conn(), "newname").is_none());
    }
    /// Signup checks a name before any profile exists: no session header, no
    /// profile database. The check must be dispatched and answer. A reserved
    /// name answers without the network, so this runs offline.
    #[tokio::test]
    async fn availability_is_checked_before_any_profile_exists() {
        use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets};

        let directory = tempfile::TempDir::new().expect("temporary app directory");
        let state = crate::profile::lifecycle_tests::state_in(directory.path());
        let state = std::sync::Arc::try_unwrap(state).unwrap_or_else(|_| panic!("unique state"));
        assert!(
            state.db.lock().expect("db lock").is_none(),
            "no profile is open"
        );
        let app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![check_username_availability])
            .build(mock_context(noop_assets()))
            .expect("mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview");

        let response = tokio::task::spawn_blocking(move || {
            get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: "check_username_availability".into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: if cfg!(any(target_os = "windows", target_os = "android")) {
                        "http://tauri.localhost"
                    } else {
                        "tauri://localhost"
                    }
                    .parse()
                    .expect("local URL"),
                    body: tauri::ipc::InvokeBody::Json(serde_json::json!({"username": "admin"})),
                    headers: tauri::http::HeaderMap::new(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .map(|body| {
                body.deserialize::<serde_json::Value>()
                    .expect("JSON response")
            })
        })
        .await
        .expect("IPC task");

        let result = response.expect("the check is dispatched without a profile session");
        assert_eq!(result["available"], false);
        assert_eq!(result["taken_by"], "reserved");
        assert_eq!(result["authoritative"], true);
    }
}
