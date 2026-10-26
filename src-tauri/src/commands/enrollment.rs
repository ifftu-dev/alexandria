use crate::profile::scope::ProfileState as State;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::crypto::hash::entity_id;
use crate::db::executor::DatabaseWorkload;
use crate::domain::enrollment::{ElementProgress, Enrollment, UpdateProgressRequest};
use crate::enrollment;
use crate::AppState;

/// List all enrollments for the local user.
#[tauri::command]
pub async fn list_enrollments(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<Enrollment>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.list",
            move |db| enrollment::list(db, status),
        )
        .await
}

/// Enroll in a course.
#[tauri::command]
pub async fn enroll(state: State<'_, AppState>, course_id: String) -> Result<Enrollment, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.enroll",
            move |db| enrollment::enroll(db, &course_id),
        )
        .await
}

/// Update progress on a course element.
#[tauri::command]
pub async fn update_progress(
    state: State<'_, AppState>,
    enrollment_id: String,
    req: UpdateProgressRequest,
) -> Result<ElementProgress, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.update_progress",
            move |db| update_progress_db(db, enrollment_id, req),
        )
        .await
}

fn update_progress_db(
    db: &crate::db::Database,
    enrollment_id: String,
    req: UpdateProgressRequest,
) -> Result<ElementProgress, String> {
    let id = entity_id(&[&enrollment_id, &req.element_id]);

    // Upsert: insert or update
    db.conn()
            .execute(
                "INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent)
                 VALUES (?1, ?2, ?3, ?4, ?5, COALESCE(?6, 0))
                 ON CONFLICT(enrollment_id, element_id) DO UPDATE SET
                    status = ?4,
                    score = COALESCE(?5, score),
                    time_spent = COALESCE(?6, time_spent),
                    completed_at = CASE WHEN ?4 = 'completed' THEN datetime('now') ELSE completed_at END,
                    updated_at = datetime('now')",
                params![
                    id,
                    enrollment_id,
                    req.element_id,
                    req.status,
                    req.score,
                    req.time_spent,
                ],
            )
            .map_err(|e| e.to_string())?;

    // Read back the progress
    let progress: ElementProgress = db
            .conn()
            .query_row(
                "SELECT id, enrollment_id, element_id, status, score, time_spent, completed_at, updated_at \
                 FROM element_progress WHERE enrollment_id = ?1 AND element_id = ?2",
                params![enrollment_id, req.element_id],
                |row| {
                    Ok(ElementProgress {
                        id: row.get(0)?,
                        enrollment_id: row.get(1)?,
                        element_id: row.get(2)?,
                        status: row.get(3)?,
                        score: row.get(4)?,
                        time_spent: row.get(5)?,
                        completed_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                },
            )
            .map_err(|e| e.to_string())?;

    // Post-migration 040: completion no longer triggers evidence
    // creation or aggregation. In the VC-first model, element
    // completion is expected to produce a Cardano completion-witness
    // tx; the Blockfrost observer (Session 2) auto-issues a signed
    // VC referencing that tx and writes it to `credentials`. That
    // wiring lands in a follow-up session; for now progress tracking
    // alone is preserved so learners still see their course state
    // move forward locally.
    Ok(progress)
}

#[cfg(test)]
mod tests {
    use crate::crypto::hash::entity_id;
    use crate::db::Database;
    use rusqlite::params;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn setup_enrollment(db: &Database) -> (String, String) {
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
        let enrollment_id = entity_id(&["stake_test1u", "c1"]);
        db.conn()
            .execute(
                "INSERT INTO enrollments (id, course_id) VALUES (?1, 'c1')",
                params![enrollment_id],
            )
            .unwrap();
        (enrollment_id, "c1".into())
    }

    #[test]
    fn enrollment_insert_and_read() {
        let db = test_db();
        let (enrollment_id, _) = setup_enrollment(&db);

        let (id, status): (String, String) = db
            .conn()
            .query_row(
                "SELECT id, status FROM enrollments WHERE id = ?1",
                params![enrollment_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(id, enrollment_id);
        assert_eq!(status, "active");
    }

    #[test]
    fn enrollment_duplicate_prevention() {
        let db = test_db();
        let (_, _) = setup_enrollment(&db);

        let already: bool = db
            .conn()
            .query_row(
                "SELECT COUNT(*) > 0 FROM enrollments WHERE course_id = 'c1' AND status = 'active'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(already);
    }

    #[test]
    fn enrollment_course_must_exist() {
        let db = test_db();
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1u', 'addr_test1q')",
                [],
            )
            .unwrap();

        let exists: bool = db
            .conn()
            .query_row(
                "SELECT COUNT(*) > 0 FROM courses WHERE id = 'nonexistent'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!exists);
    }

    #[test]
    fn element_progress_upsert() {
        let db = test_db();
        let (enrollment_id, _) = setup_enrollment(&db);

        // Setup chapter + element
        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) VALUES ('ch1', 'c1', 'Ch', 0)",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
                 VALUES ('el1', 'ch1', 'Video', 'video', 0)",
                [],
            )
            .unwrap();

        let progress_id = entity_id(&[&enrollment_id, "el1"]);

        // Insert progress
        db.conn()
            .execute(
                "INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent) \
                 VALUES (?1, ?2, 'el1', 'in_progress', NULL, 60)",
                params![progress_id, enrollment_id],
            )
            .unwrap();

        let (status, score): (String, Option<f64>) = db
            .conn()
            .query_row(
                "SELECT status, score FROM element_progress WHERE id = ?1",
                params![progress_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "in_progress");
        assert!(score.is_none());

        // Update via ON CONFLICT (upsert)
        db.conn()
            .execute(
                "INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent)
                 VALUES (?1, ?2, 'el1', 'completed', 0.95, 120)
                 ON CONFLICT(enrollment_id, element_id) DO UPDATE SET
                    status = excluded.status,
                    score = excluded.score,
                    time_spent = excluded.time_spent,
                    updated_at = datetime('now')",
                params![progress_id, enrollment_id],
            )
            .unwrap();

        let (status2, score2, time2): (String, Option<f64>, i64) = db
            .conn()
            .query_row(
                "SELECT status, score, time_spent FROM element_progress WHERE id = ?1",
                params![progress_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(status2, "completed");
        assert_eq!(score2, Some(0.95));
        assert_eq!(time2, 120);
    }

    #[test]
    fn enrollment_status_filter() {
        let db = test_db();
        let (_, _) = setup_enrollment(&db);

        // Add a completed enrollment for a second course
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES ('c2', 'Course 2', 'stake_test1u')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO enrollments (id, course_id, status) VALUES ('e2', 'c2', 'completed')",
                [],
            )
            .unwrap();

        let active: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM enrollments WHERE status = 'active'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active, 1);

        let completed: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM enrollments WHERE status = 'completed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(completed, 1);
    }
}

/// Get all progress for an enrollment.
#[tauri::command]
pub async fn get_progress(
    state: State<'_, AppState>,
    enrollment_id: String,
) -> Result<Vec<ElementProgress>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.get_progress",
            move |db| get_progress_db(db, &enrollment_id),
        )
        .await
}

fn get_progress_db(
    db: &crate::db::Database,
    enrollment_id: &str,
) -> Result<Vec<ElementProgress>, String> {
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, enrollment_id, element_id, status, score, time_spent, completed_at, updated_at \
             FROM element_progress WHERE enrollment_id = ?1 ORDER BY element_id",
        )
        .map_err(|e| e.to_string())?;

    let progress = stmt
        .query_map(params![enrollment_id], |row| {
            Ok(ElementProgress {
                id: row.get(0)?,
                enrollment_id: row.get(1)?,
                element_id: row.get(2)?,
                status: row.get(3)?,
                score: row.get(4)?,
                time_spent: row.get(5)?,
                completed_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(progress)
}

/// A learner's stored response to a built-in assessment element.
#[derive(Debug, Clone, Serialize)]
pub struct ElementSubmissionRecord {
    pub element_id: String,
    /// Raw, component-defined answer payload (selected option indices,
    /// short-answer text, essay body, per-question result). `None` for
    /// plugin submissions, whose inputs live in the iroh bundle.
    pub answers_json: Option<String>,
    pub score: f64,
    pub created_at: String,
}

/// Record a built-in assessment response (raw answers + score) to
/// `element_submissions`.
///
/// Quiz/MCQ/essay/assessment elements are graded entirely in the UI. This
/// persists the result so (a) it survives reload and can be reviewed when
/// the learner revisits the element, and (b) the completion-witness
/// assembler — which reads `element_submissions` — counts the element
/// toward course completion (it previously never saw built-in answers).
///
/// `answers_json` is the component-defined raw response payload. The row id
/// is deterministic from (element, enrollment, answers hash): resubmitting
/// identical answers is idempotent, while a changed answer records a new
/// attempt. Completion uses the best score; revisit shows the latest.
#[tauri::command]
pub async fn record_element_submission(
    state: State<'_, AppState>,
    enrollment_id: String,
    element_id: String,
    element_type: String,
    answers_json: String,
    score: f64,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.record_submission",
            move |db| {
                record_element_submission_db(
                    db,
                    enrollment_id,
                    element_id,
                    element_type,
                    answers_json,
                    score,
                )
            },
        )
        .await
}

fn record_element_submission_db(
    db: &crate::db::Database,
    enrollment_id: String,
    element_id: String,
    element_type: String,
    answers_json: String,
    score: f64,
) -> Result<(), String> {
    let conn = db.conn();

    let score = score.clamp(0.0, 1.0);

    // Learner identifier — the local node's stake address. The completion
    // assembler doesn't read this column for built-ins, but the schema
    // requires it NOT NULL and it ties the row to the local learner.
    let learner_did: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "local-learner".to_string());

    // Reproducibility-bundle fields. Built-in assessments have no wasm
    // grader, so the grader id is a stable synthetic marker per element type
    // and the submission cid is the BLAKE3 of the raw answers.
    let submission_cid = blake3::hash(answers_json.as_bytes()).to_hex().to_string();
    let grader_cid = format!("builtin:{element_type}");
    let grader_version = "builtin-1";

    // Content the grader saw: the element's stored content cid, else a hash
    // of its inline content, else empty (the column is NOT NULL).
    let (content_cid, content_inline): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT content_cid, content_inline FROM course_elements WHERE id = ?1",
            params![element_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or((None, None));
    let content_cid = content_cid
        .filter(|c| !c.is_empty())
        .or_else(|| content_inline.map(|c| blake3::hash(c.as_bytes()).to_hex().to_string()))
        .unwrap_or_default();

    let id = entity_id(&[&element_id, &enrollment_id, &submission_cid]);

    conn.execute(
        "INSERT INTO element_submissions \
         (id, element_id, enrollment_id, submission_cid, grader_cid, content_cid, \
          score, answers_json, learner_did, grader_version, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now')) \
         ON CONFLICT(id) DO UPDATE SET \
            score = ?7, answers_json = ?8, created_at = datetime('now')",
        params![
            id,
            element_id,
            enrollment_id,
            submission_cid,
            grader_cid,
            content_cid,
            score,
            answers_json,
            learner_did,
            grader_version,
        ],
    )
    .map_err(|e| format!("failed to record element submission: {e}"))?;

    Ok(())
}

/// Return the learner's latest recorded response for an element so the
/// player can restore and display it on revisit. `None` if never submitted.
#[tauri::command]
pub async fn get_element_submission(
    state: State<'_, AppState>,
    enrollment_id: String,
    element_id: String,
) -> Result<Option<ElementSubmissionRecord>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "enrollment.get_submission",
            move |db| get_element_submission_db(db, &enrollment_id, &element_id),
        )
        .await
}

fn get_element_submission_db(
    db: &crate::db::Database,
    enrollment_id: &str,
    element_id: &str,
) -> Result<Option<ElementSubmissionRecord>, String> {
    let row = db
        .conn()
        .query_row(
            "SELECT element_id, answers_json, score, created_at FROM element_submissions \
             WHERE enrollment_id = ?1 AND element_id = ?2 \
             ORDER BY created_at DESC LIMIT 1",
            params![enrollment_id, element_id],
            |row| {
                Ok(ElementSubmissionRecord {
                    element_id: row.get(0)?,
                    answers_json: row.get(1)?,
                    score: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;

    Ok(row)
}
