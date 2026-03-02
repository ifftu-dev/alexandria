//! Seed content for dev/testnet course elements.
//!
//! Contains inline content for all seed elements (HTML for text,
//! JSON for quizzes/MCQs/essays). Also provides
//! `seed_content_if_needed()` which writes content into iroh blobs
//! and populates `content_cid`.

use std::sync::Arc;
use std::sync::Mutex;

use crate::db::Database;
use crate::ipfs::content;
use crate::ipfs::node::ContentNode;

/// Seed content into iroh for elements that lack a `content_cid`.
/// Returns the number of elements updated, or 0 if skipped.
pub async fn seed_content_if_needed(
    db: &Arc<Mutex<Database>>,
    node: &Arc<ContentNode>,
) -> Result<u32, String> {
    // Check if any seed element already has content — if so, skip entirely.
    let needs_seed = {
        let db = db.lock().unwrap();
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM course_elements WHERE id LIKE 'el_%' AND content_cid IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        count == 0
    };

    if !needs_seed {
        log::info!("Seed elements already have content — skipping content seed");
        return Ok(0);
    }

    log::info!("Seeding content blobs for dev/testnet elements…");

    // Phase 1: Add all blobs to iroh WITHOUT holding the DB lock.
    // This is the slow part and must not block other DB consumers.
    let mut pending: Vec<(&str, String)> = Vec::new();
    for (element_id, body) in SEED_CONTENT {
        let result = content::add_bytes(node, body.as_bytes())
            .await
            .map_err(|e| format!("failed to add content for {element_id}: {e}"))?;
        pending.push((element_id, result.hash.clone()));
    }

    // Phase 2: Single DB write lock — batch-update all rows in a transaction.
    let updated = {
        let db = db.lock().unwrap();
        let conn = db.conn();
        conn.execute_batch("BEGIN")
            .map_err(|e| format!("begin tx: {e}"))?;

        let mut count = 0u32;
        for (element_id, hash) in &pending {
            conn.execute(
                "UPDATE course_elements SET content_cid = ?1 WHERE id = ?2",
                rusqlite::params![hash, element_id],
            )
            .map_err(|e| format!("failed to update {element_id}: {e}"))?;
            count += 1;
        }

        conn.execute_batch("COMMIT")
            .map_err(|e| format!("commit tx: {e}"))?;
        count
    };

    log::info!("Seeded content for {updated} elements");
    Ok(updated)
}

// The SEED_CONTENT constant lives in seed_content_data.rs so the CLI
// crate can also include it without pulling in iroh/app_lib dependencies.
include!("seed_content_data.rs");
