//! The iroh P2P storage layer: content survives its origin going offline
//! because a pinner replicated it and can serve it to a third node — all over
//! iroh, with no external URL origin involved.
//!
//! This is the acceptance test for the "iroh as a real storage layer" work.
//! Before it, the iroh store was a local cache only: a node that lacked a blob
//! fetched it over HTTP from the Blockfrost/external URL origin, and nothing
//! honored PinBoard commitments. `content_store::fetch` adds true peer blob transfer;
//! this test proves the survival property end to end:
//!
//!   1. A stores a blob (hash H).
//!   2. Pinner P fetches H from A over iroh (replication).
//!   3. A shuts down (origin offline).
//!   4. B fetches H from P over iroh — A is gone, no gateway is used.
//!   5. B's bytes equal the original.
//!
//! Nodes talk over loopback: `endpoint.addr()` carries direct socket addresses,
//! so `connect` establishes a direct QUIC path without depending on relays.

use app_lib::content_store::content::{self, parse_hash};
use app_lib::content_store::fetch;
use app_lib::content_store::node::ContentNode;
use tempfile::TempDir;

/// Start a fresh unencrypted content node in its own temp dir.
async fn start_node() -> (ContentNode, TempDir) {
    let tmp = TempDir::new().expect("temp dir");
    let node = ContentNode::new(tmp.path());
    node.start(None).await.expect("start node");
    (node, tmp)
}

#[tokio::test]
async fn blob_survives_origin_offline_via_pinner() {
    // ── 1. A stores content ────────────────────────────────────────────────
    let (node_a, _tmp_a) = start_node().await;
    let data = b"alexandria: content-addressed, peer-served, gateway-free".to_vec();
    let added = content::add_bytes(&node_a, &data).await.expect("A add");
    let hash = parse_hash(&added.hash).expect("valid hash");

    let addr_a = node_a
        .endpoint_addr()
        .await
        .expect("A has an address once running");

    // ── 2. Pinner P replicates H from A over iroh ──────────────────────────
    let (pinner, _tmp_p) = start_node().await;
    fetch::fetch_from_peer(&pinner, addr_a, hash)
        .await
        .expect("P fetches H from A");
    assert!(
        content::has(&pinner, &added.hash)
            .await
            .expect("P has-check"),
        "pinner must hold the blob after replication"
    );
    let addr_p = pinner.endpoint_addr().await.expect("P address");

    // ── 3. Origin A goes offline ───────────────────────────────────────────
    node_a.shutdown().await.expect("A shutdown");

    // ── 4. B fetches H from the pinner (A is gone; no gateway) ─────────────
    let (node_b, _tmp_b) = start_node().await;
    assert!(
        !content::has(&node_b, &added.hash)
            .await
            .expect("B has-check"),
        "B must start without the blob"
    );
    let served_by = fetch::fetch_from_any(&node_b, std::slice::from_ref(&addr_p), hash)
        .await
        .expect("B fetches H from the pinner while A is offline");

    // ── 5. B's bytes match the original ────────────────────────────────────
    let got = content::get_bytes(&node_b, &added.hash)
        .await
        .expect("B reads the fetched blob");
    assert_eq!(got, data, "peer-fetched bytes must equal the original");
    assert_eq!(
        served_by.id, addr_p.id,
        "the pinner (not the offline origin) served the blob"
    );

    node_b.shutdown().await.expect("B shutdown");
    pinner.shutdown().await.expect("P shutdown");
}
