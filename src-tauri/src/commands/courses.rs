use rusqlite::params;
use tauri::State;

use crate::crypto::hash::entity_id;
use crate::domain::course::{Course, CreateCourseRequest, UpdateCourseRequest};
use crate::AppState;

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
