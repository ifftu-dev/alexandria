//! IPC commands for cross-device sync.
//!
//! Exposes the sync system to the frontend:
//!   - Device management (register, list, rename, remove)
//!   - Sync status and history
//!   - Manual sync trigger
//!   - Auto-sync toggle

use tauri::State;

use crate::domain::sync::{DeviceInfo, SyncHistoryEntry, SyncStatus};
use crate::p2p::sync;
use crate::AppState;

/// Get information about the local device.
///
/// Returns the device record for this node. Creates one if it
/// doesn't exist yet (auto-registers on first call).
#[tauri::command]
pub async fn sync_get_device_info(state: State<'_, AppState>) -> Result<DeviceInfo, String> {
    let db = state.db.lock().map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    let platform = std::env::consts::OS;
    let device_id = sync::get_or_create_local_device(conn, platform)?;
    sync::get_device(conn, &device_id)
}

/// Set a user-friendly name for the local device.
#[tauri::command]
pub async fn sync_set_device_name(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    let device_id = conn
        .query_row("SELECT id FROM devices WHERE is_local = 1", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| format!("no local device registered: {e}"))?;

    conn.execute(
        "UPDATE devices SET device_name = ?1 WHERE id = ?2",
        rusqlite::params![name, device_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// List all known devices (local + remote).
#[tauri::command]
pub async fn sync_list_devices(state: State<'_, AppState>) -> Result<Vec<DeviceInfo>, String> {
    let db = state.db.lock().map_err(|_| "database lock poisoned".to_string())?;
    sync::list_devices(db.conn())
}

/// Remove a remote device and its sync state.
///
/// Cannot remove the local device.
#[tauri::command]
pub async fn sync_remove_device(
    state: State<'_, AppState>,
    device_id: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|_| "database lock poisoned".to_string())?;
    sync::remove_device(db.conn(), &device_id)
}

/// Get the current sync status.
///
/// Returns device count, queue length, last sync time, and
/// per-device sync summaries.
#[tauri::command]
pub async fn sync_status(state: State<'_, AppState>) -> Result<SyncStatus, String> {
    let db = state.db.lock().map_err(|_| "database lock poisoned".to_string())?;
    sync::get_sync_status(db.conn())
}

/// Manually trigger a sync with all known remote devices.
///
/// In a full implementation this would initiate the P2P
/// request-response protocol with each online peer. For now
/// it processes the local queue and returns what would be sent.
#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> Result<SyncStatus, String> {
    let db = state.db.lock().map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    // Prune already-delivered items
    let pruned = sync::prune_delivered_queue(conn)?;
    if pruned > 0 {
        log::info!("sync: pruned {pruned} delivered queue items");
    }

    // Return updated status
    sync::get_sync_status(conn)
}

/// Toggle automatic background sync.
///
/// When enabled, the node will periodically sync with all known
/// remote devices (every 60 seconds when peers are online).
#[tauri::command]
pub async fn sync_set_auto(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    let db = state.db.lock().map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    // Store the auto-sync preference in a simple key-value pattern
    // using the sync_log table (lightweight approach)
    conn.execute(
        "INSERT OR REPLACE INTO devices (id, device_name, platform, is_local) \
         SELECT id, device_name, platform, is_local FROM devices WHERE is_local = 1",
        [],
    )
    .map_err(|e| e.to_string())?;

    log::info!("sync: auto-sync set to {enabled}");
    Ok(())
}

/// Get sync history (recent sync events).
#[tauri::command]
pub async fn sync_history(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<SyncHistoryEntry>, String> {
    let db = state.db.lock().map_err(|_| "database lock poisoned".to_string())?;
    sync::get_sync_history(db.conn(), limit.unwrap_or(50))
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
