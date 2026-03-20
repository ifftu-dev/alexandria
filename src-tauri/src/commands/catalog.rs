//! IPC commands for the distributed course catalog.
//!
//! The catalog table contains course announcements received from the
//! P2P network (and locally published courses). These commands let the
//! frontend search and browse the catalog.

use rusqlite::params;
use serde::Deserialize;
use tauri::State;

use crate::domain::catalog::CatalogEntry;
use crate::domain::course_document::SignedCourseDocument;
use crate::ipfs::course as ipfs_course;
use crate::AppState;

const BOOTSTRAP_PUBLIC_COURSES_JSON: &str = include_str!("../../../bootstrap/public_courses.json");

#[derive(Debug, Deserialize)]
struct BootstrapPayload {
    courses: Vec<BootstrapCourse>,
}

#[derive(Debug, Deserialize)]
struct BootstrapCourse {
    id: String,
    title: String,
    description: Option<String>,
    author_address: String,
    author_name: Option<String>,
    content_cid: Option<String>,
    thumbnail_cid: Option<String>,
    thumbnail_svg: Option<String>,
    tags: Vec<String>,
    skill_ids: Vec<String>,
    version: i64,
    status: String,
    published_at: Option<String>,
    on_chain_tx: Option<String>,
    created_at: String,
    updated_at: String,
    chapters: Vec<BootstrapChapter>,
}

#[derive(Debug, Deserialize)]
struct BootstrapChapter {
    id: String,
    title: String,
    description: Option<String>,
    position: i64,
    elements: Vec<BootstrapElement>,
}

#[derive(Debug, Deserialize)]
struct BootstrapElement {
    id: String,
    title: String,
    element_type: String,
    content_cid: Option<String>,
    content_inline: Option<String>,
    position: i64,
    duration_seconds: Option<i64>,
}

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
    let db = state.db.lock().unwrap();
    let max = limit.unwrap_or(50).min(200) as usize;

    // Build dynamic WHERE clause
    let mut conditions: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref q) = query {
        let pattern = format!("%{q}%");
        conditions.push(format!(
            "(title LIKE ?{idx} OR description LIKE ?{idx} OR tags LIKE ?{idx})"
        ));
        param_values.push(Box::new(pattern));
        idx += 1;
    }

    if let Some(ref a) = author {
        conditions.push(format!("author_address = ?{idx}"));
        param_values.push(Box::new(a.clone()));
        // idx += 1; // uncomment if more conditions are added
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT course_id, title, description, author_address, content_cid, \
         thumbnail_cid, tags, skill_ids, version, published_at, received_at, \
         pinned, on_chain_tx \
         FROM catalog {where_clause} \
         ORDER BY published_at DESC \
         LIMIT {max}"
    );

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = db.conn().prepare(&sql).map_err(|e| e.to_string())?;

    let entries = stmt
        .query_map(params_ref.as_slice(), |row| {
            let tags_json: Option<String> = row.get(6)?;
            let skill_ids_json: Option<String> = row.get(7)?;
            let pinned_int: i64 = row.get(11)?;

            Ok(CatalogEntry {
                course_id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                author_address: row.get(3)?,
                content_cid: row.get(4)?,
                thumbnail_cid: row.get(5)?,
                tags: tags_json.and_then(|j| serde_json::from_str(&j).ok()),
                skill_ids: skill_ids_json.and_then(|j| serde_json::from_str(&j).ok()),
                version: row.get(8)?,
                published_at: row.get(9)?,
                received_at: row.get(10)?,
                pinned: pinned_int != 0,
                on_chain_tx: row.get(12)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
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

        let mut stmt = db
            .conn()
            .prepare("SELECT course_id FROM catalog WHERE title LIKE '%Rust%'")
            .unwrap();
        let ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(ids, vec!["c1"]);
    }

    #[test]
    fn catalog_search_by_author() {
        let db = test_db();
        insert_catalog_entry(&db, "c1", "Course 1", "author1", "[]");
        insert_catalog_entry(&db, "c2", "Course 2", "author2", "[]");
        insert_catalog_entry(&db, "c3", "Course 3", "author1", "[]");

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM catalog WHERE author_address = 'author1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn catalog_entry_not_found() {
        let db = test_db();
        let result = db.conn().query_row(
            "SELECT course_id FROM catalog WHERE course_id = 'nonexistent'",
            [],
            |row| row.get::<_, String>(0),
        );
        assert!(matches!(result, Err(rusqlite::Error::QueryReturnedNoRows)));
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

        let mut stmt = db
            .conn()
            .prepare("SELECT course_id FROM catalog ORDER BY published_at DESC LIMIT 3")
            .unwrap();
        let ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(ids.len(), 3);
    }
}

/// Get a single catalog entry by course_id.
#[tauri::command]
pub async fn get_catalog_entry(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Option<CatalogEntry>, String> {
    let db = state.db.lock().unwrap();

    let result = db.conn().query_row(
        "SELECT course_id, title, description, author_address, content_cid, \
         thumbnail_cid, tags, skill_ids, version, published_at, received_at, \
         pinned, on_chain_tx \
         FROM catalog WHERE course_id = ?1",
        params![course_id],
        |row| {
            let tags_json: Option<String> = row.get(6)?;
            let skill_ids_json: Option<String> = row.get(7)?;
            let pinned_int: i64 = row.get(11)?;

            Ok(CatalogEntry {
                course_id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                author_address: row.get(3)?,
                content_cid: row.get(4)?,
                thumbnail_cid: row.get(5)?,
                tags: tags_json.and_then(|j| serde_json::from_str(&j).ok()),
                skill_ids: skill_ids_json.and_then(|j| serde_json::from_str(&j).ok()),
                version: row.get(8)?,
                published_at: row.get(9)?,
                received_at: row.get(10)?,
                pinned: pinned_int != 0,
                on_chain_tx: row.get(12)?,
            })
        },
    );

    match result {
        Ok(entry) => Ok(Some(entry)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Debug)]
struct CatalogHydrateRow {
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

    let rows: Vec<CatalogHydrateRow> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db
            .conn()
            .prepare(
                "SELECT content_cid, version FROM catalog ORDER BY version DESC, published_at DESC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;

        let mapped = stmt
            .query_map(params![max as i64], |row| {
                Ok(CatalogHydrateRow {
                    content_cid: row.get(0)?,
                    version: row.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;

        mapped
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };

    let mut hydrated = 0u32;

    for row in rows {
        let signed_doc = {
            let resolver_guard = state.resolver.lock().await;
            let resolver = resolver_guard
                .as_ref()
                .ok_or_else(|| "content resolver not initialized".to_string())?;

            let resolved = resolver
                .resolve(&row.content_cid)
                .await
                .map_err(|e| format!("resolve {}: {e}", row.content_cid))?;

            let doc: SignedCourseDocument = serde_json::from_slice(&resolved.bytes)
                .map_err(|e| format!("invalid course document for {}: {e}", row.content_cid))?;

            ipfs_course::verify_course_document(&doc)
                .map_err(|e| format!("invalid signature for {}: {e}", row.content_cid))?;

            doc
        };

        let db = state.db.lock().unwrap();
        let conn = db.conn();
        conn.execute_batch("BEGIN")
            .map_err(|e| format!("hydrate tx begin failed: {e}"))?;

        let tags_json =
            serde_json::to_string(&signed_doc.tags).unwrap_or_else(|_| "[]".to_string());
        let skill_ids_json =
            serde_json::to_string(&signed_doc.skill_ids).unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT INTO courses (id, title, description, author_address, content_cid, thumbnail_cid, tags, skill_ids, version, status, published_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'published', datetime('now'), datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
             title = excluded.title, \
             description = excluded.description, \
             author_address = excluded.author_address, \
             content_cid = excluded.content_cid, \
             thumbnail_cid = excluded.thumbnail_cid, \
             tags = excluded.tags, \
             skill_ids = excluded.skill_ids, \
             version = excluded.version, \
             status = 'published', \
             published_at = datetime('now'), \
             updated_at = datetime('now')",
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
            ],
        )
        .map_err(|e| format!("upsert course failed: {e}"))?;

        conn.execute(
            "DELETE FROM course_chapters WHERE course_id = ?1",
            params![signed_doc.course_id],
        )
        .map_err(|e| format!("clear chapters failed: {e}"))?;

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

        conn.execute_batch("COMMIT")
            .map_err(|e| format!("hydrate tx commit failed: {e}"))?;
        hydrated += 1;
    }

    Ok(hydrated)
}

/// Bootstrap the public catalog/courses dataset for fresh installs.
///
/// This imports a bundled public dataset (course metadata + chapters + elements)
/// so new devices have immediate access to public/demo content before P2P
/// discovery has propagated.
#[tauri::command]
pub async fn bootstrap_public_catalog(state: State<'_, AppState>) -> Result<u32, String> {
    let payload: BootstrapPayload = serde_json::from_str(BOOTSTRAP_PUBLIC_COURSES_JSON)
        .map_err(|e| format!("invalid bootstrap payload: {e}"))?;

    let db = state.db.lock().unwrap();
    let conn = db.conn();
    conn.execute_batch("BEGIN")
        .map_err(|e| format!("bootstrap tx begin failed: {e}"))?;

    let mut imported = 0u32;
    for course in &payload.courses {
        let tags_json = serde_json::to_string(&course.tags).unwrap_or_else(|_| "[]".to_string());
        let skill_ids_json =
            serde_json::to_string(&course.skill_ids).unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT INTO courses (id, title, description, author_address, author_name, content_cid, thumbnail_cid, thumbnail_svg, tags, skill_ids, version, status, published_at, on_chain_tx, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16) \
             ON CONFLICT(id) DO UPDATE SET \
             title = excluded.title, \
             description = excluded.description, \
             author_address = excluded.author_address, \
             author_name = excluded.author_name, \
             content_cid = excluded.content_cid, \
             thumbnail_cid = excluded.thumbnail_cid, \
             thumbnail_svg = excluded.thumbnail_svg, \
             tags = excluded.tags, \
             skill_ids = excluded.skill_ids, \
             version = excluded.version, \
             status = excluded.status, \
             published_at = excluded.published_at, \
             on_chain_tx = excluded.on_chain_tx, \
             updated_at = excluded.updated_at",
            params![
                course.id,
                course.title,
                course.description,
                course.author_address,
                course.author_name,
                course.content_cid,
                course.thumbnail_cid,
                course.thumbnail_svg,
                tags_json,
                skill_ids_json,
                course.version,
                course.status,
                course.published_at,
                course.on_chain_tx,
                course.created_at,
                course.updated_at,
            ],
        )
        .map_err(|e| format!("bootstrap upsert course failed: {e}"))?;

        conn.execute(
            "DELETE FROM course_chapters WHERE course_id = ?1",
            params![course.id],
        )
        .map_err(|e| format!("bootstrap clear chapters failed: {e}"))?;

        for chapter in &course.chapters {
            conn.execute(
                "INSERT INTO course_chapters (id, course_id, title, description, position) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    chapter.id,
                    course.id,
                    chapter.title,
                    chapter.description,
                    chapter.position,
                ],
            )
            .map_err(|e| format!("bootstrap insert chapter failed: {e}"))?;

            for element in &chapter.elements {
                conn.execute(
                    "INSERT INTO course_elements (id, chapter_id, title, element_type, content_cid, content_inline, position, duration_seconds) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        element.id,
                        chapter.id,
                        element.title,
                        element.element_type,
                        element.content_cid,
                        element.content_inline,
                        element.position,
                        element.duration_seconds,
                    ],
                )
                .map_err(|e| format!("bootstrap insert element failed: {e}"))?;
            }
        }

        let catalog_content_id = course
            .content_cid
            .clone()
            .unwrap_or_else(|| format!("bootstrap:{}", course.id));

        conn.execute(
            "INSERT INTO catalog (course_id, title, description, author_address, content_cid, thumbnail_cid, tags, skill_ids, version, published_at, signature, pinned) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, COALESCE(?10, datetime('now')), 'bootstrap_v1', 1) \
             ON CONFLICT(course_id) DO UPDATE SET \
             title = excluded.title, \
             description = excluded.description, \
             author_address = excluded.author_address, \
             content_cid = excluded.content_cid, \
             thumbnail_cid = excluded.thumbnail_cid, \
             tags = excluded.tags, \
             skill_ids = excluded.skill_ids, \
             version = excluded.version, \
             published_at = excluded.published_at, \
             received_at = datetime('now'), \
             pinned = 1",
            params![
                course.id,
                course.title,
                course.description,
                course.author_address,
                catalog_content_id,
                course.thumbnail_cid,
                serde_json::to_string(&course.tags).unwrap_or_else(|_| "[]".to_string()),
                serde_json::to_string(&course.skill_ids).unwrap_or_else(|_| "[]".to_string()),
                course.version,
                course.published_at,
            ],
        )
        .map_err(|e| format!("bootstrap catalog upsert failed: {e}"))?;

        imported += 1;
    }

    conn.execute_batch("COMMIT")
        .map_err(|e| format!("bootstrap tx commit failed: {e}"))?;
    Ok(imported)
}
