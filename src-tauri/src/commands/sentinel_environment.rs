//! Backend IPC surface for Sentinel's Android environment layer.
//!
//! Three thin commands over `sentinel::android_environment`. The frontend
//! engages the assessment shield while an assessment element is current,
//! samples the environment report once per snapshot window, and drains
//! the obscured-touch counter with it. All three are no-ops off Android:
//! `None`, `Ok(())`, and `0` respectively, so callers never branch on
//! platform.

use crate::sentinel::android_environment::{self, AndroidEnvironment};

/// Enabled accessibility services, debug-bridge state, shield state, and
/// the running obscured-touch count, or `None` off Android / on failure.
#[tauri::command]
pub async fn sentinel_android_environment(
    _profile: crate::profile::scope::ProfileLease,
) -> Option<AndroidEnvironment> {
    tokio::task::spawn_blocking(android_environment::report)
        .await
        .ok()
        .flatten()
}

/// Engage or release the assessment shield (`FLAG_SECURE` plus
/// `setHideOverlayWindows` on API 31+). Best effort: a failure is logged
/// and reported, never fatal to the session.
#[tauri::command]
pub async fn sentinel_set_assessment_shield(
    _profile: crate::profile::scope::ProfileLease,
    on: bool,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || android_environment::set_shield(on))
        .await
        .map_err(|e| format!("shield worker failed: {e}"))?
        .inspect_err(|e| log::warn!(target: "sentinel", "assessment shield {on}: {e}"))
}

/// Return-and-reset the count of touches delivered while another window
/// was drawn over ours. Always `0` off Android.
#[tauri::command]
pub async fn sentinel_take_obscured_touches(_profile: crate::profile::scope::ProfileLease) -> u32 {
    tokio::task::spawn_blocking(android_environment::take_obscured_touches)
        .await
        .unwrap_or(0)
}
