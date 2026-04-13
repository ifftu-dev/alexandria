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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn fetch_response_ok_variant_boxes_credential() {
        // The `Ok` variant boxes because a full VC is much larger than
        // the unit variants. Without the box, clippy's
        // `large_enum_variant` fires. Locking this in prevents an
        // accidental un-boxing later.
        fn assert_size_sane<T>() -> usize {
            std::mem::size_of::<T>()
        }
        let sz = assert_size_sane::<FetchResponse>();
        assert!(sz < 128, "FetchResponse enum is suspiciously large: {}", sz);
    }

    #[test]
    #[ignore = "pending PR 9 — vc-fetch handler"]
    fn unknown_credential_returns_not_found() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let req = FetchRequest {
            credential_id: "urn:uuid:missing".into(),
            requestor: Did("did:key:zRequestor".into()),
            nonce: "n-1".into(),
        };
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::NotFound));
    }

    #[test]
    #[ignore = "pending PR 9 — vc-fetch handler"]
    fn private_credential_returns_unauthorized_for_non_allowlisted_requestor() {
        // Default policy is private: a fetch from an arbitrary peer
        // MUST NOT leak the VC even if it exists locally.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let req = FetchRequest {
            credential_id: "urn:uuid:private".into(),
            requestor: Did("did:key:zStranger".into()),
            nonce: "n-2".into(),
        };
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::Unauthorized));
    }
}
