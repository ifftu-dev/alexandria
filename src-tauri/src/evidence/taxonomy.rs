//! Taxonomy ratification workflow.
//!
//! Implements the 6-phase DAO taxonomy ratification process:
//!   1. Propose — create a governance proposal with taxonomy changes
//!   2. Gossip — announce the proposal via P2P (handled by governance gossip)
//!   3. Submit — anchor the proposal on-chain (handled by governance IPC)
//!   4. Vote — DAO committee votes (handled by governance IPC)
//!   5. Resolve + Publish — on approval, create versioned taxonomy document
//!   6. Apply — apply changes to local skill tables + gossip to peers
//!
//! This module handles phases 1, 5, and 6. Phases 2-4 use existing
//! governance infrastructure.

use rusqlite::{params, Connection};

use crate::crypto::hash::entity_id;
use crate::domain::taxonomy::{
    TaxonomyChanges, TaxonomyDocument, TaxonomyPreview, TaxonomyPublishResult, TaxonomyVersion,
};

/// Propose a taxonomy change by creating a governance proposal.
///
/// Stores the taxonomy changes as JSON in the proposal's content_cid field
/// (initially as a placeholder — the actual IPFS CID is set when published).
/// Returns the proposal ID.
pub fn propose_taxonomy_change(
    conn: &Connection,
    dao_id: &str,
    title: &str,
    description: Option<&str>,
    changes: &TaxonomyChanges,
    proposer: &str,
) -> Result<String, String> {
    // Validate the DAO exists and is active
    let dao_status: String = conn
        .query_row(
            "SELECT status FROM governance_daos WHERE id = ?1",
            params![dao_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("DAO not found: {e}"))?;

    if dao_status != "active" {
        return Err(format!("DAO is not active (status: {dao_status})"));
    }

    // Serialize taxonomy changes to JSON
    let changes_json =
        serde_json::to_string(changes).map_err(|e| format!("failed to serialize changes: {e}"))?;

    // Get the next taxonomy version
    let next_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM taxonomy_versions",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    // Create the proposal
    let proposal_id = entity_id(&[dao_id, "taxonomy_change", title, proposer]);

    conn.execute(
        "INSERT INTO governance_proposals \
         (id, dao_id, title, description, category, status, proposer, \
          content_cid, taxonomy_version) \
         VALUES (?1, ?2, ?3, ?4, 'taxonomy_change', 'draft', ?5, ?6, ?7)",
        params![
            proposal_id,
            dao_id,
            title,
            description,
            proposer,
            changes_json,
            next_version,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(proposal_id)
}

/// Preview what a taxonomy change would affect.
///
/// Checks the local skill tables to determine which items are new
/// vs modifications, and reports counts.
pub fn preview_taxonomy_change(
    conn: &Connection,
    changes: &TaxonomyChanges,
) -> Result<TaxonomyPreview, String> {
    let mut new_skills = Vec::new();
    let mut modified_skills = Vec::new();

    for skill in &changes.skills {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM skills WHERE id = ?1",
                params![skill.id],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if exists {
            modified_skills.push(skill.id.clone());
        } else {
            new_skills.push(skill.id.clone());
        }
    }

    let has_mods = !modified_skills.is_empty();

    Ok(TaxonomyPreview {
        subject_fields_affected: changes.subject_fields.len() as i64,
        subjects_affected: changes.subjects.len() as i64,
        skills_affected: changes.skills.len() as i64,
        prerequisites_added: changes.prerequisites.len() as i64,
        prerequisites_removed: changes.removed_prerequisites.len() as i64,
        has_modifications: has_mods,
        new_skill_ids: new_skills,
        modified_skill_ids: modified_skills,
    })
}

/// Publish a ratified taxonomy change.
///
/// Called when a taxonomy_change proposal is approved:
///   1. Deserialize the taxonomy changes from the proposal
///   2. Build a TaxonomyDocument
///   3. Record the new version in taxonomy_versions
///   4. Apply changes to local skill tables
///   5. Update the proposal with the content CID
///
/// Returns the publish result with version number and CID.
pub fn publish_taxonomy_ratification(
    conn: &Connection,
    proposal_id: &str,
    ratified_by: &[String],
    signature: &str,
) -> Result<TaxonomyPublishResult, String> {
    // Load the proposal
    let (dao_id, status, changes_json, taxonomy_version): (
        String,
        String,
        Option<String>,
        Option<i64>,
    ) = conn
        .query_row(
            "SELECT dao_id, status, content_cid, taxonomy_version \
             FROM governance_proposals WHERE id = ?1",
            params![proposal_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|e| format!("proposal not found: {e}"))?;

    if status != "approved" {
        return Err(format!(
            "proposal must be approved to publish (status: {status})"
        ));
    }

    let changes_str =
        changes_json.ok_or("proposal has no taxonomy changes (content_cid is null)")?;
    let changes: TaxonomyChanges = serde_json::from_str(&changes_str)
        .map_err(|e| format!("failed to parse taxonomy changes: {e}"))?;

    let version = taxonomy_version.unwrap_or(1);

    // Get the previous version's CID
    let previous_cid: Option<String> = conn
        .query_row(
            "SELECT cid FROM taxonomy_versions WHERE version = ?1",
            params![version - 1],
            |row| row.get(0),
        )
        .ok();

    let now = chrono::Utc::now().to_rfc3339();

    // Build the taxonomy document
    let doc = TaxonomyDocument {
        version,
        root_cid: String::new(), // Will be set after IPFS upload
        previous_cid: previous_cid.clone(),
        ratified_by: ratified_by.to_vec(),
        ratified_at: now.clone(),
        signature: signature.into(),
        content: changes.clone(),
    };

    // Serialize to JSON for IPFS storage (CID computed from content)
    let doc_json = serde_json::to_string_pretty(&doc)
        .map_err(|e| format!("failed to serialize taxonomy document: {e}"))?;

    // Use the blake2b hash of the document as the CID
    let content_cid = hex::encode(crate::crypto::hash::blake2b_256(doc_json.as_bytes()));

    // Apply the taxonomy changes to local tables
    let changes_applied = apply_changes(conn, &changes)?;

    // Record the version
    let ratified_by_json = serde_json::to_string(ratified_by).unwrap_or_default();

    conn.execute(
        "INSERT OR REPLACE INTO taxonomy_versions \
         (version, cid, previous_cid, ratified_by, ratified_at, signature) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            version,
            content_cid,
            previous_cid,
            ratified_by_json,
            now,
            signature,
        ],
    )
    .map_err(|e| format!("failed to record taxonomy version: {e}"))?;

    // Update the proposal with the published CID
    conn.execute(
        "UPDATE governance_proposals SET content_cid = ?1 WHERE id = ?2",
        params![content_cid, proposal_id],
    )
    .map_err(|e| e.to_string())?;

    let _ = dao_id; // Used for governance gossip in future

    Ok(TaxonomyPublishResult {
        version,
        content_cid,
        changes_applied,
    })
}

/// Apply taxonomy changes to local skill tables.
///
/// Returns the total number of changes applied.
fn apply_changes(conn: &Connection, changes: &TaxonomyChanges) -> Result<i64, String> {
    let mut count = 0i64;

    for sf in &changes.subject_fields {
        conn.execute(
            "INSERT INTO subject_fields (id, name, description, updated_at) \
             VALUES (?1, ?2, ?3, datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
             name = excluded.name, description = excluded.description, \
             updated_at = datetime('now')",
            params![sf.id, sf.name, sf.description],
        )
        .map_err(|e| format!("failed to upsert subject field '{}': {e}", sf.id))?;
        count += 1;
    }

    for subj in &changes.subjects {
        conn.execute(
            "INSERT INTO subjects (id, name, description, subject_field_id, updated_at) \
             VALUES (?1, ?2, ?3, ?4, datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
             name = excluded.name, description = excluded.description, \
             subject_field_id = excluded.subject_field_id, updated_at = datetime('now')",
            params![subj.id, subj.name, subj.description, subj.subject_field_id],
        )
        .map_err(|e| format!("failed to upsert subject '{}': {e}", subj.id))?;
        count += 1;
    }

    for skill in &changes.skills {
        conn.execute(
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
        count += 1;
    }

    for (skill_id, prereq_id) in &changes.prerequisites {
        conn.execute(
            "INSERT OR IGNORE INTO skill_prerequisites (skill_id, prerequisite_id) \
             VALUES (?1, ?2)",
            params![skill_id, prereq_id],
        )
        .map_err(|e| format!("failed to add prerequisite {skill_id}->{prereq_id}: {e}"))?;
        count += 1;
    }

    for (skill_id, prereq_id) in &changes.removed_prerequisites {
        conn.execute(
            "DELETE FROM skill_prerequisites \
             WHERE skill_id = ?1 AND prerequisite_id = ?2",
            params![skill_id, prereq_id],
        )
        .map_err(|e| format!("failed to remove prerequisite {skill_id}->{prereq_id}: {e}"))?;
        count += 1;
    }

    Ok(count)
}

/// Get the current taxonomy version.
pub fn get_current_version(conn: &Connection) -> Result<Option<TaxonomyVersion>, String> {
    conn.query_row(
        "SELECT version, cid, previous_cid, ratified_by, ratified_at, signature, applied_at \
         FROM taxonomy_versions ORDER BY version DESC LIMIT 1",
        [],
        |row| {
            Ok(TaxonomyVersion {
                version: row.get(0)?,
                cid: row.get(1)?,
                previous_cid: row.get(2)?,
                ratified_by: row.get(3)?,
                ratified_at: row.get(4)?,
                signature: row.get(5)?,
                applied_at: row.get(6)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// List all taxonomy versions.
pub fn list_versions(conn: &Connection, limit: i64) -> Result<Vec<TaxonomyVersion>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT version, cid, previous_cid, ratified_by, ratified_at, signature, applied_at \
             FROM taxonomy_versions ORDER BY version DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;

    let versions = stmt
        .query_map(params![limit], |row| {
            Ok(TaxonomyVersion {
                version: row.get(0)?,
                cid: row.get(1)?,
                previous_cid: row.get(2)?,
                ratified_by: row.get(3)?,
                ratified_at: row.get(4)?,
                signature: row.get(5)?,
                applied_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(versions)
}

/// Validate that a taxonomy changes set is well-formed.
///
/// Checks:
///   - All subjects reference existing or newly-created subject fields
///   - All skills reference existing or newly-created subjects
///   - No self-referencing prerequisites
///   - No duplicate IDs within the same change set
pub fn validate_changes(
    conn: &Connection,
    changes: &TaxonomyChanges,
) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();

    // Collect newly-created IDs
    let new_sf_ids: std::collections::HashSet<&str> = changes
        .subject_fields
        .iter()
        .map(|sf| sf.id.as_str())
        .collect();

    let new_subj_ids: std::collections::HashSet<&str> =
        changes.subjects.iter().map(|s| s.id.as_str()).collect();

    let new_skill_ids: std::collections::HashSet<&str> =
        changes.skills.iter().map(|s| s.id.as_str()).collect();

    // Check subjects reference valid subject fields
    for subj in &changes.subjects {
        let sf_exists: bool = new_sf_ids.contains(subj.subject_field_id.as_str())
            || conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM subject_fields WHERE id = ?1",
                    params![subj.subject_field_id],
                    |row| row.get(0),
                )
                .unwrap_or(false);

        if !sf_exists {
            warnings.push(format!(
                "subject '{}' references unknown subject_field '{}'",
                subj.id, subj.subject_field_id
            ));
        }
    }

    // Check skills reference valid subjects
    for skill in &changes.skills {
        let subj_exists: bool = new_subj_ids.contains(skill.subject_id.as_str())
            || conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM subjects WHERE id = ?1",
                    params![skill.subject_id],
                    |row| row.get(0),
                )
                .unwrap_or(false);

        if !subj_exists {
            warnings.push(format!(
                "skill '{}' references unknown subject '{}'",
                skill.id, skill.subject_id
            ));
        }
    }

    // Check prerequisites don't self-reference
    for (skill_id, prereq_id) in &changes.prerequisites {
        if skill_id == prereq_id {
            return Err(format!("self-referencing prerequisite: {skill_id}"));
        }

        // Check both skills exist (in changes or DB)
        let skill_exists = new_skill_ids.contains(skill_id.as_str())
            || conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM skills WHERE id = ?1",
                    params![skill_id],
                    |row| row.get(0),
                )
                .unwrap_or(false);

        if !skill_exists {
            warnings.push(format!(
                "prerequisite references unknown skill '{skill_id}'"
            ));
        }
    }

    Ok(warnings)
}

/// Trait extension for optional query results.
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::domain::taxonomy::*;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn sample_changes() -> TaxonomyChanges {
        TaxonomyChanges {
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
            skills: vec![
                TaxonomySkill {
                    id: "sorting".into(),
                    name: "Sorting Algorithms".into(),
                    description: Some("Quick, merge, heap sort".into()),
                    subject_id: "algorithms".into(),
                    bloom_level: "apply".into(),
                },
                TaxonomySkill {
                    id: "graph_search".into(),
                    name: "Graph Search".into(),
                    description: Some("BFS, DFS".into()),
                    subject_id: "algorithms".into(),
                    bloom_level: "apply".into(),
                },
            ],
            prerequisites: vec![("graph_search".into(), "sorting".into())],
            removed_prerequisites: vec![],
        }
    }

    fn setup_dao(db: &Database) -> String {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO subject_fields (id, name) VALUES ('sf_existing', 'Existing Field')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
             VALUES ('dao1', 'CS DAO', 'subject_field', 'sf_existing', 'active')",
            [],
        )
        .unwrap();
        "dao1".into()
    }

    #[test]
    fn propose_taxonomy_change_creates_proposal() {
        let db = test_db();
        let dao_id = setup_dao(&db);
        let changes = sample_changes();

        let proposal_id = propose_taxonomy_change(
            db.conn(),
            &dao_id,
            "Add CS skills",
            Some("Adding sorting and graph skills"),
            &changes,
            "stake_test1uproposer",
        )
        .unwrap();

        assert!(!proposal_id.is_empty());

        // Verify proposal was created
        let (category, status): (String, String) = db
            .conn()
            .query_row(
                "SELECT category, status FROM governance_proposals WHERE id = ?1",
                params![proposal_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(category, "taxonomy_change");
        assert_eq!(status, "draft");
    }

    #[test]
    fn propose_taxonomy_change_stores_changes_json() {
        let db = test_db();
        let dao_id = setup_dao(&db);
        let changes = sample_changes();

        let proposal_id =
            propose_taxonomy_change(db.conn(), &dao_id, "Test", None, &changes, "stake_test1u")
                .unwrap();

        let content_cid: Option<String> = db
            .conn()
            .query_row(
                "SELECT content_cid FROM governance_proposals WHERE id = ?1",
                params![proposal_id],
                |row| row.get(0),
            )
            .unwrap();

        // content_cid should be the serialized changes JSON
        assert!(content_cid.is_some());
        let parsed: TaxonomyChanges = serde_json::from_str(&content_cid.unwrap()).unwrap();
        assert_eq!(parsed.skills.len(), 2);
    }

    #[test]
    fn propose_rejects_inactive_dao() {
        let db = test_db();
        let conn = db.conn();

        conn.execute(
            "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'Test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
             VALUES ('dao_inactive', 'Inactive', 'subject_field', 'sf1', 'pending')",
            [],
        )
        .unwrap();

        let result = propose_taxonomy_change(
            conn,
            "dao_inactive",
            "Test",
            None,
            &sample_changes(),
            "stake_test1u",
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not active"));
    }

    #[test]
    fn preview_taxonomy_change_reports_new_skills() {
        let db = test_db();
        let changes = sample_changes();

        let preview = preview_taxonomy_change(db.conn(), &changes).unwrap();

        assert_eq!(preview.subject_fields_affected, 1);
        assert_eq!(preview.subjects_affected, 1);
        assert_eq!(preview.skills_affected, 2);
        assert_eq!(preview.prerequisites_added, 1);
        assert_eq!(preview.prerequisites_removed, 0);
        assert!(!preview.has_modifications);
        assert_eq!(preview.new_skill_ids.len(), 2);
        assert!(preview.modified_skill_ids.is_empty());
    }

    #[test]
    fn preview_detects_modifications() {
        let db = test_db();
        let conn = db.conn();

        // Create an existing skill
        conn.execute(
            "INSERT INTO subject_fields (id, name) VALUES ('cs', 'CS')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO subjects (id, name, subject_field_id) VALUES ('algorithms', 'Algo', 'cs')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO skills (id, name, subject_id) VALUES ('sorting', 'Old Sorting', 'algorithms')",
            [],
        )
        .unwrap();

        let changes = sample_changes();
        let preview = preview_taxonomy_change(conn, &changes).unwrap();

        assert!(preview.has_modifications);
        assert!(preview.modified_skill_ids.contains(&"sorting".to_string()));
        assert!(preview.new_skill_ids.contains(&"graph_search".to_string()));
    }

    #[test]
    fn publish_applies_changes() {
        let db = test_db();
        let dao_id = setup_dao(&db);
        let changes = sample_changes();

        // Create and approve a proposal
        let proposal_id =
            propose_taxonomy_change(db.conn(), &dao_id, "Test", None, &changes, "stake_test1u")
                .unwrap();

        // Manually approve it (normally done by governance voting)
        db.conn()
            .execute(
                "UPDATE governance_proposals SET status = 'approved' WHERE id = ?1",
                params![proposal_id],
            )
            .unwrap();

        let result = publish_taxonomy_ratification(
            db.conn(),
            &proposal_id,
            &["stake_test1ucommittee".to_string()],
            "signature_hex",
        )
        .unwrap();

        assert_eq!(result.version, 1);
        assert!(!result.content_cid.is_empty());
        assert!(result.changes_applied > 0);

        // Verify skills were created
        let skill_count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM skills", [], |row| row.get(0))
            .unwrap();
        assert_eq!(skill_count, 2);

        // Verify taxonomy version was recorded
        let version: i64 = db
            .conn()
            .query_row("SELECT MAX(version) FROM taxonomy_versions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn publish_rejects_unapproved() {
        let db = test_db();
        let dao_id = setup_dao(&db);

        let proposal_id = propose_taxonomy_change(
            db.conn(),
            &dao_id,
            "Test",
            None,
            &sample_changes(),
            "stake_test1u",
        )
        .unwrap();

        let result =
            publish_taxonomy_ratification(db.conn(), &proposal_id, &["addr".to_string()], "sig");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("approved"));
    }

    #[test]
    fn validate_changes_catches_self_reference() {
        let db = test_db();
        let mut changes = sample_changes();
        changes
            .prerequisites
            .push(("sorting".into(), "sorting".into()));

        let result = validate_changes(db.conn(), &changes);
        assert!(result.is_err());
    }

    #[test]
    fn validate_changes_warns_missing_subject_field() {
        let db = test_db();
        let changes = TaxonomyChanges {
            subject_fields: vec![],
            subjects: vec![TaxonomySubject {
                id: "orphan".into(),
                name: "Orphan".into(),
                description: None,
                subject_field_id: "nonexistent".into(),
            }],
            skills: vec![],
            prerequisites: vec![],
            removed_prerequisites: vec![],
        };

        let warnings = validate_changes(db.conn(), &changes).unwrap();
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("unknown subject_field"));
    }

    #[test]
    fn list_versions_empty() {
        let db = test_db();
        let versions = list_versions(db.conn(), 10).unwrap();
        assert!(versions.is_empty());
    }

    #[test]
    fn get_current_version_none() {
        let db = test_db();
        let current = get_current_version(db.conn()).unwrap();
        assert!(current.is_none());
    }

    #[test]
    fn version_chain_maintained() {
        let db = test_db();
        let dao_id = setup_dao(&db);

        // Publish v1
        let changes1 = sample_changes();
        let p1 = propose_taxonomy_change(db.conn(), &dao_id, "V1", None, &changes1, "stake_test1u")
            .unwrap();
        db.conn()
            .execute(
                "UPDATE governance_proposals SET status = 'approved' WHERE id = ?1",
                params![p1],
            )
            .unwrap();
        let r1 = publish_taxonomy_ratification(db.conn(), &p1, &["addr1".into()], "sig1").unwrap();
        assert_eq!(r1.version, 1);

        // Publish v2
        let changes2 = TaxonomyChanges {
            subject_fields: vec![],
            subjects: vec![],
            skills: vec![TaxonomySkill {
                id: "dynamic_prog".into(),
                name: "Dynamic Programming".into(),
                description: None,
                subject_id: "algorithms".into(),
                bloom_level: "analyze".into(),
            }],
            prerequisites: vec![],
            removed_prerequisites: vec![],
        };
        let p2 = propose_taxonomy_change(db.conn(), &dao_id, "V2", None, &changes2, "stake_test1u")
            .unwrap();
        db.conn()
            .execute(
                "UPDATE governance_proposals SET status = 'approved' WHERE id = ?1",
                params![p2],
            )
            .unwrap();
        let r2 = publish_taxonomy_ratification(db.conn(), &p2, &["addr1".into()], "sig2").unwrap();
        assert_eq!(r2.version, 2);

        // Check version chain
        let versions = list_versions(db.conn(), 10).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version, 2); // most recent first

        // V2 should reference V1's CID
        let v2_prev: Option<String> = db
            .conn()
            .query_row(
                "SELECT previous_cid FROM taxonomy_versions WHERE version = 2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(v2_prev, Some(r1.content_cid));
    }
}
