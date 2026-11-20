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

use crate::credentials::Issuer;
use crate::profile::scope::ProfileState as State;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::clock::now_rfc3339;
use crate::credentials::{self, IssuancePolicy, IssueCredentialRequest};
use crate::crypto::did::Did;
use crate::crypto::hash::entity_id;
use crate::db::executor::DatabaseWorkload;
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
        match level.as_str() {
            crate::commands::integrity::ACHIEVED_ASSURANCE_LEVEL
            | crate::sentinel::attestation::ASSURANCE_DEVICE_ATTESTED => {}
            "anchored" | "high_assurance" => {
                return Err(format!(
                    "required_assurance_level '{level}' is unavailable: no verified path can achieve it"
                ))
            }
            _ => return Err(format!("invalid required_assurance_level: {level}")),
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

/// One learner attempt made for a role, for the sponsor's issuance picker.
/// Carries the identifiers `issue_role_credential` needs and the grade
/// outcome; no Sentinel detail beyond the session id.
#[derive(Debug, Clone, Serialize)]
pub struct RoleAttemptSummary {
    pub attempt_id: String,
    pub subject_did: String,
    pub skill_id: String,
    pub integrity_session_id: Option<String>,
    pub score: Option<f64>,
    pub passed: Option<bool>,
    pub credential_id: Option<String>,
    pub started_at: String,
    pub graded_at: Option<String>,
    pub ended_at: Option<String>,
    pub end_reason: Option<String>,
}

/// Attempts learners made for `role_assessment_id`, newest first.
pub fn list_role_attempts_impl(
    conn: &Connection,
    role_assessment_id: &str,
) -> Result<Vec<RoleAttemptSummary>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, subject_did, skill_id, integrity_session_id, score, passed, \
                    credential_id, started_at, graded_at, ended_at, end_reason \
               FROM assessment_attempts \
              WHERE role_assessment_id = ?1 \
              ORDER BY started_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![role_assessment_id], |r| {
            Ok(RoleAttemptSummary {
                attempt_id: r.get(0)?,
                subject_did: r.get(1)?,
                skill_id: r.get(2)?,
                integrity_session_id: r.get(3)?,
                score: r.get(4)?,
                passed: r.get::<_, Option<i64>>(5)?.map(|p| p != 0),
                credential_id: r.get(6)?,
                started_at: r.get(7)?,
                graded_at: r.get(8)?,
                ended_at: r.get(9)?,
                end_reason: r.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// A role attempt's finished integrity session, rescored for review.
#[derive(Debug, Clone, Serialize)]
pub struct RoleSessionReview {
    pub attempt_id: String,
    pub subject_did: String,
    pub integrity_session_id: String,
    pub score: Option<f64>,
    pub passed: Option<bool>,
    pub review: crate::sentinel::surface::SessionReview,
}

/// The finished sessions of `role_assessment_id`'s attempts, most suspicious
/// first: by fusion posterior, then session status, then how many windows
/// raised a signal. Under a shared prior the posterior order is the summed
/// log-likelihood ratio order, so no prior is assumed here.
pub fn list_role_session_reviews_impl(
    conn: &Connection,
    role_assessment_id: &str,
    manifest: &crate::sentinel::scoring::RatioManifest,
    certification: Option<&crate::sentinel::scoring::measurement::Certification>,
) -> Result<Vec<RoleSessionReview>, String> {
    use crate::sentinel::scoring::SessionStatus;
    let mut reviews = Vec::new();
    for attempt in list_role_attempts_impl(conn, role_assessment_id)? {
        let Some(session_id) = attempt.integrity_session_id else {
            continue;
        };
        let review =
            crate::sentinel::surface::session_review(conn, &session_id, manifest, certification)?;
        reviews.push(RoleSessionReview {
            attempt_id: attempt.attempt_id,
            subject_did: attempt.subject_did,
            integrity_session_id: session_id,
            score: attempt.score,
            passed: attempt.passed,
            review,
        });
    }
    let status = |r: &RoleSessionReview| SessionStatus::parse(&r.review.status);
    let firings = |r: &RoleSessionReview| r.review.signals.iter().map(|s| s.windows).sum::<u32>();
    reviews.sort_by(|a, b| {
        b.review
            .posterior_llr
            .total_cmp(&a.review.posterior_llr)
            .then(status(b).cmp(&status(a)))
            .then(firings(b).cmp(&firings(a)))
    });
    Ok(reviews)
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

/// Why a sponsor's device refuses to bind an integrity session to a role
/// credential: the session must be the named subject's passed attempt at
/// that role assessment.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
enum AttemptBindingRefusal {
    #[error("integrity session {0} is unknown on this device")]
    UnknownSession(String),
    #[error("integrity session {0} is not an attempt for this role assessment")]
    NotRoleAttempt(String),
    #[error("integrity session {0} belongs to another subject's attempt")]
    OtherSubject(String),
    #[error("the attempt for integrity session {0} has not been graded")]
    NotGraded(String),
    #[error("the attempt for integrity session {0} did not pass")]
    NotPassed(String),
}

/// Check that `integrity_session_id` belongs to an attempt `subject` made
/// for `role_assessment_id`, and that the attempt was graded as passed.
/// Returns the passed attempt's assessment credential id, when grading
/// issued one.
fn check_attempt_binding(
    conn: &Connection,
    role_assessment_id: &str,
    subject: &Did,
    integrity_session_id: &str,
) -> Result<Option<String>, String> {
    let known: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM integrity_sessions WHERE id = ?1)",
            params![integrity_session_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !known {
        return Err(AttemptBindingRefusal::UnknownSession(integrity_session_id.into()).to_string());
    }
    let mut stmt = conn
        .prepare(
            "SELECT subject_did, passed, graded_at, credential_id FROM assessment_attempts \
              WHERE role_assessment_id = ?1 AND integrity_session_id = ?2",
        )
        .map_err(|e| e.to_string())?;
    let attempts = stmt
        .query_map(params![role_assessment_id, integrity_session_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if attempts.is_empty() {
        return Err(AttemptBindingRefusal::NotRoleAttempt(integrity_session_id.into()).to_string());
    }
    let mut own = attempts
        .iter()
        .filter(|(attempt_subject, ..)| *attempt_subject == subject.0)
        .peekable();
    if own.peek().is_none() {
        return Err(AttemptBindingRefusal::OtherSubject(integrity_session_id.into()).to_string());
    }
    // An attempt is graded once it has both a grade time and a pass bit.
    let mut graded = false;
    for (_, passed, graded_at, credential_id) in own {
        if let (Some(passed), Some(_)) = (passed, graded_at) {
            if *passed != 0 {
                return Ok(credential_id.clone());
            }
            graded = true;
        }
    }
    let session = integrity_session_id.to_string();
    Err(if graded {
        AttemptBindingRefusal::NotPassed(session)
    } else {
        AttemptBindingRefusal::NotGraded(session)
    }
    .to_string())
}

/// Issue a `RoleCredential` for `subject` against `role_assessment_id`,
/// bound to `integrity_session_id` and gated by the role's policy +
/// required assurance level. Reuses the P0 issuance pipeline, so the
/// resulting VC carries the integrity attestation and is refused if the
/// session doesn't satisfy the role's bounds.
///
/// Before anything is written, the session must be an attempt `subject`
/// made for this role and passed (`AttemptBindingRefusal`), so a sponsor
/// cannot sign the wrong person's session. The role's policy is then judged
/// by the shared rule inside `credentials::issue`.
pub fn issue_role_credential_impl(
    conn: &Connection,
    signer: &Issuer,
    role_assessment_id: &str,
    subject: &Did,
    integrity_session_id: &str,
    now: &str,
) -> Result<VerifiableCredential, String> {
    issue_role_credential_with(
        conn,
        signer,
        role_assessment_id,
        subject,
        integrity_session_id,
        now,
        &credentials::EvidenceSource::native(),
    )
}

/// The `AssessedCredentialEvidence` entry naming the assessment credential
/// grading issued for the attempt, verified with `trust`, or `None` when
/// grading issued none (the role's own evidence is judged then).
///
/// The sponsor's device usually cannot bind fresh evidence for a
/// candidate's session; the attempt's credential carries the evidence the
/// candidate's device bound at grading. It must be the subject's, for this
/// session, and verify: a credential that does not refuses issuance.
fn assessed_entry(
    conn: &Connection,
    credential_id: Option<&str>,
    subject: &Did,
    integrity_session_id: &str,
    now: &str,
    trust: &alexandria_verify::attestation::AttestationTrust,
) -> Result<Option<alexandria_verify::vc::EvidenceEntry>, String> {
    use alexandria_verify::assessed::AssessedCredentialEvidence;
    let Some(credential_id) = credential_id else {
        return Ok(None);
    };
    let assessed = credentials::store::get(conn, credential_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!("the attempt's assessment credential {credential_id} is not on this device")
        })?;
    let session = assessed.integrity.as_ref().map(|a| a.session_id.as_str());
    if assessed.credential_subject.id != *subject || session != Some(integrity_session_id) {
        return Err(format!(
            "the attempt's assessment credential {credential_id} is not the subject's for \
             integrity session {integrity_session_id}"
        ));
    }
    let result = alexandria_verify::vc::verify::verify_credential_with_attestation(
        &crate::domain::vc::SqliteVerificationStore(conn),
        &assessed,
        now,
        &crate::domain::vc::VerificationPolicy::default(),
        trust,
    );
    if result.acceptance_decision != crate::domain::vc::AcceptanceDecision::Accept {
        return Err(format!(
            "the attempt's assessment credential {credential_id} does not verify: {}",
            credentials::bundle::rejection_reasons(&result).join(", ")
        ));
    }
    let entry = AssessedCredentialEvidence::new(&assessed, result.attestation)
        .map_err(|e| e.to_string())?;
    Ok(Some(entry.into_entry()))
}

/// [`issue_role_credential_impl`] with an explicit evidence source: the
/// trust inputs the attempt's credential is verified with, and the attester
/// the role credential's own attestation evidence is asked of.
///
/// The role's assurance is judged from the attempt's assessment credential
/// when grading issued one: the role credential carries an
/// `AssessedCredentialEvidence` entry naming it by id and integrity hash
/// with the attestation outcome and verified level found, and the shared
/// rule takes the verified level from that entry.
pub fn issue_role_credential_with(
    conn: &Connection,
    signer: &Issuer,
    role_assessment_id: &str,
    subject: &Did,
    integrity_session_id: &str,
    now: &str,
    source: &credentials::EvidenceSource<'_>,
) -> Result<VerifiableCredential, String> {
    let ra = get_role_assessment_impl(conn, role_assessment_id)?
        .ok_or_else(|| format!("role assessment {role_assessment_id} not found"))?;
    let org = get_organization_impl(conn, &ra.org_id)?
        .ok_or_else(|| format!("organization {} not found", ra.org_id))?;
    let attempt_credential = check_attempt_binding(conn, &ra.id, subject, integrity_session_id)?;
    let assessed = assessed_entry(
        conn,
        attempt_credential.as_deref(),
        subject,
        integrity_session_id,
        now,
        &source.trust.inputs(),
    )?;

    // Issue under the organisation's own DID, not the caller's personal one.
    //
    // This is what makes the credential independent evidence. Aggregation
    // weighs a skill by how many *distinct* independent issuer clusters back
    // it (`unique_issuer_clusters`); a learner's own self-issued assessment
    // credentials are scored but add no independent cluster at all. An
    // organisation issuing under its own stable DID is an independent
    // cluster — the thing an employer is actually paying for. Using the caller's personal DID here would collapse every org this
    // person administers into one issuer and defeat that.
    //
    // The caller must hold the org's key: an org's DID defaults to its
    // owner's, so the owner acts as the org. If the org carries a DID the
    // caller cannot sign for, issuance is refused rather than silently
    // falling back to a personal signature.
    let signer_did = signer.did();
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
    // The org's DID is the signer's own (checked above), so the signer issues.
    credentials::issue_with_evidence(
        conn,
        signer,
        &req,
        now,
        source,
        assessed.into_iter().collect(),
    )
    .map(|issued| issued.credential)
    .map_err(|e| e.to_string())
}

fn issue_role_credential_transactional(
    conn: &Connection,
    signer: &Issuer,
    role_assessment_id: &str,
    subject: &Did,
    integrity_session_id: &str,
    now: &str,
) -> Result<VerifiableCredential, String> {
    crate::db::with_transaction(conn, || {
        issue_role_credential_impl(
            conn,
            signer,
            role_assessment_id,
            subject,
            integrity_session_id,
            now,
        )
    })
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
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "role-assessment.organization.create",
            move |db| {
                create_organization_impl(db.conn(), &name, &owner_address, did.as_deref(), &now)
            },
        )
        .await
}

#[tauri::command]
pub async fn list_organizations(
    state: State<'_, AppState>,
    owner_address: Option<String>,
) -> Result<Vec<Organization>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "role-assessment.organization.list",
            move |db| list_organizations_impl(db.conn(), owner_address.as_deref()),
        )
        .await
}

#[tauri::command]
pub async fn create_role_assessment(
    state: State<'_, AppState>,
    req: CreateRoleAssessmentRequest,
) -> Result<RoleAssessment, String> {
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "role-assessment.create",
            move |db| create_role_assessment_impl(db.conn(), &req, &now),
        )
        .await
}

#[tauri::command]
pub async fn list_role_assessments(
    state: State<'_, AppState>,
    org_id: Option<String>,
) -> Result<Vec<RoleAssessment>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "role-assessment.list",
            move |db| list_role_assessments_impl(db.conn(), org_id.as_deref()),
        )
        .await
}

/// Attempts made for a role, so a sponsor issues from a real attempt rather
/// than a hand-typed session id.
#[tauri::command]
pub async fn list_role_attempts(
    state: State<'_, AppState>,
    role_assessment_id: String,
) -> Result<Vec<RoleAttemptSummary>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "role-assessment.list_attempts",
            move |db| list_role_attempts_impl(db.conn(), &role_assessment_id),
        )
        .await
}

/// The role's finished sessions ranked for review by fusion posterior, with
/// each raised signal's certified rates or its advisory status.
#[tauri::command]
pub async fn list_role_session_reviews(
    state: State<'_, AppState>,
    role_assessment_id: String,
) -> Result<Vec<RoleSessionReview>, String> {
    use crate::sentinel::scoring::{active_certification, active_manifest};
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "role-assessment.session_reviews",
            move |db| {
                let certification = active_certification();
                list_role_session_reviews_impl(
                    db.conn(),
                    &role_assessment_id,
                    &active_manifest(),
                    certification.as_ref().map(|(c, _)| c),
                )
            },
        )
        .await
}

#[tauri::command]
pub async fn get_role_assessment(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<RoleAssessment>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "role-assessment.get",
            move |db| get_role_assessment_impl(db.conn(), &id),
        )
        .await
}

#[tauri::command]
pub async fn set_role_assessment_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<RoleAssessment, String> {
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "role-assessment.status.set",
            move |db| set_role_assessment_status_impl(db.conn(), &id, &status, &now),
        )
        .await
}

#[tauri::command]
pub async fn issue_role_credential(
    state: State<'_, AppState>,
    role_assessment_id: String,
    subject: String,
    integrity_session_id: String,
) -> Result<VerifiableCredential, String> {
    let signer = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?;
    let now = now_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "role-assessment.credential.issue",
            move |db| {
                issue_role_credential_transactional(
                    db.conn(),
                    &signer,
                    &role_assessment_id,
                    &Did(subject),
                    &integrity_session_id,
                    &now,
                )
            },
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::derive_did_key;
    use crate::db::Database;
    use ed25519_dalek::SigningKey;

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
            required_assurance_level: Some("local".into()),
        };
        let ra = create_role_assessment_impl(conn, &req, NOW).unwrap();
        let fetched = get_role_assessment_impl(conn, &ra.id).unwrap().unwrap();
        assert_eq!(fetched.role_title, "SRE L4");
        assert_eq!(fetched.skill_ids, vec!["skill:sre".to_string()]);
        assert_eq!(fetched.required_assurance_level.as_deref(), Some("local"));
        assert!(fetched.issuance_policy.unwrap().require_clean);
        assert_eq!(
            list_role_assessments_impl(conn, Some(&org.id))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn role_attempts_list_newest_first_and_only_for_that_role() {
        let (db, ..) = setup();
        let conn = db.conn();
        let org = create_organization_impl(conn, "Acme", "stake_owner", None, NOW).unwrap();
        let mk = |title: &str| {
            create_role_assessment_impl(
                conn,
                &CreateRoleAssessmentRequest {
                    org_id: org.id.clone(),
                    role_title: title.into(),
                    job_description: None,
                    course_id: None,
                    skill_ids: vec!["skill_rust".into()],
                    issuance_policy: None,
                    required_assurance_level: None,
                },
                NOW,
            )
            .unwrap()
            .id
        };
        let a = mk("A");
        let b = mk("B");
        conn.execute_batch(
            "INSERT INTO question_banks (id, skill_id, label, ratified) VALUES ('bank', 'skill_rust', 'T', 1);",
        )
        .unwrap();
        for (id, role, started, passed) in [
            ("att_old", &a, "2026-10-01T00:00:00Z", Some(0i64)),
            ("att_new", &a, "2026-10-02T00:00:00Z", Some(1)),
            ("att_b", &b, "2026-10-03T00:00:00Z", None),
        ] {
            conn.execute(
                "INSERT INTO assessment_attempts \
                 (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, \
                  integrity_session_id, started_at, role_assessment_id, passed) \
                 VALUES (?1, 'did:key:zL', 'bank', 'skill_rust', 1, '[]', '[]', 'isess', ?2, ?3, ?4)",
                params![id, started, role, passed],
            )
            .unwrap();
        }
        let listed = list_role_attempts_impl(conn, &a).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|x| x.attempt_id.as_str())
                .collect::<Vec<_>>(),
            vec!["att_new", "att_old"]
        );
        assert_eq!(listed[0].passed, Some(true));
        assert_eq!(listed[1].passed, Some(false));
        assert_eq!(listed[0].integrity_session_id.as_deref(), Some("isess"));
        assert_eq!(list_role_attempts_impl(conn, &b).unwrap().len(), 1);
    }

    /// A role with one finished `core` session and attempt per entry of
    /// `sessions`: (session id, status, flags of each committed window).
    fn role_with_sessions(conn: &Connection, sessions: &[(&str, &str, &[&str])]) -> String {
        let org = create_organization_impl(conn, "Acme", "stake_owner", None, NOW).unwrap();
        let role = create_role_assessment_impl(
            conn,
            &CreateRoleAssessmentRequest {
                org_id: org.id,
                role_title: "SRE".into(),
                job_description: None,
                course_id: None,
                skill_ids: vec!["skill_rust".into()],
                issuance_policy: None,
                required_assurance_level: None,
            },
            NOW,
        )
        .unwrap()
        .id;
        conn.execute_batch(
            "INSERT INTO question_banks (id, skill_id, label, ratified) VALUES ('bank', 'skill_rust', 'T', 1);",
        )
        .unwrap();
        for (session, status, windows) in sessions {
            conn.execute(
                "INSERT INTO integrity_sessions (id, status, trust_profile, started_at, ended_at)
                 VALUES (?1, ?2, 'core', ?3, ?3)",
                params![session, status, NOW],
            )
            .unwrap();
            for (i, flags) in windows.iter().enumerate() {
                conn.execute(
                    "INSERT INTO integrity_snapshots (id, session_id, composite_score, captured_at, anomaly_flags)
                     VALUES (?1, ?2, 0.9, ?3, ?4)",
                    params![format!("{session}-w{i}"), session, format!("2026-04-13T00:00:0{i}Z"), flags],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO assessment_attempts \
                 (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, \
                  integrity_session_id, started_at, role_assessment_id) \
                 VALUES (?1, 'did:key:zL', 'bank', 'skill_rust', 1, '[]', '[]', ?2, ?3, ?4)",
                params![format!("att-{session}"), session, NOW, role],
            )
            .unwrap();
        }
        role
    }

    fn certification() -> crate::sentinel::scoring::measurement::Certification {
        use crate::sentinel::scoring::measurement::{
            Certification, CertifiedProfile, CertifiedSignal,
        };
        Certification {
            format: 1,
            version: "cert-test".into(),
            date: "2026-11-12".into(),
            synthetic: false,
            corpus_id: "corpus".into(),
            corpus_digest: "blake3:00".into(),
            profiles: [(
                "core".to_string(),
                CertifiedProfile {
                    threshold: 2.0,
                    signals: [(
                        "response_idle_burst".to_string(),
                        CertifiedSignal {
                            llr: 1.5,
                            precision: 0.8,
                            recall: 0.6,
                        },
                    )]
                    .into(),
                },
            )]
            .into(),
        }
    }

    fn ranked(reviews: &[RoleSessionReview]) -> Vec<&str> {
        reviews
            .iter()
            .map(|r| r.integrity_session_id.as_str())
            .collect()
    }

    #[test]
    fn sponsor_sees_sessions_ranked_by_fusion_posterior_with_certified_rates() {
        let (db, ..) = setup();
        let conn = db.conn();
        let role = role_with_sessions(
            conn,
            &[
                ("clean", "completed", &["[]", "[]"]),
                (
                    "once",
                    "completed",
                    &[r#"["response_idle_burst","gaze_down_then_response"]"#, "[]"],
                ),
                (
                    "twice",
                    "review",
                    &[r#"["response_idle_burst"]"#, r#"["response_idle_burst"]"#],
                ),
            ],
        );
        let cert = certification();
        let reviews =
            list_role_session_reviews_impl(conn, &role, &cert.ratio_manifest(), Some(&cert))
                .unwrap();
        assert_eq!(ranked(&reviews), vec!["twice", "once", "clean"]);

        let twice = &reviews[0].review;
        assert_eq!(twice.posterior_llr, 3.0);
        assert_eq!(twice.threshold, Some(2.0));
        assert_eq!(twice.signals.len(), 1);
        let burst = &twice.signals[0];
        assert_eq!(burst.signal, "response_idle_burst");
        assert_eq!(burst.windows, 2);
        assert!(!burst.advisory);
        assert_eq!(burst.certified.as_ref().map(|c| c.precision), Some(0.8));
        assert_eq!(burst.certified.as_ref().map(|c| c.recall), Some(0.6));

        let once = &reviews[1].review;
        assert_eq!(once.posterior_llr, 1.5);
        let gaze = once
            .signals
            .iter()
            .find(|s| s.signal == "gaze_down_then_response")
            .unwrap();
        assert!(gaze.advisory);
        assert!(gaze.certified.is_none());

        assert_eq!(reviews[2].review.posterior_llr, 0.0);
        assert!(reviews[2].review.signals.is_empty());
    }

    #[test]
    fn with_no_certified_rates_every_signal_is_advisory_and_status_breaks_the_tie() {
        let (db, ..) = setup();
        let conn = db.conn();
        let role = role_with_sessions(
            conn,
            &[
                ("quiet", "completed", &["[]"]),
                ("noisy", "completed", &[r#"["response_idle_burst"]"#]),
                ("flagged", "flagged", &["[]"]),
            ],
        );
        let reviews = list_role_session_reviews_impl(
            conn,
            &role,
            &crate::sentinel::scoring::RatioManifest::uncertified(),
            None,
        )
        .unwrap();
        assert_eq!(ranked(&reviews), vec!["flagged", "noisy", "quiet"]);
        assert!(reviews.iter().all(|r| r.review.posterior_llr == 0.0));
        assert!(reviews[1].review.signals.iter().all(|s| s.advisory));
    }

    #[test]
    fn device_attested_is_an_acceptable_required_level() {
        let (db, ..) = setup();
        let conn = db.conn();
        let org = create_organization_impl(conn, "Acme", "stake_owner", None, NOW).unwrap();
        let req = CreateRoleAssessmentRequest {
            org_id: org.id.clone(),
            role_title: "Attested".into(),
            job_description: None,
            course_id: None,
            skill_ids: vec![],
            issuance_policy: None,
            required_assurance_level: Some("device_attested".into()),
        };
        let ra = create_role_assessment_impl(conn, &req, NOW).unwrap();
        assert_eq!(
            ra.required_assurance_level.as_deref(),
            Some("device_attested")
        );
    }

    #[test]
    fn unavailable_assurance_level_is_distinct_from_invalid() {
        let (db, ..) = setup();
        let conn = db.conn();
        let org = create_organization_impl(conn, "Acme", "stake_owner", None, NOW).unwrap();
        for level in ["anchored", "high_assurance"] {
            let req = CreateRoleAssessmentRequest {
                org_id: org.id.clone(),
                role_title: format!("Role {level}"),
                job_description: None,
                course_id: None,
                skill_ids: vec![],
                issuance_policy: None,
                required_assurance_level: Some(level.into()),
            };
            let error = create_role_assessment_impl(conn, &req, NOW).unwrap_err();
            assert!(error.contains("is unavailable"), "{error}");
        }
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
        let (db, key, _, subject) = setup();
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
        seed_attempt(conn, "att_ok", &ra.id, &subject.0, "sess_ok", Some(true));

        let vc = issue_role_credential_impl(
            conn,
            &Issuer::from_key(key.clone()),
            &ra.id,
            &subject,
            "sess_ok",
            NOW,
        )
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
        let (db, key, _, subject) = setup();
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
        seed_attempt(conn, "att_bad", &ra.id, &subject.0, "sess_bad", Some(true));
        let err = issue_role_credential_impl(
            conn,
            &Issuer::from_key(key.clone()),
            &ra.id,
            &subject,
            "sess_bad",
            NOW,
        )
        .unwrap_err();
        assert!(err.contains("issuance policy"), "got: {err}");
    }

    /// `org_did` seeds the organization's DID column (None to adopt).
    /// Helper: an org + role assessment + clean session `sess_ok` that
    /// `subject` passed for the role, ready to issue.
    fn ready_to_issue(conn: &Connection, org_did: Option<&str>, subject: &Did) -> String {
        let ra = ready_to_issue_role(conn, org_did, "SRE L4");
        seed_session(conn, "sess_ok", "completed", 0.95, 0);
        seed_attempt(conn, "att_ok", &ra, &subject.0, "sess_ok", Some(true));
        ra
    }

    /// An org + role assessment titled `title`, with no session.
    fn ready_to_issue_role(conn: &Connection, org_did: Option<&str>, title: &str) -> String {
        let org = create_organization_impl(conn, "Acme", "stake_owner", org_did, NOW).unwrap();
        create_role_assessment_impl(
            conn,
            &CreateRoleAssessmentRequest {
                org_id: org.id,
                role_title: title.into(),
                job_description: None,
                course_id: None,
                skill_ids: vec![],
                issuance_policy: None,
                required_assurance_level: None,
            },
            NOW,
        )
        .unwrap()
        .id
    }

    #[test]
    fn credential_is_issued_under_the_org_did_not_the_owner_personal_did() {
        // The core of 1.7: the issuer is the organisation, so its credentials
        // form one stable, independent cluster rather than the owner's
        // personal identity.
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let ra = ready_to_issue(conn, Some(&owner.0), &subject); // org DID == owner DID
        let vc = issue_role_credential_impl(
            conn,
            &Issuer::from_key(key.clone()),
            &ra,
            &subject,
            "sess_ok",
            NOW,
        )
        .unwrap();
        assert_eq!(vc.issuer, owner, "issued under the org's DID");
        assert_ne!(
            vc.issuer.as_str(),
            subject.0,
            "issuer is independent of the subject"
        );
    }

    #[test]
    fn an_org_without_a_did_adopts_the_owners_on_first_issuance() {
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let ra = ready_to_issue(conn, None, &subject);
        issue_role_credential_impl(
            conn,
            &Issuer::from_key(key.clone()),
            &ra,
            &subject,
            "sess_ok",
            NOW,
        )
        .unwrap();

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
    fn failed_credential_insert_rolls_back_org_did_adoption() {
        let (db, key, _, subject) = setup();
        let conn = db.conn();
        let role_assessment_id = ready_to_issue(conn, None, &subject);
        conn.execute_batch(
            "CREATE TRIGGER fail_role_credential_insert BEFORE INSERT ON credentials \
             BEGIN SELECT RAISE(ABORT, 'injected credential failure'); END;",
        )
        .unwrap();

        let error = issue_role_credential_transactional(
            conn,
            &Issuer::from_key(key.clone()),
            &role_assessment_id,
            &subject,
            "sess_ok",
            NOW,
        )
        .unwrap_err();

        assert!(error.contains("injected credential failure"));
        let organization_did: Option<String> = conn
            .query_row(
                "SELECT o.did FROM organizations o \
                 JOIN role_assessments ra ON ra.org_id = o.id WHERE ra.id = ?1",
                [&role_assessment_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(organization_did, None);
        let status_lists: i64 = conn
            .query_row("SELECT COUNT(*) FROM credential_status_lists", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status_lists, 0);

        conn.execute_batch("DROP TRIGGER fail_role_credential_insert")
            .unwrap();
        issue_role_credential_transactional(
            conn,
            &Issuer::from_key(key.clone()),
            &role_assessment_id,
            &subject,
            "sess_ok",
            NOW,
        )
        .unwrap();
    }

    #[test]
    fn issuance_is_refused_when_the_caller_does_not_hold_the_org_key() {
        // The org's DID belongs to someone else; this caller cannot sign as it
        // and must be refused rather than silently signing personally.
        let (db, key, _, subject) = setup();
        let conn = db.conn();
        let other_org_did = derive_did_key(&SigningKey::from_bytes(&[9u8; 32])).0;
        let ra = ready_to_issue(conn, Some(&other_org_did), &subject);
        let err = issue_role_credential_impl(
            conn,
            &Issuer::from_key(key.clone()),
            &ra,
            &subject,
            "sess_ok",
            NOW,
        )
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
        let ra = ready_to_issue(conn, Some(&owner.0), &owner);
        let err = issue_role_credential_impl(
            conn,
            &Issuer::from_key(key.clone()),
            &ra,
            &owner,
            "sess_ok",
            NOW,
        )
        .unwrap_err();
        assert!(err.contains("to itself"), "unexpected: {err}");
    }

    /// Record an attempt for `role` by `subject` on `session`. `passed` is
    /// the grade (`None` leaves the attempt ungraded).
    fn seed_attempt(
        conn: &Connection,
        id: &str,
        role: &str,
        subject: &str,
        session: &str,
        passed: Option<bool>,
    ) {
        conn.execute_batch(
            "INSERT OR IGNORE INTO question_banks (id, skill_id, label, ratified) \
             VALUES ('bank', 'skill_rust', 'T', 1);",
        )
        .unwrap();
        let graded_at = passed.map(|_| NOW);
        conn.execute(
            "INSERT INTO assessment_attempts \
             (id, subject_did, bank_id, skill_id, seed, question_ids, option_orders, \
              integrity_session_id, started_at, role_assessment_id, passed, graded_at) \
             VALUES (?1, ?2, 'bank', 'skill_rust', 1, '[]', '[]', ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                subject,
                session,
                NOW,
                role,
                passed.map(i64::from),
                graded_at
            ],
        )
        .unwrap();
    }

    fn issue_on(
        conn: &Connection,
        key: &SigningKey,
        role: &str,
        subject: &Did,
        session: &str,
    ) -> Result<VerifiableCredential, String> {
        issue_role_credential_impl(
            conn,
            &Issuer::from_key(key.clone()),
            role,
            subject,
            session,
            NOW,
        )
    }

    #[test]
    fn sponsor_issuance_refuses_an_unknown_session() {
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let ra = ready_to_issue(conn, Some(&owner.0), &subject);
        let err = issue_on(conn, &key, &ra, &subject, "sess_missing").unwrap_err();
        assert_eq!(
            err,
            "integrity session sess_missing is unknown on this device"
        );
    }

    #[test]
    fn sponsor_issuance_refuses_a_session_that_is_not_an_attempt_for_the_role() {
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let ra = ready_to_issue_role(conn, Some(&owner.0), "SRE L4");
        seed_session(conn, "sess_ok", "completed", 0.95, 0);
        // The subject passed with this session, but for another role.
        let other = ready_to_issue_role(conn, Some(&owner.0), "Other role");
        seed_attempt(conn, "att_other", &other, &subject.0, "sess_ok", Some(true));
        let err = issue_on(conn, &key, &ra, &subject, "sess_ok").unwrap_err();
        assert_eq!(
            err,
            "integrity session sess_ok is not an attempt for this role assessment"
        );
    }

    #[test]
    fn sponsor_issuance_refuses_another_subjects_attempt() {
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let someone_else = derive_did_key(&SigningKey::from_bytes(&[5u8; 32]));
        // Someone else passed the role with this session.
        let ra = ready_to_issue(conn, Some(&owner.0), &someone_else);
        let err = issue_on(conn, &key, &ra, &subject, "sess_ok").unwrap_err();
        assert_eq!(
            err,
            "integrity session sess_ok belongs to another subject's attempt"
        );
    }

    #[test]
    fn sponsor_issuance_refuses_an_ungraded_attempt() {
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let ra = ready_to_issue_role(conn, Some(&owner.0), "SRE L4");
        seed_session(conn, "sess_ok", "completed", 0.95, 0);
        seed_attempt(conn, "att_open", &ra, &subject.0, "sess_ok", None);
        let err = issue_on(conn, &key, &ra, &subject, "sess_ok").unwrap_err();
        assert_eq!(
            err,
            "the attempt for integrity session sess_ok has not been graded"
        );
    }

    #[test]
    fn sponsor_issuance_refuses_a_failed_attempt() {
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let ra = ready_to_issue_role(conn, Some(&owner.0), "SRE L4");
        seed_session(conn, "sess_ok", "completed", 0.95, 0);
        seed_attempt(conn, "att_failed", &ra, &subject.0, "sess_ok", Some(false));
        let err = issue_on(conn, &key, &ra, &subject, "sess_ok").unwrap_err();
        assert_eq!(
            err,
            "the attempt for integrity session sess_ok did not pass"
        );
    }

    #[test]
    fn sponsor_issuance_accepts_the_subjects_passed_attempt_for_the_role() {
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let ra = ready_to_issue(conn, Some(&owner.0), &subject);
        // Another candidate's failed attempt at the same role does not interfere.
        let someone_else = derive_did_key(&SigningKey::from_bytes(&[5u8; 32]));
        seed_session(conn, "sess_other", "completed", 0.95, 0);
        seed_attempt(
            conn,
            "att_other",
            &ra,
            &someone_else.0,
            "sess_other",
            Some(false),
        );

        let vc = issue_on(conn, &key, &ra, &subject, "sess_ok").unwrap();
        assert_eq!(vc.credential_subject.id, subject);
        assert_eq!(
            vc.integrity.expect("integrity embedded").session_id,
            "sess_ok"
        );
    }

    /// A finished session `sess_att` on the subject's iPhone, its App Attest
    /// key attesting the session nonce, and the subject's assessment
    /// credential for it issued at grading through `attester` (credential-
    /// bound when the phone signs the credential's digest). Returns the
    /// platform world and the credential.
    fn attested_attempt(
        conn: &Connection,
        role: &str,
        attester: &dyn crate::sentinel::attestation::BindingAttester,
        world: &alexandria_verify::attestation::test_support::World,
    ) -> VerifiableCredential {
        use crate::sentinel::attestation::{
            apple_app_id, session_nonce, AttestationPayload, DeviceAttestation, Platform,
        };
        use alexandria_verify::attestation::test_support::{b64, build_assertion};
        const STARTED: &str = "2026-04-12T23:00:00Z";
        let record = DeviceAttestation {
            platform: Platform::Ios,
            nonce_hex: hex::encode(session_nonce("sess_att", STARTED)),
            app_id: apple_app_id(),
            payload: AttestationPayload::AppAttest {
                key_id_b64: world.key_id_b64(),
                attestation_b64: world.attestation_b64(),
                assertion_b64: Some(b64(&build_assertion(world, b"c0ffee", 1))),
                asserted_root: Some("c0ffee".into()),
            },
            captured_at: STARTED.into(),
            companion: None,
        };
        conn.execute(
            "INSERT INTO integrity_sessions
                 (id, status, integrity_score, critical_count, warning_count, assurance_level,
                  attestation_json, commitment_root, started_at, ended_at)
             VALUES ('sess_att', 'completed', 0.95, 0, 0, 'device_attested', ?1, 'c0ffee',
                     ?2, ?3)",
            params![serde_json::to_string(&record).unwrap(), STARTED, NOW],
        )
        .unwrap();
        let subject_key = SigningKey::from_bytes(&[2u8; 32]);
        let subject = derive_did_key(&subject_key);
        let mut req = crate::credentials::fixtures::sample_request(subject.clone());
        req.credential_type = CredentialType::AssessmentCredential;
        req.integrity_session_id = Some("sess_att".into());
        let assessed = credentials::issue_with(
            conn,
            &Issuer::from_key(subject_key),
            &req,
            NOW,
            &credentials::EvidenceSource {
                trust: trust_of(world),
                attester,
            },
        )
        .unwrap()
        .credential;
        seed_attempt(conn, "att_att", role, &subject.0, "sess_att", Some(true));
        conn.execute(
            "UPDATE assessment_attempts SET credential_id = ?1 WHERE id = 'att_att'",
            params![assessed.id],
        )
        .unwrap();
        assessed
    }

    fn trust_of(
        world: &alexandria_verify::attestation::test_support::World,
    ) -> crate::sentinel::attestation::TrustConfig {
        crate::sentinel::attestation::TrustConfig {
            apple_root_pem: world.root_pem.clone(),
            ..Default::default()
        }
    }

    /// The subject's phone, signing whatever digest it is asked for.
    struct SubjectPhone<'a>(&'a alexandria_verify::attestation::test_support::World);
    impl crate::sentinel::attestation::BindingAttester for SubjectPhone<'_> {
        fn bind(
            &self,
            _: &str,
            _: &crate::sentinel::attestation::DeviceAttestation,
            digest: &[u8; 32],
        ) -> Result<String, String> {
            use alexandria_verify::attestation::test_support::{b64, build_assertion};
            Ok(b64(&build_assertion(self.0, digest, 2)))
        }
    }

    /// The sponsor's device, which holds no key for the candidate's session.
    struct SponsorDevice;
    impl crate::sentinel::attestation::BindingAttester for SponsorDevice {
        fn bind(
            &self,
            _: &str,
            _: &crate::sentinel::attestation::DeviceAttestation,
            _: &[u8; 32],
        ) -> Result<String, String> {
            Err("this device holds no key for the session".into())
        }
    }

    /// A role requiring `device_attested`, with the sponsor's own key.
    fn device_attested_role(conn: &Connection, owner: &Did) -> String {
        let org =
            create_organization_impl(conn, "Acme", "stake_owner", Some(&owner.0), NOW).unwrap();
        create_role_assessment_impl(
            conn,
            &CreateRoleAssessmentRequest {
                org_id: org.id,
                role_title: "SRE L4".into(),
                job_description: None,
                course_id: None,
                skill_ids: vec![],
                issuance_policy: None,
                required_assurance_level: Some("device_attested".into()),
            },
            NOW,
        )
        .unwrap()
        .id
    }

    /// The sponsor's device cannot bind fresh evidence for the candidate's
    /// session, so the role's assurance is judged from the attempt's own
    /// assessment credential: its credential-bound evidence verifies, and the
    /// role credential names it in an `AssessedCredentialEvidence` entry.
    #[test]
    fn a_device_attested_role_issues_on_an_attempt_credential_with_credential_bound_evidence() {
        use alexandria_verify::assessed::AssessedCredentialEvidence;
        use alexandria_verify::attestation::test_support::{build_world, AAGUID_PROD};
        use alexandria_verify::attestation::{
            AttestationOutcome, Platform, VerifiedAssuranceLevel,
        };
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let world = build_world(
            &crate::sentinel::attestation::apple_app_id(),
            crate::sentinel::attestation::session_nonce("sess_att", "2026-04-12T23:00:00Z"),
            AAGUID_PROD,
            0,
        );
        let role = device_attested_role(conn, &owner);
        let assessed = attested_attempt(conn, &role, &SubjectPhone(&world), &world);

        let vc = issue_role_credential_with(
            conn,
            &Issuer::from_key(key.clone()),
            &role,
            &subject,
            "sess_att",
            NOW,
            &credentials::EvidenceSource {
                trust: trust_of(&world),
                attester: &SponsorDevice,
            },
        )
        .unwrap();

        let entry = AssessedCredentialEvidence::find(&vc)
            .unwrap()
            .expect("entry");
        assert_eq!(entry.credential_id, assessed.id);
        assert_eq!(
            entry.integrity_hash,
            crate::domain::vc::integrity_hash_of(&assessed).unwrap()
        );
        assert_eq!(
            entry.attestation,
            AttestationOutcome::VerifiedCredentialBound {
                platform: Platform::Ios,
                companion: false,
            }
        );
        assert_eq!(
            entry.verified_assurance_level,
            VerifiedAssuranceLevel::DeviceAttested
        );
    }

    /// The attempt's credential carries only session-bound evidence: nothing
    /// verified the session's device for that credential, so a role
    /// requiring `device_attested` is refused.
    #[test]
    fn a_device_attested_role_refuses_an_attempt_credential_without_credential_bound_evidence() {
        use alexandria_verify::attestation::test_support::{build_world, AAGUID_PROD};
        let (db, key, owner, subject) = setup();
        let conn = db.conn();
        let world = build_world(
            &crate::sentinel::attestation::apple_app_id(),
            crate::sentinel::attestation::session_nonce("sess_att", "2026-04-12T23:00:00Z"),
            AAGUID_PROD,
            0,
        );
        let role = device_attested_role(conn, &owner);
        attested_attempt(conn, &role, &SponsorDevice, &world);

        let err = issue_role_credential_with(
            conn,
            &Issuer::from_key(key.clone()),
            &role,
            &subject,
            "sess_att",
            NOW,
            &credentials::EvidenceSource {
                trust: trust_of(&world),
                attester: &SponsorDevice,
            },
        )
        .unwrap_err();
        assert!(
            err.contains("assurance level 'local' does not meet required 'device_attested'"),
            "{err}"
        );
    }

    #[test]
    fn an_org_credential_is_a_distinct_issuer_cluster_from_self_assessment() {
        // A learner's own assessment credentials are scored but are not
        // corroboration, so they add no independent issuer cluster. An
        // org-issued skill credential is an independent cluster, so it raises
        // the count aggregation weighs.
        use crate::aggregation::store::recompute_all_impl;
        use crate::credentials::{self, IssueCredentialRequest};
        use crate::domain::vc::SkillClaim;

        let (db, owner_key, _owner, _subject) = setup();
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
            credentials::issue(
                conn,
                &crate::credentials::Issuer::from_key(learner_key.clone()),
                &req,
                NOW,
            )
            .unwrap();
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
        credentials::issue(
            conn,
            &crate::credentials::Issuer::from_key(owner_key.clone()),
            &skill_req,
            NOW,
        )
        .unwrap();
        recompute_all_impl(conn, NOW).unwrap();
        let with_org: i64 = conn
            .query_row(
                "SELECT unique_issuer_clusters FROM derived_skill_states                   WHERE subject_did = ?1 AND skill_id = 'skill:sre'",
                [&learner.0],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(
            self_only, 0,
            "self-issued credentials add no independent cluster"
        );
        assert_eq!(with_org, 1, "the org issuer is one independent cluster");
        assert!(
            with_org > self_only,
            "an independent org issuer must raise the cluster count: {with_org} !> {self_only}"
        );
    }
}
