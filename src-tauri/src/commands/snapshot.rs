//! IPC commands for reputation snapshot anchoring.
//!
//! Manages CIP-68 soulbound reputation token lifecycle:
//!   - Create a snapshot from current reputation state
//!   - List snapshots with status tracking
//!   - Get snapshot details
//!   - Retry failed snapshots

use rusqlite::params;
use tauri::State;

use crate::cardano::snapshot;
use crate::crypto::hash::entity_id;
use crate::domain::reputation::{
    cip68, CreateSnapshotParams, OnChainSkillScore, ReputationRole, SnapshotRecord,
    SnapshotStatus,
};
use crate::AppState;

/// Create a reputation snapshot for on-chain anchoring.
///
/// Gathers all reputation assertions for the specified subject+role,
/// converts them to on-chain format, creates a snapshot record,
/// and prepares it for transaction building.
#[tauri::command]
pub async fn snapshot_reputation(
    state: State<'_, AppState>,
    params: CreateSnapshotParams,
) -> Result<SnapshotRecord, String> {
    let db = state.db.write().await;
    let conn = db.conn();

    // Validate role
    let role = ReputationRole::from_str(&params.role)
        .ok_or_else(|| format!("invalid role: {}", params.role))?;

    // Get the local identity
    let actor_address: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no local identity: {e}"))?;

    // Query all reputation assertions for this subject + role
    let mut stmt = conn
        .prepare(
            "SELECT ra.skill_id, ra.proficiency_level, ra.score, ra.evidence_count \
             FROM reputation_assertions ra \
             JOIN skills s ON s.id = ra.skill_id \
             JOIN subjects sub ON sub.id = s.subject_id \
             WHERE ra.actor_address = ?1 AND ra.role = ?2 AND sub.id = ?3 \
             ORDER BY ra.skill_id",
        )
        .map_err(|e| e.to_string())?;

    let skills: Vec<OnChainSkillScore> = stmt
        .query_map(
            params![actor_address, params.role, params.subject_id],
            |row| {
                let skill_id: String = row.get(0)?;
                let prof_level: String = row.get(1)?;
                let score: f64 = row.get(2)?;
                let evidence_count: i64 = row.get(3)?;

                // Compute confidence (smoothed for instructors, direct for learners)
                let confidence = if params.role == "instructor" {
                    evidence_count as f64 / (evidence_count as f64 + 5.0)
                } else {
                    score
                };

                Ok(OnChainSkillScore {
                    skill_id_bytes: hex::encode(
                        skill_id
                            .as_bytes()
                            .iter()
                            .take(16)
                            .copied()
                            .collect::<Vec<u8>>(),
                    ),
                    proficiency: snapshot::proficiency_to_index(&prof_level),
                    impact_score: (score * cip68::IMPACT_SCALE as f64) as i64,
                    confidence: (confidence * cip68::CONFIDENCE_SCALE as f64) as i64,
                    evidence_count,
                })
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let skill_count = skills.len() as i64;

    // Generate snapshot ID
    let now = chrono::Utc::now().to_rfc3339();
    let snapshot_id = entity_id(&[&actor_address, &params.subject_id, &params.role, &now]);

    // Build asset names
    let base_name = snapshot::reputation_base_name(&params.subject_id, &role);
    let ref_name = snapshot::reference_asset_name(&base_name);
    let usr_name = snapshot::user_asset_name(&base_name);

    // Insert snapshot record
    conn.execute(
        "INSERT INTO reputation_snapshots \
         (id, actor_address, subject_id, role, skill_count, tx_status, \
          ref_asset_name, user_asset_name, snapshot_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))",
        params![
            snapshot_id,
            actor_address,
            params.subject_id,
            params.role,
            skill_count,
            SnapshotStatus::Pending.as_str(),
            hex::encode(&ref_name),
            hex::encode(&usr_name),
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(SnapshotRecord {
        id: snapshot_id,
        actor_address,
        subject_id: params.subject_id,
        role: params.role,
        skill_count,
        tx_status: SnapshotStatus::Pending.as_str().into(),
        tx_hash: None,
        policy_id: None,
        ref_asset_name: Some(hex::encode(&ref_name)),
        user_asset_name: Some(hex::encode(&usr_name)),
        error_message: None,
        snapshot_at: now,
        confirmed_at: None,
    })
}

/// List reputation snapshots with optional status filter.
#[tauri::command]
pub async fn list_snapshots(
    state: State<'_, AppState>,
    status: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<SnapshotRecord>, String> {
    let db = state.db.read().await;
    let conn = db.conn();
    let max = limit.unwrap_or(50);

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(ref s) = status {
            (
                "SELECT id, actor_address, subject_id, role, skill_count, tx_status, \
                 tx_hash, policy_id, ref_asset_name, user_asset_name, error_message, \
                 snapshot_at, confirmed_at \
                 FROM reputation_snapshots WHERE tx_status = ?1 \
                 ORDER BY snapshot_at DESC LIMIT ?2"
                    .into(),
                vec![Box::new(s.clone()), Box::new(max)],
            )
        } else {
            (
                "SELECT id, actor_address, subject_id, role, skill_count, tx_status, \
                 tx_hash, policy_id, ref_asset_name, user_asset_name, error_message, \
                 snapshot_at, confirmed_at \
                 FROM reputation_snapshots \
                 ORDER BY snapshot_at DESC LIMIT ?1"
                    .into(),
                vec![Box::new(max)],
            )
        };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let snapshots = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(SnapshotRecord {
                id: row.get(0)?,
                actor_address: row.get(1)?,
                subject_id: row.get(2)?,
                role: row.get(3)?,
                skill_count: row.get(4)?,
                tx_status: row.get(5)?,
                tx_hash: row.get(6)?,
                policy_id: row.get(7)?,
                ref_asset_name: row.get(8)?,
                user_asset_name: row.get(9)?,
                error_message: row.get(10)?,
                snapshot_at: row.get(11)?,
                confirmed_at: row.get(12)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(snapshots)
}

/// Get a specific snapshot by ID.
#[tauri::command]
pub async fn get_snapshot(
    state: State<'_, AppState>,
    snapshot_id: String,
) -> Result<SnapshotRecord, String> {
    let db = state.db.read().await;
    let conn = db.conn();

    conn.query_row(
        "SELECT id, actor_address, subject_id, role, skill_count, tx_status, \
         tx_hash, policy_id, ref_asset_name, user_asset_name, error_message, \
         snapshot_at, confirmed_at \
         FROM reputation_snapshots WHERE id = ?1",
        params![snapshot_id],
        |row| {
            Ok(SnapshotRecord {
                id: row.get(0)?,
                actor_address: row.get(1)?,
                subject_id: row.get(2)?,
                role: row.get(3)?,
                skill_count: row.get(4)?,
                tx_status: row.get(5)?,
                tx_hash: row.get(6)?,
                policy_id: row.get(7)?,
                ref_asset_name: row.get(8)?,
                user_asset_name: row.get(9)?,
                error_message: row.get(10)?,
                snapshot_at: row.get(11)?,
                confirmed_at: row.get(12)?,
            })
        },
    )
    .map_err(|e| format!("snapshot not found: {e}"))
}

/// Update snapshot status (used internally during tx building/submission).
#[tauri::command]
pub async fn update_snapshot_status(
    state: State<'_, AppState>,
    snapshot_id: String,
    status: String,
    tx_hash: Option<String>,
    policy_id: Option<String>,
    error_message: Option<String>,
) -> Result<(), String> {
    let db = state.db.write().await;
    let conn = db.conn();

    // Validate status
    let _status_enum = SnapshotStatus::from_str(&status)
        .ok_or_else(|| format!("invalid status: {status}"))?;

    let confirmed_at = if status == "confirmed" {
        Some(chrono::Utc::now().to_rfc3339())
    } else {
        None
    };

    conn.execute(
        "UPDATE reputation_snapshots SET \
         tx_status = ?1, tx_hash = ?2, policy_id = ?3, \
         error_message = ?4, confirmed_at = ?5 \
         WHERE id = ?6",
        params![
            status,
            tx_hash,
            policy_id,
            error_message,
            confirmed_at,
            snapshot_id
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
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

    fn setup_reputation_data(db: &Database) {
        let conn = db.conn();

        // Identity
        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1ulearner', 'addr_test1q123')",
            [],
        )
        .unwrap();

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
            "INSERT INTO skills (id, name, subject_id) VALUES ('sk1', 'Graphs', 'sub1')",
            [],
        )
        .unwrap();

        // Reputation assertion
        conn.execute(
            "INSERT INTO reputation_assertions \
             (id, actor_address, role, skill_id, proficiency_level, score, evidence_count, \
              computation_spec) \
             VALUES ('ra1', 'stake_test1ulearner', 'instructor', 'sk1', 'apply', 0.85, 5, 'v2')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn snapshot_record_created() {
        let db = test_db();
        setup_reputation_data(&db);
        let conn = db.conn();

        let now = chrono::Utc::now().to_rfc3339();
        let snapshot_id =
            entity_id(&["stake_test1ulearner", "sub1", "instructor", &now]);
        let base_name =
            snapshot::reputation_base_name("sub1", &ReputationRole::Instructor);
        let ref_name = snapshot::reference_asset_name(&base_name);
        let usr_name = snapshot::user_asset_name(&base_name);

        conn.execute(
            "INSERT INTO reputation_snapshots \
             (id, actor_address, subject_id, role, skill_count, tx_status, \
              ref_asset_name, user_asset_name) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                snapshot_id,
                "stake_test1ulearner",
                "sub1",
                "instructor",
                1,
                "pending",
                hex::encode(&ref_name),
                hex::encode(&usr_name),
            ],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reputation_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn snapshot_status_update() {
        let db = test_db();
        let conn = db.conn();

        conn.execute(
            "INSERT INTO reputation_snapshots \
             (id, actor_address, subject_id, role, skill_count, tx_status) \
             VALUES ('snap1', 'addr1', 'sub1', 'instructor', 1, 'pending')",
            [],
        )
        .unwrap();

        conn.execute(
            "UPDATE reputation_snapshots SET tx_status = 'submitted', \
             tx_hash = 'abc123' WHERE id = 'snap1'",
            [],
        )
        .unwrap();

        let (status, hash): (String, Option<String>) = conn
            .query_row(
                "SELECT tx_status, tx_hash FROM reputation_snapshots WHERE id = 'snap1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(status, "submitted");
        assert_eq!(hash.unwrap(), "abc123");
    }

    #[test]
    fn snapshot_status_enum_roundtrip() {
        for status in &[
            SnapshotStatus::Pending,
            SnapshotStatus::Building,
            SnapshotStatus::Submitted,
            SnapshotStatus::Confirmed,
            SnapshotStatus::Failed,
        ] {
            let s = status.as_str();
            let parsed = SnapshotStatus::from_str(s).unwrap();
            assert_eq!(*status, parsed);
        }
    }

    #[test]
    fn snapshot_with_skills_from_reputation() {
        let db = test_db();
        setup_reputation_data(&db);
        let conn = db.conn();

        // Query skills for the snapshot
        let mut stmt = conn
            .prepare(
                "SELECT ra.skill_id, ra.proficiency_level, ra.score, ra.evidence_count \
                 FROM reputation_assertions ra \
                 JOIN skills s ON s.id = ra.skill_id \
                 JOIN subjects sub ON sub.id = s.subject_id \
                 WHERE ra.actor_address = 'stake_test1ulearner' \
                 AND ra.role = 'instructor' AND sub.id = 'sub1'",
            )
            .unwrap();

        let skills: Vec<OnChainSkillScore> = stmt
            .query_map([], |row| {
                let skill_id: String = row.get(0)?;
                let prof_level: String = row.get(1)?;
                let score: f64 = row.get(2)?;
                let evidence_count: i64 = row.get(3)?;

                Ok(OnChainSkillScore {
                    skill_id_bytes: hex::encode(
                        skill_id
                            .as_bytes()
                            .iter()
                            .take(16)
                            .copied()
                            .collect::<Vec<u8>>(),
                    ),
                    proficiency: snapshot::proficiency_to_index(&prof_level),
                    impact_score: (score * cip68::IMPACT_SCALE as f64) as i64,
                    confidence: (evidence_count as f64
                        / (evidence_count as f64 + 5.0)
                        * cip68::CONFIDENCE_SCALE as f64)
                        as i64,
                    evidence_count,
                })
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].impact_score, 850_000);
        assert_eq!(skills[0].evidence_count, 5);
    }
}
