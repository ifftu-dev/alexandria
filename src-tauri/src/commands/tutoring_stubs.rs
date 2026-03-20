//! Mobile stub commands for live tutoring.
//!
//! Phase 2 (in progress): audio-only tutoring on mobile.
//! iroh-live is now available without ffmpeg via the pure Opus codec,
//! and gossip + MoQ protocols are registered on mobile.
//! These stubs will be replaced with real audio-only implementations.
//!
//! Currently stubbed: create/join/leave room, toggle video/audio,
//! screen share, chat, status, peers. Device enumeration returns
//! audio-only capabilities.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::AppState;

/// Result of a pre-join device availability check.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceCheckResult {
    pub has_camera: bool,
    pub camera_name: Option<String>,
    pub has_audio: bool,
    pub error: Option<String>,
}

/// Summary of a tutoring session from the database.
#[derive(Debug, Clone, Serialize)]
pub struct TutoringSessionInfo {
    pub id: String,
    pub title: String,
    pub ticket: Option<String>,
    pub status: String,
    pub created_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CameraDeviceInfo {
    pub index: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceList {
    pub audio_inputs: Vec<AudioDeviceInfo>,
    pub audio_outputs: Vec<AudioDeviceInfo>,
    pub cameras: Vec<CameraDeviceInfo>,
}

const UNSUPPORTED: &str = "Audio-only tutoring on mobile is coming soon";

#[tauri::command]
pub async fn tutoring_create_room(
    _title: String,
    _display_name: Option<String>,
    _camera_id: Option<String>,
    _mic_id: Option<String>,
    _speaker_id: Option<String>,
    _app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<TutoringSessionInfo, String> {
    Err(UNSUPPORTED.into())
}

#[tauri::command]
pub async fn tutoring_join_room(
    _ticket: String,
    _title: Option<String>,
    _display_name: Option<String>,
    _camera_id: Option<String>,
    _mic_id: Option<String>,
    _speaker_id: Option<String>,
    _app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<TutoringSessionInfo, String> {
    Err(UNSUPPORTED.into())
}

#[tauri::command]
pub async fn tutoring_leave_room(_state: State<'_, AppState>) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

#[tauri::command]
pub async fn tutoring_toggle_video(
    _enable: bool,
    _state: State<'_, AppState>,
) -> Result<bool, String> {
    Err(UNSUPPORTED.into())
}

#[tauri::command]
pub async fn tutoring_toggle_audio(
    _enable: bool,
    _state: State<'_, AppState>,
) -> Result<bool, String> {
    Err(UNSUPPORTED.into())
}

#[tauri::command]
pub async fn tutoring_toggle_screen_share(
    _enable: bool,
    _state: State<'_, AppState>,
) -> Result<bool, String> {
    Err(UNSUPPORTED.into())
}

#[tauri::command]
pub async fn tutoring_send_chat(
    _text: String,
    _state: State<'_, AppState>,
) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

#[tauri::command]
pub async fn tutoring_status(
    _state: State<'_, AppState>,
) -> Result<Option<serde_json::Value>, String> {
    Ok(None)
}

#[tauri::command]
pub async fn tutoring_peers(
    _state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn tutoring_list_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<TutoringSessionInfo>, String> {
    // Still read from DB on mobile (past sessions from desktop sync)
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, title, ticket, status, created_at, ended_at
             FROM tutoring_sessions
             ORDER BY created_at DESC
             LIMIT 50",
        )
        .map_err(|e| e.to_string())?;

    let sessions = stmt
        .query_map([], |row| {
            Ok(TutoringSessionInfo {
                id: row.get(0)?,
                title: row.get(1)?,
                ticket: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                ended_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(sessions)
}

#[tauri::command]
pub async fn tutoring_check_devices() -> Result<DeviceCheckResult, String> {
    Ok(DeviceCheckResult {
        has_camera: false,
        camera_name: None,
        has_audio: false,
        error: Some(UNSUPPORTED.into()),
    })
}

#[tauri::command]
pub async fn tutoring_list_devices() -> Result<DeviceList, String> {
    Ok(DeviceList {
        audio_inputs: vec![],
        audio_outputs: vec![],
        cameras: vec![],
    })
}

#[tauri::command]
pub async fn tutoring_get_audio_level(
    _state: State<'_, AppState>,
) -> Result<f32, String> {
    Ok(0.0)
}

#[tauri::command]
pub async fn tutoring_diagnostics(
    _state: State<'_, AppState>,
) -> Result<Option<serde_json::Value>, String> {
    Ok(None)
}
