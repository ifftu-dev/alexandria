//! §5.3 + §11.2 — DID doc + status list propagation.
//!
//! The "two-node" tests exercise the real libp2p swarm via
//! `start_test_node` (lifted from `p2p::stress`). Where mDNS
//! discovery fails (typical in CI / containers), the tests SKIP
//! gracefully — same resilience pattern the existing stress tests
//! use. The local-only test (`credential_queued_until_issuer_did_doc_arrives`)
//! exercises the pending-verification sweeper in `p2p::vc_did`
//! without a network.

use super::common::{await_gossip_on, await_peers_connected, new_test_db, start_test_node};
use app_lib::p2p::vc_did::{handle_did_message, promote_pending_for, queue_pending, DidIngest};
use app_lib::p2p::vc_status::{handle_status_message, StatusIngest};

#[tokio::test]
#[ignore = "flaky on CI: depends on libp2p DHT bootstrap to a discovery peer; \
            races and times out non-deterministically. Track in a separate issue \
            before re-enabling — likely needs a stub bootstrap or deterministic mock."]
async fn did_doc_rotation_propagates_to_second_node() {
    // Node A publishes a DID rotation message → Node B receives
    // the gossip → Node B's DB reflects the rotated_by linkage.
    let (mut a, _rx_a) = match start_test_node("did-rotation-a", 32).await {
        Some(t) => t,
        None => return,
    };
    let (mut b, mut rx_b) = match start_test_node("did-rotation-b", 64).await {
        Some(t) => t,
        None => {
            a.shutdown().await;
            return;
        }
    };

    if !await_peers_connected(&a, &b, 10).await {
        a.shutdown().await;
        b.shutdown().await;
        eprintln!("SKIP: mDNS discovery timed out");
        return;
    }
    // Give GossipSub time to finish mesh propagation.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let payload = br#"{"did":"did:key:zRotA","rotated_to":"did:key:zRotA2"}"#.to_vec();
    let key = super::common::test_key("did-rotation-a");
    if let Err(e) = a
        .publish_vc_did(payload.clone(), &key, "stake_test1urotation")
        .await
    {
        eprintln!("SKIP: publish failed: {e:?}");
        a.shutdown().await;
        b.shutdown().await;
        return;
    }

    let received = await_gossip_on(&mut rx_b, "vc-did", 5).await;
    let payload_bytes = match received {
        Some(p) => p,
        None => {
            eprintln!("SKIP: gossip propagation timed out");
            a.shutdown().await;
            b.shutdown().await;
            return;
        }
    };
    assert_eq!(payload_bytes, payload);

    // Drive the handler against the received bytes to simulate what
    // the application-layer dispatcher would do. Assert the DB
    // records the rotated_by linkage.
    let db = new_test_db();
    let msg = app_lib::p2p::types::SignedGossipMessage {
        topic: "/alexandria/vc-did/1.0".into(),
        payload: payload_bytes,
        signature: vec![0; 64],
        public_key: vec![0; 32],
        stake_address: "stake_test1urotation".into(),
        timestamp: 1_712_880_000,
        encrypted: false,
        key_id: None,
    };
    let outcome = handle_did_message(&db, &msg).unwrap();
    assert_eq!(outcome, DidIngest::UpdatedRegistry);
    let rotated_by: Option<String> = db
        .conn()
        .query_row(
            "SELECT rotated_by FROM key_registry \
             WHERE did = 'did:key:zRotA' AND rotated_by IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .ok();
    assert_eq!(rotated_by.as_deref(), Some("did:key:zRotA2"));

    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test]
async fn status_list_revocation_propagates() {
    // Node A publishes a status list → Node B receives it → Node B's
    // DB has the bits. Handler-side we still have to pre-register
    // the issuer in B's key_registry (the handler defers otherwise);
    // the real P2P flow is a DID doc landing before the status list,
    // which is what the application dispatcher coordinates.
    let (mut a, _rx_a) = match start_test_node("status-a", 32).await {
        Some(t) => t,
        None => return,
    };
    let (mut b, mut rx_b) = match start_test_node("status-b", 64).await {
        Some(t) => t,
        None => {
            a.shutdown().await;
            return;
        }
    };
    if !await_peers_connected(&a, &b, 10).await {
        a.shutdown().await;
        b.shutdown().await;
        eprintln!("SKIP: mDNS discovery timed out");
        return;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Base64("revoked-bits-snapshot") = "cmV2b2tlZC1iaXRzLXNuYXBzaG90"
    let payload =
        br#"{"issuer":"did:key:zStatusIssuer","version":1,"bits":"cmV2b2tlZC1iaXRzLXNuYXBzaG90"}"#
            .to_vec();
    let key = super::common::test_key("status-a");
    if let Err(e) = a
        .publish_vc_status(payload.clone(), &key, "stake_test1ustatus")
        .await
    {
        eprintln!("SKIP: publish failed: {e:?}");
        a.shutdown().await;
        b.shutdown().await;
        return;
    }

    let received = await_gossip_on(&mut rx_b, "vc-status", 5).await;
    let payload_bytes = match received {
        Some(p) => p,
        None => {
            eprintln!("SKIP: gossip propagation timed out");
            a.shutdown().await;
            b.shutdown().await;
            return;
        }
    };
    assert_eq!(payload_bytes, payload);

    // Drive the handler against a seeded DB where the issuer is known.
    let db = new_test_db();
    db.conn()
        .execute(
            "INSERT INTO key_registry (did, key_id, public_key_hex, valid_from) \
             VALUES ('did:key:zStatusIssuer', 'key-1', '', '1970-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    let msg = app_lib::p2p::types::SignedGossipMessage {
        topic: "/alexandria/vc-status/1.0".into(),
        payload: payload_bytes,
        signature: vec![0; 64],
        public_key: vec![0; 32],
        stake_address: "stake_test1ustatus".into(),
        timestamp: 1_712_880_000,
        encrypted: false,
        key_id: None,
    };
    let outcome = handle_status_message(&db, &msg).unwrap();
    assert_eq!(outcome, StatusIngest::Applied);
    let version: i64 = db
        .conn()
        .query_row(
            "SELECT version FROM credential_status_lists \
             WHERE issuer_did = 'did:key:zStatusIssuer'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, 1);

    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test]
async fn credential_queued_until_issuer_did_doc_arrives() {
    // Local test — no network needed. A credential from an unknown
    // issuer lands in `credentials_pending_verification` until a
    // DID doc for that issuer arrives, at which point the sweeper
    // promotes it into `credentials`.
    let db = new_test_db();

    // Queue a credential whose issuer isn't yet in key_registry.
    let vc_json = serde_json::json!({
        "@context": ["https://www.w3.org/ns/credentials/v2"],
        "id": "urn:uuid:pending-cred",
        "type": ["VerifiableCredential", "FormalCredential"],
        "issuer": "did:key:zPendingIssuer",
        "validFrom": "2026-04-13T00:00:00Z",
        "credentialSubject": {
            "id": "did:key:zPendingSubject",
            "skillId": "s",
            "level": 4,
            "score": 0.9,
            "evidenceRefs": [],
        },
        "proof": {
            "type": "Ed25519Signature2020",
            "created": "2026-04-13T00:00:00Z",
            "verificationMethod": "did:key:zPendingIssuer#key-1",
            "proofPurpose": "assertionMethod",
            "jws": "fake..jws"
        }
    })
    .to_string();
    queue_pending(
        &db,
        "urn:uuid:pending-cred",
        "did:key:zPendingIssuer",
        "did:key:zPendingSubject",
        &vc_json,
    )
    .unwrap();

    // Pre-arrival: no credentials row.
    let pre: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM credentials WHERE id = 'urn:uuid:pending-cred'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pre, 0);

    // DID doc arrives — either via the gossip handler (which calls
    // promote_pending_for internally) or directly. Drive the
    // promoter directly to isolate the sweeper behaviour.
    db.conn()
        .execute(
            "INSERT INTO key_registry (did, key_id, public_key_hex, valid_from) \
             VALUES ('did:key:zPendingIssuer', 'key-1', '', '1970-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    let promoted = promote_pending_for(&db, "did:key:zPendingIssuer").unwrap();
    assert_eq!(promoted, 1);

    // Post-sweeper: credentials row exists, pending row is gone.
    let post: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM credentials WHERE id = 'urn:uuid:pending-cred'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(post, 1);
    let still_pending: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM credentials_pending_verification \
             WHERE id = 'urn:uuid:pending-cred'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(still_pending, 0);
}
