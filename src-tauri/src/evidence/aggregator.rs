//! Skill proof aggregation.
//!
//! Implements the evidence → proof pipeline:
//!   1. When a learner completes an element, evidence records are created
//!      for each skill tagged on that element.
//!   2. `evaluate_and_update` computes weighted confidence from all
//!      evidence for a skill and determines the highest achieved
//!      proficiency level.
//!   3. Proofs are created/updated in the `skill_proofs` table.
//!
//! Port of `api/internal/skillproof/aggregator.go` from v1.

use rusqlite::{params, Connection};

use crate::crypto::hash::entity_id;
use crate::domain::evidence::ProficiencyLevel;
use crate::evidence::thresholds::THRESHOLDS;

/// Result of evaluating evidence for a skill.
#[derive(Debug, Clone)]
pub struct AggregationResult {
    /// The highest proficiency level achieved, if any.
    pub achieved_level: Option<ProficiencyLevel>,
    /// Weighted confidence score (0.0 to 1.0).
    pub confidence: f64,
    /// Number of evidence records considered.
    pub evidence_count: usize,
    /// The proof ID (blake2b hash), if a proof was created/updated.
    pub proof_id: Option<String>,
    /// Previous confidence (for reputation delta computation).
    pub old_confidence: f64,
}

/// Evidence data for aggregation (from evidence_records + skill_assessments).
struct EvidenceItem {
    score: f64,
    weight: f64,
    difficulty: f64,
    trust_factor: f64,
    assessment_type: String,
}

/// Create evidence records for an element completion.
///
/// Looks up skill tags on the element, finds or auto-creates
/// assessments, and creates evidence records. Then triggers
/// proof aggregation for each skill.
///
/// Returns the list of skill IDs that were evaluated.
pub fn create_evidence_for_element(
    conn: &Connection,
    course_id: &str,
    element_id: &str,
    score: f64,
    stake_address: &str,
    integrity_session_id: Option<&str>,
    integrity_score: Option<f64>,
) -> Result<Vec<String>, String> {
    // Look up skills tagged on this element
    let mut stmt = conn
        .prepare("SELECT skill_id, weight FROM element_skill_tags WHERE element_id = ?1")
        .map_err(|e| e.to_string())?;

    let skill_tags: Vec<(String, f64)> = {
        let rows = stmt
            .query_map(params![element_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    if skill_tags.is_empty() {
        return Ok(vec![]);
    }

    // Get the instructor (course author) address and course kind
    // (tutorials earn a lower trust_factor than full courses — see
    // migration 020 and publish_tutorial).
    let (instructor_address, course_kind): (Option<String>, String) = conn
        .query_row(
            "SELECT author_address, kind FROM courses WHERE id = ?1",
            params![course_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?
                        .unwrap_or_else(|| "course".into()),
                ))
            },
        )
        .unwrap_or((None, "course".into()));

    let mut evaluated_skills = Vec::new();
    let now = chrono::Utc::now().to_rfc3339();

    for (skill_id, _tag_weight) in &skill_tags {
        // Find or auto-create a skill_assessment for (course, element, skill)
        let assessment_id =
            find_or_create_assessment(conn, course_id, element_id, skill_id, &course_kind)?;

        // Get assessment details for denormalization
        let (difficulty, trust_factor, weight, proficiency_level, assessment_type): (
            f64,
            f64,
            f64,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT difficulty, trust_factor, weight, proficiency_level, assessment_type \
                 FROM skill_assessments WHERE id = ?1",
                params![assessment_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(|e| e.to_string())?;

        // Create evidence record with deterministic ID
        let evidence_id = entity_id(&[stake_address, &assessment_id, &now, skill_id]);

        conn.execute(
            "INSERT OR IGNORE INTO evidence_records \
             (id, skill_assessment_id, skill_id, proficiency_level, score, \
              difficulty, trust_factor, course_id, instructor_address, \
              integrity_session_id, integrity_score, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, datetime('now'))",
            params![
                evidence_id,
                assessment_id,
                skill_id,
                proficiency_level,
                score,
                difficulty,
                trust_factor,
                course_id,
                instructor_address,
                integrity_session_id,
                integrity_score,
            ],
        )
        .map_err(|e| e.to_string())?;

        let _ = weight;
        let _ = assessment_type;

        evaluated_skills.push(skill_id.clone());
    }

    Ok(evaluated_skills)
}

/// Find or auto-create a skill assessment for (course, element, skill).
///
/// Defaults: proficiency_level=apply, assessment_type=quiz,
/// difficulty=0.50, weight=1.0. `trust_factor` depends on `course_kind`:
///   - `"course"`   → 1.0 (a proctored course assessment)
///   - `"tutorial"` → 0.6 (a drive-by end-of-video check)
///
/// The intent is that 20 tutorial drive-by quizzes should not sum to
/// the same evidence as a full course assessment. This is the sole
/// point in the evidence pipeline where `kind` has runtime effect —
/// everything downstream (aggregation, proof confidence, reputation)
/// is driven by the assessment's trust_factor column.
fn find_or_create_assessment(
    conn: &Connection,
    course_id: &str,
    element_id: &str,
    skill_id: &str,
    course_kind: &str,
) -> Result<String, String> {
    // Try to find existing
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM skill_assessments \
             WHERE course_id = ?1 AND source_element_id = ?2 AND skill_id = ?3",
            params![course_id, element_id, skill_id],
            |row| row.get(0),
        )
        .ok();

    if let Some(id) = existing {
        return Ok(id);
    }

    let trust_factor: f64 = match course_kind {
        "tutorial" => 0.6,
        _ => 1.0,
    };

    // Auto-create with defaults
    let id = entity_id(&[course_id, element_id, skill_id]);
    conn.execute(
        "INSERT INTO skill_assessments \
         (id, skill_id, course_id, source_element_id, assessment_type, \
          proficiency_level, difficulty, weight, trust_factor) \
         VALUES (?1, ?2, ?3, ?4, 'quiz', 'apply', 0.50, 1.0, ?5)",
        params![id, skill_id, course_id, element_id, trust_factor],
    )
    .map_err(|e| e.to_string())?;

    Ok(id)
}

/// Evaluate all evidence for a skill and update the skill proof.
///
/// This is the core aggregation algorithm:
///   1. Query all evidence records for the skill
///   2. Compute weighted confidence
///   3. Determine highest achieved proficiency level
///   4. Create or update the skill_proofs row
///
/// Returns the aggregation result (for reputation callback).
pub fn evaluate_and_update(
    conn: &Connection,
    stake_address: &str,
    skill_id: &str,
) -> Result<AggregationResult, String> {
    // Load all evidence for this skill, joined with assessment data
    let mut stmt = conn
        .prepare(
            "SELECT er.score, sa.weight, er.difficulty, er.trust_factor, sa.assessment_type \
             FROM evidence_records er \
             JOIN skill_assessments sa ON sa.id = er.skill_assessment_id \
             WHERE er.skill_id = ?1 \
             ORDER BY er.created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let evidence: Vec<EvidenceItem> = {
        let rows = stmt
            .query_map(params![skill_id], |row| {
                Ok(EvidenceItem {
                    score: row.get(0)?,
                    weight: row.get(1)?,
                    difficulty: row.get(2)?,
                    trust_factor: row.get(3)?,
                    assessment_type: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    if evidence.is_empty() {
        return Ok(AggregationResult {
            achieved_level: None,
            confidence: 0.0,
            evidence_count: 0,
            proof_id: None,
            old_confidence: 0.0,
        });
    }

    // Compute weighted confidence
    let mut weighted_sum = 0.0;
    let mut weight_denominator = 0.0;
    let mut has_project = false;

    for e in &evidence {
        let w = e.weight * e.difficulty * e.trust_factor;
        weighted_sum += e.score * w;
        weight_denominator += w;

        if e.assessment_type == "project" {
            has_project = true;
        }
    }

    let confidence = if weight_denominator > 0.0 {
        weighted_sum / weight_denominator
    } else {
        0.0
    };

    let evidence_count = evidence.len();

    // Determine highest achieved level
    let mut achieved_level: Option<ProficiencyLevel> = None;

    for threshold in THRESHOLDS {
        if evidence_count < threshold.min_evidence {
            break;
        }
        if confidence < threshold.min_confidence {
            break;
        }
        if let Some(required_type) = threshold.requires_type {
            if required_type == "project" && !has_project {
                break;
            }
        }
        achieved_level = Some(threshold.level);
    }

    // If no level achieved, nothing to create/update
    let level = match achieved_level {
        Some(l) => l,
        None => {
            return Ok(AggregationResult {
                achieved_level: None,
                confidence,
                evidence_count,
                proof_id: None,
                old_confidence: 0.0,
            });
        }
    };

    // Check for existing proof at this level
    let proof_id = entity_id(&[stake_address, skill_id, level.as_str()]);

    let old_confidence: f64 = conn
        .query_row(
            "SELECT confidence FROM skill_proofs WHERE id = ?1",
            params![proof_id],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    // Only update if confidence improved (monotonically increasing)
    if confidence > old_confidence {
        conn.execute(
            "INSERT INTO skill_proofs (id, skill_id, proficiency_level, confidence, evidence_count, computed_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
                confidence = ?4, evidence_count = ?5, updated_at = datetime('now')",
            params![
                proof_id,
                skill_id,
                level.as_str(),
                confidence,
                evidence_count as i64,
            ],
        )
        .map_err(|e| e.to_string())?;

        // Link evidence to proof (idempotent)
        let evidence_ids: Vec<String> = conn
            .prepare("SELECT id FROM evidence_records WHERE skill_id = ?1")
            .map_err(|e| e.to_string())?
            .query_map(params![skill_id], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        for eid in &evidence_ids {
            conn.execute(
                "INSERT OR IGNORE INTO skill_proof_evidence (proof_id, evidence_id) VALUES (?1, ?2)",
                params![proof_id, eid],
            )
            .map_err(|e| e.to_string())?;
        }

        log::info!(
            "proof updated: skill={}, level={}, confidence={:.3} (was {:.3}), evidence={}",
            skill_id,
            level.as_str(),
            confidence,
            old_confidence,
            evidence_count
        );
    }

    Ok(AggregationResult {
        achieved_level: Some(level),
        confidence,
        evidence_count,
        proof_id: Some(proof_id),
        old_confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn setup_db() -> Database {
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrations");

        // Create a skill
        db.conn()
            .execute(
                "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'CS')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub1', 'Algorithms', 'sf1')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO skills (id, name, subject_id) VALUES ('sk1', 'Graph Traversal', 'sub1')",
                [],
            )
            .unwrap();

        // Create local identity
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1uq123', 'addr_test1q123')",
                [],
            )
            .unwrap();

        // Create a course with an author
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address) \
                 VALUES ('c1', 'Algo Course', 'stake_test1uinstructor')",
                [],
            )
            .unwrap();

        // Create a chapter and element
        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) \
                 VALUES ('ch1', 'c1', 'Chapter 1', 0)",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
                 VALUES ('el1', 'ch1', 'Quiz 1', 'quiz', 0)",
                [],
            )
            .unwrap();

        // Tag the element with a skill
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
    fn create_evidence_and_evaluate() {
        let db = setup_db();
        let conn = db.conn();

        // Complete element with 80% score
        let skills =
            create_evidence_for_element(conn, "c1", "el1", 0.80, "stake_test1uq123", None, None)
                .unwrap();
        assert_eq!(skills, vec!["sk1"]);

        // Evaluate — 1 evidence at 0.80 should meet "remember" (min_conf 0.60)
        let result = evaluate_and_update(conn, "stake_test1uq123", "sk1").unwrap();
        assert_eq!(result.achieved_level, Some(ProficiencyLevel::Remember));
        assert!(result.confidence > 0.79);
        assert_eq!(result.evidence_count, 1);
        assert!(result.proof_id.is_some());
    }

    #[test]
    fn two_evidence_records_unlock_understand() {
        let db = setup_db();
        let conn = db.conn();

        // Add a second element
        conn.execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
             VALUES ('el2', 'ch1', 'Quiz 2', 'quiz', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO element_skill_tags (element_id, skill_id, weight) \
             VALUES ('el2', 'sk1', 1.0)",
            [],
        )
        .unwrap();

        // Complete both elements with 70% score
        create_evidence_for_element(conn, "c1", "el1", 0.70, "stake_test1uq123", None, None)
            .unwrap();
        create_evidence_for_element(conn, "c1", "el2", 0.70, "stake_test1uq123", None, None)
            .unwrap();

        let result = evaluate_and_update(conn, "stake_test1uq123", "sk1").unwrap();
        // 2 evidence, confidence ~0.70 meets "apply" (min 2 evidence, min 0.70 conf)
        assert_eq!(result.achieved_level, Some(ProficiencyLevel::Apply));
        assert_eq!(result.evidence_count, 2);
    }

    #[test]
    fn low_score_limits_level() {
        let db = setup_db();
        let conn = db.conn();

        // Single evidence at 50% — below "remember" threshold (0.60)
        create_evidence_for_element(conn, "c1", "el1", 0.50, "stake_test1uq123", None, None)
            .unwrap();

        let result = evaluate_and_update(conn, "stake_test1uq123", "sk1").unwrap();
        assert_eq!(result.achieved_level, None);
        assert!(result.confidence < 0.60);
    }

    #[test]
    fn confidence_is_monotonically_increasing() {
        let db = setup_db();
        let conn = db.conn();

        // First: high score
        create_evidence_for_element(conn, "c1", "el1", 0.90, "stake_test1uq123", None, None)
            .unwrap();
        let r1 = evaluate_and_update(conn, "stake_test1uq123", "sk1").unwrap();

        // Add a low score element — should not decrease proof confidence
        conn.execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
             VALUES ('el2', 'ch1', 'Quiz 2', 'quiz', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO element_skill_tags (element_id, skill_id, weight) \
             VALUES ('el2', 'sk1', 1.0)",
            [],
        )
        .unwrap();
        create_evidence_for_element(conn, "c1", "el2", 0.50, "stake_test1uq123", None, None)
            .unwrap();
        let r2 = evaluate_and_update(conn, "stake_test1uq123", "sk1").unwrap();

        // The proof still stores the old (higher) confidence
        let stored: f64 = conn
            .query_row(
                "SELECT confidence FROM skill_proofs WHERE id = ?1",
                params![r1.proof_id.as_ref().unwrap()],
                |row| row.get(0),
            )
            .unwrap();

        assert!(
            stored >= r1.confidence,
            "proof confidence should not decrease"
        );
        assert!(
            r2.confidence < r1.confidence,
            "new weighted avg should be lower"
        );
    }

    #[test]
    fn no_skill_tags_returns_empty() {
        let db = setup_db();
        let conn = db.conn();

        // Element with no skill tags
        conn.execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
             VALUES ('el_untagged', 'ch1', 'Untagged', 'text', 2)",
            [],
        )
        .unwrap();

        let skills = create_evidence_for_element(
            conn,
            "c1",
            "el_untagged",
            0.80,
            "stake_test1uq123",
            None,
            None,
        )
        .unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn auto_creates_assessment() {
        let db = setup_db();
        let conn = db.conn();

        // No assessments exist initially
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM skill_assessments", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);

        // Creating evidence auto-creates assessment
        create_evidence_for_element(conn, "c1", "el1", 0.80, "stake_test1uq123", None, None)
            .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM skill_assessments", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);

        // Second call reuses the same assessment
        conn.execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
             VALUES ('el2', 'ch1', 'Quiz 2', 'quiz', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO element_skill_tags (element_id, skill_id, weight) \
             VALUES ('el2', 'sk1', 1.0)",
            [],
        )
        .unwrap();
        create_evidence_for_element(conn, "c1", "el2", 0.70, "stake_test1uq123", None, None)
            .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM skill_assessments", [], |row| {
                row.get(0)
            })
            .unwrap();
        // 2 assessments: one per (course, element, skill) pair
        assert_eq!(count, 2);
    }
}
