//! Longitudinal consistency for an assessment attempt, read from the profile
//! database: the learner's practice, lessons and earlier attempts on the
//! attempt's skill, all from before the attempt started, compared with its
//! outcome by [`crate::sentinel::scoring::assess_consistency`].
//!
//! Nothing here comes from the client.

use rusqlite::{params, OptionalExtension};

use crate::sentinel::scoring::{
    assess_consistency, Consistency, EvidenceSource, History, Observation, RatioManifest,
    LOOKBACK_HOURS,
};

/// Computes the attempt's consistency once its outcome is known, and feeds
/// a divergence to the fusion of the attempt's integrity session (see
/// [`crate::sentinel::snapshots::decide_with_verification`]). Called by
/// both grading paths before a credential is issued, so the assertion
/// carries the resulting status.
pub(crate) fn apply_at_grading(
    conn: &rusqlite::Connection,
    attempt_id: &str,
    outcome: f64,
    manifest: &RatioManifest,
) -> Result<Consistency, String> {
    let consistency = for_attempt(conn, attempt_id, outcome)?;
    let signals = consistency.signals();
    if signals.is_empty() {
        return Ok(consistency);
    }
    let session: Option<String> = conn
        .query_row(
            "SELECT integrity_session_id FROM assessment_attempts WHERE id = ?1",
            params![attempt_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if let Some(session) = session {
        crate::sentinel::snapshots::decide_with_verification(conn, &session, &signals, manifest)?;
    }
    Ok(consistency)
}

/// Most observations read per source; recency weighting makes older ones
/// negligible anyway.
const MAX_PER_SOURCE: i64 = 50;

/// The consistency of `attempt_id` given its outcome (score in `[0, 1]`).
///
/// Reads only history from before the attempt started, so the answer for
/// an attempt does not change as the learner keeps practising afterwards.
pub(crate) fn for_attempt(
    conn: &rusqlite::Connection,
    attempt_id: &str,
    outcome: f64,
) -> Result<Consistency, String> {
    let (subject_did, skill_id, started_at): (String, String, String) = conn
        .query_row(
            "SELECT subject_did, skill_id, started_at FROM assessment_attempts WHERE id = ?1",
            params![attempt_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("attempt not found")?;
    let mut history = History::default();
    history
        .observations
        .extend(practice(conn, &skill_id, &started_at)?);
    history.observations.extend(prior_attempt_items(
        conn,
        attempt_id,
        &subject_did,
        &skill_id,
        &started_at,
    )?);
    for (age_hours, score) in lessons(conn, &skill_id, &started_at)? {
        match score {
            Some(outcome) => history.observations.push(Observation {
                source: EvidenceSource::Lesson,
                outcome,
                age_hours,
            }),
            None => history.lessons_completed.push(age_hours),
        }
    }
    Ok(assess_consistency(&history, outcome))
}

/// Completed lesson elements tagged with the skill: age in hours, and the
/// score for scored elements (quizzes).
fn lessons(
    conn: &rusqlite::Connection,
    skill_id: &str,
    started_at: &str,
) -> Result<Vec<(f64, Option<f64>)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT (julianday(?2) - julianday(done)) * 24.0, score FROM (
                 SELECT COALESCE(p.completed_at, p.updated_at) AS done, p.score AS score
                   FROM element_progress p
                   JOIN element_skill_tags t ON t.element_id = p.element_id
                  WHERE t.skill_id = ?1 AND p.status = 'completed'
             )
              WHERE julianday(done) < julianday(?2)
                AND julianday(done) >= julianday(?2) - ?3 / 24.0
              ORDER BY julianday(done) DESC LIMIT ?4",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            params![skill_id, started_at, LOOKBACK_HOURS, MAX_PER_SOURCE],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// Graded items of the learner's earlier attempts on the skill, by score.
fn prior_attempt_items(
    conn: &rusqlite::Connection,
    attempt_id: &str,
    subject_did: &str,
    skill_id: &str,
    started_at: &str,
) -> Result<Vec<Observation>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT (julianday(?4) - julianday(ai.graded_at)) * 24.0, ai.score
               FROM attempt_items ai
               JOIN assessment_attempts a ON a.id = ai.attempt_id
              WHERE a.id <> ?1 AND a.subject_did = ?2 AND a.skill_id = ?3
                AND ai.score IS NOT NULL
                AND julianday(ai.graded_at) < julianday(?4)
                AND julianday(ai.graded_at) >= julianday(?4) - ?5 / 24.0
              ORDER BY julianday(ai.graded_at) DESC LIMIT ?6",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            params![
                attempt_id,
                subject_did,
                skill_id,
                started_at,
                LOOKBACK_HOURS,
                MAX_PER_SOURCE
            ],
            |r| {
                Ok(Observation {
                    source: EvidenceSource::PriorAttempt,
                    age_hours: r.get(0)?,
                    outcome: r.get(1)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// Practice questions on the skill, by the first answer to each: right
/// without a hint or reveal counts 1, anything else 0. A question is on the
/// skill when the label it carries is the skill's name, or its thread is
/// bound to a lesson tagged with the skill.
fn practice(
    conn: &rusqlite::Connection,
    skill_id: &str,
    started_at: &str,
) -> Result<Vec<Observation>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT (julianday(?2) - julianday(pa.created_at)) * 24.0,
                    pa.correct AND NOT pa.assisted
               FROM chat_practice_items pi
               JOIN chat_practice_attempts pa ON pa.item_id = pi.id
              WHERE pa.rowid = (SELECT first.rowid FROM chat_practice_attempts first
                                 WHERE first.item_id = pi.id
                                 ORDER BY julianday(first.created_at), first.rowid LIMIT 1)
                AND julianday(pa.created_at) < julianday(?2)
                AND julianday(pa.created_at) >= julianday(?2) - ?3 / 24.0
                AND (lower(trim(json_extract(pi.question_json, '$.skill')))
                        = (SELECT lower(trim(name)) FROM skills WHERE id = ?1)
                     OR EXISTS (SELECT 1 FROM chat_bindings b
                                  JOIN element_skill_tags t ON t.element_id = b.element_id
                                 WHERE b.thread_id = pi.thread_id AND b.kind = 'lesson'
                                   AND t.skill_id = ?1))
              ORDER BY julianday(pa.created_at) DESC LIMIT ?4",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            params![skill_id, started_at, LOOKBACK_HOURS, MAX_PER_SOURCE],
            |r| {
                Ok(Observation {
                    source: EvidenceSource::Practice,
                    age_hours: r.get(0)?,
                    outcome: if r.get::<_, bool>(1)? { 1.0 } else { 0.0 },
                })
            },
        )
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::sentinel::scoring::{ConsistencyVerdict, ProfileRatios, DEFAULT_PROFILE};

    const STARTED: &str = "2026-11-10T12:00:00Z";

    fn db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO subject_fields (id, name) VALUES ('sf', 'Field');
                 INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub', 'Subject', 'sf');
                 INSERT INTO skills (id, name, subject_id) VALUES
                     ('skill_prob', 'Conditional probability', 'sub'),
                     ('skill_other', 'Linear algebra', 'sub');
                 INSERT INTO question_banks (id, skill_id, label, ratified)
                     VALUES ('bank', 'skill_prob', 'Probability', 1);
                 INSERT INTO integrity_sessions (id, status) VALUES ('sess', 'active');",
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO assessment_attempts
                     (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders,
                      integrity_session_id, started_at)
                 VALUES ('att', 'did:key:learner', 'bank', 'skill_prob', 1, '[]', '[]', 'sess', ?1)",
                [STARTED],
            )
            .unwrap();
        db
    }

    /// A practice question on `skill` (the label the model wrote) answered
    /// once at `at`, right or wrong, with no hint.
    fn practice(db: &Database, id: &str, skill: &str, correct: bool, at: &str) {
        use crate::ai_chat::{practice, store};
        let conn = db.conn();
        conn.execute(
            "INSERT OR IGNORE INTO chat_threads (id, mode, created_at, updated_at)
             VALUES ('thread', 'practice', ?1, ?1)",
            [at],
        )
        .unwrap();
        let message = format!("msg-{id}");
        conn.execute(
            "INSERT INTO chat_messages (id, thread_id, position, role, status, created_at)
             VALUES (?1, 'thread', (SELECT COUNT(*) FROM chat_messages), 'user', 'complete', ?2)",
            params![message, at],
        )
        .unwrap();
        let question = practice::Question {
            skill: skill.into(),
            kind: practice::Kind::MultipleChoice,
            prompt: format!("question {id}"),
            options: vec!["a".into(), "b".into()],
            answer: practice::Answer::Choice { index: 0 },
            hints: vec![],
            explanation: String::new(),
        };
        store::insert_practice(conn, id, "thread", &message, &question, at).unwrap();
        let answer = if correct { "0" } else { "1" };
        store::practice_attempt(conn, id, &format!("{id}-try"), answer, at).unwrap();
    }

    /// `n` practice answers on the attempt's skill in the day before it.
    fn recent_practice(db: &Database, n: usize, correct: bool) {
        for i in 0..n {
            let at = format!("2026-11-09T{:02}:00:00Z", 8 + i);
            practice(
                db,
                &format!("p{i}"),
                "Conditional probability",
                correct,
                &at,
            );
        }
    }

    #[test]
    fn passing_right_after_consistently_failed_practice_diverges() {
        let db = db();
        recent_practice(&db, 6, false);
        let result = for_attempt(db.conn(), "att", 1.0).unwrap();
        assert_eq!(result.verdict, ConsistencyVerdict::Divergent);
        assert!(!result.satisfied());
        assert_eq!(
            result.signals(),
            vec!["longitudinal_divergence".to_string()]
        );
    }

    #[test]
    fn passing_after_solid_practice_satisfies_the_rung() {
        let db = db();
        recent_practice(&db, 6, true);
        let result = for_attempt(db.conn(), "att", 1.0).unwrap();
        assert_eq!(result.verdict, ConsistencyVerdict::Satisfied);
        assert!(result.satisfied());
        assert!(result.signals().is_empty());
    }

    #[test]
    fn practice_on_another_skill_or_after_the_attempt_started_is_not_read() {
        let db = db();
        for i in 0..6 {
            practice(
                &db,
                &format!("other{i}"),
                "Linear algebra",
                false,
                "2026-11-09T08:00:00Z",
            );
            practice(
                &db,
                &format!("later{i}"),
                "Conditional probability",
                false,
                "2026-11-10T13:00:00Z",
            );
        }
        let result = for_attempt(db.conn(), "att", 1.0).unwrap();
        assert_eq!(result.verdict, ConsistencyVerdict::InsufficientHistory);
    }

    #[test]
    fn practice_from_a_thread_about_a_lesson_on_the_skill_counts_whatever_its_label() {
        let db = db();
        for i in 0..6 {
            let at = format!("2026-11-09T{:02}:00:00Z", 8 + i);
            practice(&db, &format!("p{i}"), "", false, &at);
        }
        assert_eq!(
            for_attempt(db.conn(), "att", 1.0).unwrap().verdict,
            ConsistencyVerdict::InsufficientHistory
        );
        lesson(&db, "lesson", "text");
        db.conn()
            .execute(
                "INSERT INTO chat_bindings (id, thread_id, kind, course_id, element_id, label, created_at)
                 VALUES ('bind', 'thread', 'lesson', 'course', 'lesson', 'Lesson', ?1)",
                [STARTED],
            )
            .unwrap();
        assert_eq!(
            for_attempt(db.conn(), "att", 1.0).unwrap().verdict,
            ConsistencyVerdict::Divergent
        );
    }

    /// A lesson element tagged with the attempt's skill.
    fn lesson(db: &Database, id: &str, element_type: &str) {
        db.conn()
            .execute_batch(
                "INSERT OR IGNORE INTO courses (id, title, author_address)
                     VALUES ('course', 'Course', 'addr');
                 INSERT OR IGNORE INTO course_chapters (id, course_id, title)
                     VALUES ('chapter', 'course', 'Chapter');
                 INSERT OR IGNORE INTO enrollments (id, course_id) VALUES ('enr', 'course');",
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type)
                 VALUES (?1, 'chapter', ?1, ?2)",
                params![id, element_type],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO element_skill_tags (element_id, skill_id) VALUES (?1, 'skill_prob')",
                [id],
            )
            .unwrap();
    }

    /// A graded earlier attempt on the skill, one item per score.
    fn prior_attempt(db: &Database, id: &str, at: &str, scores: &[f64]) {
        db.conn()
            .execute(
                "INSERT INTO assessment_attempts
                     (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders,
                      started_at, graded_at)
                 VALUES (?1, 'did:key:learner', 'bank', 'skill_prob', 1, '[]', '[]', ?2, ?2)",
                params![id, at],
            )
            .unwrap();
        for (ordinal, score) in scores.iter().enumerate() {
            db.conn()
                .execute(
                    "INSERT INTO attempt_items (attempt_id, ordinal, item_id, score, graded_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, ordinal as i64, format!("{id}-{ordinal}"), score, at],
                )
                .unwrap();
        }
    }

    #[test]
    fn graded_items_of_earlier_attempts_count_as_history() {
        let db = db();
        prior_attempt(&db, "earlier", "2026-11-08T12:00:00Z", &[0.0; 6]);
        assert_eq!(
            for_attempt(db.conn(), "att", 1.0).unwrap().verdict,
            ConsistencyVerdict::Divergent
        );
    }

    /// Marks a lesson completed at `at`, with a score for scored elements.
    fn complete(db: &Database, element: &str, score: Option<f64>, at: &str) {
        db.conn()
            .execute(
                "INSERT INTO element_progress
                     (id, enrollment_id, element_id, status, score, completed_at, updated_at)
                 VALUES (?1, 'enr', ?1, 'completed', ?2, ?3, ?3)",
                params![element, score, at],
            )
            .unwrap();
    }

    #[test]
    fn scored_quizzes_on_the_skill_count_as_history() {
        let db = db();
        for i in 0..6 {
            let id = format!("quiz{i}");
            lesson(&db, &id, "quiz");
            complete(&db, &id, Some(0.0), "2026-11-09 10:00:00");
        }
        assert_eq!(
            for_attempt(db.conn(), "att", 1.0).unwrap().verdict,
            ConsistencyVerdict::Divergent
        );
    }

    #[test]
    fn completed_lessons_alone_are_not_enough_history() {
        let db = db();
        for i in 0..10 {
            let id = format!("text{i}");
            lesson(&db, &id, "text");
            complete(&db, &id, None, "2026-11-09 10:00:00");
        }
        assert_eq!(
            for_attempt(db.conn(), "att", 1.0).unwrap().verdict,
            ConsistencyVerdict::InsufficientHistory
        );
    }

    #[test]
    fn completed_lessons_give_mixed_practice_the_benefit_of_the_doubt() {
        let db = db();
        for (i, correct) in [true, true, true, false, false].into_iter().enumerate() {
            let at = format!("2026-11-09T{:02}:00:00Z", 8 + i);
            practice(
                &db,
                &format!("p{i}"),
                "Conditional probability",
                correct,
                &at,
            );
        }
        assert_eq!(
            for_attempt(db.conn(), "att", 1.0).unwrap().verdict,
            ConsistencyVerdict::Divergent
        );
        for i in 0..8 {
            let id = format!("text{i}");
            lesson(&db, &id, "text");
            complete(&db, &id, None, "2026-11-09 06:00:00");
        }
        assert_eq!(
            for_attempt(db.conn(), "att", 1.0).unwrap().verdict,
            ConsistencyVerdict::Satisfied
        );
    }

    #[test]
    fn failures_from_months_ago_have_faded() {
        let db = db();
        for i in 0..6 {
            let at = format!("2026-08-20T{:02}:00:00Z", 8 + i);
            practice(&db, &format!("p{i}"), "Conditional probability", false, &at);
        }
        assert_eq!(
            for_attempt(db.conn(), "att", 1.0).unwrap().verdict,
            ConsistencyVerdict::InsufficientHistory
        );
    }

    #[test]
    fn doing_worse_than_practice_suggests_never_diverges() {
        let db = db();
        recent_practice(&db, 6, true);
        let result = for_attempt(db.conn(), "att", 0.0).unwrap();
        assert!(result.signals().is_empty());
    }

    /// Ratios under which a divergent attempt alone puts a session over
    /// threshold.
    fn measured() -> RatioManifest {
        let mut manifest = RatioManifest::uncertified();
        manifest.version = "test-certified".into();
        manifest.profiles.insert(
            DEFAULT_PROFILE.into(),
            ProfileRatios {
                threshold: 2.0,
                ratios: [("longitudinal_divergence".to_string(), 3.0)].into(),
            },
        );
        manifest
    }

    fn session_status(db: &Database) -> String {
        db.conn()
            .query_row(
                "SELECT status FROM integrity_sessions WHERE id = 'sess'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn end_session(db: &Database) {
        db.conn()
            .execute(
                "UPDATE integrity_sessions SET status = 'completed', ended_at = ?1 WHERE id = 'sess'",
                [STARTED],
            )
            .unwrap();
    }

    #[test]
    fn a_measured_divergence_sends_the_finished_session_to_review() {
        let db = db();
        recent_practice(&db, 6, false);
        end_session(&db);
        let result = apply_at_grading(db.conn(), "att", 1.0, &measured()).unwrap();
        assert_eq!(result.verdict, ConsistencyVerdict::Divergent);
        assert_eq!(session_status(&db), "review");
    }

    #[test]
    fn an_unmeasured_divergence_is_advisory_and_leaves_the_session_alone() {
        let db = db();
        recent_practice(&db, 6, false);
        end_session(&db);
        apply_at_grading(db.conn(), "att", 1.0, &RatioManifest::uncertified()).unwrap();
        assert_eq!(session_status(&db), "completed");
    }

    #[test]
    fn a_consistent_attempt_leaves_the_session_alone() {
        let db = db();
        recent_practice(&db, 6, true);
        end_session(&db);
        apply_at_grading(db.conn(), "att", 1.0, &measured()).unwrap();
        assert_eq!(session_status(&db), "completed");
    }

    #[test]
    fn a_learner_with_no_history_on_the_skill_does_not_satisfy_the_rung() {
        let db = db();
        let result = for_attempt(db.conn(), "att", 1.0).unwrap();
        assert_eq!(result.verdict, ConsistencyVerdict::InsufficientHistory);
        assert!(!result.satisfied());
    }
}
