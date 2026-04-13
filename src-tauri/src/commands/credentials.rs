//! IPC commands for Verifiable Credentials. Stubs — PR 5.

use tauri::State;

use crate::crypto::did::Did;
use crate::domain::vc::{VerifiableCredential, VerificationResult};
use crate::AppState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IssueCredentialRequest {
    pub credential_type: crate::domain::vc::CredentialType,
    pub subject: Did,
    pub claim: crate::domain::vc::Claim,
    pub evidence_refs: Vec<String>,
    pub expiration_date: Option<String>,
}

#[tauri::command]
pub async fn issue_credential(
    _state: State<'_, AppState>,
    _req: IssueCredentialRequest,
) -> Result<VerifiableCredential, String> {
    Err("PR 5 — issue_credential not yet implemented".into())
}

#[tauri::command]
pub async fn list_credentials(
    _state: State<'_, AppState>,
    _subject: Option<String>,
    _skill_id: Option<String>,
) -> Result<Vec<VerifiableCredential>, String> {
    Err("PR 5 — list_credentials not yet implemented".into())
}

#[tauri::command]
pub async fn get_credential(
    _state: State<'_, AppState>,
    _credential_id: String,
) -> Result<Option<VerifiableCredential>, String> {
    Err("PR 5 — get_credential not yet implemented".into())
}

#[tauri::command]
pub async fn revoke_credential(
    _state: State<'_, AppState>,
    _credential_id: String,
    _reason: String,
) -> Result<(), String> {
    Err("PR 5 — revoke_credential not yet implemented".into())
}

#[tauri::command]
pub async fn verify_credential_cmd(
    _state: State<'_, AppState>,
    _credential: VerifiableCredential,
) -> Result<VerificationResult, String> {
    Err("PR 4 — verify_credential_cmd not yet implemented".into())
}
