//! IPC commands for the evidence challenge mechanism.
//!
//! Exposes 7 commands to the frontend:
//!   - `submit_evidence_challenge` — submit a new challenge
//!   - `list_challenges` — list with filters
//!   - `get_challenge` — get challenge with votes
//!   - `vote_on_challenge` — committee member votes
//!   - `resolve_challenge` — resolve after voting
//!   - `list_my_challenges` — challenges submitted by local identity
//!   - `list_challenges_against_me` — challenges against local identity

use tauri::State;

use crate::domain::challenge::{
    ChallengeResolution, ChallengeVote, EvidenceChallenge, SubmitChallengeParams,
};
use crate::evidence::challenge as challenge_logic;
use crate::AppState;

/// Submit a new evidence challenge.
#[tauri::command]
pub async fn submit_evidence_challenge(
    state: State<'_, AppState>,
    params: SubmitChallengeParams,
) -> Result<EvidenceChallenge, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    challenge_logic::submit_challenge(conn, &params)
}

/// List challenges with optional filters.
#[tauri::command]
pub async fn list_challenges(
    state: State<'_, AppState>,
    status: Option<String>,
    dao_id: Option<String>,
    learner_address: Option<String>,
    challenger: Option<String>,
) -> Result<Vec<EvidenceChallenge>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    challenge_logic::list_challenges(
        conn,
        status.as_deref(),
        dao_id.as_deref(),
        learner_address.as_deref(),
        challenger.as_deref(),
    )
}

/// Get a challenge by ID with its votes.
#[tauri::command]
pub async fn get_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
) -> Result<(EvidenceChallenge, Vec<ChallengeVote>), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    challenge_logic::get_challenge(conn, &challenge_id)
}

/// Vote on a challenge as a DAO committee member.
#[tauri::command]
pub async fn vote_on_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
    voter: String,
    upheld: bool,
    reason: Option<String>,
) -> Result<ChallengeVote, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    challenge_logic::vote_on_challenge(conn, &challenge_id, &voter, upheld, reason.as_deref())
}

/// Resolve a challenge after voting.
#[tauri::command]
pub async fn resolve_challenge(
    state: State<'_, AppState>,
    challenge_id: String,
) -> Result<ChallengeResolution, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    challenge_logic::resolve_challenge(conn, &challenge_id)
}

/// List challenges submitted by the local identity.
#[tauri::command]
pub async fn list_my_challenges(
    state: State<'_, AppState>,
) -> Result<Vec<EvidenceChallenge>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let stake_address: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no local identity: {e}"))?;

    challenge_logic::list_challenges(conn, None, None, None, Some(&stake_address))
}

/// List challenges against the local identity's evidence.
#[tauri::command]
pub async fn list_challenges_against_me(
    state: State<'_, AppState>,
) -> Result<Vec<EvidenceChallenge>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let stake_address: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no local identity: {e}"))?;

    challenge_logic::list_challenges(conn, None, None, Some(&stake_address), None)
}
