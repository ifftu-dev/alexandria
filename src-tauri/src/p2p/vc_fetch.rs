//! Pull-based credential fetch via libp2p request-response on
//! `/alexandria/vc-fetch/1.0`. Authority-respecting — a subject opts
//! in per credential to whether it's publicly fetchable.
//! Stub — implementation in PR 9.

use crate::crypto::did::Did;
use crate::domain::vc::VerifiableCredential;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FetchRequest {
    pub credential_id: String,
    pub requestor: Did,
    pub nonce: String,
}

/// `Ok` variant boxes the VC because it's much larger than the other
/// variants — clippy `large_enum_variant` would otherwise fire.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum FetchResponse {
    Ok(Box<VerifiableCredential>),
    Unauthorized,
    NotFound,
}

/// Handler for an inbound fetch request. Applies the credential's
/// presentation policy + the subject's allowlist for the requestor.
pub fn handle_fetch_request(
    _db: &rusqlite::Connection,
    _req: &FetchRequest,
) -> Result<FetchResponse, String> {
    unimplemented!("PR 9 — vc-fetch handler")
}

/// Issue an outbound fetch to a specific peer DID.
pub async fn fetch_credential(
    _peer_did: &Did,
    _credential_id: &str,
) -> Result<FetchResponse, String> {
    unimplemented!("PR 9 — vc-fetch client")
}
