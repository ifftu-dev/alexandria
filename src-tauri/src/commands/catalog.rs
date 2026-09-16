//! IPC commands for the distributed course catalog.
//!
//! The catalog table contains course announcements received from the
//! P2P network (and locally published courses). These commands let the
//! frontend search and browse the catalog.

use crate::profile::scope::ProfileState as State;
use rusqlite::params;

use crate::content_store::course as content_course;
use crate::db::executor::DatabaseWorkload;
use crate::domain::catalog::CatalogEntry;
use crate::domain::course_document::SignedCourseDocument;
use crate::AppState;

/// Search the catalog by text query (title, description, tags).
///
/// Performs a case-insensitive LIKE search across title, description,
/// and JSON-encoded tags. Returns up to `limit` results (default 50).
#[tauri::command]
pub async fn search_catalog(
    state: State<'_, AppState>,
    query: Option<String>,
    author: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<CatalogEntry>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "catalog.search",
            move |db| search_catalog_db(db, query, author, limit),
        )
        .await
}

fn search_catalog_db(
    db: &crate::db::Database,
    query: Option<String>,
    author: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<CatalogEntry>, String> {
    let max = limit.unwrap_or(50).min(200) as usize;
    let mut conditions: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref q) = query {
        conditions.push(format!(
            "(title LIKE ?{idx} OR description LIKE ?{idx} OR tags LIKE ?{idx})"
        ));
        param_values.push(Box::new(format!("%{q}%")));
        idx += 1;
    }
    if let Some(ref a) = author {
        conditions.push(format!("author_address = ?{idx}"));
        param_values.push(Box::new(a.clone()));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT course_id, title, description, author_address, content_cid, \
         thumbnail_cid, tags, skill_ids, version, published_at, received_at, \
         pinned, on_chain_tx, kind FROM catalog {where_clause} \
         ORDER BY published_at DESC LIMIT {max}"
    );
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|value| value.as_ref()).collect();
    let mut stmt = db.conn().prepare(&sql).map_err(|error| error.to_string())?;
    let entries = stmt
        .query_map(params_ref.as_slice(), |row| {
            let tags_json: Option<String> = row.get(6)?;
            let skill_ids_json: Option<String> = row.get(7)?;
            Ok(CatalogEntry {
                course_id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                author_address: row.get(3)?,
                content_cid: row.get(4)?,
                thumbnail_cid: row.get(5)?,
                tags: tags_json.and_then(|json| serde_json::from_str(&json).ok()),
                skill_ids: skill_ids_json.and_then(|json| serde_json::from_str(&json).ok()),
                version: row.get(8)?,
                published_at: row.get(9)?,
                received_at: row.get(10)?,
                pinned: row.get::<_, i64>(11)? != 0,
                on_chain_tx: row.get(12)?,
                kind: row
                    .get::<_, Option<String>>(13)?
                    .unwrap_or_else(|| "course".into()),
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::{
        get_catalog_entry_db, hydrate_catalog_course_db, search_catalog_db, CatalogHydrateRow,
    };
    use crate::db::Database;
    use crate::domain::course_document::{DocumentChapter, DocumentElement, SignedCourseDocument};
    use rusqlite::params;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn insert_catalog_entry(db: &Database, course_id: &str, title: &str, author: &str, tags: &str) {
        db.conn()
            .execute(
                "INSERT INTO catalog (course_id, title, author_address, content_cid, tags, version, published_at, signature) \
                 VALUES (?1, ?2, ?3, 'cid123', ?4, 1, datetime('now'), 'sig_placeholder')",
                params![course_id, title, author, tags],
            )
            .unwrap();
    }

    #[test]
    fn catalog_search_by_title() {
        let db = test_db();
        insert_catalog_entry(&db, "c1", "Intro to Rust", "author1", "[\"rust\"]");
        insert_catalog_entry(&db, "c2", "Advanced Python", "author2", "[\"python\"]");
        let ids: Vec<String> = search_catalog_db(&db, Some("Rust".into()), None, None)
            .unwrap()
            .into_iter()
            .map(|entry| entry.course_id)
            .collect();
        assert_eq!(ids, vec!["c1"]);
    }

    #[test]
    fn catalog_search_by_author() {
        let db = test_db();
        insert_catalog_entry(&db, "c1", "Course 1", "author1", "[]");
        insert_catalog_entry(&db, "c2", "Course 2", "author2", "[]");
        insert_catalog_entry(&db, "c3", "Course 3", "author1", "[]");

        let entries = search_catalog_db(&db, None, Some("author1".into()), None).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .all(|entry| entry.author_address == "author1"));
    }

    #[test]
    fn catalog_entry_not_found() {
        let db = test_db();
        assert!(get_catalog_entry_db(&db, "nonexistent").unwrap().is_none());
    }

    #[test]
    fn catalog_tags_as_json() {
        let db = test_db();
        insert_catalog_entry(&db, "c1", "Tagged", "author1", "[\"rust\",\"systems\"]");

        let tags_json: String = db
            .conn()
            .query_row(
                "SELECT tags FROM catalog WHERE course_id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap();
        assert_eq!(tags, vec!["rust", "systems"]);
    }

    #[test]
    fn catalog_limit_respected() {
        let db = test_db();
        for i in 0..10 {
            insert_catalog_entry(&db, &format!("c{i}"), &format!("Course {i}"), "auth", "[]");
        }

        let entries = search_catalog_db(&db, None, None, Some(3)).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn hydrate_course_rolls_back_when_an_element_fails() {
        let db = test_db();
        db.conn()
            .execute_batch(
                "CREATE TRIGGER reject_hydrated_element
                 BEFORE INSERT ON course_elements
                 BEGIN
                     SELECT RAISE(ABORT, 'injected element failure');
                 END;",
            )
            .unwrap();
        let row = CatalogHydrateRow {
            course_id: "course-hydrate".into(),
            author_address: "author-hydrate".into(),
            content_cid: "cid-hydrate".into(),
            version: 1,
        };
        let document = SignedCourseDocument {
            version: 1,
            course_id: row.course_id.clone(),
            author_address: row.author_address.clone(),
            author_did: None,
            title: "Hydrated course".into(),
            description: None,
            thumbnail_hash: None,
            tags: vec!["test".into()],
            skill_ids: vec!["skill-test".into()],
            chapters: vec![DocumentChapter {
                id: "chapter-hydrate".into(),
                position: 0,
                title: "Chapter".into(),
                description: None,
                elements: vec![DocumentElement {
                    id: "element-hydrate".into(),
                    position: 0,
                    title: "Element".into(),
                    element_type: "text".into(),
                    content_hash: Some("element-cid".into()),
                    duration_seconds: None,
                    video_chapters: vec![],
                }],
            }],
            created_at: 1,
            updated_at: 1,
            kind: "course".into(),
            completion_policy: None,
            tutor_policy: Default::default(),
            signature: "verified-before-persistence".into(),
            public_key: "verified-before-persistence".into(),
        };

        let error = hydrate_catalog_course_db(db.conn(), &row, &document).unwrap_err();
        assert!(error.contains("injected element failure"), "got: {error}");
        for table in ["courses", "course_chapters", "course_elements"] {
            let count: i64 = db
                .conn()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |result| {
                    result.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table} must roll back as one hydration unit");
        }
    }
}

/// Get a single catalog entry by course_id.
#[tauri::command]
pub async fn get_catalog_entry(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Option<CatalogEntry>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "catalog.get",
            move |db| get_catalog_entry_db(db, &course_id),
        )
        .await
}

fn get_catalog_entry_db(
    db: &crate::db::Database,
    course_id: &str,
) -> Result<Option<CatalogEntry>, String> {
    let result = db.conn().query_row(
        "SELECT course_id, title, description, author_address, content_cid, \
         thumbnail_cid, tags, skill_ids, version, published_at, received_at, \
         pinned, on_chain_tx, kind FROM catalog WHERE course_id = ?1",
        params![course_id],
        |row| {
            let tags_json: Option<String> = row.get(6)?;
            let skill_ids_json: Option<String> = row.get(7)?;
            Ok(CatalogEntry {
                course_id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                author_address: row.get(3)?,
                content_cid: row.get(4)?,
                thumbnail_cid: row.get(5)?,
                tags: tags_json.and_then(|json| serde_json::from_str(&json).ok()),
                skill_ids: skill_ids_json.and_then(|json| serde_json::from_str(&json).ok()),
                version: row.get(8)?,
                published_at: row.get(9)?,
                received_at: row.get(10)?,
                pinned: row.get::<_, i64>(11)? != 0,
                on_chain_tx: row.get(12)?,
                kind: row
                    .get::<_, Option<String>>(13)?
                    .unwrap_or_else(|| "course".into()),
            })
        },
    );
    match result {
        Ok(entry) => Ok(Some(entry)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Debug)]
struct CatalogHydrateRow {
    course_id: String,
    author_address: String,
    content_cid: String,
    version: i64,
}

/// Hydrate local course/chapter/element tables from catalog announcements.
///
/// This command turns public catalog metadata into local, queryable course
/// rows by resolving each catalog `content_cid` as a signed course document,
/// verifying it, and upserting the full structure.
///
/// Returns number of courses successfully hydrated.
#[tauri::command]
pub async fn hydrate_catalog_courses(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<u32, String> {
    let max = limit.unwrap_or(200).min(500) as usize;
    let rows: Vec<CatalogHydrateRow> = state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            state.profile_lease(),
            "catalog.hydrate-list",
            move |db| {
                let mut stmt = db
                    .conn()
                    .prepare(
                        "SELECT course_id, author_address, content_cid, version \
                         FROM catalog ORDER BY version DESC, published_at DESC LIMIT ?1",
                    )
                    .map_err(|e| e.to_string())?;
                let mapped = stmt
                    .query_map(params![max as i64], |row| {
                        Ok(CatalogHydrateRow {
                            course_id: row.get(0)?,
                            author_address: row.get(1)?,
                            content_cid: row.get(2)?,
                            version: row.get(3)?,
                        })
                    })
                    .map_err(|e| e.to_string())?;
                mapped
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())
            },
        )
        .await?;

    if rows.is_empty() {
        return Ok(0);
    }
    let resolver = {
        let resolver_guard = state.resolver.lock().await;
        resolver_guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "content resolver not initialized".to_string())?
    };

    let mut hydrated = 0u32;

    for row in rows {
        let resolved = resolver
            .resolve(&row.content_cid)
            .await
            .map_err(|e| format!("resolve {}: {e}", row.content_cid))?;
        let signed_doc: SignedCourseDocument = serde_json::from_slice(&resolved.bytes)
            .map_err(|e| format!("invalid course document for {}: {e}", row.content_cid))?;
        content_course::verify_course_document(&signed_doc)
            .map_err(|e| format!("invalid signature for {}: {e}", row.content_cid))?;
        if signed_doc.author_address != row.author_address {
            return Err(format!(
                "hydrated course document author mismatch for {}",
                row.content_cid
            ));
        }
        if signed_doc.course_id != row.course_id {
            return Err(format!(
                "hydrated course document course_id mismatch for {}",
                row.content_cid
            ));
        }

        state
            .db_executor
            .execute(
                DatabaseWorkload::Background,
                state.profile_lease(),
                "catalog.hydrate-course",
                move |db| hydrate_catalog_course_db(db.conn(), &row, &signed_doc),
            )
            .await?;
        hydrated += 1;
    }

    Ok(hydrated)
}

fn hydrate_catalog_course_db(
    conn: &rusqlite::Connection,
    row: &CatalogHydrateRow,
    signed_doc: &SignedCourseDocument,
) -> Result<(), String> {
    crate::db::with_transaction(conn, || {
        let tags_json = serde_json::to_string(&signed_doc.tags).map_err(|e| e.to_string())?;
        let skill_ids_json =
            serde_json::to_string(&signed_doc.skill_ids).map_err(|e| e.to_string())?;
        let completion_policy_json = signed_doc
            .completion_policy
            .as_ref()
            .map(serde_json_canonicalizer::to_string)
            .transpose()
            .map_err(|error| error.to_string())?;

        conn.execute(
            "INSERT INTO courses (id, title, description, author_address, content_cid, thumbnail_cid, tags, skill_ids, version, course_document_version, completion_policy_json, status, published_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'published', datetime('now'), datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
             title = excluded.title, description = excluded.description, \
             author_address = excluded.author_address, content_cid = excluded.content_cid, \
             thumbnail_cid = excluded.thumbnail_cid, tags = excluded.tags, \
             skill_ids = excluded.skill_ids, version = excluded.version, \
             course_document_version = excluded.course_document_version, \
             completion_policy_json = excluded.completion_policy_json, \
             status = 'published', published_at = datetime('now'), updated_at = datetime('now')",
            params![
                signed_doc.course_id,
                signed_doc.title,
                signed_doc.description,
                signed_doc.author_address,
                row.content_cid,
                signed_doc.thumbnail_hash,
                tags_json,
                skill_ids_json,
                row.version,
                i64::from(signed_doc.version),
                completion_policy_json,
            ],
        )
        .map_err(|e| format!("upsert course failed: {e}"))?;

        conn.execute(
            "DELETE FROM course_chapters WHERE course_id = ?1",
            params![signed_doc.course_id],
        )
        .map_err(|e| format!("clear chapters failed: {e}"))?;

        alexandria_studio::store::write_tutor_policy(
            conn,
            &signed_doc.course_id,
            &signed_doc.tutor_policy,
        )
        .map_err(|e| format!("save tutor policy failed: {e}"))?;

        for chapter in &signed_doc.chapters {
            conn.execute(
                "INSERT INTO course_chapters (id, course_id, title, description, position) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    chapter.id,
                    signed_doc.course_id,
                    chapter.title,
                    chapter.description,
                    chapter.position,
                ],
            )
            .map_err(|e| format!("insert chapter failed: {e}"))?;

            for element in &chapter.elements {
                conn.execute(
                    "INSERT INTO course_elements (id, chapter_id, title, element_type, content_cid, position, duration_seconds) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        element.id,
                        chapter.id,
                        element.title,
                        element.element_type,
                        element.content_hash,
                        element.position,
                        element.duration_seconds,
                    ],
                )
                .map_err(|e| format!("insert element failed: {e}"))?;
            }
        }
        Ok(())
    })
}
