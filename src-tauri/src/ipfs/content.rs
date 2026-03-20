//! Content-addressed blob operations.
//!
//! Provides high-level functions for adding, fetching, and managing
//! content in the embedded iroh node. Content is addressed by BLAKE3
//! hash (32 bytes, typically represented as 64-char hex strings).
//!
//! Pin tracking is stored in SQLite (`pins` table) to survive restarts
//! and support storage management (LRU eviction under pressure).

use iroh_blobs::Hash;
use thiserror::Error;

use super::node::ContentNode;

#[derive(Error, Debug)]
pub enum ContentError {
    #[error("node not running")]
    NodeNotRunning,
    #[error("content not found: {0}")]
    NotFound(String),
    #[error("store error: {0}")]
    Store(String),
    #[error("invalid hash: {0}")]
    InvalidHash(String),
}

/// Result of adding content to the store.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AddResult {
    /// The BLAKE3 hash of the content (64-char hex string).
    pub hash: String,
    /// Size of the content in bytes.
    pub size: u64,
}

/// Information about stored content.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContentInfo {
    /// The BLAKE3 hash (hex).
    pub hash: String,
    /// Whether the content is available locally.
    pub is_local: bool,
}

/// Add raw bytes to the content store.
///
/// Returns the BLAKE3 hash of the content. The content is stored
/// persistently and will survive app restarts.
pub async fn add_bytes(node: &ContentNode, data: &[u8]) -> Result<AddResult, ContentError> {
    let store = node
        .store()
        .await
        .map_err(|_| ContentError::NodeNotRunning)?;
    let size = data.len() as u64;

    let tag = store
        .add_slice(data)
        .await
        .map_err(|e| ContentError::Store(e.to_string()))?;

    let hash_hex = tag.hash.to_hex().to_string();
    log::debug!("added {} bytes, hash: {}", size, hash_hex);

    Ok(AddResult {
        hash: hash_hex,
        size,
    })
}

/// Fetch content from the local store by BLAKE3 hash.
///
/// Returns the raw bytes. Returns `ContentError::NotFound` if the
/// content is not available locally.
pub async fn get_bytes(node: &ContentNode, hash_hex: &str) -> Result<Vec<u8>, ContentError> {
    let store = node
        .store()
        .await
        .map_err(|_| ContentError::NodeNotRunning)?;
    let hash = parse_hash(hash_hex)?;

    let exists = store
        .has(hash)
        .await
        .map_err(|e| ContentError::Store(e.to_string()))?;

    if !exists {
        return Err(ContentError::NotFound(hash_hex.to_string()));
    }

    let bytes = store
        .get_bytes(hash)
        .await
        .map_err(|e| ContentError::Store(e.to_string()))?;

    Ok(bytes.to_vec())
}

/// Check if content exists in the local store.
pub async fn has(node: &ContentNode, hash_hex: &str) -> Result<bool, ContentError> {
    let store = node
        .store()
        .await
        .map_err(|_| ContentError::NodeNotRunning)?;
    let hash = parse_hash(hash_hex)?;

    store
        .has(hash)
        .await
        .map_err(|e| ContentError::Store(e.to_string()))
}

/// Get information about content in the local store.
pub async fn info(node: &ContentNode, hash_hex: &str) -> Result<ContentInfo, ContentError> {
    let store = node
        .store()
        .await
        .map_err(|_| ContentError::NodeNotRunning)?;
    let hash = parse_hash(hash_hex)?;

    let is_local = store
        .has(hash)
        .await
        .map_err(|e| ContentError::Store(e.to_string()))?;

    Ok(ContentInfo {
        hash: hash_hex.to_string(),
        is_local,
    })
}

/// Parse a hex string into an iroh Hash.
pub fn parse_hash(hex_str: &str) -> Result<Hash, ContentError> {
    let hex_str = hex_str.trim();
    if hex_str.len() != 64 {
        return Err(ContentError::InvalidHash(format!(
            "expected 64-char hex string, got {} chars",
            hex_str.len()
        )));
    }

    let bytes: [u8; 32] = hex::decode(hex_str)
        .map_err(|e| ContentError::InvalidHash(e.to_string()))?
        .try_into()
        .map_err(|_| ContentError::InvalidHash("decoded to wrong length".into()))?;

    Ok(Hash::from_bytes(bytes))
}

/// Format an iroh Hash as a hex string.
pub fn hash_to_hex(hash: &Hash) -> String {
    hash.to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipfs::node::ContentNode;
    use tempfile::TempDir;

    async fn make_node() -> (ContentNode, TempDir) {
        let tmp = TempDir::new().expect("create temp dir");
        let node = ContentNode::new(tmp.path());
        node.start().await.expect("start node");
        (node, tmp)
    }

    #[tokio::test]
    async fn add_and_get_roundtrip() {
        let (node, _tmp) = make_node().await;

        let data = b"Hello, Alexandria!";
        let result = add_bytes(&node, data).await.expect("add failed");

        assert_eq!(result.size, data.len() as u64);
        assert_eq!(result.hash.len(), 64); // hex-encoded BLAKE3

        let retrieved = get_bytes(&node, &result.hash).await.expect("get failed");
        assert_eq!(retrieved, data);

        node.shutdown().await.expect("shutdown failed");
    }

    #[tokio::test]
    async fn get_nonexistent_returns_not_found() {
        let (node, _tmp) = make_node().await;

        let fake_hash = "0".repeat(64);
        let result = get_bytes(&node, &fake_hash).await;
        assert!(matches!(result, Err(ContentError::NotFound(_))));

        node.shutdown().await.expect("shutdown failed");
    }

    #[tokio::test]
    async fn has_returns_correct_status() {
        let (node, _tmp) = make_node().await;

        let data = b"test content";
        let result = add_bytes(&node, data).await.expect("add failed");

        assert!(has(&node, &result.hash).await.expect("has failed"));

        let fake_hash = "0".repeat(64);
        assert!(!has(&node, &fake_hash).await.expect("has failed"));

        node.shutdown().await.expect("shutdown failed");
    }

    #[tokio::test]
    async fn content_survives_restart() {
        let tmp = TempDir::new().expect("create temp dir");

        // Add content, shut down
        let node = ContentNode::new(tmp.path());
        node.start().await.expect("start");
        let data = b"persistent content";
        let result = add_bytes(&node, data).await.expect("add");
        let hash = result.hash.clone();
        node.shutdown().await.expect("shutdown");

        // Restart and verify content is still there
        let node2 = ContentNode::new(tmp.path());
        node2.start().await.expect("restart");
        let retrieved = get_bytes(&node2, &hash).await.expect("get after restart");
        assert_eq!(retrieved, data);
        node2.shutdown().await.expect("shutdown 2");
    }

    #[tokio::test]
    async fn same_content_produces_same_hash() {
        let (node, _tmp) = make_node().await;

        let data = b"deterministic hashing";
        let r1 = add_bytes(&node, data).await.expect("add 1");
        let r2 = add_bytes(&node, data).await.expect("add 2");
        assert_eq!(r1.hash, r2.hash);

        node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn different_content_produces_different_hash() {
        let (node, _tmp) = make_node().await;

        let r1 = add_bytes(&node, b"content A").await.expect("add 1");
        let r2 = add_bytes(&node, b"content B").await.expect("add 2");
        assert_ne!(r1.hash, r2.hash);

        node.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn parse_hash_validates_format() {
        // Valid
        let valid = "a".repeat(64);
        assert!(parse_hash(&valid).is_ok());

        // Too short
        assert!(matches!(
            parse_hash("abcd"),
            Err(ContentError::InvalidHash(_))
        ));

        // Invalid hex chars
        assert!(matches!(
            parse_hash(&"g".repeat(64)),
            Err(ContentError::InvalidHash(_))
        ));
    }

    #[tokio::test]
    async fn info_returns_correct_status() {
        let (node, _tmp) = make_node().await;

        let data = b"info test";
        let result = add_bytes(&node, data).await.expect("add");

        let content_info = info(&node, &result.hash).await.expect("info");
        assert!(content_info.is_local);
        assert_eq!(content_info.hash, result.hash);

        node.shutdown().await.expect("shutdown");
    }
}
