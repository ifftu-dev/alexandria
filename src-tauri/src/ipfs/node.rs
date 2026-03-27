//! Embedded iroh node lifecycle.
//!
//! Manages the iroh QUIC endpoint, blob store, and protocol router.
//! The node starts at app launch and provides content-addressed storage
//! via BLAKE3 hashing. Content is persisted to disk via `FsStore`.
//!
//! The router also registers gossip and MoQ (Media over QUIC) ALPNs
//! so the same QUIC endpoint can be shared with live tutoring.
//!
//! Architecture:
//!   Endpoint (QUIC + relay) → BlobsProtocol (iroh-blobs)
//!                            → Gossip (iroh-gossip, for room peer discovery)
//!                            → MoQ (iroh-moq, for media streaming)
//!                            → Router (accept loop)
//!                                    ↕
//!                              FsStore (redb on disk)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::QuicTransportConfig;
use iroh::protocol::Router;
use iroh::{Endpoint, SecretKey};
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::BlobsProtocol;
use iroh_gossip::Gossip;
use iroh_live::Live;
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
/// Wraps the router (which owns the endpoint), the blob store,
/// and the gossip + live instances needed for tutoring.
struct RunningNode {
    router: Router,
    store: FsStore,
    gossip: Gossip,
    live: Live,
}

/// The embedded iroh content node.
///
/// Manages the lifecycle of the iroh QUIC endpoint and blob store.
/// Thread-safe via `Arc<Mutex<>>` — intended to be stored in `AppState`.
pub struct ContentNode {
    inner: Arc<Mutex<Option<RunningNode>>>,
    data_dir: PathBuf,
    /// AES-256-GCM key for transparent content encryption.
    /// Set after vault unlock via `set_content_key()`.
    content_key: Arc<Mutex<Option<[u8; 32]>>>,
}

impl ContentNode {
    /// Create a new content node that will store data in `data_dir`.
    ///
    /// Does NOT start the node — call `start()` to begin.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            data_dir: data_dir.to_path_buf(),
            content_key: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the content encryption key (derived from vault password).
    pub async fn set_content_key(&self, key: [u8; 32]) {
        *self.content_key.lock().await = Some(key);
    }

    /// Get the content encryption key, if set.
    pub async fn content_key(&self) -> Option<[u8; 32]> {
        *self.content_key.lock().await
    }

    /// Start the iroh node.
    ///
    /// Creates the FsStore, binds the QUIC endpoint with a persistent
    /// identity key, registers the blobs protocol, and starts the
    /// accept loop.
    ///
    /// If `node_enc_key` is provided, the node's Ed25519 secret key is
    /// encrypted at rest using AES-256-GCM. Legacy plaintext keys are
    /// auto-migrated on first start with an encryption key.
    pub async fn start(&self, node_enc_key: Option<&[u8; 32]>) -> Result<(), NodeError> {
        let mut inner = self.inner.lock().await;
        if inner.is_some() {
            return Err(NodeError::AlreadyRunning);
        }

        // Create persistent blob store
        crate::diag::log(&format!(
            "node.start: FsStore::load at {}...",
            self.data_dir.display()
        ));
        let store = FsStore::load(&self.data_dir)
            .await
            .map_err(|e| NodeError::StoreInit(e.to_string()))?;
        crate::diag::log("node.start: FsStore loaded OK");

        log::info!("iroh blob store loaded at {}", self.data_dir.display());

        // Load or generate the node's persistent identity key
        let secret_key = load_or_generate_secret_key(&self.data_dir, node_enc_key)?;

        // QUIC transport: aggressive timeouts for real-time media.
        // keep_alive=2s ensures the connection stays active during audio DTX gaps.
        // idle_timeout=10s forces fast teardown of zombie connections — critical
        // because a 120s timeout causes the mobile's MoQ publisher to deadlock on
        // socket backpressure, preventing it from serving new subscriptions.
        let transport_config = QuicTransportConfig::builder()
            .keep_alive_interval(Duration::from_secs(2))
            .max_idle_timeout(Some(
                Duration::from_secs(10)
                    .try_into()
                    .expect("10s fits IdleTimeout"),
            ))
            .build();

        // Create QUIC endpoint with persistent identity
        crate::diag::log("node.start: Endpoint::bind()...");
        let endpoint = Endpoint::builder()
            .secret_key(secret_key)
            .transport_config(transport_config)
            .bind()
            .await
            .map_err(|e| NodeError::EndpointBind(e.to_string()))?;

        let node_id = endpoint.id();
        crate::diag::log(&format!(
            "node.start: endpoint bound, node_id={}",
            &node_id.to_string()[..12]
        ));
        log::info!("iroh endpoint bound, node ID: {node_id}");

        // Register protocols on the shared router.
        // All platforms: blobs + gossip (room peer discovery) + MoQ (media streaming)
        // Desktop: full video + audio via iroh-live with ffmpeg
        // Mobile: audio-only via iroh-live without ffmpeg (pure Opus codec)
        let blobs = BlobsProtocol::new(&store, None);
        let gossip = Gossip::builder().spawn(endpoint.clone());
        let live = Live::new(endpoint.clone());

        crate::diag::log("node.start: Router::builder().spawn()...");
        let router = Router::builder(endpoint)
            .accept(iroh_blobs::ALPN, blobs)
            .accept(iroh_gossip::ALPN, gossip.clone())
            .accept(iroh_live::ALPN, live.protocol_handler())
            .spawn();
        crate::diag::log("node.start: router spawned OK");

        log::info!("iroh router started, accepting blobs + gossip + moq connections");

        *inner = Some(RunningNode {
            router,
            store,
            gossip,
            live,
        });
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
        inner.as_ref().map(|n| n.router.endpoint().id().to_string())
    }

    /// Get a clone of the running Endpoint for use by other protocols.
    ///
    /// Returns `None` if the node is not running.
    pub async fn endpoint(&self) -> Option<Endpoint> {
        let inner = self.inner.lock().await;
        inner.as_ref().map(|n| n.router.endpoint().clone())
    }

    /// Get a clone of the Gossip instance for tutoring room peer discovery.
    ///
    /// Returns `None` if the node is not running.
    /// Get a clone of the Gossip instance for room peer discovery.
    ///
    /// Returns `None` if the node is not running.
    pub async fn gossip(&self) -> Option<Gossip> {
        let inner = self.inner.lock().await;
        inner.as_ref().map(|n| n.gossip.clone())
    }

    /// Get a clone of the Live instance for MoQ media streaming.
    ///
    /// Returns `None` if the node is not running.
    /// Desktop: full video + audio; Mobile: audio-only (no ffmpeg).
    pub async fn live(&self) -> Option<Live> {
        let inner = self.inner.lock().await;
        inner.as_ref().map(|n| n.live.clone())
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

/// File format version bytes.
const KEY_VERSION_PLAINTEXT: u8 = 0x00;
const KEY_VERSION_AES_GCM: u8 = 0x01;

/// Load the node's secret key from disk, or generate a new one.
///
/// When `enc_key` is provided, the key file is encrypted at rest using
/// AES-256-GCM. The file format is:
///   `version(1) || nonce(12) || ciphertext(32 + 16 auth tag)`
///
/// Legacy plaintext files (32 raw bytes) are auto-migrated to encrypted
/// format on first read when an encryption key is available.
fn load_or_generate_secret_key(
    data_dir: &Path,
    enc_key: Option<&[u8; 32]>,
) -> Result<SecretKey, NodeError> {
    let key_path = data_dir.join(SECRET_KEY_FILE);

    if key_path.exists() {
        let bytes = std::fs::read(&key_path)
            .map_err(|e| NodeError::KeyPersistence(format!("read key: {e}")))?;

        let key_bytes = if bytes.len() == 32 {
            // Legacy plaintext format (version 0x00 implicit)
            let mut kb = [0u8; 32];
            kb.copy_from_slice(&bytes);

            // Auto-migrate to encrypted if we have an encryption key
            if let Some(ek) = enc_key {
                let encrypted = encrypt_node_key(&kb, ek)?;
                std::fs::write(&key_path, &encrypted)
                    .map_err(|e| NodeError::KeyPersistence(format!("migrate key: {e}")))?;
                log::info!("migrated node key to encrypted format");
            }

            kb
        } else if bytes.first() == Some(&KEY_VERSION_AES_GCM) && bytes.len() == 1 + 12 + 32 + 16 {
            // Encrypted format: version(1) || nonce(12) || ciphertext(48)
            let ek = enc_key.ok_or_else(|| {
                NodeError::KeyPersistence(
                    "node key is encrypted but no decryption key provided".into(),
                )
            })?;
            decrypt_node_key(&bytes[1..], ek)?
        } else {
            return Err(NodeError::KeyPersistence(format!(
                "key file has unexpected length: {} bytes",
                bytes.len()
            )));
        };

        let key = SecretKey::from_bytes(&key_bytes);
        log::info!("loaded existing iroh node key from {}", key_path.display());
        Ok(key)
    } else {
        // Generate 32 random bytes for the Ed25519 secret key.
        use rand::RngCore;
        let mut key_bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key_bytes);
        let key = SecretKey::from_bytes(&key_bytes);

        // Write encrypted if we have an encryption key, plaintext otherwise
        if let Some(ek) = enc_key {
            let encrypted = encrypt_node_key(&key_bytes, ek)?;
            std::fs::write(&key_path, &encrypted)
                .map_err(|e| NodeError::KeyPersistence(format!("write key: {e}")))?;
        } else {
            std::fs::write(&key_path, key.to_bytes())
                .map_err(|e| NodeError::KeyPersistence(format!("write key: {e}")))?;
        }

        log::info!(
            "generated new iroh node key, saved to {}",
            key_path.display()
        );
        Ok(key)
    }
}

/// Encrypt a 32-byte node key using AES-256-GCM.
/// Returns: `version(1) || nonce(12) || ciphertext(48)`
fn encrypt_node_key(key_bytes: &[u8; 32], enc_key: &[u8; 32]) -> Result<Vec<u8>, NodeError> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
    use rand::RngCore;

    let cipher = Aes256Gcm::new(enc_key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, key_bytes.as_ref())
        .map_err(|e| NodeError::KeyPersistence(format!("encrypt key: {e}")))?;

    let mut out = Vec::with_capacity(1 + 12 + ciphertext.len());
    out.push(KEY_VERSION_AES_GCM);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a node key from `nonce(12) || ciphertext(48)`.
fn decrypt_node_key(data: &[u8], enc_key: &[u8; 32]) -> Result<[u8; 32], NodeError> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

    if data.len() != 12 + 32 + 16 {
        return Err(NodeError::KeyPersistence(format!(
            "encrypted key has wrong length: {}",
            data.len()
        )));
    }

    let nonce = Nonce::from_slice(&data[..12]);
    let cipher = Aes256Gcm::new(enc_key.into());

    let plaintext = cipher
        .decrypt(nonce, &data[12..])
        .map_err(|_| NodeError::KeyPersistence("decryption failed (wrong password?)".into()))?;

    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&plaintext);
    Ok(key_bytes)
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
        node.start(None).await.expect("start failed");
        assert!(node.is_running().await);
        assert!(node.node_id().await.is_some());

        // Can't start twice
        assert!(matches!(
            node.start(None).await,
            Err(NodeError::AlreadyRunning)
        ));

        // Shutdown
        node.shutdown().await.expect("shutdown failed");
        assert!(!node.is_running().await);

        // Can't shutdown twice
        assert!(matches!(node.shutdown().await, Err(NodeError::NotRunning)));
    }

    #[tokio::test]
    async fn node_id_is_stable_across_restart() {
        let tmp = TempDir::new().expect("create temp dir");
        let node = ContentNode::new(tmp.path());

        // The secret key is persisted to disk, so the node ID should be
        // stable across restarts from the same data directory.
        node.start(None).await.expect("start failed");
        let id1 = node.node_id().await.unwrap();
        node.shutdown().await.expect("shutdown failed");

        node.start(None).await.expect("restart failed");
        let id2 = node.node_id().await.unwrap();
        node.shutdown().await.expect("shutdown failed");

        assert_eq!(id1, id2, "node ID should be stable across restarts");
    }

    #[test]
    fn secret_key_persists_to_disk() {
        let tmp = TempDir::new().expect("create temp dir");

        // First call generates and saves (plaintext, no encryption key)
        let key1 = load_or_generate_secret_key(tmp.path(), None).expect("gen key");

        // Second call loads from file
        let key2 = load_or_generate_secret_key(tmp.path(), None).expect("load key");

        assert_eq!(
            key1.to_bytes(),
            key2.to_bytes(),
            "key should persist across loads"
        );

        // File should exist
        assert!(tmp.path().join(SECRET_KEY_FILE).exists());
    }

    #[test]
    fn secret_key_encrypted_roundtrip() {
        let tmp = TempDir::new().expect("create temp dir");
        let enc_key = [42u8; 32];

        // Generate with encryption
        let key1 = load_or_generate_secret_key(tmp.path(), Some(&enc_key)).expect("gen key");

        // File should be encrypted (61 bytes: 1 + 12 + 32 + 16)
        let file = std::fs::read(tmp.path().join(SECRET_KEY_FILE)).expect("read file");
        assert_eq!(file.len(), 61, "encrypted key file should be 61 bytes");
        assert_eq!(file[0], KEY_VERSION_AES_GCM);

        // Load with same key
        let key2 = load_or_generate_secret_key(tmp.path(), Some(&enc_key)).expect("load key");
        assert_eq!(key1.to_bytes(), key2.to_bytes());

        // Load with wrong key should fail
        let wrong_key = [99u8; 32];
        assert!(load_or_generate_secret_key(tmp.path(), Some(&wrong_key)).is_err());
    }

    #[test]
    fn plaintext_key_auto_migrates_to_encrypted() {
        let tmp = TempDir::new().expect("create temp dir");

        // Create plaintext key
        let key1 = load_or_generate_secret_key(tmp.path(), None).expect("gen key");
        let file = std::fs::read(tmp.path().join(SECRET_KEY_FILE)).expect("read");
        assert_eq!(file.len(), 32, "should be plaintext");

        // Load with encryption key — should auto-migrate
        let enc_key = [42u8; 32];
        let key2 = load_or_generate_secret_key(tmp.path(), Some(&enc_key)).expect("migrate");
        assert_eq!(key1.to_bytes(), key2.to_bytes());

        // File should now be encrypted
        let file = std::fs::read(tmp.path().join(SECRET_KEY_FILE)).expect("read");
        assert_eq!(file.len(), 61, "should be encrypted after migration");
    }
}
