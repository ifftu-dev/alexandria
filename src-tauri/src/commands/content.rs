//! IPC commands for content-addressed storage (iroh blobs).
//!
//! These commands expose the iroh blob store to the frontend for
//! adding, fetching, and querying content by BLAKE3 hash. The
//! `content_resolve` command additionally accepts a public URL and
//! caches the fetched bytes into the local store.

use crate::profile::scope::ProfileState as State;
use serde::Serialize;

use crate::content_store::content;
use crate::content_store::resolver;
use crate::content_store::storage;
use crate::db::executor::DatabaseWorkload;
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

    // Track as a cache pin (auto_unpin = true) by default.
    let pin_hash = result.hash.clone();
    if let Err(error) = state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            state.profile_lease(),
            "content.add.track-pin",
            move |db| storage::upsert_pin(db.conn(), &pin_hash, "cache", result.size, true),
        )
        .await
    {
        log::warn!("content_add: failed to track cache pin: {error}");
    }

    // Trigger eviction if over quota
    storage::maybe_evict(&state.content_node, &state.db).await;

    // Announce over iroh that this node now serves the blob, so peers can fetch
    // it directly (the P2P storage path) instead of from the origin URL.
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

/// Fetch UTF-8 text from the local blob store without expanding every byte
/// into a JSON number in the webview bridge.
#[tauri::command]
pub async fn content_get_text(state: State<'_, AppState>, hash: String) -> Result<String, String> {
    decode_utf8(get_and_touch(&state, &hash).await?)
}

async fn get_and_touch(state: &State<'_, AppState>, hash: &str) -> Result<Vec<u8>, String> {
    let bytes = content::get_bytes(&state.content_node, hash)
        .await
        .map_err(|e| e.to_string())?;

    let pin_hash = hash.to_string();
    if let Err(error) = state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            state.profile_lease(),
            "content.get.touch-pin",
            move |db| storage::touch_pin(db.conn(), &pin_hash),
        )
        .await
    {
        log::warn!("content_get: failed to update cache access time: {error}");
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
    /// Public URL for the content if known.
    pub external_id: Option<String>,
    /// Where the content was resolved from.
    pub source: resolver::ResolveSource,
    /// Size in bytes.
    pub size: u64,
}

/// Resolve content by any identifier (BLAKE3 hex or public URL).
///
/// Uses the full resolution chain: local store → iroh peers →
/// public URL fallback. Content fetched from a URL is cached
/// locally and mapped for future lookups.
///
/// Returns the raw bytes and metadata about the resolution.
#[tauri::command]
pub async fn content_resolve(
    state: State<'_, AppState>,
    identifier: String,
) -> Result<ResolveResponse, String> {
    let result = resolve_and_track(&state, &identifier).await?;

    Ok(ResolveResponse {
        blake3_hash: result.blake3_hash,
        external_id: result.external_id,
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
    let result = resolve_and_track(&state, &identifier).await?;

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
    Ok(resolve_and_track(&state, &identifier).await?.bytes)
}

/// Resolve UTF-8 text without serializing the payload as a JavaScript
/// `number[]`. Binary consumers must continue to use a binary/file path.
#[tauri::command]
pub async fn content_resolve_text(
    state: State<'_, AppState>,
    identifier: String,
) -> Result<String, String> {
    decode_utf8(resolve_and_track(&state, &identifier).await?.bytes)
}

async fn resolve_and_track(
    state: &State<'_, AppState>,
    identifier: &str,
) -> Result<resolver::ResolveResult, String> {
    let resolver = {
        let guard = state.resolver.lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "content resolver not initialized".to_string())?
    };

    let result = resolver
        .resolve(identifier)
        .await
        .map_err(|e| e.to_string())?;

    if result.source != resolver::ResolveSource::Local {
        let pin_hash = result.blake3_hash.clone();
        if let Err(error) = state
            .db_executor
            .execute(
                DatabaseWorkload::Background,
                state.profile_lease(),
                "content.resolve.track-pin",
                move |db| storage::upsert_pin(db.conn(), &pin_hash, "cache", result.size, true),
            )
            .await
        {
            log::warn!("content_resolve: failed to track cache pin: {error}");
        }
        storage::maybe_evict(&state.content_node, &state.db).await;
    } else {
        let pin_hash = result.blake3_hash.clone();
        if let Err(error) = state
            .db_executor
            .execute(
                DatabaseWorkload::Background,
                state.profile_lease(),
                "content.resolve.touch-pin",
                move |db| storage::touch_pin(db.conn(), &pin_hash),
            )
            .await
        {
            log::warn!("content_resolve: failed to update cache access time: {error}");
        }
    }

    Ok(result)
}

fn decode_utf8(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|error| format!("content is not valid UTF-8: {error}"))
}

#[cfg(test)]
mod tests {
    use super::decode_utf8;

    #[test]
    fn text_payloads_require_valid_utf8() {
        assert_eq!(decode_utf8("नमस्ते".as_bytes().to_vec()).unwrap(), "नमस्ते");
        assert!(decode_utf8(vec![0xff, 0xfe]).is_err());
    }
}
