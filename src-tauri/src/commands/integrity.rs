use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::crypto::hash::entity_id;
use crate::AppState;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct IntegritySession {
    pub id: String,
    pub enrollment_id: String,
    pub status: String,
    pub integrity_score: Option<f64>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IntegritySnapshot {
    pub id: String,
    pub session_id: String,
    pub typing_score: Option<f64>,
    pub mouse_score: Option<f64>,
    pub human_score: Option<f64>,
    pub tab_score: Option<f64>,
    pub paste_score: Option<f64>,
    pub devtools_score: Option<f64>,
    pub camera_score: Option<f64>,
    pub composite_score: Option<f64>,
    pub captured_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SubmitSnapshotRequest {
    pub session_id: String,
    pub element_id: String,
    pub integrity_score: f64,
    pub consistency_score: f64,
    pub typing_score: Option<f64>,
    pub mouse_score: Option<f64>,
    pub human_score: Option<f64>,
    pub tab_score: Option<f64>,
    pub paste_score: Option<f64>,
    pub devtools_score: Option<f64>,
    pub camera_score: Option<f64>,
    pub anomaly_flags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct EndSessionRequest {
    pub overall_integrity_score: f64,
    pub overall_consistency_score: f64,
}

#[derive(Debug, Serialize)]
pub struct StartSessionResponse {
    pub session_id: String,
}

// ============================================================================
// Commands
// ============================================================================

/// Start a new integrity monitoring session for an enrollment.
#[tauri::command]
pub async fn integrity_start_session(
    state: State<'_, AppState>,
    enrollment_id: String,
) -> Result<StartSessionResponse, String> {
    let db = state.db.lock().unwrap();

    let session_id = entity_id(&[&enrollment_id, &chrono::Utc::now().to_rfc3339()]);

    db.conn()
        .execute(
            "INSERT INTO integrity_sessions (id, enrollment_id, status)
             VALUES (?1, ?2, 'active')",
            params![session_id, enrollment_id],
        )
        .map_err(|e| e.to_string())?;

    Ok(StartSessionResponse { session_id })
}

/// Submit an integrity snapshot with signal scores.
#[tauri::command]
pub async fn integrity_submit_snapshot(
    state: State<'_, AppState>,
    req: SubmitSnapshotRequest,
) -> Result<IntegritySnapshot, String> {
    let db = state.db.lock().unwrap();

    // Verify session exists and is active
    let session_exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM integrity_sessions WHERE id = ?1 AND status = 'active'",
            params![req.session_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if !session_exists {
        return Err("session not found or not active".into());
    }

    let snapshot_id = entity_id(&[&req.session_id, &req.element_id, &chrono::Utc::now().to_rfc3339()]);

    // Composite score is the weighted integrity score from the client
    let composite = req.integrity_score;

    db.conn()
        .execute(
            "INSERT INTO integrity_snapshots (id, session_id, typing_score, mouse_score, human_score,
             tab_score, paste_score, devtools_score, camera_score, composite_score)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                snapshot_id,
                req.session_id,
                req.typing_score,
                req.mouse_score,
                req.human_score,
                req.tab_score,
                req.paste_score,
                req.devtools_score,
                req.camera_score,
                composite,
            ],
        )
        .map_err(|e| e.to_string())?;

    // Update the session's running integrity score as an average
    db.conn()
        .execute(
            "UPDATE integrity_sessions SET integrity_score = (
                SELECT AVG(composite_score) FROM integrity_snapshots WHERE session_id = ?1
             ) WHERE id = ?1",
            params![req.session_id],
        )
        .map_err(|e| e.to_string())?;

    // Read back the snapshot
    db.conn()
        .query_row(
            "SELECT id, session_id, typing_score, mouse_score, human_score,
                    tab_score, paste_score, devtools_score, camera_score,
                    composite_score, captured_at
             FROM integrity_snapshots WHERE id = ?1",
            params![snapshot_id],
            |row| {
                Ok(IntegritySnapshot {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    typing_score: row.get(2)?,
                    mouse_score: row.get(3)?,
                    human_score: row.get(4)?,
                    tab_score: row.get(5)?,
                    paste_score: row.get(6)?,
                    devtools_score: row.get(7)?,
                    camera_score: row.get(8)?,
                    composite_score: row.get(9)?,
                    captured_at: row.get(10)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}

/// End an integrity session and record final scores.
#[tauri::command]
pub async fn integrity_end_session(
    state: State<'_, AppState>,
    session_id: String,
    req: EndSessionRequest,
) -> Result<IntegritySession, String> {
    let db = state.db.lock().unwrap();

    db.conn()
        .execute(
            "UPDATE integrity_sessions SET
                status = 'completed',
                integrity_score = ?2,
                ended_at = datetime('now')
             WHERE id = ?1 AND status = 'active'",
            params![session_id, req.overall_integrity_score],
        )
        .map_err(|e| e.to_string())?;

    let rows_affected = db.conn().changes();
    if rows_affected == 0 {
        return Err("session not found or already ended".into());
    }

    db.conn()
        .query_row(
            "SELECT id, enrollment_id, status, integrity_score, started_at, ended_at
             FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |row| {
                Ok(IntegritySession {
                    id: row.get(0)?,
                    enrollment_id: row.get(1)?,
                    status: row.get(2)?,
                    integrity_score: row.get(3)?,
                    started_at: row.get(4)?,
                    ended_at: row.get(5)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}

/// Get the current integrity session for an enrollment.
#[tauri::command]
pub async fn integrity_get_session(
    state: State<'_, AppState>,
    enrollment_id: String,
) -> Result<Option<IntegritySession>, String> {
    let db = state.db.lock().unwrap();

    let result = db.conn().query_row(
        "SELECT id, enrollment_id, status, integrity_score, started_at, ended_at
         FROM integrity_sessions
         WHERE enrollment_id = ?1
         ORDER BY started_at DESC LIMIT 1",
        params![enrollment_id],
        |row| {
            Ok(IntegritySession {
                id: row.get(0)?,
                enrollment_id: row.get(1)?,
                status: row.get(2)?,
                integrity_score: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
            })
        },
    );

    match result {
        Ok(session) => Ok(Some(session)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// List all integrity sessions, optionally filtered by status.
#[tauri::command]
pub async fn integrity_list_sessions(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<IntegritySession>, String> {
    let db = state.db.lock().unwrap();

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(ref s) = status {
            (
                "SELECT id, enrollment_id, status, integrity_score, started_at, ended_at
                 FROM integrity_sessions WHERE status = ?1
                 ORDER BY started_at DESC".to_string(),
                vec![Box::new(s.clone())],
            )
        } else {
            (
                "SELECT id, enrollment_id, status, integrity_score, started_at, ended_at
                 FROM integrity_sessions
                 ORDER BY started_at DESC".to_string(),
                vec![],
            )
        };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = db.conn().prepare(&sql).map_err(|e| e.to_string())?;

    let sessions = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(IntegritySession {
                id: row.get(0)?,
                enrollment_id: row.get(1)?,
                status: row.get(2)?,
                integrity_score: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(sessions)
}

/// Get snapshots for a session.
#[tauri::command]
pub async fn integrity_list_snapshots(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<IntegritySnapshot>, String> {
    let db = state.db.lock().unwrap();

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, session_id, typing_score, mouse_score, human_score,
                    tab_score, paste_score, devtools_score, camera_score,
                    composite_score, captured_at
             FROM integrity_snapshots
             WHERE session_id = ?1
             ORDER BY captured_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let snapshots = stmt
        .query_map(params![session_id], |row| {
            Ok(IntegritySnapshot {
                id: row.get(0)?,
                session_id: row.get(1)?,
                typing_score: row.get(2)?,
                mouse_score: row.get(3)?,
                human_score: row.get(4)?,
                tab_score: row.get(5)?,
                paste_score: row.get(6)?,
                devtools_score: row.get(7)?,
                camera_score: row.get(8)?,
                composite_score: row.get(9)?,
                captured_at: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(snapshots)
}
