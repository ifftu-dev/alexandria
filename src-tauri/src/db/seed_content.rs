//! Seed content for dev/testnet course elements.
//!
//! Contains inline content for all seed elements (HTML for text,
//! JSON for quizzes/MCQs/essays). Also provides
//! `seed_content_if_needed()` which writes content into iroh blobs
//! and populates `content_cid`.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use std::collections::{HashMap, HashSet};

use crate::content_store::content;
use crate::content_store::node::ContentNode;
use crate::db::Database;

struct SeedContent<'a> {
    element_id: &'a str,
    hash: String,
    inline_seed: Option<&'a str>,
    public_source: Option<(&'a str, u64)>,
}

/// Seed content into iroh for elements that lack a `content_cid`.
/// Returns the number of elements updated, or 0 if skipped.
pub async fn seed_content_if_needed(
    db: &Arc<Mutex<Option<Database>>>,
    node: &Arc<ContentNode>,
) -> Result<u32, String> {
    // Find all seed elements that still need content CIDs.
    let needs_seed: HashSet<String> = {
        let guard = db.lock().unwrap();
        let db = guard.as_ref().ok_or("database not initialized")?;
        let mut stmt = db
            .conn()
            .prepare("SELECT id FROM course_elements WHERE id LIKE 'el_%' AND content_cid IS NULL")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut ids = HashSet::new();
        for row in rows {
            ids.insert(row.map_err(|e| e.to_string())?);
        }
        ids
    };

    if needs_seed.is_empty() {
        log::info!("No seed elements need content — skipping content seed");
        return Ok(0);
    }

    log::info!(
        "Seeding content blobs for dev/testnet elements ({} pending)…",
        needs_seed.len()
    );

    // Phase 1: Add all blobs to iroh WITHOUT holding the DB lock.
    // This is the slow part and must not block other DB consumers.
    let mut pending = Vec::new();
    for (element_id, body) in SEED_CONTENT {
        if !needs_seed.contains(*element_id) {
            continue;
        }
        let result = content::add_bytes(node, body.as_bytes())
            .await
            .map_err(|e| format!("failed to add content for {element_id}: {e}"))?;
        pending.push(SeedContent {
            element_id,
            hash: result.hash,
            inline_seed: Some(body),
            public_source: None,
        });
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|e| format!("failed to build HTTP client for media seed: {e}"))?;
    let mut media_cache: HashMap<&'static str, Vec<u8>> = HashMap::new();

    for asset in REMOTE_SEED_ASSETS {
        if !needs_seed.contains(asset.element_id) {
            continue;
        }

        let bytes = if let Some(cached) = media_cache.get(asset.url) {
            cached.clone()
        } else {
            let response = client
                .get(asset.url)
                .send()
                .await
                .map_err(|e| {
                    format!(
                        "failed to download {} for {}: {e}",
                        asset.url, asset.element_id
                    )
                })?
                .error_for_status()
                .map_err(|e| {
                    format!(
                        "download returned error status for {} ({}): {e}",
                        asset.element_id, asset.url
                    )
                })?;

            let body = response
                .bytes()
                .await
                .map_err(|e| format!("failed to read media body for {}: {e}", asset.element_id))?
                .to_vec();

            media_cache.insert(asset.url, body.clone());
            body
        };

        let result = content::add_bytes(node, &bytes).await.map_err(|e| {
            format!(
                "failed to add downloaded media for {}: {e}",
                asset.element_id
            )
        })?;

        // Store the BLAKE3 hash as content_cid (not the URL). This gives
        // the resolver a direct iroh-local lookup without indirection
        // through the mapping table. The URL→BLAKE3 mapping is still
        // kept in content_mappings so cross-device resolver fallback
        // can re-fetch from the public URL if the local store is missing
        // the blob.
        pending.push(SeedContent {
            element_id: asset.element_id,
            hash: result.hash,
            inline_seed: None,
            public_source: Some((asset.url, result.size)),
        });
    }

    // Phase 2: Single DB write lock — batch-update all rows in a transaction.
    let updated = {
        let guard = db.lock().unwrap();
        let db = guard.as_ref().ok_or("database not initialized")?;
        persist_seed_content(db.conn(), &pending)?
    };

    log::info!("Seeded content for {updated} elements");
    Ok(updated)
}

fn persist_seed_content(
    conn: &rusqlite::Connection,
    pending: &[SeedContent<'_>],
) -> Result<u32, String> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("begin content seed: {e}"))?;
    let mut count = 0u32;
    for content in pending {
        // Downloads happen without the DB lock. A newer edit, deletion or
        // competing seed must win over our earlier snapshot of missing CIDs.
        let changed = tx
            .execute(
                "UPDATE course_elements SET content_cid = ?1 WHERE id = ?2 AND content_cid IS NULL
             AND (content_inline IS NULL OR content_inline = ?3)",
                rusqlite::params![content.hash, content.element_id, content.inline_seed],
            )
            .map_err(|e| format!("failed to update {}: {e}", content.element_id))?;
        if changed == 0 {
            continue;
        }
        count += 1;
        if let Some((public_id, size)) = content.public_source {
            let size = i64::try_from(size).map_err(|_| "seed content size exceeds SQLite range")?;
            tx.execute(
                "INSERT INTO content_mappings (external_id, blake3_hash, size_bytes)
                 VALUES (?1, ?2, ?3) ON CONFLICT(external_id) DO UPDATE SET
                 blake3_hash = excluded.blake3_hash, size_bytes = excluded.size_bytes,
                 mapped_at = datetime('now')",
                rusqlite::params![public_id, content.hash, size],
            )
            .map_err(|e| format!("failed to map public content ID {public_id}: {e}"))?;
        }
    }
    tx.commit()
        .map_err(|e| format!("commit content seed: {e}"))?;
    Ok(count)
}

struct RemoteSeedAsset {
    element_id: &'static str,
    url: &'static str,
}

const REMOTE_SEED_ASSETS: &[RemoteSeedAsset] = &[
    // ── Videos ──────────────────────────────────────────────────────
    // Short CC0/CC-BY clips that work reliably through the Tauri IPC
    // bridge (~0.3–5 MB each, H.264 MP4). The full Blender films
    // (100+ MB) are too large to pass as number[] through JSON IPC.
    //
    // Sources:
    //   - W3C media test files (public domain / CC BY)
    //   - MDN CC0 sample videos
    //   - Blender Foundation trailers (CC BY 3.0)
    RemoteSeedAsset {
        // Algo: Big Buck Bunny trailer (~2.7 MB, CC BY 3.0)
        element_id: "el_algo_1_4",
        url: "https://media.w3.org/2010/05/bunny/trailer.mp4",
    },
    RemoteSeedAsset {
        // Web: W3C test movie clip (~300 KB, public domain)
        element_id: "el_web_3_2",
        url: "https://media.w3.org/2010/05/video/movie_300.mp4",
    },
    RemoteSeedAsset {
        // ML: MDN flower CC0 sample (~1.4 MB, CC0)
        element_id: "el_ml_1_3",
        url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4",
    },
    RemoteSeedAsset {
        // Crypto: Sintel trailer (~5 MB, CC BY 3.0)
        element_id: "el_cry_1_2",
        url: "https://media.w3.org/2010/05/sintel/trailer.mp4",
    },
    RemoteSeedAsset {
        // UX: MDN friday CC0 sample (~1 MB, CC0)
        element_id: "el_ux_1_3",
        url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/friday.mp4",
    },
    // ── PDFs ────────────────────────────────────────────────────────
    // Varied real-world reference PDFs, all freely distributable.
    RemoteSeedAsset {
        // Web: MDN HTML cheat sheet surrogate — Mozilla PDF reference doc
        element_id: "el_web_1_4",
        url: "https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf",
    },
    RemoteSeedAsset {
        // ML: Bitcoin whitepaper — real, landmark technical paper (public domain)
        element_id: "el_ml_4_4",
        url: "https://bitcoin.org/bitcoin.pdf",
    },
    RemoteSeedAsset {
        // UX: W3C WCAG summary surrogate
        element_id: "el_ux_4_3",
        url: "https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf",
    },
    // ── Downloadable text assets (IETF RFC files, all public domain) ─
    RemoteSeedAsset {
        element_id: "el_web_5_4",
        url: "https://www.rfc-editor.org/rfc/rfc1149.txt",
    },
    RemoteSeedAsset {
        element_id: "el_cry_3_4",
        url: "https://www.rfc-editor.org/rfc/rfc2324.txt",
    },
    RemoteSeedAsset {
        element_id: "el_ux_3_3",
        url: "https://www.rfc-editor.org/rfc/rfc8259.txt",
    },
    // ── Tutorial videos (kind='tutorial' courses from BACKFILL_SQL) ─
    // Reuse reliable W3C + MDN CC-licensed clips. The same URL is fine
    // across multiple tutorials — the cache in seed_content_if_needed
    // dedups by URL so the blob is downloaded once per unique URL.
    RemoteSeedAsset {
        // Big-O in 8 Minutes — Big Buck Bunny trailer as a stand-in
        element_id: "el_tut_bigO_video",
        url: "https://media.w3.org/2010/05/bunny/trailer.mp4",
    },
    RemoteSeedAsset {
        // Async/Await Quick Tour — W3C short test clip
        element_id: "el_tut_asyncawait_video",
        url: "https://media.w3.org/2010/05/video/movie_300.mp4",
    },
    RemoteSeedAsset {
        // Linear Regression from First Principles — MDN flower
        element_id: "el_tut_ml_regression_video",
        url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4",
    },
    RemoteSeedAsset {
        // AES Walkthrough — Sintel trailer
        element_id: "el_tut_aes_video",
        url: "https://media.w3.org/2010/05/sintel/trailer.mp4",
    },
    RemoteSeedAsset {
        // Running a Good User Interview — MDN friday
        element_id: "el_tut_ux_interviews_video",
        url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/friday.mp4",
    },
    // ── Civic Sense course + tutorials (AI-generated example content) ──
    RemoteSeedAsset {
        // Civics ch1 video — "Reading a National Constitution"
        element_id: "el_civ_1_5",
        url: "https://media.w3.org/2010/05/bunny/trailer.mp4",
    },
    RemoteSeedAsset {
        // Civics ch2 PDF — standing in for UDHR text
        element_id: "el_civ_2_5",
        url: "https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf",
    },
    RemoteSeedAsset {
        // Civics tutorial — constitution walkthrough
        element_id: "el_tut_civ_constitution_video",
        url: "https://media.w3.org/2010/05/sintel/trailer.mp4",
    },
    RemoteSeedAsset {
        // Civics tutorial — reading a budget
        element_id: "el_tut_civ_budget_video",
        url: "https://media.w3.org/2010/05/video/movie_300.mp4",
    },
];

// The SEED_CONTENT constant lives in seed_content_data.rs so the CLI
// crate can also include it without pulling in iroh/app_lib dependencies.
include!("seed_content_data.rs");

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(conn: &rusqlite::Connection) {
        conn.execute_batch(
            "INSERT INTO courses (id, title, author_address) VALUES ('seed-course', 'Seed', 'author');
             INSERT INTO course_chapters (id, course_id, title) VALUES ('seed-chapter', 'seed-course', 'Seed');
             INSERT INTO course_elements (id, chapter_id, title, element_type)
             VALUES ('el_first', 'seed-chapter', 'First', 'text'), ('el_second', 'seed-chapter', 'Second', 'video');",
        ).unwrap();
    }

    fn test_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        setup(db.conn());
        db
    }

    fn pending() -> Vec<SeedContent<'static>> {
        vec![
            SeedContent {
                element_id: "el_first",
                hash: "first-hash".into(),
                inline_seed: Some("original body"),
                public_source: None,
            },
            SeedContent {
                element_id: "el_second",
                hash: "second-hash".into(),
                inline_seed: None,
                public_source: Some(("https://example.test/media", 42)),
            },
        ]
    }

    fn assert_rolled_back(conn: &rusqlite::Connection) {
        assert!(
            conn.is_autocommit(),
            "failed seed must not retain a transaction"
        );
        let populated: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM course_elements WHERE content_cid IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mappings: i64 = conn
            .query_row("SELECT COUNT(*) FROM content_mappings", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!((populated, mappings), (0, 0));
    }

    #[test]
    fn content_and_mapping_failures_rollback_and_allow_retry() {
        let db = test_db();
        for trigger in [
            "BEFORE UPDATE ON course_elements WHEN NEW.id = 'el_second'",
            "BEFORE INSERT ON content_mappings",
        ] {
            db.conn().execute_batch(&format!(
                "CREATE TEMP TRIGGER fail_seed {trigger} BEGIN SELECT RAISE(ABORT, 'injected seed failure'); END;"
            )).unwrap();
            assert!(persist_seed_content(db.conn(), &pending())
                .unwrap_err()
                .contains("injected seed failure"));
            assert_rolled_back(db.conn());
            db.conn().execute_batch("DROP TRIGGER fail_seed;").unwrap();
        }
        assert_eq!(persist_seed_content(db.conn(), &pending()).unwrap(), 2);
        assert_eq!(persist_seed_content(db.conn(), &pending()).unwrap(), 0);
        let mapping: (String, i64) = db.conn().query_row(
            "SELECT blake3_hash, size_bytes FROM content_mappings WHERE external_id = 'https://example.test/media'", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(mapping, ("second-hash".into(), 42));
    }

    #[test]
    fn commit_failure_rolls_back_the_entire_seed_batch() {
        let db = test_db();
        db.conn().execute_batch(
            "CREATE TABLE seed_test_parent (id INTEGER PRIMARY KEY);
             CREATE TABLE seed_test_child (parent INTEGER REFERENCES seed_test_parent(id) DEFERRABLE INITIALLY DEFERRED);
             CREATE TEMP TRIGGER fail_commit AFTER INSERT ON content_mappings
             BEGIN INSERT INTO seed_test_child VALUES (7); END;",
        ).unwrap();
        assert!(persist_seed_content(db.conn(), &pending())
            .unwrap_err()
            .contains("commit content seed"));
        assert_rolled_back(db.conn());
        let children: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM seed_test_child", [], |row| row.get(0))
            .unwrap();
        assert_eq!(children, 0);
        db.conn()
            .execute_batch("DROP TRIGGER fail_commit;")
            .unwrap();
        assert_eq!(persist_seed_content(db.conn(), &pending()).unwrap(), 2);
    }

    #[test]
    fn delayed_seed_preserves_edited_or_deleted_content_and_its_mapping() {
        let db = test_db();
        db.conn()
            .execute_batch(
                "UPDATE course_elements SET content_cid = 'author-edit' WHERE id = 'el_second';
             UPDATE course_elements SET content_inline = 'author inline edit' WHERE id = 'el_first';
             INSERT INTO content_mappings (external_id, blake3_hash, size_bytes)
             VALUES ('https://example.test/media', 'author-edit', 5);",
            )
            .unwrap();
        assert_eq!(persist_seed_content(db.conn(), &pending()).unwrap(), 0);
        let preserved: bool = db.conn().query_row(
            "SELECT content_cid IS NULL AND content_inline = 'author inline edit' FROM course_elements WHERE id = 'el_first'", [], |row| row.get(0),
        ).unwrap();
        assert!(preserved);
        let mapping: String = db
            .conn()
            .query_row("SELECT blake3_hash FROM content_mappings", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(mapping, "author-edit");
        db.conn()
            .execute("DELETE FROM course_elements WHERE id = 'el_second'", [])
            .unwrap();
        assert_eq!(persist_seed_content(db.conn(), &pending()).unwrap(), 0);
    }

    #[test]
    fn invalid_mapping_size_rolls_back_preceding_content_updates() {
        let db = test_db();
        let mut batch = pending();
        batch[1].public_source = Some(("https://example.test/media", u64::MAX));
        assert!(persist_seed_content(db.conn(), &batch)
            .unwrap_err()
            .contains("SQLite range"));
        assert_rolled_back(db.conn());
    }

    #[test]
    fn existing_caller_transaction_is_not_committed_or_rolled_back_by_seeding() {
        let db = test_db();
        let caller = db.conn().unchecked_transaction().unwrap();
        caller
            .execute(
                "UPDATE course_elements SET title = 'Caller edit' WHERE id = 'el_first'",
                [],
            )
            .unwrap();
        assert!(persist_seed_content(db.conn(), &pending()).is_err());
        assert!(!db.conn().is_autocommit());
        caller.commit().unwrap();
        let title: String = db
            .conn()
            .query_row(
                "SELECT title FROM course_elements WHERE id = 'el_first'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "Caller edit");
        assert_eq!(persist_seed_content(db.conn(), &pending()).unwrap(), 2);
    }

    #[test]
    fn successful_seed_survives_reopen_without_reapplying() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("content-seed.db");
        let db = Database::open(&path).unwrap();
        db.run_migrations().unwrap();
        setup(db.conn());
        assert_eq!(persist_seed_content(db.conn(), &pending()).unwrap(), 2);
        drop(db);
        let reopened = Database::open(&path).unwrap();
        reopened.run_migrations().unwrap();
        assert_eq!(
            persist_seed_content(reopened.conn(), &pending()).unwrap(),
            0
        );
        let hash: String = reopened
            .conn()
            .query_row(
                "SELECT content_cid FROM course_elements WHERE id = 'el_second'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(hash, "second-hash");
    }
}
