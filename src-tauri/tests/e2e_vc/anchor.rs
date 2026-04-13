//! §12.3 — integrity anchor queue. Every credential gets its hash
//! anchored on Cardano as a metadata-only tx. Idempotent; retries
//! with backoff; silently skips without Blockfrost creds.

use super::common::new_test_db;
use app_lib::cardano::anchor_queue::{enqueue, tick, AnchorStatus};

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

#[tokio::test]
async fn tick_without_blockfrost_is_noop() {
    let db = std::sync::Arc::new(std::sync::Mutex::new(Some(new_test_db())));
    let n = tick(&db, &None, &None).await.expect("tick ok");
    assert_eq!(n, 0);
}

#[tokio::test]
#[ignore = "pending PR 8 — anchor queue"]
async fn confirmed_anchors_never_reprocess() {
    // Seed a credential_anchors row with status=confirmed, then run
    // tick; the status must not flip backwards and no new tx is built.
    let _db = new_test_db();
    let _status = AnchorStatus::Confirmed;
    unimplemented!("seed and assert idempotency once PR 8 lands the processor")
}
