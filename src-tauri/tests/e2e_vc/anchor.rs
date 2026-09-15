//! §12.3 — integrity anchor queue. Every credential gets its hash
//! anchored on Cardano as a metadata-only tx. Idempotent; retries
//! with backoff; silently skips without Blockfrost creds.
//!
//! `anchor_queue::tick` runs its database phases through a profile-fenced
//! executor journal that only the application crate can construct. Its
//! idle-node, confirmed-row and uncertain-outcome behavior is covered by the
//! crate's unit tests against a loopback provider.

use super::common::new_test_db;
use app_lib::cardano::anchor_queue::enqueue;

#[tokio::test]
async fn enqueue_is_idempotent() {
    let db = new_test_db();
    // FK requires the referenced credential exists. Insert a stub row.
    db.conn()
        .execute(
            "INSERT INTO credentials \
             (id, issuer_did, subject_did, credential_type, claim_kind, \
              issuance_date, signed_vc_json, integrity_hash) \
             VALUES ('cred-1', 'did:key:zI', 'did:key:zS', 'FormalCredential', \
                     'skill', '2026-04-13T00:00:00Z', '{}', 'h')",
            [],
        )
        .unwrap();
    enqueue(db.conn(), "cred-1").expect("first enqueue");
    enqueue(db.conn(), "cred-1").expect("second enqueue no-op");
    let count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM credential_anchors WHERE credential_id = ?1",
            rusqlite::params!["cred-1"],
            |r| r.get(0),
        )
        .unwrap_or(0);
    assert_eq!(count, 1);
}
