//! IPC commands for cross-device sync.
//!
//! Exposes the sync system to the frontend:
//!   - Device management (register, list, rename, remove)
//!   - Sync status and history
//!   - Manual sync trigger
//!   - Auto-sync toggle

use crate::profile::scope::ProfileState as State;

use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::domain::sync::{DeviceInfo, SyncHistoryEntry, SyncResult, SyncStatus};
use crate::p2p::device_sync::SyncRequest;
use crate::p2p::sync;
use crate::profile::scope::ProfileLease;
use crate::AppState;

/// Get information about the local device.
///
/// Returns the device record for this node. Creates one if it
/// doesn't exist yet (auto-registers on first call).
#[tauri::command]
pub async fn sync_get_device_info(state: State<'_, AppState>) -> Result<DeviceInfo, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sync.device-info",
            move |db| {
                let device_id = sync::get_or_create_local_device(db.conn(), std::env::consts::OS)?;
                sync::get_device(db.conn(), &device_id)
            },
        )
        .await
}

/// Set a user-friendly name for the local device.
#[tauri::command]
pub async fn sync_set_device_name(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sync.device-name.set",
            move |db| {
                let device_id = db
                    .conn()
                    .query_row("SELECT id FROM devices WHERE is_local = 1", [], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(|e| format!("no local device registered: {e}"))?;
                db.conn()
                    .execute(
                        "UPDATE devices SET device_name = ?1 WHERE id = ?2",
                        rusqlite::params![name, device_id],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(())
            },
        )
        .await
}

/// List all known devices (local + remote).
#[tauri::command]
pub async fn sync_list_devices(state: State<'_, AppState>) -> Result<Vec<DeviceInfo>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sync.devices.list",
            move |db| sync::list_devices(db.conn()),
        )
        .await
}

/// Remove a remote device and its sync state.
///
/// Cannot remove the local device.
#[tauri::command]
pub async fn sync_remove_device(
    state: State<'_, AppState>,
    device_id: String,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sync.device.remove",
            move |db| sync::remove_device(db.conn(), &device_id),
        )
        .await
}

/// Get the current sync status.
///
/// Returns device count, queue length, last sync time, and
/// per-device sync summaries.
#[tauri::command]
pub async fn sync_status(state: State<'_, AppState>) -> Result<SyncStatus, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sync.status",
            move |db| sync::get_sync_status(db.conn()),
        )
        .await
}

/// Manually trigger a sync with every paired device.
///
/// Dials each paired peer over `/alexandria/sync/1.0`, exchanges sealed
/// payloads, and merges what comes back (LWW). Returns an aggregate
/// [`SyncResult`] across all peers. Peers that are offline or
/// unreachable are skipped (logged), not fatal.
#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> Result<SyncResult, String> {
    let started = std::time::Instant::now();

    // Snapshot the paired peers up front, then release the DB lock
    // before any network I/O.
    let targets = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sync.targets",
            move |db| {
                sync::prune_delivered_queue(db.conn()).ok();
                sync_targets(db.conn())
            },
        )
        .await?;

    let mut total = SyncResult {
        rows_sent: 0,
        rows_received: 0,
        rows_merged: 0,
        table_stats: vec![],
        duration_ms: 0,
    };

    for (peer_id, key, addrs) in targets {
        match sync_one_peer(
            &state.db_executor,
            state.profile_lease(),
            DatabaseWorkload::Learner,
            &state.p2p_node,
            &peer_id,
            &key,
            &addrs,
        )
        .await
        {
            Ok(r) => {
                total.rows_sent += r.rows_sent;
                total.rows_received += r.rows_received;
                total.rows_merged += r.rows_merged;
            }
            Err(e) => log::warn!("sync: peer {peer_id} skipped: {e}"),
        }
    }

    total.duration_ms = started.elapsed().as_millis() as i64;
    Ok(total)
}

/// A paired peer worth syncing: `(peer_id, shared_key, addresses)`.
type SyncTarget = (String, [u8; 32], Vec<String>);

/// Snapshot the paired peers worth syncing.
fn sync_targets(conn: &rusqlite::Connection) -> Result<Vec<SyncTarget>, String> {
    let mut out = Vec::new();
    for (_device_id, peer_id) in sync::list_paired_peers(conn)? {
        let Some(key) = sync::get_pair_key(conn, &peer_id)? else {
            continue;
        };
        let addrs: Vec<String> = conn
            .query_row(
                "SELECT addresses FROM peers WHERE peer_id = ?1",
                rusqlite::params![peer_id],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        out.push((peer_id, key, addrs));
    }
    Ok(out)
}

/// Background auto-sync driver: if the device-local auto-sync toggle is
/// on, run one sync exchange with every paired peer. Invoked from the
/// app's periodic queue loop. Returns rows merged across all peers.
pub(crate) async fn auto_sync_all(
    db_executor: &DatabaseExecutor,
    lease: &ProfileLease,
    node: &crate::commands::pairing::NodeHandle,
) -> usize {
    let targets = match db_executor
        .execute(
            DatabaseWorkload::Background,
            lease.clone(),
            "sync.auto-targets",
            move |db| {
                if sync::is_auto_sync_enabled(db.conn()) {
                    sync_targets(db.conn())
                } else {
                    Ok(Vec::new())
                }
            },
        )
        .await
    {
        Ok(targets) => targets,
        Err(e) => {
            log::debug!("auto-sync: target query failed: {e}");
            return 0;
        }
    };

    let mut merged = 0usize;
    for (peer_id, key, addrs) in targets {
        match sync_one_peer(
            db_executor,
            lease.clone(),
            DatabaseWorkload::Background,
            node,
            &peer_id,
            &key,
            &addrs,
        )
        .await
        {
            Ok(r) => merged += r.rows_merged.max(0) as usize,
            Err(e) => log::debug!("auto-sync: peer {peer_id} skipped: {e}"),
        }
    }
    merged
}

/// Run a single already-paired sync exchange (no pairing handshake).
async fn sync_one_peer(
    db_executor: &DatabaseExecutor,
    lease: ProfileLease,
    workload: DatabaseWorkload,
    node_handle: &crate::commands::pairing::NodeHandle,
    peer_id: &str,
    key: &[u8; 32],
    addrs: &[String],
) -> Result<SyncResult, String> {
    let started = std::time::Instant::now();

    let key_for_payload = *key;
    let (device_id, stake_address, sealed) = db_executor
        .execute(workload, lease.clone(), "sync.payload.build", move |db| {
            let device_id = sync::get_or_create_local_device(db.conn(), std::env::consts::OS)?;
            let stake = sync::local_stake_address(db.conn())?.ok_or("no local identity")?;
            let payload = sync::build_sync_payload(db.conn())?;
            Ok((
                device_id,
                stake,
                sync::seal_payload(&key_for_payload, &payload)?,
            ))
        })
        .await?;

    let peer: libp2p::PeerId = peer_id.parse().map_err(|e| format!("bad peer id: {e}"))?;
    let multiaddrs: Vec<libp2p::Multiaddr> = addrs.iter().filter_map(|a| a.parse().ok()).collect();
    let request = SyncRequest {
        device_id,
        stake_address,
        sealed,
        pairing: None,
    };

    let response = {
        let node = node_handle.lock().await;
        let node = node.as_ref().ok_or("P2P node not running")?;
        let _ = node.connect_peer(peer, multiaddrs).await;
        node.sync_with_peer(peer, request)
            .await
            .map_err(|e| e.to_string())?
    };

    crate::commands::pairing::finish_exchange(
        db_executor,
        lease,
        workload,
        peer_id,
        key,
        response,
        started,
    )
    .await
}

/// Toggle automatic background sync.
///
/// When enabled, the node will periodically sync with all known
/// remote devices (every 60 seconds when peers are online).
#[tauri::command]
pub async fn sync_set_auto(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sync.auto.set",
            move |db| sync::set_auto_sync(db.conn(), enabled),
        )
        .await?;
    log::info!("sync: auto-sync set to {enabled}");
    Ok(())
}

/// Get sync history (recent sync events).
#[tauri::command]
pub async fn sync_history(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<SyncHistoryEntry>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sync.history",
            move |db| sync::get_sync_history(db.conn(), limit.unwrap_or(50)),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    #[test]
    fn device_registration_and_naming() {
        let db = test_db();
        let conn = db.conn();

        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1u', 'addr_test1q')",
            [],
        )
        .unwrap();

        let device_id = sync::register_local_device(conn, Some("My Mac"), "macos").unwrap();

        // Rename
        conn.execute(
            "UPDATE devices SET device_name = 'Work Laptop' WHERE id = ?1",
            rusqlite::params![device_id],
        )
        .unwrap();

        let device = sync::get_device(conn, &device_id).unwrap();
        assert_eq!(device.device_name.as_deref(), Some("Work Laptop"));
        assert!(device.is_local);
    }

    #[test]
    fn sync_status_with_devices() {
        let db = test_db();
        let conn = db.conn();

        sync::register_remote_device(conn, "r1", Some("Phone"), Some("android"), None).unwrap();
        sync::register_remote_device(conn, "r2", Some("Tablet"), Some("linux"), None).unwrap();

        let status = sync::get_sync_status(conn).unwrap();
        assert_eq!(status.device_count, 2);
        assert_eq!(status.devices.len(), 2);
    }

    #[test]
    fn sync_history_empty() {
        let db = test_db();
        let history = sync::get_sync_history(db.conn(), 10).unwrap();
        assert!(history.is_empty());
    }
}
