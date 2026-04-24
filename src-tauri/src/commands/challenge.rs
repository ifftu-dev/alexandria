//! IPC for the credential-challenge mechanism (VC-first rebuild).

use tauri::State;

use crate::domain::challenge::{
    ChallengeResolution, ChallengeVote, CredentialChallenge, SubmitCredentialChallengeParams,
};
use crate::evidence::challenge as challenge_logic;
use crate::AppState;

#[tauri::command]
pub async fn submit_credential_challenge(
    state: State<'_, AppState>,
    params: SubmitCredentialChallengeParams,
) -> Result<CredentialChallenge, String> {
    // Look up the challenger (local node's stake address) and a
    // placeholder signature — real signing is done at the gossip
    // envelope layer.
    let challenger: String = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT stake_address FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("local identity not initialized: {e}"))?
    };

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::submit_challenge(db.conn(), &params, &challenger, "ipc-placeholder-sig")
}

#[tauri::command]
pub async fn vote_on_credential_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
    upheld: bool,
    reason: Option<String>,
) -> Result<ChallengeVote, String> {
    let voter: String = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT stake_address FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("local identity not initialized: {e}"))?
    };

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::vote(db.conn(), &challenge_id, &voter, upheld, reason.as_deref())
}

#[tauri::command]
pub async fn resolve_credential_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
) -> Result<ChallengeResolution, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::resolve(db.conn(), &challenge_id)
}

#[tauri::command]
pub async fn list_credential_challenges(
    state: State<'_, AppState>,
    status: Option<String>,
    credential_id: Option<String>,
) -> Result<Vec<CredentialChallenge>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::list_challenges(db.conn(), status.as_deref(), credential_id.as_deref())
}

#[tauri::command]
pub async fn get_credential_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
) -> Result<Option<CredentialChallenge>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::get_challenge(db.conn(), &challenge_id)
}

#[tauri::command]
pub async fn expire_overdue_credential_challenges(
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    challenge_logic::expire_overdue(db.conn())
}
