//! Backend IPC surface for Sentinel's hidden-overlay scan (desktop).
//!
//! One command over `sentinel::hidden_overlay`. The frontend samples it
//! once per snapshot window and raises `hidden_overlay` when the scan
//! returns any suspicious window. `None` on mobile and Wayland.

use crate::sentinel::hidden_overlay::{self, OverlayScan};

/// Windows that opted out of screen capture (or are override-redirect /
/// click-through topmost), minus our own and an allowlist of password
/// managers and OS shell owners.
#[tauri::command]
pub async fn sentinel_hidden_overlay(
    _profile: crate::profile::scope::ProfileLease,
) -> Option<OverlayScan> {
    tokio::task::spawn_blocking(hidden_overlay::scan)
        .await
        .ok()
        .flatten()
}
