//! Record shares: learning records sent to an organisation's hosted service
//! under an access grant.
//!
//! A record share is a scope on the existing access request and grant
//! ([`alexandria_verify::access::RecordScope`]); there is no second consent
//! path. The grant commands (`commands::organisations`) sign and send the
//! statements and keep the grant row; this module is the records half:
//!
//! - [`offer`] is what the consent screen shows for a request's records part,
//!   [`approval`] turns the learner's answer into the grant's scope and the
//!   share's recipient, and [`approve`] sets the share up to send. Nothing is
//!   sent before that.
//! - [`narrowed`] builds the update statement for a narrower scope and
//!   [`narrow`] applies it here; [`stop`] ends sending when the grant is
//!   revoked.
//! - [`send_pending`] builds batches within each share's scope and sends them
//!   through the outbox ([`super::send_due`]) to the organisation's hosted
//!   service as a [`SignedRecordBatch`], signed by the learner.
//!
//! A share's recipient id is its grant id, and only a grant that is not
//! revoked is sent to. Shares go quiet while the organisations switch is off.
//!
//! # Wire format (duplicated in the hosted service on purpose)
//!
//! `POST {cloud}/api/grants/{grant_id}/records` with a JSON
//! [`SignedRecordBatch`]: `{ "batch": RecordBatch, "signature": base64url }`.
//! The signature is Ed25519 by the batch's `subject_did` (the grant's
//! subject) over `RECORD_BATCH_FORMAT || 0x00 || JCS(batch)`, the scheme
//! grant statements use. The statements are xAPI statements built for this
//! organisation alone: the actor is the account the grant names.

use std::future::Future;
use std::pin::Pin;

use alexandria_studio::xapi::lrs::LrsError;
use alexandria_studio::xapi::{Account, Statement};
use alexandria_verify::access::{
    self, AccessRequest, GrantAction, GrantStatement, RecordAccount, RecordKind,
    RecordScope as GrantScope, GRANT_FORMAT,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signer, SigningKey};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{
    outbox, recipients, send_due, timestamp, BatchOutcome, Identity, RecipientEntry, RecipientKind,
    RecordDb, Scope, SendError, Transport,
};
use crate::domain::record_sending::{
    CourseChoice, GrantRecords, IdentifierChoice, RecordShareDecision, RecordShareOffer,
};
use crate::settings::{registry::keys, SettingsStore};

pub const RECORD_BATCH_FORMAT: &str = "alexandria-record-batch/1";

/// Where a share's batches go, under the grant's hosted service.
pub fn records_path(grant_id: &str) -> String {
    format!("/api/grants/{grant_id}/records")
}

/// The recipient label of a share: the request it answers. Unique per
/// request (a request is granted at most once) and known before the learner
/// approves, so the consent screen can show the identifier. Never change
/// the format: it keys every identifier the organisation has seen.
pub fn label(request: &AccessRequest) -> String {
    format!("share|{}|{}", request.audience, request.id)
}

fn record_account(account: &Account) -> RecordAccount {
    RecordAccount {
        home_page: account.home_page.clone(),
        name: account.name.clone(),
    }
}

fn studio_account(account: &RecordAccount) -> Account {
    Account {
        home_page: account.home_page.clone(),
        name: account.name.clone(),
    }
}

fn account_for(
    identity: &Identity,
    secret: &[u8; 32],
    did: &str,
    label: &str,
) -> Result<RecordAccount, SendError> {
    Ok(record_account(
        recipients::recipient(identity, secret, did, label)?.account(),
    ))
}

fn course_choices(conn: &Connection, ids: &[String]) -> Result<Vec<CourseChoice>, SendError> {
    ids.iter()
        .map(|id| {
            let title: Option<String> = conn
                .query_row(
                    "SELECT title FROM courses WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(CourseChoice {
                id: id.clone(),
                title: title.unwrap_or_else(|| id.clone()),
            })
        })
        .collect()
}

/// What the consent screen shows for the records part of `request`, or
/// `None` when it asks for none. Sends nothing.
pub fn offer(
    conn: &Connection,
    request: &AccessRequest,
    learner_secret: &[u8; 32],
    learner_did: &str,
) -> Result<Option<RecordShareOffer>, SendError> {
    let Some(asked) = &request.records else {
        return Ok(None);
    };
    let label = label(request);
    Ok(Some(RecordShareOffer {
        courses: course_choices(conn, &asked.course_ids)?,
        interactions_requested: asked.includes(RecordKind::Interaction),
        pairwise_actor: account_for(&Identity::Pairwise, learner_secret, learner_did, &label)?.name,
        real_did_actor: account_for(&Identity::RealDid, learner_secret, learner_did, &label)?.name,
        proposed_account: asked.account.clone(),
    }))
}

/// The learner's approval of a request's records part: the scope the grant
/// statement carries and the recipient that sends under it.
#[derive(Debug, Clone)]
pub struct ShareApproval {
    pub records: GrantScope,
    pub entry: RecipientEntry,
}

/// Turns the learner's answer into the grant's record scope and the share's
/// recipient. Refuses a course or kind the request did not ask for, and the
/// proposed account when none was proposed.
pub fn approval(
    request: &AccessRequest,
    decision: &RecordShareDecision,
    grant_id: &str,
    learner_secret: &[u8; 32],
    learner_did: &str,
    now: DateTime<Utc>,
) -> Result<ShareApproval, SendError> {
    let asked = request.records.as_ref().ok_or_else(|| {
        SendError::InvalidScope("this request does not ask for learning records".into())
    })?;
    let scope = Scope {
        course_ids: decision.course_ids.clone(),
        include_interactions: decision.include_interactions,
    };
    scope.validate()?;
    if let Some(stranger) = scope
        .course_ids
        .iter()
        .find(|id| !asked.course_ids.contains(id))
    {
        return Err(SendError::InvalidScope(format!(
            "course {stranger} was not asked for"
        )));
    }
    if scope.include_interactions && !asked.includes(RecordKind::Interaction) {
        return Err(SendError::InvalidScope(
            "per-answer records were not asked for".into(),
        ));
    }
    let identity = match decision.identifier {
        IdentifierChoice::Pairwise => Identity::Pairwise,
        IdentifierChoice::RealDid => Identity::RealDid,
        IdentifierChoice::Proposed => match &asked.account {
            Some(account) => Identity::Proposed(studio_account(account)),
            None => {
                return Err(SendError::InvalidScope(
                    "the organisation did not propose an account".into(),
                ))
            }
        },
    };
    let label = label(request);
    let account = account_for(&identity, learner_secret, learner_did, &label)?;
    let records = GrantScope {
        course_ids: scope.course_ids.clone(),
        kinds: kinds(&scope),
        account: Some(account.clone()),
    };
    records
        .validate()
        .map_err(|e| SendError::InvalidScope(e.to_string()))?;
    Ok(ShareApproval {
        records,
        entry: RecipientEntry {
            id: grant_id.to_string(),
            kind: RecipientKind::Share,
            label,
            identity,
            actor_name: account.name,
            scope,
            created_at: timestamp(now),
        },
    })
}

fn kinds(scope: &Scope) -> Vec<RecordKind> {
    let mut kinds = vec![RecordKind::Completion];
    if scope.include_interactions {
        kinds.push(RecordKind::Interaction);
    }
    kinds
}

/// The `grant` statement answering `request`, for the learner to sign.
pub fn grant_statement(
    request: &AccessRequest,
    grant_id: &str,
    skill_ids: Vec<String>,
    sync: bool,
    records: Option<GrantScope>,
    now_unix: i64,
) -> GrantStatement {
    GrantStatement {
        format: GRANT_FORMAT.into(),
        action: GrantAction::Grant,
        grant_id: grant_id.to_string(),
        request_id: request.id.clone(),
        audience: request.audience.clone(),
        nonce: request.nonce.clone(),
        subject_did: request.subject_did.clone(),
        skill_ids,
        sync,
        created: now_unix,
        records,
    }
}

/// Sets an approved share up to send: stores its recipient and queues the
/// records in scope. Call in the transaction that stores the grant.
pub fn approve(
    conn: &Connection,
    entry: &RecipientEntry,
    now: DateTime<Utc>,
) -> Result<(), SendError> {
    recipients::insert(conn, entry)?;
    outbox::queue(conn, entry, now)?;
    Ok(())
}

/// The update statement narrowing `current`'s record scope to
/// `course_ids` (and dropping per-answer records unless kept), with the
/// local scope to apply once the hosted service has accepted it. Refuses
/// anything wider than what the grant shares now.
pub fn narrowed(
    current: &GrantStatement,
    course_ids: Vec<String>,
    include_interactions: bool,
    now_unix: i64,
) -> Result<(GrantStatement, Scope), SendError> {
    let held = current
        .records
        .as_ref()
        .ok_or_else(|| SendError::InvalidScope("this grant shares no learning records".into()))?;
    let scope = Scope {
        course_ids,
        include_interactions,
    };
    scope.validate()?;
    let next = GrantStatement {
        action: GrantAction::Update,
        created: now_unix.max(current.created),
        records: Some(GrantScope {
            course_ids: scope.course_ids.clone(),
            kinds: kinds(&scope),
            account: held.account.clone(),
        }),
        ..current.clone()
    };
    access::check_follows(current, &next).map_err(|e| SendError::InvalidScope(e.to_string()))?;
    Ok((next, scope))
}

/// Applies a narrower scope to a share: what it no longer covers is not
/// sent. Returns whether the share exists.
pub fn narrow(conn: &Connection, grant_id: &str, scope: &Scope) -> Result<bool, SendError> {
    recipients::update_scope(conn, grant_id, scope)
}

/// Stops a share sending, for good: its recipient and queue go. Its access
/// log rows stay. Returns whether it was sending.
pub fn stop(conn: &Connection, grant_id: &str) -> Result<bool, SendError> {
    match recipients::get(conn, grant_id)? {
        Some(entry) if entry.kind == RecipientKind::Share => recipients::delete(conn, grant_id),
        _ => Ok(false),
    }
}

/// The share of a grant as the learner sees it, from the grant's statement
/// (what was granted) and the share's recipient (whether it still sends).
pub fn view(
    conn: &Connection,
    grant_id: &str,
    granted: Option<&GrantScope>,
) -> Result<Option<GrantRecords>, SendError> {
    let Some(granted) = granted else {
        return Ok(None);
    };
    let entry = recipients::get(conn, grant_id)?.filter(|e| e.kind == RecipientKind::Share);
    let status = match &entry {
        Some(entry) => Some(outbox::status(conn, &entry.id)?),
        None => None,
    };
    Ok(Some(GrantRecords {
        courses: course_choices(conn, &granted.course_ids)?,
        include_interactions: granted.includes(RecordKind::Interaction),
        identifier: entry
            .as_ref()
            .map(|e| e.identity.as_str().to_string())
            .unwrap_or_default(),
        actor_name: granted
            .account
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_default(),
        status,
    }))
}

// --- batches -------------------------------------------------------------

/// One request's worth of records for a share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordBatch {
    pub format: String,
    pub grant_id: String,
    /// The grant's subject, whose key signs the batch.
    pub subject_did: String,
    /// Fresh per request, so the service can refuse a replayed one.
    pub batch_id: String,
    /// Unix seconds. The service accepts it within the grant clock skew.
    pub created: i64,
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedRecordBatch {
    pub batch: RecordBatch,
    /// base64url Ed25519 signature over [`batch_signing_bytes`].
    pub signature: String,
}

pub fn batch_signing_bytes(batch: &RecordBatch) -> Result<Vec<u8>, SendError> {
    let mut bytes = RECORD_BATCH_FORMAT.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend(
        serde_json_canonicalizer::to_vec(batch).map_err(|e| SendError::Database(e.to_string()))?,
    );
    Ok(bytes)
}

pub fn sign_batch(batch: RecordBatch, key: &SigningKey) -> Result<SignedRecordBatch, SendError> {
    let signature = key.sign(&batch_signing_bytes(&batch)?);
    Ok(SignedRecordBatch {
        batch,
        signature: URL_SAFE_NO_PAD.encode(signature.to_bytes()),
    })
}

/// Sends a share's batches to its organisation's hosted service.
struct ShareTransport {
    url: String,
    grant_id: String,
    key: SigningKey,
    did: String,
}

impl ShareTransport {
    async fn post(&self, statements: &[Statement]) -> Result<(), LrsError> {
        let signed = sign_batch(
            RecordBatch {
                format: RECORD_BATCH_FORMAT.into(),
                grant_id: self.grant_id.clone(),
                subject_did: self.did.clone(),
                batch_id: uuid::Uuid::new_v4().to_string(),
                created: Utc::now().timestamp(),
                statements: statements.to_vec(),
            },
            &self.key,
        )
        .map_err(|e| LrsError::InvalidResponse(e.to_string()))?;
        let http = crate::cloud::client().map_err(LrsError::Unreachable)?;
        let response = http
            .post(&self.url)
            .json(&signed)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LrsError::Timeout
                } else {
                    LrsError::Unreachable(e.to_string())
                }
            })?;
        let status = response.status().as_u16();
        if response.status().is_success() {
            return Ok(());
        }
        if status == 408 || status == 429 || status >= 500 {
            return Err(LrsError::Unavailable { status });
        }
        let body = response.text().await.unwrap_or_default();
        let detail: String = body.chars().take(200).collect();
        Err(LrsError::Refused {
            status,
            detail: if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            },
        })
    }
}

impl Transport for ShareTransport {
    fn deliver<'a>(
        &'a self,
        statements: &'a [Statement],
    ) -> Pin<Box<dyn Future<Output = Result<(), LrsError>> + Send + 'a>> {
        Box::pin(self.post(statements))
    }
}

/// A share that may send now, with where and to whom.
struct ActiveShare {
    entry: RecipientEntry,
    cloud_url: String,
    org_name: String,
}

fn active(conn: &Connection) -> Result<Vec<ActiveShare>, SendError> {
    if !SettingsStore::get(conn, keys::ORGANISATIONS_ENABLED) {
        return Ok(Vec::new());
    }
    let mut statement = conn.prepare(
        "SELECT g.grant_id, g.cloud_url, g.org_name FROM org_grants g \
         JOIN record_recipients r ON r.id = g.grant_id \
         WHERE r.kind = 'share' AND g.revoked_at IS NULL ORDER BY g.granted_at, g.grant_id",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut shares = Vec::with_capacity(rows.len());
    for (grant_id, cloud_url, org_name) in rows {
        if !crate::cloud::is_allowed_origin(&cloud_url) {
            continue;
        }
        if let Some(entry) = recipients::get(conn, &grant_id)? {
            shares.push(ActiveShare {
                entry,
                cloud_url,
                org_name,
            });
        }
    }
    Ok(shares)
}

/// Sends what is due to every share whose grant is still in force.
pub async fn send_pending<D: RecordDb>(
    db: &D,
    key: &SigningKey,
    now: DateTime<Utc>,
) -> Result<BatchOutcome, String> {
    let shares = db
        .run("records.shares.load", |db| {
            active(db.conn()).map_err(|e| e.to_string())
        })
        .await?;
    if shares.is_empty() {
        return Ok(BatchOutcome::default());
    }
    let secret = zeroize::Zeroizing::new(key.to_bytes());
    let did = crate::crypto::did::derive_did_key(key).as_str().to_string();
    let mut total = BatchOutcome::default();
    for share in shares {
        let transport = ShareTransport {
            url: format!(
                "{}{}",
                share.cloud_url.trim_end_matches('/'),
                records_path(&share.entry.id)
            ),
            grant_id: share.entry.id.clone(),
            key: key.clone(),
            did: did.clone(),
        };
        match send_due(
            db,
            &share.entry,
            &share.org_name,
            &transport,
            &secret,
            &did,
            now,
        )
        .await
        {
            Ok(outcome) => {
                total.sent += outcome.sent;
                total.failed += outcome.failed;
                total.waiting += outcome.waiting;
            }
            Err(error) => log::warn!("sending to a record share failed: {error}"),
        }
    }
    Ok(total)
}

#[cfg(test)]
pub(crate) mod tests_support {
    use alexandria_studio::xapi::{self, ActivityRef, BuildOptions, LearningEvent, Target};
    use chrono::TimeZone;

    use super::*;

    /// The shared batch vector: a fixed learner key, grant and statement.
    /// The hosted service keeps a copy and checks its signature.
    pub fn vector() -> SignedRecordBatch {
        let key = SigningKey::from_bytes(&[1; 32]);
        let secret = key.to_bytes();
        let creator = crate::crypto::did::derive_did_key(&SigningKey::from_bytes(&[9; 32]))
            .as_str()
            .to_string();
        let at = Utc.with_ymd_and_hms(2026, 11, 20, 9, 0, 0).unwrap();
        let event = LearningEvent {
            local_id: "registered|enrolment-1".into(),
            timestamp: at,
            registration: Some("enrolment-1".into()),
            action: xapi::Action::Registered {
                target: Target::named(
                    ActivityRef::course(creator, "aa".repeat(32)),
                    "en-US",
                    "Warehouse safety",
                ),
            },
            profile: None,
        };
        let recipient =
            xapi::Recipient::pairwise(&secret, "share|aud|6f1d0c52-3a8e-4f7b-9d2a-0c1e5b7a9f10")
                .unwrap();
        let statement =
            xapi::build_statement(&event, &recipient, &BuildOptions::default()).unwrap();
        sign_batch(
            RecordBatch {
                format: RECORD_BATCH_FORMAT.into(),
                grant_id: "0d5b8f3e-1c2a-4b6d-8e9f-7a1b2c3d4e5f".into(),
                subject_did: crate::crypto::did::derive_did_key(&key)
                    .as_str()
                    .to_string(),
                batch_id: "9b2f6c1e-4d3a-4e8b-a7c5-1f0e2d3c4b5a".into(),
                created: at.timestamp(),
                statements: vec![statement],
            },
            &key,
        )
        .unwrap()
    }
}
