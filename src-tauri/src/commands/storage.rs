//! IPC commands for storage quota management and content eviction.

use crate::profile::scope::ProfileState as State;

use crate::content_store::storage;
use crate::db::executor::DatabaseWorkload;
use crate::AppState;

/// Get the current storage quota in bytes (0 = unlimited).
#[tauri::command]
pub async fn storage_get_quota(state: State<'_, AppState>) -> Result<u64, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "storage.get-quota",
            |db| Ok(storage::get_storage_quota(db.conn())),
        )
        .await
}

/// Set the storage quota in bytes (0 = unlimited).
///
/// If the new quota is lower than current usage, eviction runs immediately.
#[tauri::command]
pub async fn storage_set_quota(state: State<'_, AppState>, bytes: u64) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "storage.set-quota",
            move |db| {
                storage::set_storage_quota(db.conn(), bytes).map_err(|error| error.to_string())
            },
        )
        .await?;

    // Trigger eviction if the quota was lowered
    if bytes > 0 {
        storage::maybe_evict(&state.content_node, &state.db).await;
    }

    Ok(())
}

/// Get storage usage statistics.
#[tauri::command]
pub async fn storage_stats(state: State<'_, AppState>) -> Result<storage::StorageStats, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "storage.stats",
            |db| Ok(storage::storage_stats(db.conn())),
        )
        .await
}

/// Manually trigger eviction to free space.
#[tauri::command]
pub async fn storage_evict_now(
    state: State<'_, AppState>,
) -> Result<storage::EvictionResult, String> {
    Ok(storage::maybe_evict(&state.content_node, &state.db).await)
}
