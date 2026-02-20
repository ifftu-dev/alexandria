//! Reputation computation from skill proofs.
//!
//! Computes instructor impact and learner reputation when skill
//! proofs are created or updated. Port of
//! `api/internal/reputation/aggregator.go` from v1.

use rusqlite::{params, Connection};

use crate::crypto::hash::entity_id;

/// Confidence smoothing constant (K).
/// Smoothed confidence = evidence_count / (evidence_count + K)
const SMOOTHING_K: f64 = 5.0;

/// Called after a skill proof is created or updated.
///
/// Computes:
///   1. Instructor impact (attribution-weighted delta confidence)
///   2. Learner reputation (mirrors proof confidence directly)
pub fn on_proof_updated(
    conn: &Connection,
    stake_address: &str,
    skill_id: &str,
    old_confidence: f64,
    new_confidence: f64,
    proficiency_level: &str,
    _proof_id: &str,
) -> Result<(), String> {
    // Always update learner reputation (mirrors proof)
    compute_learner_reputation(
        conn,
        stake_address,
        skill_id,
        new_confidence,
        proficiency_level,
    )?;

    // Instructor impact only on positive delta
    let delta = new_confidence - old_confidence;
    if delta > 0.0 {
        compute_instructor_impact(conn, skill_id, delta, proficiency_level)?;
    }

    Ok(())
}

/// Compute instructor impact from evidence attribution.
///
/// Traces evidence_records → skill_assessments → courses → author_address
/// to determine each instructor's contribution. Attribution is
/// proportional to (weight * difficulty) share.
fn compute_instructor_impact(
    conn: &Connection,
    skill_id: &str,
    delta: f64,
    proficiency_level: &str,
) -> Result<(), String> {
    // Query all evidence attributable to instructors for this skill
    let mut stmt = conn
        .prepare(
            "SELECT c.author_address, sa.weight, er.difficulty \
             FROM evidence_records er \
             JOIN skill_assessments sa ON sa.id = er.skill_assessment_id \
             JOIN courses c ON c.id = er.course_id \
             WHERE er.skill_id = ?1 AND er.course_id IS NOT NULL",
        )
        .map_err(|e| e.to_string())?;

    struct AttrRow {
        instructor_address: String,
        weight: f64,
        difficulty: f64,
    }

    let attributions: Vec<AttrRow> = {
        let rows = stmt
            .query_map(params![skill_id], |row| {
                Ok(AttrRow {
                    instructor_address: row.get(0)?,
                    weight: row.get(1)?,
                    difficulty: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    if attributions.is_empty() {
        return Ok(());
    }

    // Compute per-instructor weight and total weight
    let mut instructor_weights: std::collections::HashMap<String, f64> =
        std::collections::HashMap::new();
    let mut total_weight = 0.0;

    for attr in &attributions {
        let w = attr.weight * attr.difficulty;
        *instructor_weights
            .entry(attr.instructor_address.clone())
            .or_insert(0.0) += w;
        total_weight += w;
    }

    if total_weight == 0.0 {
        return Ok(());
    }

    // Upsert reputation assertion for each instructor
    for (instructor_addr, instr_weight) in &instructor_weights {
        let attribution = instr_weight / total_weight;
        let impact_delta = delta * attribution;

        let assertion_id = entity_id(&[instructor_addr, "instructor", skill_id, proficiency_level]);

        // Try to load existing assertion
        let existing: Option<(f64, i64)> = conn
            .query_row(
                "SELECT score, evidence_count FROM reputation_assertions WHERE id = ?1",
                params![assertion_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        let (new_score, new_evidence_count) = match existing {
            Some((old_score, old_count)) => (old_score + impact_delta, old_count + 1),
            None => (impact_delta, 1_i64),
        };

        // Apply confidence smoothing
        let _smoothed = new_evidence_count as f64 / (new_evidence_count as f64 + SMOOTHING_K);

        conn.execute(
            "INSERT INTO reputation_assertions \
             (id, actor_address, role, skill_id, proficiency_level, score, evidence_count, \
              computation_spec, updated_at) \
             VALUES (?1, ?2, 'instructor', ?3, ?4, ?5, ?6, 'v2', datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
                score = ?5, evidence_count = ?6, updated_at = datetime('now')",
            params![
                assertion_id,
                instructor_addr,
                skill_id,
                proficiency_level,
                new_score,
                new_evidence_count,
            ],
        )
        .map_err(|e| e.to_string())?;

        log::info!(
            "instructor reputation: {} skill={} delta={:.4} (attribution={:.2}), total={:.4}",
            instructor_addr,
            skill_id,
            impact_delta,
            attribution,
            new_score
        );
    }

    Ok(())
}

/// Compute learner reputation — mirrors proof confidence directly.
///
/// No smoothing for learner reputation: the demonstrated ability IS
/// the reputation score.
fn compute_learner_reputation(
    conn: &Connection,
    stake_address: &str,
    skill_id: &str,
    confidence: f64,
    proficiency_level: &str,
) -> Result<(), String> {
    let assertion_id = entity_id(&[stake_address, "learner", skill_id, proficiency_level]);

    // Get current evidence count
    let current_count: i64 = conn
        .query_row(
            "SELECT evidence_count FROM reputation_assertions WHERE id = ?1",
            params![assertion_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO reputation_assertions \
         (id, actor_address, role, skill_id, proficiency_level, score, evidence_count, \
          computation_spec, updated_at) \
         VALUES (?1, ?2, 'learner', ?3, ?4, ?5, ?6, 'v2', datetime('now')) \
         ON CONFLICT(id) DO UPDATE SET \
            score = ?5, evidence_count = ?6, updated_at = datetime('now')",
        params![
            assertion_id,
            stake_address,
            skill_id,
            proficiency_level,
            confidence,
            current_count + 1,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::evidence::aggregator;

    fn setup_db() -> Database {
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrations");

        db.conn()
            .execute(
                "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'CS')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub1', 'Algo', 'sf1')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO skills (id, name, subject_id) VALUES ('sk1', 'Graphs', 'sub1')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1ulearner', 'addr_test1q123')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) \
                 VALUES ('c1', 'Algo 101', 'stake_test1uinstructor')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) \
                 VALUES ('ch1', 'c1', 'Ch1', 0)",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
                 VALUES ('el1', 'ch1', 'Quiz', 'quiz', 0)",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO element_skill_tags (element_id, skill_id, weight) \
                 VALUES ('el1', 'sk1', 1.0)",
                [],
            )
            .unwrap();

        db
    }

    #[test]
    fn instructor_reputation_created_on_proof_update() {
        let db = setup_db();
        let conn = db.conn();

        // Create evidence and evaluate
        aggregator::create_evidence_for_element(conn, "c1", "el1", 0.80, "stake_test1ulearner")
            .unwrap();
        let result = aggregator::evaluate_and_update(conn, "stake_test1ulearner", "sk1").unwrap();

        // Trigger reputation callback
        on_proof_updated(
            conn,
            "stake_test1ulearner",
            "sk1",
            result.old_confidence,
            result.confidence,
            result.achieved_level.unwrap().as_str(),
            result.proof_id.as_ref().unwrap(),
        )
        .unwrap();

        // Check instructor reputation exists
        let (score, role): (f64, String) = conn
            .query_row(
                "SELECT score, role FROM reputation_assertions WHERE actor_address = 'stake_test1uinstructor'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("instructor reputation should exist");

        assert_eq!(role, "instructor");
        assert!(score > 0.0);
    }

    #[test]
    fn learner_reputation_mirrors_proof() {
        let db = setup_db();
        let conn = db.conn();

        aggregator::create_evidence_for_element(conn, "c1", "el1", 0.80, "stake_test1ulearner")
            .unwrap();
        let result = aggregator::evaluate_and_update(conn, "stake_test1ulearner", "sk1").unwrap();

        on_proof_updated(
            conn,
            "stake_test1ulearner",
            "sk1",
            result.old_confidence,
            result.confidence,
            result.achieved_level.unwrap().as_str(),
            result.proof_id.as_ref().unwrap(),
        )
        .unwrap();

        // Learner reputation should match proof confidence
        let score: f64 = conn
            .query_row(
                "SELECT score FROM reputation_assertions \
                 WHERE actor_address = 'stake_test1ulearner' AND role = 'learner'",
                [],
                |row| row.get(0),
            )
            .expect("learner reputation should exist");

        assert!((score - result.confidence).abs() < 0.001);
    }

    #[test]
    fn no_reputation_on_zero_delta() {
        let db = setup_db();
        let conn = db.conn();

        // Call with zero delta — no instructor reputation should be created
        on_proof_updated(
            conn,
            "stake_test1ulearner",
            "sk1",
            0.80,
            0.80, // same confidence = zero delta
            "remember",
            "proof_123",
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reputation_assertions WHERE role = 'instructor'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
