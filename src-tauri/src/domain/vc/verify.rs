//! Verify a signed VC per §22.1. Stub — implementation in PR 4.

use rusqlite::Connection;

use super::{VerifiableCredential, VerificationPolicy, VerificationResult};

/// Verification algorithm per spec §13.2, steps 1–10. The DB handle is
/// used to look up the issuer's key registry (§5.3 historical keys)
/// and status lists (§11).
pub fn verify_credential(
    _db: &Connection,
    _credential: &VerifiableCredential,
    _verification_time: &str,
    _policy: &VerificationPolicy,
) -> VerificationResult {
    unimplemented!("PR 4 — verify credential")
}
