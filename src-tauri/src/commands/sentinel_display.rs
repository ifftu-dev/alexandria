//! Backend IPC surface for Sentinel display-topology detection.
//!
//! The frontend samples this once at session start and once per snapshot
//! window, then derives the `external_display` / `display_change` /
//! `split_screen` / `screen_captured` flags in `useSentinel.computeScores`.
//! Nothing is persisted here; the reading only crosses IPC.

use crate::sentinel::display_topology::{self, DisplayTopology};

/// Current display arrangement as the OS reports it, or `None` when the
/// platform has no probe (or the probe failed). Never an error: a
/// missing capability must not break monitoring.
#[tauri::command]
pub async fn sentinel_display_topology(
    app: tauri::AppHandle,
    _profile: crate::profile::scope::ProfileLease,
) -> Option<DisplayTopology> {
    // Tauri's monitor enumeration round-trips through the event loop and
    // must not run on the main thread; UIKit (iOS) must. The module
    // handles the iOS hop itself, so a blocking worker is right for both.
    tokio::task::spawn_blocking(move || display_topology::current(&app))
        .await
        .ok()
        .flatten()
}
