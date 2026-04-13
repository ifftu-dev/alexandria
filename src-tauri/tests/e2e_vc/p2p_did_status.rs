//! §5.3 + §11.2 — DID doc + status list propagation over gossip.

#[tokio::test]
#[ignore = "pending PR 9 — P2P VC propagation"]
async fn did_doc_rotation_propagates_to_second_node() {
    // Two in-process Database handles as node A and node B.
    // A rotates its key → publishes a vc_did gossip message → B's
    // handler inserts the new KeyRegistryEntry → B can verify a
    // pre-rotation credential using the historical key.
    unimplemented!("exercise p2p::vc_did::handle_did_message")
}

#[tokio::test]
#[ignore = "pending PR 9 — P2P VC propagation"]
async fn status_list_revocation_propagates() {
    // Issuer publishes status list with revoked bit set → receivers
    // flip the corresponding credential's `revoked` flag locally.
    unimplemented!("exercise p2p::vc_status::handle_status_message")
}

#[tokio::test]
#[ignore = "pending PR 9 — P2P VC propagation"]
async fn credential_queued_until_issuer_did_doc_arrives() {
    // Deliver a credential whose issuer DID is unknown locally → queued
    // into credentials_pending_verification → DID doc arrives → sweeper
    // promotes the credential into `credentials`.
    unimplemented!("exercise credentials_pending_verification promotion")
}
