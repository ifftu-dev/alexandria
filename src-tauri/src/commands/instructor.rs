//! Instructor dashboard + unified inbox IPC commands.
//!
//! All queries are scoped to courses authored by the active identity
//! (`courses.author_address = local stake address`) — instructor A must
//! never see instructor B's data even if both courses are cached
//! locally.
//!
//! Local-first caveat: enrollment/progress rows live on the enrolling
//! learner's device. These aggregates therefore cover what THIS node
//! knows — locally recorded enrollments, IRL-review submissions queued
//! on this node (in-class flow), and gossip-synced classroom join
//! requests. Network-wide enrollment telemetry is deliberately out of
//! scope.

use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::State;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct CourseOverview {
    pub course_id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub enrollment_count: i64,
    pub completed_count: i64,
    pub avg_score: Option<f64>,
    pub last_activity: Option<String>,
    pub pending_reviews: i64,
}

#[derive(Debug, Serialize)]
pub struct CourseLearner {
    pub learner_did: Option<String>,
    pub enrollment_id: String,
    pub display_name: Option<String>,
    pub enrolled_at: String,
    pub enrollment_status: String,
    pub completed_elements: i64,
    pub total_elements: i64,
    pub avg_score: Option<f64>,
    pub time_spent_seconds: i64,
    pub last_activity: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InboxItem {
    /// `irl_submission` | `join_request`
    pub kind: String,
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub created_at: String,
    /// Route hint for the frontend (submission id / classroom id).
    pub target_id: String,
}

fn local_stake_address(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT stake_address FROM local_identity WHERE id = 1",
        [],
        |row| row.get(0),
    )
    .map_err(|e| format!("no identity found: {e}"))
}

pub(crate) fn instructor_overview_impl(conn: &Connection) -> Result<Vec<CourseOverview>, String> {
    let author = local_stake_address(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT c.id, c.title, c.kind, c.status, \
             (SELECT COUNT(*) FROM enrollments e WHERE e.course_id = c.id), \
             (SELECT COUNT(*) FROM enrollments e WHERE e.course_id = c.id AND e.status = 'completed'), \
             (SELECT AVG(ep.score) FROM element_progress ep \
                JOIN enrollments e ON e.id = ep.enrollment_id \
                WHERE e.course_id = c.id AND ep.score IS NOT NULL), \
             (SELECT MAX(ep.updated_at) FROM element_progress ep \
                JOIN enrollments e ON e.id = ep.enrollment_id \
                WHERE e.course_id = c.id), \
             (SELECT COUNT(*) FROM plugin_irl_submissions pis \
                JOIN course_elements el ON el.id = pis.element_id \
                JOIN course_chapters ch ON ch.id = el.chapter_id \
                WHERE ch.course_id = c.id AND pis.status = 'pending') \
             FROM courses c WHERE c.author_address = ?1 \
             ORDER BY c.updated_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![author], |row| {
            Ok(CourseOverview {
                course_id: row.get(0)?,
                title: row.get(1)?,
                kind: row.get(2)?,
                status: row.get(3)?,
                enrollment_count: row.get(4)?,
                completed_count: row.get(5)?,
                avg_score: row.get(6)?,
                last_activity: row.get(7)?,
                pending_reviews: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Per-course aggregate stats for every course the active identity authored.
#[tauri::command]
pub async fn instructor_overview(
    state: State<'_, AppState>,
) -> Result<Vec<CourseOverview>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    instructor_overview_impl(db.conn())
}

pub(crate) fn instructor_course_learners_impl(
    conn: &Connection,
    course_id: &str,
) -> Result<Vec<CourseLearner>, String> {
    let author = local_stake_address(conn)?;
    // Author isolation: refuse to aggregate someone else's course.
    let owns: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM courses WHERE id = ?1 AND author_address = ?2",
            params![course_id, author],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !owns {
        return Err("course not found or not authored by you".into());
    }

    let total_elements: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM course_elements el \
             JOIN course_chapters ch ON ch.id = el.chapter_id \
             WHERE ch.course_id = ?1",
            params![course_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT e.id, e.enrolled_at, e.status, \
             (SELECT COUNT(*) FROM element_progress ep WHERE ep.enrollment_id = e.id AND ep.status = 'completed'), \
             (SELECT AVG(ep.score) FROM element_progress ep WHERE ep.enrollment_id = e.id AND ep.score IS NOT NULL), \
             (SELECT COALESCE(SUM(ep.time_spent), 0) FROM element_progress ep WHERE ep.enrollment_id = e.id), \
             (SELECT MAX(ep.updated_at) FROM element_progress ep WHERE ep.enrollment_id = e.id), \
             (SELECT es.learner_did FROM element_submissions es WHERE es.enrollment_id = e.id LIMIT 1) \
             FROM enrollments e WHERE e.course_id = ?1 \
             ORDER BY e.enrolled_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let mut learners = stmt
        .query_map(params![course_id], |row| {
            Ok(CourseLearner {
                enrollment_id: row.get(0)?,
                enrolled_at: row.get(1)?,
                enrollment_status: row.get(2)?,
                completed_elements: row.get(3)?,
                avg_score: row.get(4)?,
                time_spent_seconds: row.get(5)?,
                last_activity: row.get(6)?,
                learner_did: row.get(7)?,
                display_name: None,
                total_elements,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    // Resolve display names from the peer-profile cache, best-effort.
    for l in &mut learners {
        if let Some(did) = &l.learner_did {
            l.display_name = conn
                .query_row(
                    "SELECT display_name FROM peer_profiles WHERE did = ?1",
                    params![did],
                    |row| row.get(0),
                )
                .ok()
                .flatten();
        }
    }
    Ok(learners)
}

/// Per-learner progress rows for one authored course.
#[tauri::command]
pub async fn instructor_course_learners(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Vec<CourseLearner>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    instructor_course_learners_impl(db.conn(), &course_id)
}

pub(crate) fn instructor_inbox_impl(conn: &Connection) -> Result<Vec<InboxItem>, String> {
    let author = local_stake_address(conn)?;
    let mut items: Vec<InboxItem> = Vec::new();

    // Pending IRL-review submissions (in-class manual review queue).
    let mut stmt = conn
        .prepare(
            "SELECT pis.id, pis.learner_did, pis.created_at, \
             COALESCE(el.title, pis.plugin_cid), \
             (SELECT pp.display_name FROM peer_profiles pp WHERE pp.did = pis.learner_did) \
             FROM plugin_irl_submissions pis \
             LEFT JOIN course_elements el ON el.id = pis.element_id \
             WHERE pis.status = 'pending' \
             ORDER BY pis.created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let irl = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let learner_did: String = row.get(1)?;
            let created_at: String = row.get(2)?;
            let element_title: String = row.get(3)?;
            let display_name: Option<String> = row.get(4)?;
            Ok(InboxItem {
                kind: "irl_submission".into(),
                target_id: id.clone(),
                id,
                title: element_title,
                subtitle: Some(display_name.unwrap_or(learner_did)),
                created_at,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    items.extend(irl);

    // Pending join requests for classrooms this identity owns.
    let mut stmt = conn
        .prepare(
            "SELECT jr.id, jr.classroom_id, jr.display_name, jr.stake_address, jr.requested_at, c.name \
             FROM classroom_join_requests jr \
             JOIN classrooms c ON c.id = jr.classroom_id \
             WHERE jr.status = 'pending' AND c.owner_address = ?1 \
             ORDER BY jr.requested_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let joins = stmt
        .query_map(params![author], |row| {
            let id: String = row.get(0)?;
            let classroom_id: String = row.get(1)?;
            let display_name: Option<String> = row.get(2)?;
            let stake_address: String = row.get(3)?;
            let requested_at: String = row.get(4)?;
            let classroom_name: String = row.get(5)?;
            Ok(InboxItem {
                kind: "join_request".into(),
                id,
                title: format!("Join request — {classroom_name}"),
                subtitle: Some(display_name.unwrap_or(stake_address)),
                created_at: requested_at,
                target_id: classroom_id,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    items.extend(joins);

    items.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(items)
}

/// Unified instructor inbox: pending IRL submissions + classroom join
/// requests for owned classrooms, oldest first.
#[tauri::command]
pub async fn instructor_inbox(state: State<'_, AppState>) -> Result<Vec<InboxItem>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    instructor_inbox_impl(db.conn())
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

    fn seed_identity(db: &Database, stake: &str) {
        db.conn()
            .execute(
                "INSERT OR REPLACE INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, ?1, 'addr_test1q')",
                params![stake],
            )
            .unwrap();
    }

    fn seed_course(db: &Database, id: &str, author: &str) {
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) VALUES (?1, ?1, ?2)",
                params![id, author],
            )
            .unwrap();
    }

    #[test]
    fn overview_only_shows_own_courses() {
        let db = test_db();
        seed_identity(&db, "stake_me");
        seed_course(&db, "mine", "stake_me");
        seed_course(&db, "theirs", "stake_other");

        let rows = instructor_overview_impl(db.conn()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].course_id, "mine");
    }

    #[test]
    fn course_learners_refuses_foreign_course() {
        let db = test_db();
        seed_identity(&db, "stake_me");
        seed_course(&db, "theirs", "stake_other");

        assert!(instructor_course_learners_impl(db.conn(), "theirs").is_err());
        assert!(instructor_course_learners_impl(db.conn(), "missing").is_err());
    }

    #[test]
    fn course_learners_aggregates_progress() {
        let db = test_db();
        seed_identity(&db, "stake_me");
        seed_course(&db, "c1", "stake_me");
        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) VALUES ('ch1', 'c1', 'Ch', 0)",
                [],
            )
            .unwrap();
        for el in ["e1", "e2"] {
            db.conn()
                .execute(
                    "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
                     VALUES (?1, 'ch1', ?1, 'text', 0)",
                    params![el],
                )
                .unwrap();
        }
        db.conn()
            .execute(
                "INSERT INTO enrollments (id, course_id, status) VALUES ('en1', 'c1', 'active')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent) \
                 VALUES ('p1', 'en1', 'e1', 'completed', 0.8, 120)",
                [],
            )
            .unwrap();

        let learners = instructor_course_learners_impl(db.conn(), "c1").unwrap();
        assert_eq!(learners.len(), 1);
        assert_eq!(learners[0].completed_elements, 1);
        assert_eq!(learners[0].total_elements, 2);
        assert_eq!(learners[0].time_spent_seconds, 120);
        assert!((learners[0].avg_score.unwrap() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn inbox_scopes_join_requests_to_owned_classrooms() {
        let db = test_db();
        seed_identity(&db, "stake_me");
        db.conn()
            .execute(
                "INSERT INTO classrooms (id, name, owner_address, invite_code) \
                 VALUES ('cl_mine', 'Mine', 'stake_me', 'inv1'), \
                        ('cl_other', 'Other', 'stake_other', 'inv2')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO classroom_join_requests (id, classroom_id, stake_address, status) \
                 VALUES ('jr1', 'cl_mine', 'stake_kid', 'pending'), \
                        ('jr2', 'cl_other', 'stake_kid', 'pending'), \
                        ('jr3', 'cl_mine', 'stake_kid2', 'approved')",
                [],
            )
            .unwrap();

        let items = instructor_inbox_impl(db.conn()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "jr1");
        assert_eq!(items[0].kind, "join_request");
    }
}
