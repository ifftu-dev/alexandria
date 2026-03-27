use serde::Serialize;
use tauri::State;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub database: String,
}

/// Check the health of the Alexandria node.
#[tauri::command]
pub async fn check_health(state: State<'_, AppState>) -> Result<HealthResponse, String> {
    let db_status = match state.db.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(db) => match db.conn().query_row("SELECT 1", [], |_| Ok(())) {
                Ok(()) => "ok".to_string(),
                Err(e) => format!("error: {}", e),
            },
            None => "not initialized".to_string(),
        },
        Err(e) => format!("mutex poisoned: {}", e),
    };

    Ok(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        database: db_status,
    })
}

/// Read the diagnostic log (for debugging P2P / iOS issues).
#[tauri::command]
pub async fn read_diag_log() -> Result<String, String> {
    Ok(crate::diag::read())
}
