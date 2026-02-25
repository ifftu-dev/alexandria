use rusqlite::params;
use tauri::State;

use crate::crypto::hash::entity_id;
use crate::domain::course::{Course, CreateCourseRequest, UpdateCourseRequest};
use crate::AppState;

#[cfg(desktop)]
use crate::crypto::wallet;
#[cfg(desktop)]
use crate::domain::course_document::{
    CourseDocumentPayload, DocumentChapter, DocumentElement, PublishCourseResult,
    SignedCourseDocument,
};
#[cfg(desktop)]
use crate::ipfs::course as ipfs_course;
#[cfg(desktop)]
use crate::p2p::catalog;

/// List all courses in the local database.
#[tauri::command]
pub async fn list_courses(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<Course>, String> {
    let db = state.db.lock().await;

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(ref s) = status {
            (
                "SELECT id, title, description, author_address, content_cid, thumbnail_cid, \
                 tags, skill_ids, version, status, published_at, on_chain_tx, created_at, updated_at \
                 FROM courses WHERE status = ?1 ORDER BY updated_at DESC"
                    .to_string(),
                vec![Box::new(s.clone())],
            )
        } else {
            (
                "SELECT id, title, description, author_address, content_cid, thumbnail_cid, \
                 tags, skill_ids, version, status, published_at, on_chain_tx, created_at, updated_at \
                 FROM courses ORDER BY updated_at DESC"
                    .to_string(),
                vec![],
            )
        };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = db.conn().prepare(&sql).map_err(|e| e.to_string())?;

    let courses = stmt
        .query_map(params_ref.as_slice(), |row| {
            let tags_json: Option<String> = row.get(6)?;
            let skill_ids_json: Option<String> = row.get(7)?;

            Ok(Course {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                author_address: row.get(3)?,
                content_cid: row.get(4)?,
                thumbnail_cid: row.get(5)?,
                tags: tags_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                skill_ids: skill_ids_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                version: row.get(8)?,
                status: row.get(9)?,
                published_at: row.get(10)?,
                on_chain_tx: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(courses)
}

/// Get a single course by ID.
#[tauri::command]
pub async fn get_course(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Option<Course>, String> {
    let db = state.db.lock().await;

    let result = db.conn().query_row(
        "SELECT id, title, description, author_address, content_cid, thumbnail_cid, \
         tags, skill_ids, version, status, published_at, on_chain_tx, created_at, updated_at \
         FROM courses WHERE id = ?1",
        params![course_id],
        |row| {
            let tags_json: Option<String> = row.get(6)?;
            let skill_ids_json: Option<String> = row.get(7)?;

            Ok(Course {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                author_address: row.get(3)?,
                content_cid: row.get(4)?,
                thumbnail_cid: row.get(5)?,
                tags: tags_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                skill_ids: skill_ids_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                version: row.get(8)?,
                status: row.get(9)?,
                published_at: row.get(10)?,
                on_chain_tx: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        },
    );

    match result {
        Ok(course) => Ok(Some(course)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Create a new course (authored by the local user).
#[tauri::command]
pub async fn create_course(
    state: State<'_, AppState>,
    req: CreateCourseRequest,
) -> Result<Course, String> {
    let db = state.db.lock().await;

    // Get the local user's stake address
    let author_address: String = db
        .conn()
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no identity found — generate a wallet first: {}", e))?;

    // Generate deterministic ID
    let id = entity_id(&[&author_address, &req.title, &chrono::Utc::now().to_rfc3339()]);

    let tags_json = req.tags.as_ref().map(|t| serde_json::to_string(t).unwrap());
    let skill_ids_json = req
        .skill_ids
        .as_ref()
        .map(|s| serde_json::to_string(s).unwrap());

    db.conn()
        .execute(
            "INSERT INTO courses (id, title, description, author_address, tags, skill_ids) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                req.title,
                req.description,
                author_address,
                tags_json,
                skill_ids_json,
            ],
        )
        .map_err(|e| e.to_string())?;

    // Return the created course
    get_course_by_id(db.conn(), &id)
}

/// Update an existing course.
#[tauri::command]
pub async fn update_course(
    state: State<'_, AppState>,
    course_id: String,
    req: UpdateCourseRequest,
) -> Result<Course, String> {
    let db = state.db.lock().await;

    let mut set_clauses = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref title) = req.title {
        set_clauses.push("title = ?");
        values.push(Box::new(title.clone()));
    }
    if let Some(ref desc) = req.description {
        set_clauses.push("description = ?");
        values.push(Box::new(desc.clone()));
    }
    if let Some(ref tags) = req.tags {
        set_clauses.push("tags = ?");
        values.push(Box::new(serde_json::to_string(tags).unwrap()));
    }
    if let Some(ref skill_ids) = req.skill_ids {
        set_clauses.push("skill_ids = ?");
        values.push(Box::new(serde_json::to_string(skill_ids).unwrap()));
    }
    if let Some(ref status) = req.status {
        set_clauses.push("status = ?");
        values.push(Box::new(status.clone()));
        if status == "published" {
            set_clauses.push("published_at = datetime('now')");
        }
    }

    if set_clauses.is_empty() {
        return Err("no fields to update".into());
    }

    set_clauses.push("updated_at = datetime('now')");
    values.push(Box::new(course_id.clone()));

    let sql = format!(
        "UPDATE courses SET {} WHERE id = ?",
        set_clauses.join(", ")
    );

    let params: Vec<&dyn rusqlite::types::ToSql> =
        values.iter().map(|v| v.as_ref()).collect();

    let rows = db
        .conn()
        .execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err("course not found".into());
    }

    get_course_by_id(db.conn(), &course_id)
}

/// Delete a course.
#[tauri::command]
pub async fn delete_course(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<(), String> {
    let db = state.db.lock().await;

    let rows = db
        .conn()
        .execute("DELETE FROM courses WHERE id = ?1", params![course_id])
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err("course not found".into());
    }

    Ok(())
}

/// Publish a course to the iroh blob store.
///
/// Reads the course, its chapters, and elements from SQLite, builds
/// a CourseDocumentPayload, signs it with the wallet key, stores it
/// on iroh, and updates the course's `content_cid` with the BLAKE3 hash.
///
/// Requires the vault to be unlocked (wallet key needed for signing).
#[cfg(desktop)]
#[tauri::command]
pub async fn publish_course(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<PublishCourseResult, String> {
    // Get the wallet signing key from the vault
    let keystore = state.keystore.lock().await;
    let ks = keystore.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(keystore);

    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;

    // Read course data from DB (scoped to release the lock before iroh calls)
    let payload = {
        let db = state.db.lock().await;
        let course = get_course_by_id(db.conn(), &course_id)?;

        // Read chapters with their elements
        let chapter_rows: Vec<(String, String, Option<String>, i64)> = {
            let mut stmt = db
                .conn()
                .prepare(
                    "SELECT id, title, description, position \
                     FROM course_chapters WHERE course_id = ?1 ORDER BY position ASC",
                )
                .map_err(|e| e.to_string())?;

            let rows = stmt.query_map(params![course_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
            rows
        };

        let mut chapters = Vec::new();
        for (ch_id, ch_title, ch_desc, ch_pos) in &chapter_rows {
            let elements: Vec<DocumentElement> = {
                let mut el_stmt = db
                    .conn()
                    .prepare(
                        "SELECT id, title, element_type, content_cid, position, duration_seconds \
                         FROM course_elements WHERE chapter_id = ?1 ORDER BY position ASC",
                    )
                    .map_err(|e| e.to_string())?;

                let els = el_stmt
                    .query_map(params![ch_id], |row| {
                        Ok(DocumentElement {
                            id: row.get(0)?,
                            title: row.get(1)?,
                            element_type: row.get(2)?,
                            content_hash: row.get(3)?,
                            position: row.get(4)?,
                            duration_seconds: row.get(5)?,
                        })
                    })
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                els
            };

            chapters.push(DocumentChapter {
                id: ch_id.clone(),
                position: *ch_pos,
                title: ch_title.clone(),
                description: ch_desc.clone(),
                elements,
            });
        }

        let created_at = parse_datetime_to_unix(&course.created_at);
        let updated_at = chrono::Utc::now().timestamp();

        CourseDocumentPayload {
            version: 1,
            course_id: course.id.clone(),
            author_address: course.author_address.clone(),
            title: course.title.clone(),
            description: course.description.clone(),
            thumbnail_hash: course.thumbnail_cid.clone(),
            tags: course.tags.clone().unwrap_or_default(),
            skill_ids: course.skill_ids.clone().unwrap_or_default(),
            chapters,
            created_at,
            updated_at,
        }
        // db lock dropped here
    };

    // Sign the document
    let signed = ipfs_course::sign_course_document(&payload, &w.signing_key)
        .map_err(|e| e.to_string())?;

    // Publish to iroh
    let result = ipfs_course::publish_course_document(&state.content_node, &signed)
        .await
        .map_err(|e| e.to_string())?;

    // Update the course in the database
    let db = state.db.lock().await;
    db.conn()
        .execute(
            "UPDATE courses SET content_cid = ?1, status = 'published', \
             version = version + 1, published_at = datetime('now'), \
             updated_at = datetime('now') WHERE id = ?2",
            params![result.content_hash, course_id],
        )
        .map_err(|e| e.to_string())?;

    // Track the pin
    db.conn()
        .execute(
            "INSERT OR REPLACE INTO pins (cid, pin_type, size_bytes, last_accessed, auto_unpin) \
             VALUES (?1, 'course', ?2, datetime('now'), 0)",
            params![result.content_hash, result.size as i64],
        )
        .map_err(|e| e.to_string())?;

    // Read back the updated course to get the new version number
    let updated_course = get_course_by_id(db.conn(), &course_id)?;
    let version = updated_course.version;

    // Build a catalog announcement for P2P discovery
    let announcement = catalog::build_catalog_announcement(
        &payload.author_address,
        &payload.title,
        payload.description.as_deref(),
        &result.content_hash,
        payload.thumbnail_hash.as_deref(),
        &payload.tags,
        &payload.skill_ids,
        version,
    );

    // Sign the announcement payload to get the signature for the catalog entry
    let ann_json = serde_json::to_vec(&announcement).map_err(|e| e.to_string())?;
    let signed_ann =
        crate::p2p::signing::sign_gossip_message(
            crate::p2p::types::TOPIC_CATALOG,
            ann_json,
            &w.signing_key,
            &w.stake_address,
        );
    let signature_hex = hex::encode(&signed_ann.signature);

    // Insert into local catalog table (author's own course, pinned=1)
    catalog::insert_own_catalog_entry(&db, &announcement, &signature_hex)
        .map_err(|e| format!("catalog insert: {e}"))?;

    // Release DB lock before P2P publish (which is async and may take time)
    drop(db);

    // Broadcast via P2P if the node is running (best-effort — don't fail publish)
    let p2p_node = state.p2p_node.lock().await;
    if let Some(ref node) = *p2p_node {
        if let Err(e) = node.publish_signed(&signed_ann).await {
            log::warn!("Failed to broadcast catalog announcement via P2P: {e}");
        } else {
            log::info!(
                "Broadcast catalog announcement for '{}' (v{version})",
                announcement.title,
            );
        }
    }

    Ok(result)
}

/// Fetch and verify a course document by identifier (BLAKE3 hash or IPFS CID).
///
/// Resolves the content, deserializes the signed JSON document,
/// verifies the author's Ed25519 signature, and returns the verified
/// course document.
#[cfg(desktop)]
#[tauri::command]
pub async fn fetch_course_document(
    state: State<'_, AppState>,
    identifier: String,
) -> Result<SignedCourseDocument, String> {
    // Try local iroh first
    ipfs_course::resolve_course_document(&state.content_node, &identifier)
        .await
        .map_err(|e| e.to_string())
}

/// Parse a SQLite datetime string to a Unix timestamp.
/// Falls back to current time if parsing fails.
#[cfg(desktop)]
fn parse_datetime_to_unix(datetime_str: &str) -> i64 {
    chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or_else(|_| chrono::Utc::now().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn setup_identity(db: &Database) {
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1uauthor', 'addr_test1q123')",
                [],
            )
            .unwrap();
    }

    fn insert_course(db: &Database, id: &str, title: &str, status: &str) {
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address, status) \
                 VALUES (?1, ?2, 'stake_test1uauthor', ?3)",
                params![id, title, status],
            )
            .unwrap();
    }

    #[test]
    fn get_course_by_id_returns_course() {
        let db = test_db();
        setup_identity(&db);
        insert_course(&db, "c1", "Test Course", "draft");

        let course = get_course_by_id(db.conn(), "c1").unwrap();
        assert_eq!(course.title, "Test Course");
        assert_eq!(course.status, "draft");
        assert_eq!(course.version, 1);
    }

    #[test]
    fn get_course_by_id_not_found() {
        let db = test_db();
        let result = get_course_by_id(db.conn(), "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn course_list_with_status_filter() {
        let db = test_db();
        setup_identity(&db);
        insert_course(&db, "c1", "Draft Course", "draft");
        insert_course(&db, "c2", "Published Course", "published");
        insert_course(&db, "c3", "Another Draft", "draft");

        // Count drafts
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM courses WHERE status = 'draft'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        // Count published
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM courses WHERE status = 'published'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn course_crud_lifecycle() {
        let db = test_db();
        setup_identity(&db);

        // Create
        let id = entity_id(&["stake_test1uauthor", "Test", "2025"]);
        let tags = serde_json::to_string(&vec!["rust", "programming"]).unwrap();
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, description, author_address, tags) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, "Test", "A test course", "stake_test1uauthor", tags],
            )
            .unwrap();

        let course = get_course_by_id(db.conn(), &id).unwrap();
        assert_eq!(course.title, "Test");
        assert_eq!(course.tags.unwrap(), vec!["rust", "programming"]);

        // Update
        db.conn()
            .execute(
                "UPDATE courses SET title = 'Updated', updated_at = datetime('now') WHERE id = ?1",
                params![id],
            )
            .unwrap();
        let updated = get_course_by_id(db.conn(), &id).unwrap();
        assert_eq!(updated.title, "Updated");

        // Delete
        let rows = db
            .conn()
            .execute("DELETE FROM courses WHERE id = ?1", params![id])
            .unwrap();
        assert_eq!(rows, 1);
        assert!(get_course_by_id(db.conn(), &id).is_err());
    }

    #[test]
    fn course_json_columns_null_handling() {
        let db = test_db();
        setup_identity(&db);
        insert_course(&db, "c1", "No Tags", "draft");

        let course = get_course_by_id(db.conn(), "c1").unwrap();
        assert!(course.tags.is_none());
        assert!(course.skill_ids.is_none());
        assert!(course.content_cid.is_none());
    }
}

/// Internal helper: fetch a course by ID from the connection.
fn get_course_by_id(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<Course, String> {
    conn.query_row(
        "SELECT id, title, description, author_address, content_cid, thumbnail_cid, \
         tags, skill_ids, version, status, published_at, on_chain_tx, created_at, updated_at \
         FROM courses WHERE id = ?1",
        params![id],
        |row| {
            let tags_json: Option<String> = row.get(6)?;
            let skill_ids_json: Option<String> = row.get(7)?;

            Ok(Course {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                author_address: row.get(3)?,
                content_cid: row.get(4)?,
                thumbnail_cid: row.get(5)?,
                tags: tags_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                skill_ids: skill_ids_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                version: row.get(8)?,
                status: row.get(9)?,
                published_at: row.get(10)?,
                on_chain_tx: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}
