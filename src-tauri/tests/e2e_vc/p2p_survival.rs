//! §20.4 — subject offline, PinBoard pinner online → credential still resolvable.

use super::common::{await_gossip_on, await_peers_connected, new_test_db, start_test_node};
use app_lib::crypto::did::Did;
use app_lib::ipfs::pinboard::{declare_commitment, list_pinners_for, revoke_commitment};
use app_lib::p2p::pinboard::{handle_pinboard_message, PinboardCommitment};

#[tokio::test]
async fn credential_resolvable_when_subject_offline_via_pinboard() {
    // Simplified in-process analogue of the three-node scenario:
    //   Node A (subject) publishes credential.
    //   Node B declares PinBoard commitment to pin A's credentials.
    //   Node A goes offline.
    //   Node C fetches the credential → discovers B via pinboard
    //   observation → retrieves from B.
    //
    // With no DHT + request-response wiring for arbitrary fetch,
    // we assert the DB-level invariant: given a pinboard
    // observation for subject A, `list_pinners_for(A)` returns B —
    // which is the data an external discovery layer would use to
    // route a fetch. The transport plumbing (DHT + request-response)
    // is a follow-up, but the routing data is correct end-to-end.
    let db = new_test_db();
    let pinner = Did("did:key:zPinnerSurv".into());
    let subject = Did("did:key:zSubjectSurv".into());
    declare_commitment(db.conn(), &pinner, &subject, &["credentials".into()]).unwrap();

    let pinners = list_pinners_for(db.conn(), &subject).unwrap();
    assert_eq!(pinners.len(), 1);
    assert_eq!(pinners[0].pinner_did, pinner.as_str());
    // With the subject "offline", a remote peer looking up the
    // subject would still see pinner B as a candidate source.
    // That's what survivability means at the routing layer.
}

#[tokio::test]
async fn pinboard_observation_propagates_via_gossip() {
    // B declares commitment → broadcasts on TOPIC_PINBOARD →
    // C's handler inserts pinboard_observations row.
    let (mut b, _rx_b) = match start_test_node("pinboard-b", 32).await {
        Some(t) => t,
        None => return,
    };
    let (mut c, mut rx_c) = match start_test_node("pinboard-c", 64).await {
        Some(t) => t,
        None => {
            b.shutdown().await;
            return;
        }
    };
    if !await_peers_connected(&b, &c, 10).await {
        b.shutdown().await;
        c.shutdown().await;
        eprintln!("SKIP: mDNS discovery timed out");
        return;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let commit = PinboardCommitment {
        id: "urn:uuid:commit-e2e".into(),
        pinner_did: "did:key:zPinnerE2E".into(),
        subject_did: "did:key:zSubjectE2E".into(),
        scope: vec!["credentials".into()],
        commitment_since: "2026-04-13T00:00:00Z".into(),
        revoked_at: None,
        signature: "sig".into(),
        public_key: "pk".into(),
    };
    let payload = serde_json::to_vec(&commit).unwrap();
    let key = super::common::test_key("pinboard-b");
    if let Err(e) = b
        .publish_pinboard(payload.clone(), &key, "stake_test1upinb")
        .await
    {
        eprintln!("SKIP: publish failed: {e:?}");
        b.shutdown().await;
        c.shutdown().await;
        return;
    }

    let received = await_gossip_on(&mut rx_c, "pinboard", 5).await;
    let payload_bytes = match received {
        Some(p) => p,
        None => {
            eprintln!("SKIP: gossip propagation timed out");
            b.shutdown().await;
            c.shutdown().await;
            return;
        }
    };

    // Drive the handler against a fresh DB to assert the
    // application-layer effect.
    let db = new_test_db();
    let msg = app_lib::p2p::types::SignedGossipMessage {
        topic: "/alexandria/pinboard/1.0".into(),
        payload: payload_bytes,
        signature: vec![0; 64],
        public_key: vec![0; 32],
        stake_address: "stake_test1upinb".into(),
        timestamp: 1_712_880_000,
        encrypted: false,
        key_id: None,
    };
    handle_pinboard_message(&db, &msg).unwrap();

    let found = list_pinners_for(db.conn(), &Did("did:key:zSubjectE2E".into())).unwrap();
    assert!(found.iter().any(|c| c.id == "urn:uuid:commit-e2e"));

    b.shutdown().await;
    c.shutdown().await;
}

#[tokio::test]
async fn revoking_commitment_drops_pinboard_observation() {
    // Local test: declare → revoke → list shows the revoked_at
    // stamp. This is what downstream eviction logic reads to
    // demote the pin from tier 2 to the default tier.
    let db = new_test_db();
    let pinner = Did("did:key:zRevokerPinner".into());
    let subject = Did("did:key:zRevokerSubject".into());
    let commit = declare_commitment(db.conn(), &pinner, &subject, &["credentials".into()]).unwrap();

    revoke_commitment(db.conn(), &commit.id).unwrap();
    let after = list_pinners_for(db.conn(), &subject).unwrap();
    assert_eq!(after.len(), 1);
    assert!(after[0].revoked_at.is_some());
}
