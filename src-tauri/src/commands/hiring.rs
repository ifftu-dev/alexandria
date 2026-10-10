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

use crate::commands::credentials::load_issuer_key;
use crate::commands::exchange::{client, directory, response_json};
use crate::commands::holder_pull::{Directory, Problem, PullResult};
use crate::crypto::did::{resolve_did_key, Did};
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryOffer {
    pub directory_url: String,
    pub offer: Offer,
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
    let mut bytes = FORMAT.as_bytes().to_vec();
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
    let (key, _) = load_issuer_key(state).await?;
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
    let (_, did) = load_issuer_key(&state).await?;
    let path = format!("/api/interviews/for/{}", did.as_str());
    let dirs = super::holder_pull::list_directories(state.clone()).await?;
    let mut items = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs {
        match async {
            let resolved = directory(&state, &dir.url).await?;
            signed_get::<InterviewInvite>(&state, &resolved, &path).await
        }
        .await
        {
            Ok(found) => items.extend(
                found
                    .into_iter()
                    .filter(|i| {
                        i.subject_did == did.as_str()
                            && i.expires_at > chrono::Utc::now().timestamp()
                    })
                    .map(|invite| DirectoryInterview {
                        directory_url: dir.url.clone(),
                        invite,
                    }),
            ),
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
    let (_, did) = load_issuer_key(&state).await?;
    let path = format!("/api/offers/for/{}", did.as_str());
    let dirs = super::holder_pull::list_directories(state.clone()).await?;
    let mut items = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs {
        match async {
            let resolved = directory(&state, &dir.url).await?;
            signed_get::<Offer>(&state, &resolved, &path).await
        }
        .await
        {
            Ok(found) => items.extend(
                found
                    .into_iter()
                    .filter(|o| {
                        o.subject_did == did.as_str()
                            && o.expires_at > chrono::Utc::now().timestamp()
                    })
                    .map(|offer| DirectoryOffer {
                        directory_url: dir.url.clone(),
                        offer,
                    }),
            ),
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
    decision: String,
    chosen_slot: Option<i64>,
    note: Option<String>,
) -> Result<serde_json::Value, String> {
    let dir = directory(&state, &directory_url).await?;
    let (key, did) = load_issuer_key(&state).await?;
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
        responded_at: super::credentials::now_rfc3339(),
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
    decision: String,
    note: Option<String>,
) -> Result<serde_json::Value, String> {
    let dir = directory(&state, &directory_url).await?;
    let (key, did) = load_issuer_key(&state).await?;
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
        responded_at: super::credentials::now_rfc3339(),
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
}
