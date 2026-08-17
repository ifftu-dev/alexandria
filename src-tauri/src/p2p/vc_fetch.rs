//! Pull-based credential fetch via libp2p request-response on
//! `/alexandria/vc-fetch/1.0`. Authority-respecting — a subject opts
//! in per credential to whether it's publicly fetchable, and may
//! additionally restrict a credential to an allowlist of requestor
//! DIDs (`allow_fetch` / `disallow_fetch` / `is_allowlisted`).
//!
//! Request-response, not gossip: 1-to-1, served only when the node has
//! a `Database` wired into its swarm event loop. Wired up in
//! `p2p::network` (behaviour field `vc_fetch`).

use crate::crypto::did::Did;
use crate::domain::vc::VerifiableCredential;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FetchRequest {
    pub credential_id: String,
    /// The DID the caller claims to be. **Self-asserted** — on its own it
    /// proves nothing, because a subject DID is public: it is broadcast on
    /// `TOPIC_VC_DID`, sits inside every credential, and is the primary key of
    /// the talent index. It is authorised only in combination with `proof`.
    pub requestor: Did,
    /// Freshness token, echoed into the signed material so a captured proof
    /// cannot be replayed for a different request.
    pub nonce: String,
    /// Detached Ed25519 signature by `requestor` over
    /// [`canonical_fetch_bytes`]. Required.
    #[serde(default)]
    pub proof: Vec<u8>,
}

/// The bytes a requestor signs to prove they are who they claim.
///
/// Covers the credential being asked for (so a proof for one cannot open
/// another), the requestor DID (so it cannot be re-attributed), and the nonce
/// (so a captured proof is single-use against a responder that tracks nonces).
/// Length-prefixed so no two different tuples collide.
pub fn canonical_fetch_bytes(credential_id: &str, requestor: &str, nonce: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(credential_id.len() + requestor.len() + nonce.len() + 32);
    out.extend_from_slice(b"alexandria-vc-fetch-v1");
    for field in [
        credential_id.as_bytes(),
        requestor.as_bytes(),
        nonce.as_bytes(),
    ] {
        out.extend_from_slice(&(field.len() as u64).to_be_bytes());
        out.extend_from_slice(field);
    }
    out
}

/// Build a signed fetch request. The only supported way to make one, so a
/// caller cannot produce an unsigned request by forgetting a field.
pub fn build_fetch_request(
    signing_key: &ed25519_dalek::SigningKey,
    credential_id: &str,
    nonce: &str,
) -> FetchRequest {
    use ed25519_dalek::Signer;
    let requestor = alexandria_verify::did::derive_did_key(signing_key);
    let proof = signing_key
        .sign(&canonical_fetch_bytes(
            credential_id,
            requestor.as_str(),
            nonce,
        ))
        .to_bytes()
        .to_vec();
    FetchRequest {
        credential_id: credential_id.to_string(),
        requestor,
        nonce: nonce.to_string(),
        proof,
    }
}

/// Whether `req.proof` proves control of `req.requestor`.
///
/// `did:key` is self-resolving, so the key is read straight out of the claimed
/// DID and there is nothing to look up and nothing to poison in advance.
fn requestor_proved(req: &FetchRequest) -> bool {
    let Ok(sig_array): Result<[u8; 64], _> = req.proof.clone().try_into() else {
        return false;
    };
    let Ok(parsed) = alexandria_verify::did::parse_did_key(req.requestor.as_str()) else {
        return false;
    };
    let Ok(vk) = alexandria_verify::did::resolve_did_key(&parsed) else {
        return false;
    };
    let signed = canonical_fetch_bytes(&req.credential_id, req.requestor.as_str(), &req.nonce);
    vk.verify_strict(&signed, &ed25519_dalek::Signature::from_bytes(&sig_array))
        .is_ok()
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
///   0. If `proof` does not verify for the claimed `requestor` →
///      `Unauthorized`. Everything below trusts `requestor`, so nothing below
///      runs until it has been earned.
///   1. If we don't have the credential locally → `NotFound`.
///   2. If the requestor is the subject themselves → `Ok(vc)`.
///   3. If the credential has an allowlist row matching the
///      requestor DID exactly → `Ok(vc)`.
///   4. If the credential has an allowlist row marking it
///      `'public'` → `Ok(vc)` (anyone can fetch).
///   5. Otherwise → `Unauthorized`.
///
/// Step 0 is the whole access-control model. Without it `requestor` was a
/// self-asserted field in the request body, and because subject DIDs are
/// public, any peer could set `requestor` to the subject's own DID and take
/// step 2 — retrieving any credential this node held, whatever the allowlist
/// said.
pub fn handle_fetch_request(
    db: &rusqlite::Connection,
    req: &FetchRequest,
) -> Result<FetchResponse, String> {
    if !requestor_proved(req) {
        log::debug!(
            "vc-fetch: request claiming {} carries no valid proof — refusing",
            req.requestor.as_str()
        );
        return Ok(FetchResponse::Unauthorized);
    }

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
    use ed25519_dalek::SigningKey;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn subject_did(seed: u8) -> Did {
        alexandria_verify::did::derive_did_key(&key(seed))
    }

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
        // Matches the on-disk W3C VC v2 shape produced by
        // `sign_credential` — camelCase keys, inline subject
        // properties (no nested `claim` discriminator). Hand-crafted
        // to avoid pulling sign_credential into this unit test.
        let json = serde_json::json!({
            "@context": ["https://www.w3.org/ns/credentials/v2"],
            "id": id,
            "type": ["VerifiableCredential", "FormalCredential"],
            "issuer": "did:key:zIssuer",
            "validFrom": "2026-04-13T00:00:00Z",
            "credentialSubject": {
                "id": subject,
                "skillId": "s",
                "level": 3,
                "score": 0.7,
                "evidenceRefs": [],
            },
            "proof": {
                "type": "Ed25519Signature2020",
                "created": "2026-04-13T00:00:00Z",
                "verificationMethod": "did:key:zIssuer#key-1",
                "proofPurpose": "assertionMethod",
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
        let req = build_fetch_request(&key(1), "urn:uuid:missing", "n-1");
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::NotFound));
    }

    #[test]
    fn private_credential_returns_unauthorized_for_non_allowlisted_requestor() {
        // Default policy is private: a fetch from an arbitrary peer
        // (not the subject) MUST NOT leak the VC even if it exists.
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "urn:uuid:private", subject_did(2).as_str());
        let req = build_fetch_request(&key(99), "urn:uuid:private", "n-2");
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
        seed_credential(db.conn(), "urn:uuid:mine", subject_did(3).as_str());
        let req = build_fetch_request(&key(3), "urn:uuid:mine", "n-3");
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::Ok(_)));
    }

    #[test]
    fn allowlisted_requestor_can_fetch() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "urn:uuid:al", subject_did(4).as_str());
        allow_fetch(db.conn(), "urn:uuid:al", subject_did(5).as_str()).unwrap();
        let req = build_fetch_request(&key(5), "urn:uuid:al", "n-allow");
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::Ok(_)));
    }

    #[test]
    fn public_flag_allows_anyone() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "urn:uuid:pub", subject_did(6).as_str());
        allow_fetch(db.conn(), "urn:uuid:pub", "public").unwrap();
        let req = build_fetch_request(&key(7), "urn:uuid:pub", "n-pub");
        let resp = handle_fetch_request(db.conn(), &req).unwrap();
        assert!(matches!(resp, FetchResponse::Ok(_)));
    }

    #[test]
    fn disallow_revokes_allowlist_entry() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_credential(db.conn(), "urn:uuid:rev", subject_did(8).as_str());
        allow_fetch(db.conn(), "urn:uuid:rev", subject_did(9).as_str()).unwrap();
        disallow_fetch(db.conn(), "urn:uuid:rev", subject_did(9).as_str()).unwrap();
        let req = build_fetch_request(&key(9), "urn:uuid:rev", "n-rev");
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

    /// The finding. Subject DIDs are public — broadcast on TOPIC_VC_DID,
    /// embedded in every credential, the primary key of the talent index — so
    /// "requestor == subject" was a check anyone could pass by typing the
    /// subject's own DID into the request they were sending.
    #[test]
    fn claiming_to_be_the_subject_without_the_key_is_refused() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let subject = subject_did(20);
        seed_credential(db.conn(), "urn:uuid:spoof", subject.as_str());

        // Exactly what an attacker sends: the subject's public DID, no proof.
        let req = FetchRequest {
            credential_id: "urn:uuid:spoof".into(),
            requestor: subject.clone(),
            nonce: "n-spoof".into(),
            proof: Vec::new(),
        };
        assert!(matches!(
            handle_fetch_request(db.conn(), &req).unwrap(),
            FetchResponse::Unauthorized
        ));

        // And with a proof from the wrong key, which is the other half of it.
        let mut forged = build_fetch_request(&key(21), "urn:uuid:spoof", "n-spoof");
        forged.requestor = subject;
        assert!(matches!(
            handle_fetch_request(db.conn(), &forged).unwrap(),
            FetchResponse::Unauthorized
        ));
    }

    /// A proof is bound to the credential it was made for, so one handed over
    /// for a public credential cannot be reused to open a private one.
    #[test]
    fn a_proof_does_not_transfer_between_credentials() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let subject = subject_did(30);
        seed_credential(db.conn(), "urn:uuid:open", subject.as_str());
        seed_credential(db.conn(), "urn:uuid:closed", subject.as_str());

        let mut req = build_fetch_request(&key(30), "urn:uuid:open", "n");
        req.credential_id = "urn:uuid:closed".into();
        assert!(matches!(
            handle_fetch_request(db.conn(), &req).unwrap(),
            FetchResponse::Unauthorized
        ));
    }
}
