//! `TOPIC_VC_PRESENTATION` — subject opts in to broadcast a
//! selectively-disclosed presentation of a credential. Stub — PR 9.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

pub fn handle_presentation_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<(), String> {
    unimplemented!("PR 9 — presentation gossip")
}
