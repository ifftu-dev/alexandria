//! IPC for the appeal-evidence consent flow.
//!
//! The order these commands are meant to be called in is the privacy design,
//! so it is worth stating: the frontend shows a flagged session, calls
//! [`sentinel_evidence_pending`] to tell the learner exactly what could be kept,
//! and calls [`sentinel_evidence_decide`] with their answer. Only that last call
//! writes anything, and only when the answer is yes.
//!
//! There is deliberately no command that persists evidence without a decision,
//! and none that re-opens a decision once made. See `sentinel::evidence`.

use tauri::State;

use crate::sentinel::evidence::{self, EvidencePreview, EvidenceSummary};
use crate::AppState;

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// What is staged in memory for this session, and would be written if the
/// learner consents. Reads nothing from disk.
///
/// This is what the consent prompt must show. "Keep the evidence?" without
/// saying that the evidence includes camera frames is not informed consent.
#[tauri::command]
pub async fn sentinel_evidence_pending(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<EvidenceSummary, String> {
    Ok(state.evidence_staging.summary(&session_id))
}

/// Record the learner's answer.
///
/// `granted = true` writes the staged evidence with an expiry; `false` writes a
/// consent row saying no and discards the staged bytes. Either way the decision
/// is final — asking twice is how a refusal gets worn down.
#[tauri::command]
pub async fn sentinel_evidence_decide(
    state: State<'_, AppState>,
    session_id: String,
    granted: bool,
) -> Result<usize, String> {
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    evidence::decide(
        db.conn(),
        &state.evidence_staging,
        &session_id,
        granted,
        &now_iso(),
    )
    .map_err(|e| e.to_string())
}

/// What is currently retained on disk for this session, so a learner can see
/// what they are holding and decide whether to keep holding it.
#[tauri::command]
pub async fn sentinel_evidence_stored(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<EvidenceSummary, String> {
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    evidence::stored_summary(db.conn(), &session_id).map_err(|e| e.to_string())
}

/// Delete this session's retained evidence now, ahead of expiry.
///
/// Always available. Deleting evidence must never cost a learner their appeal —
/// an adjudication rests on the scores, and absent evidence may not be held
/// against them (see `docs/sentinel.md`).
#[tauri::command]
pub async fn sentinel_evidence_delete(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<usize, String> {
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    evidence::delete_for_session(db.conn(), &session_id).map_err(|e| e.to_string())
}

/// The staged evidence itself, rendered for display.
///
/// The consent prompt shows this before asking. A learner deciding whether to
/// keep camera frames is entitled to look at them first — not least because the
/// frame is often what shows the flag was wrong.
#[tauri::command]
pub async fn sentinel_evidence_preview(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<EvidencePreview>, String> {
    Ok(evidence::preview_staged(
        &state.evidence_staging,
        &session_id,
    ))
}

/// Retained evidence, rendered for review after consent was given.
#[tauri::command]
pub async fn sentinel_evidence_stored_preview(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<EvidencePreview>, String> {
    let guard = state.db.lock().map_err(|e| e.to_string())?;
    let db = guard.as_ref().ok_or("database not initialized")?;
    evidence::preview_stored(db.conn(), &session_id).map_err(|e| e.to_string())
}
