//! Enterprise sponsors + role/JD assessments (§ productization P2).
//!
//! An `Organization` (the sponsor) defines `RoleAssessment`s that map a
//! job description / role to a backing assessment, a per-role
//! `IssuancePolicy` (P0), and a required assurance level (P1). The
//! keystone `issue_role_credential` ties it together: completing the
//! backing assessment with a satisfying integrity session mints a gated
//! `RoleCredential` whose embedded integrity attestation proves how it
//! was earned.
//!
//! Thin `#[tauri::command]` handlers delegate to pure `*_impl` functions
//! taking `&Connection`, keeping the logic unit-testable.

use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::credentials::{
    issue_credential_impl, load_issuer_key, now_rfc3339, IssuancePolicy, IssueCredentialRequest,
};
use crate::crypto::did::Did;
use crate::crypto::hash::entity_id;
use crate::domain::vc::{Claim, CredentialType, RoleClaim, VerifiableCredential};
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: String,
    pub name: String,
    pub owner_address: String,
    pub did: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssessment {
    pub id: String,
    pub org_id: String,
    pub role_title: String,
    pub job_description: Option<String>,
    pub course_id: Option<String>,
    pub skill_ids: Vec<String>,
    pub issuance_policy: Option<IssuancePolicy>,
    pub required_assurance_level: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoleAssessmentRequest {
    pub org_id: String,
    pub role_title: String,
    #[serde(default)]
    pub job_description: Option<String>,
    #[serde(default)]
    pub course_id: Option<String>,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub issuance_policy: Option<IssuancePolicy>,
    #[serde(default)]
    pub required_assurance_level: Option<String>,
}

// ============================================================================
// Organizations
// ============================================================================

pub fn create_organization_impl(
    conn: &Connection,
    name: &str,
    owner_address: &str,
    did: Option<&str>,
    now: &str,
) -> Result<Organization, String> {
    if name.trim().is_empty() {
        return Err("organization name is required".into());
    }
    let id = entity_id(&[name, owner_address]);
    conn.execute(
        "INSERT INTO organizations (id, name, owner_address, did, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO NOTHING",
        params![id, name, owner_address, did, now],
    )
    .map_err(|e| e.to_string())?;
    get_organization_impl(conn, &id)?.ok_or_else(|| "failed to create organization".into())
}

pub fn get_organization_impl(conn: &Connection, id: &str) -> Result<Option<Organization>, String> {
    conn.query_row(
        "SELECT id, name, owner_address, did, created_at FROM organizations WHERE id = ?1",
        params![id],
        |r| {
            Ok(Organization {
                id: r.get(0)?,
                name: r.get(1)?,
                owner_address: r.get(2)?,
                did: r.get(3)?,
                created_at: r.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn list_organizations_impl(
    conn: &Connection,
    owner_address: Option<&str>,
) -> Result<Vec<Organization>, String> {
    let mut sql =
        String::from("SELECT id, name, owner_address, did, created_at FROM organizations");
    if owner_address.is_some() {
        sql.push_str(" WHERE owner_address = ?1");
    }
    sql.push_str(" ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let map = |r: &rusqlite::Row<'_>| {
        Ok(Organization {
            id: r.get(0)?,
            name: r.get(1)?,
            owner_address: r.get(2)?,
            did: r.get(3)?,
            created_at: r.get(4)?,
        })
    };
    let rows = if let Some(owner) = owner_address {
        stmt.query_map(params![owner], map)
    } else {
        stmt.query_map([], map)
    }
    .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

// ============================================================================
// Role assessments
// ============================================================================

pub fn create_role_assessment_impl(
    conn: &Connection,
    req: &CreateRoleAssessmentRequest,
    now: &str,
) -> Result<RoleAssessment, String> {
    if req.role_title.trim().is_empty() {
        return Err("role_title is required".into());
    }
    if get_organization_impl(conn, &req.org_id)?.is_none() {
        return Err(format!("organization {} not found", req.org_id));
    }
    if let Some(level) = &req.required_assurance_level {
        if !matches!(level.as_str(), "local" | "anchored" | "high_assurance") {
            return Err(format!("invalid required_assurance_level: {level}"));
        }
    }
    let id = entity_id(&[&req.org_id, &req.role_title, now]);
    let skill_ids_json = serde_json::to_string(&req.skill_ids).map_err(|e| e.to_string())?;
    let policy_json = match &req.issuance_policy {
        Some(p) => Some(serde_json::to_string(p).map_err(|e| e.to_string())?),
        None => None,
    };
    conn.execute(
        "INSERT INTO role_assessments
            (id, org_id, role_title, job_description, course_id, skill_ids,
             issuance_policy_json, required_assurance_level, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'draft', ?9, ?9)",
        params![
            id,
            req.org_id,
            req.role_title,
            req.job_description,
            req.course_id,
            skill_ids_json,
            policy_json,
            req.required_assurance_level,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;
    get_role_assessment_impl(conn, &id)?.ok_or_else(|| "failed to create role assessment".into())
}

pub fn get_role_assessment_impl(
    conn: &Connection,
    id: &str,
) -> Result<Option<RoleAssessment>, String> {
    conn.query_row(
        "SELECT id, org_id, role_title, job_description, course_id, skill_ids,
                issuance_policy_json, required_assurance_level, status, created_at, updated_at
         FROM role_assessments WHERE id = ?1",
        params![id],
        map_role_assessment,
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn list_role_assessments_impl(
    conn: &Connection,
    org_id: Option<&str>,
) -> Result<Vec<RoleAssessment>, String> {
    let mut sql = String::from(
        "SELECT id, org_id, role_title, job_description, course_id, skill_ids,
                issuance_policy_json, required_assurance_level, status, created_at, updated_at
         FROM role_assessments",
    );
    if org_id.is_some() {
        sql.push_str(" WHERE org_id = ?1");
    }
    sql.push_str(" ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = if let Some(org) = org_id {
        stmt.query_map(params![org], map_role_assessment)
    } else {
        stmt.query_map([], map_role_assessment)
    }
    .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn set_role_assessment_status_impl(
    conn: &Connection,
    id: &str,
    status: &str,
    now: &str,
) -> Result<RoleAssessment, String> {
    if !matches!(status, "draft" | "published" | "archived") {
        return Err(format!("invalid status: {status}"));
    }
    let n = conn
        .execute(
            "UPDATE role_assessments SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status, now],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("role assessment not found".into());
    }
    get_role_assessment_impl(conn, id)?.ok_or_else(|| "role assessment not found".into())
}

fn map_role_assessment(r: &rusqlite::Row<'_>) -> rusqlite::Result<RoleAssessment> {
    let skill_ids_json: Option<String> = r.get(5)?;
    let policy_json: Option<String> = r.get(6)?;
    Ok(RoleAssessment {
        id: r.get(0)?,
        org_id: r.get(1)?,
        role_title: r.get(2)?,
        job_description: r.get(3)?,
        course_id: r.get(4)?,
        skill_ids: skill_ids_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        issuance_policy: policy_json.and_then(|s| serde_json::from_str(&s).ok()),
        required_assurance_level: r.get(7)?,
        status: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

// ============================================================================
// Keystone — issue a role credential gated by the role's policy
// ============================================================================

/// Issue a `RoleCredential` for `subject` against `role_assessment_id`,
/// bound to `integrity_session_id` and gated by the role's policy +
/// required assurance level. Reuses the P0 issuance pipeline, so the
/// resulting VC carries the integrity attestation and is refused if the
/// session doesn't satisfy the role's bounds.
#[allow(clippy::too_many_arguments)]
pub fn issue_role_credential_impl(
    conn: &Connection,
    issuer_key: &SigningKey,
    issuer_did: &Did,
    role_assessment_id: &str,
    subject: &Did,
    integrity_session_id: &str,
    now: &str,
) -> Result<VerifiableCredential, String> {
    let ra = get_role_assessment_impl(conn, role_assessment_id)?
        .ok_or_else(|| format!("role assessment {role_assessment_id} not found"))?;
    let org = get_organization_impl(conn, &ra.org_id)?
        .ok_or_else(|| format!("organization {} not found", ra.org_id))?;

    // Fold the role's required assurance level into its issuance policy
    // so a single gate covers both the integrity bounds and the
    // attestation requirement.
    let mut policy = ra.issuance_policy.clone().unwrap_or_default();
    if ra.required_assurance_level.is_some() {
        policy.required_assurance_level = ra.required_assurance_level.clone();
    }

    let req = IssueCredentialRequest {
        credential_type: CredentialType::RoleCredential,
        subject: subject.clone(),
        claim: Claim::Role(RoleClaim {
            role: ra.role_title.clone(),
            scope: Some(org.name.clone()),
        }),
        evidence_refs: vec![],
        expiration_date: None,
        supersedes: None,
        integrity_session_id: Some(integrity_session_id.to_string()),
        integrity_policy: Some(policy),
    };
    issue_credential_impl(conn, issuer_key, issuer_did, &req, now)
}

// ============================================================================
// Tauri command handlers
// ============================================================================

#[tauri::command]
pub async fn create_organization(
    state: State<'_, AppState>,
    name: String,
    owner_address: String,
    did: Option<String>,
) -> Result<Organization, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    create_organization_impl(
        db.conn(),
        &name,
        &owner_address,
        did.as_deref(),
        &now_rfc3339(),
    )
}

#[tauri::command]
pub async fn list_organizations(
    state: State<'_, AppState>,
    owner_address: Option<String>,
) -> Result<Vec<Organization>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    list_organizations_impl(db.conn(), owner_address.as_deref())
}

#[tauri::command]
pub async fn create_role_assessment(
    state: State<'_, AppState>,
    req: CreateRoleAssessmentRequest,
) -> Result<RoleAssessment, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    create_role_assessment_impl(db.conn(), &req, &now_rfc3339())
}

#[tauri::command]
pub async fn list_role_assessments(
    state: State<'_, AppState>,
    org_id: Option<String>,
) -> Result<Vec<RoleAssessment>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    list_role_assessments_impl(db.conn(), org_id.as_deref())
}

#[tauri::command]
pub async fn get_role_assessment(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<RoleAssessment>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    get_role_assessment_impl(db.conn(), &id)
}

#[tauri::command]
pub async fn set_role_assessment_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<RoleAssessment, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    set_role_assessment_status_impl(db.conn(), &id, &status, &now_rfc3339())
}

#[tauri::command]
pub async fn issue_role_credential(
    state: State<'_, AppState>,
    role_assessment_id: String,
    subject: String,
    integrity_session_id: String,
) -> Result<VerifiableCredential, String> {
    let (signing_key, issuer_did) = load_issuer_key(&state).await?;
    let now = now_rfc3339();
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    issue_role_credential_impl(
        db.conn(),
        &signing_key,
        &issuer_did,
        &role_assessment_id,
        &Did(subject),
        &integrity_session_id,
        &now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::derive_did_key;
    use crate::db::Database;

    const NOW: &str = "2026-04-13T00:00:00Z";

    fn setup() -> (Database, SigningKey, Did, Did) {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let issuer_key = SigningKey::from_bytes(&[1u8; 32]);
        let issuer = derive_did_key(&issuer_key);
        let subject = derive_did_key(&SigningKey::from_bytes(&[2u8; 32]));
        (db, issuer_key, issuer, subject)
    }

    fn seed_session(conn: &Connection, id: &str, status: &str, score: f64, critical: i64) {
        conn.execute(
            "INSERT INTO integrity_sessions
                (id, enrollment_id, status, integrity_score, critical_count, warning_count,
                 started_at, ended_at)
             VALUES (?1, NULL, ?2, ?3, ?4, 0, ?5, ?5)",
            params![id, status, score, critical, NOW],
        )
        .unwrap();
    }

    #[test]
    fn org_and_role_assessment_round_trip() {
        let (db, ..) = setup();
        let conn = db.conn();
        let org = create_organization_impl(conn, "Acme Corp", "stake_owner", None, NOW).unwrap();
        let req = CreateRoleAssessmentRequest {
            org_id: org.id.clone(),
            role_title: "SRE L4".into(),
            job_description: Some("Operate prod at scale".into()),
            course_id: None,
            skill_ids: vec!["skill:sre".into()],
            issuance_policy: Some(IssuancePolicy {
                min_integrity: Some(0.7),
                require_clean: true,
                ..Default::default()
            }),
            required_assurance_level: Some("anchored".into()),
        };
        let ra = create_role_assessment_impl(conn, &req, NOW).unwrap();
        let fetched = get_role_assessment_impl(conn, &ra.id).unwrap().unwrap();
        assert_eq!(fetched.role_title, "SRE L4");
        assert_eq!(fetched.skill_ids, vec!["skill:sre".to_string()]);
        assert_eq!(
            fetched.required_assurance_level.as_deref(),
            Some("anchored")
        );
        assert!(fetched.issuance_policy.unwrap().require_clean);
        assert_eq!(
            list_role_assessments_impl(conn, Some(&org.id))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn invalid_assurance_level_rejected() {
        let (db, ..) = setup();
        let conn = db.conn();
        let org = create_organization_impl(conn, "Acme", "stake_owner", None, NOW).unwrap();
        let req = CreateRoleAssessmentRequest {
            org_id: org.id,
            role_title: "X".into(),
            job_description: None,
            course_id: None,
            skill_ids: vec![],
            issuance_policy: None,
            required_assurance_level: Some("super_duper".into()),
        };
        assert!(create_role_assessment_impl(conn, &req, NOW).is_err());
    }

    #[test]
    fn issue_role_credential_passes_gate_and_embeds_role() {
        let (db, key, issuer, subject) = setup();
        let conn = db.conn();
        let org = create_organization_impl(conn, "Acme", "stake_owner", None, NOW).unwrap();
        let ra = create_role_assessment_impl(
            conn,
            &CreateRoleAssessmentRequest {
                org_id: org.id,
                role_title: "SRE L4".into(),
                job_description: None,
                course_id: None,
                skill_ids: vec![],
                issuance_policy: Some(IssuancePolicy {
                    min_integrity: Some(0.7),
                    require_clean: true,
                    ..Default::default()
                }),
                required_assurance_level: None,
            },
            NOW,
        )
        .unwrap();
        seed_session(conn, "sess_ok", "completed", 0.9, 0);

        let vc = issue_role_credential_impl(conn, &key, &issuer, &ra.id, &subject, "sess_ok", NOW)
            .unwrap();
        assert!(vc.type_.contains(&"RoleCredential".to_string()));
        let role = RoleClaim::extract(&vc.credential_subject).expect("role claim");
        assert_eq!(role.role, "SRE L4");
        assert_eq!(role.scope.as_deref(), Some("Acme"));
        let attest = vc.integrity.expect("integrity embedded");
        assert_eq!(attest.session_id, "sess_ok");
    }

    #[test]
    fn issue_role_credential_blocked_by_failing_session() {
        let (db, key, issuer, subject) = setup();
        let conn = db.conn();
        let org = create_organization_impl(conn, "Acme", "stake_owner", None, NOW).unwrap();
        let ra = create_role_assessment_impl(
            conn,
            &CreateRoleAssessmentRequest {
                org_id: org.id,
                role_title: "SRE L4".into(),
                job_description: None,
                course_id: None,
                skill_ids: vec![],
                issuance_policy: Some(IssuancePolicy {
                    min_integrity: Some(0.7),
                    require_clean: true,
                    ..Default::default()
                }),
                required_assurance_level: None,
            },
            NOW,
        )
        .unwrap();
        seed_session(conn, "sess_bad", "suspended", 0.3, 2);
        let err =
            issue_role_credential_impl(conn, &key, &issuer, &ra.id, &subject, "sess_bad", NOW)
                .unwrap_err();
        assert!(err.contains("issuance policy"), "got: {err}");
    }
}
