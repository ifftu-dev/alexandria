use rusqlite::params;
use tauri::State;

use crate::crypto::hash::entity_id;
use crate::domain::enrollment::{ElementProgress, Enrollment, UpdateProgressRequest};
use crate::evidence::{aggregator, reputation};
use crate::AppState;

use crate::crypto::wallet;
use crate::p2p::evidence as p2p_evidence;
use crate::p2p::types::TOPIC_EVIDENCE;

/// List all enrollments for the local user.
#[tauri::command]
pub async fn list_enrollments(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<Enrollment>, String> {
    let db = state.db.lock().unwrap();

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
    let db = state.db.lock().unwrap();

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
    // All DB work in a block so the guard is dropped before any .await
    let (progress, broadcast_data) = {
        let db = state.db.lock().unwrap();

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
        // Collect broadcast data while holding the DB lock
        let mut broadcast_data: Vec<(
            crate::domain::evidence::EvidenceAnnouncement,
            String, // stake_address
        )> = Vec::new();

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
                    req.integrity_session_id.as_deref(),
                    req.integrity_score,
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

                // Collect un-sent evidence for P2P broadcast
                for skill_id in &skills {
                    match p2p_evidence::collect_evidence_for_broadcast(&db, skill_id) {
                        Ok(rows) => {
                            for row in rows {
                                let ann = p2p_evidence::build_evidence_announcement(
                                    &row.evidence_id,
                                    &stake_address,
                                    skill_id,
                                    &row.proficiency_level,
                                    &row.assessment_id,
                                    row.score,
                                    row.difficulty,
                                    row.trust_factor,
                                    row.course_id.as_deref(),
                                    row.instructor_address.as_deref(),
                                );
                                broadcast_data.push((ann, stake_address.clone()));
                            }
                        }
                        Err(e) => {
                            log::warn!("failed to collect evidence for broadcast: {e}");
                        }
                    }
                }
            }
        }

        (progress, broadcast_data)
    }; // db guard dropped here — before any .await

    // Broadcast evidence to P2P network (best-effort, don't fail progress update)
    if !broadcast_data.is_empty() {
        // Get signing key from vault first
        let signing_key_result = {
            let ks_guard = state.keystore.lock().await;
            match ks_guard.as_ref() {
                Some(ks) => ks
                    .retrieve_mnemonic()
                    .map_err(|e| e.to_string())
                    .and_then(|m| wallet::wallet_from_mnemonic(&m).map_err(|e| e.to_string())),
                None => Err("vault locked".to_string()),
            }
        };

        if let Ok(w) = signing_key_result {
            // Sign all messages and mark as sent in DB (synchronous, with DB lock)
            let signed_messages: Vec<_> = {
                let db = state.db.lock().unwrap();
                broadcast_data
                    .iter()
                    .filter_map(|(ann, stake_addr)| {
                        let payload = match serde_json::to_vec(ann) {
                            Ok(p) => p,
                            Err(e) => {
                                log::warn!("failed to serialize evidence announcement: {e}");
                                return None;
                            }
                        };

                        let signed = crate::p2p::signing::sign_gossip_message(
                            TOPIC_EVIDENCE,
                            payload,
                            &w.signing_key,
                            stake_addr,
                        );
                        let sig_hex = hex::encode(&signed.signature);

                        // Mark as sent in sync_log
                        let _ = p2p_evidence::mark_evidence_broadcast(
                            &db,
                            &ann.evidence_id,
                            &sig_hex,
                        );

                        Some((signed, ann.evidence_id.clone(), ann.skill_id.clone()))
                    })
                    .collect()
            }; // db guard dropped here — before async publish

            // Now publish all signed messages (async, no DB lock held)
            let p2p_node = state.p2p_node.lock().await;
            if let Some(ref node) = *p2p_node {
                for (signed, evidence_id, skill_id) in &signed_messages {
                    match node.publish_signed(signed).await {
                        Ok(()) => {
                            log::info!(
                                "Broadcast evidence '{}' for skill '{}'",
                                evidence_id,
                                skill_id,
                            );
                        }
                        Err(e) => {
                            log::warn!("Failed to broadcast evidence: {e}");
                        }
                    }
                }
            }
        }
    }

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
    let db = state.db.lock().unwrap();

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
