//! Multi-party attestation logic.
//!
//! Implements governance-gated attestation requirements for high-stakes
//! skills. When a DAO marks a skill+proficiency as requiring multi-party
//! attestation, evidence records need assessor co-signatures before they
//! count toward skill proof aggregation.
//!
//! Key functions:
//!   - `check_attestation_required` — look up requirement for a skill
//!   - `set_attestation_requirement` — governance action to set requirement
//!   - `submit_attestation` — assessor co-signs evidence
//!   - `is_evidence_fully_attested` — check if evidence has enough signatures

use rusqlite::{params, Connection};

use crate::crypto::hash::entity_id;
use crate::domain::attestation::{
    AttestationRequirement, AttestationStatus, EvidenceAttestation, SetRequirementParams,
    SubmitAttestationParams,
};

/// Check whether multi-party attestation is required for a skill at a
/// given proficiency level.
///
/// Returns `None` if no requirement exists (self-attestation is the default).
pub fn check_attestation_required(
    conn: &Connection,
    skill_id: &str,
    proficiency_level: &str,
) -> Result<Option<AttestationRequirement>, String> {
    let result = conn.query_row(
        "SELECT skill_id, proficiency_level, required_attestors, dao_id, \
         set_by_proposal, created_at, updated_at \
         FROM attestation_requirements \
         WHERE skill_id = ?1 AND proficiency_level = ?2",
        params![skill_id, proficiency_level],
        |row| {
            Ok(AttestationRequirement {
                skill_id: row.get(0)?,
                proficiency_level: row.get(1)?,
                required_attestors: row.get(2)?,
                dao_id: row.get(3)?,
                set_by_proposal: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        },
    );

    match result {
        Ok(req) => Ok(Some(req)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Set (or update) an attestation requirement for a skill+proficiency.
///
/// This is a governance action — typically triggered by a DAO proposal.
pub fn set_attestation_requirement(
    conn: &Connection,
    params: &SetRequirementParams,
) -> Result<AttestationRequirement, String> {
    conn.execute(
        "INSERT INTO attestation_requirements \
         (skill_id, proficiency_level, required_attestors, dao_id, set_by_proposal) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(skill_id, proficiency_level) DO UPDATE SET \
            required_attestors = excluded.required_attestors, \
            dao_id = excluded.dao_id, \
            set_by_proposal = excluded.set_by_proposal, \
            updated_at = datetime('now')",
        params![
            params.skill_id,
            params.proficiency_level,
            params.required_attestors,
            params.dao_id,
            params.set_by_proposal,
        ],
    )
    .map_err(|e| e.to_string())?;

    // Return the created/updated record
    check_attestation_required(conn, &params.skill_id, &params.proficiency_level)?
        .ok_or_else(|| "failed to read back attestation requirement".to_string())
}

/// Remove an attestation requirement for a skill+proficiency.
///
/// After removal, evidence for that skill+level no longer requires
/// multi-party attestation. Existing attestations are not deleted.
pub fn remove_attestation_requirement(
    conn: &Connection,
    skill_id: &str,
    proficiency_level: &str,
) -> Result<bool, String> {
    let affected = conn
        .execute(
            "DELETE FROM attestation_requirements \
             WHERE skill_id = ?1 AND proficiency_level = ?2",
            params![skill_id, proficiency_level],
        )
        .map_err(|e| e.to_string())?;

    Ok(affected > 0)
}

/// Submit an attestation (assessor co-signs evidence).
///
/// Verifies:
///   1. The attestor is a DAO assessor (member with `role = 'assessor'`
///      in an active DAO).
///   2. The evidence record exists.
///   3. No duplicate attestation from the same attestor.
///
/// The attestor's signature is passed in from the caller (the command
/// layer handles signing with the local keystore).
pub fn submit_attestation(
    conn: &Connection,
    attestor_address: &str,
    signature: &str,
    params: &SubmitAttestationParams,
) -> Result<EvidenceAttestation, String> {
    // 1. Verify the attestor is a DAO assessor in any active DAO
    let is_assessor: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM governance_dao_members m \
             JOIN governance_daos d ON d.id = m.dao_id \
             WHERE m.stake_address = ?1 AND m.role = 'assessor' AND d.status = 'active'",
            params![attestor_address],
            |row| Ok(row.get::<_, i64>(0)? > 0),
        )
        .map_err(|e| e.to_string())?;

    if !is_assessor {
        return Err("attestor is not an assessor in any active DAO".to_string());
    }

    // 2. Verify the evidence record exists
    let _evidence_exists: String = conn
        .query_row(
            "SELECT id FROM evidence_records WHERE id = ?1",
            params![params.evidence_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("evidence record not found: {e}"))?;

    // 3. Check for duplicate attestation
    let already_attested: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM evidence_attestations \
             WHERE evidence_id = ?1 AND attestor_address = ?2",
            params![params.evidence_id, attestor_address],
            |row| Ok(row.get::<_, i64>(0)? > 0),
        )
        .map_err(|e| e.to_string())?;

    if already_attested {
        return Err("attestor has already attested this evidence".to_string());
    }

    let attestation_type = params.attestation_type.as_deref().unwrap_or("co_sign");

    let id = entity_id(&[&params.evidence_id, attestor_address]);

    conn.execute(
        "INSERT INTO evidence_attestations \
         (id, evidence_id, attestor_address, attestor_role, attestation_type, \
          integrity_score, session_cid, signature) \
         VALUES (?1, ?2, ?3, 'assessor', ?4, ?5, ?6, ?7)",
        params![
            id,
            params.evidence_id,
            attestor_address,
            attestation_type,
            params.integrity_score,
            params.session_cid,
            signature,
        ],
    )
    .map_err(|e| e.to_string())?;

    // Read back the created record
    conn.query_row(
        "SELECT id, evidence_id, attestor_address, attestor_role, attestation_type, \
         integrity_score, session_cid, signature, created_at \
         FROM evidence_attestations WHERE id = ?1",
        params![id],
        |row| {
            Ok(EvidenceAttestation {
                id: row.get(0)?,
                evidence_id: row.get(1)?,
                attestor_address: row.get(2)?,
                attestor_role: row.get(3)?,
                attestation_type: row.get(4)?,
                integrity_score: row.get(5)?,
                session_cid: row.get(6)?,
                signature: row.get(7)?,
                created_at: row.get(8)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

/// Get the full attestation status for an evidence record.
///
/// If no attestation requirement exists for the evidence's skill+level,
/// the evidence is considered fully attested by default.
pub fn get_attestation_status(
    conn: &Connection,
    evidence_id: &str,
) -> Result<AttestationStatus, String> {
    // Get skill_id and proficiency_level from the evidence record
    let (skill_id, proficiency_level): (String, String) = conn
        .query_row(
            "SELECT skill_id, proficiency_level FROM evidence_records WHERE id = ?1",
            params![evidence_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("evidence record not found: {e}"))?;

    // Check if there's a requirement
    let requirement = check_attestation_required(conn, &skill_id, &proficiency_level)?;

    // Get current attestations
    let attestations = list_attestations_for_evidence(conn, evidence_id)?;
    let current_attestors = attestations.len() as i64;

    let (required_attestors, is_fully_attested) = match requirement {
        Some(req) => {
            let fully = current_attestors >= req.required_attestors;
            (req.required_attestors, fully)
        }
        None => {
            // No requirement = self-attestation is default (always fully attested)
            (0, true)
        }
    };

    Ok(AttestationStatus {
        evidence_id: evidence_id.to_string(),
        skill_id,
        proficiency_level,
        required_attestors,
        current_attestors,
        is_fully_attested,
        attestations,
    })
}

/// List all attestation requirements, optionally filtered by dao_id.
pub fn list_attestation_requirements(
    conn: &Connection,
    dao_id: Option<&str>,
) -> Result<Vec<AttestationRequirement>, String> {
    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(dao) = dao_id {
            (
                "SELECT skill_id, proficiency_level, required_attestors, dao_id, \
                 set_by_proposal, created_at, updated_at \
                 FROM attestation_requirements WHERE dao_id = ?1 \
                 ORDER BY skill_id, proficiency_level"
                    .to_string(),
                vec![Box::new(dao.to_string())],
            )
        } else {
            (
                "SELECT skill_id, proficiency_level, required_attestors, dao_id, \
                 set_by_proposal, created_at, updated_at \
                 FROM attestation_requirements \
                 ORDER BY skill_id, proficiency_level"
                    .to_string(),
                vec![],
            )
        };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let requirements = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(AttestationRequirement {
                skill_id: row.get(0)?,
                proficiency_level: row.get(1)?,
                required_attestors: row.get(2)?,
                dao_id: row.get(3)?,
                set_by_proposal: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(requirements)
}

/// Get all attestations on an evidence record.
pub fn list_attestations_for_evidence(
    conn: &Connection,
    evidence_id: &str,
) -> Result<Vec<EvidenceAttestation>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, evidence_id, attestor_address, attestor_role, attestation_type, \
             integrity_score, session_cid, signature, created_at \
             FROM evidence_attestations WHERE evidence_id = ?1 \
             ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let attestations = stmt
        .query_map(params![evidence_id], |row| {
            Ok(EvidenceAttestation {
                id: row.get(0)?,
                evidence_id: row.get(1)?,
                attestor_address: row.get(2)?,
                attestor_role: row.get(3)?,
                attestation_type: row.get(4)?,
                integrity_score: row.get(5)?,
                session_cid: row.get(6)?,
                signature: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(attestations)
}

/// Quick boolean check: does the evidence have enough attestations?
///
/// If no attestation requirement exists for the evidence's skill+level,
/// returns `true` (self-attestation is the default).
pub fn is_evidence_fully_attested(conn: &Connection, evidence_id: &str) -> Result<bool, String> {
    let status = get_attestation_status(conn, evidence_id)?;
    Ok(status.is_fully_attested)
}

/// Find evidence records that require attestation but don't have enough.
///
/// Returns evidence IDs that:
///   1. Have a matching attestation requirement (skill+level)
///   2. Have fewer attestations than required
pub fn list_unattested_evidence(conn: &Connection) -> Result<Vec<AttestationStatus>, String> {
    // Find evidence records whose skill+level has a requirement,
    // and whose attestation count is below the threshold.
    let mut stmt = conn
        .prepare(
            "SELECT er.id, er.skill_id, er.proficiency_level, ar.required_attestors, \
             COALESCE(ea_count.cnt, 0) as current_count \
             FROM evidence_records er \
             JOIN attestation_requirements ar \
               ON ar.skill_id = er.skill_id AND ar.proficiency_level = er.proficiency_level \
             LEFT JOIN ( \
               SELECT evidence_id, COUNT(*) as cnt \
               FROM evidence_attestations \
               GROUP BY evidence_id \
             ) ea_count ON ea_count.evidence_id = er.id \
             WHERE COALESCE(ea_count.cnt, 0) < ar.required_attestors \
             ORDER BY er.created_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows: Vec<(String, String, String, i64, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for (evidence_id, skill_id, proficiency_level, required, current) in rows {
        let attestations = list_attestations_for_evidence(conn, &evidence_id)?;
        results.push(AttestationStatus {
            evidence_id,
            skill_id,
            proficiency_level,
            required_attestors: required,
            current_attestors: current,
            is_fully_attested: false,
            attestations,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    /// Create an in-memory database with the test fixtures needed for
    /// attestation tests: subject field, subject, skill, course,
    /// DAO, evidence record, and local identity.
    fn setup_db() -> Database {
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrations");

        let conn = db.conn();

        // Taxonomy
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
            "INSERT INTO skills (id, name, subject_id) VALUES ('sk1', 'Graph Traversal', 'sub1')",
            [],
        )
        .unwrap();

        // Local identity
        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1ulearner', 'addr_test1qlearner')",
            [],
        )
        .unwrap();

        // Course
        conn.execute(
            "INSERT INTO courses (id, title, author_address) \
             VALUES ('c1', 'Algo Course', 'stake_test1uinstructor')",
            [],
        )
        .unwrap();

        // Skill assessment
        conn.execute(
            "INSERT INTO skill_assessments (id, skill_id, course_id, assessment_type, proficiency_level, difficulty) \
             VALUES ('sa1', 'sk1', 'c1', 'quiz', 'apply', 0.50)",
            [],
        )
        .unwrap();

        // Evidence record
        conn.execute(
            "INSERT INTO evidence_records \
             (id, skill_assessment_id, skill_id, proficiency_level, score, difficulty, trust_factor, \
              course_id, instructor_address, signature) \
             VALUES ('ev1', 'sa1', 'sk1', 'apply', 0.85, 0.50, 1.0, 'c1', 'stake_test1uinstructor', 'sig_ev1')",
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

        db
    }

    /// Add an assessor to the DAO.
    fn add_assessor(conn: &Connection, address: &str) {
        conn.execute(
            "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
             VALUES ('dao1', ?1, 'assessor')",
            params![address],
        )
        .unwrap();
    }

    // ---- Tests ----

    #[test]
    fn check_no_requirement_returns_none() {
        let db = setup_db();
        let conn = db.conn();

        let result = check_attestation_required(conn, "sk1", "apply").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn set_attestation_requirement_creates_record() {
        let db = setup_db();
        let conn = db.conn();

        let req = set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 2,
                dao_id: "dao1".into(),
                set_by_proposal: Some("prop1".into()),
            },
        )
        .unwrap();

        assert_eq!(req.skill_id, "sk1");
        assert_eq!(req.proficiency_level, "apply");
        assert_eq!(req.required_attestors, 2);
        assert_eq!(req.dao_id, "dao1");
        assert_eq!(req.set_by_proposal, Some("prop1".into()));
    }

    #[test]
    fn set_requirement_updates_on_conflict() {
        let db = setup_db();
        let conn = db.conn();

        // First insert
        set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 2,
                dao_id: "dao1".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        // Update on conflict
        let updated = set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 3,
                dao_id: "dao1".into(),
                set_by_proposal: Some("prop2".into()),
            },
        )
        .unwrap();

        assert_eq!(updated.required_attestors, 3);
        assert_eq!(updated.set_by_proposal, Some("prop2".into()));

        // Only one row should exist
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM attestation_requirements", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn submit_attestation_creates_record() {
        let db = setup_db();
        let conn = db.conn();

        add_assessor(conn, "stake_test1uassessor1");

        let attestation = submit_attestation(
            conn,
            "stake_test1uassessor1",
            "sig_assessor1",
            &SubmitAttestationParams {
                evidence_id: "ev1".into(),
                attestation_type: None,
                integrity_score: Some(0.95),
                session_cid: None,
            },
        )
        .unwrap();

        assert_eq!(attestation.evidence_id, "ev1");
        assert_eq!(attestation.attestor_address, "stake_test1uassessor1");
        assert_eq!(attestation.attestor_role, "assessor");
        assert_eq!(attestation.attestation_type, "co_sign");
        assert_eq!(attestation.integrity_score, Some(0.95));
        assert_eq!(attestation.signature, "sig_assessor1");
    }

    #[test]
    fn submit_rejects_non_assessor() {
        let db = setup_db();
        let conn = db.conn();

        // Add as regular member, not assessor
        conn.execute(
            "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
             VALUES ('dao1', 'stake_test1umember', 'member')",
            [],
        )
        .unwrap();

        let result = submit_attestation(
            conn,
            "stake_test1umember",
            "sig_member",
            &SubmitAttestationParams {
                evidence_id: "ev1".into(),
                attestation_type: None,
                integrity_score: None,
                session_cid: None,
            },
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not an assessor"));
    }

    #[test]
    fn submit_rejects_duplicate() {
        let db = setup_db();
        let conn = db.conn();

        add_assessor(conn, "stake_test1uassessor1");

        // First attestation succeeds
        submit_attestation(
            conn,
            "stake_test1uassessor1",
            "sig1",
            &SubmitAttestationParams {
                evidence_id: "ev1".into(),
                attestation_type: None,
                integrity_score: None,
                session_cid: None,
            },
        )
        .unwrap();

        // Second from same attestor fails
        let result = submit_attestation(
            conn,
            "stake_test1uassessor1",
            "sig2",
            &SubmitAttestationParams {
                evidence_id: "ev1".into(),
                attestation_type: None,
                integrity_score: None,
                session_cid: None,
            },
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already attested"));
    }

    #[test]
    fn get_attestation_status_no_requirement() {
        let db = setup_db();
        let conn = db.conn();

        let status = get_attestation_status(conn, "ev1").unwrap();

        assert_eq!(status.evidence_id, "ev1");
        assert_eq!(status.required_attestors, 0);
        assert_eq!(status.current_attestors, 0);
        assert!(status.is_fully_attested);
        assert!(status.attestations.is_empty());
    }

    #[test]
    fn get_attestation_status_insufficient() {
        let db = setup_db();
        let conn = db.conn();

        // Set requirement for 2 attestors
        set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 2,
                dao_id: "dao1".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        // Add one attestation
        add_assessor(conn, "stake_test1uassessor1");
        submit_attestation(
            conn,
            "stake_test1uassessor1",
            "sig1",
            &SubmitAttestationParams {
                evidence_id: "ev1".into(),
                attestation_type: None,
                integrity_score: None,
                session_cid: None,
            },
        )
        .unwrap();

        let status = get_attestation_status(conn, "ev1").unwrap();

        assert_eq!(status.required_attestors, 2);
        assert_eq!(status.current_attestors, 1);
        assert!(!status.is_fully_attested);
        assert_eq!(status.attestations.len(), 1);
    }

    #[test]
    fn get_attestation_status_sufficient() {
        let db = setup_db();
        let conn = db.conn();

        // Require 2 attestors
        set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 2,
                dao_id: "dao1".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        // Add two attestations
        add_assessor(conn, "stake_test1uassessor1");
        add_assessor(conn, "stake_test1uassessor2");

        submit_attestation(
            conn,
            "stake_test1uassessor1",
            "sig1",
            &SubmitAttestationParams {
                evidence_id: "ev1".into(),
                attestation_type: None,
                integrity_score: None,
                session_cid: None,
            },
        )
        .unwrap();

        submit_attestation(
            conn,
            "stake_test1uassessor2",
            "sig2",
            &SubmitAttestationParams {
                evidence_id: "ev1".into(),
                attestation_type: None,
                integrity_score: None,
                session_cid: None,
            },
        )
        .unwrap();

        let status = get_attestation_status(conn, "ev1").unwrap();

        assert_eq!(status.required_attestors, 2);
        assert_eq!(status.current_attestors, 2);
        assert!(status.is_fully_attested);
        assert_eq!(status.attestations.len(), 2);
    }

    #[test]
    fn is_fully_attested_true_when_no_requirement() {
        let db = setup_db();
        let conn = db.conn();

        let result = is_evidence_fully_attested(conn, "ev1").unwrap();
        assert!(result);
    }

    #[test]
    fn is_fully_attested_false_when_insufficient() {
        let db = setup_db();
        let conn = db.conn();

        set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 1,
                dao_id: "dao1".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        // No attestations submitted
        let result = is_evidence_fully_attested(conn, "ev1").unwrap();
        assert!(!result);
    }

    #[test]
    fn list_unattested_evidence_finds_pending() {
        let db = setup_db();
        let conn = db.conn();

        // Set requirement
        set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 2,
                dao_id: "dao1".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        // ev1 has no attestations → should appear in unattested list
        let unattested = list_unattested_evidence(conn).unwrap();
        assert_eq!(unattested.len(), 1);
        assert_eq!(unattested[0].evidence_id, "ev1");
        assert_eq!(unattested[0].required_attestors, 2);
        assert_eq!(unattested[0].current_attestors, 0);
        assert!(!unattested[0].is_fully_attested);

        // Add enough attestations → should no longer appear
        add_assessor(conn, "stake_test1uassessor1");
        add_assessor(conn, "stake_test1uassessor2");

        submit_attestation(
            conn,
            "stake_test1uassessor1",
            "sig1",
            &SubmitAttestationParams {
                evidence_id: "ev1".into(),
                attestation_type: None,
                integrity_score: None,
                session_cid: None,
            },
        )
        .unwrap();
        submit_attestation(
            conn,
            "stake_test1uassessor2",
            "sig2",
            &SubmitAttestationParams {
                evidence_id: "ev1".into(),
                attestation_type: None,
                integrity_score: None,
                session_cid: None,
            },
        )
        .unwrap();

        let unattested = list_unattested_evidence(conn).unwrap();
        assert!(unattested.is_empty());
    }

    #[test]
    fn remove_requirement_works() {
        let db = setup_db();
        let conn = db.conn();

        set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 2,
                dao_id: "dao1".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        let removed = remove_attestation_requirement(conn, "sk1", "apply").unwrap();
        assert!(removed);

        let check = check_attestation_required(conn, "sk1", "apply").unwrap();
        assert!(check.is_none());

        // Removing again returns false
        let removed_again = remove_attestation_requirement(conn, "sk1", "apply").unwrap();
        assert!(!removed_again);
    }

    #[test]
    fn list_requirements_filters_by_dao() {
        let db = setup_db();
        let conn = db.conn();

        // Create a second DAO
        conn.execute(
            "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
             VALUES ('dao2', 'Math DAO', 'subject_field', 'sf1', 'active')",
            [],
        )
        .unwrap();

        // Create a second skill
        conn.execute(
            "INSERT INTO skills (id, name, subject_id) VALUES ('sk2', 'Sorting', 'sub1')",
            [],
        )
        .unwrap();

        // Requirements in different DAOs
        set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 2,
                dao_id: "dao1".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();
        set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk2".into(),
                proficiency_level: "evaluate".into(),
                required_attestors: 3,
                dao_id: "dao2".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        // All requirements
        let all = list_attestation_requirements(conn, None).unwrap();
        assert_eq!(all.len(), 2);

        // Filter by dao1
        let dao1_reqs = list_attestation_requirements(conn, Some("dao1")).unwrap();
        assert_eq!(dao1_reqs.len(), 1);
        assert_eq!(dao1_reqs[0].skill_id, "sk1");

        // Filter by dao2
        let dao2_reqs = list_attestation_requirements(conn, Some("dao2")).unwrap();
        assert_eq!(dao2_reqs.len(), 1);
        assert_eq!(dao2_reqs[0].skill_id, "sk2");
    }
}
