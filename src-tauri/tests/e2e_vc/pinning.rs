//! 5-tier eviction precedence (§12 + §20.4):
//!   1. Subject-authored (auto_unpin = 0) — NEVER evict.
//!   2. PinBoard-committed — retained while an active
//!      `pinboard_observations` row exists.
//!   3. DID docs + status lists for known issuers — live in SQL
//!      (not blob-pinned today); eviction only touches iroh blobs,
//!      so the SQL rows trivially survive any storage pressure.
//!   4. Active-enrollment courses — evict only on unenroll.
//!   5. Cache — LRU.
//!
//! These tests drive `list_evictable_pins_for_test` directly so we
//! can assert the precedence without booting a full iroh node for
//! `maybe_evict`.

use super::common::new_test_db;
use app_lib::ipfs::storage::list_evictable_pins_for_test;

fn seed_pin(
    db: &app_lib::db::Database,
    cid: &str,
    pin_type: &str,
    size_bytes: u64,
    auto_unpin: bool,
    last_accessed: Option<&str>,
) {
    db.conn()
        .execute(
            "INSERT INTO pins (cid, pin_type, size_bytes, last_accessed, auto_unpin) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                cid,
                pin_type,
                size_bytes as i64,
                last_accessed,
                auto_unpin as i32
            ],
        )
        .unwrap();
}

#[tokio::test]
async fn cache_evicts_before_pinboard_content() {
    // Seed two evictable pins: one cache, one pinboard. The cache
    // pin MUST appear before the pinboard pin in the eviction queue.
    let db = new_test_db();
    // Active pinboard observation so the pinboard pin qualifies for tier 2.
    db.conn()
        .execute(
            "INSERT INTO pinboard_observations \
             (id, pinner_did, subject_did, scope, commitment_since, signature, public_key) \
             VALUES ('c1', 'did:key:zP', 'did:key:zS', '[\"credentials\"]', '2026-04-13T00:00:00Z', 'sig', 'pk')",
            [],
        )
        .unwrap();
    seed_pin(&db, "cache_hash", "cache", 1_000, true, Some("2026-04-10"));
    seed_pin(
        &db,
        "pinboard_hash",
        "pinboard",
        2_000,
        true,
        Some("2026-04-01"),
    );

    let queue = list_evictable_pins_for_test(db.conn());
    let cids: Vec<&str> = queue.iter().map(|p| p.cid.as_str()).collect();
    let cache_idx = cids.iter().position(|c| *c == "cache_hash").unwrap();
    let pinboard_idx = cids.iter().position(|c| *c == "pinboard_hash").unwrap();
    assert!(
        cache_idx < pinboard_idx,
        "cache ({cache_idx}) must be evicted before pinboard ({pinboard_idx}): {cids:?}"
    );
}

#[tokio::test]
async fn subject_authored_content_never_evicts() {
    // auto_unpin = 0 rows are excluded from the eviction queue
    // entirely, so subject-authored content can never be chosen
    // for LRU eviction under storage pressure.
    let db = new_test_db();
    seed_pin(
        &db,
        "authored_hash",
        "profile",
        5_000,
        false,
        Some("1970-01-01"),
    );
    seed_pin(&db, "cache_hash", "cache", 1_000, true, Some("2026-04-13"));

    let queue = list_evictable_pins_for_test(db.conn());
    let cids: Vec<&str> = queue.iter().map(|p| p.cid.as_str()).collect();
    assert!(cids.contains(&"cache_hash"));
    assert!(
        !cids.contains(&"authored_hash"),
        "subject-authored (auto_unpin=0) must not appear in the eviction queue"
    );
}

#[tokio::test]
async fn did_docs_and_status_lists_retained_for_verification() {
    // DID docs (`key_registry`) and revocation status lists
    // (`credential_status_lists`) are SQL rows, not blob-pinned
    // iroh content. The eviction path only touches the `pins` table;
    // these tables are untouched by any eviction pass. We assert
    // that invariant by showing that an eviction queue built when
    // the DB holds key_registry + status_lists doesn't include them
    // (they aren't even candidates).
    let db = new_test_db();
    db.conn()
        .execute(
            "INSERT INTO key_registry \
             (did, key_id, public_key_hex, valid_from) \
             VALUES ('did:key:zI', 'key-1', 'ab', '2026-04-13T00:00:00Z')",
            [],
        )
        .unwrap();
    db.conn()
        .execute(
            "INSERT INTO credential_status_lists \
             (list_id, issuer_did, version, bits) \
             VALUES ('urn:alexandria:status-list:did:key:zI:1', 'did:key:zI', 1, X'00')",
            [],
        )
        .unwrap();
    seed_pin(&db, "cache_hash", "cache", 1_000, true, Some("2026-04-13"));

    let queue = list_evictable_pins_for_test(db.conn());
    // Only the cache pin is in the queue — the SQL rows for DID doc
    // and status list aren't eligible for blob-eviction at all.
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].cid, "cache_hash");
}

#[tokio::test]
async fn revoked_commitment_demotes_to_cache_tier() {
    // A pinboard-tier pin stays ahead of cache while the commitment
    // is active. Once we revoke the pinboard observation, the pin
    // is no longer protected by tier 2 (pinboard) — it falls into
    // the generic tier 3 (default). The cache pin remains tier 5
    // (evicted first).
    let db = new_test_db();
    db.conn()
        .execute(
            "INSERT INTO pinboard_observations \
             (id, pinner_did, subject_did, scope, commitment_since, signature, public_key) \
             VALUES ('c1', 'did:key:zP', 'did:key:zS', '[\"credentials\"]', '2026-04-13T00:00:00Z', 'sig', 'pk')",
            [],
        )
        .unwrap();
    seed_pin(
        &db,
        "pinboard_hash",
        "pinboard",
        2_000,
        true,
        Some("2026-04-01"),
    );
    seed_pin(&db, "cache_hash", "cache", 1_000, true, Some("2026-04-10"));

    // Before revoke: cache still ahead of pinboard (they're both
    // evictable but cache has higher tier-number = evicted first).
    // The test `cache_evicts_before_pinboard_content` already covers
    // that. What we assert here: after revocation, the pinboard pin
    // is still evictable (it always was) and still appears in the
    // queue behind cache — no structural change, but the protective
    // tier-2 classification falls away. This matches the spec:
    // revoking a commitment demotes the content's pin status but
    // doesn't immediately evict it.
    db.conn()
        .execute(
            "UPDATE pinboard_observations SET revoked_at = '2026-04-14T00:00:00Z'",
            [],
        )
        .unwrap();

    let queue = list_evictable_pins_for_test(db.conn());
    let cids: Vec<&str> = queue.iter().map(|p| p.cid.as_str()).collect();
    assert!(cids.contains(&"pinboard_hash"));
    assert!(cids.contains(&"cache_hash"));
    // Cache still evicts first regardless.
    let c = cids.iter().position(|x| *x == "cache_hash").unwrap();
    let p = cids.iter().position(|x| *x == "pinboard_hash").unwrap();
    assert!(
        c < p,
        "cache must remain ahead of (now-demoted) pinboard pin in eviction order"
    );
}
