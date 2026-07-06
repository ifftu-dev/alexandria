//! IPC commands for element CRUD.
//!
//! Elements are the atomic learning items within chapters: videos,
//! text lessons, quizzes, assessments, etc. Ordered by `position`.

use rusqlite::params;
use tauri::State;

use crate::crypto::hash::entity_id;
use crate::domain::course::{CreateElementRequest, Element, UpdateElementRequest};
use crate::AppState;

/// List all elements for a chapter, ordered by position.
#[tauri::command]
pub async fn list_elements(
    state: State<'_, AppState>,
    chapter_id: String,
) -> Result<Vec<Element>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, chapter_id, title, element_type, content_cid, content_inline, position, duration_seconds, \
             plugin_cid, plugin_version, plugin_config_cid \
             FROM course_elements WHERE chapter_id = ?1 ORDER BY position ASC",
        )
        .map_err(|e| e.to_string())?;

    let elements = stmt
        .query_map(params![chapter_id], |row| {
            Ok(Element {
                id: row.get(0)?,
                chapter_id: row.get(1)?,
                title: row.get(2)?,
                element_type: row.get(3)?,
                content_cid: row.get(4)?,
                content_inline: row.get(5)?,
                position: row.get(6)?,
                duration_seconds: row.get(7)?,
                plugin_cid: row.get(8)?,
                plugin_version: row.get(9)?,
                plugin_config_cid: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(elements)
}

/// Create a new element in a chapter.
///
/// Appended at the end (highest position + 1).
#[tauri::command]
pub async fn create_element(
    state: State<'_, AppState>,
    chapter_id: String,
    req: CreateElementRequest,
) -> Result<Element, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    // Get the next position
    let next_pos: i64 = db
        .conn()
        .query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM course_elements WHERE chapter_id = ?1",
            params![chapter_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let id = entity_id(&[
        &chapter_id,
        &req.title,
        &req.element_type,
        &next_pos.to_string(),
    ]);

    db.conn()
        .execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, content_cid, content_inline, position, duration_seconds, plugin_cid, plugin_version, plugin_config_cid) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                chapter_id,
                req.title,
                req.element_type,
                req.content_hash,
                req.content_inline,
                next_pos,
                req.duration_seconds,
                req.plugin_cid,
                req.plugin_version,
                req.plugin_config_cid,
            ],
        )
        .map_err(|e| e.to_string())?;

    Ok(Element {
        id,
        chapter_id,
        title: req.title,
        element_type: req.element_type,
        content_cid: req.content_hash,
        content_inline: req.content_inline,
        position: next_pos,
        duration_seconds: req.duration_seconds,
        plugin_cid: req.plugin_cid,
        plugin_version: req.plugin_version,
        plugin_config_cid: req.plugin_config_cid,
    })
}

/// Update an existing element.
#[tauri::command]
pub async fn update_element(
    state: State<'_, AppState>,
    element_id: String,
    req: UpdateElementRequest,
) -> Result<Element, String> {
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
    if let Some(ref element_type) = req.element_type {
        set_clauses.push("element_type = ?");
        values.push(Box::new(element_type.clone()));
    }
    if let Some(ref content_hash) = req.content_hash {
        set_clauses.push("content_cid = ?");
        values.push(Box::new(content_hash.clone()));
    }
    if let Some(pos) = req.position {
        set_clauses.push("position = ?");
        values.push(Box::new(pos));
    }
    if let Some(dur) = req.duration_seconds {
        set_clauses.push("duration_seconds = ?");
        values.push(Box::new(dur));
    }
    if let Some(ref inline) = req.content_inline {
        set_clauses.push("content_inline = ?");
        values.push(Box::new(inline.clone()));
    }
    if let Some(ref cid) = req.plugin_cid {
        set_clauses.push("plugin_cid = ?");
        values.push(Box::new(cid.clone()));
    }
    if let Some(ref v) = req.plugin_version {
        set_clauses.push("plugin_version = ?");
        values.push(Box::new(v.clone()));
    }
    if let Some(ref cfg) = req.plugin_config_cid {
        set_clauses.push("plugin_config_cid = ?");
        values.push(Box::new(cfg.clone()));
    }

    if set_clauses.is_empty() {
        return Err("no fields to update".into());
    }

    values.push(Box::new(element_id.clone()));

    let sql = format!(
        "UPDATE course_elements SET {} WHERE id = ?",
        set_clauses.join(", ")
    );

    let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();

    let rows = db
        .conn()
        .execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err("element not found".into());
    }

    db.conn()
        .query_row(
            "SELECT id, chapter_id, title, element_type, content_cid, content_inline, position, duration_seconds, \
             plugin_cid, plugin_version, plugin_config_cid \
             FROM course_elements WHERE id = ?1",
            params![element_id],
            |row| {
                Ok(Element {
                    id: row.get(0)?,
                    chapter_id: row.get(1)?,
                    title: row.get(2)?,
                    element_type: row.get(3)?,
                    content_cid: row.get(4)?,
                    content_inline: row.get(5)?,
                    position: row.get(6)?,
                    duration_seconds: row.get(7)?,
                    plugin_cid: row.get(8)?,
                    plugin_version: row.get(9)?,
                    plugin_config_cid: row.get(10)?,
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

    fn setup_chapter(db: &Database) {
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1u', 'addr_test1q')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES ('c1', 'Course', 'stake_test1u')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) VALUES ('ch1', 'c1', 'Chapter', 0)",
                [],
            )
            .unwrap();
    }

    #[test]
    fn create_element_auto_positions() {
        let db = test_db();
        setup_chapter(&db);

        let id1 = entity_id(&["ch1", "Video", "video", "0"]);
        db.conn()
            .execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
                 VALUES (?1, 'ch1', 'Video', 'video', 0)",
                params![id1],
            )
            .unwrap();

        let next_pos: i64 = db
            .conn()
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM course_elements WHERE chapter_id = 'ch1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(next_pos, 1);
    }

    #[test]
    fn element_crud_lifecycle() {
        let db = test_db();
        setup_chapter(&db);

        // Insert
        let id = entity_id(&["ch1", "Quiz", "assessment", "0"]);
        db.conn()
            .execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, content_cid, position, duration_seconds) \
                 VALUES (?1, 'ch1', 'Quiz', 'assessment', 'hash1', 0, 300)",
                params![id],
            )
            .unwrap();

        // Read
        let (title, etype, dur): (String, String, Option<i64>) = db
            .conn()
            .query_row(
                "SELECT title, element_type, duration_seconds FROM course_elements WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(title, "Quiz");
        assert_eq!(etype, "assessment");
        assert_eq!(dur, Some(300));

        // Update
        db.conn()
            .execute(
                "UPDATE course_elements SET title = 'Final Quiz' WHERE id = ?1",
                params![id],
            )
            .unwrap();
        let new_title: String = db
            .conn()
            .query_row(
                "SELECT title FROM course_elements WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(new_title, "Final Quiz");

        // Delete
        let rows = db
            .conn()
            .execute("DELETE FROM course_elements WHERE id = ?1", params![id])
            .unwrap();
        assert_eq!(rows, 1);
    }

    fn insert_el(db: &Database, id: &str, chapter: &str, pos: i64) {
        db.conn()
            .execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
                 VALUES (?1, ?2, ?3, 'text', ?4)",
                params![id, chapter, id, pos],
            )
            .unwrap();
    }

    fn positions(db: &Database, chapter: &str) -> Vec<(String, i64)> {
        db.conn()
            .prepare(
                "SELECT id, position FROM course_elements WHERE chapter_id = ?1 ORDER BY position",
            )
            .unwrap()
            .query_map(params![chapter], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn reorder_elements_rewrites_positions() {
        let db = test_db();
        setup_chapter(&db);
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            insert_el(&db, id, "ch1", i as i64);
        }

        reorder_elements_impl(db.conn(), "ch1", &["c".into(), "a".into(), "b".into()]).unwrap();
        assert_eq!(
            positions(&db, "ch1"),
            vec![("c".into(), 0), ("a".into(), 1), ("b".into(), 2)]
        );
    }

    #[test]
    fn reorder_elements_rejects_foreign_and_partial_lists() {
        let db = test_db();
        setup_chapter(&db);
        insert_el(&db, "a", "ch1", 0);
        insert_el(&db, "b", "ch1", 1);

        // Partial list.
        assert!(reorder_elements_impl(db.conn(), "ch1", &["a".into()]).is_err());
        // Foreign id smuggled in.
        assert!(reorder_elements_impl(db.conn(), "ch1", &["a".into(), "zzz".into()]).is_err());
        // Duplicate id.
        assert!(reorder_elements_impl(db.conn(), "ch1", &["a".into(), "a".into()]).is_err());
        // Untouched on failure.
        assert_eq!(
            positions(&db, "ch1"),
            vec![("a".into(), 0), ("b".into(), 1)]
        );
    }

    #[test]
    fn move_element_across_chapters_compacts_both() {
        let db = test_db();
        setup_chapter(&db);
        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) VALUES ('ch2', 'c1', 'Two', 1)",
                [],
            )
            .unwrap();
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            insert_el(&db, id, "ch1", i as i64);
        }
        insert_el(&db, "x", "ch2", 0);

        move_element_impl(db.conn(), "b", "ch2", 0).unwrap();
        assert_eq!(
            positions(&db, "ch1"),
            vec![("a".into(), 0), ("c".into(), 1)]
        );
        assert_eq!(
            positions(&db, "ch2"),
            vec![("b".into(), 0), ("x".into(), 1)]
        );
    }

    #[test]
    fn move_element_rejects_cross_course() {
        let db = test_db();
        setup_chapter(&db);
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES ('c2', 'Other', 'stake_test1u')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) VALUES ('other', 'c2', 'Ch', 0)",
                [],
            )
            .unwrap();
        insert_el(&db, "a", "ch1", 0);

        assert!(move_element_impl(db.conn(), "a", "other", 0).is_err());
    }

    #[test]
    fn element_with_null_optional_fields() {
        let db = test_db();
        setup_chapter(&db);

        db.conn()
            .execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
                 VALUES ('el1', 'ch1', 'Text', 'text', 0)",
                [],
            )
            .unwrap();

        let (cid, dur): (Option<String>, Option<i64>) = db
            .conn()
            .query_row(
                "SELECT content_cid, duration_seconds FROM course_elements WHERE id = 'el1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(cid.is_none());
        assert!(dur.is_none());
    }
}

/// Rewrite the positions of a chapter's elements to match `ordered_ids`.
/// The list must be exactly the chapter's element ids (a permutation) —
/// anything else is rejected so a stale composer can't corrupt ordering.
pub(crate) fn reorder_elements_impl(
    conn: &rusqlite::Connection,
    chapter_id: &str,
    ordered_ids: &[String],
) -> Result<(), String> {
    let existing: Vec<String> = conn
        .prepare("SELECT id FROM course_elements WHERE chapter_id = ?1")
        .map_err(|e| e.to_string())?
        .query_map(params![chapter_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut expected: Vec<&str> = existing.iter().map(String::as_str).collect();
    let mut given: Vec<&str> = ordered_ids.iter().map(String::as_str).collect();
    expected.sort_unstable();
    given.sort_unstable();
    if expected != given {
        return Err("ordered_ids must be a permutation of the chapter's element ids".into());
    }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for (pos, id) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE course_elements SET position = ?1 WHERE id = ?2 AND chapter_id = ?3",
            params![pos as i64, id, chapter_id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

/// Reorder all elements within a chapter (drag-and-drop persistence).
#[tauri::command]
pub async fn reorder_elements(
    state: State<'_, AppState>,
    chapter_id: String,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    reorder_elements_impl(db.conn(), &chapter_id, &ordered_ids)
}

/// Move an element into another chapter of the same course at `position`.
/// Both chapters' positions are compacted in one transaction.
pub(crate) fn move_element_impl(
    conn: &rusqlite::Connection,
    element_id: &str,
    target_chapter_id: &str,
    position: i64,
) -> Result<(), String> {
    let (source_chapter, source_course): (String, String) = conn
        .query_row(
            "SELECT e.chapter_id, c.course_id FROM course_elements e \
             JOIN course_chapters c ON c.id = e.chapter_id WHERE e.id = ?1",
            params![element_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| "element not found".to_string())?;

    let target_course: String = conn
        .query_row(
            "SELECT course_id FROM course_chapters WHERE id = ?1",
            params![target_chapter_id],
            |row| row.get(0),
        )
        .map_err(|_| "target chapter not found".to_string())?;

    if source_course != target_course {
        return Err("cannot move an element across courses".into());
    }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    // Detach: close the gap in the source chapter.
    let old_pos: i64 = tx
        .query_row(
            "SELECT position FROM course_elements WHERE id = ?1",
            params![element_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE course_elements SET position = position - 1 \
         WHERE chapter_id = ?1 AND position > ?2",
        params![source_chapter, old_pos],
    )
    .map_err(|e| e.to_string())?;

    // Attach: open a slot in the target chapter (clamped to its length).
    let target_len: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM course_elements WHERE chapter_id = ?1 AND id != ?2",
            params![target_chapter_id, element_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let position = position.clamp(0, target_len);
    tx.execute(
        "UPDATE course_elements SET position = position + 1 \
         WHERE chapter_id = ?1 AND position >= ?2 AND id != ?3",
        params![target_chapter_id, position, element_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE course_elements SET chapter_id = ?1, position = ?2 WHERE id = ?3",
        params![target_chapter_id, position, element_id],
    )
    .map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())
}

/// Move an element to another chapter (cross-chapter drag).
#[tauri::command]
pub async fn move_element(
    state: State<'_, AppState>,
    element_id: String,
    target_chapter_id: String,
    position: i64,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    move_element_impl(db.conn(), &element_id, &target_chapter_id, position)
}

/// Replace a video element's chapter markers wholesale. Until now video
/// chapters were only writable through `publish_tutorial`; the composer
/// edits them standalone.
#[tauri::command]
pub async fn set_video_chapters(
    state: State<'_, AppState>,
    element_id: String,
    chapters: Vec<crate::domain::course::VideoChapterInput>,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM course_elements WHERE id = ?1",
            params![element_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("element not found".into());
    }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM video_chapters WHERE element_id = ?1",
        params![element_id],
    )
    .map_err(|e| e.to_string())?;
    for (pos, ch) in chapters.iter().enumerate() {
        if ch.start_seconds < 0 {
            return Err("chapter start_seconds cannot be negative".into());
        }
        let id = entity_id(&[
            &element_id,
            &ch.title,
            &ch.start_seconds.to_string(),
            &pos.to_string(),
        ]);
        tx.execute(
            "INSERT INTO video_chapters (id, element_id, title, start_seconds, position) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, element_id, ch.title, ch.start_seconds, pos as i64],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

/// List a video element's chapter markers, ordered by position.
#[tauri::command]
pub async fn list_video_chapters(
    state: State<'_, AppState>,
    element_id: String,
) -> Result<Vec<crate::domain::course::VideoChapterInput>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT title, start_seconds FROM video_chapters \
             WHERE element_id = ?1 ORDER BY position ASC",
        )
        .map_err(|e| e.to_string())?;
    let chapters = stmt
        .query_map(params![element_id], |row| {
            Ok(crate::domain::course::VideoChapterInput {
                title: row.get(0)?,
                start_seconds: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(chapters)
}

/// Delete an element.
#[tauri::command]
pub async fn delete_element(state: State<'_, AppState>, element_id: String) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let rows = db
        .conn()
        .execute(
            "DELETE FROM course_elements WHERE id = ?1",
            params![element_id],
        )
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err("element not found".into());
    }

    Ok(())
}
