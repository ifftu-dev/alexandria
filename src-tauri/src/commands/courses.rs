use rusqlite::params;
use tauri::State;

use crate::crypto::hash::entity_id;
use crate::domain::course::{
    Course, CreateCourseRequest, PublishTutorialRequest, UpdateCourseRequest,
};
use crate::AppState;

use crate::crypto::wallet;
use crate::domain::course_document::{
    CourseDocumentPayload, DocumentChapter, DocumentElement, PublishCourseResult,
    SignedCourseDocument,
};
use crate::ipfs::course as ipfs_course;
use crate::p2p::catalog;

/// List all courses in the local database.
#[tauri::command]
pub async fn list_courses(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<Course>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(ref s) =
        status
    {
        (
                "SELECT id, title, description, author_address, author_name, content_cid, thumbnail_cid, \
                 thumbnail_svg, tags, skill_ids, version, status, published_at, on_chain_tx, created_at, updated_at, kind, provenance \
                 FROM courses WHERE status = ?1 ORDER BY updated_at DESC"
                    .to_string(),
                vec![Box::new(s.clone())],
            )
    } else {
        (
                "SELECT id, title, description, author_address, author_name, content_cid, thumbnail_cid, \
                 thumbnail_svg, tags, skill_ids, version, status, published_at, on_chain_tx, created_at, updated_at, kind, provenance \
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
            let tags_json: Option<String> = row.get(8)?;
            let skill_ids_json: Option<String> = row.get(9)?;

            Ok(Course {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                author_address: row.get(3)?,
                author_name: row.get(4)?,
                content_cid: row.get(5)?,
                thumbnail_cid: row.get(6)?,
                thumbnail_svg: row.get(7)?,
                tags: tags_json.and_then(|j| serde_json::from_str(&j).ok()),
                skill_ids: skill_ids_json.and_then(|j| serde_json::from_str(&j).ok()),
                version: row.get(10)?,
                status: row.get(11)?,
                published_at: row.get(12)?,
                on_chain_tx: row.get(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
                kind: row
                    .get::<_, Option<String>>(16)?
                    .unwrap_or_else(|| "course".into()),
                provenance: row.get::<_, Option<String>>(17)?,
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
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let result = db.conn().query_row(
        "SELECT id, title, description, author_address, author_name, content_cid, thumbnail_cid, \
         thumbnail_svg, tags, skill_ids, version, status, published_at, on_chain_tx, created_at, updated_at, kind, provenance \
         FROM courses WHERE id = ?1",
        params![course_id],
        |row| {
            let tags_json: Option<String> = row.get(8)?;
            let skill_ids_json: Option<String> = row.get(9)?;

            Ok(Course {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                author_address: row.get(3)?,
                author_name: row.get(4)?,
                content_cid: row.get(5)?,
                thumbnail_cid: row.get(6)?,
                thumbnail_svg: row.get(7)?,
                tags: tags_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                skill_ids: skill_ids_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                version: row.get(10)?,
                status: row.get(11)?,
                published_at: row.get(12)?,
                on_chain_tx: row.get(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
                kind: row.get::<_, Option<String>>(16)?.unwrap_or_else(|| "course".into()),
                provenance: row.get::<_, Option<String>>(17)?,
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
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

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
    let id = entity_id(&[
        &author_address,
        &req.title,
        &chrono::Utc::now().to_rfc3339(),
    ]);

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
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

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

    let sql = format!("UPDATE courses SET {} WHERE id = ?", set_clauses.join(", "));

    let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();

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
pub async fn delete_course(state: State<'_, AppState>, course_id: String) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

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
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
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

            let rows = stmt
                .query_map(params![course_id], |row| {
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
                        let el_id: String = row.get(0)?;
                        Ok(DocumentElement {
                            id: el_id,
                            title: row.get(1)?,
                            element_type: row.get(2)?,
                            content_hash: row.get(3)?,
                            position: row.get(4)?,
                            duration_seconds: row.get(5)?,
                            // video_chapters are joined in below after the
                            // element list is materialised, to keep the row
                            // closure free of outer borrows.
                            video_chapters: Vec::new(),
                        })
                    })
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;

                // Load chapter markers for any video elements in this
                // chapter. Small N — a chapter rarely has more than a
                // handful of videos, so a per-element query is fine.
                let mut els = els;
                for el in els.iter_mut() {
                    if el.element_type != "video" {
                        continue;
                    }
                    let mut vc_stmt = db
                        .conn()
                        .prepare(
                            "SELECT title, start_seconds, position \
                             FROM video_chapters WHERE element_id = ?1 \
                             ORDER BY position ASC",
                        )
                        .map_err(|e| e.to_string())?;
                    let vcs: Vec<_> = vc_stmt
                        .query_map(params![el.id], |row| {
                            Ok(crate::domain::course_document::VideoChapter {
                                title: row.get(0)?,
                                start_seconds: row.get(1)?,
                                position: row.get(2)?,
                            })
                        })
                        .map_err(|e| e.to_string())?
                        .filter_map(|r| r.ok())
                        .collect();
                    el.video_chapters = vcs;
                }
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
            kind: course.kind.clone(),
        }
        // db lock dropped here
    };

    // Sign the document
    let signed =
        ipfs_course::sign_course_document(&payload, &w.signing_key).map_err(|e| e.to_string())?;

    // Publish to iroh
    let result = ipfs_course::publish_course_document(&state.content_node, &signed)
        .await
        .map_err(|e| e.to_string())?;

    // Update the course in the database and build catalog announcement
    let (announcement, signed_ann, version) = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .execute(
                "UPDATE courses SET content_cid = ?1, status = 'published', \
                 version = version + 1, published_at = datetime('now'), \
                 updated_at = datetime('now') WHERE id = ?2",
                params![result.content_hash, course_id],
            )
            .map_err(|e| e.to_string())?;

        // Track as a non-evictable pin (authored content)
        crate::ipfs::storage::upsert_pin(
            db.conn(),
            &result.content_hash,
            "course",
            result.size,
            false, // auto_unpin = false: authored content is never evicted
        );

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
            &payload.kind,
        );

        // Sign the announcement payload to get the signature for the catalog entry
        let ann_json = serde_json::to_vec(&announcement).map_err(|e| e.to_string())?;
        let signed_ann = crate::p2p::signing::sign_gossip_message(
            crate::p2p::types::TOPIC_CATALOG,
            ann_json,
            &w.signing_key,
            &w.stake_address,
        );
        let signature_hex = hex::encode(&signed_ann.signature);

        // Insert into local catalog table (author's own course, pinned=1)
        catalog::insert_own_catalog_entry(db, &announcement, &signature_hex)
            .map_err(|e| format!("catalog insert: {e}"))?;

        (announcement, signed_ann, version)
    }; // db guard dropped here — before any .await

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

/// Publish a standalone video tutorial.
///
/// Structurally, a tutorial is a minimal course: one synthetic chapter
/// containing a video element, optionally followed by an end-of-video
/// quiz. By reusing the course model we inherit the entire P2P,
/// evidence, pin-lifecycle, and signing pipeline for free — the only
/// difference is the `kind='tutorial'` discriminator on `courses` and
/// `catalog`, which the UI uses to route/label and which the evidence
/// pipeline uses (via migration 020) to apply a lower `trust_factor`
/// than full courses earn.
///
/// Workflow:
///   1. Insert a `courses` row with `kind='tutorial'`.
///   2. Insert one hidden chapter at position 0.
///   3. Insert the video element (`element_type='video'`) pointing at
///      `video_content_hash`.
///   4. If `quiz` is present, insert a second element of
///      `element_type='quiz'` with `content_inline=<quiz json>`.
///   5. Insert `element_skill_tags` for every requested skill
///      (applied to every authored element so watching + quiz both
///      count toward the same skills).
///   6. Insert `video_chapters` rows, if provided.
///   7. Delegate to `publish_course` to sign, publish to iroh,
///      broadcast on `TOPIC_CATALOG`, and register non-evictable pins.
///
/// Requires the vault to be unlocked (the publish step needs the
/// signing key).
#[tauri::command]
pub async fn publish_tutorial(
    state: State<'_, AppState>,
    req: PublishTutorialRequest,
) -> Result<PublishCourseResult, String> {
    if req.title.trim().is_empty() {
        return Err("tutorial title must not be empty".into());
    }
    if req.video_content_hash.trim().is_empty() {
        return Err("video_content_hash is required".into());
    }
    if req.skill_tags.is_empty() {
        return Err(
            "at least one skill tag is required — a tutorial without a skill is just a video"
                .into(),
        );
    }

    // Step 1–5 & 7: write the minimal course/chapter/element rows
    // inside a single DB scope, then release the lock before the
    // async `publish_course` call which acquires it again and also
    // touches iroh/gossip (which must not hold the DB mutex).
    let course_id: String = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();

        let author_address: String = conn
            .query_row(
                "SELECT stake_address FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("no identity found — generate a wallet first: {e}"))?;

        // Deterministic IDs — the course_id depends on the video
        // hash (not a timestamp) so re-publishing the exact same
        // video is idempotent.
        let course_id = entity_id(&[&author_address, &req.video_content_hash, &req.title]);
        let chapter_id = entity_id(&[&course_id, "ch_0"]);
        let video_element_id = entity_id(&[&course_id, "el_video"]);
        let quiz_element_id = entity_id(&[&course_id, "el_quiz"]);

        let tags_json = serde_json::to_string(&req.tags).unwrap_or_else(|_| "[]".into());
        let skill_ids: Vec<&String> = req.skill_tags.iter().map(|t| &t.skill_id).collect();
        let skill_ids_json = serde_json::to_string(&skill_ids).unwrap_or_else(|_| "[]".into());

        // 1. course (kind='tutorial')
        conn.execute(
            "INSERT INTO courses (id, title, description, author_address, thumbnail_cid, \
             tags, skill_ids, status, kind) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'draft', 'tutorial')",
            params![
                course_id,
                req.title,
                req.description,
                author_address,
                req.thumbnail_hash,
                tags_json,
                skill_ids_json,
            ],
        )
        .map_err(|e| format!("insert tutorial course row: {e}"))?;

        // 2. synthetic chapter
        conn.execute(
            "INSERT INTO course_chapters (id, course_id, title, description, position) \
             VALUES (?1, ?2, ?3, NULL, 0)",
            params![chapter_id, course_id, req.title],
        )
        .map_err(|e| format!("insert tutorial chapter: {e}"))?;

        // 3. video element
        conn.execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, \
             content_cid, position, duration_seconds) \
             VALUES (?1, ?2, ?3, 'video', ?4, 0, ?5)",
            params![
                video_element_id,
                chapter_id,
                req.title,
                req.video_content_hash,
                req.duration_seconds,
            ],
        )
        .map_err(|e| format!("insert tutorial video element: {e}"))?;

        // 4. optional quiz element
        if let Some(quiz) = &req.quiz {
            conn.execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, \
                 content_inline, position) \
                 VALUES (?1, ?2, ?3, 'quiz', ?4, 1)",
                params![
                    quiz_element_id,
                    chapter_id,
                    format!("{} — Check", req.title),
                    quiz.content_json,
                ],
            )
            .map_err(|e| format!("insert tutorial quiz element: {e}"))?;
        }

        // 5. skill tags — applied to the video element AND the quiz
        // (when present) so both paths feed evidence correctly.
        for tag in &req.skill_tags {
            let weight = tag.weight.unwrap_or(1.0);
            conn.execute(
                "INSERT INTO element_skill_tags (element_id, skill_id, weight) \
                 VALUES (?1, ?2, ?3)",
                params![video_element_id, tag.skill_id, weight],
            )
            .map_err(|e| format!("tag video element with skill {}: {e}", tag.skill_id))?;
            if req.quiz.is_some() {
                conn.execute(
                    "INSERT INTO element_skill_tags (element_id, skill_id, weight) \
                     VALUES (?1, ?2, ?3)",
                    params![quiz_element_id, tag.skill_id, weight],
                )
                .map_err(|e| format!("tag quiz element with skill {}: {e}", tag.skill_id))?;
            }
        }

        // 7. video chapters (timestamp navigation)
        for (idx, vc) in req.video_chapters.iter().enumerate() {
            let vc_id = entity_id(&[&video_element_id, &idx.to_string()]);
            conn.execute(
                "INSERT INTO video_chapters (id, element_id, title, start_seconds, position) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    vc_id,
                    video_element_id,
                    vc.title,
                    vc.start_seconds,
                    idx as i64,
                ],
            )
            .map_err(|e| format!("insert video chapter: {e}"))?;
        }

        course_id
    };

    // 8. delegate to publish_course for the signing / iroh / gossip work.
    publish_course(state, course_id).await
}

/// Parse a SQLite datetime string to a Unix timestamp.
/// Falls back to current time if parsing fails.
fn parse_datetime_to_unix(datetime_str: &str) -> i64 {
    chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or_else(|_| chrono::Utc::now().timestamp())
}

/// Internal helper: fetch a course by ID from the connection.
fn get_course_by_id(conn: &rusqlite::Connection, id: &str) -> Result<Course, String> {
    conn.query_row(
        "SELECT id, title, description, author_address, author_name, content_cid, thumbnail_cid, \
         thumbnail_svg, tags, skill_ids, version, status, published_at, on_chain_tx, created_at, updated_at, kind, provenance \
         FROM courses WHERE id = ?1",
        params![id],
        |row| {
            let tags_json: Option<String> = row.get(8)?;
            let skill_ids_json: Option<String> = row.get(9)?;

            Ok(Course {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                author_address: row.get(3)?,
                author_name: row.get(4)?,
                content_cid: row.get(5)?,
                thumbnail_cid: row.get(6)?,
                thumbnail_svg: row.get(7)?,
                tags: tags_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                skill_ids: skill_ids_json
                    .and_then(|j| serde_json::from_str(&j).ok()),
                version: row.get(10)?,
                status: row.get(11)?,
                published_at: row.get(12)?,
                on_chain_tx: row.get(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
                kind: row.get::<_, Option<String>>(16)?.unwrap_or_else(|| "course".into()),
                provenance: row.get::<_, Option<String>>(17)?,
            })
        },
    )
    .map_err(|e| e.to_string())
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
