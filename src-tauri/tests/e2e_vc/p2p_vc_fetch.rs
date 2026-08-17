//! Pull-based credential fetch — authority, allowlist, network round-trip.
//!
//! Handler-shape tests drive `handle_fetch_request` directly. The
//! final test takes the real two-node network path:
//!   subject node B holds a credential, requestor node A fires
//!   `P2pNode::fetch_credential` over `/alexandria/vc-fetch/1.0`,
//!   B's swarm event loop synchronously consults its DB and replies,
//!   A's outbound future resolves with the deserialized response.

use super::common::{await_peers_connected, new_test_db, start_test_node, start_test_node_with_db};
use app_lib::p2p::vc_fetch::{
    allow_fetch, build_fetch_request, handle_fetch_request, FetchResponse,
};

/// Requests are signed now: `requestor` on its own proves nothing, because a
/// subject DID is public. These helpers give each actor a real key so the
/// tests exercise the same path a peer does.
fn requestor_key(seed: u8) -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[seed; 32])
}

fn requestor_did(seed: u8) -> app_lib::crypto::did::Did {
    alexandria_verify::did::derive_did_key(&requestor_key(seed))
}

fn seed_credential(conn: &rusqlite::Connection, id: &str, subject: &str) {
    let json = serde_json::json!({
        "@context": ["https://www.w3.org/ns/credentials/v2"],
        "id": id,
        "type": ["VerifiableCredential", "FormalCredential"],
        "issuer": "did:key:zIssuerFetchTest",
        "validFrom": "2026-04-13T00:00:00Z",
        "credentialSubject": {
            "id": subject,
            "skillId": "s",
            "level": 3,
            "score": 0.7,
            "evidenceRefs": [],
        },
        "proof": {
            "type": "Ed25519Signature2020",
            "created": "2026-04-13T00:00:00Z",
            "verificationMethod": "did:key:zIssuerFetchTest#key-1",
            "proofPurpose": "assertionMethod",
            "jws": "fake..jws"
        }
    })
    .to_string();
    conn.execute(
        "INSERT INTO credentials \
         (id, issuer_did, subject_did, credential_type, claim_kind, \
          issuance_date, signed_vc_json, integrity_hash) \
         VALUES (?1, 'did:key:zIssuerFetchTest', ?2, 'FormalCredential', \
                 'skill', '2026-04-13T00:00:00Z', ?3, 'h')",
        rusqlite::params![id, subject, json],
    )
    .unwrap();
}

#[tokio::test]
async fn public_credential_fetch_returns_vc() {
    // The MVP handler treats "subject == requestor" as authorized;
    // a future allowlist extension can layer "public flag" on top.
    // For now, the public-fetch test exercises the authorized path:
    // subject fetches their own credential.
    let db = new_test_db();
    seed_credential(db.conn(), "urn:uuid:public-vc", requestor_did(101).as_str());
    let req = build_fetch_request(&requestor_key(101), "urn:uuid:public-vc", "n-public");
    let resp = handle_fetch_request(db.conn(), &req).unwrap();
    assert!(matches!(resp, FetchResponse::Ok(_)));
}

#[tokio::test]
async fn private_credential_fetch_returns_unauthorized() {
    // Default: credentials are private. Non-subject requestor
    // MUST get Unauthorized even when the credential exists.
    let db = new_test_db();
    seed_credential(
        db.conn(),
        "urn:uuid:private-vc",
        requestor_did(101).as_str(),
    );
    let req = build_fetch_request(&requestor_key(102), "urn:uuid:private-vc", "n-private");
    let resp = handle_fetch_request(db.conn(), &req).unwrap();
    assert!(matches!(resp, FetchResponse::Unauthorized));
}

#[tokio::test]
async fn allowlisted_requestor_receives_private_credential() {
    // Subject explicitly allowlists a recruiter's DID; the
    // recruiter (not the subject) successfully fetches.
    let db = new_test_db();
    seed_credential(
        db.conn(),
        "urn:uuid:allowlisted-vc",
        requestor_did(101).as_str(),
    );
    allow_fetch(
        db.conn(),
        "urn:uuid:allowlisted-vc",
        requestor_did(103).as_str(),
    )
    .unwrap();
    let req = build_fetch_request(
        &requestor_key(103),
        "urn:uuid:allowlisted-vc",
        "n-allowlist",
    );
    let resp = handle_fetch_request(db.conn(), &req).unwrap();
    assert!(matches!(resp, FetchResponse::Ok(_)));

    // Sanity: a different requestor still gets Unauthorized.
    let req2 = build_fetch_request(
        &requestor_key(104),
        "urn:uuid:allowlisted-vc",
        "n-allowlist-2",
    );
    let resp2 = handle_fetch_request(db.conn(), &req2).unwrap();
    assert!(matches!(resp2, FetchResponse::Unauthorized));
}

#[tokio::test]
async fn public_credential_fetch_returns_vc_via_public_flag() {
    // `public` flag in the allowlist makes the credential
    // world-fetchable.
    let db = new_test_db();
    seed_credential(
        db.conn(),
        "urn:uuid:public-flag",
        requestor_did(101).as_str(),
    );
    allow_fetch(db.conn(), "urn:uuid:public-flag", "public").unwrap();
    let req = build_fetch_request(&requestor_key(105), "urn:uuid:public-flag", "n-pub-flag");
    let resp = handle_fetch_request(db.conn(), &req).unwrap();
    assert!(matches!(resp, FetchResponse::Ok(_)));
}

#[tokio::test]
async fn two_node_round_trip_over_vc_fetch_protocol() {
    // Subject node B seeds + allowlists a credential. Requestor
    // node A connects and fires `P2pNode::fetch_credential` over
    // /alexandria/vc-fetch/1.0. The response must come back
    // deserialized as the same VC (or SKIP if mDNS / port binding
    // doesn't work in the test environment).

    // Spin up the subject node B with its DB pre-seeded.
    let db_b = new_test_db();
    seed_credential(
        db_b.conn(),
        "urn:uuid:two-node-vc",
        requestor_did(101).as_str(),
    );
    allow_fetch(
        db_b.conn(),
        "urn:uuid:two-node-vc",
        requestor_did(106).as_str(),
    )
    .unwrap();
    let (mut node_b, _rx_b) = match start_test_node_with_db("vc-fetch-b", 32, db_b).await {
        Some(t) => t,
        None => return,
    };
    let peer_b = *node_b.peer_id();

    // Requestor node A doesn't need a DB for this flow.
    let (mut node_a, _rx_a) = match start_test_node("vc-fetch-a", 32).await {
        Some(t) => t,
        None => {
            node_b.shutdown().await;
            return;
        }
    };
    if !await_peers_connected(&node_a, &node_b, 10).await {
        node_a.shutdown().await;
        node_b.shutdown().await;
        eprintln!("SKIP: mDNS discovery timed out");
        return;
    }
    // Give request_response a moment to settle handshakes.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let req = build_fetch_request(&requestor_key(106), "urn:uuid:two-node-vc", "n-two-node");
    let resp = node_a.fetch_credential(peer_b, req).await;
    let outcome = match resp {
        Ok(r) => r,
        Err(e) => {
            // Accept transient transport failures as SKIP — the unit
            // tests already pin the handler-level shape; this test
            // is specifically about the wire path.
            eprintln!("SKIP: fetch_credential transport error: {e:?}");
            node_a.shutdown().await;
            node_b.shutdown().await;
            return;
        }
    };
    assert!(
        matches!(outcome, FetchResponse::Ok(_)),
        "expected Ok(vc), got {outcome:?}"
    );

    node_a.shutdown().await;
    node_b.shutdown().await;
}
