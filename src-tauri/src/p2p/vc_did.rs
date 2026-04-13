//! `TOPIC_VC_DID` handler — issuers broadcast their DID document and
//! key rotations. Stub — implementation in PR 9.

use crate::db::Database;
use crate::p2p::types::SignedGossipMessage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DidIngest {
    Stored,
    UpdatedRegistry,
    Ignored,
}

pub fn handle_did_message(
    _db: &Database,
    _message: &SignedGossipMessage,
) -> Result<DidIngest, String> {
    unimplemented!("PR 9 — DID doc propagation")
}
