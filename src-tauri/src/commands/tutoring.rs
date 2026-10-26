//! Tauri commands for live calls, on every platform.
//!
//! All commands operate through the `TutoringManager` in `AppState`. Room
//! creation/joining requires the iroh content node to be running (it
//! provides the shared QUIC endpoint, gossip, and live instances). What a
//! platform cannot do (share a screen, switch cameras, change the audio
//! route mid-call) the manager refuses with a message for the learner.

use crate::profile::scope::ProfileState as State;
use crate::tutoring::call_kind::CallKind;
pub use crate::tutoring::manager::{DeviceCheckResult, DeviceList};
use crate::tutoring::manager::{Network, RoomRequest};
use crate::tutoring::media::DeviceSelection;
use crate::tutoring::records::ProfileRecords;
use rusqlite::params;
use serde::Serialize;

use crate::db::executor::DatabaseWorkload;
use crate::AppState;

/// Summary of a tutoring session from the database.
#[derive(Debug, Clone, Serialize)]
pub struct TutoringSessionInfo {
    pub id: String,
    pub title: String,
    pub ticket: Option<String>,
    pub status: String,
    pub created_at: String,
    pub ended_at: Option<String>,
    /// What the call is for; see [`CallKind`].
    pub kind: CallKind,
    /// The classroom, interview or plugin element the call belongs to.
    pub context_id: Option<String>,
}

/// The unlocked profile's database, for a call's session rows.
pub(crate) fn records(state: &State<'_, AppState>) -> ProfileRecords {
    ProfileRecords {
        executor: state.db_executor.clone(),
        lease: state.profile_lease(),
    }
}

/// The iroh node a call rides on.
pub(crate) async fn network(state: &State<'_, AppState>) -> Result<Network, String> {
    let content_node = &state.content_node;
    let endpoint = content_node
        .endpoint()
        .await
        .ok_or("iroh node not running")?;
    let gossip = content_node.gossip().await.ok_or("gossip not available")?;
    let live = content_node.live().await.ok_or("live not available")?;
    Ok(Network {
        endpoint,
        gossip,
        live,
    })
}

/// Create a new tutoring room (host).
///
/// Starts camera + mic capture, creates a gossip topic, and returns
/// a serialized room ticket that others can use to join.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn tutoring_create_room(
    title: String,
    display_name: Option<String>,
    camera_id: Option<String>,
    mic_id: Option<String>,
    speaker_id: Option<String>,
    kind: Option<String>,
    context_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<TutoringSessionInfo, String> {
    let kind = CallKind::parse(kind.as_deref())?;
    // Same gate as join: the host opens the same devices.
    crate::av_permissions::ensure_camera_and_microphone().await?;
    log::info!("[cmd] tutoring_create_room: start");
    let network = network(&state).await?;
    let session_id = uuid::Uuid::new_v4().to_string();
    let request = RoomRequest {
        session_id: session_id.clone(),
        title: title.clone(),
        display_name: display_name.unwrap_or_else(|| title.clone()),
        kind,
        context_id: context_id.clone(),
        devices: DeviceSelection {
            camera_index: camera_id,
            mic_device_id: mic_id,
            speaker_device_id: speaker_id,
        },
        classroom_call: None,
    };
    let ticket = state
        .tutoring
        .create_room(request, network, &records(&state))
        .await?;
    log::info!("[cmd] tutoring_create_room: done, returning to frontend");
    Ok(TutoringSessionInfo {
        id: session_id,
        title,
        ticket: Some(ticket),
        status: "active".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        ended_at: None,
        kind,
        context_id,
    })
}

/// Join an existing tutoring room using a ticket.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn tutoring_join_room(
    ticket: String,
    title: Option<String>,
    display_name: Option<String>,
    camera_id: Option<String>,
    mic_id: Option<String>,
    speaker_id: Option<String>,
    kind: Option<String>,
    context_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<TutoringSessionInfo, String> {
    let kind = CallKind::parse(kind.as_deref())?;
    let records = records(&state);
    state
        .tutoring
        .check_join_kind(&records, &ticket, kind)
        .await?;
    crate::tutoring::presence::check_join(&state, &ticket).await?;
    log::info!("[cmd] tutoring_join_room: start");
    // First consumption of the camera and microphone on this path: ask now,
    // not at app launch, and refuse cleanly rather than abort inside cpal.
    crate::av_permissions::ensure_camera_and_microphone().await?;
    let network = network(&state).await?;
    let session_id = uuid::Uuid::new_v4().to_string();
    let title = title.unwrap_or_else(|| "Joined session".into());
    let request = RoomRequest {
        session_id: session_id.clone(),
        title: title.clone(),
        display_name: display_name.unwrap_or_else(|| title.clone()),
        kind,
        context_id: context_id.clone(),
        devices: DeviceSelection {
            camera_index: camera_id,
            mic_device_id: mic_id,
            speaker_device_id: speaker_id,
        },
        classroom_call: None,
    };
    let resolved_ticket = state
        .tutoring
        .join_room(request, &ticket, network, &records)
        .await?;
    log::info!("[cmd] tutoring_join_room: done, returning to frontend");
    Ok(TutoringSessionInfo {
        id: session_id,
        title,
        ticket: Some(resolved_ticket),
        status: "active".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        ended_at: None,
        kind,
        context_id,
    })
}

/// Leave the current tutoring room.
#[tauri::command]
pub async fn tutoring_leave_room(state: State<'_, AppState>) -> Result<(), String> {
    state.tutoring.leave_room(&records(&state)).await
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

/// Use a different camera for the rest of the session, switching now if
/// the camera is on. Returns the name of the camera in use.
#[tauri::command]
pub async fn tutoring_set_camera(
    camera_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    state.tutoring.set_camera(camera_id).await
}

/// Move the call's audio to other devices, where the platform allows it
/// mid-call.
#[tauri::command]
pub async fn tutoring_set_audio_devices(
    mic_id: Option<String>,
    speaker_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.tutoring.set_audio_devices(mic_id, speaker_id).await
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
pub async fn tutoring_send_chat(text: String, state: State<'_, AppState>) -> Result<(), String> {
    state.tutoring.send_chat(text).await
}

/// Share a participant-controlled on-device transcript segment with the room.
#[tauri::command]
pub async fn tutoring_send_transcript(
    text: String,
    confidence: Option<f64>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.tutoring.send_transcript(text, confidence).await
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
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "tutoring.list-sessions",
            |db| {
                let mut stmt = db
                    .conn()
                    .prepare(
                        "SELECT id, title, ticket, status, created_at, ended_at, kind, context_id
                         FROM tutoring_sessions
                         ORDER BY created_at DESC
                         LIMIT 50",
                    )
                    .map_err(|e| e.to_string())?;

                let sessions = stmt
                    .query_map(params![], |row| {
                        Ok(TutoringSessionInfo {
                            id: row.get(0)?,
                            title: row.get(1)?,
                            ticket: row.get(2)?,
                            status: row.get(3)?,
                            created_at: row.get(4)?,
                            ended_at: row.get(5)?,
                            kind: CallKind::parse(row.get::<_, Option<String>>(6)?.as_deref())
                                .unwrap_or(CallKind::Tutoring),
                            context_id: row.get(7)?,
                        })
                    })
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                Ok(sessions)
            },
        )
        .await
}

/// Check device availability (camera + audio) before joining a session.
///
/// Uses lightweight enumeration only — does NOT start audio streams or
/// camera capture.
#[tauri::command]
pub async fn tutoring_check_devices(
    state: tauri::State<'_, AppState>,
) -> Result<DeviceCheckResult, String> {
    Ok(state.tutoring.check_devices().await)
}

/// List all available audio and camera devices.
#[tauri::command]
pub async fn tutoring_list_devices(
    state: tauri::State<'_, AppState>,
) -> Result<DeviceList, String> {
    Ok(state.tutoring.list_devices().await)
}

/// Get current mic audio level (0.0–1.0) for the VU meter.
///
/// This is a poll-based alternative to the `tutoring:audio-level` Tauri event.
/// The frontend can use either mechanism.
#[tauri::command]
pub async fn tutoring_get_audio_level(state: State<'_, AppState>) -> Result<f32, String> {
    Ok(state.tutoring.get_mic_level().await)
}

/// Get diagnostic info about the current A/V pipeline state for debugging.
#[tauri::command]
pub async fn tutoring_diagnostics(
    state: State<'_, AppState>,
) -> Result<Option<crate::tutoring::manager::SessionDiagnostics>, String> {
    Ok(state.tutoring.diagnostics().await)
}
