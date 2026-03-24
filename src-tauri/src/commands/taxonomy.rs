//! IPC commands for taxonomy browsing and DAO ratification.
//!
//! Read commands for the skill taxonomy:
//!   - Browse subject fields, subjects, skills
//!   - Query prerequisites and relations
//!
//! Write commands for the taxonomy ratification workflow:
//!   - Propose a taxonomy change via governance
//!   - Preview what a change would affect
//!   - Publish a ratified taxonomy version
//!   - Query taxonomy versions

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::domain::taxonomy::{
    ProposeTaxonomyParams, TaxonomyPreview, TaxonomyPublishResult, TaxonomyVersion,
};
use crate::evidence::taxonomy;
use crate::AppState;

const BOOTSTRAP_PUBLIC_TAXONOMY_JSON: &str =
    include_str!("../../../bootstrap/public_taxonomy.json");

#[derive(Debug, Deserialize)]
struct BootstrapTaxonomyPayload {
    subject_fields: Vec<BootstrapSubjectField>,
    subjects: Vec<BootstrapSubject>,
    skills: Vec<BootstrapSkill>,
    skill_prerequisites: Vec<BootstrapSkillPrerequisite>,
    skill_relations: Vec<BootstrapSkillRelation>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSubjectField {
    id: String,
    name: String,
    description: Option<String>,
    icon_emoji: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSubject {
    id: String,
    name: String,
    description: Option<String>,
    subject_field_id: String,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSkill {
    id: String,
    name: String,
    description: Option<String>,
    subject_id: String,
    bloom_level: String,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSkillPrerequisite {
    skill_id: String,
    prerequisite_id: String,
}

#[derive(Debug, Deserialize)]
struct BootstrapSkillRelation {
    skill_id: String,
    related_skill_id: String,
    relation_type: String,
}

// ============================================================================
// Read-only taxonomy types (returned to frontend)
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct SubjectFieldInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_emoji: Option<String>,
    pub subject_count: i64,
    pub skill_count: i64,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubjectInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub subject_field_id: Option<String>,
    pub subject_field_name: Option<String>,
    pub skill_count: i64,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub subject_id: Option<String>,
    pub subject_name: Option<String>,
    pub subject_field_id: Option<String>,
    pub subject_field_name: Option<String>,
    pub bloom_level: String,
    pub prerequisite_count: i64,
    pub dependent_count: i64,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillDetail {
    pub skill: SkillInfo,
    pub prerequisites: Vec<SkillSummary>,
    pub dependents: Vec<SkillSummary>,
    pub related: Vec<SkillRelation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub bloom_level: String,
    pub subject_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillRelation {
    pub skill_id: String,
    pub skill_name: String,
    pub bloom_level: String,
    pub relation_type: String,
}

// ============================================================================
// Taxonomy read commands
// ============================================================================

/// Bootstrap bundled taxonomy tables for fresh installs.
///
/// This only writes when the local taxonomy is empty.
#[tauri::command]
pub async fn bootstrap_public_taxonomy(state: State<'_, AppState>) -> Result<i64, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    let existing_skills: i64 = conn
        .query_row("SELECT COUNT(*) FROM skills", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    if existing_skills > 0 {
        return Ok(0);
    }

    let payload: BootstrapTaxonomyPayload =
        serde_json::from_str(BOOTSTRAP_PUBLIC_TAXONOMY_JSON).map_err(|e| e.to_string())?;

    for f in &payload.subject_fields {
        conn.execute(
            "INSERT OR REPLACE INTO subject_fields
             (id, name, description, icon_emoji, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, COALESCE(?5, datetime('now')), COALESCE(?6, datetime('now')))",
            params![
                f.id,
                f.name,
                f.description,
                f.icon_emoji,
                f.created_at,
                f.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for s in &payload.subjects {
        conn.execute(
            "INSERT OR REPLACE INTO subjects
             (id, name, description, subject_field_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, COALESCE(?5, datetime('now')), COALESCE(?6, datetime('now')))",
            params![
                s.id,
                s.name,
                s.description,
                s.subject_field_id,
                s.created_at,
                s.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for sk in &payload.skills {
        conn.execute(
            "INSERT OR REPLACE INTO skills
             (id, name, description, subject_id, bloom_level, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, COALESCE(?6, datetime('now')), COALESCE(?7, datetime('now')))",
            params![
                sk.id,
                sk.name,
                sk.description,
                sk.subject_id,
                sk.bloom_level,
                sk.created_at,
                sk.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for edge in &payload.skill_prerequisites {
        conn.execute(
            "INSERT OR IGNORE INTO skill_prerequisites (skill_id, prerequisite_id)
             VALUES (?1, ?2)",
            params![edge.skill_id, edge.prerequisite_id],
        )
        .map_err(|e| e.to_string())?;
    }

    for rel in &payload.skill_relations {
        conn.execute(
            "INSERT OR IGNORE INTO skill_relations (skill_id, related_skill_id, relation_type)
             VALUES (?1, ?2, ?3)",
            params![rel.skill_id, rel.related_skill_id, rel.relation_type],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(payload.skills.len() as i64)
}

/// List all subject fields with aggregate counts.
#[tauri::command]
pub async fn list_subject_fields(
    state: State<'_, AppState>,
) -> Result<Vec<SubjectFieldInfo>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT sf.id, sf.name, sf.description, sf.icon_emoji, sf.created_at,
                    (SELECT COUNT(*) FROM subjects s WHERE s.subject_field_id = sf.id) as subject_count,
                    (SELECT COUNT(*) FROM skills sk
                     JOIN subjects s ON sk.subject_id = s.id
                     WHERE s.subject_field_id = sf.id) as skill_count
             FROM subject_fields sf
             ORDER BY sf.name",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(SubjectFieldInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                icon_emoji: row.get(3)?,
                created_at: row.get(4)?,
                subject_count: row.get(5)?,
                skill_count: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(rows)
}

/// List subjects, optionally filtered by subject_field_id.
#[tauri::command]
pub async fn list_subjects(
    state: State<'_, AppState>,
    subject_field_id: Option<String>,
) -> Result<Vec<SubjectInfo>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    let sql = if subject_field_id.is_some() {
        "SELECT s.id, s.name, s.description, s.subject_field_id, sf.name, s.created_at,
                (SELECT COUNT(*) FROM skills sk WHERE sk.subject_id = s.id) as skill_count
         FROM subjects s
         LEFT JOIN subject_fields sf ON s.subject_field_id = sf.id
         WHERE s.subject_field_id = ?1
         ORDER BY s.name"
    } else {
        "SELECT s.id, s.name, s.description, s.subject_field_id, sf.name, s.created_at,
                (SELECT COUNT(*) FROM skills sk WHERE sk.subject_id = s.id) as skill_count
         FROM subjects s
         LEFT JOIN subject_fields sf ON s.subject_field_id = sf.id
         ORDER BY s.name"
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let rows = if let Some(ref field_id) = subject_field_id {
        stmt.query_map(params![field_id], |row| {
            Ok(SubjectInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                subject_field_id: row.get(3)?,
                subject_field_name: row.get(4)?,
                created_at: row.get(5)?,
                skill_count: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    } else {
        stmt.query_map([], |row| {
            Ok(SubjectInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                subject_field_id: row.get(3)?,
                subject_field_name: row.get(4)?,
                created_at: row.get(5)?,
                skill_count: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    };

    Ok(rows)
}

/// List skills, optionally filtered by subject_id or search query.
#[tauri::command]
pub async fn list_skills(
    state: State<'_, AppState>,
    subject_id: Option<String>,
    search: Option<String>,
    bloom_level: Option<String>,
) -> Result<Vec<SkillInfo>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    // Build dynamic query
    let mut conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref sid) = subject_id {
        conditions.push(format!("sk.subject_id = ?{param_idx}"));
        param_values.push(Box::new(sid.clone()));
        param_idx += 1;
    }

    if let Some(ref q) = search {
        conditions.push(format!(
            "(sk.name LIKE ?{param_idx} OR sk.description LIKE ?{param_idx})"
        ));
        param_values.push(Box::new(format!("%{q}%")));
        param_idx += 1;
    }

    if let Some(ref bl) = bloom_level {
        conditions.push(format!("sk.bloom_level = ?{param_idx}"));
        param_values.push(Box::new(bl.clone()));
        // param_idx not needed after last use
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT sk.id, sk.name, sk.description, sk.subject_id, s.name, sf.id, sf.name,
                sk.bloom_level, sk.created_at,
                (SELECT COUNT(*) FROM skill_prerequisites sp WHERE sp.skill_id = sk.id) as prereq_count,
                (SELECT COUNT(*) FROM skill_prerequisites sp WHERE sp.prerequisite_id = sk.id) as dep_count
         FROM skills sk
         LEFT JOIN subjects s ON sk.subject_id = s.id
         LEFT JOIN subject_fields sf ON s.subject_field_id = sf.id
         {where_clause}
         ORDER BY sk.name"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_slice: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let rows = stmt
        .query_map(params_slice.as_slice(), |row| {
            Ok(SkillInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                subject_id: row.get(3)?,
                subject_name: row.get(4)?,
                subject_field_id: row.get(5)?,
                subject_field_name: row.get(6)?,
                bloom_level: row.get(7)?,
                created_at: row.get(8)?,
                prerequisite_count: row.get(9)?,
                dependent_count: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(rows)
}

/// Get full detail for a single skill, including prerequisites and relations.
#[tauri::command]
pub async fn get_skill(
    state: State<'_, AppState>,
    skill_id: String,
) -> Result<SkillDetail, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    // Main skill info
    let skill = conn
        .query_row(
            "SELECT sk.id, sk.name, sk.description, sk.subject_id, s.name, sf.id, sf.name,
                    sk.bloom_level, sk.created_at,
                    (SELECT COUNT(*) FROM skill_prerequisites sp WHERE sp.skill_id = sk.id),
                    (SELECT COUNT(*) FROM skill_prerequisites sp WHERE sp.prerequisite_id = sk.id)
             FROM skills sk
             LEFT JOIN subjects s ON sk.subject_id = s.id
             LEFT JOIN subject_fields sf ON s.subject_field_id = sf.id
             WHERE sk.id = ?1",
            params![skill_id],
            |row| {
                Ok(SkillInfo {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    subject_id: row.get(3)?,
                    subject_name: row.get(4)?,
                    subject_field_id: row.get(5)?,
                    subject_field_name: row.get(6)?,
                    bloom_level: row.get(7)?,
                    created_at: row.get(8)?,
                    prerequisite_count: row.get(9)?,
                    dependent_count: row.get(10)?,
                })
            },
        )
        .map_err(|e| format!("skill not found: {e}"))?;

    // Prerequisites (skills this skill depends on)
    let mut prereq_stmt = conn
        .prepare(
            "SELECT sk.id, sk.name, sk.bloom_level, s.name
             FROM skill_prerequisites sp
             JOIN skills sk ON sp.prerequisite_id = sk.id
             LEFT JOIN subjects s ON sk.subject_id = s.id
             WHERE sp.skill_id = ?1
             ORDER BY sk.name",
        )
        .map_err(|e| e.to_string())?;

    let prerequisites = prereq_stmt
        .query_map(params![skill_id], |row| {
            Ok(SkillSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                bloom_level: row.get(2)?,
                subject_name: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    // Dependents (skills that depend on this skill)
    let mut dep_stmt = conn
        .prepare(
            "SELECT sk.id, sk.name, sk.bloom_level, s.name
             FROM skill_prerequisites sp
             JOIN skills sk ON sp.skill_id = sk.id
             LEFT JOIN subjects s ON sk.subject_id = s.id
             WHERE sp.prerequisite_id = ?1
             ORDER BY sk.name",
        )
        .map_err(|e| e.to_string())?;

    let dependents = dep_stmt
        .query_map(params![skill_id], |row| {
            Ok(SkillSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                bloom_level: row.get(2)?,
                subject_name: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    // Related skills
    let mut rel_stmt = conn
        .prepare(
            "SELECT sk.id, sk.name, sk.bloom_level, sr.relation_type
             FROM skill_relations sr
             JOIN skills sk ON sr.related_skill_id = sk.id
             WHERE sr.skill_id = ?1
             UNION
             SELECT sk.id, sk.name, sk.bloom_level, sr.relation_type
             FROM skill_relations sr
             JOIN skills sk ON sr.skill_id = sk.id
             WHERE sr.related_skill_id = ?1
             ORDER BY 2",
        )
        .map_err(|e| e.to_string())?;

    let related = rel_stmt
        .query_map(params![skill_id], |row| {
            Ok(SkillRelation {
                skill_id: row.get(0)?,
                skill_name: row.get(1)?,
                bloom_level: row.get(2)?,
                relation_type: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(SkillDetail {
        skill,
        prerequisites,
        dependents,
        related,
    })
}

/// Get all prerequisite edges for building the skill graph.
///
/// Returns all (skill_id, prerequisite_id) pairs with names for rendering.
#[tauri::command]
pub async fn list_skill_graph_edges(
    state: State<'_, AppState>,
) -> Result<Vec<SkillGraphEdge>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT sp.skill_id, sk1.name, sk1.bloom_level,
                    sp.prerequisite_id, sk2.name, sk2.bloom_level
             FROM skill_prerequisites sp
             JOIN skills sk1 ON sp.skill_id = sk1.id
             JOIN skills sk2 ON sp.prerequisite_id = sk2.id
             ORDER BY sk2.name, sk1.name",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(SkillGraphEdge {
                skill_id: row.get(0)?,
                skill_name: row.get(1)?,
                skill_bloom: row.get(2)?,
                prerequisite_id: row.get(3)?,
                prerequisite_name: row.get(4)?,
                prerequisite_bloom: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(rows)
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillGraphEdge {
    pub skill_id: String,
    pub skill_name: String,
    pub skill_bloom: String,
    pub prerequisite_id: String,
    pub prerequisite_name: String,
    pub prerequisite_bloom: String,
}

// ============================================================================
// Element skill tag commands
// ============================================================================

/// Tag an element with a skill (for the evidence pipeline).
#[tauri::command]
pub async fn tag_element_skill(
    state: State<'_, AppState>,
    element_id: String,
    skill_id: String,
    weight: Option<f64>,
) -> Result<(), String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db.conn()
        .execute(
            "INSERT OR REPLACE INTO element_skill_tags (element_id, skill_id, weight)
             VALUES (?1, ?2, ?3)",
            params![element_id, skill_id, weight.unwrap_or(1.0)],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Remove a skill tag from an element.
#[tauri::command]
pub async fn untag_element_skill(
    state: State<'_, AppState>,
    element_id: String,
    skill_id: String,
) -> Result<(), String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    db.conn()
        .execute(
            "DELETE FROM element_skill_tags WHERE element_id = ?1 AND skill_id = ?2",
            params![element_id, skill_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// List skill tags for an element.
#[tauri::command]
pub async fn list_element_skill_tags(
    state: State<'_, AppState>,
    element_id: String,
) -> Result<Vec<ElementSkillTag>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT est.skill_id, sk.name, sk.bloom_level, est.weight
             FROM element_skill_tags est
             JOIN skills sk ON est.skill_id = sk.id
             WHERE est.element_id = ?1
             ORDER BY sk.name",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![element_id], |row| {
            Ok(ElementSkillTag {
                skill_id: row.get(0)?,
                skill_name: row.get(1)?,
                bloom_level: row.get(2)?,
                weight: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(rows)
}

#[derive(Debug, Clone, Serialize)]
pub struct ElementSkillTag {
    pub skill_id: String,
    pub skill_name: String,
    pub bloom_level: String,
    pub weight: f64,
}

/// Propose a taxonomy change via a governance proposal.
///
/// Creates a draft proposal with category 'taxonomy_change' under
/// the specified DAO. The changes are stored as JSON and will be
/// applied when the proposal is approved and published.
#[tauri::command]
pub async fn propose_taxonomy_change(
    state: State<'_, AppState>,
    params: ProposeTaxonomyParams,
) -> Result<String, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let conn = db.conn();

    // Get proposer address from local identity
    let proposer: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no local identity: {e}"))?;

    // Validate changes first
    let warnings = taxonomy::validate_changes(conn, &params.changes)?;
    if !warnings.is_empty() {
        log::warn!("taxonomy proposal warnings: {:?}", warnings);
    }

    taxonomy::propose_taxonomy_change(
        conn,
        &params.dao_id,
        &params.title,
        params.description.as_deref(),
        &params.changes,
        &proposer,
    )
}

/// Preview what a taxonomy change would affect.
///
/// Returns counts of affected items and lists of new vs modified skill IDs.
#[tauri::command]
pub async fn preview_taxonomy_change(
    state: State<'_, AppState>,
    params: ProposeTaxonomyParams,
) -> Result<TaxonomyPreview, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    taxonomy::preview_taxonomy_change(db.conn(), &params.changes)
}

/// Publish a ratified taxonomy version.
///
/// Called after a taxonomy_change proposal is approved by the DAO.
/// Applies changes to local skill tables, records the version,
/// and prepares the taxonomy document for IPFS storage.
#[tauri::command]
pub async fn publish_taxonomy_ratification(
    state: State<'_, AppState>,
    proposal_id: String,
    ratified_by: Vec<String>,
    signature: String,
) -> Result<TaxonomyPublishResult, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    taxonomy::publish_taxonomy_ratification(db.conn(), &proposal_id, &ratified_by, &signature)
}

/// Get the current (latest) taxonomy version.
#[tauri::command]
pub async fn get_taxonomy_version(
    state: State<'_, AppState>,
) -> Result<Option<TaxonomyVersion>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    taxonomy::get_current_version(db.conn())
}

/// List all taxonomy versions (most recent first).
#[tauri::command]
pub async fn list_taxonomy_versions(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<TaxonomyVersion>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    taxonomy::list_versions(db.conn(), limit.unwrap_or(50))
}

/// Validate a set of taxonomy changes.
///
/// Returns a list of warnings (empty = all valid).
#[tauri::command]
pub async fn validate_taxonomy_changes(
    state: State<'_, AppState>,
    params: ProposeTaxonomyParams,
) -> Result<Vec<String>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    taxonomy::validate_changes(db.conn(), &params.changes)
}
