//! Interview invitations and offers addressed to this identity.
//!
//! The learner polls each configured directory with the same signed GET proof
//! the assessment exchange uses, and answers with a message signed by the
//! key that owns their DID: the whole invitation (or offer), the decision,
//! and a short validity window. The wire contract is mirrored byte for byte
//! in Alexandria Cloud (`src/hiring.rs`): `FORMAT || 0x00 || JCS(response)`.
//!
//! Every answer is also kept locally in `hiring_responses`, so the learner
//! can see what they accepted, when, and where to turn up — the directory's
//! copy is the organisation's record, this is theirs.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::cloud::client;
use crate::commands::exchange::{directory, response_json};
use crate::commands::holder_pull::{Directory, Problem, PullResult};
use crate::credentials::Issuer;
use crate::crypto::did::{resolve_did_key, Did};
use crate::db::executor::DatabaseWorkload;
use crate::org_trust::{self, OrgCheck};
use crate::profile::scope::ProfileState as State;
use crate::AppState;
use alexandria_verify::org::OrgProof;

pub const FORMAT: &str = "alexandria-hiring-response/1";
pub const RESPONSE_LIFETIME_SECS: i64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InterviewInvite {
    pub id: String,
    pub audience: String,
    pub nonce: String,
    pub organization: String,
    pub subject_did: String,
    pub role_label: String,
    pub message: String,
    pub mode: String,
    pub proposed_slots: Vec<i64>,
    pub meeting_url: Option<String>,
    pub run_id: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    /// The organisation asks the candidate to run the integrity check during
    /// an interview held in Alexandria. Absent from the signed bytes when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub integrity_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InterviewResponse {
    pub format: String,
    pub invite: InterviewInvite,
    pub decision: String,
    pub chosen_slot: Option<i64>,
    pub note: Option<String>,
    pub issued_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignedInterviewResponse {
    pub response: InterviewResponse,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub id: String,
    pub audience: String,
    pub nonce: String,
    pub organization: String,
    pub subject_did: String,
    pub interview_id: String,
    pub role_label: String,
    pub terms: String,
    pub start_date: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OfferResponse {
    pub format: String,
    pub offer: Offer,
    pub decision: String,
    pub note: Option<String>,
    pub issued_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignedOfferResponse {
    pub response: OfferResponse,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryInterview {
    pub directory_url: String,
    pub invite: InterviewInvite,
    #[serde(rename = "orgProof")]
    pub org_proof: Option<OrgProof>,
    pub org: OrgCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryOffer {
    pub directory_url: String,
    pub offer: Offer,
    #[serde(rename = "orgProof")]
    pub org_proof: Option<OrgProof>,
    pub org: OrgCheck,
}

/// An invitation as Cloud serves it, with the organisation's proof.
#[derive(Debug, Clone, Deserialize)]
struct InviteEnvelope {
    invite: InterviewInvite,
    #[serde(rename = "orgProof")]
    org_proof: Option<OrgProof>,
}

#[derive(Debug, Clone, Deserialize)]
struct OfferEnvelope {
    offer: Offer,
    #[serde(rename = "orgProof")]
    org_proof: Option<OrgProof>,
}

/// What the learner keeps: the answer they signed, and where it went.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HiringRecord {
    pub id: String,
    /// `interview` or `offer`.
    pub kind: String,
    pub directory_url: String,
    pub organization: String,
    pub role_label: String,
    pub decision: String,
    pub chosen_slot: Option<i64>,
    pub meeting_url: Option<String>,
    pub responded_at: String,
    pub payload_json: String,
}

pub fn signing_bytes<T: Serialize>(message: &T) -> Result<Vec<u8>, String> {
    signing_bytes_as(FORMAT, message)
}

/// `format || 0x00 || JCS(message)`, so a signature over one kind of message
/// never verifies as another.
pub fn signing_bytes_as<T: Serialize>(format: &str, message: &T) -> Result<Vec<u8>, String> {
    let mut bytes = format.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend(serde_json_canonicalizer::to_vec(message).map_err(|e| e.to_string())?);
    if bytes.len() > 64 * 1024 {
        return Err("hiring response exceeds 64 KiB".into());
    }
    Ok(bytes)
}

fn sign<T: Serialize>(message: &T, key: &SigningKey) -> Result<String, String> {
    Ok(URL_SAFE_NO_PAD.encode(key.sign(&signing_bytes(message)?).to_bytes()))
}

fn check_signature<T: Serialize>(message: &T, signature: &str, did: &str) -> Result<(), String> {
    let key = resolve_did_key(&Did(did.to_string())).map_err(|e| e.to_string())?;
    let signature = Signature::from_slice(
        &URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    key.verify(&signing_bytes(message)?, &signature)
        .map_err(|_| "invalid holder signature".to_string())
}

fn check_request(
    audience: &str,
    nonce: &str,
    subject: &str,
    expires: i64,
    did: &str,
    now: i64,
) -> Result<(), String> {
    if subject != did {
        return Err("this request belongs to another identity".into());
    }
    if audience.is_empty() || nonce.is_empty() {
        return Err("request is not bound to an organisation".into());
    }
    if expires <= now {
        return Err("request has expired".into());
    }
    Ok(())
}

fn check_decision(decision: &str) -> Result<(), String> {
    match decision {
        "accept" | "decline" => Ok(()),
        _ => Err("decision must be accept or decline".into()),
    }
}

/// Build and sign the learner's answer to an invitation. Pure: the caller
/// supplies the key and the clock, which is what makes it testable.
pub fn sign_interview_response(
    invite: InterviewInvite,
    decision: &str,
    chosen_slot: Option<i64>,
    note: Option<String>,
    key: &SigningKey,
    did: &str,
    now: i64,
) -> Result<SignedInterviewResponse, String> {
    check_request(
        &invite.audience,
        &invite.nonce,
        &invite.subject_did,
        invite.expires_at,
        did,
        now,
    )?;
    check_decision(decision)?;
    match (decision, chosen_slot) {
        ("accept", Some(slot)) if invite.proposed_slots.contains(&slot) => {}
        ("accept", _) => return Err("choose one of the proposed times to accept".into()),
        ("decline", None) => {}
        ("decline", Some(_)) => return Err("a declined invitation has no time".into()),
        _ => unreachable!("decision already validated"),
    }
    let note = note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    if note.as_ref().is_some_and(|n| n.len() > 2000) {
        return Err("note is limited to 2000 characters".into());
    }
    let response = InterviewResponse {
        format: FORMAT.into(),
        invite,
        decision: decision.into(),
        chosen_slot,
        note,
        issued_at: now,
        expires_at: now + RESPONSE_LIFETIME_SECS,
    };
    let signature = sign(&response, key)?;
    let signed = SignedInterviewResponse {
        response,
        signature,
    };
    check_signature(&signed.response, &signed.signature, did)?;
    Ok(signed)
}

pub fn sign_offer_response(
    offer: Offer,
    decision: &str,
    note: Option<String>,
    key: &SigningKey,
    did: &str,
    now: i64,
) -> Result<SignedOfferResponse, String> {
    check_request(
        &offer.audience,
        &offer.nonce,
        &offer.subject_did,
        offer.expires_at,
        did,
        now,
    )?;
    check_decision(decision)?;
    let note = note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    if note.as_ref().is_some_and(|n| n.len() > 2000) {
        return Err("note is limited to 2000 characters".into());
    }
    let response = OfferResponse {
        format: FORMAT.into(),
        offer,
        decision: decision.into(),
        note,
        issued_at: now,
        expires_at: now + RESPONSE_LIFETIME_SECS,
    };
    let signature = sign(&response, key)?;
    let signed = SignedOfferResponse {
        response,
        signature,
    };
    check_signature(&signed.response, &signed.signature, did)?;
    Ok(signed)
}

async fn signed_get<T: serde::de::DeserializeOwned>(
    state: &State<'_, AppState>,
    dir: &Directory,
    path: &str,
) -> Result<Vec<T>, String> {
    let (key, _) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = uuid::Uuid::new_v4().to_string();
    let proof = super::holder_pull::proof(&key, "GET", path, timestamp, &nonce);
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

#[tauri::command]
pub async fn hiring_interviews(
    state: State<'_, AppState>,
) -> Result<PullResult<DirectoryInterview>, String> {
    let (_, did) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let path = format!("/api/interviews/for/{}", did.as_str());
    let dirs = super::holder_pull::directories(&state).await?;
    let mut items = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs {
        match async {
            let resolved = directory(&state, &dir.url).await?;
            signed_get::<InviteEnvelope>(&state, &resolved, &path).await
        }
        .await
        {
            Ok(found) => {
                for envelope in found.into_iter().filter(|e| {
                    e.invite.subject_did == did.as_str()
                        && e.invite.expires_at > chrono::Utc::now().timestamp()
                }) {
                    let org = org_trust::check_item(
                        &state,
                        &envelope.invite,
                        envelope.org_proof.as_ref(),
                    )
                    .await;
                    items.push(DirectoryInterview {
                        directory_url: dir.url.clone(),
                        invite: envelope.invite,
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

#[tauri::command]
pub async fn hiring_offers(
    state: State<'_, AppState>,
) -> Result<PullResult<DirectoryOffer>, String> {
    let (_, did) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let path = format!("/api/offers/for/{}", did.as_str());
    let dirs = super::holder_pull::directories(&state).await?;
    let mut items = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs {
        match async {
            let resolved = directory(&state, &dir.url).await?;
            signed_get::<OfferEnvelope>(&state, &resolved, &path).await
        }
        .await
        {
            Ok(found) => {
                for envelope in found.into_iter().filter(|e| {
                    e.offer.subject_did == did.as_str()
                        && e.offer.expires_at > chrono::Utc::now().timestamp()
                }) {
                    let org =
                        org_trust::check_item(&state, &envelope.offer, envelope.org_proof.as_ref())
                            .await;
                    items.push(DirectoryOffer {
                        directory_url: dir.url.clone(),
                        offer: envelope.offer,
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

fn store_record(conn: &rusqlite::Connection, record: &HiringRecord) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO hiring_responses (id,kind,directory_url,organization,role_label,decision,chosen_slot,meeting_url,responded_at,payload_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            record.id, record.kind, record.directory_url, record.organization, record.role_label,
            record.decision, record.chosen_slot, record.meeting_url, record.responded_at, record.payload_json
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn hiring_interview_respond(
    state: State<'_, AppState>,
    directory_url: String,
    invite: InterviewInvite,
    org_proof: Option<OrgProof>,
    decision: String,
    chosen_slot: Option<i64>,
    note: Option<String>,
) -> Result<serde_json::Value, String> {
    let dir = directory(&state, &directory_url).await?;
    org_trust::check_item(&state, &invite, org_proof.as_ref())
        .await
        .require()?;
    let (key, did) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let signed = sign_interview_response(
        invite,
        &decision,
        chosen_slot,
        note,
        &key,
        did.as_str(),
        chrono::Utc::now().timestamp(),
    )?;
    let id = uuid::Uuid::parse_str(&signed.response.invite.id).map_err(|e| e.to_string())?;
    if !state.profile_lease().is_current() {
        return Err("profile changed".into());
    }
    let response = client()?
        .post(format!(
            "{}/api/interviews/{id}/respond",
            dir.url.trim_end_matches('/')
        ))
        .json(&signed)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let receipt = response_json(response).await?;
    let record = HiringRecord {
        id: signed.response.invite.id.clone(),
        kind: "interview".into(),
        directory_url: dir.url.clone(),
        organization: signed.response.invite.organization.clone(),
        role_label: signed.response.invite.role_label.clone(),
        decision: signed.response.decision.clone(),
        chosen_slot: signed.response.chosen_slot,
        meeting_url: signed.response.invite.meeting_url.clone(),
        responded_at: crate::clock::now_rfc3339(),
        payload_json: serde_json::to_string(&signed).map_err(|e| e.to_string())?,
    };
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "hiring.record",
            move |db| store_record(db.conn(), &record),
        )
        .await?;
    Ok(receipt)
}

#[tauri::command]
pub async fn hiring_offer_respond(
    state: State<'_, AppState>,
    directory_url: String,
    offer: Offer,
    org_proof: Option<OrgProof>,
    decision: String,
    note: Option<String>,
) -> Result<serde_json::Value, String> {
    let dir = directory(&state, &directory_url).await?;
    org_trust::check_item(&state, &offer, org_proof.as_ref())
        .await
        .require()?;
    let (key, did) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let signed = sign_offer_response(
        offer,
        &decision,
        note,
        &key,
        did.as_str(),
        chrono::Utc::now().timestamp(),
    )?;
    let id = uuid::Uuid::parse_str(&signed.response.offer.id).map_err(|e| e.to_string())?;
    if !state.profile_lease().is_current() {
        return Err("profile changed".into());
    }
    let response = client()?
        .post(format!(
            "{}/api/offers/{id}/respond",
            dir.url.trim_end_matches('/')
        ))
        .json(&signed)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let receipt = response_json(response).await?;
    let record = HiringRecord {
        id: signed.response.offer.id.clone(),
        kind: "offer".into(),
        directory_url: dir.url.clone(),
        organization: signed.response.offer.organization.clone(),
        role_label: signed.response.offer.role_label.clone(),
        decision: signed.response.decision.clone(),
        chosen_slot: None,
        meeting_url: None,
        responded_at: crate::clock::now_rfc3339(),
        payload_json: serde_json::to_string(&signed).map_err(|e| e.to_string())?,
    };
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "hiring.record",
            move |db| store_record(db.conn(), &record),
        )
        .await?;
    Ok(receipt)
}

/// The room for an accepted interview held in Alexandria, once the panel has
/// opened it. Asked with the learner's own proof, so only the candidate the
/// interview is for can learn the ticket.
#[tauri::command]
pub async fn hiring_interview_room(
    state: State<'_, AppState>,
    directory_url: String,
    interview_id: String,
) -> Result<serde_json::Value, String> {
    let dir = directory(&state, &directory_url).await?;
    let id = uuid::Uuid::parse_str(&interview_id).map_err(|e| e.to_string())?;
    let (key, _) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let path = format!("/api/interviews/{id}/room");
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
    response_json(response).await
}

// ── The candidate's integrity check ─────────────────────────────────────

/// What the app signs when it hands an organisation the result of the
/// integrity check the candidate ran during an interview held in Alexandria.
/// Mirrored in Alexandria Cloud (`src/hiring.rs`).
pub const INTEGRITY_FORMAT: &str = "alexandria-interview-integrity/1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IntegrityFlag {
    pub flag: String,
    pub severity: String,
    pub count: u32,
}

/// Derived values only. There is no field for video, audio, keystrokes,
/// screenshots or what else was running, and Cloud refuses unknown fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IntegrityReport {
    pub format: String,
    pub invite: InterviewInvite,
    /// `clear` or `flagged`.
    pub outcome: String,
    /// 0–100.
    pub score: Option<u8>,
    pub started_at: i64,
    pub ended_at: i64,
    pub snapshots: u32,
    pub camera: bool,
    pub camera_coverage: u8,
    pub flags: Vec<IntegrityFlag>,
    pub commitment_root: Option<String>,
    pub issued_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SignedIntegrityReport {
    pub report: IntegrityReport,
    pub signature: String,
}

/// One finished Sentinel session, as stored.
#[derive(Debug, Clone)]
pub struct CheckRun {
    pub purpose: String,
    pub status: String,
    pub integrity_score: Option<f64>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub commitment_root: Option<String>,
    /// Per snapshot: whether the camera was in use, and the flags it raised.
    pub snapshots: Vec<(bool, Vec<String>)>,
}

/// SQLite's `datetime('now')`, which is UTC.
fn sqlite_time(text: &str) -> Result<i64, String> {
    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
        .map(|t| t.and_utc().timestamp())
        .map_err(|_| format!("unreadable time {text:?}"))
}

/// Turn a finished interview-purpose session into the result the
/// organisation sees. Pure: the caller supplies the clock.
pub fn build_integrity_report(
    invite: InterviewInvite,
    run: &CheckRun,
    now: i64,
) -> Result<IntegrityReport, String> {
    if !invite.integrity_check || invite.mode != "alexandria" {
        return Err("this interview did not ask for an integrity check".into());
    }
    if run.purpose != "interview" {
        return Err("that check was not run for an interview".into());
    }
    let ended = run
        .ended_at
        .as_deref()
        .ok_or("the check is still running")?;
    let started_at = sqlite_time(&run.started_at)?;
    let ended_at = sqlite_time(ended)?;
    if started_at < invite.created_at {
        return Err("that check ran before this interview existed".into());
    }

    let mut counts: std::collections::BTreeMap<String, u32> = Default::default();
    let mut with_camera = 0usize;
    for (camera, flags) in &run.snapshots {
        with_camera += usize::from(*camera);
        let mut seen = std::collections::HashSet::new();
        for flag in flags {
            // Only the app's own flag vocabulary, counted once per snapshot.
            let word = !flag.is_empty()
                && flag.len() <= 64
                && flag
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if word && seen.insert(flag.as_str()) {
                *counts.entry(flag.clone()).or_default() += 1;
            }
        }
    }
    let flags: Vec<IntegrityFlag> = counts
        .into_iter()
        .take(64)
        .map(|(flag, count)| IntegrityFlag {
            severity: crate::sentinel::snapshots::flag_severity(&flag)
                .as_str()
                .into(),
            flag,
            count,
        })
        .collect();
    let serious = flags.iter().any(|f| f.severity != "info");
    let score = run
        .integrity_score
        .filter(|s| s.is_finite())
        .map(|s| (s.clamp(0.0, 1.0) * 100.0).round() as u8);
    let flagged = serious
        || score.is_some_and(|s| s < 40)
        || matches!(run.status.as_str(), "flagged" | "suspended");
    let total = run.snapshots.len();
    let camera = with_camera > 0;
    let camera_coverage = if total == 0 {
        0
    } else {
        ((with_camera * 100 + total / 2) / total) as u8
    };
    let commitment_root = run
        .commitment_root
        .clone()
        .filter(|r| !r.is_empty() && r.len() <= 128 && r.bytes().all(|b| b.is_ascii_hexdigit()));
    Ok(IntegrityReport {
        format: INTEGRITY_FORMAT.into(),
        invite,
        outcome: if flagged { "flagged" } else { "clear" }.into(),
        score,
        started_at,
        ended_at,
        snapshots: u32::try_from(total).unwrap_or(u32::MAX),
        camera,
        camera_coverage,
        flags,
        commitment_root,
        issued_at: now,
        expires_at: now + RESPONSE_LIFETIME_SECS,
    })
}

pub fn sign_integrity_report(
    report: IntegrityReport,
    key: &SigningKey,
    did: &str,
) -> Result<SignedIntegrityReport, String> {
    if report.invite.subject_did != did {
        return Err("this interview belongs to another identity".into());
    }
    let signature = URL_SAFE_NO_PAD.encode(
        key.sign(&signing_bytes_as(INTEGRITY_FORMAT, &report)?)
            .to_bytes(),
    );
    Ok(SignedIntegrityReport { report, signature })
}

fn load_check(conn: &rusqlite::Connection, session_id: &str) -> Result<CheckRun, String> {
    let mut run = conn
        .query_row(
            "SELECT purpose, status, integrity_score, started_at, ended_at, commitment_root
             FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| {
                Ok(CheckRun {
                    purpose: r.get(0)?,
                    status: r.get(1)?,
                    integrity_score: r.get(2)?,
                    started_at: r.get(3)?,
                    ended_at: r.get(4)?,
                    commitment_root: r.get(5)?,
                    snapshots: Vec::new(),
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "no such integrity check".to_string(),
            other => other.to_string(),
        })?;
    let mut stmt = conn
        .prepare(
            "SELECT camera_score IS NOT NULL, anomaly_flags FROM integrity_snapshots
             WHERE session_id = ?1 ORDER BY captured_at",
        )
        .map_err(|e| e.to_string())?;
    run.snapshots = stmt
        .query_map(params![session_id], |r| {
            let flags: Option<String> = r.get(1)?;
            Ok((
                r.get::<_, bool>(0)?,
                flags
                    .and_then(|f| serde_json::from_str(&f).ok())
                    .unwrap_or_default(),
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(run)
}

/// The accepted invitation behind an interview, from the learner's own
/// record of the answer they signed.
fn accepted_invite(
    conn: &rusqlite::Connection,
    interview_id: &str,
) -> Result<(String, InterviewInvite), String> {
    let (directory_url, payload): (String, String) = conn
        .query_row(
            "SELECT directory_url, payload_json FROM hiring_responses
             WHERE id = ?1 AND kind = 'interview' AND decision = 'accept'",
            params![interview_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "you have not accepted this interview".into(),
            other => other.to_string(),
        })?;
    let signed: SignedInterviewResponse =
        serde_json::from_str(&payload).map_err(|e| e.to_string())?;
    Ok((directory_url, signed.response.invite))
}

/// One result the learner sent, as they keep it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SentIntegrityReport {
    pub integrity_session_id: String,
    pub interview_id: String,
    pub organization: String,
    pub sent_at: String,
    pub report: IntegrityReport,
}

pub fn sent_integrity_reports(
    conn: &rusqlite::Connection,
    interview_id: &str,
) -> Result<Vec<SentIntegrityReport>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT integrity_session_id, interview_id, organization, sent_at, payload_json
             FROM interview_integrity_reports WHERE interview_id = ?1 ORDER BY sent_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![interview_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(session, interview, organization, sent_at, payload)| {
            let signed: SignedIntegrityReport =
                serde_json::from_str(&payload).map_err(|e| e.to_string())?;
            Ok(SentIntegrityReport {
                integrity_session_id: session,
                interview_id: interview,
                organization,
                sent_at,
                report: signed.report,
            })
        })
        .collect()
}

/// Send the organisation the result of the integrity check the learner ran
/// during an interview held in Alexandria, and keep a copy. A check already
/// sent is not sent again.
#[tauri::command]
pub async fn hiring_interview_integrity_send(
    state: State<'_, AppState>,
    interview_id: String,
    integrity_session_id: String,
) -> Result<SentIntegrityReport, String> {
    let id = uuid::Uuid::parse_str(&interview_id).map_err(|e| e.to_string())?;
    let lookup = (interview_id.clone(), integrity_session_id.clone());
    let (directory_url, invite, run, already) = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "hiring.integrity-load",
            move |db| {
                let (url, invite) = accepted_invite(db.conn(), &lookup.0)?;
                let run = load_check(db.conn(), &lookup.1)?;
                let already = sent_integrity_reports(db.conn(), &lookup.0)?
                    .into_iter()
                    .find(|r| r.integrity_session_id == lookup.1);
                Ok::<_, String>((url, invite, run, already))
            },
        )
        .await?;
    if let Some(sent) = already {
        return Ok(sent);
    }
    let dir = directory(&state, &directory_url).await?;
    let (key, did) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let report = build_integrity_report(invite, &run, chrono::Utc::now().timestamp())?;
    let signed = sign_integrity_report(report, &key, did.as_str())?;
    if !state.profile_lease().is_current() {
        return Err("profile changed".into());
    }
    let response = client()?
        .post(format!(
            "{}/api/interviews/{id}/integrity",
            dir.url.trim_end_matches('/')
        ))
        .json(&signed)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    response_json(response).await?;
    let sent = SentIntegrityReport {
        integrity_session_id,
        interview_id,
        organization: signed.report.invite.organization.clone(),
        sent_at: crate::clock::now_rfc3339(),
        report: signed.report.clone(),
    };
    let payload = serde_json::to_string(&signed).map_err(|e| e.to_string())?;
    let row = sent.clone();
    let cloud = dir.url.clone();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "hiring.integrity-record",
            move |db| {
                db.conn()
                    .execute(
                        "INSERT OR IGNORE INTO interview_integrity_reports
                         (integrity_session_id, interview_id, cloud_url, organization, sent_at, payload_json)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            row.integrity_session_id,
                            row.interview_id,
                            cloud,
                            row.organization,
                            row.sent_at,
                            payload
                        ],
                    )
                    .map_err(|e| e.to_string())
            },
        )
        .await?;
    Ok(sent)
}

/// The integrity results the learner has sent for one interview.
#[tauri::command]
pub async fn hiring_interview_integrity_sent(
    state: State<'_, AppState>,
    interview_id: String,
) -> Result<Vec<SentIntegrityReport>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "hiring.integrity-sent",
            move |db| sent_integrity_reports(db.conn(), &interview_id),
        )
        .await
}

pub fn history(conn: &rusqlite::Connection) -> Result<Vec<HiringRecord>, String> {
    let mut stmt = conn
        .prepare("SELECT id,kind,directory_url,organization,role_label,decision,chosen_slot,meeting_url,responded_at,payload_json FROM hiring_responses ORDER BY responded_at DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(HiringRecord {
                id: r.get(0)?,
                kind: r.get(1)?,
                directory_url: r.get(2)?,
                organization: r.get(3)?,
                role_label: r.get(4)?,
                decision: r.get(5)?,
                chosen_slot: r.get(6)?,
                meeting_url: r.get(7)?,
                responded_at: r.get(8)?,
                payload_json: r.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn hiring_history(state: State<'_, AppState>) -> Result<Vec<HiringRecord>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "hiring.history",
            |db| history(db.conn()),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::derive_did_key;
    use crate::db::Database;

    fn holder() -> (SigningKey, String) {
        let key = SigningKey::from_bytes(&[42; 32]);
        let did = derive_did_key(&key).as_str().to_string();
        (key, did)
    }

    fn invite(did: &str, now: i64) -> InterviewInvite {
        InterviewInvite {
            id: "11111111-1111-4111-8111-111111111111".into(),
            audience: "urn:alexandria:organization:test".into(),
            nonce: "nonce-1".into(),
            organization: "Alexandria Demo".into(),
            subject_did: did.into(),
            role_label: "Junior engineer".into(),
            message: "Let us talk about the assessment you completed.".into(),
            mode: "video".into(),
            proposed_slots: vec![now + 86_400, now + 2 * 86_400],
            meeting_url: Some("https://meet.example.test/room".into()),
            run_id: None,
            created_at: now - 60,
            expires_at: now + 14 * 86_400,
            integrity_check: false,
        }
    }

    fn checked(did: &str, now: i64) -> InterviewInvite {
        InterviewInvite {
            mode: "alexandria".into(),
            meeting_url: None,
            integrity_check: true,
            ..invite(did, now)
        }
    }

    /// A run that started an hour after `created`, with these snapshots.
    fn run(created: i64, status: &str, snapshots: Vec<(bool, Vec<&str>)>) -> CheckRun {
        let at = |t: i64| {
            chrono::DateTime::from_timestamp(t, 0)
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        };
        CheckRun {
            purpose: "interview".into(),
            status: status.into(),
            integrity_score: Some(0.874),
            started_at: at(created + 3600),
            ended_at: Some(at(created + 5400)),
            commitment_root: Some("ab".repeat(32)),
            snapshots: snapshots
                .into_iter()
                .map(|(c, f)| (c, f.into_iter().map(String::from).collect()))
                .collect(),
        }
    }

    fn offer(did: &str, now: i64) -> Offer {
        Offer {
            id: "22222222-2222-4222-8222-222222222222".into(),
            audience: "urn:alexandria:organization:test".into(),
            nonce: "nonce-2".into(),
            organization: "Alexandria Demo".into(),
            subject_did: did.into(),
            interview_id: "11111111-1111-4111-8111-111111111111".into(),
            role_label: "Junior engineer".into(),
            terms: "Full time, Bengaluru.".into(),
            start_date: Some("2026-11-09".into()),
            created_at: now - 60,
            expires_at: now + 14 * 86_400,
        }
    }

    /// The exact bytes Cloud verifies, so a drift on either side fails here.
    #[test]
    fn signing_bytes_are_format_nul_jcs() {
        let (key, did) = holder();
        let now = 1_800_000_000;
        let signed = sign_interview_response(
            invite(&did, now),
            "accept",
            Some(now + 86_400),
            Some("  Looking forward to it.  ".into()),
            &key,
            &did,
            now,
        )
        .unwrap();
        let bytes = signing_bytes(&signed.response).unwrap();
        let prefix = format!("{FORMAT}\0");
        assert!(bytes.starts_with(prefix.as_bytes()));
        let json = std::str::from_utf8(&bytes[prefix.len()..]).unwrap();
        assert!(json.starts_with("{\"chosen_slot\":"), "{json}");
        assert_eq!(
            signed.response.note.as_deref(),
            Some("Looking forward to it.")
        );
        assert_eq!(signed.response.expires_at, now + RESPONSE_LIFETIME_SECS);
        let verifying = derive_did_key(&key);
        assert_eq!(verifying.as_str(), did);
        check_signature(&signed.response, &signed.signature, &did).unwrap();
        let mut tampered = signed.clone();
        tampered.response.decision = "decline".into();
        assert_eq!(
            check_signature(&tampered.response, &tampered.signature, &did).unwrap_err(),
            "invalid holder signature"
        );
    }

    #[test]
    fn the_learner_cannot_answer_for_someone_else_or_with_a_bad_slot() {
        let (key, did) = holder();
        let now = 1_800_000_000;
        let mut other = invite(&did, now);
        other.subject_did = "did:key:z6MkSomeoneElse".into();
        assert!(
            sign_interview_response(other, "decline", None, None, &key, &did, now)
                .unwrap_err()
                .contains("another identity")
        );
        let bad_slot = sign_interview_response(
            invite(&did, now),
            "accept",
            Some(now + 99),
            None,
            &key,
            &did,
            now,
        )
        .unwrap_err();
        assert!(bad_slot.contains("proposed times"), "{bad_slot}");
        assert!(sign_interview_response(
            invite(&did, now),
            "decline",
            Some(now + 86_400),
            None,
            &key,
            &did,
            now
        )
        .is_err());
        assert!(
            sign_interview_response(invite(&did, now), "maybe", None, None, &key, &did, now)
                .is_err()
        );
        let mut expired = invite(&did, now);
        expired.expires_at = now - 1;
        assert!(sign_interview_response(expired, "decline", None, None, &key, &did, now).is_err());
        let mut unbound = invite(&did, now);
        unbound.nonce.clear();
        assert!(sign_interview_response(unbound, "decline", None, None, &key, &did, now).is_err());
        let fine = sign_interview_response(
            invite(&did, now),
            "decline",
            None,
            Some("   ".into()),
            &key,
            &did,
            now,
        )
        .unwrap();
        assert_eq!(fine.response.note, None);
    }

    #[test]
    fn offers_are_signed_the_same_way() {
        let (key, did) = holder();
        let now = 1_800_000_000;
        let signed =
            sign_offer_response(offer(&did, now), "accept", None, &key, &did, now).unwrap();
        assert_eq!(signed.response.format, FORMAT);
        check_signature(&signed.response, &signed.signature, &did).unwrap();
        let long = "x".repeat(2001);
        assert!(
            sign_offer_response(offer(&did, now), "accept", Some(long), &key, &did, now).is_err()
        );
        let wrong = SigningKey::from_bytes(&[7; 32]);
        assert!(sign_offer_response(offer(&did, now), "accept", None, &wrong, &did, now).is_err());
    }

    #[test]
    fn answers_are_kept_locally_newest_first() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let record = |id: &str, kind: &str, at: &str| HiringRecord {
            id: id.into(),
            kind: kind.into(),
            directory_url: "http://127.0.0.1:8787".into(),
            organization: "Alexandria Demo".into(),
            role_label: "Junior engineer".into(),
            decision: "accept".into(),
            chosen_slot: Some(1_900_000_000),
            meeting_url: Some("https://meet.example.test/room".into()),
            responded_at: at.into(),
            payload_json: "{}".into(),
        };
        store_record(db.conn(), &record("a", "interview", "2026-10-09T10:00:00Z")).unwrap();
        store_record(db.conn(), &record("b", "offer", "2026-10-10T10:00:00Z")).unwrap();
        store_record(db.conn(), &record("a", "interview", "2026-10-11T10:00:00Z")).unwrap();
        let kept = history(db.conn()).unwrap();
        assert_eq!(
            kept.len(),
            2,
            "re-sending the same answer replaces, not duplicates"
        );
        assert_eq!(kept[0].id, "a");
        assert_eq!(kept[0].responded_at, "2026-10-11T10:00:00Z");
        assert_eq!(kept[1].kind, "offer");
    }

    #[test]
    fn an_integrity_result_carries_counts_not_what_was_seen() {
        let (key, did) = holder();
        let now = 1_800_000_000;
        let invite = checked(&did, now - 7200);
        let r = run(
            invite.created_at,
            "completed",
            vec![
                (true, vec!["tab_switching"]),
                (true, vec!["tab_switching", "tab_switching", "Not A Flag"]),
                (false, vec![]),
                (true, vec!["external_display"]),
            ],
        );
        let report = build_integrity_report(invite.clone(), &r, now).unwrap();
        assert_eq!(report.outcome, "clear");
        assert_eq!(report.score, Some(87));
        assert_eq!(report.snapshots, 4);
        assert!(report.camera);
        assert_eq!(report.camera_coverage, 75);
        assert_eq!(report.ended_at - report.started_at, 1800);
        assert_eq!(
            report.flags,
            vec![
                IntegrityFlag {
                    flag: "external_display".into(),
                    severity: "info".into(),
                    count: 1
                },
                IntegrityFlag {
                    flag: "tab_switching".into(),
                    severity: "info".into(),
                    count: 2
                },
            ],
            "counted once per snapshot; a word outside the vocabulary is dropped"
        );
        let signed = sign_integrity_report(report, &key, &did).unwrap();
        let bytes = signing_bytes_as(INTEGRITY_FORMAT, &signed.report).unwrap();
        let sig =
            Signature::from_slice(&URL_SAFE_NO_PAD.decode(&signed.signature).unwrap()).unwrap();
        resolve_did_key(&Did(did.clone()))
            .unwrap()
            .verify(&bytes, &sig)
            .unwrap();
        // Not a signature an interview answer would accept.
        assert!(check_signature(&signed.report, &signed.signature, &did).is_err());
        let (_, stranger) = {
            let k = SigningKey::from_bytes(&[9; 32]);
            let d = crate::crypto::did::derive_did_key(&k).as_str().to_string();
            (k, d)
        };
        assert!(sign_integrity_report(signed.report, &key, &stranger).is_err());
    }

    #[test]
    fn a_warning_makes_the_result_flagged() {
        let (_, did) = holder();
        let now = 1_800_000_000;
        let invite = checked(&did, now - 7200);
        let r = run(
            invite.created_at,
            "completed",
            vec![(false, vec!["app_switch"]), (false, vec!["app_switch"])],
        );
        let report = build_integrity_report(invite, &r, now).unwrap();
        assert_eq!(report.outcome, "flagged");
        assert!(!report.camera);
        assert_eq!(report.camera_coverage, 0);
        assert_eq!(report.flags[0].severity, "warning");
        assert_eq!(report.flags[0].count, 2);
    }

    #[test]
    fn only_a_finished_interview_check_for_an_interview_that_asked() {
        let (_, did) = holder();
        let now = 1_800_000_000;
        let asked = checked(&did, now - 7200);
        let mut unasked = asked.clone();
        unasked.integrity_check = false;
        let ok = run(asked.created_at, "completed", vec![]);
        assert!(build_integrity_report(unasked, &ok, now).is_err());

        let mut assessment = ok.clone();
        assessment.purpose = "assessment".into();
        assert!(build_integrity_report(asked.clone(), &assessment, now).is_err());

        let mut running = ok.clone();
        running.ended_at = None;
        assert_eq!(
            build_integrity_report(asked.clone(), &running, now).unwrap_err(),
            "the check is still running"
        );

        let before = run(asked.created_at - 7200, "completed", vec![]);
        assert!(build_integrity_report(asked, &before, now).is_err());
    }

    #[test]
    fn an_invitation_that_never_asked_signs_as_before() {
        let (_, did) = holder();
        let plain = invite(&did, 1_800_000_000);
        assert!(serde_json::to_value(&plain)
            .unwrap()
            .get("integrity_check")
            .is_none());
        let parsed: InterviewInvite =
            serde_json::from_value(serde_json::to_value(checked(&did, 1_800_000_000)).unwrap())
                .unwrap();
        assert!(parsed.integrity_check);
    }

    #[test]
    fn the_learner_keeps_what_they_sent() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let (key, did) = holder();
        let now = 1_800_000_000;
        let invite = checked(&did, now - 7200);
        db.conn()
            .execute(
                "INSERT INTO integrity_sessions (id, status, purpose) VALUES ('s1', 'completed', 'interview')",
                [],
            )
            .unwrap();
        let report = build_integrity_report(
            invite.clone(),
            &run(invite.created_at, "completed", vec![]),
            now,
        )
        .unwrap();
        let signed = sign_integrity_report(report, &key, &did).unwrap();
        db.conn()
            .execute(
                "INSERT INTO interview_integrity_reports VALUES ('s1', ?1, 'https://cloud.test', 'Alexandria Demo', '2026-10-12T10:00:00Z', ?2)",
                params![invite.id, serde_json::to_string(&signed).unwrap()],
            )
            .unwrap();
        let kept = sent_integrity_reports(db.conn(), &invite.id).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].report, signed.report);
        assert!(sent_integrity_reports(db.conn(), "other")
            .unwrap()
            .is_empty());
    }
}
