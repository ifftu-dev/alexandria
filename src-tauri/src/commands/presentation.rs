//! IPC commands for selective disclosure + presentations. Stubs — PR 11.

use tauri::State;

use crate::AppState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreatePresentationRequest {
    pub credential_ids: Vec<String>,
    /// JSONPath-style field subsets to reveal (e.g. ["credentialSubject.claim.level"]).
    pub reveal: Vec<String>,
    pub audience: String,
    pub nonce: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PresentationEnvelope {
    pub id: String,
    pub payload_json: String,
    pub proof: String,
}

#[tauri::command]
pub async fn create_presentation(
    _state: State<'_, AppState>,
    _req: CreatePresentationRequest,
) -> Result<PresentationEnvelope, String> {
    Err("PR 11 — create_presentation not yet implemented".into())
}
