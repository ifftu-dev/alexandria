//! Evidence broadcast — gossip-based evidence sharing for reputation computation.
//!
//! Handles both sides of the evidence gossip protocol:
//!
//! **Incoming** (receive side): When a validated `GossipMessage` arrives on
//! `/alexandria/evidence/1.0`, deserialize the `EvidenceAnnouncement` payload,
//! store in the local `evidence_records` table (for reputation computation),
//! and record in `sync_log`.
//!
//! **Outgoing** (publish side): When a learner completes an element and evidence
//! is created locally, construct an `EvidenceAnnouncement` and broadcast via
//! GossipSub. Other nodes store this for instructor reputation computation.
//!
//! **Important**: Received evidence does NOT trigger local aggregation — only
//! the learner's own node aggregates proofs. Peers store evidence solely for
//! reputation inputs (instructor impact computation, verification).

use rusqlite::params;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::Database;
use crate::domain::evidence::EvidenceAnnouncement;
use crate::p2p::types::SignedGossipMessage;

/// Handle an incoming evidence announcement from the P2P network.
///
/// Deserializes the gossip message payload as an `EvidenceAnnouncement`,
/// validates required fields, and stores in the local `evidence_records`
/// table. Uses `INSERT OR IGNORE` for dedup (evidence IDs are deterministic).
///
/// If the referenced skill doesn't exist locally (taxonomy not yet synced),
/// the evidence is recorded in `sync_log` only and skipped for now.
///
/// **Does NOT trigger aggregation** — only the learner's own node
/// evaluates and updates proofs.
pub fn handle_evidence_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<EvidenceAnnouncement, String> {
    // Deserialize the inner payload
    let announcement: EvidenceAnnouncement = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("invalid evidence announcement: {e}"))?;

    // Validate required fields
    if announcement.evidence_id.is_empty() {
        return Err("evidence announcement missing evidence_id".into());
    }
    if announcement.learner_address.is_empty() {
        return Err("evidence announcement missing learner_address".into());
    }
    if announcement.skill_id.is_empty() {
        return Err("evidence announcement missing skill_id".into());
    }
    if announcement.assessment_id.is_empty() {
        return Err("evidence announcement missing assessment_id".into());
    }
    if !(0.0..=1.0).contains(&announcement.score) {
        return Err(format!(
            "evidence announcement score out of range: {}",
            announcement.score
        ));
    }

    let signature_hex = hex::encode(&message.signature);

    // Check if the skill exists locally (FK constraint enforced)
    let skill_exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM skills WHERE id = ?1",
            params![announcement.skill_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !skill_exists {
        // Skill not in local taxonomy yet — record in sync_log for future processing
        db.conn()
            .execute(
                "INSERT INTO sync_log (entity_type, entity_id, direction, peer_id, signature) \
                 VALUES ('evidence', ?1, 'received', ?2, ?3)",
                params![
                    announcement.evidence_id,
                    message.stake_address,
                    signature_hex
                ],
            )
            .map_err(|e| format!("failed to record sync_log: {e}"))?;

        log::debug!(
            "Evidence: skill '{}' not in local taxonomy — recorded in sync_log for later",
            announcement.skill_id,
        );

        return Ok(announcement);
    }

    // Ensure a skill_assessment stub exists for this assessment_id.
    // Received evidence references assessments from other nodes — we create
    // stubs so the FK constraint is satisfied.
    let assessment_exists: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM skill_assessments WHERE id = ?1",
            params![announcement.assessment_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !assessment_exists {
        // Only reference the course if it exists locally (FK constraint)
        let local_course_id: Option<String> = announcement.course_id.as_ref().and_then(|cid| {
            db.conn()
                .query_row(
                    "SELECT id FROM courses WHERE id = ?1",
                    params![cid],
                    |row| row.get(0),
                )
                .ok()
        });

        db.conn()
            .execute(
                "INSERT OR IGNORE INTO skill_assessments \
                 (id, skill_id, course_id, assessment_type, proficiency_level, \
                  difficulty, trust_factor, weight) \
                 VALUES (?1, ?2, ?3, 'quiz', ?4, ?5, ?6, 1.0)",
                params![
                    announcement.assessment_id,
                    announcement.skill_id,
                    local_course_id,
                    announcement.proficiency_level,
                    announcement.difficulty,
                    announcement.trust_factor,
                ],
            )
            .map_err(|e| format!("failed to create assessment stub: {e}"))?;
    }

    // Convert timestamp to datetime string
    let created_at = chrono::DateTime::from_timestamp(announcement.created_at, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    // For the evidence record, also only reference course if it exists locally
    let evidence_course_id: Option<String> = announcement.course_id.as_ref().and_then(|cid| {
        db.conn()
            .query_row(
                "SELECT id FROM courses WHERE id = ?1",
                params![cid],
                |row| row.get(0),
            )
            .ok()
    });

    // Insert evidence record (INSERT OR IGNORE for idempotent dedup)
    db.conn()
        .execute(
            "INSERT OR IGNORE INTO evidence_records \
             (id, skill_assessment_id, skill_id, proficiency_level, score, \
              difficulty, trust_factor, course_id, instructor_address, \
              signature, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                announcement.evidence_id,
                announcement.assessment_id,
                announcement.skill_id,
                announcement.proficiency_level,
                announcement.score,
                announcement.difficulty,
                announcement.trust_factor,
                evidence_course_id,
                announcement.instructor_address,
                signature_hex,
                created_at,
            ],
        )
        .map_err(|e| format!("failed to insert evidence record: {e}"))?;

    // Record in sync_log
    db.conn()
        .execute(
            "INSERT INTO sync_log (entity_type, entity_id, direction, peer_id, signature) \
             VALUES ('evidence', ?1, 'received', ?2, ?3)",
            params![
                announcement.evidence_id,
                message.stake_address,
                signature_hex
            ],
        )
        .map_err(|e| format!("failed to record sync_log: {e}"))?;

    log::info!(
        "Evidence: received record '{}' for skill '{}' from {}",
        announcement.evidence_id,
        announcement.skill_id,
        announcement.learner_address,
    );

    Ok(announcement)
}

/// Build an `EvidenceAnnouncement` from locally created evidence data.
///
/// Called after `create_evidence_for_element` stores evidence locally.
/// The announcement is a lightweight summary for gossip broadcast.
#[allow(clippy::too_many_arguments)]
pub fn build_evidence_announcement(
    evidence_id: &str,
    learner_address: &str,
    skill_id: &str,
    proficiency_level: &str,
    assessment_id: &str,
    score: f64,
    difficulty: f64,
    trust_factor: f64,
    course_id: Option<&str>,
    instructor_address: Option<&str>,
) -> EvidenceAnnouncement {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    EvidenceAnnouncement {
        evidence_id: evidence_id.to_string(),
        learner_address: learner_address.to_string(),
        skill_id: skill_id.to_string(),
        proficiency_level: proficiency_level.to_string(),
        assessment_id: assessment_id.to_string(),
        score,
        difficulty,
        trust_factor,
        course_id: course_id.map(String::from),
        instructor_address: instructor_address.map(String::from),
        created_at,
    }
}

/// Collect recently created evidence IDs from the local database
/// for a given skill, to build announcements for broadcast.
///
/// Returns `(evidence_id, assessment_id, proficiency_level, score,
/// difficulty, trust_factor, course_id, instructor_address)` for each record.
pub fn collect_evidence_for_broadcast(
    db: &Database,
    skill_id: &str,
) -> Result<Vec<LocalEvidenceRow>, String> {
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT er.id, er.skill_assessment_id, er.proficiency_level, \
             er.score, er.difficulty, er.trust_factor, er.course_id, \
             er.instructor_address \
             FROM evidence_records er \
             WHERE er.skill_id = ?1 \
             AND er.id NOT IN (SELECT entity_id FROM sync_log WHERE entity_type = 'evidence' AND direction = 'sent') \
             ORDER BY er.created_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![skill_id], |row| {
            Ok(LocalEvidenceRow {
                evidence_id: row.get(0)?,
                assessment_id: row.get(1)?,
                proficiency_level: row.get(2)?,
                score: row.get(3)?,
                difficulty: row.get(4)?,
                trust_factor: row.get(5)?,
                course_id: row.get(6)?,
                instructor_address: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(rows)
}

/// A row of local evidence data for broadcast.
#[derive(Debug, Clone)]
pub struct LocalEvidenceRow {
    pub evidence_id: String,
    pub assessment_id: String,
    pub proficiency_level: String,
    pub score: f64,
    pub difficulty: f64,
    pub trust_factor: f64,
    pub course_id: Option<String>,
    pub instructor_address: Option<String>,
}

/// Record that evidence was broadcast (mark in sync_log as sent).
pub fn mark_evidence_broadcast(
    db: &Database,
    evidence_id: &str,
    signature_hex: &str,
) -> Result<(), String> {
    db.conn()
        .execute(
            "INSERT INTO sync_log (entity_type, entity_id, direction, signature) \
             VALUES ('evidence', ?1, 'sent', ?2)",
            params![evidence_id, signature_hex],
        )
        .map_err(|e| format!("failed to record sync_log: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    /// Insert a stub skill for FK constraints in tests.
    /// Must insert subject_fields → subjects → skills (FK order).
    fn insert_test_skill(db: &Database, skill_id: &str) {
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO subject_fields (id, name) VALUES ('test_field', 'Test Field')",
                params![],
            )
            .expect("insert field");
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO subjects (id, name, subject_field_id) VALUES ('test_subject', 'Test Subject', 'test_field')",
                params![],
            )
            .expect("insert subject");
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO skills (id, name, subject_id) VALUES (?1, ?2, 'test_subject')",
                params![skill_id, skill_id],
            )
            .expect("insert skill");
    }

    fn sample_announcement() -> EvidenceAnnouncement {
        build_evidence_announcement(
            "ev_test_001",
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
            "skill_graph_traversal",
            "apply",
            "assessment_001",
            0.85,
            0.60,
            1.0,
            Some("course_001"),
            Some("stake_test1instructor"),
        )
    }

    fn sample_message(announcement: &EvidenceAnnouncement) -> SignedGossipMessage {
        let payload = serde_json::to_vec(announcement).unwrap();
        SignedGossipMessage {
            topic: "/alexandria/evidence/1.0".into(),
            payload,
            signature: vec![0xDE, 0xAD, 0xBE, 0xEF],
            public_key: vec![0; 32],
            stake_address: announcement.learner_address.clone(),
            timestamp: 1_700_000_000,
            encrypted: false,
            key_id: None,
        }
    }

    #[test]
    fn build_announcement_sets_fields() {
        let ann = sample_announcement();
        assert_eq!(ann.evidence_id, "ev_test_001");
        assert_eq!(ann.skill_id, "skill_graph_traversal");
        assert_eq!(ann.proficiency_level, "apply");
        assert!((ann.score - 0.85).abs() < f64::EPSILON);
        assert!(ann.created_at > 0);
    }

    #[test]
    fn handle_evidence_inserts_when_skill_exists() {
        let db = test_db();
        insert_test_skill(&db, "skill_graph_traversal");

        let ann = sample_announcement();
        let msg = sample_message(&ann);

        let result = handle_evidence_message(&db, &msg);
        assert!(result.is_ok());

        // Verify evidence_records row
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM evidence_records WHERE id = ?1",
                params![ann.evidence_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify sync_log
        let direction: String = db
            .conn()
            .query_row(
                "SELECT direction FROM sync_log WHERE entity_id = ?1 AND entity_type = 'evidence'",
                params![ann.evidence_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(direction, "received");
    }

    #[test]
    fn handle_evidence_skips_when_skill_missing() {
        let db = test_db();
        // Don't insert the skill — it should be gracefully skipped

        let ann = sample_announcement();
        let msg = sample_message(&ann);

        let result = handle_evidence_message(&db, &msg);
        assert!(result.is_ok());

        // Evidence record should NOT be in evidence_records (FK would fail)
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM evidence_records WHERE id = ?1",
                params![ann.evidence_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);

        // But sync_log should still have it recorded
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_log WHERE entity_id = ?1 AND entity_type = 'evidence'",
                params![ann.evidence_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn handle_evidence_is_idempotent() {
        let db = test_db();
        insert_test_skill(&db, "skill_graph_traversal");

        let ann = sample_announcement();
        let msg = sample_message(&ann);

        // Insert twice
        handle_evidence_message(&db, &msg).unwrap();
        handle_evidence_message(&db, &msg).unwrap();

        // Should still only have one evidence_records row (INSERT OR IGNORE)
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM evidence_records WHERE id = ?1",
                params![ann.evidence_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn handle_evidence_creates_assessment_stub() {
        let db = test_db();
        insert_test_skill(&db, "skill_graph_traversal");

        let ann = sample_announcement();
        let msg = sample_message(&ann);

        handle_evidence_message(&db, &msg).unwrap();

        // Verify assessment stub was created
        let assessment_type: String = db
            .conn()
            .query_row(
                "SELECT assessment_type FROM skill_assessments WHERE id = ?1",
                params![ann.assessment_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assessment_type, "quiz");
    }

    #[test]
    fn handle_evidence_rejects_empty_evidence_id() {
        let db = test_db();
        let mut ann = sample_announcement();
        ann.evidence_id = String::new();
        let msg = sample_message(&ann);

        assert!(handle_evidence_message(&db, &msg).is_err());
    }

    #[test]
    fn handle_evidence_rejects_invalid_score() {
        let db = test_db();
        let mut ann = sample_announcement();
        ann.score = 1.5; // out of range
        let msg = sample_message(&ann);

        assert!(handle_evidence_message(&db, &msg).is_err());
    }

    #[test]
    fn mark_evidence_broadcast_records_sync_log() {
        let db = test_db();

        mark_evidence_broadcast(&db, "ev_001", "deadbeef").unwrap();

        let direction: String = db
            .conn()
            .query_row(
                "SELECT direction FROM sync_log WHERE entity_id = 'ev_001' AND entity_type = 'evidence'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(direction, "sent");
    }

    #[test]
    fn collect_evidence_excludes_already_sent() {
        let db = test_db();
        insert_test_skill(&db, "skill_graph_traversal");

        // Insert an assessment stub
        db.conn()
            .execute(
                "INSERT INTO skill_assessments (id, skill_id, assessment_type, proficiency_level) \
                 VALUES ('sa_001', 'skill_graph_traversal', 'quiz', 'apply')",
                [],
            )
            .unwrap();

        // Insert two evidence records
        db.conn()
            .execute(
                "INSERT INTO evidence_records (id, skill_assessment_id, skill_id, proficiency_level, score, difficulty, trust_factor) \
                 VALUES ('ev_001', 'sa_001', 'skill_graph_traversal', 'apply', 0.85, 0.50, 1.0)",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO evidence_records (id, skill_assessment_id, skill_id, proficiency_level, score, difficulty, trust_factor) \
                 VALUES ('ev_002', 'sa_001', 'skill_graph_traversal', 'apply', 0.90, 0.50, 1.0)",
                [],
            )
            .unwrap();

        // Mark ev_001 as already sent
        mark_evidence_broadcast(&db, "ev_001", "sig001").unwrap();

        // Collect should only return ev_002
        let rows = collect_evidence_for_broadcast(&db, "skill_graph_traversal").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].evidence_id, "ev_002");
    }
}
