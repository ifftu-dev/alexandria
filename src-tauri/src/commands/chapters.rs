//! IPC commands for chapter CRUD.
//!
//! Chapters belong to courses and contain elements. Ordered by
//! `position` (0-indexed).

use rusqlite::params;
use tauri::State;

use crate::crypto::hash::entity_id;
use crate::domain::course::{Chapter, CreateChapterRequest, UpdateChapterRequest};
use crate::AppState;

/// List all chapters for a course, ordered by position.
#[tauri::command]
pub async fn list_chapters(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Vec<Chapter>, String> {
    let db = state.db.lock().unwrap();

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, course_id, title, description, position \
             FROM course_chapters WHERE course_id = ?1 ORDER BY position ASC",
        )
        .map_err(|e| e.to_string())?;

    let chapters = stmt
        .query_map(params![course_id], |row| {
            Ok(Chapter {
                id: row.get(0)?,
                course_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                position: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(chapters)
}

/// Create a new chapter in a course.
///
/// The chapter is appended at the end (highest position + 1).
#[tauri::command]
pub async fn create_chapter(
    state: State<'_, AppState>,
    course_id: String,
    req: CreateChapterRequest,
) -> Result<Chapter, String> {
    let db = state.db.lock().unwrap();

    // Get the next position
    let next_pos: i64 = db
        .conn()
        .query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM course_chapters WHERE course_id = ?1",
            params![course_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let id = entity_id(&[&course_id, &req.title, &next_pos.to_string()]);

    db.conn()
        .execute(
            "INSERT INTO course_chapters (id, course_id, title, description, position) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, course_id, req.title, req.description, next_pos],
        )
        .map_err(|e| e.to_string())?;

    // Mark course as modified
    db.conn()
        .execute(
            "UPDATE courses SET updated_at = datetime('now') WHERE id = ?1",
            params![course_id],
        )
        .map_err(|e| e.to_string())?;

    Ok(Chapter {
        id,
        course_id,
        title: req.title,
        description: req.description,
        position: next_pos,
    })
}

/// Update an existing chapter.
#[tauri::command]
pub async fn update_chapter(
    state: State<'_, AppState>,
    chapter_id: String,
    req: UpdateChapterRequest,
) -> Result<Chapter, String> {
    let db = state.db.lock().unwrap();

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
    if let Some(pos) = req.position {
        set_clauses.push("position = ?");
        values.push(Box::new(pos));
    }

    if set_clauses.is_empty() {
        return Err("no fields to update".into());
    }

    values.push(Box::new(chapter_id.clone()));

    let sql = format!(
        "UPDATE course_chapters SET {} WHERE id = ?",
        set_clauses.join(", ")
    );

    let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();

    let rows = db
        .conn()
        .execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err("chapter not found".into());
    }

    // Return updated chapter
    db.conn()
        .query_row(
            "SELECT id, course_id, title, description, position \
             FROM course_chapters WHERE id = ?1",
            params![chapter_id],
            |row| {
                Ok(Chapter {
                    id: row.get(0)?,
                    course_id: row.get(1)?,
                    title: row.get(2)?,
                    description: row.get(3)?,
                    position: row.get(4)?,
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

    fn setup_course(db: &Database) {
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1u', 'addr_test1q')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) \
                 VALUES ('c1', 'Test Course', 'stake_test1u')",
                [],
            )
            .unwrap();
    }

    #[test]
    fn create_chapter_auto_positions() {
        let db = test_db();
        setup_course(&db);

        // First chapter gets position 0
        let id1 = entity_id(&["c1", "Ch1", "0"]);
        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) VALUES (?1, 'c1', 'Ch1', 0)",
                params![id1],
            )
            .unwrap();

        // Verify next position calculation
        let next_pos: i64 = db
            .conn()
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM course_chapters WHERE course_id = 'c1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(next_pos, 1);

        // Second chapter
        let id2 = entity_id(&["c1", "Ch2", "1"]);
        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) VALUES (?1, 'c1', 'Ch2', 1)",
                params![id2],
            )
            .unwrap();

        // List in order
        let mut stmt = db
            .conn()
            .prepare("SELECT title FROM course_chapters WHERE course_id = 'c1' ORDER BY position")
            .unwrap();
        let titles: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(titles, vec!["Ch1", "Ch2"]);
    }

    #[test]
    fn update_chapter_title() {
        let db = test_db();
        setup_course(&db);

        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) VALUES ('ch1', 'c1', 'Old', 0)",
                [],
            )
            .unwrap();

        db.conn()
            .execute(
                "UPDATE course_chapters SET title = 'New' WHERE id = 'ch1'",
                [],
            )
            .unwrap();

        let title: String = db
            .conn()
            .query_row(
                "SELECT title FROM course_chapters WHERE id = 'ch1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "New");
    }

    #[test]
    fn delete_chapter_removes_row() {
        let db = test_db();
        setup_course(&db);

        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) VALUES ('ch1', 'c1', 'Ch1', 0)",
                [],
            )
            .unwrap();

        let rows = db
            .conn()
            .execute("DELETE FROM course_chapters WHERE id = 'ch1'", [])
            .unwrap();
        assert_eq!(rows, 1);

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM course_chapters WHERE course_id = 'c1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_nonexistent_chapter_returns_zero() {
        let db = test_db();
        let rows = db
            .conn()
            .execute("DELETE FROM course_chapters WHERE id = 'nonexistent'", [])
            .unwrap();
        assert_eq!(rows, 0);
    }
}

/// Delete a chapter and all its elements.
#[tauri::command]
pub async fn delete_chapter(state: State<'_, AppState>, chapter_id: String) -> Result<(), String> {
    let db = state.db.lock().unwrap();

    let rows = db
        .conn()
        .execute(
            "DELETE FROM course_chapters WHERE id = ?1",
            params![chapter_id],
        )
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err("chapter not found".into());
    }

    Ok(())
}
