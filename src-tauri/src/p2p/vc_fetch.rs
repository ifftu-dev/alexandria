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
///   3. If the credential has an allowlist row matching the
///      requestor DID exactly → `Ok(vc)`.
///   4. If the credential has an allowlist row marking it
///      `'public'` → `Ok(vc)` (anyone can fetch).
///   5. Otherwise → `Unauthorized`.
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
    if req.requestor.as_str() == subject_did
        || is_allowlisted(db, &req.credential_id, req.requestor.as_str())
    {
        let vc: VerifiableCredential = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        return Ok(FetchResponse::Ok(Box::new(vc)));
    }
    Ok(FetchResponse::Unauthorized)
}

/// True iff the credential has an allowlist row for this requestor
/// (exact match) OR a `'public'` row (anyone can fetch). Pure SQL —
/// the allowlist is local-only and not synchronised across the
/// network.
fn is_allowlisted(db: &rusqlite::Connection, credential_id: &str, requestor: &str) -> bool {
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM credential_allowlist \
             WHERE credential_id = ?1 \
               AND (requestor_did = ?2 OR requestor_did = 'public')",
            rusqlite::params![credential_id, requestor],
            |r| r.get(0),
        )
        .unwrap_or(0);
    count > 0
}

/// Insert a (credential_id, requestor_did) allowlist entry. Use the
/// literal string `"public"` as `requestor_did` to mark the
/// credential as world-fetchable. Idempotent.
pub fn allow_fetch(
    db: &rusqlite::Connection,
    credential_id: &str,
    requestor_did: &str,
) -> Result<(), String> {
    db.execute(
        "INSERT OR IGNORE INTO credential_allowlist \
         (credential_id, requestor_did) VALUES (?1, ?2)",
        rusqlite::params![credential_id, requestor_did],
    )
    .map_err(|e| format!("allow_fetch: {e}"))?;
    Ok(())
}

/// Remove a (credential_id, requestor_did) entry. Idempotent.
pub fn disallow_fetch(
    db: &rusqlite::Connection,
    credential_id: &str,
    requestor_did: &str,
) -> Result<(), String> {
    db.execute(
        "DELETE FROM credential_allowlist \
         WHERE credential_id = ?1 AND requestor_did = ?2",
        rusqlite::params![credential_id, requestor_did],
    )
    .map_err(|e| format!("disallow_fetch: {e}"))?;
    Ok(())
}

/// Issue an outbound fetch to a specific peer DID.
///
/// **Deprecated**: this free function predates the libp2p request-
/// response wiring. Use `crate::p2p::network::P2pNode::fetch_credential`
/// instead — it takes a libp2p `PeerId` (not a DID) and round-trips
/// through the real `/alexandria/vc-fetch/1.0` protocol. We keep
/// this stub returning Err so the function name stays free for any
/// caller that hasn't migrated yet.
#[deprecated(
    since = "0.0.6-alpha",
    note = "use P2pNode::fetch_credential — request-response is now wired"
)]
pub async fn fetch_credential(
    _peer_did: &Did,
    _credential_id: &str,
) -> Result<FetchResponse, String> {
    Err("use P2pNode::fetch_credential — request-response is now wired".into())
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
        // The subject themselves is always allowed regardless of
        // the allowlist; any allowlist policy layers above this
        // baseline.
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

    #[test]
    fn allowlisted_requestor_can_fetch() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "urn:uuid:al", "did:key:zSubject");
        allow_fetch(db.conn(), "urn:uuid:al", "did:key:zRecruiter").unwrap();
        let req = FetchRequest {
            credential_id: "urn:uuid:al".into(),
            requestor: Did("did:key:zRecruiter".into()),
            nonce: "n-allow".into(),
        };
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::Ok(_)));
    }

    #[test]
    fn public_flag_allows_anyone() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "urn:uuid:pub", "did:key:zSubject");
        allow_fetch(db.conn(), "urn:uuid:pub", "public").unwrap();
        let req = FetchRequest {
            credential_id: "urn:uuid:pub".into(),
            requestor: Did("did:key:zRandom".into()),
            nonce: "n-pub".into(),
        };
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::Ok(_)));
    }

    #[test]
    fn disallow_revokes_allowlist_entry() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "urn:uuid:rev", "did:key:zSubject");
        allow_fetch(db.conn(), "urn:uuid:rev", "did:key:zRecruiter").unwrap();
        disallow_fetch(db.conn(), "urn:uuid:rev", "did:key:zRecruiter").unwrap();
        let req = FetchRequest {
            credential_id: "urn:uuid:rev".into(),
            requestor: Did("did:key:zRecruiter".into()),
            nonce: "n-rev".into(),
        };
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::Unauthorized));
    }

    #[test]
    fn allow_fetch_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "urn:uuid:idem", "did:key:zSubject");
        allow_fetch(db.conn(), "urn:uuid:idem", "did:key:zRec").unwrap();
        allow_fetch(db.conn(), "urn:uuid:idem", "did:key:zRec").unwrap();
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credential_allowlist \
                 WHERE credential_id = 'urn:uuid:idem'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
