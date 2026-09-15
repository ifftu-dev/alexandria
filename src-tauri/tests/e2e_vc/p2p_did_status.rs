//! §5.3 + §11.2 — DID doc + status list propagation.
//!
//! Two-node tests connect real loopback swarms directly and fail on missed
//! delivery. The local-only test (`credential_queued_until_issuer_did_doc_arrives`)
//! exercises the pending-verification sweeper in `p2p::vc_did`
//! without a network.

use super::common::{
    await_gossip_on, await_peers_connected, discard_events, new_test_db, publish_until_ready,
    start_test_node,
};
use app_lib::p2p::vc_did::{handle_did_message, promote_pending_for, queue_pending, DidIngest};
use app_lib::p2p::vc_status::{handle_status_message, StatusIngest};
use base64::Engine as _;
use ed25519_dalek::Signer as _;

#[tokio::test]
async fn did_doc_rotation_propagates_to_second_node() {
    // Node A publishes a DID rotation message → Node B receives
    // the gossip → Node B's DB reflects the rotated_by linkage.
    let (mut a, rx_a) = start_test_node("did-rotation-a", 32).await;
    let _events_a = discard_events(rx_a);
    let (mut b, mut rx_b) = start_test_node("did-rotation-b", 64).await;
    await_peers_connected(&a, &b, 10).await;
    let key = super::common::test_key("did-rotation-a");
    let did = alexandria_verify::did::derive_did_key(&key);
    let successor = super::common::test_did("did-rotation-successor");
    let announcement = serde_json::to_vec(&serde_json::json!({"did": did.as_str()})).unwrap();
    publish_until_ready(|| a.publish_vc_did(announcement.clone(), &key, "stake_test1urotation"))
        .await;
    let announcement = await_gossip_on(&mut rx_b, "vc-did", 5).await;

    // Drive the handler against the received bytes to simulate what
    // the application-layer dispatcher would do. Assert the DB
    // records the rotated_by linkage.
    let db = new_test_db();
    assert_eq!(
        handle_did_message(&db, &announcement).unwrap(),
        DidIngest::Stored
    );
    let payload = serde_json::to_vec(
        &serde_json::json!({"did": did.as_str(), "rotated_to": successor.as_str()}),
    )
    .unwrap();
    publish_until_ready(|| a.publish_vc_did(payload.clone(), &key, "stake_test1urotation")).await;
    let msg = await_gossip_on(&mut rx_b, "vc-did", 5).await;
    assert_eq!(msg.payload, payload);
    let outcome = handle_did_message(&db, &msg).unwrap();
    assert_eq!(outcome, DidIngest::UpdatedRegistry);
    let rotated_by: Option<String> = db
        .conn()
        .query_row(
            "SELECT rotated_by FROM key_registry \
             WHERE did = ?1 AND rotated_by IS NOT NULL",
            [did.as_str()],
            |r| r.get(0),
        )
        .ok();
    assert_eq!(rotated_by.as_deref(), Some(successor.as_str()));

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
    let (mut a, rx_a) = start_test_node("status-a", 32).await;
    let _events_a = discard_events(rx_a);
    let (mut b, mut rx_b) = start_test_node("status-b", 64).await;
    await_peers_connected(&a, &b, 10).await;

    // Signed by the issuer whose DID the payload names, because that is the
    // only kind of status list the handler accepts.
    //
    // This used to be a literal with `"issuer":"did:key:zStatusIssuer"` — not a
    // resolvable did:key — and no proof at all, asserting `Applied`. That is
    // the behaviour the VC gossip layer had before it was authenticated: any
    // peer could publish any issuer's status list, and zeroing the bits
    // mass-*un*-revoked. The assertion outlived the fix, and survived only
    // because the test skips out above whenever mDNS does not connect. Run it
    // where discovery works and it failed on the fix that made it safe.
    let key = super::common::test_key("status-a");
    let issuer = alexandria_verify::did::derive_did_key(&key);
    let bits_b64 = "cmV2b2tlZC1iaXRzLXNuYXBzaG90"; // Base64("revoked-bits-snapshot")
    let bits = base64::engine::general_purpose::STANDARD
        .decode(bits_b64)
        .expect("bits are valid base64");
    let list_id = format!("urn:alexandria:status-list:{}:1", issuer.as_str());
    let proof = base64::engine::general_purpose::STANDARD.encode(
        key.sign(&app_lib::p2p::vc_status::canonical_status_bytes(
            &list_id,
            issuer.as_str(),
            1,
            &bits,
        ))
        .to_bytes(),
    );
    let payload = serde_json::to_vec(&serde_json::json!({
        "issuer": issuer.as_str(),
        "version": 1,
        "bits": bits_b64,
        "proof": proof,
    }))
    .expect("payload serialises");
    publish_until_ready(|| a.publish_vc_status(payload.clone(), &key, "stake_test1ustatus")).await;
    let msg = await_gossip_on(&mut rx_b, "vc-status", 5).await;
    assert_eq!(msg.payload, payload);

    // Drive the handler against a seeded DB where the issuer is known.
    let db = new_test_db();
    db.conn()
        .execute(
            "INSERT INTO key_registry (did, key_id, public_key_hex, valid_from) \
             VALUES (?1, 'key-1', '', '1970-01-01T00:00:00Z')",
            rusqlite::params![issuer.as_str()],
        )
        .unwrap();
    let outcome = handle_status_message(&db, &msg).unwrap();
    assert_eq!(outcome, StatusIngest::Applied);
    let version: i64 = db
        .conn()
        .query_row(
            "SELECT version FROM credential_status_lists WHERE issuer_did = ?1",
            rusqlite::params![issuer.as_str()],
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
