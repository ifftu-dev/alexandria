//! Distributed course catalog — gossip-based course discovery.
//!
//! Handles both sides of the catalog gossip protocol:
//!
//! **Incoming** (receive side): When a validated `GossipMessage` arrives on
//! `/alexandria/catalog/1.0`, deserialize the `CatalogAnnouncement` payload,
//! UPSERT into the local `catalog` table, and record in `sync_log`.
//!
//! **Outgoing** (publish side): When an author publishes a course to iroh,
//! construct a `CatalogAnnouncement`, insert into the local `catalog` table,
//! and broadcast via GossipSub.

use rusqlite::params;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::crypto::hash::entity_id;
use crate::db::Database;
use crate::domain::catalog::CatalogAnnouncement;
use crate::p2p::types::SignedGossipMessage;

/// Handle an incoming catalog announcement from the P2P network.
///
/// Deserializes the gossip message payload as a `CatalogAnnouncement`,
/// validates required fields, and UPSERTs into the local `catalog` table.
/// Higher-version announcements overwrite lower ones (monotonic version).
///
/// Also records the sync event in `sync_log`.
pub fn handle_catalog_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<CatalogAnnouncement, String> {
    // Deserialize the inner payload
    let announcement: CatalogAnnouncement = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("invalid catalog announcement: {e}"))?;

    // Validate required fields
    if announcement.course_id.is_empty() {
        return Err("catalog announcement missing course_id".into());
    }
    if announcement.title.is_empty() {
        return Err("catalog announcement missing title".into());
    }
    if announcement.content_cid.is_empty() {
        return Err("catalog announcement missing content_cid".into());
    }
    if announcement.author_address.is_empty() {
        return Err("catalog announcement missing author_address".into());
    }
    if announcement.author_address != message.stake_address {
        return Err("catalog announcement author does not match envelope signer".into());
    }
    let expected_course_id = entity_id(&[&announcement.author_address, &announcement.content_cid]);
    if announcement.course_id != expected_course_id {
        return Err("catalog announcement has invalid deterministic course_id".into());
    }

    // Check if we already have a newer version
    let existing_version: Option<i64> = db
        .conn()
        .query_row(
            "SELECT version FROM catalog WHERE course_id = ?1",
            params![announcement.course_id],
            |row| row.get(0),
        )
        .ok();

    if let Some(v) = existing_version {
        if v >= announcement.version {
            log::debug!(
                "Skipping catalog announcement for '{}' — local version {} >= received {}",
                announcement.title,
                v,
                announcement.version,
            );
            return Ok(announcement);
        }
    }

    // UPSERT into catalog table
    let tags_json = serde_json::to_string(&announcement.tags).unwrap_or_default();
    let skill_ids_json = serde_json::to_string(&announcement.skill_ids).unwrap_or_default();
    let published_at = chrono::DateTime::from_timestamp(announcement.published_at, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
    let signature_hex = hex::encode(&message.signature);

    db.conn()
        .execute(
            "INSERT INTO catalog (course_id, title, description, author_address, content_cid, \
             thumbnail_cid, tags, skill_ids, version, published_at, signature, kind) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
             ON CONFLICT(course_id) DO UPDATE SET \
             title = excluded.title, \
             description = excluded.description, \
             content_cid = excluded.content_cid, \
             thumbnail_cid = excluded.thumbnail_cid, \
             tags = excluded.tags, \
             skill_ids = excluded.skill_ids, \
             version = excluded.version, \
             published_at = excluded.published_at, \
             received_at = datetime('now'), \
             signature = excluded.signature, \
             kind = excluded.kind",
            params![
                announcement.course_id,
                announcement.title,
                announcement.description,
                announcement.author_address,
                announcement.content_cid,
                announcement.thumbnail_cid,
                tags_json,
                skill_ids_json,
                announcement.version,
                published_at,
                signature_hex,
                announcement.kind,
            ],
        )
        .map_err(|e| format!("failed to upsert catalog entry: {e}"))?;

    // Record in sync_log
    db.conn()
        .execute(
            "INSERT INTO sync_log (entity_type, entity_id, direction, peer_id, signature) \
             VALUES ('catalog', ?1, 'received', ?2, ?3)",
            params![announcement.course_id, message.stake_address, signature_hex,],
        )
        .map_err(|e| format!("failed to record sync_log: {e}"))?;

    log::info!(
        "Catalog: received course '{}' (v{}) from {}",
        announcement.title,
        announcement.version,
        announcement.author_address,
    );

    Ok(announcement)
}

/// Create a `CatalogAnnouncement` from a locally published course.
///
/// Called after `publish_course` stores the course document on iroh.
/// The announcement is a lightweight summary for gossip discovery.
#[allow(clippy::too_many_arguments)]
pub fn build_catalog_announcement(
    author_address: &str,
    title: &str,
    description: Option<&str>,
    content_cid: &str,
    thumbnail_cid: Option<&str>,
    tags: &[String],
    skill_ids: &[String],
    version: i64,
    kind: &str,
) -> CatalogAnnouncement {
    // Spec: course_id = blake2b(author_address + content_cid)
    let course_id = entity_id(&[author_address, content_cid]);

    let published_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    CatalogAnnouncement {
        course_id,
        title: title.to_string(),
        description: description.map(String::from),
        content_cid: content_cid.to_string(),
        author_address: author_address.to_string(),
        thumbnail_cid: thumbnail_cid.map(String::from),
        tags: tags.to_vec(),
        skill_ids: skill_ids.to_vec(),
        version,
        published_at,
        kind: kind.to_string(),
    }
}

/// Insert an author's own course into the local catalog table.
///
/// Called alongside `build_catalog_announcement` so the author's own
/// course appears in their local catalog immediately (without waiting
/// for the gossip round-trip).
pub fn insert_own_catalog_entry(
    db: &Database,
    announcement: &CatalogAnnouncement,
    signature_hex: &str,
) -> Result<(), String> {
    let tags_json = serde_json::to_string(&announcement.tags).unwrap_or_default();
    let skill_ids_json = serde_json::to_string(&announcement.skill_ids).unwrap_or_default();
    let published_at = chrono::DateTime::from_timestamp(announcement.published_at, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    db.conn()
        .execute(
            "INSERT INTO catalog (course_id, title, description, author_address, content_cid, \
             thumbnail_cid, tags, skill_ids, version, published_at, pinned, signature, kind) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12) \
             ON CONFLICT(course_id) DO UPDATE SET \
             title = excluded.title, \
             description = excluded.description, \
             content_cid = excluded.content_cid, \
             thumbnail_cid = excluded.thumbnail_cid, \
             tags = excluded.tags, \
             skill_ids = excluded.skill_ids, \
             version = excluded.version, \
             published_at = excluded.published_at, \
             pinned = 1, \
             signature = excluded.signature, \
             kind = excluded.kind",
            params![
                announcement.course_id,
                announcement.title,
                announcement.description,
                announcement.author_address,
                announcement.content_cid,
                announcement.thumbnail_cid,
                tags_json,
                skill_ids_json,
                announcement.version,
                published_at,
                signature_hex,
                announcement.kind,
            ],
        )
        .map_err(|e| format!("failed to insert own catalog entry: {e}"))?;

    // Record in sync_log
    db.conn()
        .execute(
            "INSERT INTO sync_log (entity_type, entity_id, direction) \
             VALUES ('catalog', ?1, 'sent')",
            params![announcement.course_id],
        )
        .map_err(|e| format!("failed to record sync_log: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn sample_announcement() -> CatalogAnnouncement {
        build_catalog_announcement(
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
            "Algorithm Design",
            Some("An advanced algorithms course"),
            "abc123def456",
            None,
            &["algorithms".into(), "graphs".into()],
            &["skill_graph_traversal".into()],
            1,
            "course",
        )
    }

    #[test]
    fn build_announcement_generates_deterministic_id() {
        let a1 = build_catalog_announcement(
            "stake1u8abc",
            "Test",
            None,
            "cid123",
            None,
            &[],
            &[],
            1,
            "course",
        );
        let a2 = build_catalog_announcement(
            "stake1u8abc",
            "Different Title",
            None,
            "cid123",
            None,
            &[],
            &[],
            1,
            "course",
        );
        // Same author + content_cid → same course_id (title not in ID)
        assert_eq!(a1.course_id, a2.course_id);
    }

    #[test]
    fn different_cids_produce_different_ids() {
        let a1 = build_catalog_announcement(
            "stake1u8abc",
            "Test",
            None,
            "cid123",
            None,
            &[],
            &[],
            1,
            "course",
        );
        let a2 = build_catalog_announcement(
            "stake1u8abc",
            "Test",
            None,
            "cid456",
            None,
            &[],
            &[],
            1,
            "course",
        );
        assert_ne!(a1.course_id, a2.course_id);
    }

    #[test]
    fn insert_own_catalog_entry_works() {
        let db = test_db();
        let ann = sample_announcement();

        insert_own_catalog_entry(&db, &ann, "deadbeef").expect("insert");

        // Verify it's in the catalog table
        let title: String = db
            .conn()
            .query_row(
                "SELECT title FROM catalog WHERE course_id = ?1",
                params![ann.course_id],
                |row| row.get(0),
            )
            .expect("query");
        assert_eq!(title, "Algorithm Design");

        // Verify pinned = 1 (own course)
        let pinned: i64 = db
            .conn()
            .query_row(
                "SELECT pinned FROM catalog WHERE course_id = ?1",
                params![ann.course_id],
                |row| row.get(0),
            )
            .expect("query");
        assert_eq!(pinned, 1);

        // Verify sync_log entry
        let direction: String = db
            .conn()
            .query_row(
                "SELECT direction FROM sync_log WHERE entity_id = ?1",
                params![ann.course_id],
                |row| row.get(0),
            )
            .expect("query");
        assert_eq!(direction, "sent");
    }

    #[test]
    fn handle_catalog_message_inserts_entry() {
        let db = test_db();
        let ann = sample_announcement();
        let payload = serde_json::to_vec(&ann).unwrap();

        let msg = SignedGossipMessage {
            topic: "/alexandria/catalog/1.0".into(),
            payload,
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: ann.author_address.clone(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        };

        let result = handle_catalog_message(&db, &msg);
        assert!(result.is_ok());

        // Verify catalog entry exists
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM catalog WHERE course_id = ?1",
                params![ann.course_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn handle_catalog_message_skips_older_version() {
        let db = test_db();
        let ann = sample_announcement();

        // Insert version 2 first
        let mut ann_v2 = ann.clone();
        ann_v2.version = 2;
        ann_v2.title = "Algorithm Design v2".into();
        insert_own_catalog_entry(&db, &ann_v2, "sig_v2").unwrap();

        // Receive version 1 via gossip — should be skipped
        let payload = serde_json::to_vec(&ann).unwrap();
        let msg = SignedGossipMessage {
            topic: "/alexandria/catalog/1.0".into(),
            payload,
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: ann.author_address.clone(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        };

        handle_catalog_message(&db, &msg).unwrap();

        // Title should still be v2 (not overwritten)
        let title: String = db
            .conn()
            .query_row(
                "SELECT title FROM catalog WHERE course_id = ?1",
                params![ann.course_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "Algorithm Design v2");
    }

    #[test]
    fn handle_catalog_message_updates_newer_version() {
        let db = test_db();
        let ann = sample_announcement();
        insert_own_catalog_entry(&db, &ann, "sig_v1").unwrap();

        // Receive version 2 via gossip — should update
        let mut ann_v2 = ann.clone();
        ann_v2.version = 2;
        ann_v2.title = "Algorithm Design v2".into();
        let payload = serde_json::to_vec(&ann_v2).unwrap();
        let msg = SignedGossipMessage {
            topic: "/alexandria/catalog/1.0".into(),
            payload,
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: ann.author_address.clone(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        };

        handle_catalog_message(&db, &msg).unwrap();

        let title: String = db
            .conn()
            .query_row(
                "SELECT title FROM catalog WHERE course_id = ?1",
                params![ann.course_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "Algorithm Design v2");
    }

    #[test]
    fn handle_catalog_message_rejects_empty_course_id() {
        let db = test_db();
        let mut ann = sample_announcement();
        ann.course_id = String::new();
        let payload = serde_json::to_vec(&ann).unwrap();

        let msg = SignedGossipMessage {
            topic: "/alexandria/catalog/1.0".into(),
            payload,
            signature: vec![],
            public_key: vec![],
            stake_address: String::new(),
            timestamp: 0,
            encrypted: false,
            key_id: None,
        };

        assert!(handle_catalog_message(&db, &msg).is_err());
    }

    #[test]
    fn handle_catalog_message_rejects_mismatched_author() {
        let db = test_db();
        let ann = sample_announcement();
        let payload = serde_json::to_vec(&ann).unwrap();

        let msg = SignedGossipMessage {
            topic: "/alexandria/catalog/1.0".into(),
            payload,
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: "stake_test1someoneelse".into(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        };

        assert!(handle_catalog_message(&db, &msg).is_err());
    }

    #[test]
    fn handle_catalog_message_rejects_invalid_course_id() {
        let db = test_db();
        let mut ann = sample_announcement();
        ann.course_id = "tampered".into();
        let payload = serde_json::to_vec(&ann).unwrap();

        let msg = SignedGossipMessage {
            topic: "/alexandria/catalog/1.0".into(),
            payload,
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: ann.author_address.clone(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        };

        assert!(handle_catalog_message(&db, &msg).is_err());
    }
}
