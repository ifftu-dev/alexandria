//! IPC commands for PinBoard management + quota introspection. Stubs — PR 10.

use tauri::State;

use crate::p2p::pinboard::PinboardCommitment;
use crate::AppState;

#[tauri::command]
pub async fn declare_pinboard_commitment(
    _state: State<'_, AppState>,
    _subject_did: String,
    _scope: Vec<String>,
) -> Result<PinboardCommitment, String> {
    Err("PR 10 — declare_pinboard_commitment not yet implemented".into())
}

#[tauri::command]
pub async fn revoke_pinboard_commitment(
    _state: State<'_, AppState>,
    _commitment_id: String,
) -> Result<(), String> {
    Err("PR 10 — revoke_pinboard_commitment not yet implemented".into())
}

#[tauri::command]
pub async fn list_my_commitments(
    _state: State<'_, AppState>,
) -> Result<Vec<PinboardCommitment>, String> {
    Err("PR 10 — list_my_commitments not yet implemented".into())
}

#[tauri::command]
pub async fn list_incoming_commitments(
    _state: State<'_, AppState>,
) -> Result<Vec<PinboardCommitment>, String> {
    Err("PR 10 — list_incoming_commitments not yet implemented".into())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QuotaBreakdown {
    pub subject_authored_bytes: u64,
    pub pinboard_bytes: u64,
    pub cache_bytes: u64,
    pub enrollment_bytes: u64,
    pub total_quota_bytes: u64,
}

#[tauri::command]
pub async fn get_quota_breakdown(_state: State<'_, AppState>) -> Result<QuotaBreakdown, String> {
    Err("PR 10 — get_quota_breakdown not yet implemented".into())
}
