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
    let mut pending: Vec<(&str, String)> = Vec::new();
    let mut mappings: Vec<(&str, String, u64)> = Vec::new();
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

        // Store globally resolvable URL for public media, and keep a local
        // URL->BLAKE3 cache mapping for fast future lookups.
        pending.push((asset.element_id, asset.url.to_string()));
        mappings.push((asset.url, result.hash.clone(), result.size));
    }

    // Phase 2: Single DB write lock — batch-update all rows in a transaction.
    let updated = {
        let guard = db.lock().unwrap();
        let db = guard.as_ref().ok_or("database not initialized")?;
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

        for (public_id, blake3_hash, size) in &mappings {
            conn.execute(
                "INSERT OR REPLACE INTO content_mappings (ipfs_cid, blake3_hash, size_bytes) VALUES (?1, ?2, ?3)",
                rusqlite::params![public_id, blake3_hash, *size as i64],
            )
            .map_err(|e| format!("failed to map public content ID {public_id}: {e}"))?;
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
    // ── Videos ──────────────────────────────────────────────────────
    // Blender Foundation open-movie clips (CC BY), served from Google's
    // public test CDN. Distinct video per topic so the demo isn't monotonous.
    RemoteSeedAsset {
        // Algo: Big Buck Bunny — classic "complexity / performance" vibe
        element_id: "el_algo_1_4",
        url: "https://commondatastorage.googleapis.com/gtv-videos-bucket/sample/BigBuckBunny.mp4",
    },
    RemoteSeedAsset {
        // Web: For Bigger Blazes — short Chromecast demo
        element_id: "el_web_3_2",
        url:
            "https://commondatastorage.googleapis.com/gtv-videos-bucket/sample/ForBiggerBlazes.mp4",
    },
    RemoteSeedAsset {
        // ML: Elephants Dream — surreal, abstract (matches ML "black box" vibe)
        element_id: "el_ml_1_3",
        url: "https://commondatastorage.googleapis.com/gtv-videos-bucket/sample/ElephantsDream.mp4",
    },
    RemoteSeedAsset {
        // Crypto: Sintel — longer, thematic (a tale of obsession & secrets)
        element_id: "el_cry_1_2",
        url: "https://commondatastorage.googleapis.com/gtv-videos-bucket/sample/Sintel.mp4",
    },
    RemoteSeedAsset {
        // UX: Tears of Steel — character-driven (fits user research/personas)
        element_id: "el_ux_1_3",
        url: "https://commondatastorage.googleapis.com/gtv-videos-bucket/sample/TearsOfSteel.mp4",
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
