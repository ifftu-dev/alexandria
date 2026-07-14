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
use crate::ipfs::storage;
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
/// Tracks the content as a pin and triggers eviction if over quota.
#[tauri::command]
pub async fn content_add(
    state: State<'_, AppState>,
    data: Vec<u8>,
) -> Result<content::AddResult, String> {
    let result = content::add_bytes(&state.content_node, &data)
        .await
        .map_err(|e| e.to_string())?;

    // Track as a cache pin (auto_unpin = true) by default
    if let Ok(guard) = state.db.lock() {
        if let Some(db) = guard.as_ref() {
            storage::upsert_pin(db.conn(), &result.hash, "cache", result.size, true);
        }
    }

    // Trigger eviction if over quota
    storage::maybe_evict(&state.content_node, &state.db).await;

    // Announce over iroh that this node now serves the blob, so peers can fetch
    // it directly (the P2P storage path) instead of via the IPFS gateway.
    if let (Ok(hash), Some(endpoint)) = (
        content::parse_hash(&result.hash),
        state.content_node.endpoint().await,
    ) {
        if let Err(e) = state.discovery.announce_have(hash, &endpoint).await {
            log::debug!("content_add: discovery announce failed: {e}");
        }
    }

    Ok(result)
}

/// Fetch content from the local blob store by BLAKE3 hash.
///
/// Returns the raw bytes. Errors if the content is not available locally.
/// Updates the pin's last_accessed timestamp.
#[tauri::command]
pub async fn content_get(state: State<'_, AppState>, hash: String) -> Result<Vec<u8>, String> {
    let bytes = content::get_bytes(&state.content_node, &hash)
        .await
        .map_err(|e| e.to_string())?;

    // Touch last_accessed so frequently-read content is evicted last
    if let Ok(guard) = state.db.lock() {
        if let Some(db) = guard.as_ref() {
            storage::touch_pin(db.conn(), &hash);
        }
    }

    Ok(bytes)
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

    // Track resolved content as a cache pin
    if result.source != resolver::ResolveSource::Local {
        if let Ok(guard) = state.db.lock() {
            if let Some(db) = guard.as_ref() {
                storage::upsert_pin(db.conn(), &result.blake3_hash, "cache", result.size, true);
            }
        }
        // Trigger eviction if over quota
        storage::maybe_evict(&state.content_node, &state.db).await;
    } else {
        // Touch existing pin on local hit
        if let Ok(guard) = state.db.lock() {
            if let Some(db) = guard.as_ref() {
                storage::touch_pin(db.conn(), &result.blake3_hash);
            }
        }
    }

    Ok(ResolveResponse {
        blake3_hash: result.blake3_hash,
        ipfs_cid: result.ipfs_cid,
        source: result.source,
        size: result.size,
    })
}

/// Resolve content and materialize it as a file in the video cache.
///
/// Returns the absolute path of the cached file. The frontend wraps this
/// with `convertFileSrc()` so the `<video>` element loads it through
/// Tauri's asset protocol — the only media path WKWebView's AVFoundation
/// engine reliably honors on iOS (custom URI-scheme handlers are ignored
/// for `<video>` media loads).
///
/// The file is named by its BLAKE3 hash and reused on subsequent calls,
/// so each blob is written to disk at most once.
#[tauri::command]
pub async fn content_cache_file(
    state: State<'_, AppState>,
    identifier: String,
) -> Result<String, String> {
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

    let path = state
        .video_cache_dir()?
        .join(format!("{}.mp4", result.blake3_hash));

    // Reuse the file if it's already materialized at the right size.
    let needs_write = match std::fs::metadata(&path) {
        Ok(meta) => meta.len() != result.size,
        Err(_) => true,
    };
    if needs_write {
        std::fs::write(&path, &result.bytes)
            .map_err(|e| format!("failed to write video cache file: {e}"))?;
    }

    // Track resolved content as a cache pin (mirrors content_resolve_bytes).
    if result.source != resolver::ResolveSource::Local {
        if let Ok(guard) = state.db.lock() {
            if let Some(db) = guard.as_ref() {
                storage::upsert_pin(db.conn(), &result.blake3_hash, "cache", result.size, true);
            }
        }
        storage::maybe_evict(&state.content_node, &state.db).await;
    } else if let Ok(guard) = state.db.lock() {
        if let Some(db) = guard.as_ref() {
            storage::touch_pin(db.conn(), &result.blake3_hash);
        }
    }

    Ok(path.to_string_lossy().into_owned())
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

    // Track resolved content as a cache pin
    if result.source != resolver::ResolveSource::Local {
        if let Ok(guard) = state.db.lock() {
            if let Some(db) = guard.as_ref() {
                storage::upsert_pin(db.conn(), &result.blake3_hash, "cache", result.size, true);
            }
        }
        storage::maybe_evict(&state.content_node, &state.db).await;
    } else if let Ok(guard) = state.db.lock() {
        if let Some(db) = guard.as_ref() {
            storage::touch_pin(db.conn(), &result.blake3_hash);
        }
    }

    Ok(result.bytes)
}
