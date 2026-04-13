//! §20.4 — subject offline, PinBoard pinner online → credential still resolvable.

#[tokio::test]
#[ignore = "pending PR 10 — PinBoard"]
async fn credential_resolvable_when_subject_offline_via_pinboard() {
    // Node A (subject) publishes credential.
    // Node B declares PinBoard commitment to pin A's credentials.
    // Node A goes offline.
    // Node C fetches credential via discovery → finds it at B.
    unimplemented!("simulate routing through pinboard_observations")
}

#[tokio::test]
#[ignore = "pending PR 10 — PinBoard"]
async fn pinboard_observation_propagates_via_gossip() {
    // B declares commitment → broadcasts on TOPIC_PINBOARD →
    // C's handler inserts pinboard_observations row.
    unimplemented!("exercise p2p::pinboard::handle_pinboard_message")
}

#[tokio::test]
#[ignore = "pending PR 10 — PinBoard"]
async fn revoking_commitment_drops_pinboard_observation() {
    // B revokes → new signed message with revoked_at → C updates row.
    unimplemented!("exercise revocation path")
}
