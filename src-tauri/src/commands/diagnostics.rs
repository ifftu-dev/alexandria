//! Explicit, process-local diagnostics mode.
//!
//! Diagnostics is never persisted and is disabled on every launch/profile
//! lock. Entering it consumes any open assessment attempt only after the
//! frontend has successfully ended Sentinel; this prevents diagnostic tools
//! from becoming an unrecorded escape hatch during an assessment.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::profile::scope::ProfileState as State;
use serde::Serialize;
#[cfg(desktop)]
use tauri::Manager;
use tauri::{AppHandle, Emitter};

use crate::db::executor::DatabaseWorkload;
use crate::settings::{registry::keys, SettingsStore};
use crate::AppState;

static DIAGNOSTICS_ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsStatus {
    pub enabled: bool,
    pub open_assessment_count: u32,
}

pub(crate) fn is_enabled() -> bool {
    DIAGNOSTICS_ENABLED.load(Ordering::Acquire)
}

fn open_attempt_count(conn: &rusqlite::Connection, subject_did: &str) -> Result<u32, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM assessment_attempts \
          WHERE subject_did = ?1 AND graded_at IS NULL AND ended_at IS NULL",
        [subject_did],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count.max(0) as u32)
    .map_err(|e| e.to_string())
}

pub(crate) fn enter_impl(
    conn: &rusqlite::Connection,
    subject_did: &str,
    now: &str,
) -> Result<u32, String> {
    if subject_did.is_empty() {
        return Err("no local identity".into());
    }
    let unfinished_integrity: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assessment_attempts a \
              WHERE a.subject_did = ?1 AND a.graded_at IS NULL AND a.ended_at IS NULL \
                AND a.integrity_session_id IS NOT NULL \
                AND NOT EXISTS (SELECT 1 FROM integrity_sessions s \
                                 WHERE s.id = a.integrity_session_id \
                                   AND s.ended_at IS NOT NULL)",
            [subject_did],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if unfinished_integrity > 0 {
        return Err("assessment monitoring cleanup has not finished".into());
    }

    let changed = conn
        .execute(
            "UPDATE assessment_attempts \
                SET ended_at = ?1, end_reason = 'diagnostics' \
              WHERE subject_did = ?2 AND graded_at IS NULL AND ended_at IS NULL",
            rusqlite::params![now, subject_did],
        )
        .map_err(|e| e.to_string())?;
    Ok(changed as u32)
}

pub(crate) fn force_exit(app: &AppHandle) {
    let was_enabled = DIAGNOSTICS_ENABLED.swap(false, Ordering::AcqRel);
    #[cfg(desktop)]
    for (_, window) in app.webview_windows() {
        window.close_devtools();
    }
    if was_enabled {
        let _ = app.emit("diagnostics://changed", false);
    }
}

#[tauri::command]
pub async fn diagnostics_status(state: State<'_, AppState>) -> Result<DiagnosticsStatus, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "diagnostics.status",
            |db| {
                let subject_did = SettingsStore::get(db.conn(), keys::IDENTITY_LOCAL_DID);
                Ok(DiagnosticsStatus {
                    enabled: is_enabled(),
                    open_assessment_count: open_attempt_count(db.conn(), &subject_did)?,
                })
            },
        )
        .await
}

#[tauri::command]
pub async fn diagnostics_enter(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DiagnosticsStatus, String> {
    let now = crate::commands::credentials::now_rfc3339();
    let status = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "diagnostics.enter",
            move |db| {
                let subject_did = SettingsStore::get(db.conn(), keys::IDENTITY_LOCAL_DID);
                enter_impl(db.conn(), &subject_did, &now)?;
                DIAGNOSTICS_ENABLED.store(true, Ordering::Release);
                Ok(DiagnosticsStatus {
                    enabled: true,
                    open_assessment_count: 0,
                })
            },
        )
        .await?;
    let _ = app.emit("diagnostics://changed", true);
    Ok(status)
}

#[tauri::command]
pub async fn diagnostics_exit(app: AppHandle, _state: State<'_, AppState>) -> Result<(), String> {
    force_exit(&app);
    Ok(())
}

#[tauri::command]
pub async fn diagnostics_run_action(
    app: AppHandle,
    _state: State<'_, AppState>,
    action: String,
) -> Result<(), String> {
    if !is_enabled() {
        return Err("diagnostics mode is not enabled".into());
    }
    match action.as_str() {
        "reload" =>
        {
            #[cfg(desktop)]
            for (_, window) in app.webview_windows() {
                let _ = window.eval("window.location.reload()");
            }
        }
        "devtools" =>
        {
            #[cfg(desktop)]
            for (_, window) in app.webview_windows() {
                window.open_devtools();
            }
        }
        "sentinel" => {
            app.emit("develop://toggle-sentinel", ())
                .map_err(|e| e.to_string())?;
        }
        "install_cli" => {
            app.emit("develop://install-cli", ())
                .map_err(|e| e.to_string())?;
        }
        _ => return Err("unknown diagnostics action".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn entry_ends_open_attempts_without_grading_them() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn().execute_batch(
            "INSERT INTO question_banks (id, skill_id, label, ratified) VALUES ('b', 's', 'B', 1);
             INSERT INTO assessment_attempts
               (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, started_at)
             VALUES ('a', 'did:key:zLearner', 'b', 's', 1, '[]', '[]', '2026-09-14T00:00:00Z');",
        ).unwrap();

        assert_eq!(
            enter_impl(db.conn(), "did:key:zLearner", "2026-09-14T01:00:00Z").unwrap(),
            1
        );
        let row: (Option<String>, Option<String>, Option<String>, Option<i64>) = db.conn().query_row(
            "SELECT ended_at, end_reason, graded_at, passed FROM assessment_attempts WHERE id = 'a'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).unwrap();
        assert_eq!(row.0.as_deref(), Some("2026-09-14T01:00:00Z"));
        assert_eq!(row.1.as_deref(), Some("diagnostics"));
        assert!(row.2.is_none());
        assert!(row.3.is_none());
    }

    #[test]
    fn entry_waits_for_linked_integrity_cleanup() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn().execute_batch(
            "INSERT INTO question_banks (id, skill_id, label, ratified) VALUES ('b', 's', 'B', 1);
             INSERT INTO integrity_sessions (id, status) VALUES ('monitor', 'active');
             INSERT INTO assessment_attempts
               (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders,
                integrity_session_id, started_at)
             VALUES ('a', 'did:key:zLearner', 'b', 's', 1, '[]', '[]', 'monitor',
                     '2026-09-14T00:00:00Z');",
        ).unwrap();

        assert!(
            enter_impl(db.conn(), "did:key:zLearner", "2026-09-14T01:00:00Z")
                .unwrap_err()
                .contains("cleanup")
        );
    }
}
