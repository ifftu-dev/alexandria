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
    let db = state.db.lock().await;

    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, chapter_id, title, element_type, content_cid, position, duration_seconds \
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
                position: row.get(5)?,
                duration_seconds: row.get(6)?,
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
    let db = state.db.lock().await;

    // Get the next position
    let next_pos: i64 = db
        .conn()
        .query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM course_elements WHERE chapter_id = ?1",
            params![chapter_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let id = entity_id(&[&chapter_id, &req.title, &req.element_type, &next_pos.to_string()]);

    db.conn()
        .execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, content_cid, position, duration_seconds) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                chapter_id,
                req.title,
                req.element_type,
                req.content_hash,
                next_pos,
                req.duration_seconds,
            ],
        )
        .map_err(|e| e.to_string())?;

    Ok(Element {
        id,
        chapter_id,
        title: req.title,
        element_type: req.element_type,
        content_cid: req.content_hash,
        position: next_pos,
        duration_seconds: req.duration_seconds,
    })
}

/// Update an existing element.
#[tauri::command]
pub async fn update_element(
    state: State<'_, AppState>,
    element_id: String,
    req: UpdateElementRequest,
) -> Result<Element, String> {
    let db = state.db.lock().await;

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
            "SELECT id, chapter_id, title, element_type, content_cid, position, duration_seconds \
             FROM course_elements WHERE id = ?1",
            params![element_id],
            |row| {
                Ok(Element {
                    id: row.get(0)?,
                    chapter_id: row.get(1)?,
                    title: row.get(2)?,
                    element_type: row.get(3)?,
                    content_cid: row.get(4)?,
                    position: row.get(5)?,
                    duration_seconds: row.get(6)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}

/// Delete an element.
#[tauri::command]
pub async fn delete_element(
    state: State<'_, AppState>,
    element_id: String,
) -> Result<(), String> {
    let db = state.db.lock().await;

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
