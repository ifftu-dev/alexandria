// SPDX-License-Identifier: AGPL-3.0-or-later
//! Organisations asking for access, and the access the learner has given.
//!
//! An organisation asks to see some of a learner's skills. The learner can
//! share them once, or share them and keep them in sync, and can stop sharing
//! at any time. Every grant is a statement the learner signs
//! ([`alexandria_verify::access`]); the organisation's copy of the data is a
//! signed record narrowed to the granted skills, built here, on this device.
//!
//! "Keep in sync" means this device sends a fresh record whenever what it
//! would send changes, and refreshes it weekly so it never lapses. Stopping
//! is a signed revoke, after which Cloud deletes the record it held.
//!
//! The planning, sending and recording of sync are three separate steps so
//! that the commands, the background pass and the post-issuance hook all run
//! the same code without holding the database across the network.

use alexandria_verify::access::{
    self, AccessRequest, GrantAction, GrantStatement, SignedGrantStatement, GRANT_FORMAT,
};
use alexandria_verify::org::OrgProof;
use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::clock::now_rfc3339;
use crate::commands::credentials::load_issuer_key;
use crate::commands::exchange::{client, directory, response_json};
use crate::commands::holder_pull::{Problem, PullResult};
use crate::db::executor::DatabaseWorkload;
use crate::domain::talent_index::{sign_record, SignedTalentIndexRecord, TalentIndexRecord};
use crate::org_trust::{self, OrgCheck};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// A synced record is resent at least this often even when nothing changed,
/// so the copy an organisation holds never reaches its 30-day expiry.
pub const RESEND_AFTER_SECS: i64 = 7 * 86_400;

#[derive(Debug, Clone, Deserialize)]
struct AccessEnvelope {
    request: AccessRequest,
    #[serde(rename = "orgProof")]
    org_proof: Option<OrgProof>,
}

/// One skill an organisation asked about, as the learner can answer it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillChoice {
    pub skill_id: String,
    pub name: String,
    /// Whether there is evidence to share. A skill without any is shown but
    /// cannot be granted: there would be nothing to send.
    pub has_evidence: bool,
    pub bloom_level: Option<u8>,
    pub level: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessRequestItem {
    pub directory_url: String,
    pub request: AccessRequest,
    pub org_proof: Option<OrgProof>,
    pub org: OrgCheck,
    pub skills: Vec<SkillChoice>,
}

/// A grant as the learner sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantView {
    pub grant_id: String,
    pub org_domain: String,
    pub org_name: String,
    pub purpose: String,
    pub skills: Vec<SkillChoice>,
    pub sync: bool,
    pub granted_at: i64,
    pub synced_at: Option<i64>,
    pub sync_error: Option<String>,
    pub revoked_at: Option<i64>,
    /// When the organisation last looked, as Cloud reports it.
    pub last_viewed_at: Option<String>,
    pub view_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantsView {
    pub grants: Vec<GrantView>,
    pub problems: Vec<Problem>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub sent: usize,
    pub unchanged: usize,
    pub failed: Vec<String>,
}

// --- local records ---------------------------------------------------------

#[derive(Debug, Clone)]
struct GrantRow {
    grant_id: String,
    cloud_url: String,
    org_domain: String,
    org_name: String,
    purpose: String,
    skill_ids: Vec<String>,
    sync: bool,
    statement: GrantStatement,
    granted_at: i64,
    revoked_at: Option<i64>,
    snapshot_digest: Option<String>,
    synced_at: Option<i64>,
    sync_error: Option<String>,
}

const GRANT_COLUMNS: &str = "grant_id, cloud_url, org_domain, org_name, purpose, skill_ids_json, \
     sync, statement_json, granted_at, revoked_at, snapshot_digest, synced_at, sync_error";

fn map_grant(r: &rusqlite::Row<'_>) -> rusqlite::Result<GrantRow> {
    let skills: String = r.get(5)?;
    let statement: String = r.get(7)?;
    let json_err = |e: serde_json::Error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    };
    Ok(GrantRow {
        grant_id: r.get(0)?,
        cloud_url: r.get(1)?,
        org_domain: r.get(2)?,
        org_name: r.get(3)?,
        purpose: r.get(4)?,
        skill_ids: serde_json::from_str(&skills).map_err(json_err)?,
        sync: r.get(6)?,
        statement: serde_json::from_str(&statement).map_err(json_err)?,
        granted_at: r.get(8)?,
        revoked_at: r.get(9)?,
        snapshot_digest: r.get(10)?,
        synced_at: r.get(11)?,
        sync_error: r.get(12)?,
    })
}

fn grants(conn: &Connection) -> Result<Vec<GrantRow>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {GRANT_COLUMNS} FROM org_grants ORDER BY revoked_at IS NOT NULL, granted_at DESC"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], map_grant).map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

fn grant(conn: &Connection, grant_id: &str) -> Result<GrantRow, String> {
    conn.query_row(
        &format!("SELECT {GRANT_COLUMNS} FROM org_grants WHERE grant_id = ?1"),
        params![grant_id],
        map_grant,
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "no such grant".to_string())
}

fn insert_grant(conn: &Connection, row: &GrantRow) -> Result<(), String> {
    conn.execute(
        &format!("INSERT INTO org_grants ({GRANT_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)"),
        params![
            row.grant_id,
            row.cloud_url,
            row.org_domain,
            row.org_name,
            row.purpose,
            serde_json::to_string(&row.skill_ids).map_err(|e| e.to_string())?,
            row.sync,
            serde_json::to_string(&row.statement).map_err(|e| e.to_string())?,
            row.granted_at,
            row.revoked_at,
            row.snapshot_digest,
            row.synced_at,
            row.sync_error,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn record_statement(conn: &Connection, statement: &GrantStatement) -> Result<(), String> {
    let revoked = (statement.action == GrantAction::Revoke).then_some(statement.created);
    conn.execute(
        "UPDATE org_grants SET statement_json = ?2, sync = ?3,
                revoked_at = COALESCE(revoked_at, ?4)
          WHERE grant_id = ?1",
        params![
            statement.grant_id,
            serde_json::to_string(statement).map_err(|e| e.to_string())?,
            statement.sync,
            revoked,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// What the record says, without when it expires. Two records with the same
/// digest show an organisation the same thing, so only a change is resent.
pub fn snapshot_digest(record: &TalentIndexRecord) -> Result<String, String> {
    let value = json!({
        "subjectDid": record.subject_did,
        "skills": record.skills,
        "displayName": record.display_name,
    });
    let bytes = serde_json_canonicalizer::to_vec(&value).map_err(|e| e.to_string())?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn skill_choices(conn: &Connection, did: &str, ids: &[String]) -> Result<Vec<SkillChoice>, String> {
    let now = now_rfc3339();
    let preview = super::talent_index::get_talent_index_preview_impl(conn, did, &now)?;
    ids.iter()
        .map(|id| {
            if let Some(c) = preview.candidates.iter().find(|c| &c.skill_id == id) {
                return Ok(SkillChoice {
                    skill_id: id.clone(),
                    name: c.name.clone(),
                    has_evidence: true,
                    bloom_level: Some(c.bloom_level),
                    level: Some(c.level),
                });
            }
            let name: Option<String> = conn
                .query_row("SELECT name FROM skills WHERE id = ?1", params![id], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(|e| e.to_string())?;
            Ok(SkillChoice {
                skill_id: id.clone(),
                name: name.unwrap_or_else(|| id.clone()),
                has_evidence: false,
                bloom_level: None,
                level: None,
            })
        })
        .collect()
}

// --- sync: plan, send, record ------------------------------------------------

/// One record that needs sending.
#[derive(Debug, Clone)]
pub struct PendingSnapshot {
    pub grant_id: String,
    pub cloud_url: String,
    pub digest: String,
    pub signed: SignedTalentIndexRecord,
}

/// Decide which synced grants need a fresh record, and sign those records.
pub fn plan_sync(
    conn: &Connection,
    key: &SigningKey,
    did: &str,
    now_unix: i64,
    now: &str,
) -> Result<(Vec<PendingSnapshot>, usize), String> {
    let mut pending = Vec::new();
    let mut unchanged = 0;
    for row in grants(conn)?
        .into_iter()
        .filter(|g| g.sync && g.revoked_at.is_none())
    {
        let Some(record) =
            super::talent_index::grant_snapshot_record(conn, did, &row.skill_ids, now)?
        else {
            unchanged += 1;
            continue;
        };
        let digest = snapshot_digest(&record)?;
        let fresh = row
            .synced_at
            .is_some_and(|t| now_unix - t < RESEND_AFTER_SECS);
        if row.snapshot_digest.as_deref() == Some(digest.as_str())
            && fresh
            && row.sync_error.is_none()
        {
            unchanged += 1;
            continue;
        }
        pending.push(PendingSnapshot {
            grant_id: row.grant_id,
            cloud_url: row.cloud_url,
            digest,
            signed: sign_record(&record, key, now)?,
        });
    }
    Ok((pending, unchanged))
}

/// Send planned records. Never touches the database.
pub async fn send_snapshots(pending: &[PendingSnapshot]) -> Vec<(String, Result<(), String>)> {
    let mut results = Vec::new();
    let Ok(http) = client() else {
        return pending
            .iter()
            .map(|p| (p.grant_id.clone(), Err("no HTTP client".to_string())))
            .collect();
    };
    for p in pending {
        let outcome = async {
            let response = http
                .put(format!(
                    "{}/api/grants/{}/snapshot",
                    p.cloud_url.trim_end_matches('/'),
                    p.grant_id
                ))
                .json(&p.signed)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            response_json(response).await.map(|_| ())
        }
        .await;
        results.push((p.grant_id.clone(), outcome));
    }
    results
}

/// Record what was sent.
pub fn record_sync(
    conn: &Connection,
    pending: &[PendingSnapshot],
    results: &[(String, Result<(), String>)],
    now_unix: i64,
) -> Result<SyncReport, String> {
    let mut report = SyncReport::default();
    for (grant_id, outcome) in results {
        match outcome {
            Ok(()) => {
                let digest = pending
                    .iter()
                    .find(|p| &p.grant_id == grant_id)
                    .map(|p| p.digest.clone());
                conn.execute(
                    "UPDATE org_grants SET snapshot_digest = ?2, synced_at = ?3, sync_error = NULL
                      WHERE grant_id = ?1",
                    params![grant_id, digest, now_unix],
                )
                .map_err(|e| e.to_string())?;
                report.sent += 1;
            }
            Err(e) => {
                conn.execute(
                    "UPDATE org_grants SET sync_error = ?2 WHERE grant_id = ?1",
                    params![grant_id, e],
                )
                .map_err(|e| e.to_string())?;
                report.failed.push(format!("{grant_id}: {e}"));
            }
        }
    }
    Ok(report)
}

/// The listing a learner asked to keep up to date, when it changed.
pub fn plan_listing(
    conn: &Connection,
    key: &SigningKey,
    did: &str,
    now_unix: i64,
    now: &str,
) -> Result<Option<(String, String, SignedTalentIndexRecord)>, String> {
    use crate::settings::{registry::keys, SettingsStore};
    if !SettingsStore::get(conn, keys::TALENT_INDEX_SYNC) {
        return Ok(None);
    }
    let Some(cloud) = super::holder_pull::directories_db(conn)?.into_iter().next() else {
        return Ok(None);
    };
    let Some(record) = super::talent_index::get_talent_index_preview_impl(conn, did, now)?.record
    else {
        return Ok(None);
    };
    let digest = snapshot_digest(&record)?;
    let last: Option<(String, i64)> = conn
        .query_row(
            "SELECT digest, published_at FROM listing_publications WHERE cloud_url = ?1",
            params![cloud.url],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if last.is_some_and(|(d, at)| d == digest && now_unix - at < RESEND_AFTER_SECS) {
        return Ok(None);
    }
    Ok(Some((cloud.url, digest, sign_record(&record, key, now)?)))
}

pub fn record_listing(
    conn: &Connection,
    cloud_url: &str,
    digest: &str,
    now_unix: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO listing_publications (cloud_url, digest, published_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(cloud_url) DO UPDATE SET digest = excluded.digest,
                                              published_at = excluded.published_at",
        params![cloud_url, digest, now_unix],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn send_listing(cloud_url: &str, signed: &SignedTalentIndexRecord) -> Result<(), String> {
    let response = client()?
        .post(format!(
            "{}/api/index/records",
            cloud_url.trim_end_matches('/')
        ))
        .json(signed)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    response_json(response).await.map(|_| ())
}

/// Run one sync pass for the open profile: grants, then the listing.
pub(crate) async fn sync_for(state: &State<'_, AppState>) -> Result<SyncReport, String> {
    let (key, did) = load_issuer_key(state).await?;
    let now_unix = chrono::Utc::now().timestamp();
    let now = now_rfc3339();
    let (plan_key, plan_did, plan_now) = (key.clone(), did.as_str().to_string(), now.clone());
    let (pending, unchanged, listing) = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "organisations.sync.plan",
            move |db| {
                let (pending, unchanged) =
                    plan_sync(db.conn(), &plan_key, &plan_did, now_unix, &plan_now)?;
                let listing = plan_listing(db.conn(), &plan_key, &plan_did, now_unix, &plan_now)?;
                Ok((pending, unchanged, listing))
            },
        )
        .await?;
    let results = send_snapshots(&pending).await;
    let listing_result = match &listing {
        Some((url, _, signed)) => Some(send_listing(url, signed).await),
        None => None,
    };
    let mut report = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "organisations.sync.record",
            move |db| {
                let mut report = record_sync(db.conn(), &pending, &results, now_unix)?;
                if let (Some((url, digest, _)), Some(outcome)) = (&listing, &listing_result) {
                    match outcome {
                        Ok(()) => {
                            record_listing(db.conn(), url, digest, now_unix)?;
                            report.sent += 1;
                        }
                        Err(e) => report.failed.push(format!("listing: {e}")),
                    }
                }
                Ok(report)
            },
        )
        .await?;
    report.unchanged += unchanged;
    Ok(report)
}

/// Sync after something changed, logging rather than failing the caller.
pub(crate) async fn sync_quietly(state: &State<'_, AppState>) {
    match sync_for(state).await {
        Ok(r) if r.failed.is_empty() => {}
        Ok(r) => log::warn!("organisation sync: {:?}", r.failed),
        Err(e) => log::debug!("organisation sync skipped: {e}"),
    }
}

// --- talking to Cloud ----------------------------------------------------------

async fn signed_get(
    state: &State<'_, AppState>,
    cloud_url: &str,
    path: &str,
) -> Result<Value, String> {
    let (key, _) = load_issuer_key(state).await?;
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = uuid::Uuid::new_v4().to_string();
    let proof = super::holder_pull::proof(&key, "GET", path, timestamp, &nonce);
    let response = client()?
        .get(format!("{}{path}", cloud_url.trim_end_matches('/')))
        .header("x-alexandria-timestamp", timestamp.to_string())
        .header("x-alexandria-nonce", nonce)
        .header("x-alexandria-proof", proof)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    response_json(response).await
}

async fn post(cloud_url: &str, path: &str, body: &impl Serialize) -> Result<Value, String> {
    let response = client()?
        .post(format!("{}{path}", cloud_url.trim_end_matches('/')))
        .json(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    response_json(response).await
}

// --- commands --------------------------------------------------------------------

/// Organisations asking for access, with who they really are.
#[tauri::command]
pub async fn org_access_requests(
    state: State<'_, AppState>,
) -> Result<PullResult<AccessRequestItem>, String> {
    let (_, did) = load_issuer_key(&state).await?;
    let path = format!("/api/access-requests/for/{}", did.as_str());
    let mut items = Vec::new();
    let mut problems = Vec::new();
    for cloud in super::holder_pull::directories(&state).await? {
        let fetched = async {
            let resolved = directory(&state, &cloud.url).await?;
            let raw = signed_get(&state, &resolved.url, &path).await?;
            serde_json::from_value::<Vec<AccessEnvelope>>(raw).map_err(|e| e.to_string())
        }
        .await;
        let envelopes = match fetched {
            Ok(found) => found,
            Err(detail) => {
                problems.push(Problem {
                    directory: cloud.name,
                    detail,
                });
                continue;
            }
        };
        let now = chrono::Utc::now().timestamp();
        for envelope in envelopes {
            if envelope.request.subject_did != did.as_str() || envelope.request.expires_at <= now {
                continue;
            }
            let org =
                org_trust::check_item(&state, &envelope.request, envelope.org_proof.as_ref()).await;
            let ids = envelope.request.skill_ids.clone();
            let subject = did.as_str().to_string();
            let skills = state
                .db_executor
                .execute(
                    DatabaseWorkload::Learner,
                    state.profile_lease(),
                    "organisations.request.skills",
                    move |db| skill_choices(db.conn(), &subject, &ids),
                )
                .await?;
            items.push(AccessRequestItem {
                directory_url: cloud.url.clone(),
                request: envelope.request,
                org_proof: envelope.org_proof,
                org,
                skills,
            });
        }
    }
    Ok(PullResult { items, problems })
}

/// Share some of the requested skills with the organisation that asked.
#[tauri::command]
pub async fn org_grant(
    state: State<'_, AppState>,
    directory_url: String,
    request: AccessRequest,
    org_proof: Option<OrgProof>,
    skill_ids: Vec<String>,
    sync: bool,
) -> Result<GrantView, String> {
    let cloud = directory(&state, &directory_url).await?;
    let org = org_trust::check_item(&state, &request, org_proof.as_ref()).await;
    let org_name = org.require()?.to_string();
    let org_domain = org.domain.clone().unwrap_or_default();
    let (key, did) = load_issuer_key(&state).await?;
    let now_unix = chrono::Utc::now().timestamp();
    let now = now_rfc3339();

    let statement = GrantStatement {
        format: GRANT_FORMAT.into(),
        action: GrantAction::Grant,
        grant_id: uuid::Uuid::new_v4().to_string(),
        request_id: request.id.clone(),
        audience: request.audience.clone(),
        nonce: request.nonce.clone(),
        subject_did: did.as_str().to_string(),
        skill_ids: skill_ids.clone(),
        sync,
        created: now_unix,
    };
    access::check_answers(&request, &statement, now_unix).map_err(|e| e.to_string())?;
    let signed = access::sign(&statement, &key).map_err(|e| e.to_string())?;

    let (subject, ids, at) = (did.as_str().to_string(), skill_ids.clone(), now.clone());
    let record = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "organisations.grant.snapshot",
            move |db| super::talent_index::grant_snapshot_record(db.conn(), &subject, &ids, &at),
        )
        .await?
        .ok_or("none of these skills has evidence to share yet")?;
    let digest = snapshot_digest(&record)?;
    let snapshot = sign_record(&record, &key, &now)?;
    access::check_snapshot(&statement, &snapshot, &now).map_err(|e| e.to_string())?;

    if !state.profile_lease().is_current() {
        return Err("profile changed".into());
    }
    post(
        &cloud.url,
        "/api/grants",
        &json!({ "statement": signed, "snapshot": snapshot }),
    )
    .await?;

    let row = GrantRow {
        grant_id: statement.grant_id.clone(),
        cloud_url: cloud.url.clone(),
        org_domain,
        org_name,
        purpose: request.purpose.clone(),
        skill_ids,
        sync,
        statement,
        granted_at: now_unix,
        revoked_at: None,
        snapshot_digest: Some(digest),
        synced_at: Some(now_unix),
        sync_error: None,
    };
    let subject = did.as_str().to_string();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "organisations.grant.store",
            move |db| {
                insert_grant(db.conn(), &row)?;
                view(db.conn(), &subject, row, None)
            },
        )
        .await
}

fn view(
    conn: &Connection,
    did: &str,
    row: GrantRow,
    remote: Option<&Value>,
) -> Result<GrantView, String> {
    let skills = skill_choices(conn, did, &row.skill_ids)?;
    let field = |name: &str| remote.and_then(|r| r.get(name));
    Ok(GrantView {
        grant_id: row.grant_id,
        org_domain: row.org_domain,
        org_name: row.org_name,
        purpose: row.purpose,
        skills,
        sync: row.sync,
        granted_at: row.granted_at,
        synced_at: row.synced_at,
        sync_error: row.sync_error,
        revoked_at: row.revoked_at,
        last_viewed_at: field("lastViewedAt")
            .and_then(Value::as_str)
            .map(str::to_string),
        view_count: field("viewCount").and_then(Value::as_i64),
    })
}

/// Every organisation the learner has given access to, newest first, with
/// when each last looked.
#[tauri::command]
pub async fn org_grants(state: State<'_, AppState>) -> Result<GrantsView, String> {
    let (_, did) = load_issuer_key(&state).await?;
    let mut problems = Vec::new();
    let mut remote: Vec<Value> = Vec::new();
    for cloud in super::holder_pull::directories(&state).await? {
        let path = format!("/api/grants/for/{}", did.as_str());
        match signed_get(&state, &cloud.url, &path).await {
            Ok(Value::Array(found)) => remote.extend(found),
            Ok(_) => {}
            Err(detail) => problems.push(Problem {
                directory: cloud.name,
                detail,
            }),
        }
    }
    let subject = did.as_str().to_string();
    let grants = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "organisations.grants",
            move |db| {
                grants(db.conn())?
                    .into_iter()
                    .map(|row| {
                        let theirs = remote.iter().find(|r| {
                            r.get("grantId")
                                .or_else(|| r.get("id"))
                                .and_then(Value::as_str)
                                == Some(row.grant_id.as_str())
                        });
                        view(db.conn(), &subject, row, theirs)
                    })
                    .collect::<Result<Vec<_>, _>>()
            },
        )
        .await?;
    Ok(GrantsView { grants, problems })
}

async fn follow_up(
    state: &State<'_, AppState>,
    grant_id: String,
    action: GrantAction,
    sync: bool,
) -> Result<GrantView, String> {
    let (key, did) = load_issuer_key(state).await?;
    let id = grant_id.clone();
    let current = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "organisations.grant.read",
            move |db| grant(db.conn(), &id),
        )
        .await?;
    if current.revoked_at.is_some() {
        return Err("you already stopped sharing with this organisation".into());
    }
    let next = GrantStatement {
        action,
        sync,
        skill_ids: if action == GrantAction::Revoke {
            Vec::new()
        } else {
            current.skill_ids.clone()
        },
        created: chrono::Utc::now()
            .timestamp()
            .max(current.statement.created),
        ..current.statement.clone()
    };
    access::check_follows(&current.statement, &next).map_err(|e| e.to_string())?;
    let signed: SignedGrantStatement = access::sign(&next, &key).map_err(|e| e.to_string())?;
    post(
        &current.cloud_url,
        &format!("/api/grants/{}/statement", current.grant_id),
        &signed,
    )
    .await?;
    let subject = did.as_str().to_string();
    let view = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "organisations.grant.update",
            move |db| {
                record_statement(db.conn(), &next)?;
                view(db.conn(), &subject, grant(db.conn(), &next.grant_id)?, None)
            },
        )
        .await?;
    if action == GrantAction::Update && sync {
        sync_quietly(state).await;
    }
    Ok(view)
}

/// Turn keeping a grant in sync on or off.
#[tauri::command]
pub async fn org_grant_set_sync(
    state: State<'_, AppState>,
    grant_id: String,
    sync: bool,
) -> Result<GrantView, String> {
    follow_up(&state, grant_id, GrantAction::Update, sync).await
}

/// Stop sharing with an organisation. Cloud deletes what the grant let it hold.
#[tauri::command]
pub async fn org_grant_revoke(
    state: State<'_, AppState>,
    grant_id: String,
) -> Result<GrantView, String> {
    follow_up(&state, grant_id, GrantAction::Revoke, false).await
}

/// Send whatever synced grants and the listing need now.
#[tauri::command]
pub async fn org_sync_now(state: State<'_, AppState>) -> Result<SyncReport, String> {
    sync_for(&state).await
}

/// The Alexandria Cloud requests come through, or none when requests are
/// switched off or no Cloud is set. Without it an empty inbox reads the same
/// as "nobody asked", and the screen has no address to answer a visibility
/// change to.
#[tauri::command]
pub async fn org_cloud(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(super::holder_pull::directories(&state)
        .await?
        .into_iter()
        .next()
        .map(|cloud| cloud.url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[11; 32])
    }

    fn did() -> String {
        alexandria_verify::did::derive_did_key(&key())
            .as_str()
            .to_string()
    }

    fn row(sync: bool) -> GrantRow {
        GrantRow {
            grant_id: "grant-1".into(),
            cloud_url: "http://127.0.0.1:9".into(),
            org_domain: "alexandria.ifftu.dev".into(),
            org_name: "IFFTU".into(),
            purpose: "Engineering Manager".into(),
            skill_ids: vec!["skill_big_o".into()],
            sync,
            statement: GrantStatement {
                format: GRANT_FORMAT.into(),
                action: GrantAction::Grant,
                grant_id: "grant-1".into(),
                request_id: "req-1".into(),
                audience: alexandria_verify::org::audience("alexandria.ifftu.dev"),
                nonce: "n".into(),
                subject_did: did(),
                skill_ids: vec!["skill_big_o".into()],
                sync,
                created: 100,
            },
            granted_at: 100,
            revoked_at: None,
            snapshot_digest: None,
            synced_at: None,
            sync_error: None,
        }
    }

    fn db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    #[test]
    fn grants_round_trip_and_a_revoke_sticks() {
        let db = db();
        insert_grant(db.conn(), &row(true)).unwrap();
        let mut revoke = row(true).statement;
        revoke.action = GrantAction::Revoke;
        revoke.sync = false;
        revoke.skill_ids.clear();
        revoke.created = 200;
        record_statement(db.conn(), &revoke).unwrap();
        let stored = grant(db.conn(), "grant-1").unwrap();
        assert_eq!(stored.revoked_at, Some(200));
        assert!(!stored.sync);
        // A later statement cannot un-revoke it.
        let mut later = revoke.clone();
        later.created = 300;
        record_statement(db.conn(), &later).unwrap();
        assert_eq!(grant(db.conn(), "grant-1").unwrap().revoked_at, Some(200));
    }

    #[test]
    fn nothing_is_planned_without_evidence_or_with_sync_off() {
        let db = db();
        insert_grant(db.conn(), &row(true)).unwrap();
        let (pending, unchanged) =
            plan_sync(db.conn(), &key(), &did(), 1_000, "2026-10-12T00:00:00Z").unwrap();
        // No derived skill state for skill_big_o in an empty profile.
        assert!(pending.is_empty());
        assert_eq!(unchanged, 1);

        db.conn().execute("DELETE FROM org_grants", []).unwrap();
        insert_grant(db.conn(), &row(false)).unwrap();
        let (pending, unchanged) =
            plan_sync(db.conn(), &key(), &did(), 1_000, "2026-10-12T00:00:00Z").unwrap();
        assert!(pending.is_empty());
        assert_eq!(unchanged, 0);
    }

    #[test]
    fn a_failed_send_is_kept_and_a_good_one_clears_it() {
        let db = db();
        insert_grant(db.conn(), &row(true)).unwrap();
        let record = TalentIndexRecord {
            subject_did: did(),
            skills: Vec::new(),
            display_name: None,
            bio: None,
            valid_until: "2027-01-01T00:00:00Z".into(),
        };
        let pending = vec![PendingSnapshot {
            grant_id: "grant-1".into(),
            cloud_url: "http://127.0.0.1:9".into(),
            digest: "d1".into(),
            signed: sign_record(&record, &key(), "2026-10-12T00:00:00Z").unwrap(),
        }];
        let report = record_sync(
            db.conn(),
            &pending,
            &[("grant-1".into(), Err("offline".into()))],
            500,
        )
        .unwrap();
        assert_eq!(report.failed.len(), 1);
        assert_eq!(
            grant(db.conn(), "grant-1").unwrap().sync_error.as_deref(),
            Some("offline")
        );

        let report = record_sync(db.conn(), &pending, &[("grant-1".into(), Ok(()))], 600).unwrap();
        assert_eq!(report.sent, 1);
        let stored = grant(db.conn(), "grant-1").unwrap();
        assert_eq!(
            (
                stored.snapshot_digest.as_deref(),
                stored.synced_at,
                stored.sync_error
            ),
            (Some("d1"), Some(600), None)
        );
    }

    #[test]
    fn the_digest_ignores_expiry_but_not_content() {
        let mut a = TalentIndexRecord {
            subject_did: did(),
            skills: Vec::new(),
            display_name: Some("Demo".into()),
            bio: None,
            valid_until: "2027-01-01T00:00:00Z".into(),
        };
        let first = snapshot_digest(&a).unwrap();
        a.valid_until = "2027-02-01T00:00:00Z".into();
        assert_eq!(snapshot_digest(&a).unwrap(), first);
        a.display_name = Some("Someone else".into());
        assert_ne!(snapshot_digest(&a).unwrap(), first);
    }
}
