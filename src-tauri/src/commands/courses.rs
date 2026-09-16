use crate::profile::scope::ProfileState as State;
use rusqlite::{params, OptionalExtension};

use crate::crypto::hash::entity_id;
use crate::db::executor::DatabaseWorkload;
use crate::domain::course::{Course, CreateCourseRequest, UpdateCourseRequest};
use crate::AppState;

use crate::content_store::course as content_course;
use crate::crypto::did::did_from_verifying_key;
use crate::crypto::wallet;
use crate::domain::course_document::{
    CourseCompletionPolicy, CourseDocumentPayload, DocumentChapter, DocumentElement,
    PublishCourseResult,
};
use crate::p2p::catalog;

/// List all courses in the local database.
#[tauri::command]
pub async fn list_courses(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<Course>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "courses.list",
            move |db| list_courses_db(db, status),
        )
        .await
}

fn list_courses_db(
    db: &crate::db::Database,
    status: Option<String>,
) -> Result<Vec<Course>, String> {
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
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "courses.get",
            move |db| get_course_db(db, &course_id),
        )
        .await
}

fn get_course_db(db: &crate::db::Database, course_id: &str) -> Result<Option<Course>, String> {
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
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "courses.create",
            move |db| create_course_db(db, req),
        )
        .await
}

fn create_course_db(db: &crate::db::Database, req: CreateCourseRequest) -> Result<Course, String> {
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

    let kind = match req.kind.as_deref() {
        None | Some("course") => "course",
        Some("tutorial") => "tutorial",
        Some(other) => return Err(format!("unknown course kind '{other}'")),
    };

    db.conn()
        .execute(
            "INSERT INTO courses (id, title, description, author_address, tags, skill_ids, kind) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                req.title,
                req.description,
                author_address,
                tags_json,
                skill_ids_json,
                kind,
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
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "courses.update",
            move |db| update_course_db(db, &course_id, req),
        )
        .await
}

fn update_course_db(
    db: &crate::db::Database,
    course_id: &str,
    req: UpdateCourseRequest,
) -> Result<Course, String> {
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
    values.push(Box::new(course_id.to_owned()));

    let sql = format!("UPDATE courses SET {} WHERE id = ?", set_clauses.join(", "));

    let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();

    let rows = db
        .conn()
        .execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err("course not found".into());
    }

    get_course_by_id(db.conn(), course_id)
}

/// Delete a course.
#[tauri::command]
pub async fn delete_course(state: State<'_, AppState>, course_id: String) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "courses.delete",
            move |db| delete_course_db(db, &course_id),
        )
        .await
}

fn delete_course_db(db: &crate::db::Database, course_id: &str) -> Result<(), String> {
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
/// Reads the course, its chapters, and elements from SQLite, uploads any
/// inline text lessons, builds a CourseDocumentPayload, signs it with the
/// wallet key, stores it on iroh, and updates the course's `content_cid` with
/// the BLAKE3 hash.
///
/// Requires the vault to be unlocked (wallet key needed for signing), and
/// refuses a course the signer did not author.
#[tauri::command]
pub async fn publish_course(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<PublishCourseResult, String> {
    // Publication spans several awaits. Database phases are fenced by profile
    // leases; the studio epoch additionally catches a profile switch between
    // them, so a commit can never land in a different profile than the read.
    let studio_epoch = state.studio.epoch.load(std::sync::atomic::Ordering::SeqCst);
    let profile_unchanged =
        || state.studio.epoch.load(std::sync::atomic::Ordering::SeqCst) == studio_epoch;

    // Get the wallet signing key from the vault
    let keystore = state.keystore.lock().await;
    let ks = keystore.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(keystore);

    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    let author_did = did_from_verifying_key(&w.signing_key.verifying_key());
    let signer_address = w.stake_address.clone();

    // Read course data off the async runtime before iroh calls.
    let read_course_id = course_id.clone();
    let (mut payload, inline_text) = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "courses.publish.read",
            move |db| {
                let course = get_course_by_id(db.conn(), &read_course_id)?;
                if course.author_address != signer_address {
                    return Err("course not found or not authored by you".into());
                }
                let draft_policy_json: Option<String> = db
                    .conn()
                    .query_row(
                        "SELECT draft_completion_policy_json FROM courses WHERE id = ?1",
                        params![read_course_id],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                let completion_policy = draft_policy_json
                    .as_deref()
                    .map(serde_json::from_str::<CourseCompletionPolicy>)
                    .transpose()
                    .map_err(|error| format!("invalid draft completion policy: {error}"))?;
                if let Some(policy) = &completion_policy {
                    policy
                        .validate()
                        .map_err(|error| format!("invalid draft completion policy: {error}"))?;
                }

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
                        .query_map(params![read_course_id], |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                        })
                        .map_err(|e| e.to_string())?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| e.to_string())?;
                    rows
                };

                // Text lessons written in the studio are stored inline and
                // uploaded as blobs before signing, so the signed document
                // references content rather than carrying it.
                let mut inline_text: Vec<(String, String)> = Vec::new();
                let mut chapters = Vec::new();
                for (ch_id, ch_title, ch_desc, ch_pos) in &chapter_rows {
                    let elements: Vec<DocumentElement> = {
                        let mut el_stmt = db
                            .conn()
                            .prepare(
                                "SELECT id, title, element_type, content_cid, position, duration_seconds, content_inline \
                                 FROM course_elements WHERE chapter_id = ?1 ORDER BY position ASC",
                            )
                            .map_err(|e| e.to_string())?;

                        let els = el_stmt
                            .query_map(params![ch_id], |row| {
                                let el_id: String = row.get(0)?;
                                let element_type: String = row.get(2)?;
                                if element_type == "text" {
                                    if let Some(text) = row.get::<_, Option<String>>(6)? {
                                        inline_text.push((el_id.clone(), text));
                                    }
                                }
                                Ok(DocumentElement {
                                    id: el_id,
                                    title: row.get(1)?,
                                    element_type,
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
                let tutor_policy = alexandria_studio::store::tutor_policy(db.conn(), &course.id)
                    .map_err(|error| error.to_string())?;

                Ok((
                    CourseDocumentPayload {
                        version: crate::domain::course_document::COURSE_DOCUMENT_VERSION,
                        course_id: course.id.clone(),
                        author_address: course.author_address.clone(),
                        author_did: Some(author_did),
                        title: course.title.clone(),
                        description: course.description.clone(),
                        thumbnail_hash: course.thumbnail_cid.clone(),
                        tags: course.tags.clone().unwrap_or_default(),
                        skill_ids: course.skill_ids.clone().unwrap_or_default(),
                        chapters,
                        created_at,
                        updated_at,
                        kind: course.kind.clone(),
                        completion_policy,
                        tutor_policy,
                    },
                    inline_text,
                ))
            },
        )
        .await?;

    if !profile_unchanged() {
        return Err("profile changed during publication".into());
    }
    content_course::materialize_text_lessons(&state.content_node, &mut payload, &inline_text)
        .await
        .map_err(|error| error.to_string())?;
    if !profile_unchanged() {
        return Err("profile changed during publication".into());
    }

    // Sign the document
    let signed = content_course::sign_course_document(&payload, &w.signing_key)
        .map_err(|e| e.to_string())?;

    // Publish to iroh
    let result = content_course::publish_course_document(&state.content_node, &signed)
        .await
        .map_err(|e| e.to_string())?;

    if !profile_unchanged() {
        return Err("profile changed during publication".into());
    }

    // Update the course and build the catalog announcement off the runtime.
    let update_course_id = course_id.clone();
    let content_hash = result.content_hash.clone();
    let content_size = result.size;
    let document_version = i64::from(signed.version);
    let completion_policy_json = signed
        .completion_policy
        .as_ref()
        .map(serde_json_canonicalizer::to_string)
        .transpose()
        .map_err(|error| error.to_string())?;
    let (announcement, signed_ann, version) = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "courses.publish.commit",
            move |db| {
                // The signed document carries the lesson text as it was read.
                // If an author edited a lesson while it uploaded, publishing
                // would record a document that no longer matches the draft.
                for (element_id, expected_text) in &inline_text {
                    let matches: bool = db
                        .conn()
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM course_elements e \
                             JOIN course_chapters ch ON ch.id = e.chapter_id \
                             WHERE e.id = ?1 AND ch.course_id = ?2 \
                             AND e.content_inline = ?3 AND e.element_type = 'text')",
                            params![element_id, update_course_id, expected_text],
                            |row| row.get(0),
                        )
                        .map_err(|error| error.to_string())?;
                    if !matches {
                        return Err(
                            "lesson changed during publication; review and publish again".into(),
                        );
                    }
                }

                db.conn()
                    .execute(
                        "UPDATE courses SET content_cid = ?1, course_document_version = ?2, \
                 completion_policy_json = ?3, status = 'published', \
                 version = version + 1, published_at = datetime('now'), \
                 updated_at = datetime('now') WHERE id = ?4",
                        params![
                            content_hash,
                            document_version,
                            completion_policy_json,
                            update_course_id,
                        ],
                    )
                    .map_err(|e| e.to_string())?;

                // Track as a non-evictable pin (authored content)
                crate::content_store::storage::upsert_pin(
                    db.conn(),
                    &content_hash,
                    "course",
                    content_size,
                    false, // auto_unpin = false: authored content is never evicted
                )?;

                // Read back the updated course to get the new version number
                let updated_course = get_course_by_id(db.conn(), &update_course_id)?;
                let version = updated_course.version;

                // Build a catalog announcement for P2P discovery
                let announcement = catalog::build_catalog_announcement(
                    &payload.course_id,
                    &payload.author_address,
                    &payload.title,
                    payload.description.as_deref(),
                    &content_hash,
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

                Ok((announcement, signed_ann, version))
            },
        )
        .await?;

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

/// Update the local draft policy that will be covered by the next signed
/// course publication. Passing `None` clears it. Published documents and
/// existing enrollments remain immutable.
#[tauri::command]
pub async fn set_course_completion_policy(
    state: State<'_, AppState>,
    course_id: String,
    policy: Option<CourseCompletionPolicy>,
) -> Result<(), String> {
    let canonical = policy
        .as_ref()
        .map(|policy| {
            policy.validate().map_err(|error| error.to_string())?;
            serde_json_canonicalizer::to_string(policy).map_err(|error| error.to_string())
        })
        .transpose()?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "courses.completion-policy.update",
            move |db| {
                set_course_completion_policy_impl(db.conn(), &course_id, canonical.as_deref())
            },
        )
        .await
}

/// Read the active author's unpublished policy for the next course version.
#[tauri::command]
pub async fn get_course_completion_policy(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Option<CourseCompletionPolicy>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "courses.completion-policy.read",
            move |db| get_course_completion_policy_impl(db.conn(), &course_id),
        )
        .await
}

fn get_course_completion_policy_impl(
    conn: &rusqlite::Connection,
    course_id: &str,
) -> Result<Option<CourseCompletionPolicy>, String> {
    let row = conn
        .query_row(
            "SELECT draft_completion_policy_json FROM courses \
             WHERE id = ?1 AND author_address = \
               (SELECT stake_address FROM local_identity WHERE id = 1)",
            [course_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or("course not found or active profile is not its author")?;
    row.map(|json| {
        serde_json::from_str(&json)
            .map_err(|error| format!("invalid draft completion policy: {error}"))
    })
    .transpose()
}

fn set_course_completion_policy_impl(
    conn: &rusqlite::Connection,
    course_id: &str,
    canonical_policy: Option<&str>,
) -> Result<(), String> {
    let rows = conn
        .execute(
            "UPDATE courses SET draft_completion_policy_json = ?1, \
             updated_at = datetime('now') \
             WHERE id = ?2 AND author_address = \
               (SELECT stake_address FROM local_identity WHERE id = 1)",
            params![canonical_policy, course_id],
        )
        .map_err(|error| error.to_string())?;
    if rows != 1 {
        return Err("course not found or active profile is not its author".into());
    }
    Ok(())
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
    fn draft_completion_policy_is_author_scoped_and_clearable() {
        let db = test_db();
        setup_identity(&db);
        insert_course(&db, "owned", "Owned", "draft");
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) \
                 VALUES ('foreign', 'Foreign', 'stake_test1uother')",
                [],
            )
            .unwrap();

        set_course_completion_policy_impl(db.conn(), "owned", Some("{}"))
            .expect("owned draft policy");
        assert!(
            get_course_completion_policy_impl(db.conn(), "owned").is_err(),
            "invalid stored policy must fail closed"
        );
        let stored: Option<String> = db
            .conn()
            .query_row(
                "SELECT draft_completion_policy_json FROM courses WHERE id = 'owned'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored.as_deref(), Some("{}"));

        assert!(
            set_course_completion_policy_impl(db.conn(), "foreign", Some("{}"))
                .unwrap_err()
                .contains("not found or active profile is not its author")
        );
        set_course_completion_policy_impl(db.conn(), "owned", None).expect("clear policy");
        assert!(get_course_completion_policy_impl(db.conn(), "owned")
            .expect("read cleared policy")
            .is_none());
        let cleared: Option<String> = db
            .conn()
            .query_row(
                "SELECT draft_completion_policy_json FROM courses WHERE id = 'owned'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(cleared.is_none());
        assert!(get_course_completion_policy_impl(db.conn(), "foreign").is_err());
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
