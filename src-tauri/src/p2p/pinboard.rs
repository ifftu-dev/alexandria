//! `TOPIC_PINBOARD` handler — peers broadcast opt-in commitments to
//! pin specific subjects' content for community redundancy.
//! Stub — implementation in PR 10.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PinboardCommitment {
    pub id: String,
    pub pinner_did: String,
    pub subject_did: String,
    pub scope: Vec<String>,
    pub commitment_since: String,
    pub revoked_at: Option<String>,
    pub signature: String,
    pub public_key: String,
}

pub fn handle_pinboard_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<(), String> {
    unimplemented!("PR 10 — PinBoard gossip")
}
