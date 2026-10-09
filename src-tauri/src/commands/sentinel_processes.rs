//! Backend IPC surface for Sentinel's process watchlist scan (desktop).
//!
//! The full process list never leaves the backend: only entries matching
//! the watchlist (cheat tools, remote-desktop hosts, virtual cameras, VM
//! guest agents, AI assistants, screen-share apps) cross IPC, with the
//! total scanned for context. `None` on mobile.

use crate::sentinel::processes::{self, ProcessScan};

/// Watched processes currently running, or `None` when the platform has
/// no probe. Sampled by the frontend once per snapshot window.
#[tauri::command]
pub async fn sentinel_process_scan(
    _profile: crate::profile::scope::ProfileLease,
) -> Option<ProcessScan> {
    tokio::task::spawn_blocking(processes::scan)
        .await
        .ok()
        .flatten()
}
