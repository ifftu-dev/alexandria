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
use crate::crypto::did::{derive_did_key, Did};
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

    // Issue under the organisation's own DID, not the caller's personal one.
    //
    // This is what makes the credential independent evidence. Aggregation
    // weighs a skill by how many *distinct* issuer clusters back it
    // (`unique_issuer_clusters`); a learner's own self-issued assessment
    // credentials all share one cluster and so cannot raise confidence past a
    // structural cap. An organisation issuing under its own stable DID is a
    // second, independent cluster — the thing an employer is actually paying
    // for. Using the caller's personal DID here would collapse every org this
    // person administers into one issuer and defeat that.
    //
    // The caller must hold the org's key: an org's DID defaults to its
    // owner's, so the owner acts as the org. If the org carries a DID the
    // caller cannot sign for, issuance is refused rather than silently
    // falling back to a personal signature.
    let signer_did = derive_did_key(issuer_key);
    let org_did = match org.did.as_deref() {
        Some(did) if did == signer_did.0 => did.to_string(),
        Some(did) => {
            return Err(format!(
                "cannot issue as organization '{}': its DID is {did}, but you hold {}",
                org.name, signer_did.0
            ));
        }
        // No DID yet — adopt the owner's, so the org has a stable issuer
        // identity from its first issuance onward.
        None => {
            conn.execute(
                "UPDATE organizations SET did = ?1 WHERE id = ?2",
                params![signer_did.0, org.id],
            )
            .map_err(|e| e.to_string())?;
            signer_did.0.clone()
        }
    };
    let org_issuer = Did(org_did);

    // A learner cannot be issued an independent role credential by "their own
    // organisation" — that would be self-issuance wearing an org hat, and it
    // must not count as a second cluster.
    if org_issuer.0 == subject.0 {
        return Err("an organization cannot issue a role credential to itself".into());
    }

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
    // `issuer_did` is retained in the signature for API compatibility but the
    // credential is issued under the org's DID.
    let _ = issuer_did;
    issue_credential_impl(conn, issuer_key, &org_issuer, &req, now)
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

    /// Helper: a completed org + role assessment + clean session, ready to
    /// issue. `org_did` seeds the organization's DID column (None to adopt).
    fn ready_to_issue(conn: &Connection, org_did: Option<&str>) -> String {
        let org = create_organization_impl(conn, "Acme", "stake_owner", org_did, NOW).unwrap();
        let ra = create_role_assessment_impl(
            conn,
            &CreateRoleAssessmentRequest {
                org_id: org.id,
                role_title: "SRE L4".into(),
                job_description: None,
                course_id: None,
                skill_ids: vec![],
                issuance_policy: None,
                required_assurance_level: None,
            },
            NOW,
        )
        .unwrap();
        seed_session(conn, "sess_ok", "completed", 0.95, 0);
        ra.id
    }

    #[test]
    fn credential_is_issued_under_the_org_did_not_the_owner_personal_did() {
        // The core of 1.7: the issuer is the organisation, so its credentials
        // form one stable, independent cluster rather than the owner's
        // personal identity.
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let ra = ready_to_issue(conn, Some(&owner.0)); // org DID == owner DID
        let vc =
            issue_role_credential_impl(conn, &key, &owner, &ra, &subject, "sess_ok", NOW).unwrap();
        assert_eq!(vc.issuer, owner, "issued under the org's DID");
        assert_ne!(
            vc.issuer.0, subject.0,
            "issuer is independent of the subject"
        );
    }

    #[test]
    fn an_org_without_a_did_adopts_the_owners_on_first_issuance() {
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let ra = ready_to_issue(conn, None);
        issue_role_credential_impl(conn, &key, &owner, &ra, &subject, "sess_ok", NOW).unwrap();

        // The org now carries a stable DID for every future issuance.
        let org_id: String = conn
            .query_row(
                "SELECT org_id FROM role_assessments WHERE id = ?1",
                [&ra],
                |r| r.get(0),
            )
            .unwrap();
        let org = get_organization_impl(conn, &org_id).unwrap().unwrap();
        assert_eq!(org.did.as_deref(), Some(owner.0.as_str()));
    }

    #[test]
    fn issuance_is_refused_when_the_caller_does_not_hold_the_org_key() {
        // The org's DID belongs to someone else; this caller cannot sign as it
        // and must be refused rather than silently signing personally.
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let other_org_did = derive_did_key(&SigningKey::from_bytes(&[9u8; 32])).0;
        let ra = ready_to_issue(conn, Some(&other_org_did));
        let err = issue_role_credential_impl(conn, &key, &owner, &ra, &subject, "sess_ok", NOW)
            .unwrap_err();
        assert!(
            err.contains("cannot issue as organization"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn an_org_cannot_issue_a_role_credential_to_itself() {
        // Issuing to the org's own DID would be self-issuance in disguise and
        // must not count as an independent cluster.
        let (db, key, owner, _subject) = setup();
        let conn = db.conn();
        let ra = ready_to_issue(conn, Some(&owner.0));
        let err = issue_role_credential_impl(conn, &key, &owner, &ra, &owner, "sess_ok", NOW)
            .unwrap_err();
        assert!(err.contains("to itself"), "unexpected: {err}");
    }

    #[test]
    fn an_org_credential_is_a_distinct_issuer_cluster_from_self_assessment() {
        // The payoff finding 1 predicted: a learner's own assessment
        // credentials share one issuer cluster and cannot lift confidence past
        // a cap, but an org-issued role credential is a second, independent
        // cluster — so it raises the count aggregation weighs.
        use crate::commands::aggregation::recompute_all_impl;
        use crate::commands::credentials::{issue_credential_impl, IssueCredentialRequest};
        use crate::domain::vc::SkillClaim;

        let (db, owner_key, owner, _subject) = setup();
        let conn = db.conn();
        let learner_key = SigningKey::from_bytes(&[7u8; 32]);
        let learner = derive_did_key(&learner_key);

        // Learner self-issues two assessment credentials for a skill.
        for i in 0..2 {
            let req = IssueCredentialRequest {
                credential_type: CredentialType::AssessmentCredential,
                subject: learner.clone(),
                claim: Claim::Skill(SkillClaim {
                    skill_id: "skill:sre".into(),
                    level: 3,
                    score: 0.8,
                    evidence_refs: vec![format!("attempt_{i}")],
                    rubric_version: None,
                    assessment_method: Some("proctored_quiz".into()),
                    provenance: None,
                }),
                evidence_refs: vec![],
                expiration_date: None,
                supersedes: None,
                integrity_session_id: None,
                integrity_policy: None,
            };
            issue_credential_impl(conn, &learner_key, &learner, &req, NOW).unwrap();
        }
        recompute_all_impl(conn, NOW).unwrap();
        let self_only: i64 = conn
            .query_row(
                "SELECT unique_issuer_clusters FROM derived_skill_states                   WHERE subject_did = ?1 AND skill_id = 'skill:sre'",
                [&learner.0],
                |r| r.get(0),
            )
            .unwrap();

        // The org now issues a *skill* credential for the same skill under its
        // own DID (distinct issuer). Independent cluster count must rise.
        let skill_req = IssueCredentialRequest {
            credential_type: CredentialType::AssessmentCredential,
            subject: learner.clone(),
            claim: Claim::Skill(SkillClaim {
                skill_id: "skill:sre".into(),
                level: 3,
                score: 0.8,
                evidence_refs: vec![],
                rubric_version: None,
                assessment_method: Some("org_assessment".into()),
                provenance: None,
            }),
            evidence_refs: vec![],
            expiration_date: None,
            supersedes: None,
            integrity_session_id: None,
            integrity_policy: None,
        };
        issue_credential_impl(conn, &owner_key, &owner, &skill_req, NOW).unwrap();
        recompute_all_impl(conn, NOW).unwrap();
        let with_org: i64 = conn
            .query_row(
                "SELECT unique_issuer_clusters FROM derived_skill_states                   WHERE subject_did = ?1 AND skill_id = 'skill:sre'",
                [&learner.0],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(self_only, 1, "self-issued credentials are one cluster");
        assert!(
            with_org > self_only,
            "an independent org issuer must raise the cluster count: {with_org} !> {self_only}"
        );
    }
}
