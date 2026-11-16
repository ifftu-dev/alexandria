//! DID announcement + status list propagation (§11.2).
//!
//! Two-node tests connect real loopback swarms directly and fail on missed
//! delivery. The local-only test (`queued_credentials_are_promoted_only_when_they_verify`)
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
async fn did_announcement_propagates_and_records_nothing() {
    // Node A announces its DID → Node B receives the gossip. A did:key is its
    // key, so B records nothing in its key registry, and a payload that still
    // carries the retired `rotated_to` field is read as a plain announcement.
    let (mut a, rx_a) = start_test_node("did-announce-a", 32).await;
    let _events_a = discard_events(rx_a);
    let (mut b, mut rx_b) = start_test_node("did-announce-b", 64).await;
    await_peers_connected(&a, &b, 10).await;
    let key = super::common::test_key("did-announce-a");
    let did = alexandria_verify::did::derive_did_key(&key);
    let successor = super::common::test_did("did-announce-successor");

    let db = new_test_db();
    for payload in [
        serde_json::json!({"did": did.as_str()}),
        serde_json::json!({"did": did.as_str(), "rotated_to": successor.as_str()}),
    ] {
        let payload = serde_json::to_vec(&payload).unwrap();
        publish_until_ready(|| a.publish_vc_did(payload.clone(), &key, "stake_test1uannounce"))
            .await;
        let msg = await_gossip_on(&mut rx_b, "vc-did", 5).await;
        assert_eq!(msg.payload, payload);
        // Drive the handler against the received bytes, as the
        // application-layer dispatcher would.
        assert_eq!(
            handle_did_message(&db, &msg).unwrap(),
            DidIngest::Announced { promoted: 0 }
        );
    }
    let rows: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM key_registry", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);

    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test]
async fn status_list_revocation_propagates() {
    // Node A publishes a status list → Node B receives it → Node B's
    // DB has the bits. The issuer's did:key is its key, so B needs no
    // prior record of it.
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
            "revocation",
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

    // Drive the handler against a fresh DB.
    let db = new_test_db();
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
async fn queued_credentials_are_promoted_only_when_they_verify() {
    // Local test — no network needed. Credentials waiting in
    // `credentials_pending_verification` are retried when their issuer
    // announces itself, each through full verification: a signed credential
    // with nothing outstanding is promoted, a forged one is dropped.
    use app_lib::domain::vc::sign::{sign_credential, UnsignedCredential};
    use app_lib::domain::vc::{CredentialSubject, Proof, VerifiableCredential};

    let db = new_test_db();
    let key = super::common::test_key("pending-issuer");
    let issuer = alexandria_verify::did::derive_did_key(&key);
    let subject = super::common::test_did("pending-subject");
    let signed = |id: &str| {
        let mut properties = serde_json::Map::new();
        properties.insert("skillId".into(), serde_json::json!("s"));
        let vc = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some(id.into()),
            type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
            issuer: issuer.clone().into(),
            valid_from: super::common::TEST_NOW.into(),
            valid_until: None,
            credential_subject: CredentialSubject {
                id: subject.clone(),
                properties,
            },
            credential_status: None,
            terms_of_use: None,
            witness: None,
            integrity: None,
            evidence: Vec::new(),
            related_resource: Vec::new(),
            proof: Proof::unsigned(super::common::TEST_NOW),
        };
        sign_credential(UnsignedCredential { credential: vc }, &key, &issuer).unwrap()
    };
    let genuine = signed("urn:uuid:pending-genuine");
    let mut forged = signed("urn:uuid:pending-forged");
    forged
        .credential_subject
        .properties
        .insert("skillId".into(), serde_json::json!("another"));
    for vc in [&genuine, &forged] {
        queue_pending(
            db.conn(),
            vc.id.as_deref().unwrap(),
            issuer.as_str(),
            subject.as_str(),
            &serde_json::to_string(vc).unwrap(),
        )
        .unwrap();
    }

    let promoted = promote_pending_for(&db, issuer.as_str()).unwrap();
    assert_eq!(promoted, 1);

    let stored: Vec<String> = {
        let mut stmt = db.conn().prepare("SELECT id FROM credentials").unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(stored, vec!["urn:uuid:pending-genuine".to_string()]);
    let still_pending: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM credentials_pending_verification",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(still_pending, 0);
}
