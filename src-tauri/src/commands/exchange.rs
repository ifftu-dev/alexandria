use alexandria_learning_contracts::TaxonomySnapshot;
use alexandria_verify::exchange::{
    present_credential, shared_credential, verify_share, CredentialRequest, IssuerState,
};
use alexandria_verify::vc::presentation::VerifiablePresentation;
use reqwest::Url;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::commands::credentials::{load_issuer_key, now_rfc3339};
use crate::commands::holder_pull::{Directory, Problem, PullResult};
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryRequest {
    pub directory_url: String,
    pub request: CredentialRequest,
}

pub(super) async fn directory(
    state: &State<'_, AppState>,
    address: &str,
) -> Result<Directory, String> {
    let directories = super::holder_pull::list_directories(state.clone()).await?;
    let dir = directories
        .into_iter()
        .find(|d| d.url.trim_end_matches('/') == address.trim_end_matches('/'))
        .ok_or("configure this directory before contacting it")?;
    let url = Url::parse(&dir.url).map_err(|e| e.to_string())?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("directory requires HTTPS, or HTTP on loopback".into());
    }
    Ok(dir)
}

pub(super) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}

pub(super) async fn response_json(
    mut response: reqwest::Response,
) -> Result<serde_json::Value, String> {
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("directory response is too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return Err(format!(
            "directory returned {status}: {}",
            String::from_utf8_lossy(&bytes)
        ));
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

async fn requests(
    state: &State<'_, AppState>,
    dir: &Directory,
) -> Result<Vec<CredentialRequest>, String> {
    let (key, did) = load_issuer_key(state).await?;
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
    let profile = crate::network_profile::embedded_preprod().map_err(|e| e.to_string())?;
    let snapshot = TaxonomySnapshot::from_reference(
        &profile.network_id,
        "bundled-v1",
        include_str!("../../../bootstrap/public_taxonomy.json"),
    )
    .map_err(|e| e.to_string())?;
    if request.subject_did != did {
        return Err("request belongs to another identity".into());
    }
    if request.network_id != snapshot.network_id || request.taxonomy_digest != snapshot.digest {
        return Err("directory and app taxonomy do not match".into());
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
    let dirs = super::holder_pull::list_directories(state.clone()).await?;
    let mut items = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs {
        match async {
            let dir = directory(&state, &dir.url).await?;
            requests(&state, &dir).await
        }
        .await
        {
            Ok(found) => items.extend(found.into_iter().map(|request| DirectoryRequest {
                directory_url: dir.url.clone(),
                request,
            })),
            Err(detail) => problems.push(Problem {
                directory: dir.name,
                detail,
            }),
        }
    }
    Ok(PullResult { items, problems })
}

#[tauri::command]
pub async fn exchange_start_assessment(
    state: State<'_, AppState>,
    directory_url: String,
    request_id: String,
    integrity_session_id: String,
) -> Result<super::assessment::StartedAttempt, String> {
    let dir = directory(&state, &directory_url).await?;
    let request = requests(&state, &dir)
        .await?
        .into_iter()
        .find(|r| r.id == request_id)
        .ok_or("request is not available")?;
    let (_, did) = load_issuer_key(&state).await?;
    validate_request(&request, did.as_str())?;
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
                let tx = db
                    .conn()
                    .unchecked_transaction()
                    .map_err(|e| e.to_string())?;
                let attempt = super::assessment::start_attempt_db(
                    db,
                    request.skill_id,
                    Some(integrity_session_id),
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
            },
        )
        .await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareableCredential {
    pub id: String,
    pub issuer: String,
    pub issued_at: String,
}

#[tauri::command]
pub async fn exchange_credentials(
    state: State<'_, AppState>,
    request: CredentialRequest,
) -> Result<Vec<ShareableCredential>, String> {
    let (_, did) = load_issuer_key(&state).await?;
    validate_request(&request, did.as_str())?;
    state.db_executor.execute(DatabaseWorkload::Learner,state.profile_lease(),"exchange.credentials",move |db| {
        let mut query = db.conn().prepare("SELECT id,issuer_did,issuance_date,signed_vc_json FROM credentials WHERE subject_did=?1 AND skill_id=?2 AND revoked=0 ORDER BY issuance_date DESC").map_err(|e|e.to_string())?;
        let rows = query.query_map(params![request.subject_did,request.skill_id],|r| Ok((ShareableCredential{id:r.get(0)?,issuer:r.get(1)?,issued_at:r.get(2)?},r.get::<_,String>(3)?))).map_err(|e|e.to_string())?;
        let mut result = Vec::new();
        for row in rows {
            let (summary,json) = row.map_err(|e|e.to_string())?;
            let vc:alexandria_verify::vc::VerifiableCredential = serde_json::from_str(&json).map_err(|e|e.to_string())?;
            let binding = format!("request:{}:{}",request.id,request.nonce);
            if !request.require_new_assessment || alexandria_verify::vc::SkillClaim::extract(&vc.credential_subject).is_some_and(|c|c.evidence_refs.contains(&binding)) { result.push(summary); }
        }
        Ok(result)
    }).await
}

#[tauri::command]
pub async fn exchange_preview(
    state: State<'_, AppState>,
    request: CredentialRequest,
    credential_id: String,
) -> Result<VerifiablePresentation, String> {
    let (key, did) = load_issuer_key(&state).await?;
    validate_request(&request, did.as_str())?;
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
    let (_, did) = load_issuer_key(&state).await?;
    let request = shared_credential(&signed)?.request;
    validate_request(&request, did.as_str())?;
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
