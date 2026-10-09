//! Backend IPC surface for Sentinel's global modifier-combo monitor
//! (desktop, phantom-hotkey detection).
//!
//! Privacy: the monitor records only key-downs with Cmd/Win/Super, Ctrl or
//! Alt held, as a normalised combo string plus a monotonic timestamp. No
//! plain characters, no Shift-only combos, no key-ups. See
//! `sentinel::global_hotkeys`. The frontend starts it only for
//! assessment-purpose sessions and only when the OS already granted the
//! listening permission; the permission prompt is a wizard step, never a
//! mid-assessment surprise.

use serde::Serialize;

use crate::sentinel::global_hotkeys::{self, HotkeyEvent, HotkeyStatus, OS_COMBOS};

/// Monitor status plus the OS-owned combo vocabulary the frontend needs
/// to subtract system shortcuts (one source of truth for the list).
#[derive(Debug, Clone, Serialize)]
pub struct HotkeyStatusResponse {
    #[serde(flatten)]
    pub status: HotkeyStatus,
    /// Combos the OS itself consumes; never phantom.
    pub os_combos: Vec<String>,
    /// On Windows / Linux every `cmd+…` (Win / Super) is system-owned.
    pub cmd_is_system: bool,
}

fn respond(status: HotkeyStatus) -> HotkeyStatusResponse {
    HotkeyStatusResponse {
        status,
        os_combos: OS_COMBOS.iter().map(|s| (*s).to_owned()).collect(),
        cmd_is_system: cfg!(any(target_os = "windows", target_os = "linux")),
    }
}

/// Capability and permission state; never prompts.
#[tauri::command]
pub async fn sentinel_hotkeys_status(
    _profile: crate::profile::scope::ProfileLease,
) -> HotkeyStatusResponse {
    respond(global_hotkeys::status())
}

/// Ask the OS for listening permission (macOS shows the Accessibility /
/// Input Monitoring prompt). Returns the status afterwards.
#[tauri::command]
pub async fn sentinel_hotkeys_request_permission(
    _profile: crate::profile::scope::ProfileLease,
) -> HotkeyStatusResponse {
    respond(global_hotkeys::request_permission())
}

/// Start the listen-only monitor. Idempotent. `Err` when unsupported or
/// not permitted; the frontend treats that as "signal absent".
#[tauri::command]
pub async fn sentinel_hotkeys_start(
    _profile: crate::profile::scope::ProfileLease,
) -> Result<HotkeyStatusResponse, String> {
    tokio::task::spawn_blocking(global_hotkeys::start)
        .await
        .map_err(|e| format!("hotkey monitor worker failed: {e}"))?
        .map(respond)
}

/// Stop the monitor and discard anything buffered. Idempotent.
#[tauri::command]
pub async fn sentinel_hotkeys_stop(_profile: crate::profile::scope::ProfileLease) {
    let _ = tokio::task::spawn_blocking(|| {
        global_hotkeys::stop();
        global_hotkeys::drain();
    })
    .await;
}

/// Return-and-clear the combos recorded since the previous drain.
#[tauri::command]
pub async fn sentinel_hotkeys_drain(
    _profile: crate::profile::scope::ProfileLease,
) -> Vec<HotkeyEvent> {
    global_hotkeys::drain()
}
