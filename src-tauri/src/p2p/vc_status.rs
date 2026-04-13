//! `TOPIC_VC_STATUS` handler — issuers broadcast status list snapshots
//! and deltas for revocation/suspension. Stub — PR 9.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusIngest {
    Applied,
    IgnoredNewer,
    IgnoredUnknownIssuer,
}

pub fn handle_status_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<StatusIngest, String> {
    unimplemented!("PR 9 — status list propagation")
}
