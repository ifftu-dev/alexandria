use alexandria_verify::exchange::{
    present_credential, shared_credential, verify_share, CredentialRequest, IssuerState,
};
use alexandria_verify::vc::presentation::VerifiablePresentation;
use reqwest::Url;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::clock::now_rfc3339;
use crate::cloud::client;
use crate::commands::holder_pull::{Directory, Problem, PullResult};
use crate::credentials::{IssuancePolicy, Issuer};
use crate::db::executor::DatabaseWorkload;
use crate::org_trust::{self, OrgCheck};
use crate::profile::scope::ProfileState as State;
use crate::AppState;
use alexandria_verify::integrity::ProfileSignal;
use alexandria_verify::org::OrgProof;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryRequest {
    pub directory_url: String,
    pub request: CredentialRequest,
    #[serde(rename = "orgProof")]
    pub org_proof: Option<OrgProof>,
    /// Who sent it, as its domain publishes. Only a verified sender can be
    /// answered.
    pub org: OrgCheck,
}

/// One request as Cloud serves it: the organisation's payload and its proof.
#[derive(Debug, Clone, Deserialize)]
struct RequestEnvelope {
    request: CredentialRequest,
    #[serde(rename = "orgProof")]
    org_proof: Option<OrgProof>,
}

pub(super) async fn directory(
    state: &State<'_, AppState>,
    address: &str,
) -> Result<Directory, String> {
    let directories = super::holder_pull::directories(state).await?;
    let dir = directories
        .into_iter()
        .find(|d| d.url.trim_end_matches('/') == address.trim_end_matches('/'))
        .ok_or("configure this directory before contacting it")?;
    let url = Url::parse(&dir.url).map_err(|e| e.to_string())?;
    if !crate::cloud::is_allowed_origin(&dir.url)
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("directory requires HTTPS, or HTTP on loopback".into());
    }
    Ok(dir)
}

pub(super) use crate::cloud::response_json;

async fn requests(
    state: &State<'_, AppState>,
    dir: &Directory,
) -> Result<Vec<RequestEnvelope>, String> {
    let (key, did) = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .into_parts();
    let path = format!("/api/assessment-requests/for/{}", did.as_str());
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = uuid::Uuid::new_v4().to_string();
    let proof = super::holder_pull::proof(&key, "GET", &path, timestamp, &nonce);
    let response = client()?
        .get(format!("{}{path}", dir.url.trim_end_matches('/')))
        .header("x-alexandria-timestamp", timestamp.to_string())
        .header("x-alexandria-nonce", nonce)
        .header("x-alexandria-proof", proof)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(response_json(response).await?).map_err(|e| e.to_string())
}

pub(crate) fn validate_request(request: &CredentialRequest, did: &str) -> Result<(), String> {
    request.check()?;
    let snapshot = crate::db::bundled::taxonomy_snapshot()?;
    if request.subject_did != did {
        return Err("request belongs to another identity".into());
    }
    if request.network_id != snapshot.network_id || request.taxonomy_digest != snapshot.digest {
        return Err("the organisation's skill list does not match this app's".into());
    }
    if request.expires_at <= chrono::Utc::now().timestamp() {
        return Err("request has expired".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn exchange_requests(
    state: State<'_, AppState>,
) -> Result<PullResult<DirectoryRequest>, String> {
    let dirs = super::holder_pull::directories(&state).await?;
    let mut items = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs {
        match async {
            let dir = directory(&state, &dir.url).await?;
            requests(&state, &dir).await
        }
        .await
        {
            Ok(found) => {
                for envelope in found {
                    let org = org_trust::check_item(
                        &state,
                        &envelope.request,
                        envelope.org_proof.as_ref(),
                    )
                    .await;
                    items.push(DirectoryRequest {
                        directory_url: dir.url.clone(),
                        request: envelope.request,
                        org_proof: envelope.org_proof,
                        org,
                    });
                }
            }
            Err(detail) => problems.push(Problem {
                directory: dir.name,
                detail,
            }),
        }
    }
    Ok(PullResult { items, problems })
}

/// Fetch the request `request_id` from the directory again, require that the
/// organisation's domain vouches for it and that it is this identity's, and
/// store it as verified ([`crate::exchange_requests`]).
async fn verified_request(
    state: &State<'_, AppState>,
    directory_url: &str,
    request_id: &str,
) -> Result<CredentialRequest, String> {
    let dir = directory(state, directory_url).await?;
    let envelope = requests(state, &dir)
        .await?
        .into_iter()
        .find(|r| r.request.id == request_id)
        .ok_or("request is not available")?;
    org_trust::check_item(state, &envelope.request, envelope.org_proof.as_ref())
        .await
        .require()?;
    let request = envelope.request;
    let did = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .did()
        .clone();
    validate_request(&request, did.as_str())?;
    let stored = request.clone();
    let directory_url = dir.url.clone();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "exchange.store-request",
            move |db| {
                crate::exchange_requests::store(
                    db.conn(),
                    &directory_url,
                    &stored,
                    chrono::Utc::now().timestamp(),
                    &now_rfc3339(),
                )
            },
        )
        .await?;
    Ok(request)
}

/// What an attempt answering a credential request runs under, for the
/// assessment page to show and prepare for before monitoring starts.
#[derive(Debug, Clone, Serialize)]
pub struct RequestAttemptTarget {
    pub request_id: String,
    pub organization: String,
    pub role_label: String,
    /// The policy the organisation will judge the answer against.
    pub issuance_policy: Option<IssuancePolicy>,
    /// True when the policy's minimum trust profile runs the camera.
    pub camera_required: bool,
    /// True when the policy's minimum trust profile needs device attestation.
    pub attestation_required: bool,
}

impl RequestAttemptTarget {
    fn of(request: &CredentialRequest) -> Self {
        let floor = request
            .issuance_policy
            .as_ref()
            .and_then(|p| p.min_trust_profile);
        let needs = |signal| floor.is_some_and(|f| f.includes(signal));
        Self {
            request_id: request.id.clone(),
            organization: request.organization.clone(),
            role_label: request.role_label.clone(),
            issuance_policy: request.issuance_policy.clone(),
            camera_required: needs(ProfileSignal::Camera),
            attestation_required: needs(ProfileSignal::DeviceAttestation),
        }
    }
}

/// Verify and store a credential request before an attempt answering it
/// starts, so the trust profile chosen at session start takes the request's
/// floor from what this device verified
/// ([`crate::sentinel::trust_profile::ProfileRequest::exchange_request_id`]).
#[tauri::command]
pub async fn exchange_prepare_assessment(
    state: State<'_, AppState>,
    directory_url: String,
    request_id: String,
) -> Result<RequestAttemptTarget, String> {
    let request = verified_request(&state, &directory_url, &request_id).await?;
    Ok(RequestAttemptTarget::of(&request))
}

/// Start (or resume) the attempt answering a credential request. The request
/// is fetched and verified again; its issuance policy's floor is judged
/// before any item and its probe policy runs after submission, as a sponsor
/// role's do.
#[tauri::command]
pub async fn exchange_start_assessment(
    state: State<'_, AppState>,
    directory_url: String,
    request_id: String,
    integrity_session_id: String,
) -> Result<super::assessment::StartedAttempt, String> {
    let request = verified_request(&state, &directory_url, &request_id).await?;
    let seed: u64 = rand::random();
    let now = now_rfc3339();
    let attempt_id = format!("{now}-{seed}");
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "exchange.start",
            move |db| {
                start_request_attempt(db, &request, integrity_session_id, seed, attempt_id, now)
            },
        )
        .await
}

/// [`exchange_start_assessment`] on the database: start the attempt under the
/// request's policy and bind it to the request.
pub(crate) fn start_request_attempt(
    db: &crate::db::Database,
    request: &CredentialRequest,
    integrity_session_id: String,
    seed: u64,
    attempt_id: String,
    now: String,
) -> Result<super::assessment::StartedAttempt, String> {
    let tx = db
        .conn()
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let attempt = super::assessment::start_request_attempt_db(
        db,
        request.skill_id.clone(),
        Some(integrity_session_id),
        request.issuance_policy.as_ref(),
        seed,
        attempt_id,
        now,
    )?;
    let binding = format!("request:{}:{}", request.id, request.nonce);
    let previous: Option<String> = db
        .conn()
        .query_row(
            "SELECT exchange_binding FROM assessment_attempts WHERE id=?1",
            [&attempt.attempt_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if previous.as_ref().is_some_and(|b| b != &binding) {
        return Err("attempt already belongs to another request".into());
    }
    db.conn()
        .execute(
            "UPDATE assessment_attempts SET exchange_binding=?2 WHERE id=?1",
            params![attempt.attempt_id, binding],
        )
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(attempt)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareableCredential {
    pub id: String,
    pub issuer: String,
    pub issued_at: String,
    /// Whether the request's issuance policy, judged by the shared rule
    /// ([`alexandria_verify::policy::evaluate`]) as the organisation will,
    /// accepts this credential. True when the request carries no policy.
    pub meets_policy: bool,
    /// The first unmet requirement's stable code (a
    /// [`alexandria_verify::policy::PolicyRefusal`] code,
    /// `no_integrity_assertion`, or `unreadable_evidence`), when refused.
    pub refusal_code: Option<String>,
    /// The refusal in words, when refused.
    pub refusal: Option<String>,
}

#[tauri::command]
pub async fn exchange_credentials(
    state: State<'_, AppState>,
    request: CredentialRequest,
) -> Result<Vec<ShareableCredential>, String> {
    let did = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .did()
        .clone();
    validate_request(&request, did.as_str())?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "exchange.credentials",
            move |db| {
                shareable_credentials(
                    db.conn(),
                    &request,
                    crate::sentinel::attestation::profile_trust_inputs(),
                )
            },
        )
        .await
}

/// The holder's credentials that can answer `request`, newest first, each
/// judged against the request's issuance policy with the attestation
/// outcome established against `trust`. A refused credential stays listed
/// with its reason: the policy decides whether the organisation issues its
/// own credential, not whether the candidate may share.
pub(crate) fn shareable_credentials(
    conn: &rusqlite::Connection,
    request: &CredentialRequest,
    trust: &alexandria_verify::attestation::AttestationTrust,
) -> Result<Vec<ShareableCredential>, String> {
    let mut query = conn
        .prepare(
            "SELECT id,issuer_did,issuance_date,signed_vc_json FROM credentials
              WHERE subject_did=?1 AND skill_id=?2 AND revoked=0 ORDER BY issuance_date DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = query
        .query_map(params![request.subject_did, request.skill_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let binding = format!("request:{}:{}", request.id, request.nonce);
    let mut result = Vec::new();
    for row in rows {
        let (id, issuer, issued_at, json) = row.map_err(|e| e.to_string())?;
        let vc: alexandria_verify::vc::VerifiableCredential =
            serde_json::from_str(&json).map_err(|e| e.to_string())?;
        if request.require_new_assessment
            && !alexandria_verify::vc::SkillClaim::extract(&vc.credential_subject)
                .is_some_and(|c| c.evidence_refs.contains(&binding))
        {
            continue;
        }
        let refused = request
            .issuance_policy
            .as_ref()
            .and_then(|policy| policy_refusal(policy, &vc, trust));
        result.push(ShareableCredential {
            id,
            issuer,
            issued_at,
            meets_policy: refused.is_none(),
            refusal_code: refused.as_ref().map(|(code, _)| code.clone()),
            refusal: refused.map(|(_, message)| message),
        });
    }
    Ok(result)
}

/// The first requirement of `policy` that `vc` does not meet, as a code and
/// its words; `None` when the policy accepts it. A role that requires
/// issuer-held answers refuses every shared credential with the custody code
/// before anything else, as the organisation's share path does (ADR 0009):
/// only a hosted attempt meets it.
fn policy_refusal(
    policy: &IssuancePolicy,
    vc: &alexandria_verify::vc::VerifiableCredential,
    trust: &alexandria_verify::attestation::AttestationTrust,
) -> Option<(String, String)> {
    use alexandria_verify::integrity::AnswerCustody;
    if policy.required_answer_custody == Some(AnswerCustody::Issuer) {
        let refusal = alexandria_verify::policy::PolicyRefusal::CustodyBelowRequired {
            custody: AnswerCustody::Device.as_str().into(),
            required: AnswerCustody::Issuer.as_str().into(),
        };
        return Some((refusal.code().into(), refusal.to_string()));
    }
    let Some(assertion) = &vc.integrity else {
        return Some((
            "no_integrity_assertion".into(),
            "the credential carries no integrity result".into(),
        ));
    };
    match crate::credentials::issue::judge_shared_rule(policy, assertion, vc, trust) {
        Ok(()) => None,
        Err(crate::credentials::CredentialError::Policy(refusal)) => {
            Some((refusal.code().into(), refusal.to_string()))
        }
        Err(other) => Some(("unreadable_evidence".into(), other.to_string())),
    }
}

#[tauri::command]
pub async fn exchange_preview(
    state: State<'_, AppState>,
    request: CredentialRequest,
    org_proof: Option<OrgProof>,
    credential_id: String,
) -> Result<VerifiablePresentation, String> {
    let (key, did) = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .into_parts();
    validate_request(&request, did.as_str())?;
    // The presentation is bound to this organisation and is what leaves the
    // device, so it is only made for one whose domain vouches for it.
    org_trust::check_item(&state, &request, org_proof.as_ref())
        .await
        .require()?;
    state.db_executor.execute(DatabaseWorkload::Learner,state.profile_lease(),"exchange.preview",move |db| {
        let row: Option<(String,bool,bool,Option<String>,bool)> = db.conn().query_row(
            "SELECT signed_vc_json,revoked,suspended,suspended_until,EXISTS(SELECT 1 FROM credentials n WHERE n.supersedes=c.id AND n.issuer_did=c.issuer_did) FROM credentials c WHERE id=?1 AND subject_did=?2",
            params![credential_id,did.as_str()], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(|e|e.to_string())?;
        let (json,revoked,suspended,suspended_until,superseded) = row.ok_or("credential not found for this identity")?;
        let credential:alexandria_verify::vc::VerifiableCredential = serde_json::from_str(&json).map_err(|e|e.to_string())?;
        let issuer_state = (credential.issuer==did).then_some(IssuerState {revoked,suspended,suspended_until,superseded});
        let now = chrono::Utc::now().timestamp();
        let presentation = present_credential(request.clone(), credential, issuer_state, now, &key)?;
        verify_share(&presentation, &request, now, &now_rfc3339())?;
        Ok(presentation)
    }).await
}

#[tauri::command]
pub async fn exchange_send(
    state: State<'_, AppState>,
    directory_url: String,
    signed: VerifiablePresentation,
) -> Result<serde_json::Value, String> {
    let dir = directory(&state, &directory_url).await?;
    let did = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .did()
        .clone();
    let request = shared_credential(&signed)?.request;
    validate_request(&request, did.as_str())?;
    // Checked again here rather than trusted from the preview: this is the
    // step that sends, and the request it answers must still be one the
    // organisation's domain vouches for.
    let envelope = requests(&state, &dir)
        .await?
        .into_iter()
        .find(|e| e.request == request)
        .ok_or("this request is no longer open")?;
    org_trust::check_item(&state, &envelope.request, envelope.org_proof.as_ref())
        .await
        .require()?;
    verify_share(
        &signed,
        &request,
        chrono::Utc::now().timestamp(),
        &now_rfc3339(),
    )?;
    let id = uuid::Uuid::parse_str(&request.id).map_err(|e| e.to_string())?;
    if !state.profile_lease().is_current() {
        return Err("profile changed".into());
    }
    let response = client()?
        .post(format!(
            "{}/api/assessment-requests/{id}/share",
            dir.url.trim_end_matches('/')
        ))
        .json(&signed)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    response_json(response).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::exchange_requests::tests::request;
    use alexandria_verify::integrity::TrustProfile;
    use alexandria_verify::policy::ProbePolicy;

    const SUBJECT: &str = "did:key:z6MkExample";

    /// Store a credential for `skill_vector` whose integrity session ran
    /// `profile` with `signals`, cleanly and with a full score.
    fn store_credential(conn: &rusqlite::Connection, id: &str, profile: &str, signals: &[&str]) {
        let raw: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/alexandria-verify/tests/vectors/01-valid.json"
        ))
        .unwrap();
        let mut vc = raw["credential"].clone();
        vc["id"] = serde_json::json!(id);
        vc["credentialSubject"]["id"] = serde_json::json!(SUBJECT);
        vc["integrity"] = serde_json::json!({
            "sessionId": format!("session-{id}"),
            "status": "completed",
            "integrityScore": 1.0,
            "criticalCount": 0,
            "warningCount": 0,
            "assuranceLevel": "local",
            "generatedAt": "2026-06-01T00:00:00Z",
            "trustProfile": profile,
            "signalsRun": signals,
        });
        conn.execute(
            "INSERT INTO credentials (id, issuer_did, subject_did, credential_type, claim_kind,
                 skill_id, issuance_date, signed_vc_json, integrity_hash)
             VALUES (?1, 'did:key:issuer', ?2, 'AssessmentCredential', 'skill', 'skill_vector',
                 '2026-06-01T00:00:00Z', ?3, 'hash')",
            params![id, SUBJECT, vc.to_string()],
        )
        .unwrap();
    }

    fn camera_request() -> CredentialRequest {
        let mut req = request(
            "req-1",
            "skill_vector",
            Some(IssuancePolicy {
                min_trust_profile: Some(TrustProfile::Camera),
                ..IssuancePolicy::default()
            }),
        );
        req.require_new_assessment = false;
        req
    }

    #[test]
    fn the_picker_marks_a_credential_below_the_floor_as_refused_with_its_code() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        store_credential(db.conn(), "urn:core", "core", &["core"]);
        store_credential(db.conn(), "urn:camera", "camera", &["core", "camera"]);
        let trust = crate::sentinel::attestation::TrustConfig::default().inputs();

        let listed = shareable_credentials(db.conn(), &camera_request(), &trust).unwrap();
        let by_id = |id: &str| listed.iter().find(|c| c.id == id).unwrap();
        let core = by_id("urn:core");
        assert!(!core.meets_policy);
        assert_eq!(
            core.refusal_code.as_deref(),
            Some("trust_profile_below_floor")
        );
        assert!(core.refusal.as_deref().unwrap().contains("'camera' floor"));
        let camera = by_id("urn:camera");
        assert!(camera.meets_policy);
        assert_eq!(camera.refusal_code, None);

        // Without a policy every credential meets it.
        let mut plain = camera_request();
        plain.issuance_policy = None;
        let listed = shareable_credentials(db.conn(), &plain, &trust).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|c| c.meets_policy));
    }

    #[test]
    fn the_picker_refuses_a_credential_without_an_integrity_result_under_a_policy() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        store_credential(db.conn(), "urn:bare", "core", &["core"]);
        db.conn()
            .execute(
                "UPDATE credentials SET signed_vc_json = json_remove(signed_vc_json, '$.integrity')",
                [],
            )
            .unwrap();
        let mut req = camera_request();
        req.issuance_policy = Some(IssuancePolicy {
            probe_policy: Some(ProbePolicy::Reseed),
            ..IssuancePolicy::default()
        });
        let trust = crate::sentinel::attestation::TrustConfig::default().inputs();
        let listed = shareable_credentials(db.conn(), &req, &trust).unwrap();
        assert_eq!(
            listed[0].refusal_code.as_deref(),
            Some("no_integrity_assertion")
        );
    }

    /// A role that requires issuer-held answers refuses the share path with
    /// the custody code before anything else, as the organisation does
    /// (ADR 0009): no shared credential can meet it, whatever its assertion
    /// states and even with none.
    #[test]
    fn a_role_requiring_issuer_custody_refuses_every_shared_credential_first() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        store_credential(db.conn(), "urn:stating-issuer", "core", &["core"]);
        db.conn()
            .execute(
                "UPDATE credentials SET signed_vc_json =
                     json_set(signed_vc_json, '$.integrity.answerCustody', 'issuer')",
                [],
            )
            .unwrap();
        store_credential(db.conn(), "urn:bare", "core", &["core"]);
        db.conn()
            .execute(
                "UPDATE credentials SET signed_vc_json = json_remove(signed_vc_json, '$.integrity')
                  WHERE id = 'urn:bare'",
                [],
            )
            .unwrap();
        let mut req = camera_request();
        req.issuance_policy = Some(IssuancePolicy {
            required_answer_custody: Some(alexandria_verify::integrity::AnswerCustody::Issuer),
            min_trust_profile: Some(TrustProfile::Camera),
            ..IssuancePolicy::default()
        });
        let trust = crate::sentinel::attestation::TrustConfig::default().inputs();
        let listed = shareable_credentials(db.conn(), &req, &trust).unwrap();
        assert_eq!(listed.len(), 2);
        for c in &listed {
            assert!(!c.meets_policy, "{}", c.id);
            assert_eq!(
                c.refusal_code.as_deref(),
                Some("custody_below_required"),
                "{}",
                c.id
            );
        }
    }

    #[test]
    fn a_request_naming_a_model_connection_fails_validation() {
        let mut req = camera_request();
        req.issuance_policy.as_mut().unwrap().probe_connection_id = Some("conn".into());
        let err = validate_request(&req, SUBJECT).unwrap_err();
        assert!(err.contains("probe_connection_id"), "{err}");
    }
}
