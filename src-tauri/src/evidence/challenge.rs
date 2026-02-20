//! Evidence challenge lifecycle logic.
//!
//! Core operations for the challenge mechanism:
//!   - `submit_challenge` — validate params, create record with expiry
//!   - `vote_on_challenge` — committee member votes, transitions to reviewing
//!   - `resolve_challenge` — tally votes, apply outcome (upheld or rejected)
//!   - `invalidate_evidence` — delete evidence and re-aggregate proofs
//!   - `zero_skill_reputation` — zero reputation for learner+skill
//!   - `check_expired_challenges` — expire challenges past deadline
//!   - `get_challenge` / `list_challenges` — query operations

use rusqlite::{params, Connection};

use crate::crypto::hash::entity_id;
use crate::domain::challenge::{
    ChallengeResolution, ChallengeStatus, ChallengeTargetType, ChallengeVote, EvidenceChallenge,
    SubmitChallengeParams, CHALLENGE_DEADLINE_DAYS, MIN_STAKE_LOVELACE,
};

/// Supermajority threshold for challenge resolution (2/3).
const SUPERMAJORITY_THRESHOLD: f64 = 2.0 / 3.0;

/// Submit a new evidence challenge.
///
/// Validates parameters, checks minimum stake, creates the challenge
/// record with an expiry date (now + 30 days), and signs it using the
/// challenger's local identity.
pub fn submit_challenge(
    conn: &Connection,
    params: &SubmitChallengeParams,
) -> Result<EvidenceChallenge, String> {
    // Validate target type
    ChallengeTargetType::from_str(&params.target_type)
        .ok_or_else(|| format!("invalid target_type: {}", params.target_type))?;

    // Validate minimum stake
    if params.stake_lovelace < MIN_STAKE_LOVELACE {
        return Err(format!(
            "stake must be at least {} lovelace (5 ADA), got {}",
            MIN_STAKE_LOVELACE, params.stake_lovelace
        ));
    }

    // Validate non-empty fields
    if params.target_ids.is_empty() {
        return Err("target_ids must not be empty".into());
    }
    if params.reason.is_empty() {
        return Err("reason must not be empty".into());
    }

    // Verify DAO exists and is active
    let dao_status: String = conn
        .query_row(
            "SELECT status FROM governance_daos WHERE id = ?1",
            params![params.dao_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("DAO not found: {e}"))?;

    if dao_status != "active" {
        return Err(format!("DAO is not active (status: {dao_status})"));
    }

    // Get local identity as challenger
    let challenger: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no local identity: {e}"))?;

    let now = chrono::Utc::now();
    let created_at = now.to_rfc3339();
    let expires_at = (now + chrono::Duration::days(CHALLENGE_DEADLINE_DAYS)).to_rfc3339();

    let target_ids_json = serde_json::to_string(&params.target_ids).map_err(|e| e.to_string())?;
    let evidence_cids_json =
        serde_json::to_string(&params.evidence_cids).map_err(|e| e.to_string())?;

    // Deterministic ID from challenger + target + reason
    let id = entity_id(&[
        &challenger,
        &params.target_type,
        &target_ids_json,
        &params.reason,
    ]);

    // Sign the challenge (placeholder — in production, sign with Ed25519 key)
    let signature = entity_id(&[&id, &challenger, &created_at]);

    conn.execute(
        "INSERT INTO evidence_challenges \
         (id, challenger, target_type, target_ids, evidence_cids, reason, \
          stake_lovelace, status, dao_id, learner_address, reviewed_by, \
          signature, created_at, expires_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9, '[]', ?10, ?11, ?12)",
        params![
            id,
            challenger,
            params.target_type,
            target_ids_json,
            evidence_cids_json,
            params.reason,
            params.stake_lovelace as i64,
            params.dao_id,
            params.learner_address,
            signature,
            created_at,
            expires_at,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(EvidenceChallenge {
        id,
        challenger,
        target_type: params.target_type.clone(),
        target_ids: params.target_ids.clone(),
        evidence_cids: params.evidence_cids.clone(),
        reason: params.reason.clone(),
        stake_lovelace: params.stake_lovelace,
        stake_tx_hash: None,
        status: "pending".into(),
        dao_id: params.dao_id.clone(),
        learner_address: params.learner_address.clone(),
        reviewed_by: vec![],
        resolution_tx: None,
        signature,
        created_at,
        resolved_at: None,
        expires_at: Some(expires_at),
    })
}

/// Vote on a challenge as a DAO committee member.
///
/// Verifies the voter is a committee or chair member of the challenge's
/// DAO. Records the vote and transitions status to "reviewing" on the
/// first vote.
pub fn vote_on_challenge(
    conn: &Connection,
    challenge_id: &str,
    voter: &str,
    upheld: bool,
    reason: Option<&str>,
) -> Result<ChallengeVote, String> {
    // Load challenge
    let (status, dao_id): (String, String) = conn
        .query_row(
            "SELECT status, dao_id FROM evidence_challenges WHERE id = ?1",
            params![challenge_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("challenge not found: {e}"))?;

    // Must be pending or reviewing
    if status != "pending" && status != "reviewing" {
        return Err(format!(
            "challenge is not open for voting (status: {status})"
        ));
    }

    // Verify voter is a committee/chair member of the DAO
    let is_committee: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM governance_dao_members \
             WHERE dao_id = ?1 AND stake_address = ?2 AND role IN ('committee', 'chair')",
            params![dao_id, voter],
            |row| Ok(row.get::<_, i64>(0)? > 0),
        )
        .map_err(|e| e.to_string())?;

    if !is_committee {
        return Err("voter is not a committee member of this DAO".into());
    }

    // Check duplicate vote
    let already_voted: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM challenge_votes \
             WHERE challenge_id = ?1 AND voter = ?2",
            params![challenge_id, voter],
            |row| Ok(row.get::<_, i64>(0)? > 0),
        )
        .map_err(|e| e.to_string())?;

    if already_voted {
        return Err("already voted on this challenge".into());
    }

    let vote_id = entity_id(&[challenge_id, voter]);
    let upheld_int: i64 = if upheld { 1 } else { 0 };

    conn.execute(
        "INSERT INTO challenge_votes \
         (id, challenge_id, voter, upheld, reason) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![vote_id, challenge_id, voter, upheld_int, reason],
    )
    .map_err(|e| e.to_string())?;

    // Transition to reviewing on first vote
    if status == "pending" {
        conn.execute(
            "UPDATE evidence_challenges SET status = 'reviewing' WHERE id = ?1",
            params![challenge_id],
        )
        .map_err(|e| e.to_string())?;
    }

    // Update reviewed_by list
    let reviewed_json: String = conn
        .query_row(
            "SELECT reviewed_by FROM evidence_challenges WHERE id = ?1",
            params![challenge_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let mut reviewed: Vec<String> = serde_json::from_str(&reviewed_json).unwrap_or_default();
    if !reviewed.contains(&voter.to_string()) {
        reviewed.push(voter.to_string());
    }
    let updated_json = serde_json::to_string(&reviewed).map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE evidence_challenges SET reviewed_by = ?1 WHERE id = ?2",
        params![updated_json, challenge_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(ChallengeVote {
        id: vote_id,
        challenge_id: challenge_id.to_string(),
        voter: voter.to_string(),
        upheld,
        reason: reason.map(|s| s.to_string()),
        voted_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Resolve a challenge by tallying votes.
///
/// Requires supermajority (2/3) to uphold. If upheld: invalidates
/// evidence, zeros reputation for the skill. If rejected: marks as
/// rejected (challenger's stake is slashed).
pub fn resolve_challenge(
    conn: &Connection,
    challenge_id: &str,
) -> Result<ChallengeResolution, String> {
    // Load challenge
    let (status, target_type, target_ids_json, learner_address, dao_id): (
        String,
        String,
        String,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT status, target_type, target_ids, learner_address, dao_id \
             FROM evidence_challenges WHERE id = ?1",
            params![challenge_id],
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
        .map_err(|e| format!("challenge not found: {e}"))?;

    if status != "reviewing" && status != "pending" {
        return Err(format!(
            "challenge must be pending or reviewing to resolve (status: {status})"
        ));
    }

    // Tally votes
    let votes_upheld: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM challenge_votes \
             WHERE challenge_id = ?1 AND upheld = 1",
            params![challenge_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let votes_rejected: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM challenge_votes \
             WHERE challenge_id = ?1 AND upheld = 0",
            params![challenge_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let total_votes = votes_upheld + votes_rejected;
    if total_votes == 0 {
        return Err("cannot resolve challenge with no votes".into());
    }

    let upheld_ratio = votes_upheld as f64 / total_votes as f64;
    let is_upheld = upheld_ratio >= SUPERMAJORITY_THRESHOLD;

    let new_status = if is_upheld {
        ChallengeStatus::Upheld
    } else {
        ChallengeStatus::Rejected
    };

    conn.execute(
        "UPDATE evidence_challenges SET status = ?1, resolved_at = datetime('now') WHERE id = ?2",
        params![new_status.as_str(), challenge_id],
    )
    .map_err(|e| e.to_string())?;

    let mut proofs_invalidated: i64 = 0;
    let mut reputation_zeroed = false;

    if is_upheld {
        let target_ids: Vec<String> = serde_json::from_str(&target_ids_json).unwrap_or_default();

        // Get skill_ids from the targeted evidence/proofs for reputation zeroing
        let skill_ids = get_skill_ids_for_targets(conn, &target_type, &target_ids)?;

        // Invalidate evidence
        proofs_invalidated = invalidate_evidence(conn, &target_type, &target_ids)?;

        // Zero reputation for each affected skill
        for skill_id in &skill_ids {
            zero_skill_reputation(conn, &learner_address, skill_id)?;
            reputation_zeroed = true;
        }
    }

    let _ = dao_id;

    Ok(ChallengeResolution {
        challenge_id: challenge_id.to_string(),
        status: new_status.as_str().to_string(),
        votes_upheld,
        votes_rejected,
        proofs_invalidated,
        reputation_zeroed,
    })
}

/// Get skill IDs associated with challenge targets.
fn get_skill_ids_for_targets(
    conn: &Connection,
    target_type: &str,
    target_ids: &[String],
) -> Result<Vec<String>, String> {
    let mut skill_ids = Vec::new();

    for tid in target_ids {
        match target_type {
            "evidence" => {
                if let Ok(sid) = conn.query_row(
                    "SELECT skill_id FROM evidence_records WHERE id = ?1",
                    params![tid],
                    |row| row.get::<_, String>(0),
                ) {
                    if !skill_ids.contains(&sid) {
                        skill_ids.push(sid);
                    }
                }
            }
            "skill_proof" => {
                if let Ok(sid) = conn.query_row(
                    "SELECT skill_id FROM skill_proofs WHERE id = ?1",
                    params![tid],
                    |row| row.get::<_, String>(0),
                ) {
                    if !skill_ids.contains(&sid) {
                        skill_ids.push(sid);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(skill_ids)
}

/// Invalidate evidence by deleting records and linked proof evidence.
///
/// For "evidence" targets: deletes evidence_records and skill_proof_evidence
/// links. For "skill_proof" targets: deletes the proof and its evidence links.
///
/// Returns the number of records deleted.
pub fn invalidate_evidence(
    conn: &Connection,
    target_type: &str,
    target_ids: &[String],
) -> Result<i64, String> {
    let mut deleted = 0_i64;

    for tid in target_ids {
        match target_type {
            "evidence" => {
                // Delete proof-evidence links first (FK)
                conn.execute(
                    "DELETE FROM skill_proof_evidence WHERE evidence_id = ?1",
                    params![tid],
                )
                .map_err(|e| e.to_string())?;

                // Delete the evidence record
                let affected = conn
                    .execute("DELETE FROM evidence_records WHERE id = ?1", params![tid])
                    .map_err(|e| e.to_string())?;
                deleted += affected as i64;
            }
            "skill_proof" => {
                // Delete proof-evidence links first (FK cascade would handle
                // this, but be explicit)
                conn.execute(
                    "DELETE FROM skill_proof_evidence WHERE proof_id = ?1",
                    params![tid],
                )
                .map_err(|e| e.to_string())?;

                // Delete the proof
                let affected = conn
                    .execute("DELETE FROM skill_proofs WHERE id = ?1", params![tid])
                    .map_err(|e| e.to_string())?;
                deleted += affected as i64;
            }
            _ => {}
        }
    }

    Ok(deleted)
}

/// Zero reputation assertions for a learner+skill combination.
///
/// Deletes all reputation assertions (both learner and instructor-side
/// impact deltas) scoped to the specific skill. Per design: reputation
/// zeroing is per-skill, not total wipe.
pub fn zero_skill_reputation(
    conn: &Connection,
    learner_address: &str,
    skill_id: &str,
) -> Result<(), String> {
    // Delete reputation impact deltas linked to this learner+skill
    conn.execute(
        "DELETE FROM reputation_impact_deltas \
         WHERE learner_address = ?1 AND assertion_id IN \
         (SELECT id FROM reputation_assertions WHERE skill_id = ?2)",
        params![learner_address, skill_id],
    )
    .map_err(|e| e.to_string())?;

    // Delete reputation evidence links
    conn.execute(
        "DELETE FROM reputation_evidence \
         WHERE assertion_id IN \
         (SELECT id FROM reputation_assertions \
          WHERE actor_address = ?1 AND skill_id = ?2)",
        params![learner_address, skill_id],
    )
    .map_err(|e| e.to_string())?;

    // Delete the reputation assertions themselves
    conn.execute(
        "DELETE FROM reputation_assertions \
         WHERE actor_address = ?1 AND skill_id = ?2",
        params![learner_address, skill_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Find and expire challenges that have passed their deadline.
///
/// Updates status to "expired" for any pending or reviewing challenge
/// whose `expires_at` is in the past.
pub fn check_expired_challenges(conn: &Connection) -> Result<i64, String> {
    let affected = conn
        .execute(
            "UPDATE evidence_challenges SET status = 'expired', resolved_at = datetime('now') \
             WHERE status IN ('pending', 'reviewing') \
             AND expires_at IS NOT NULL AND expires_at < datetime('now')",
            [],
        )
        .map_err(|e| e.to_string())?;

    Ok(affected as i64)
}

/// Get a single challenge with its votes.
pub fn get_challenge(
    conn: &Connection,
    challenge_id: &str,
) -> Result<(EvidenceChallenge, Vec<ChallengeVote>), String> {
    let challenge = query_challenge(conn, challenge_id)?;

    let mut stmt = conn
        .prepare(
            "SELECT id, challenge_id, voter, upheld, reason, voted_at \
             FROM challenge_votes WHERE challenge_id = ?1 ORDER BY voted_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let votes = stmt
        .query_map(params![challenge_id], |row| {
            Ok(ChallengeVote {
                id: row.get(0)?,
                challenge_id: row.get(1)?,
                voter: row.get(2)?,
                upheld: row.get(3)?,
                reason: row.get(4)?,
                voted_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok((challenge, votes))
}

/// List challenges with optional filters.
pub fn list_challenges(
    conn: &Connection,
    status: Option<&str>,
    dao_id: Option<&str>,
    learner_address: Option<&str>,
    challenger: Option<&str>,
) -> Result<Vec<EvidenceChallenge>, String> {
    let mut conditions: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(s) = status {
        conditions.push(format!("status = ?{idx}"));
        param_values.push(Box::new(s.to_string()));
        idx += 1;
    }
    if let Some(d) = dao_id {
        conditions.push(format!("dao_id = ?{idx}"));
        param_values.push(Box::new(d.to_string()));
        idx += 1;
    }
    if let Some(l) = learner_address {
        conditions.push(format!("learner_address = ?{idx}"));
        param_values.push(Box::new(l.to_string()));
        idx += 1;
    }
    if let Some(c) = challenger {
        conditions.push(format!("challenger = ?{idx}"));
        param_values.push(Box::new(c.to_string()));
        idx += 1;
    }
    let _ = idx;

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT id, challenger, target_type, target_ids, evidence_cids, reason, \
         stake_lovelace, stake_tx_hash, status, dao_id, learner_address, \
         reviewed_by, resolution_tx, signature, created_at, resolved_at, expires_at \
         FROM evidence_challenges {where_clause} ORDER BY created_at DESC"
    );

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let challenges = stmt
        .query_map(params_ref.as_slice(), |row| Ok(row_to_challenge(row)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(challenges)
}

/// Query a single challenge by ID.
fn query_challenge(conn: &Connection, challenge_id: &str) -> Result<EvidenceChallenge, String> {
    conn.query_row(
        "SELECT id, challenger, target_type, target_ids, evidence_cids, reason, \
         stake_lovelace, stake_tx_hash, status, dao_id, learner_address, \
         reviewed_by, resolution_tx, signature, created_at, resolved_at, expires_at \
         FROM evidence_challenges WHERE id = ?1",
        params![challenge_id],
        |row| Ok(row_to_challenge(row)),
    )
    .map_err(|e| format!("challenge not found: {e}"))
}

/// Map a database row to an `EvidenceChallenge`.
fn row_to_challenge(row: &rusqlite::Row) -> EvidenceChallenge {
    let target_ids_json: String = row.get(3).unwrap_or_default();
    let evidence_cids_json: String = row.get(4).unwrap_or_default();
    let reviewed_by_json: String = row.get(11).unwrap_or_default();

    EvidenceChallenge {
        id: row.get(0).unwrap_or_default(),
        challenger: row.get(1).unwrap_or_default(),
        target_type: row.get(2).unwrap_or_default(),
        target_ids: serde_json::from_str(&target_ids_json).unwrap_or_default(),
        evidence_cids: serde_json::from_str(&evidence_cids_json).unwrap_or_default(),
        reason: row.get(5).unwrap_or_default(),
        stake_lovelace: row.get::<_, i64>(6).unwrap_or_default() as u64,
        stake_tx_hash: row.get(7).unwrap_or_default(),
        status: row.get(8).unwrap_or_default(),
        dao_id: row.get(9).unwrap_or_default(),
        learner_address: row.get(10).unwrap_or_default(),
        reviewed_by: serde_json::from_str(&reviewed_by_json).unwrap_or_default(),
        resolution_tx: row.get(12).unwrap_or_default(),
        signature: row.get(13).unwrap_or_default(),
        created_at: row.get(14).unwrap_or_default(),
        resolved_at: row.get(15).unwrap_or_default(),
        expires_at: row.get(16).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    /// Set up a test database with required fixtures for challenge tests.
    fn setup_db() -> Database {
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrations");

        let conn = db.conn();

        // Local identity (the challenger)
        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1uchallenger', 'addr_test1q_challenger')",
            [],
        )
        .unwrap();

        // Subject field, subject, skill
        conn.execute(
            "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'CS')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub1', 'Algo', 'sf1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO skills (id, name, subject_id) VALUES ('sk1', 'Graphs', 'sub1')",
            [],
        )
        .unwrap();

        // Active DAO
        conn.execute(
            "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
             VALUES ('dao1', 'CS DAO', 'subject_field', 'sf1', 'active')",
            [],
        )
        .unwrap();

        // Committee members
        conn.execute(
            "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
             VALUES ('dao1', 'stake_test1ucommittee1', 'committee')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
             VALUES ('dao1', 'stake_test1ucommittee2', 'committee')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
             VALUES ('dao1', 'stake_test1uchair', 'chair')",
            [],
        )
        .unwrap();

        // Course and evidence for testing invalidation
        conn.execute(
            "INSERT INTO courses (id, title, author_address) \
             VALUES ('c1', 'Test Course', 'stake_test1uinstructor')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO course_chapters (id, course_id, title, position) \
             VALUES ('ch1', 'c1', 'Ch1', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
             VALUES ('el1', 'ch1', 'Quiz', 'quiz', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO element_skill_tags (element_id, skill_id, weight) \
             VALUES ('el1', 'sk1', 1.0)",
            [],
        )
        .unwrap();

        // Skill assessment
        conn.execute(
            "INSERT INTO skill_assessments \
             (id, skill_id, course_id, assessment_type, proficiency_level, difficulty, trust_factor) \
             VALUES ('sa1', 'sk1', 'c1', 'quiz', 'apply', 0.50, 1.0)",
            [],
        )
        .unwrap();

        // Evidence record for learner
        conn.execute(
            "INSERT INTO evidence_records \
             (id, skill_assessment_id, skill_id, proficiency_level, score, \
              difficulty, trust_factor, course_id, instructor_address) \
             VALUES ('ev1', 'sa1', 'sk1', 'apply', 0.80, 0.50, 1.0, 'c1', 'stake_test1uinstructor')",
            [],
        )
        .unwrap();

        // Skill proof
        conn.execute(
            "INSERT INTO skill_proofs \
             (id, skill_id, proficiency_level, confidence, evidence_count) \
             VALUES ('sp1', 'sk1', 'apply', 0.80, 1)",
            [],
        )
        .unwrap();

        // Link evidence to proof
        conn.execute(
            "INSERT INTO skill_proof_evidence (proof_id, evidence_id) VALUES ('sp1', 'ev1')",
            [],
        )
        .unwrap();

        // Reputation assertion for learner
        conn.execute(
            "INSERT INTO reputation_assertions \
             (id, actor_address, role, skill_id, proficiency_level, score, evidence_count) \
             VALUES ('ra1', 'stake_test1ulearner', 'learner', 'sk1', 'apply', 0.80, 1)",
            [],
        )
        .unwrap();

        db
    }

    fn default_params() -> SubmitChallengeParams {
        SubmitChallengeParams {
            target_type: "evidence".into(),
            target_ids: vec!["ev1".into()],
            evidence_cids: vec!["bafy123".into()],
            reason: "suspected plagiarism".into(),
            stake_lovelace: 5_000_000,
            dao_id: "dao1".into(),
            learner_address: "stake_test1ulearner".into(),
        }
    }

    // ---- Test 1: submit_challenge creates record ----
    #[test]
    fn submit_challenge_creates_record() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        let challenge = submit_challenge(conn, &params).unwrap();

        assert!(!challenge.id.is_empty());
        assert_eq!(challenge.status, "pending");
        assert_eq!(challenge.challenger, "stake_test1uchallenger");
        assert_eq!(challenge.target_type, "evidence");
        assert_eq!(challenge.target_ids, vec!["ev1"]);
        assert!(challenge.expires_at.is_some());

        // Verify in database
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM evidence_challenges WHERE id = ?1",
                params![challenge.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    // ---- Test 2: submit rejects below min stake ----
    #[test]
    fn submit_rejects_below_min_stake() {
        let db = setup_db();
        let conn = db.conn();
        let mut params = default_params();
        params.stake_lovelace = 1_000_000; // Below 5 ADA

        let result = submit_challenge(conn, &params);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("stake must be at least"));
    }

    // ---- Test 3: submit validates target type ----
    #[test]
    fn submit_validates_target_type() {
        let db = setup_db();
        let conn = db.conn();
        let mut params = default_params();
        params.target_type = "invalid_type".into();

        let result = submit_challenge(conn, &params);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid target_type"));
    }

    // ---- Test 4: vote_on_challenge records vote ----
    #[test]
    fn vote_on_challenge_records_vote() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        let challenge = submit_challenge(conn, &params).unwrap();
        let vote = vote_on_challenge(
            conn,
            &challenge.id,
            "stake_test1ucommittee1",
            true,
            Some("clear evidence of fraud"),
        )
        .unwrap();

        assert_eq!(vote.challenge_id, challenge.id);
        assert_eq!(vote.voter, "stake_test1ucommittee1");
        assert!(vote.upheld);
        assert_eq!(vote.reason, Some("clear evidence of fraud".into()));

        // Verify status transitioned to reviewing
        let status: String = conn
            .query_row(
                "SELECT status FROM evidence_challenges WHERE id = ?1",
                params![challenge.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "reviewing");
    }

    // ---- Test 5: vote rejects non-committee members ----
    #[test]
    fn vote_rejects_non_committee_members() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        let challenge = submit_challenge(conn, &params).unwrap();
        let result =
            vote_on_challenge(conn, &challenge.id, "stake_test1urandom_person", true, None);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a committee member"));
    }

    // ---- Test 6: vote rejects duplicate votes ----
    #[test]
    fn vote_rejects_duplicate_votes() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        let challenge = submit_challenge(conn, &params).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee1", true, None).unwrap();

        let result = vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee1", false, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already voted"));
    }

    // ---- Test 7: resolve upheld with supermajority ----
    #[test]
    fn resolve_upheld_with_supermajority() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        let challenge = submit_challenge(conn, &params).unwrap();

        // 3 votes to uphold (supermajority)
        vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee1", true, None).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee2", true, None).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1uchair", true, None).unwrap();

        let resolution = resolve_challenge(conn, &challenge.id).unwrap();
        assert_eq!(resolution.status, "upheld");
        assert_eq!(resolution.votes_upheld, 3);
        assert_eq!(resolution.votes_rejected, 0);
    }

    // ---- Test 8: resolve rejected when insufficient upheld votes ----
    #[test]
    fn resolve_rejected_when_insufficient_votes() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        let challenge = submit_challenge(conn, &params).unwrap();

        // 1 upheld, 2 rejected → 33% < 67% threshold
        vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee1", true, None).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee2", false, None).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1uchair", false, None).unwrap();

        let resolution = resolve_challenge(conn, &challenge.id).unwrap();
        assert_eq!(resolution.status, "rejected");
        assert_eq!(resolution.votes_upheld, 1);
        assert_eq!(resolution.votes_rejected, 2);
    }

    // ---- Test 9: upheld challenge deletes evidence ----
    #[test]
    fn upheld_challenge_deletes_evidence() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        // Verify evidence exists before
        let count_before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM evidence_records WHERE id = 'ev1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count_before, 1);

        let challenge = submit_challenge(conn, &params).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee1", true, None).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee2", true, None).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1uchair", true, None).unwrap();

        let resolution = resolve_challenge(conn, &challenge.id).unwrap();
        assert_eq!(resolution.status, "upheld");
        assert!(resolution.proofs_invalidated > 0);

        // Evidence should be deleted
        let count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM evidence_records WHERE id = 'ev1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count_after, 0);
    }

    // ---- Test 10: upheld challenge zeros reputation ----
    #[test]
    fn upheld_challenge_zeros_reputation() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        // Verify reputation exists before
        let count_before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reputation_assertions \
                 WHERE actor_address = 'stake_test1ulearner' AND skill_id = 'sk1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count_before, 1);

        let challenge = submit_challenge(conn, &params).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee1", true, None).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee2", true, None).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1uchair", true, None).unwrap();

        let resolution = resolve_challenge(conn, &challenge.id).unwrap();
        assert_eq!(resolution.status, "upheld");
        assert!(resolution.reputation_zeroed);

        // Reputation should be deleted
        let count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reputation_assertions \
                 WHERE actor_address = 'stake_test1ulearner' AND skill_id = 'sk1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count_after, 0);
    }

    // ---- Test 11: check_expired_challenges expires old challenges ----
    #[test]
    fn check_expired_challenges_expires_old() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        let challenge = submit_challenge(conn, &params).unwrap();

        // Manually set expires_at to the past
        conn.execute(
            "UPDATE evidence_challenges SET expires_at = datetime('now', '-1 day') WHERE id = ?1",
            params![challenge.id],
        )
        .unwrap();

        let expired = check_expired_challenges(conn).unwrap();
        assert_eq!(expired, 1);

        // Verify status is expired
        let status: String = conn
            .query_row(
                "SELECT status FROM evidence_challenges WHERE id = ?1",
                params![challenge.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "expired");
    }

    // ---- Test 12: list_challenges filters by status ----
    #[test]
    fn list_challenges_filters_by_status() {
        let db = setup_db();
        let conn = db.conn();

        // Create two challenges
        let params1 = default_params();
        submit_challenge(conn, &params1).unwrap();

        let mut params2 = default_params();
        params2.reason = "different reason".into();
        params2.target_ids = vec!["ev_other".into()];
        submit_challenge(conn, &params2).unwrap();

        // Both should be pending
        let all = list_challenges(conn, None, None, None, None).unwrap();
        assert_eq!(all.len(), 2);

        let pending = list_challenges(conn, Some("pending"), None, None, None).unwrap();
        assert_eq!(pending.len(), 2);

        // None should be reviewing yet
        let reviewing = list_challenges(conn, Some("reviewing"), None, None, None).unwrap();
        assert_eq!(reviewing.len(), 0);
    }

    // ---- Additional: get_challenge returns votes ----
    #[test]
    fn get_challenge_returns_votes() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        let challenge = submit_challenge(conn, &params).unwrap();
        vote_on_challenge(conn, &challenge.id, "stake_test1ucommittee1", true, None).unwrap();

        let (ch, votes) = get_challenge(conn, &challenge.id).unwrap();
        assert_eq!(ch.id, challenge.id);
        assert_eq!(votes.len(), 1);
        assert_eq!(votes[0].voter, "stake_test1ucommittee1");
    }

    // ---- Additional: list by dao_id filter ----
    #[test]
    fn list_challenges_filters_by_dao_id() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        submit_challenge(conn, &params).unwrap();

        let matches = list_challenges(conn, None, Some("dao1"), None, None).unwrap();
        assert_eq!(matches.len(), 1);

        let no_matches = list_challenges(conn, None, Some("nonexistent"), None, None).unwrap();
        assert_eq!(no_matches.len(), 0);
    }

    // ---- Additional: skill_proof target type works ----
    #[test]
    fn submit_with_skill_proof_target() {
        let db = setup_db();
        let conn = db.conn();
        let mut params = default_params();
        params.target_type = "skill_proof".into();
        params.target_ids = vec!["sp1".into()];

        let challenge = submit_challenge(conn, &params).unwrap();
        assert_eq!(challenge.target_type, "skill_proof");
    }

    // ---- Additional: empty target_ids rejected ----
    #[test]
    fn submit_rejects_empty_target_ids() {
        let db = setup_db();
        let conn = db.conn();
        let mut params = default_params();
        params.target_ids = vec![];

        let result = submit_challenge(conn, &params);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("target_ids must not be empty"));
    }

    // ---- Additional: empty reason rejected ----
    #[test]
    fn submit_rejects_empty_reason() {
        let db = setup_db();
        let conn = db.conn();
        let mut params = default_params();
        params.reason = String::new();

        let result = submit_challenge(conn, &params);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("reason must not be empty"));
    }

    // ---- Additional: resolve with no votes fails ----
    #[test]
    fn resolve_with_no_votes_fails() {
        let db = setup_db();
        let conn = db.conn();
        let params = default_params();

        let challenge = submit_challenge(conn, &params).unwrap();
        let result = resolve_challenge(conn, &challenge.id);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no votes"));
    }

    // ---- Additional: inactive DAO rejected ----
    #[test]
    fn submit_rejects_inactive_dao() {
        let db = setup_db();
        let conn = db.conn();

        // Mark DAO as inactive
        conn.execute(
            "UPDATE governance_daos SET status = 'pending' WHERE id = 'dao1'",
            [],
        )
        .unwrap();

        let params = default_params();
        let result = submit_challenge(conn, &params);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not active"));
    }
}
