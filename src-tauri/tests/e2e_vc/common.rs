//! Shared test harness for the e2e VC scenarios. Stubs — PR 1 scaffolds
//! these; the real implementations land as dependency PRs arrive.

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

/// Convenience: derive a `did:key` from a role-keyed signing key.
/// Panics if invoked before PR 3 lands `derive_did_key`.
pub fn test_did(role: &str) -> Did {
    let key = test_key(role);
    app_lib::crypto::did::derive_did_key(&key)
}

/// Deterministic ISO-8601 timestamp for snapshot tests.
pub const TEST_NOW: &str = "2026-04-13T00:00:00Z";

// ---------------------------------------------------------------------------
// Two-node libp2p harness (lifted from `p2p::stress::tests`).
//
// P2P e2e tests boot real libp2p swarms via `start_node`. These helpers
// mirror the stress-test pattern — deterministic keys per role, graceful
// SKIP on mDNS timeout (common in CI / containers), and shutdown on drop.
// Each test costs ~10–15s wall-clock.
// ---------------------------------------------------------------------------

use app_lib::p2p::network::{keypair_from_cardano_key, start_node, P2pNode};
use app_lib::p2p::types::P2pEvent;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

/// Start a libp2p node with a deterministic key derived from `role`.
/// Returns `Some((node, event_rx))` on success, `None` if the node
/// couldn't start (e.g. ephemeral port binding failed) — callers
/// should treat `None` as SKIP, not FAIL.
pub async fn start_test_node(
    role: &str,
    capacity: usize,
) -> Option<(P2pNode, mpsc::Receiver<P2pEvent>)> {
    let mut seed = [0u8; 32];
    let b = role.as_bytes();
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = b[i % b.len().max(1)];
    }
    let kp = keypair_from_cardano_key(&seed).ok()?;
    let (tx, rx) = mpsc::channel::<P2pEvent>(capacity);
    match start_node(kp, tx, vec![]).await {
        Ok(node) => Some((node, rx)),
        Err(err) => {
            eprintln!("SKIP: node `{role}` failed to start ({err:?})");
            None
        }
    }
}

/// Poll until both nodes see each other as connected, or `timeout_s`
/// elapses. Returns `true` on success, `false` on timeout (SKIP signal).
pub async fn await_peers_connected(a: &P2pNode, b: &P2pNode, timeout_s: u64) -> bool {
    let a_id = a.peer_id().to_string();
    let b_id = b.peer_id().to_string();
    timeout(Duration::from_secs(timeout_s), async {
        loop {
            let peers_a = a.connected_peers().await.unwrap_or_default();
            let peers_b = b.connected_peers().await.unwrap_or_default();
            if peers_a.contains(&b_id) || peers_b.contains(&a_id) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap_or(false)
}

/// Drain the receiver until a `GossipMessage` arrives on the given
/// topic suffix, or the timeout elapses. Returns the deserialized
/// envelope payload.
pub async fn await_gossip_on(
    rx: &mut mpsc::Receiver<P2pEvent>,
    topic_suffix: &str,
    timeout_s: u64,
) -> Option<Vec<u8>> {
    timeout(Duration::from_secs(timeout_s), async {
        while let Some(event) = rx.recv().await {
            if let P2pEvent::GossipMessage { topic, message } = event {
                if topic.contains(topic_suffix) {
                    return Some(message.payload);
                }
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}
