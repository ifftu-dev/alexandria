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
///
/// Decision tree:
///   1. If we don't have the credential locally → `NotFound`.
///   2. If the requestor is the subject themselves → `Ok(vc)`.
///   3. Otherwise → `Unauthorized`. (PR 11 layers per-credential
///      allowlists + public-flag overrides on top of this.)
pub fn handle_fetch_request(
    db: &rusqlite::Connection,
    req: &FetchRequest,
) -> Result<FetchResponse, String> {
    let row: Option<(String, String)> = db
        .query_row(
            "SELECT signed_vc_json, subject_did FROM credentials WHERE id = ?1",
            rusqlite::params![&req.credential_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    let (json, subject_did) = match row {
        Some(r) => r,
        None => return Ok(FetchResponse::NotFound),
    };
    if req.requestor.as_str() == subject_did {
        let vc: VerifiableCredential = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        return Ok(FetchResponse::Ok(Box::new(vc)));
    }
    Ok(FetchResponse::Unauthorized)
}

/// Issue an outbound fetch to a specific peer DID. Real network
/// transport (libp2p request-response) lands when the protocol
/// behaviour is wired in `p2p::network`; until then, callers should
/// treat this as not-yet-available rather than an error condition.
pub async fn fetch_credential(
    _peer_did: &Did,
    _credential_id: &str,
) -> Result<FetchResponse, String> {
    Err("vc-fetch outbound: libp2p request-response not yet wired".into())
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

    fn seed_credential(conn: &rusqlite::Connection, id: &str, subject: &str) {
        // Matches the on-disk shape produced by `sign_credential` —
        // snake_case keys, since the Rust structs don't apply
        // `rename_all = "camelCase"`. Hand-crafted to avoid pulling
        // sign_credential into this unit test.
        let json = serde_json::json!({
            "@context": ["https://www.w3.org/2018/credentials/v1"],
            "id": id,
            "type": ["VerifiableCredential", "FormalCredential"],
            "issuer": "did:key:zIssuer",
            "issuance_date": "2026-04-13T00:00:00Z",
            "credential_subject": {
                "id": subject,
                "claim": { "kind": "skill", "skill_id": "s", "level": 3, "score": 0.7, "evidence_refs": [] }
            },
            "proof": {
                "type": "Ed25519Signature2020",
                "created": "2026-04-13T00:00:00Z",
                "verification_method": "did:key:zIssuer#key-1",
                "proof_purpose": "assertionMethod",
                "jws": "fake..jws"
            }
        })
        .to_string();
        conn.execute(
            "INSERT INTO credentials \
             (id, issuer_did, subject_did, credential_type, claim_kind, \
              issuance_date, signed_vc_json, integrity_hash) \
             VALUES (?1, 'did:key:zIssuer', ?2, 'FormalCredential', 'skill', \
                     '2026-04-13T00:00:00Z', ?3, 'h')",
            rusqlite::params![id, subject, json],
        )
        .unwrap();
    }

    #[test]
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
    fn private_credential_returns_unauthorized_for_non_allowlisted_requestor() {
        // Default policy is private: a fetch from an arbitrary peer
        // (not the subject) MUST NOT leak the VC even if it exists.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "urn:uuid:private", "did:key:zSubject");
        let req = FetchRequest {
            credential_id: "urn:uuid:private".into(),
            requestor: Did("did:key:zStranger".into()),
            nonce: "n-2".into(),
        };
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::Unauthorized));
    }

    #[test]
    fn subject_can_fetch_their_own_credential() {
        // The subject themselves is always allowed; any future
        // allowlist policy layers above this baseline.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "urn:uuid:mine", "did:key:zSubject");
        let req = FetchRequest {
            credential_id: "urn:uuid:mine".into(),
            requestor: Did("did:key:zSubject".into()),
            nonce: "n-3".into(),
        };
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::Ok(_)));
    }
}
