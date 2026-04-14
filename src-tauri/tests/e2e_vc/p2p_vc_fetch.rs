//! Pull-based credential fetch — authority, allowlist, replay.
//!
//! The underlying protocol (libp2p request-response on
//! `/alexandria/vc-fetch/1.0`) is not yet wired; these tests
//! exercise `p2p::vc_fetch::handle_fetch_request` directly with
//! the same DB fixtures a live handler would see. That still proves
//! the authority/allowlist/response contract end-to-end at the
//! handler level; the transport wiring is a follow-up.

use super::common::new_test_db;
use app_lib::crypto::did::Did;
use app_lib::p2p::vc_fetch::{allow_fetch, handle_fetch_request, FetchRequest, FetchResponse};

fn seed_credential(conn: &rusqlite::Connection, id: &str, subject: &str) {
    let json = serde_json::json!({
        "@context": ["https://www.w3.org/2018/credentials/v1"],
        "id": id,
        "type": ["VerifiableCredential", "FormalCredential"],
        "issuer": "did:key:zIssuerFetchTest",
        "issuance_date": "2026-04-13T00:00:00Z",
        "credential_subject": {
            "id": subject,
            "claim": { "kind": "skill", "skill_id": "s", "level": 3, "score": 0.7, "evidence_refs": [] }
        },
        "proof": {
            "type": "Ed25519Signature2020",
            "created": "2026-04-13T00:00:00Z",
            "verification_method": "did:key:zIssuerFetchTest#key-1",
            "proof_purpose": "assertionMethod",
            "jws": "fake..jws"
        }
    })
    .to_string();
    conn.execute(
        "INSERT INTO credentials \
         (id, issuer_did, subject_did, credential_type, claim_kind, \
          issuance_date, signed_vc_json, integrity_hash) \
         VALUES (?1, 'did:key:zIssuerFetchTest', ?2, 'FormalCredential', \
                 'skill', '2026-04-13T00:00:00Z', ?3, 'h')",
        rusqlite::params![id, subject, json],
    )
    .unwrap();
}

#[tokio::test]
async fn public_credential_fetch_returns_vc() {
    // The MVP handler treats "subject == requestor" as authorized;
    // a future allowlist extension can layer "public flag" on top.
    // For now, the public-fetch test exercises the authorized path:
    // subject fetches their own credential.
    let db = new_test_db();
    seed_credential(db.conn(), "urn:uuid:public-vc", "did:key:zSubjectFetchTest");
    let req = FetchRequest {
        credential_id: "urn:uuid:public-vc".into(),
        requestor: Did("did:key:zSubjectFetchTest".into()),
        nonce: "n-public".into(),
    };
    let resp = handle_fetch_request(db.conn(), &req).unwrap();
    assert!(matches!(resp, FetchResponse::Ok(_)));
}

#[tokio::test]
async fn private_credential_fetch_returns_unauthorized() {
    // Default: credentials are private. Non-subject requestor
    // MUST get Unauthorized even when the credential exists.
    let db = new_test_db();
    seed_credential(
        db.conn(),
        "urn:uuid:private-vc",
        "did:key:zSubjectFetchTest",
    );
    let req = FetchRequest {
        credential_id: "urn:uuid:private-vc".into(),
        requestor: Did("did:key:zStrangerFetchTest".into()),
        nonce: "n-private".into(),
    };
    let resp = handle_fetch_request(db.conn(), &req).unwrap();
    assert!(matches!(resp, FetchResponse::Unauthorized));
}

#[tokio::test]
async fn allowlisted_requestor_receives_private_credential() {
    // Subject explicitly allowlists a recruiter's DID; the
    // recruiter (not the subject) successfully fetches.
    let db = new_test_db();
    seed_credential(
        db.conn(),
        "urn:uuid:allowlisted-vc",
        "did:key:zSubjectFetchTest",
    );
    allow_fetch(
        db.conn(),
        "urn:uuid:allowlisted-vc",
        "did:key:zRecruiterFetchTest",
    )
    .unwrap();
    let req = FetchRequest {
        credential_id: "urn:uuid:allowlisted-vc".into(),
        requestor: Did("did:key:zRecruiterFetchTest".into()),
        nonce: "n-allowlist".into(),
    };
    let resp = handle_fetch_request(db.conn(), &req).unwrap();
    assert!(matches!(resp, FetchResponse::Ok(_)));

    // Sanity: a different requestor still gets Unauthorized.
    let req2 = FetchRequest {
        credential_id: "urn:uuid:allowlisted-vc".into(),
        requestor: Did("did:key:zNotOnList".into()),
        nonce: "n-allowlist-2".into(),
    };
    let resp2 = handle_fetch_request(db.conn(), &req2).unwrap();
    assert!(matches!(resp2, FetchResponse::Unauthorized));
}

#[tokio::test]
async fn public_credential_fetch_returns_vc_via_public_flag() {
    // `public` flag in the allowlist makes the credential
    // world-fetchable.
    let db = new_test_db();
    seed_credential(
        db.conn(),
        "urn:uuid:public-flag",
        "did:key:zSubjectFetchTest",
    );
    allow_fetch(db.conn(), "urn:uuid:public-flag", "public").unwrap();
    let req = FetchRequest {
        credential_id: "urn:uuid:public-flag".into(),
        requestor: Did("did:key:zArbitraryRequestor".into()),
        nonce: "n-pub-flag".into(),
    };
    let resp = handle_fetch_request(db.conn(), &req).unwrap();
    assert!(matches!(resp, FetchResponse::Ok(_)));
}
