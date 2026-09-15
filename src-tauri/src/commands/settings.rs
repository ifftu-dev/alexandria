//! IPC commands for the unified per-profile settings store.
//!
//! The frontend mirrors the registry via [`list_settings`] at
//! mount and listens for the `settings-changed` event on every
//! write so multiple windows stay in sync.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::settings::{registry::SettingEntry, store::SettingsError, SettingsStore};
use crate::AppState;

#[derive(Clone, Serialize)]
struct SettingsChangedEvent<'a> {
    /// Single key that changed, or `null` if a full refresh is needed.
    key: Option<&'a str>,
}

fn emit_changed(app: &AppHandle, key: Option<&str>) {
    let _ = app.emit("settings-changed", SettingsChangedEvent { key });
}

fn map_settings_error(e: SettingsError) -> String {
    e.to_string()
}

/// Return every registered setting with its current value, default
/// value, and metadata. Refused while no profile is unlocked.
#[tauri::command]
pub async fn list_settings(state: State<'_, AppState>) -> Result<Vec<SettingEntry>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "settings.list",
            |db| SettingsStore::list_all(db.conn()).map_err(map_settings_error),
        )
        .await
}

/// Persist a single setting. The key MUST be one declared in
/// `settings::registry::keys`; unknown keys are rejected. Emits
/// `settings-changed` so every window updates.
#[tauri::command]
pub async fn set_setting(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    let changed_key = key.clone();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "settings.set",
            move |db| SettingsStore::set_raw(db.conn(), &key, &value).map_err(map_settings_error),
        )
        .await?;
    emit_changed(&app, Some(&changed_key));
    Ok(())
}

/// Delete the override for a key, restoring the registry default.
#[tauri::command]
pub async fn reset_setting(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<(), String> {
    let changed_key = key.clone();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "settings.reset",
            move |db| SettingsStore::reset(db.conn(), &key).map_err(map_settings_error),
        )
        .await?;
    emit_changed(&app, Some(&changed_key));
    Ok(())
}
