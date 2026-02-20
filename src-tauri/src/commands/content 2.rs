//! IPC commands for content-addressed storage (iroh).
//!
//! These commands expose the iroh blob store to the frontend for
//! adding, fetching, and querying content by BLAKE3 hash.

use serde::Serialize;
use tauri::State;

use crate::ipfs::content;
use crate::AppState;

/// Status of the content node.
#[derive(Debug, Serialize)]
pub struct NodeStatus {
    /// Whether the iroh node is running.
    pub running: bool,
    /// The node's public key / peer ID (hex), or null if not running.
    pub node_id: Option<String>,
}

/// Get the current status of the iroh content node.
#[tauri::command]
pub async fn content_node_status(state: State<'_, AppState>) -> Result<NodeStatus, String> {
    let running = state.content_node.is_running().await;
    let node_id = state.content_node.node_id().await;

    Ok(NodeStatus { running, node_id })
}

/// Add raw content to the local blob store.
///
/// Accepts base64-encoded data from the frontend.
/// Returns the BLAKE3 hash (hex) and size in bytes.
#[tauri::command]
pub async fn content_add(
    state: State<'_, AppState>,
    data: Vec<u8>,
) -> Result<content::AddResult, String> {
    content::add_bytes(&state.content_node, &data)
        .await
        .map_err(|e| e.to_string())
}

/// Fetch content from the local blob store by BLAKE3 hash.
///
/// Returns the raw bytes. Errors if the content is not available locally.
#[tauri::command]
pub async fn content_get(
    state: State<'_, AppState>,
    hash: String,
) -> Result<Vec<u8>, String> {
    content::get_bytes(&state.content_node, &hash)
        .await
        .map_err(|e| e.to_string())
}

/// Check if content exists in the local blob store.
#[tauri::command]
pub async fn content_has(
    state: State<'_, AppState>,
    hash: String,
) -> Result<bool, String> {
    content::has(&state.content_node, &hash)
        .await
        .map_err(|e| e.to_string())
}
