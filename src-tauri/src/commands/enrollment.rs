use rusqlite::params;
use tauri::State;

use crate::crypto::hash::entity_id;
use crate::domain::enrollment::{ElementProgress, Enrollment, UpdateProgressRequest};
use crate::evidence::{aggregator, reputation};
use crate::AppState;

/// List all enrollments for the local user.
#[tauri::command]
pub async fn list_enrollments(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<Enrollment>, String> {
    let db = state.db.lock().await;

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(ref s) = status {
            (
                "SELECT id, course_id, enrolled_at, completed_at, status, updated_at \
                 FROM enrollments WHERE status = ?1 ORDER BY enrolled_at DESC"
                    .to_string(),
                vec![Box::new(s.clone())],
            )
        } else {
            (
                "SELECT id, course_id, enrolled_at, completed_at, status, updated_at \
                 FROM enrollments ORDER BY enrolled_at DESC"
                    .to_string(),
                vec![],
            )
        };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = db.conn().prepare(&sql).map_err(|e| e.to_string())?;

    let enrollments = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(Enrollment {
                id: row.get(0)?,
                course_id: row.get(1)?,
                enrolled_at: row.get(2)?,
                completed_at: row.get(3)?,
                status: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(enrollments)
}

/// Enroll in a course.
#[tauri::command]
pub async fn enroll(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Enrollment, String> {
    let db = state.db.lock().await;

    // Get the local user's stake address for deterministic ID
    let stake_address: String = db
        .conn()
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no identity found — generate a wallet first: {}", e))?;

    // Verify course exists
    let course_exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM courses WHERE id = ?1",
            params![course_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if !course_exists {
        return Err("course not found".into());
    }

    // Check for existing active enrollment
    let already_enrolled: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM enrollments WHERE course_id = ?1 AND status = 'active'",
            params![course_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if already_enrolled {
        return Err("already enrolled in this course".into());
    }

    let id = entity_id(&[&stake_address, &course_id]);

    db.conn()
        .execute(
            "INSERT INTO enrollments (id, course_id) VALUES (?1, ?2)",
            params![id, course_id],
        )
        .map_err(|e| e.to_string())?;

    db.conn()
        .query_row(
            "SELECT id, course_id, enrolled_at, completed_at, status, updated_at \
             FROM enrollments WHERE id = ?1",
            params![id],
            |row| {
                Ok(Enrollment {
                    id: row.get(0)?,
                    course_id: row.get(1)?,
                    enrolled_at: row.get(2)?,
                    completed_at: row.get(3)?,
                    status: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}

/// Update progress on a course element.
#[tauri::command]
pub async fn update_progress(
    state: State<'_, AppState>,
    enrollment_id: String,
    req: UpdateProgressRequest,
) -> Result<ElementProgress, String> {
    let db = state.db.lock().await;

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

    // Trigger evidence pipeline on completion with a score
    if req.status == "completed" {
        if let Some(score) = req.score {
            // Get course_id and stake_address for evidence creation
            let course_id: String = db
                .conn()
                .query_row(
                    "SELECT course_id FROM enrollments WHERE id = ?1",
                    params![enrollment_id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;

            let stake_address: String = db
                .conn()
                .query_row(
                    "SELECT stake_address FROM local_identity WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;

            // Create evidence records for each skill tagged on this element
            let skills = aggregator::create_evidence_for_element(
                db.conn(),
                &course_id,
                &req.element_id,
                score,
                &stake_address,
            )
            .map_err(|e| format!("evidence creation failed: {}", e))?;

            // Evaluate and update proofs + reputation for each skill
            for skill_id in &skills {
                match aggregator::evaluate_and_update(db.conn(), &stake_address, skill_id) {
                    Ok(result) => {
                        if let (Some(level), Some(ref proof_id)) =
                            (result.achieved_level, &result.proof_id)
                        {
                            if result.confidence > result.old_confidence {
                                let _ = reputation::on_proof_updated(
                                    db.conn(),
                                    &stake_address,
                                    skill_id,
                                    result.old_confidence,
                                    result.confidence,
                                    level.as_str(),
                                    proof_id,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("proof evaluation failed for skill {}: {}", skill_id, e);
                    }
                }
            }
        }
    }

    Ok(progress)
}

/// Get all progress for an enrollment.
#[tauri::command]
pub async fn get_progress(
    state: State<'_, AppState>,
    enrollment_id: String,
) -> Result<Vec<ElementProgress>, String> {
    let db = state.db.lock().await;

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
