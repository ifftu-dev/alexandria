//! Local PinBoard commitment management — declare, revoke, observe.
//! Stub — implementation in PR 10.

use crate::crypto::did::Did;
use crate::p2p::pinboard::PinboardCommitment;

pub fn declare_commitment(
    _db: &rusqlite::Connection,
    _pinner_did: &Did,
    _subject_did: &Did,
    _scope: &[String],
) -> Result<PinboardCommitment, String> {
    unimplemented!("PR 10 — declare pinboard commitment")
}

pub fn revoke_commitment(_db: &rusqlite::Connection, _commitment_id: &str) -> Result<(), String> {
    unimplemented!("PR 10 — revoke pinboard commitment")
}

pub fn record_observation(
    _db: &rusqlite::Connection,
    _commitment: &PinboardCommitment,
) -> Result<(), String> {
    unimplemented!("PR 10 — record remote pinboard commitment")
}

pub fn list_pinners_for(
    _db: &rusqlite::Connection,
    _subject: &Did,
) -> Result<Vec<PinboardCommitment>, String> {
    unimplemented!("PR 10 — list pinners for subject")
}
