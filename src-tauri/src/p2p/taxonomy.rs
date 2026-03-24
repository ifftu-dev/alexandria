//! Taxonomy sync — gossip-based skill graph synchronization.
//!
//! Handles incoming ratified taxonomy updates from the DAO committee.
//! Per spec §8.2: taxonomy changes are DAO-ratified, signed by committee
//! members, and propagated via `/alexandria/taxonomy/1.0`.
//!
//! When a TaxonomyUpdate is received:
//! 1. Validate the version is newer than what we have locally
//! 2. Validate the previous_cid chain links to our current version
//! 3. Apply changes to local skill tables (subject_fields, subjects, skills)
//! 4. Record the new version in `taxonomy_versions`
//! 5. Record in `sync_log`
//!
//! **Authority**: The validation pipeline (§7.3) screens taxonomy messages,
//! and this handler re-checks committee membership and chain continuity
//! before applying any update.

use rusqlite::params;

use crate::db::Database;
use crate::domain::taxonomy::TaxonomyUpdate;
use crate::p2p::types::SignedGossipMessage;

/// Handle an incoming taxonomy update from the P2P network.
///
/// The message is expected to have passed the validation pipeline, but this
/// handler still enforces committee authority and previous_cid continuity
/// before mutating local state.
///
/// Applies the taxonomy changes to local skill tables if the version
/// is newer than what we have.
pub fn handle_taxonomy_message(
    db: &Database,
    message: &SignedGossipMessage,
) -> Result<TaxonomyUpdate, String> {
    let update: TaxonomyUpdate = serde_json::from_slice(&message.payload)
        .map_err(|e| format!("invalid taxonomy update: {e}"))?;

    // Validate required fields
    if update.cid.is_empty() {
        return Err("taxonomy update missing cid".into());
    }
    if update.version < 1 {
        return Err("taxonomy update version must be >= 1".into());
    }
    if !is_committee_member(db, &message.stake_address) {
        return Err("taxonomy update signer is not a committee member".into());
    }
    if !update
        .ratified_by
        .iter()
        .any(|addr| addr == &message.stake_address)
    {
        return Err("taxonomy update signer is not listed in ratified_by".into());
    }

    // Check local version — only apply if newer
    let (local_version, current_cid): (i64, Option<String>) = db
        .conn()
        .query_row(
            "SELECT COALESCE(MAX(version), 0), \
             (SELECT cid FROM taxonomy_versions ORDER BY version DESC LIMIT 1) \
             FROM taxonomy_versions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or((0, None));

    if update.version <= local_version {
        log::debug!(
            "Taxonomy: skipping v{} — local version is {}",
            update.version,
            local_version,
        );
        return Ok(update);
    }
    if local_version == 0 {
        if update.previous_cid.is_some() {
            return Err("taxonomy update has previous_cid but no local taxonomy exists".into());
        }
    } else if update.previous_cid.as_deref() != current_cid.as_deref() {
        return Err("taxonomy update previous_cid does not match local head".into());
    }

    // Apply changes to local skill tables
    apply_taxonomy_changes(db, &update)?;

    // Record the taxonomy version
    let ratified_by_json = serde_json::to_string(&update.ratified_by).unwrap_or_default();
    let ratified_at = chrono::DateTime::from_timestamp(update.ratified_at, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default();
    let signature_hex = hex::encode(&message.signature);

    db.conn()
        .execute(
            "INSERT OR REPLACE INTO taxonomy_versions \
             (version, cid, previous_cid, ratified_by, ratified_at, signature) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                update.version,
                update.cid,
                update.previous_cid,
                ratified_by_json,
                ratified_at,
                signature_hex,
            ],
        )
        .map_err(|e| format!("failed to record taxonomy version: {e}"))?;

    // Record in sync_log
    db.conn()
        .execute(
            "INSERT INTO sync_log (entity_type, entity_id, direction, peer_id, signature) \
             VALUES ('taxonomy', ?1, 'received', ?2, ?3)",
            params![
                format!("v{}", update.version),
                message.stake_address,
                signature_hex,
            ],
        )
        .map_err(|e| format!("failed to record sync_log: {e}"))?;

    log::info!(
        "Taxonomy: applied v{} ({} fields, {} subjects, {} skills)",
        update.version,
        update.changes.subject_fields.len(),
        update.changes.subjects.len(),
        update.changes.skills.len(),
    );

    Ok(update)
}

/// Apply taxonomy changes to local skill tables.
///
/// Uses INSERT OR REPLACE for upsert semantics — new items are inserted,
/// existing items are updated.
fn apply_taxonomy_changes(db: &Database, update: &TaxonomyUpdate) -> Result<(), String> {
    // Apply subject fields
    for sf in &update.changes.subject_fields {
        db.conn()
            .execute(
                "INSERT INTO subject_fields (id, name, description, updated_at) \
                 VALUES (?1, ?2, ?3, datetime('now')) \
                 ON CONFLICT(id) DO UPDATE SET \
                 name = excluded.name, description = excluded.description, \
                 updated_at = datetime('now')",
                params![sf.id, sf.name, sf.description],
            )
            .map_err(|e| format!("failed to upsert subject field '{}': {e}", sf.id))?;
    }

    // Apply subjects
    for subj in &update.changes.subjects {
        db.conn()
            .execute(
                "INSERT INTO subjects (id, name, description, subject_field_id, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, datetime('now')) \
                 ON CONFLICT(id) DO UPDATE SET \
                 name = excluded.name, description = excluded.description, \
                 subject_field_id = excluded.subject_field_id, updated_at = datetime('now')",
                params![subj.id, subj.name, subj.description, subj.subject_field_id],
            )
            .map_err(|e| format!("failed to upsert subject '{}': {e}", subj.id))?;
    }

    // Apply skills
    for skill in &update.changes.skills {
        db.conn()
            .execute(
                "INSERT INTO skills (id, name, description, subject_id, bloom_level, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, datetime('now')) \
                 ON CONFLICT(id) DO UPDATE SET \
                 name = excluded.name, description = excluded.description, \
                 subject_id = excluded.subject_id, bloom_level = excluded.bloom_level, \
                 updated_at = datetime('now')",
                params![
                    skill.id,
                    skill.name,
                    skill.description,
                    skill.subject_id,
                    skill.bloom_level
                ],
            )
            .map_err(|e| format!("failed to upsert skill '{}': {e}", skill.id))?;
    }

    // Apply new prerequisite edges
    for (skill_id, prereq_id) in &update.changes.prerequisites {
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO skill_prerequisites (skill_id, prerequisite_id) \
                 VALUES (?1, ?2)",
                params![skill_id, prereq_id],
            )
            .map_err(|e| format!("failed to add prerequisite {skill_id}->{prereq_id}: {e}"))?;
    }

    // Remove prerequisite edges
    for (skill_id, prereq_id) in &update.changes.removed_prerequisites {
        db.conn()
            .execute(
                "DELETE FROM skill_prerequisites \
                 WHERE skill_id = ?1 AND prerequisite_id = ?2",
                params![skill_id, prereq_id],
            )
            .map_err(|e| format!("failed to remove prerequisite {skill_id}->{prereq_id}: {e}"))?;
    }

    Ok(())
}

/// Check if a stake address is a DAO committee member.
///
/// Used by the validation pipeline for authority checks on taxonomy
/// messages. Returns `true` if the address is a committee member of
/// any active DAO.
pub fn is_committee_member(db: &Database, stake_address: &str) -> bool {
    db.conn()
        .query_row(
            "SELECT COUNT(*) > 0 FROM governance_dao_members m \
             JOIN governance_daos d ON d.id = m.dao_id \
             WHERE m.stake_address = ?1 AND m.role IN ('committee', 'chair') \
             AND d.status = 'active'",
            params![stake_address],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::taxonomy::{
        TaxonomyChanges, TaxonomySkill, TaxonomySubject, TaxonomySubjectField,
    };

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn sample_update() -> TaxonomyUpdate {
        TaxonomyUpdate {
            version: 1,
            cid: "blake3_taxonomy_v1".into(),
            previous_cid: None,
            ratified_by: vec!["stake_test1committee".into()],
            ratified_at: 1_700_000_000,
            changes: TaxonomyChanges {
                subject_fields: vec![TaxonomySubjectField {
                    id: "cs".into(),
                    name: "Computer Science".into(),
                    description: Some("The study of computation".into()),
                }],
                subjects: vec![TaxonomySubject {
                    id: "algorithms".into(),
                    name: "Algorithms".into(),
                    description: None,
                    subject_field_id: "cs".into(),
                }],
                skills: vec![TaxonomySkill {
                    id: "graph_traversal".into(),
                    name: "Graph Traversal".into(),
                    description: Some("BFS, DFS, Dijkstra".into()),
                    subject_id: "algorithms".into(),
                    bloom_level: "apply".into(),
                }],
                prerequisites: vec![],
                removed_prerequisites: vec![],
            },
        }
    }

    fn sample_message(update: &TaxonomyUpdate) -> SignedGossipMessage {
        let payload = serde_json::to_vec(update).unwrap();
        SignedGossipMessage {
            topic: "/alexandria/taxonomy/1.0".into(),
            payload,
            signature: vec![0xDE, 0xAD],
            public_key: vec![0; 32],
            stake_address: "stake_test1committee".into(),
            timestamp: 1_700_000_000,
        }
    }

    fn seed_committee(db: &Database) {
        db.conn()
            .execute(
                "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'Test Field')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
                 VALUES ('dao1', 'Test DAO', 'subject_field', 'sf1', 'active')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES ('dao1', 'stake_test1committee', 'committee')",
                [],
            )
            .unwrap();
    }

    #[test]
    fn handle_taxonomy_applies_changes() {
        let db = test_db();
        seed_committee(&db);
        let update = sample_update();
        let msg = sample_message(&update);

        let result = handle_taxonomy_message(&db, &msg);
        assert!(result.is_ok());

        // Verify subject field was created
        let name: String = db
            .conn()
            .query_row(
                "SELECT name FROM subject_fields WHERE id = 'cs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "Computer Science");

        // Verify skill was created
        let skill_name: String = db
            .conn()
            .query_row(
                "SELECT name FROM skills WHERE id = 'graph_traversal'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(skill_name, "Graph Traversal");

        // Verify taxonomy version recorded
        let version: i64 = db
            .conn()
            .query_row("SELECT MAX(version) FROM taxonomy_versions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn handle_taxonomy_skips_older_version() {
        let db = test_db();
        seed_committee(&db);

        // Apply v1 first
        let update = sample_update();
        let msg = sample_message(&update);
        handle_taxonomy_message(&db, &msg).unwrap();

        // Try to apply v1 again — should skip
        let result = handle_taxonomy_message(&db, &msg);
        assert!(result.is_ok());

        // Only one version record
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM taxonomy_versions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn handle_taxonomy_applies_v2_after_v1() {
        let db = test_db();
        seed_committee(&db);

        // Apply v1
        let v1 = sample_update();
        handle_taxonomy_message(&db, &sample_message(&v1)).unwrap();

        // Apply v2 with a new skill
        let v2 = TaxonomyUpdate {
            version: 2,
            cid: "blake3_taxonomy_v2".into(),
            previous_cid: Some("blake3_taxonomy_v1".into()),
            ratified_by: vec!["stake_test1committee".into()],
            ratified_at: 1_700_100_000,
            changes: TaxonomyChanges {
                subject_fields: vec![],
                subjects: vec![],
                skills: vec![TaxonomySkill {
                    id: "dynamic_programming".into(),
                    name: "Dynamic Programming".into(),
                    description: None,
                    subject_id: "algorithms".into(),
                    bloom_level: "analyze".into(),
                }],
                prerequisites: vec![("dynamic_programming".into(), "graph_traversal".into())],
                removed_prerequisites: vec![],
            },
        };
        handle_taxonomy_message(&db, &sample_message(&v2)).unwrap();

        // Both skills should exist
        let skill_count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM skills", [], |row| row.get(0))
            .unwrap();
        assert_eq!(skill_count, 2);

        // Prerequisite edge should exist
        let prereq_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM skill_prerequisites WHERE skill_id = 'dynamic_programming'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(prereq_count, 1);
    }

    #[test]
    fn handle_taxonomy_rejects_empty_cid() {
        let db = test_db();
        let mut update = sample_update();
        update.cid = String::new();
        let msg = sample_message(&update);

        assert!(handle_taxonomy_message(&db, &msg).is_err());
    }

    #[test]
    fn handle_taxonomy_rejects_non_committee_sender() {
        let db = test_db();
        let msg = sample_message(&sample_update());

        assert!(handle_taxonomy_message(&db, &msg).is_err());
    }

    #[test]
    fn handle_taxonomy_rejects_wrong_previous_cid() {
        let db = test_db();
        seed_committee(&db);
        db.conn()
            .execute(
                "INSERT INTO taxonomy_versions (version, cid) VALUES (1, 'existing_head')",
                [],
            )
            .unwrap();

        let mut update = sample_update();
        update.version = 2;
        update.previous_cid = Some("wrong_head".into());
        let msg = sample_message(&update);

        assert!(handle_taxonomy_message(&db, &msg).is_err());
    }

    #[test]
    fn is_committee_member_returns_false_when_empty() {
        let db = test_db();
        assert!(!is_committee_member(&db, "stake_test1nobody"));
    }

    #[test]
    fn is_committee_member_returns_true_for_committee() {
        let db = test_db();
        seed_committee(&db);

        assert!(is_committee_member(&db, "stake_test1committee"));
        assert!(!is_committee_member(&db, "stake_test1regular"));
    }
}
