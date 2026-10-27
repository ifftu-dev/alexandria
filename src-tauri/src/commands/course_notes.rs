//! Commands for course notes. The notes themselves live in
//! [`crate::course_notes`].

use super::with_conn;
use crate::course_notes::{self, CourseNote};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

#[tauri::command]
pub async fn course_notes_list(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Vec<CourseNote>, String> {
    with_conn(&state, "course-notes.list", move |conn| {
        course_notes::list(conn, &course_id)
    })
    .await
}

#[tauri::command]
pub async fn course_note_delete(state: State<'_, AppState>, note_id: String) -> Result<(), String> {
    with_conn(&state, "course-notes.delete", move |conn| {
        course_notes::delete(conn, &note_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_save_course_note(
    state: State<'_, AppState>,
    artifact_id: String,
) -> Result<CourseNote, String> {
    with_conn(&state, "course-notes.save-from-chat", move |conn| {
        course_notes::save_from_chat(conn, &artifact_id)
    })
    .await
}
