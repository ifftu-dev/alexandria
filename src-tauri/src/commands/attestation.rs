//! IPC commands for multi-party attestation.
//!
//! Exposes attestation requirements (governance-gated high-stakes skills)
//! and assessor attestation workflows to the frontend.

use tauri::State;

use crate::domain::attestation::{
    AttestationRequirement, AttestationStatus, EvidenceAttestation, SetRequirementParams,
    SubmitAttestationParams,
};
use crate::evidence::attestation;
use crate::AppState;

/// Check if multi-party attestation is needed for a skill at a proficiency level.
#[tauri::command]
pub async fn get_attestation_requirement(
    state: State<'_, AppState>,
    skill_id: String,
    proficiency_level: String,
) -> Result<Option<AttestationRequirement>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    attestation::check_attestation_required(db.conn(), &skill_id, &proficiency_level)
}

/// List all high-stakes skills with attestation requirements.
#[tauri::command]
pub async fn list_attestation_requirements(
    state: State<'_, AppState>,
    dao_id: Option<String>,
) -> Result<Vec<AttestationRequirement>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    attestation::list_attestation_requirements(db.conn(), dao_id.as_deref())
}

/// Set an attestation requirement for a skill (governance action).
#[tauri::command]
pub async fn set_attestation_requirement(
    state: State<'_, AppState>,
    params: SetRequirementParams,
) -> Result<AttestationRequirement, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    attestation::set_attestation_requirement(db.conn(), &params)
}

/// Remove an attestation requirement for a skill.
#[tauri::command]
pub async fn remove_attestation_requirement(
    state: State<'_, AppState>,
    skill_id: String,
    proficiency_level: String,
) -> Result<bool, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    attestation::remove_attestation_requirement(db.conn(), &skill_id, &proficiency_level)
}

/// Submit an attestation (assessor co-signs evidence).
///
/// Uses the local identity's stake address as the attestor, and signs
/// with the local keystore.
#[tauri::command]
pub async fn submit_attestation(
    state: State<'_, AppState>,
    params: SubmitAttestationParams,
) -> Result<EvidenceAttestation, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    // Get local identity as the attestor
    let attestor_address: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no local identity: {e}"))?;

    // TODO: Sign the evidence with the keystore — for now, generate
    // a deterministic placeholder signature from the evidence ID.
    let signature = format!("attestation_sig_{}", params.evidence_id);

    attestation::submit_attestation(conn, &attestor_address, &signature, &params)
}

/// Get all attestations on an evidence record.
#[tauri::command]
pub async fn list_attestations_for_evidence(
    state: State<'_, AppState>,
    evidence_id: String,
) -> Result<Vec<EvidenceAttestation>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    attestation::list_attestations_for_evidence(db.conn(), &evidence_id)
}

/// Get full attestation status for an evidence record.
#[tauri::command]
pub async fn get_attestation_status(
    state: State<'_, AppState>,
    evidence_id: String,
) -> Result<AttestationStatus, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    attestation::get_attestation_status(db.conn(), &evidence_id)
}

/// Find evidence records that need attestation but don't have enough.
#[tauri::command]
pub async fn list_unattested_evidence(
    state: State<'_, AppState>,
) -> Result<Vec<AttestationStatus>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    attestation::list_unattested_evidence(db.conn())
}
