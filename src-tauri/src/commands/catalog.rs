//! IPC commands for the distributed course catalog.
//!
//! The catalog table contains course announcements received from the
//! P2P network (and locally published courses). These commands let the
//! frontend search and browse the catalog.

use rusqlite::params;
use tauri::State;

use crate::domain::catalog::CatalogEntry;
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
    let db = state.db.lock().await;
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

/// Get a single catalog entry by course_id.
#[tauri::command]
pub async fn get_catalog_entry(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Option<CatalogEntry>, String> {
    let db = state.db.lock().await;

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
