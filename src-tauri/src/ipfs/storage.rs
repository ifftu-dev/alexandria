//! Storage management: quota enforcement, pin tracking, and LRU eviction.
//!
//! Content in the iroh blob store is tracked via the `pins` table in SQLite.
//! Each pin records its type, size, last access time, and whether it may be
//! evicted under storage pressure (`auto_unpin`).
//!
//! When a user-configured quota is exceeded, the eviction engine removes
//! content in priority-tier order (cached gateway content first, then
//! unenrolled course content, then completed-enrollment content), using
//! LRU within each tier.

use std::sync::Arc;

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::db::Database;

use super::node::ContentNode;

// ── Pin record ──────────────────────────────────────────────────────────

/// A row from the `pins` table.
#[derive(Debug, Clone)]
pub struct PinRecord {
    pub cid: String,
    pub pin_type: String,
    pub size_bytes: u64,
    pub last_accessed: Option<String>,
    pub auto_unpin: bool,
    pub pinned_at: String,
}

// ── Storage stats ───────────────────────────────────────────────────────

/// Summary of local content storage usage.
#[derive(Debug, Clone, Serialize)]
pub struct StorageStats {
    /// Total bytes tracked in the pins table.
    pub total_pinned_bytes: u64,
    /// User-configured quota in bytes (0 = unlimited).
    pub quota_bytes: u64,
    /// Bytes that *could* be freed by eviction (auto_unpin = 1).
    pub evictable_bytes: u64,
    /// Number of pinned items.
    pub pin_count: u64,
    /// Usage as a percentage of quota, if a quota is set.
    pub usage_percent: Option<f64>,
}

/// Result of an eviction run.
#[derive(Debug, Clone, Serialize)]
pub struct EvictionResult {
    /// Number of blobs removed.
    pub blobs_evicted: u64,
    /// Total bytes freed.
    pub bytes_freed: u64,
}

// ── Settings CRUD ───────────────────────────────────────────────────────

/// Read the storage quota from `app_settings`. Returns 0 (unlimited) on error.
pub fn get_storage_quota(conn: &Connection) -> u64 {
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = 'storage_quota_bytes'",
        [],
        |row| {
            let v: String = row.get(0)?;
            Ok(v.parse::<u64>().unwrap_or(0))
        },
    )
    .unwrap_or(0)
}

/// Persist the storage quota to `app_settings`.
pub fn set_storage_quota(conn: &Connection, bytes: u64) {
    let _ = conn.execute(
        "INSERT OR REPLACE INTO app_settings (key, value, updated_at) \
         VALUES ('storage_quota_bytes', ?1, datetime('now'))",
        params![bytes.to_string()],
    );
}

// ── Pin tracking ────────────────────────────────────────────────────────

/// Insert or update a pin row.
///
/// `pin_type`: one of `cache`, `course`, `evidence`, `profile`, `taxonomy`.
/// `auto_unpin`: if `true`, this content may be evicted under storage pressure.
pub fn upsert_pin(
    conn: &Connection,
    blake3_hash: &str,
    pin_type: &str,
    size_bytes: u64,
    auto_unpin: bool,
) {
    let _ = conn.execute(
        "INSERT INTO pins (cid, pin_type, size_bytes, last_accessed, auto_unpin, pinned_at) \
         VALUES (?1, ?2, ?3, datetime('now'), ?4, datetime('now')) \
         ON CONFLICT(cid) DO UPDATE SET \
           pin_type   = excluded.pin_type, \
           size_bytes = excluded.size_bytes, \
           auto_unpin = excluded.auto_unpin, \
           last_accessed = datetime('now')",
        params![blake3_hash, pin_type, size_bytes as i64, auto_unpin as i32],
    );
}

/// Update `last_accessed` for a pin (called on content reads).
pub fn touch_pin(conn: &Connection, blake3_hash: &str) {
    let _ = conn.execute(
        "UPDATE pins SET last_accessed = datetime('now') WHERE cid = ?1",
        params![blake3_hash],
    );
}

/// Total bytes tracked across all pins.
pub fn total_pinned_bytes(conn: &Connection) -> u64 {
    conn.query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM pins", [], |row| {
        row.get::<_, i64>(0)
    })
    .unwrap_or(0) as u64
}

/// Total bytes that could be freed (auto_unpin = 1).
fn evictable_bytes(conn: &Connection) -> u64 {
    conn.query_row(
        "SELECT COALESCE(SUM(size_bytes), 0) FROM pins WHERE auto_unpin = 1",
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0) as u64
}

/// Number of pinned items.
fn pin_count(conn: &Connection) -> u64 {
    conn.query_row("SELECT COUNT(*) FROM pins", [], |row| row.get::<_, i64>(0))
        .unwrap_or(0) as u64
}

/// Build a `StorageStats` snapshot.
pub fn storage_stats(conn: &Connection) -> StorageStats {
    let total = total_pinned_bytes(conn);
    let quota = get_storage_quota(conn);
    let evictable = evictable_bytes(conn);
    let count = pin_count(conn);
    let usage_percent = if quota > 0 {
        Some((total as f64 / quota as f64) * 100.0)
    } else {
        None
    };
    StorageStats {
        total_pinned_bytes: total,
        quota_bytes: quota,
        evictable_bytes: evictable,
        pin_count: count,
        usage_percent,
    }
}

// ── Eviction ────────────────────────────────────────────────────────────

/// List pins eligible for eviction, ordered by priority (first = evict first).
///
/// Tier 1: cached gateway content (`pin_type = 'cache'`), LRU.
/// Tier 2: course content with no active enrollment, LRU.
/// Tier 3: course content for completed/dropped enrollments, LRU.
///
/// Excludes `auto_unpin = 0` (authored content, profiles, taxonomy).
fn list_evictable_pins(conn: &Connection) -> Vec<PinRecord> {
    let sql = r#"
        SELECT p.cid, p.pin_type, p.size_bytes, p.last_accessed, p.auto_unpin, p.pinned_at,
               CASE
                 WHEN p.pin_type = 'cache' THEN 1
                 WHEN p.pin_type = 'course' AND NOT EXISTS (
                   SELECT 1 FROM enrollments e
                   JOIN courses c ON e.course_id = c.id
                   WHERE (c.content_cid = p.cid OR c.thumbnail_cid = p.cid
                          OR EXISTS (
                            SELECT 1 FROM course_elements ce
                            JOIN course_chapters cc ON ce.chapter_id = cc.id
                            WHERE cc.course_id = c.id AND ce.content_cid = p.cid
                          ))
                     AND e.status = 'active'
                 ) THEN 2
                 ELSE 3
               END AS eviction_tier
        FROM pins p
        WHERE p.auto_unpin = 1
        ORDER BY eviction_tier ASC, p.last_accessed ASC NULLS FIRST
    "#;

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[storage] Failed to prepare eviction query: {e}");
            return Vec::new();
        }
    };

    let rows = stmt
        .query_map([], |row| {
            Ok(PinRecord {
                cid: row.get(0)?,
                pin_type: row.get(1)?,
                size_bytes: row.get::<_, i64>(2)? as u64,
                last_accessed: row.get(3)?,
                auto_unpin: row.get::<_, i32>(4)? != 0,
                pinned_at: row.get(5)?,
            })
        })
        .ok();

    match rows {
        Some(r) => r.filter_map(|r| r.ok()).collect(),
        None => Vec::new(),
    }
}

/// Delete the named tag for a blob so iroh can GC it.
async fn delete_blob_tag(node: &ContentNode, hash_hex: &str) -> Result<(), String> {
    let store = node.store().await.map_err(|e| e.to_string())?;
    let _ = store
        .tags()
        .delete(hash_hex.as_bytes())
        .await
        .map_err(|e| format!("tag delete: {e}"))?;
    Ok(())
}

/// Run eviction if current usage exceeds the configured quota.
///
/// Acquires the DB lock only for short synchronous operations, releasing
/// it before any async iroh calls to avoid holding a `!Send` guard
/// across `.await`.
pub async fn maybe_evict(
    node: &ContentNode,
    db: &Arc<std::sync::Mutex<Database>>,
) -> EvictionResult {
    // Read quota and total — short lock, released immediately
    let (quota, total, evictable) = {
        let Ok(db) = db.lock() else {
            return EvictionResult {
                blobs_evicted: 0,
                bytes_freed: 0,
            };
        };
        (
            get_storage_quota(db.conn()),
            total_pinned_bytes(db.conn()),
            list_evictable_pins(db.conn()),
        )
    };

    if quota == 0 || total <= quota {
        return EvictionResult {
            blobs_evicted: 0,
            bytes_freed: 0,
        };
    }

    let mut bytes_to_free = total - quota;
    let mut blobs_evicted = 0u64;
    let mut bytes_freed = 0u64;

    for pin in &evictable {
        if bytes_to_free == 0 {
            break;
        }

        // Delete iroh tag (async — no DB lock held)
        if let Err(e) = delete_blob_tag(node, &pin.cid).await {
            log::warn!("[storage] Failed to delete tag for {}: {e}", pin.cid);
        }

        // Delete pin row — short lock
        if let Ok(db) = db.lock() {
            let _ = db
                .conn()
                .execute("DELETE FROM pins WHERE cid = ?1", params![pin.cid]);
        }

        let freed = pin.size_bytes.min(bytes_to_free);
        bytes_to_free = bytes_to_free.saturating_sub(pin.size_bytes);
        bytes_freed += freed;
        blobs_evicted += 1;
    }

    if blobs_evicted > 0 {
        log::info!("[storage] Evicted {blobs_evicted} blobs, freed {bytes_freed} bytes");
    }

    EvictionResult {
        blobs_evicted,
        bytes_freed,
    }
}

// ── Backfill ────────────────────────────────────────────────────────────

/// One-time backfill: scan existing content references and insert missing
/// pin rows. Called at startup for users upgrading from versions without
/// pin tracking.
pub fn backfill_pins(conn: &Connection) {
    // Backfill from courses (course documents + thumbnails) authored locally
    let _ = conn.execute(
        "INSERT OR IGNORE INTO pins (cid, pin_type, size_bytes, auto_unpin, pinned_at) \
         SELECT cm.blake3_hash, 'course', COALESCE(cm.size_bytes, 0), 0, datetime('now') \
         FROM courses c \
         JOIN content_mappings cm ON cm.ipfs_cid = c.content_cid \
         JOIN local_identity li ON li.stake_address = c.author_address \
         WHERE cm.blake3_hash IS NOT NULL",
        [],
    );

    // Backfill authored course elements (auto_unpin = 0)
    let _ = conn.execute(
        "INSERT OR IGNORE INTO pins (cid, pin_type, size_bytes, auto_unpin, pinned_at) \
         SELECT cm.blake3_hash, 'course', COALESCE(cm.size_bytes, 0), 0, datetime('now') \
         FROM course_elements ce \
         JOIN course_chapters cc ON ce.chapter_id = cc.id \
         JOIN courses c ON cc.course_id = c.id \
         JOIN local_identity li ON li.stake_address = c.author_address \
         JOIN content_mappings cm ON cm.ipfs_cid = ce.content_cid \
         WHERE cm.blake3_hash IS NOT NULL",
        [],
    );

    // Backfill non-authored content mappings as cache (auto_unpin = 1)
    let _ = conn.execute(
        "INSERT OR IGNORE INTO pins (cid, pin_type, size_bytes, auto_unpin, pinned_at) \
         SELECT cm.blake3_hash, 'cache', COALESCE(cm.size_bytes, 0), 1, datetime('now') \
         FROM content_mappings cm \
         WHERE cm.blake3_hash IS NOT NULL \
           AND cm.blake3_hash NOT IN (SELECT cid FROM pins)",
        [],
    );

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM pins", [], |row| row.get(0))
        .unwrap_or(0);
    if count > 0 {
        log::info!("[storage] Pin table has {count} entries after backfill");
    }
}
