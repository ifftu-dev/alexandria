//! Tauri commands for live tutoring sessions.
//!
//! All commands operate through the `TutoringManager` in `AppState`.
//! Room creation/joining requires the iroh content node to be running
//! (it provides the shared QUIC endpoint, gossip, and live instances).

use iroh_live::media::audio::AudioBackend;
use iroh_live::media::capture::CameraCapturer;
use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::AppState;
use crate::tutoring::manager::DeviceSelection;

/// Result of a pre-join device availability check.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceCheckResult {
    /// Whether at least one camera was found.
    pub has_camera: bool,
    /// Name of the first camera (e.g. "FaceTime HD Camera").
    pub camera_name: Option<String>,
    /// Whether the audio backend initialized successfully.
    pub has_audio: bool,
    /// Error message if something failed (informational).
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

/// Create a new tutoring room (host).
///
/// Starts camera + mic capture, creates a gossip topic, and returns
/// a serialized room ticket that others can use to join.
#[tauri::command]
pub async fn tutoring_create_room(
    title: String,
    display_name: Option<String>,
    camera_id: Option<String>,
    mic_id: Option<String>,
    speaker_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TutoringSessionInfo, String> {
    log::info!("[cmd] tutoring_create_room: start");
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

    log::info!("[cmd] tutoring_create_room: got endpoint/gossip/live, calling create_room...");
    let session_id = uuid::Uuid::new_v4().to_string();
    let name = display_name.unwrap_or_else(|| title.clone());

    let devices = DeviceSelection {
        camera_index: camera_id,
        mic_device_id: mic_id,
        speaker_device_id: speaker_id,
    };

    let ticket = state
        .tutoring
        .create_room(session_id.clone(), title.clone(), name, &endpoint, gossip, live, app, devices)
        .await?;

    log::info!("[cmd] tutoring_create_room: create_room returned, inserting into DB...");
    // Persist to database
    {
        let db = state.db.lock().unwrap();
        db.conn()
            .execute(
                "INSERT INTO tutoring_sessions (id, title, ticket, status) VALUES (?1, ?2, ?3, 'active')",
                params![session_id, title, ticket],
            )
            .map_err(|e| {
                log::error!("[cmd] tutoring_create_room: DB insert failed: {e}");
                e.to_string()
            })?;
    }

    log::info!("[cmd] tutoring_create_room: done, returning to frontend");
    Ok(TutoringSessionInfo {
        id: session_id,
        title,
        ticket: Some(ticket),
        status: "active".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        ended_at: None,
    })
}

/// Join an existing tutoring room using a ticket.
#[tauri::command]
pub async fn tutoring_join_room(
    ticket: String,
    title: Option<String>,
    display_name: Option<String>,
    camera_id: Option<String>,
    mic_id: Option<String>,
    speaker_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TutoringSessionInfo, String> {
    log::info!("[cmd] tutoring_join_room: start");
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

    log::info!("[cmd] tutoring_join_room: got endpoint/gossip/live, calling join_room...");
    let session_id = uuid::Uuid::new_v4().to_string();
    let title = title.unwrap_or_else(|| "Joined session".into());
    let name = display_name.unwrap_or_else(|| title.clone());

    let devices = DeviceSelection {
        camera_index: camera_id,
        mic_device_id: mic_id,
        speaker_device_id: speaker_id,
    };

    let resolved_ticket = state
        .tutoring
        .join_room(session_id.clone(), title.clone(), name, &ticket, &endpoint, gossip, live, app, devices)
        .await?;

    log::info!("[cmd] tutoring_join_room: join_room returned, inserting into DB...");
    // Persist to database
    {
        let db = state.db.lock().unwrap();
        db.conn()
            .execute(
                "INSERT INTO tutoring_sessions (id, title, ticket, status) VALUES (?1, ?2, ?3, 'active')",
                params![session_id, title, resolved_ticket],
            )
            .map_err(|e| {
                log::error!("[cmd] tutoring_join_room: DB insert failed: {e}");
                e.to_string()
            })?;
    }

    log::info!("[cmd] tutoring_join_room: done, returning to frontend");
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

/// Toggle camera on/off.
#[tauri::command]
pub async fn tutoring_toggle_video(
    enable: bool,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    state.tutoring.toggle_video(enable).await
}

/// Toggle microphone on/off.
#[tauri::command]
pub async fn tutoring_toggle_audio(
    enable: bool,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    state.tutoring.toggle_audio(enable).await
}

/// Toggle screen sharing on/off.
#[tauri::command]
pub async fn tutoring_toggle_screen_share(
    enable: bool,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    state.tutoring.toggle_screen_share(enable).await
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
) -> Result<Option<crate::tutoring::manager::SessionStatus>, String> {
    Ok(state.tutoring.status().await)
}

/// Get peers in the current room.
#[tauri::command]
pub async fn tutoring_peers(
    state: State<'_, AppState>,
) -> Result<Vec<crate::tutoring::manager::TutoringPeer>, String> {
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

/// Check device availability (camera + audio) before joining a session.
///
/// Uses lightweight enumeration only — does NOT start audio streams or
/// camera capture. Camera check runs on a blocking thread with timeout.
#[tauri::command]
pub async fn tutoring_check_devices() -> Result<DeviceCheckResult, String> {
    // Check audio via cpal device enumeration (no streams started)
    let has_audio = !AudioBackend::list_input_devices().is_empty();

    // Check camera on blocking thread with timeout
    let (has_camera, camera_name) = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(|| CameraCapturer::list_cameras()),
    )
    .await
    {
        Ok(Ok(Ok(cameras))) => {
            let name = cameras.first().map(|(_, n)| n.clone());
            (!cameras.is_empty(), name)
        }
        _ => (false, None),
    };

    Ok(DeviceCheckResult {
        has_camera,
        camera_name,
        has_audio,
        error: None,
    })
}

// ── Device listing & audio levels ─────────────────────────────────

/// Info about an available audio device.
#[derive(Debug, Clone, Serialize)]
pub struct AudioDeviceInfo {
    /// Device ID string (stable across restarts, can be passed back for selection).
    pub id: String,
    /// Human-readable name.
    pub name: Option<String>,
    /// Whether this is the system default device.
    pub is_default: bool,
}

/// Info about an available camera.
#[derive(Debug, Clone, Serialize)]
pub struct CameraDeviceInfo {
    /// Camera index (numeric or string key).
    pub index: String,
    /// Human-readable name.
    pub name: String,
}

/// Available devices for audio and video.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceList {
    pub audio_inputs: Vec<AudioDeviceInfo>,
    pub audio_outputs: Vec<AudioDeviceInfo>,
    pub cameras: Vec<CameraDeviceInfo>,
}

/// List all available audio and camera devices.
///
/// Camera enumeration via nokhwa uses AVFoundation on macOS which can
/// block waiting for the permission dialog. We run it on a blocking
/// thread with a timeout to avoid hanging the UI.
#[tauri::command]
pub async fn tutoring_list_devices() -> Result<DeviceList, String> {
    // Audio devices (cpal enumeration — lightweight, no streams started)
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

    // Camera devices — run on blocking thread with timeout since
    // nokhwa_initialize can block on the macOS permission dialog.
    let cameras = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(|| CameraCapturer::list_cameras()),
    )
    .await
    {
        Ok(Ok(Ok(cams))) => cams
            .into_iter()
            .map(|(idx, name)| CameraDeviceInfo {
                index: format!("{idx:?}"),
                name,
            })
            .collect(),
        Ok(Ok(Err(e))) => {
            log::warn!("tutoring: camera enumeration failed: {e}");
            Vec::new()
        }
        Ok(Err(e)) => {
            log::warn!("tutoring: camera enumeration task panicked: {e}");
            Vec::new()
        }
        Err(_) => {
            log::warn!("tutoring: camera enumeration timed out (5s) — may need camera permission");
            Vec::new()
        }
    };

    Ok(DeviceList {
        audio_inputs,
        audio_outputs,
        cameras,
    })
}

/// Get current mic audio level (0.0–1.0) for the VU meter.
///
/// This is a poll-based alternative to the `tutoring:audio-level` Tauri event.
/// The frontend can use either mechanism.
#[tauri::command]
pub async fn tutoring_get_audio_level(
    state: State<'_, AppState>,
) -> Result<f32, String> {
    Ok(state.tutoring.get_mic_level().await)
}

/// Get diagnostic info about the current A/V pipeline state for debugging.
#[tauri::command]
pub async fn tutoring_diagnostics(
    state: State<'_, AppState>,
) -> Result<Option<crate::tutoring::manager::SessionDiagnostics>, String> {
    Ok(state.tutoring.diagnostics().await)
}
