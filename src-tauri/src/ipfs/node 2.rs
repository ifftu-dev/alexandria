//! Embedded iroh node lifecycle.
//!
//! Manages the iroh QUIC endpoint, blob store, and protocol router.
//! The node starts at app launch and provides content-addressed storage
//! via BLAKE3 hashing. Content is persisted to disk via `FsStore`.
//!
//! Architecture:
//!   Endpoint (QUIC + relay) → BlobsProtocol (iroh-blobs) → Router (accept loop)
//!                                    ↕
//!                              FsStore (redb on disk)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use iroh::protocol::Router;
use iroh::{Endpoint, SecretKey};
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::BlobsProtocol;
use thiserror::Error;
use tokio::sync::Mutex;

/// Name of the file where the node's Ed25519 secret key is persisted.
const SECRET_KEY_FILE: &str = "node_secret.key";

#[derive(Error, Debug)]
pub enum NodeError {
    #[error("failed to create blob store: {0}")]
    StoreInit(String),
    #[error("failed to bind endpoint: {0}")]
    EndpointBind(String),
    #[error("node is not running")]
    NotRunning,
    #[error("node is already running")]
    AlreadyRunning,
    #[error("shutdown failed: {0}")]
    Shutdown(String),
    #[error("key persistence failed: {0}")]
    KeyPersistence(String),
}

/// Handle to the running iroh node.
///
/// Wraps the router (which owns the endpoint) and the blob store.
/// The store is exposed for direct content operations via `content.rs`.
struct RunningNode {
    router: Router,
    store: FsStore,
}

/// The embedded iroh content node.
///
/// Manages the lifecycle of the iroh QUIC endpoint and blob store.
/// Thread-safe via `Arc<Mutex<>>` — intended to be stored in `AppState`.
pub struct ContentNode {
    inner: Arc<Mutex<Option<RunningNode>>>,
    data_dir: PathBuf,
}

impl ContentNode {
    /// Create a new content node that will store data in `data_dir`.
    ///
    /// Does NOT start the node — call `start()` to begin.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            data_dir: data_dir.to_path_buf(),
        }
    }

    /// Start the iroh node.
    ///
    /// Creates the FsStore, binds the QUIC endpoint with a persistent
    /// identity key, registers the blobs protocol, and starts the
    /// accept loop.
    pub async fn start(&self) -> Result<(), NodeError> {
        let mut inner = self.inner.lock().await;
        if inner.is_some() {
            return Err(NodeError::AlreadyRunning);
        }

        // Create persistent blob store
        let store = FsStore::load(&self.data_dir)
            .await
            .map_err(|e| NodeError::StoreInit(e.to_string()))?;

        log::info!("iroh blob store loaded at {}", self.data_dir.display());

        // Load or generate the node's persistent identity key
        let secret_key = load_or_generate_secret_key(&self.data_dir)?;

        // Create QUIC endpoint with persistent identity
        let endpoint = Endpoint::builder()
            .secret_key(secret_key)
            .bind()
            .await
            .map_err(|e| NodeError::EndpointBind(e.to_string()))?;

        let node_id = endpoint.id();
        log::info!("iroh endpoint bound, node ID: {node_id}");

        // Register blobs protocol and start accept loop
        let blobs = BlobsProtocol::new(&store, None);
        let router = Router::builder(endpoint)
            .accept(iroh_blobs::ALPN, blobs)
            .spawn();

        log::info!("iroh router started, accepting connections");

        *inner = Some(RunningNode { router, store });
        Ok(())
    }

    /// Shut down the iroh node gracefully.
    ///
    /// Stops the accept loop, closes connections, and flushes the store.
    pub async fn shutdown(&self) -> Result<(), NodeError> {
        let mut inner = self.inner.lock().await;
        let node = inner.take().ok_or(NodeError::NotRunning)?;

        log::info!("shutting down iroh node...");
        node.router
            .shutdown()
            .await
            .map_err(|e| NodeError::Shutdown(e.to_string()))?;

        log::info!("iroh node shut down");
        Ok(())
    }

    /// Check if the node is currently running.
    pub async fn is_running(&self) -> bool {
        let inner = self.inner.lock().await;
        inner.is_some()
    }

    /// Get the node's public key (peer ID) as a hex string.
    ///
    /// Returns `None` if the node is not running.
    pub async fn node_id(&self) -> Option<String> {
        let inner = self.inner.lock().await;
        inner
            .as_ref()
            .map(|n| n.router.endpoint().id().to_string())
    }

    /// Access the blob store for content operations.
    ///
    /// The returned guard holds the mutex — drop it promptly.
    /// For content operations, prefer using the methods in `content.rs`
    /// which handle locking internally.
    pub(crate) async fn store(
        &self,
    ) -> Result<impl std::ops::Deref<Target = FsStore> + '_, NodeError> {
        let guard = self.inner.lock().await;
        if guard.is_none() {
            return Err(NodeError::NotRunning);
        }
        Ok(StoreGuard(guard))
    }
}

/// RAII guard that provides access to the FsStore through the mutex.
struct StoreGuard<'a>(tokio::sync::MutexGuard<'a, Option<RunningNode>>);

impl std::ops::Deref for StoreGuard<'_> {
    type Target = FsStore;

    fn deref(&self) -> &Self::Target {
        // Safe: we only construct StoreGuard when inner.is_some()
        &self.0.as_ref().unwrap().store
    }
}

/// Load the node's secret key from disk, or generate a new one.
///
/// The key is stored as raw 32 bytes in `data_dir/node_secret.key`.
/// This ensures the node has a stable identity across restarts.
fn load_or_generate_secret_key(data_dir: &Path) -> Result<SecretKey, NodeError> {
    let key_path = data_dir.join(SECRET_KEY_FILE);

    if key_path.exists() {
        let bytes = std::fs::read(&key_path)
            .map_err(|e| NodeError::KeyPersistence(format!("read key: {e}")))?;
        if bytes.len() != 32 {
            return Err(NodeError::KeyPersistence(format!(
                "key file has wrong length: {} (expected 32)",
                bytes.len()
            )));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&bytes);
        let key = SecretKey::from_bytes(&key_bytes);
        log::info!("loaded existing iroh node key from {}", key_path.display());
        Ok(key)
    } else {
        // Generate 32 random bytes for the Ed25519 secret key.
        // We use rand 0.8's OsRng via RngCore::fill_bytes, then construct
        // the iroh SecretKey from raw bytes to avoid rand version conflicts
        // (iroh uses rand 0.9 internally, we have rand 0.8 for other deps).
        use rand::RngCore;
        let mut key_bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key_bytes);
        let key = SecretKey::from_bytes(&key_bytes);
        std::fs::write(&key_path, key.to_bytes())
            .map_err(|e| NodeError::KeyPersistence(format!("write key: {e}")))?;
        log::info!(
            "generated new iroh node key, saved to {}",
            key_path.display()
        );
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn node_lifecycle() {
        let tmp = TempDir::new().expect("create temp dir");
        let node = ContentNode::new(tmp.path());

        // Not running initially
        assert!(!node.is_running().await);
        assert!(node.node_id().await.is_none());

        // Start
        node.start().await.expect("start failed");
        assert!(node.is_running().await);
        assert!(node.node_id().await.is_some());

        // Can't start twice
        assert!(matches!(
            node.start().await,
            Err(NodeError::AlreadyRunning)
        ));

        // Shutdown
        node.shutdown().await.expect("shutdown failed");
        assert!(!node.is_running().await);

        // Can't shutdown twice
        assert!(matches!(
            node.shutdown().await,
            Err(NodeError::NotRunning)
        ));
    }

    #[tokio::test]
    async fn node_id_is_stable_across_restart() {
        let tmp = TempDir::new().expect("create temp dir");
        let node = ContentNode::new(tmp.path());

        // The secret key is persisted to disk, so the node ID should be
        // stable across restarts from the same data directory.
        node.start().await.expect("start failed");
        let id1 = node.node_id().await.unwrap();
        node.shutdown().await.expect("shutdown failed");

        node.start().await.expect("restart failed");
        let id2 = node.node_id().await.unwrap();
        node.shutdown().await.expect("shutdown failed");

        assert_eq!(id1, id2, "node ID should be stable across restarts");
    }

    #[test]
    fn secret_key_persists_to_disk() {
        let tmp = TempDir::new().expect("create temp dir");

        // First call generates and saves
        let key1 = load_or_generate_secret_key(tmp.path()).expect("gen key");

        // Second call loads from file
        let key2 = load_or_generate_secret_key(tmp.path()).expect("load key");

        assert_eq!(
            key1.to_bytes(),
            key2.to_bytes(),
            "key should persist across loads"
        );

        // File should exist
        assert!(tmp.path().join(SECRET_KEY_FILE).exists());
    }
}
