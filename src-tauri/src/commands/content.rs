//! IPC commands for content-addressed storage (iroh + IPFS gateway).
//!
//! These commands expose the iroh blob store to the frontend for
//! adding, fetching, and querying content by BLAKE3 hash. The
//! `content_resolve` command additionally supports IPFS CIDs with
//! automatic gateway fallback.

use serde::Serialize;
use tauri::State;

use crate::ipfs::content;
use crate::ipfs::resolver;
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
pub async fn content_get(state: State<'_, AppState>, hash: String) -> Result<Vec<u8>, String> {
    content::get_bytes(&state.content_node, &hash)
        .await
        .map_err(|e| e.to_string())
}

/// Check if content exists in the local blob store.
#[tauri::command]
pub async fn content_has(state: State<'_, AppState>, hash: String) -> Result<bool, String> {
    content::has(&state.content_node, &hash)
        .await
        .map_err(|e| e.to_string())
}

/// Metadata about resolved content (bytes excluded for the response).
#[derive(Debug, Serialize)]
pub struct ResolveResponse {
    /// BLAKE3 hash of the content.
    pub blake3_hash: String,
    /// IPFS CID if known.
    pub ipfs_cid: Option<String>,
    /// Where the content was resolved from.
    pub source: resolver::ResolveSource,
    /// Size in bytes.
    pub size: u64,
}

/// Resolve content by any identifier (BLAKE3 hex or IPFS CID).
///
/// Uses the full resolution chain: local store → CID mapping →
/// IPFS gateway fallback. Content fetched from gateways is cached
/// locally and mapped for future lookups.
///
/// Returns the raw bytes and metadata about the resolution.
#[tauri::command]
pub async fn content_resolve(
    state: State<'_, AppState>,
    identifier: String,
) -> Result<ResolveResponse, String> {
    let resolver = {
        let guard = state.resolver.lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "content resolver not initialized".to_string())?
    };

    let result = resolver
        .resolve(&identifier)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ResolveResponse {
        blake3_hash: result.blake3_hash,
        ipfs_cid: result.ipfs_cid,
        source: result.source,
        size: result.size,
    })
}

/// Resolve content and return the raw bytes.
///
/// Same as `content_resolve` but returns the actual content data.
/// Use this when you need the bytes (e.g., displaying course content).
#[tauri::command]
pub async fn content_resolve_bytes(
    state: State<'_, AppState>,
    identifier: String,
) -> Result<Vec<u8>, String> {
    let resolver = {
        let guard = state.resolver.lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "content resolver not initialized".to_string())?
    };

    let result = resolver
        .resolve(&identifier)
        .await
        .map_err(|e| e.to_string())?;

    Ok(result.bytes)
}
