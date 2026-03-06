//! Mobile Tauri commands for live tutoring sessions (audio-only).
//!
//! Phase 2: real audio-only tutoring on mobile via the mobile
//! TutoringManager. Uses PureOpusEncoder/Decoder (no ffmpeg).
//!
//! Video and screen share commands return errors explaining they
//! are not available on mobile.

use iroh_live::media::audio::AudioBackend;
use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::AppState;
use crate::tutoring::manager_mobile::DeviceSelection;

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

/// Info about an available audio device.
#[derive(Debug, Clone, Serialize)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: Option<String>,
    pub is_default: bool,
}

/// Info about an available camera (empty on mobile).
#[derive(Debug, Clone, Serialize)]
pub struct CameraDeviceInfo {
    pub index: String,
    pub name: String,
}

/// Available devices for audio and video.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceList {
    pub audio_inputs: Vec<AudioDeviceInfo>,
    pub audio_outputs: Vec<AudioDeviceInfo>,
    pub cameras: Vec<CameraDeviceInfo>,
}

/// Create a new tutoring room (host, audio-only).
#[tauri::command]
pub async fn tutoring_create_room(
    title: String,
    display_name: Option<String>,
    _camera_id: Option<String>,
    mic_id: Option<String>,
    speaker_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TutoringSessionInfo, String> {
    let content_node = &state.content_node;

    let endpoint = content_node
        .endpoint()
        .await
        .ok_or("iroh node not running")?;
    let gossip = content_node
        .gossip()
        .await
        .ok_or("gossip not available")?;
    let live = content_node
        .live()
        .await
        .ok_or("live not available")?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let name = display_name.unwrap_or_else(|| title.clone());

    let devices = DeviceSelection {
        mic_device_id: mic_id,
        speaker_device_id: speaker_id,
    };

    let ticket = state
        .tutoring
        .create_room(session_id.clone(), title.clone(), name, &endpoint, gossip, live, app, devices)
        .await?;

    // Persist to database
    {
        let db = state.db.lock().unwrap();
        db.conn()
            .execute(
                "INSERT INTO tutoring_sessions (id, title, ticket, status) VALUES (?1, ?2, ?3, 'active')",
                params![session_id, title, ticket],
            )
            .map_err(|e| e.to_string())?;
    }

    Ok(TutoringSessionInfo {
        id: session_id,
        title,
        ticket: Some(ticket),
        status: "active".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        ended_at: None,
    })
}

/// Join an existing tutoring room using a ticket (audio-only).
#[tauri::command]
pub async fn tutoring_join_room(
    ticket: String,
    title: Option<String>,
    display_name: Option<String>,
    _camera_id: Option<String>,
    mic_id: Option<String>,
    speaker_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TutoringSessionInfo, String> {
    let content_node = &state.content_node;

    let endpoint = content_node
        .endpoint()
        .await
        .ok_or("iroh node not running")?;
    let gossip = content_node
        .gossip()
        .await
        .ok_or("gossip not available")?;
    let live = content_node
        .live()
        .await
        .ok_or("live not available")?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let title = title.unwrap_or_else(|| "Joined session".into());
    let name = display_name.unwrap_or_else(|| title.clone());

    let devices = DeviceSelection {
        mic_device_id: mic_id,
        speaker_device_id: speaker_id,
    };

    let resolved_ticket = state
        .tutoring
        .join_room(session_id.clone(), title.clone(), name, &ticket, &endpoint, gossip, live, app, devices)
        .await?;

    // Persist to database
    {
        let db = state.db.lock().unwrap();
        db.conn()
            .execute(
                "INSERT INTO tutoring_sessions (id, title, ticket, status) VALUES (?1, ?2, ?3, 'active')",
                params![session_id, title, resolved_ticket],
            )
            .map_err(|e| e.to_string())?;
    }

    Ok(TutoringSessionInfo {
        id: session_id,
        title,
        ticket: Some(resolved_ticket),
        status: "active".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        ended_at: None,
    })
}

/// Leave the current tutoring room.
#[tauri::command]
pub async fn tutoring_leave_room(state: State<'_, AppState>) -> Result<(), String> {
    // Get session ID before leaving
    let session_id = state
        .tutoring
        .status()
        .await
        .map(|s| s.session_id);

    state.tutoring.leave_room().await?;

    // Update database
    if let Some(id) = session_id {
        let db = state.db.lock().unwrap();
        db.conn()
            .execute(
                "UPDATE tutoring_sessions SET status = 'ended', ended_at = datetime('now') WHERE id = ?1",
                params![id],
            )
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Toggle camera on/off — not available on mobile.
#[tauri::command]
pub async fn tutoring_toggle_video(
    _enable: bool,
    _state: State<'_, AppState>,
) -> Result<bool, String> {
    Err("Video is not available on mobile (audio-only mode)".into())
}

/// Toggle microphone on/off.
#[tauri::command]
pub async fn tutoring_toggle_audio(
    enable: bool,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    state.tutoring.toggle_audio(enable).await
}

/// Toggle screen sharing — not available on mobile.
#[tauri::command]
pub async fn tutoring_toggle_screen_share(
    _enable: bool,
    _state: State<'_, AppState>,
) -> Result<bool, String> {
    Err("Screen sharing is not available on mobile".into())
}

/// Send a chat message to all peers in the current room.
#[tauri::command]
pub async fn tutoring_send_chat(
    text: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.tutoring.send_chat(text).await
}

/// Get the current session status (or null if not in a session).
#[tauri::command]
pub async fn tutoring_status(
    state: State<'_, AppState>,
) -> Result<Option<crate::tutoring::manager_mobile::SessionStatus>, String> {
    Ok(state.tutoring.status().await)
}

/// Get peers in the current room.
#[tauri::command]
pub async fn tutoring_peers(
    state: State<'_, AppState>,
) -> Result<Vec<crate::tutoring::manager_mobile::TutoringPeer>, String> {
    Ok(state.tutoring.peers().await)
}

/// List all tutoring sessions from the database.
#[tauri::command]
pub async fn tutoring_list_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<TutoringSessionInfo>, String> {
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

/// Check device availability — audio only on mobile (no camera).
#[tauri::command]
pub async fn tutoring_check_devices() -> Result<DeviceCheckResult, String> {
    let has_audio = !AudioBackend::list_input_devices().is_empty();

    Ok(DeviceCheckResult {
        has_camera: false,
        camera_name: None,
        has_audio,
        error: if !has_audio {
            Some("No microphone detected".into())
        } else {
            None
        },
    })
}

/// List available audio devices (no cameras on mobile).
#[tauri::command]
pub async fn tutoring_list_devices() -> Result<DeviceList, String> {
    let audio_inputs: Vec<AudioDeviceInfo> = AudioBackend::list_input_devices()
        .into_iter()
        .map(|d| AudioDeviceInfo {
            id: d.id.to_string(),
            name: d.name,
            is_default: d.is_default,
        })
        .collect();

    let audio_outputs: Vec<AudioDeviceInfo> = AudioBackend::list_output_devices()
        .into_iter()
        .map(|d| AudioDeviceInfo {
            id: d.id.to_string(),
            name: d.name,
            is_default: d.is_default,
        })
        .collect();

    Ok(DeviceList {
        audio_inputs,
        audio_outputs,
        cameras: vec![], // No cameras on mobile (Phase 2 = audio-only)
    })
}

/// Get current mic audio level (0.0–1.0) for the VU meter.
#[tauri::command]
pub async fn tutoring_get_audio_level(
    state: State<'_, AppState>,
) -> Result<f32, String> {
    Ok(state.tutoring.get_mic_level().await)
}
