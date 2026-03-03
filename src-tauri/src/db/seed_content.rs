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

use crate::db::Database;
use crate::ipfs::content;
use crate::ipfs::node::ContentNode;

/// Seed content into iroh for elements that lack a `content_cid`.
/// Returns the number of elements updated, or 0 if skipped.
pub async fn seed_content_if_needed(
    db: &Arc<Mutex<Database>>,
    node: &Arc<ContentNode>,
) -> Result<u32, String> {
    // Find all seed elements that still need content CIDs.
    let needs_seed: HashSet<String> = {
        let db = db.lock().unwrap();
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
    let mut pending: Vec<(&str, String)> = Vec::new();
    for (element_id, body) in SEED_CONTENT {
        if !needs_seed.contains(*element_id) {
            continue;
        }
        let result = content::add_bytes(node, body.as_bytes())
            .await
            .map_err(|e| format!("failed to add content for {element_id}: {e}"))?;
        pending.push((element_id, result.hash.clone()));
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
                .map_err(|e| format!("failed to download {} for {}: {e}", asset.url, asset.element_id))?
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

        let result = content::add_bytes(node, &bytes)
            .await
            .map_err(|e| format!("failed to add downloaded media for {}: {e}", asset.element_id))?;
        pending.push((asset.element_id, result.hash.clone()));
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

struct RemoteSeedAsset {
    element_id: &'static str,
    url: &'static str,
}

const REMOTE_SEED_ASSETS: &[RemoteSeedAsset] = &[
    // Videos (CC0, via MDN interactive examples)
    RemoteSeedAsset {
        element_id: "el_algo_1_4",
        url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4",
    },
    RemoteSeedAsset {
        element_id: "el_web_3_2",
        url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4",
    },
    RemoteSeedAsset {
        element_id: "el_ml_1_3",
        url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4",
    },
    RemoteSeedAsset {
        element_id: "el_cry_1_2",
        url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4",
    },
    RemoteSeedAsset {
        element_id: "el_ux_1_3",
        url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4",
    },
    // PDFs
    RemoteSeedAsset {
        element_id: "el_web_1_4",
        url: "https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf",
    },
    RemoteSeedAsset {
        element_id: "el_ml_4_4",
        url: "https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf",
    },
    RemoteSeedAsset {
        element_id: "el_ux_4_3",
        url: "https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf",
    },
    // Downloadable assets (IETF RFC text files)
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
];

// The SEED_CONTENT constant lives in seed_content_data.rs so the CLI
// crate can also include it without pulling in iroh/app_lib dependencies.
include!("seed_content_data.rs");
