//! Shared fixtures for VC scenarios and directly connected loopback swarms.

use app_lib::crypto::did::Did;
use app_lib::db::Database;
use ed25519_dalek::SigningKey;
use std::path::PathBuf;

/// A deterministic signing key per role for reproducible test fixtures.
pub fn test_key(role: &str) -> SigningKey {
    let mut bytes = [0u8; 32];
    let b = role.as_bytes();
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = b[i % b.len().max(1)];
    }
    SigningKey::from_bytes(&bytes)
}

/// Spin up a fresh database in a unique temp path with all migrations
/// applied. Integration tests don't see `#[cfg(test)]` items in the
/// library (`Database::open_in_memory`), so we use a tempfile.
pub fn new_test_db() -> Database {
    let dir = std::env::temp_dir().join(format!("alexandria-vc-e2e-{}", uuid_like()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path: PathBuf = dir.join("test.db");
    let db = Database::open(&path).expect("open db");
    db.run_migrations().expect("apply migrations");
    db
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    // Tests run in parallel inside one process — same nanosecond +
    // same PID can collide. Bumping a process-global counter
    // guarantees uniqueness without needing a real UUID dep here.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{nanos}-{}-{seq}", std::process::id())
}

/// Derive a `did:key` from a role-keyed signing key.
pub fn test_did(role: &str) -> Did {
    let key = test_key(role);
    app_lib::crypto::did::derive_did_key(&key)
}

/// Deterministic ISO-8601 timestamp for snapshot tests.
pub const TEST_NOW: &str = "2026-04-13T00:00:00Z";

// ---------------------------------------------------------------------------
// Real libp2p swarms, loopback listeners, explicit peers, bounded failures.
// ---------------------------------------------------------------------------

use app_lib::p2p::network::{
    derive_libp2p_keypair, start_loopback_node_with_db, NetworkError, P2pNode,
};
use app_lib::p2p::types::{P2pEvent, SignedGossipMessage};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

pub struct EventDrain(tokio::task::JoinHandle<()>);

impl Drop for EventDrain {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn discard_events(mut receiver: mpsc::Receiver<P2pEvent>) -> EventDrain {
    EventDrain(tokio::spawn(async move {
        while receiver.recv().await.is_some() {}
    }))
}

/// Start a real loopback node. Startup failures fail the test.
pub async fn start_test_node(role: &str, capacity: usize) -> (P2pNode, mpsc::Receiver<P2pEvent>) {
    start_fixture(role, capacity, None).await
}

/// Fixed device id for deterministic test PeerIds — real installs use a
/// random per-device value persisted via `p2p::device_id`.
const TEST_DEVICE_ID: [u8; 32] = [0xCCu8; 32];

/// Variant that wires a `Database` into the swarm event loop so the
/// node can answer inbound vc-fetch requests against local
/// credentials.
pub async fn start_test_node_with_db(
    role: &str,
    capacity: usize,
    db: app_lib::db::Database,
) -> (P2pNode, mpsc::Receiver<P2pEvent>) {
    start_fixture(role, capacity, Some(db)).await
}

async fn start_fixture(
    role: &str,
    capacity: usize,
    db: Option<Database>,
) -> (P2pNode, mpsc::Receiver<P2pEvent>) {
    let mut seed = [0u8; 32];
    let b = role.as_bytes();
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = b[i % b.len().max(1)];
    }
    let kp = derive_libp2p_keypair(&seed, &TEST_DEVICE_ID).expect("test keypair");
    let (tx, rx) = mpsc::channel::<P2pEvent>(capacity);
    let db_arc = db.map(|database| Arc::new(StdMutex::new(Some(database))));
    let node = timeout(
        Duration::from_secs(10),
        start_loopback_node_with_db(kp, tx, db_arc),
    )
    .await
    .expect("loopback node startup timed out")
    .expect("loopback node startup failed");
    (node, rx)
}

/// Dial the fixture's loopback listener and require mutual connection.
pub async fn await_peers_connected(a: &P2pNode, b: &P2pNode, timeout_s: u64) {
    let a_id = a.peer_id().to_string();
    let b_id = b.peer_id().to_string();
    timeout(Duration::from_secs(timeout_s), async {
        let address = loop {
            let status = b.status().await.expect("fixture status");
            assert!(
                status
                    .listening_addresses
                    .iter()
                    .all(|address| address.starts_with("/ip4/127.0.0.1/tcp/")),
                "fixture exposed a non-loopback listener"
            );
            if let Some(address) = status.listening_addresses.first() {
                break address.parse().expect("loopback multiaddress");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert!(a.connected_peers().await.expect("fixture peers").is_empty());
        assert!(b.connected_peers().await.expect("fixture peers").is_empty());
        a.connect_peer(*b.peer_id(), vec![address])
            .await
            .expect("direct loopback dial");
        loop {
            let peers_a = a.connected_peers().await.expect("fixture A peers");
            let peers_b = b.connected_peers().await.expect("fixture B peers");
            if peers_a.contains(&b_id) && peers_b.contains(&a_id) {
                assert_eq!(peers_a.len(), 1);
                assert_eq!(peers_b.len(), 1);
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("direct test-peer connection timed out");
}

pub async fn publish_until_ready<F, Fut>(mut publish: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), NetworkError>>,
{
    timeout(Duration::from_secs(10), async {
        loop {
            match publish().await {
                Ok(()) => return,
                Err(NetworkError::Publish(error)) if error.contains("InsufficientPeers") => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => panic!("gossip publication failed: {error}"),
            }
        }
    })
    .await
    .expect("gossip mesh did not become ready");
}

/// Drain the receiver until a `GossipMessage` arrives on the given
/// topic suffix. Timeout or channel closure fails the test. The original
/// signed envelope is retained for application-handler assertions.
pub async fn await_gossip_on(
    rx: &mut mpsc::Receiver<P2pEvent>,
    topic_suffix: &str,
    timeout_s: u64,
) -> SignedGossipMessage {
    timeout(Duration::from_secs(timeout_s), async {
        while let Some(event) = rx.recv().await {
            if let P2pEvent::GossipMessage { topic, message } = event {
                if topic.contains(topic_suffix) {
                    return Some(message);
                }
            }
        }
        None
    })
    .await
    .expect("gossip propagation timed out")
    .expect("gossip event channel closed")
}
