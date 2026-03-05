//! Tauri commands for live tutoring sessions.
//!
//! All commands operate through the `TutoringManager` in `AppState`.
//! Room creation/joining requires the iroh content node to be running
//! (it provides the shared QUIC endpoint, gossip, and live instances).

use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, State};

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
}

/// Create a new tutoring room (host).
///
/// Starts camera + mic capture, creates a gossip topic, and returns
/// a serialized room ticket that others can use to join.
#[tauri::command]
pub async fn tutoring_create_room(
    title: String,
    display_name: Option<String>,
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

    let ticket = state
        .tutoring
        .create_room(session_id.clone(), title.clone(), name, &endpoint, gossip, live, app)
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

/// Join an existing tutoring room using a ticket.
#[tauri::command]
pub async fn tutoring_join_room(
    ticket: String,
    title: Option<String>,
    display_name: Option<String>,
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

    let resolved_ticket = state
        .tutoring
        .join_room(session_id.clone(), title.clone(), name, &ticket, &endpoint, gossip, live, app)
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
