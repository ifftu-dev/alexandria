//! IPC commands for the community plugin system (Phase 1).
//!
//! Install, uninstall, list, inspect, and per-capability consent flows.
//! Phase 3 will add discovery/gossip commands alongside these.

use std::path::PathBuf;

use tauri::State;

use crate::domain::plugin::{
    InstalledPlugin, PluginCapability, PluginManifest, PluginPermissionRecord,
};
use crate::plugins::registry;
use crate::AppState;

/// Install a plugin from a directory on the user's local filesystem.
/// The directory must contain `manifest.json`, `manifest.sig`, and the
/// `entry` HTML file the manifest references. Phase 3 will add P2P
/// install from a gossip-announced CID.
#[tauri::command]
pub async fn plugin_install_from_file(
    state: State<'_, AppState>,
    directory: String,
) -> Result<InstalledPlugin, String> {
    check_rate_limit(&state, "plugin_install_from_file")?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let src = PathBuf::from(&directory);
    registry::install_from_directory(db, &state.plugins_dir, &src)
}

/// Uninstall a plugin by CID. Removes the bundle directory and cascades
/// deletion of `plugin_permissions`. Does *not* affect any course elements
/// that reference the plugin — they will simply fail to mount with a
/// "plugin not installed" message until the user re-installs it.
#[tauri::command]
pub async fn plugin_uninstall(
    state: State<'_, AppState>,
    plugin_cid: String,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::uninstall(db, &state.plugins_dir, &plugin_cid)
}

/// List every plugin installed on this node, newest first.
#[tauri::command]
pub async fn plugin_list(state: State<'_, AppState>) -> Result<Vec<InstalledPlugin>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::list_installed(db)
}

/// Return the parsed manifest for an installed plugin.
#[tauri::command]
pub async fn plugin_get_manifest(
    state: State<'_, AppState>,
    plugin_cid: String,
) -> Result<PluginManifest, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::get_manifest(db, &plugin_cid)
}

/// Grant a capability to a plugin. Scope is `"once"`, `"session"`, or
/// `"always"`. Calling twice for the same `(plugin_cid, capability)` pair
/// overwrites the previous grant. The frontend is expected to call this
/// *after* presenting a consent prompt to the user.
#[tauri::command]
pub async fn plugin_grant_capability(
    state: State<'_, AppState>,
    plugin_cid: String,
    capability: String,
    scope: String,
) -> Result<(), String> {
    let cap = PluginCapability::parse(&capability)
        .ok_or_else(|| format!("unknown capability '{capability}'"))?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::grant_capability(db, &plugin_cid, cap, &scope, None)
}

/// Revoke a previously-granted capability. Safe to call when no grant
/// exists (no-op).
#[tauri::command]
pub async fn plugin_revoke_capability(
    state: State<'_, AppState>,
    plugin_cid: String,
    capability: String,
) -> Result<(), String> {
    let cap = PluginCapability::parse(&capability)
        .ok_or_else(|| format!("unknown capability '{capability}'"))?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::revoke_capability(db, &plugin_cid, cap)
}

/// List all current capability grants for a plugin. Used by the Installed
/// Plugins page to show revoke buttons.
#[tauri::command]
pub async fn plugin_list_permissions(
    state: State<'_, AppState>,
    plugin_cid: String,
) -> Result<Vec<PluginPermissionRecord>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::list_permissions(db, &plugin_cid)
}

fn check_rate_limit(state: &State<'_, AppState>, command: &str) -> Result<(), String> {
    let mut limiter = state
        .ipc_limiter
        .lock()
        .map_err(|_| "rate limiter poisoned".to_string())?;
    limiter.check(command)
}
