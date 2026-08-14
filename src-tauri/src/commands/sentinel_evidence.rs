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
///
/// Deletion reaches the copies too. If this evidence was released to a service
/// in order to contest a flag, that service is asked to destroy its copy as
/// part of the same action — because a person who deletes something and is told
/// it is gone has been told something untrue otherwise. The request is recorded
/// before it is attempted and retried on every later unlock, so being offline at
/// the moment of the decision delays the withdrawal rather than losing it.
#[tauri::command]
pub async fn sentinel_evidence_delete(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<usize, String> {
    // Marked first and inside the same lock as the local delete, so there is no
    // window where the local copy is gone and nothing records that the remote
    // ones were meant to follow it.
    let deleted = {
        let guard = state.db.lock().map_err(|e| e.to_string())?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        let sent_to = crate::commands::holder_release::releases_for_session(db.conn(), &session_id)
            .map_err(|e| e.to_string())?;
        for (directory_url, run_id) in &sent_to {
            crate::commands::holder_release::want_withdrawal(
                db.conn(),
                &session_id,
                directory_url,
                run_id,
            )
            .map_err(|e| e.to_string())?;
        }
        evidence::delete_for_session(db.conn(), &session_id).map_err(|e| e.to_string())?
    };

    // Best effort, on purpose. A withdrawal that could not be sent is still
    // owed and still queued; reporting the local deletion as a failure because
    // a server was unreachable would leave a person believing their own device
    // still held it.
    if let Err(e) = crate::commands::holder_release::holder_retry_withdrawals(state).await {
        tracing::warn!(
            error = %e,
            "evidence deleted locally; withdrawing the released copies is still owed"
        );
    }

    Ok(deleted)
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
