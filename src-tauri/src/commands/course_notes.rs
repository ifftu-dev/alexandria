//! Course notes: the learner's own notes on a course's lessons.
//!
//! Notes reach a course from chat: a saved chat note that cites a lesson can
//! be copied into that course, at that lesson. The course player lists a
//! course's notes and lets the learner delete them. Notes are hidden while an
//! assessment is in progress, like chat itself.
//!
//! `course_notes` syncs between the learner's devices as full snapshots
//! without deletions, so a delete keeps the row as a tombstone with its text
//! cleared and a fresh `updated_at`, which wins on the other devices.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::ai_chat::{artifact, bank_guard, store as chat_store};
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// Most characters of a note shown as its preview.
const PREVIEW_CHARS: usize = 160;
/// Most notes listed for one course.
const MAX_LISTED: usize = 500;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CourseNote {
    pub id: String,
    pub course_id: String,
    pub element_id: Option<String>,
    pub element_title: Option<String>,
    pub title: Option<String>,
    pub body: String,
    pub origin: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

async fn with_conn<T, F>(
    state: &State<'_, AppState>,
    label: &'static str,
    f: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T, String> + Send + 'static,
{
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            label,
            move |db| f(db.conn()),
        )
        .await
}

fn no_assessment(conn: &Connection) -> Result<(), String> {
    alexandria_studio::gate::require_no_assessment(conn).map_err(|e| e.to_string())
}

fn preview(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(PREVIEW_CHARS)
        .collect()
}

const NOTE_COLUMNS: &str = "n.id, e.course_id, n.element_id, el.title, n.preview_text, n.body, n.origin, n.created_at, n.updated_at";

fn note_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CourseNote> {
    Ok(CourseNote {
        id: r.get(0)?,
        course_id: r.get(1)?,
        element_id: r.get(2)?,
        element_title: r.get(3)?,
        title: r.get(4)?,
        body: r.get(5)?,
        origin: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

fn note(conn: &Connection, id: &str) -> Result<CourseNote, String> {
    conn.query_row(
        &format!(
            "SELECT {NOTE_COLUMNS} FROM course_notes n
               JOIN enrollments e ON e.id = n.enrollment_id
               LEFT JOIN course_elements el ON el.id = n.element_id
              WHERE n.id = ?1 AND n.deleted_at IS NULL AND n.body IS NOT NULL"
        ),
        [id],
        note_row,
    )
    .optional()
    .map_err(|_| "storage_error: the note could not be read".to_string())?
    .ok_or_else(|| "not_found: no such note".to_string())
}

/// A course's notes, newest first.
pub fn list(conn: &Connection, course_id: &str) -> Result<Vec<CourseNote>, String> {
    no_assessment(conn)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {NOTE_COLUMNS} FROM course_notes n
               JOIN enrollments e ON e.id = n.enrollment_id
               LEFT JOIN course_elements el ON el.id = n.element_id
              WHERE e.course_id = ?1 AND n.deleted_at IS NULL AND n.body IS NOT NULL
              ORDER BY n.updated_at DESC, n.id LIMIT {MAX_LISTED}"
        ))
        .map_err(|_| "storage_error: notes could not be read".to_string())?;
    let rows = stmt
        .query_map([course_id], note_row)
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|_| "storage_error: notes could not be read".to_string())?;
    Ok(rows)
}

/// Deletes a note, leaving a tombstone that syncs.
pub fn delete(conn: &Connection, id: &str) -> Result<(), String> {
    let changed = conn
        .execute(
            "UPDATE course_notes
                SET body = NULL, preview_text = NULL, content_cid = NULL,
                    deleted_at = datetime('now'), updated_at = datetime('now')
              WHERE id = ?1 AND deleted_at IS NULL",
            [id],
        )
        .map_err(|_| "storage_error: the note could not be deleted".to_string())?;
    if changed == 0 {
        return Err("not_found: no such note".into());
    }
    Ok(())
}

/// Copies a saved chat note into the course of the first lesson it cites.
/// Copying the same note to the same lesson twice keeps one course note.
pub fn save_from_chat(conn: &Connection, artifact_id: &str) -> Result<CourseNote, String> {
    no_assessment(conn)?;
    let item = chat_store::artifact(conn, artifact_id).map_err(|e| e.to_string())?;
    let artifact::Body::Note { text } = &item.body else {
        return Err("invalid_input: only a note can be saved to a course".into());
    };
    if item.status != "saved" {
        return Err("invalid_input: save the note first".into());
    }
    let Some(lesson) = item.sources.iter().find(|s| s.kind == "lesson") else {
        return Err("no_lesson: this note does not cite a lesson".into());
    };
    if bank_guard::matches_assessment_item(conn, text) {
        return Err("assessment_item: notes cannot hold assessment questions".into());
    }
    let enrollment: Option<String> = conn
        .query_row(
            "SELECT id FROM enrollments WHERE course_id = ?1 AND status != 'dropped'
              ORDER BY enrolled_at DESC, id LIMIT 1",
            [&lesson.course_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "storage_error: enrollments could not be read".to_string())?;
    let Some(enrollment) = enrollment else {
        return Err("not_enrolled: enroll in the course to keep notes there".into());
    };
    // The lesson may have been removed from the course since; the note then
    // belongs to the course without a lesson.
    let element: Option<(String, String)> = conn
        .query_row(
            "SELECT el.id, el.chapter_id FROM course_elements el
               JOIN course_chapters ch ON ch.id = el.chapter_id
              WHERE el.id = ?1 AND ch.course_id = ?2",
            params![lesson.element_id, lesson.course_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|_| "storage_error: the lesson could not be read".to_string())?;
    let (element_id, chapter_id) = element.unzip();

    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM course_notes
              WHERE enrollment_id = ?1 AND element_id IS ?2 AND body = ?3 AND deleted_at IS NULL",
            params![enrollment, element_id, text],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "storage_error: notes could not be read".to_string())?;
    if let Some(id) = existing {
        return note(conn, &id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO course_notes (id, enrollment_id, chapter_id, element_id, preview_text, body, origin)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'chat')",
        params![
            id,
            enrollment,
            chapter_id,
            element_id,
            if item.title.is_empty() {
                preview(text)
            } else {
                item.title.clone()
            },
            text
        ],
    )
    .map_err(|_| "storage_error: the note could not be saved".to_string())?;
    note(conn, &id)
}

#[tauri::command]
pub async fn course_notes_list(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Vec<CourseNote>, String> {
    with_conn(&state, "course-notes.list", move |conn| {
        list(conn, &course_id)
    })
    .await
}

#[tauri::command]
pub async fn course_note_delete(state: State<'_, AppState>, note_id: String) -> Result<(), String> {
    with_conn(&state, "course-notes.delete", move |conn| {
        delete(conn, &note_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_save_course_note(
    state: State<'_, AppState>,
    artifact_id: String,
) -> Result<CourseNote, String> {
    with_conn(&state, "course-notes.save-from-chat", move |conn| {
        save_from_chat(conn, &artifact_id)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_chat::store::SourceRef;

    const NOW: &str = "2026-10-24T12:00:00Z";

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        for (_, _, sql) in crate::db::schema::MIGRATIONS {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO courses(id, title, author_address, status)
                  VALUES ('course', 'Probability', 'author', 'published');
             INSERT INTO course_chapters(id, course_id, title, position)
                  VALUES ('ch', 'course', 'Chapter 2', 0);
             INSERT INTO course_elements(id, chapter_id, title, element_type, content_inline, position)
                  VALUES ('lesson', 'ch', 'Conditional probability', 'text', 'P(A|B)', 0);",
        )
        .unwrap();
        conn
    }

    fn chat_note(conn: &Connection, id: &str, sources: &[SourceRef]) {
        chat_store::create_thread(conn, "t", Some("c1"), None, NOW).ok();
        let body = serde_json::to_string(&artifact::Body::Note {
            text: "Bayes: update a prior with evidence.".into(),
        })
        .unwrap();
        conn.execute(
            "INSERT INTO chat_artifacts (id, kind, status, title, body_json, sources_json, created_at, updated_at)
             VALUES (?1, 'note', 'saved', 'Bayes rule', ?2, ?3, ?4, ?4)",
            params![id, body, serde_json::to_string(sources).unwrap(), NOW],
        )
        .unwrap();
    }

    fn lesson() -> SourceRef {
        SourceRef {
            id: "S1".into(),
            kind: "lesson".into(),
            label: "Conditional probability".into(),
            course_id: "course".into(),
            element_id: "lesson".into(),
        }
    }

    #[test]
    fn a_chat_note_lands_on_its_lesson_once_and_a_delete_leaves_a_tombstone() {
        let conn = db();
        chat_note(&conn, "a1", &[lesson()]);
        let err = save_from_chat(&conn, "a1").unwrap_err();
        assert!(err.starts_with("not_enrolled"), "{err}");

        conn.execute_batch("INSERT INTO enrollments(id, course_id) VALUES ('en', 'course');")
            .unwrap();
        let saved = save_from_chat(&conn, "a1").unwrap();
        assert_eq!(saved.element_id.as_deref(), Some("lesson"));
        assert_eq!(
            saved.element_title.as_deref(),
            Some("Conditional probability")
        );
        assert_eq!(saved.title.as_deref(), Some("Bayes rule"));
        assert_eq!(saved.origin.as_deref(), Some("chat"));
        assert_eq!(
            save_from_chat(&conn, "a1").unwrap().id,
            saved.id,
            "saved once"
        );
        assert_eq!(list(&conn, "course").unwrap(), vec![saved.clone()]);

        delete(&conn, &saved.id).unwrap();
        assert!(list(&conn, "course").unwrap().is_empty());
        assert!(delete(&conn, &saved.id).is_err());
        let (body, deleted): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT body, deleted_at FROM course_notes WHERE id = ?1",
                [&saved.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(body, None, "the text is gone");
        assert!(deleted.is_some(), "the row stays so the delete syncs");
    }

    #[test]
    fn only_saved_notes_citing_a_lesson_can_go_to_a_course() {
        let conn = db();
        conn.execute_batch("INSERT INTO enrollments(id, course_id) VALUES ('en', 'course');")
            .unwrap();
        chat_note(&conn, "uncited", &[]);
        assert!(save_from_chat(&conn, "uncited")
            .unwrap_err()
            .starts_with("no_lesson"));
        chat_note(&conn, "draft", &[lesson()]);
        conn.execute(
            "UPDATE chat_artifacts SET status = 'proposed' WHERE id = 'draft'",
            [],
        )
        .unwrap();
        assert!(save_from_chat(&conn, "draft").is_err());
        assert!(save_from_chat(&conn, "missing").is_err());
    }

    #[test]
    fn notes_are_hidden_during_an_assessment() {
        let conn = db();
        conn.execute("INSERT INTO integrity_sessions(id) VALUES ('attempt')", [])
            .unwrap();
        let err = list(&conn, "course").unwrap_err();
        assert!(err.contains("assessment"), "{err}");
    }
}
