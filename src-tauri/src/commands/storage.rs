//! IPC commands for storage quota management and content eviction.

use tauri::State;

use crate::ipfs::storage;
use crate::AppState;

/// Get the current storage quota in bytes (0 = unlimited).
#[tauri::command]
pub async fn storage_get_quota(state: State<'_, AppState>) -> Result<u64, String> {
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    Ok(storage::get_storage_quota(db.conn()))
}

/// Set the storage quota in bytes (0 = unlimited).
///
/// If the new quota is lower than current usage, eviction runs immediately.
#[tauri::command]
pub async fn storage_set_quota(state: State<'_, AppState>, bytes: u64) -> Result<(), String> {
    {
        let db_guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        storage::set_storage_quota(db.conn(), bytes);
    }

    // Trigger eviction if the quota was lowered
    if bytes > 0 {
        storage::maybe_evict(&state.content_node, &state.db).await;
    }

    Ok(())
}

/// Get storage usage statistics.
#[tauri::command]
pub async fn storage_stats(state: State<'_, AppState>) -> Result<storage::StorageStats, String> {
    let db_guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    Ok(storage::storage_stats(db.conn()))
}

/// Manually trigger eviction to free space.
#[tauri::command]
pub async fn storage_evict_now(
    state: State<'_, AppState>,
) -> Result<storage::EvictionResult, String> {
    Ok(storage::maybe_evict(&state.content_node, &state.db).await)
}
