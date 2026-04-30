//! IPC commands for selective disclosure + presentations.
//!
//! Spec §18 + §23.3. The MVP encoding is "redact-and-resign":
//!
//! 1. Each referenced credential is loaded and its JSON is filtered
//!    to keep only the structural keys (`@context`, `id`, `type`,
//!    `issuer`, `issuance_date`, `credential_subject.id`) plus any
//!    paths the caller listed in `reveal`.
//! 2. The redacted bundle, the audience, and the nonce are
//!    canonicalized via JCS.
//! 3. The subject signs those bytes with their Ed25519 key. The
//!    detached JWS goes in `envelope.proof`.
//!
//! Because we redact, the original issuer signature on each
//! credential no longer covers the visible payload — the
//! presentation's authority is the subject's signature instead.
//! Real BBS+/zk-style selective disclosure that preserves the
//! issuer's signature is the §18.3 follow-up.
//!
//! Replay protection: every successful `verify_presentation_impl`
//! records `(audience, nonce)` in `presentations_seen`. A second
//! verification with the same pair returns `Replayed`.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rusqlite::{params, Connection, OptionalExtension};
use tauri::State;
use uuid::Uuid;

use crate::crypto::did::{derive_did_key, parse_did_key, resolve_did_key, Did};
use crate::crypto::wallet;
use crate::AppState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreatePresentationRequest {
    pub credential_ids: Vec<String>,
    /// Dot-separated field paths to reveal (e.g.
    /// `["credentialSubject.level"]`). Paths use the on-disk W3C VC v2
    /// camelCase shape of the VC, matching the storage format.
    pub reveal: Vec<String>,
    pub audience: String,
    pub nonce: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PresentationEnvelope {
    pub id: String,
    /// JCS-canonical JSON containing the redacted credential bundle
    /// plus the audience and nonce that bound the presentation.
    pub payload_json: String,
    /// Detached Ed25519 JWS over `payload_json`, signed by the
    /// subject's signing key. Format: `header..signature`.
    pub proof: String,
    /// Subject DID — the verifier resolves this to a public key
    /// to check `proof` against `payload_json`.
    pub subject: String,
}

/// Verification outcome from `verify_presentation_impl`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationVerification {
    Accepted,
    BadSignature,
    AudienceMismatch,
    Replayed,
    Malformed,
}

/// Always-included structural keys per credential — without these
/// the verifier can't even tell which credential it's looking at.
const STRUCTURAL_KEYS: &[&str] = &[
    "@context",
    "id",
    "type",
    "issuer",
    "validFrom",
    "validUntil",
];

/// Build a presentation envelope. Pure function — no AppState.
pub fn create_presentation_impl(
    conn: &Connection,
    subject_signing_key: &SigningKey,
    subject_did: &Did,
    req: &CreatePresentationRequest,
) -> Result<PresentationEnvelope, String> {
    if req.audience.trim().is_empty() {
        return Err("audience MUST be non-empty (§23.3 audience binding)".into());
    }
    if req.nonce.trim().is_empty() {
        return Err("nonce MUST be non-empty (§23.3 replay resistance)".into());
    }

    // Load + redact each referenced credential.
    let mut redacted: Vec<serde_json::Value> = Vec::with_capacity(req.credential_ids.len());
    for id in &req.credential_ids {
        let json: Option<String> = conn
            .query_row(
                "SELECT signed_vc_json FROM credentials WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let raw = json.ok_or_else(|| format!("credential {id} not found"))?;
        let value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("parse VC: {e}"))?;
        // Subject binding (§10): MUST be a presentation by the same
        // subject — refuse to wrap someone else's credential.
        if let Some(cs_id) = value
            .pointer("/credentialSubject/id")
            .and_then(|v| v.as_str())
        {
            if cs_id != subject_did.as_str() {
                return Err(format!(
                    "credential {id} subject {cs_id} does not match presenter {}",
                    subject_did.as_str()
                ));
            }
        }
        redacted.push(redact(&value, &req.reveal));
    }

    let id = format!("urn:uuid:{}", Uuid::new_v4());
    let payload = serde_json::json!({
        "id": id,
        "audience": req.audience,
        "nonce": req.nonce,
        "credentials": redacted,
    });
    let payload_json = serde_json_canonicalizer::to_string(&payload)
        .map_err(|e| format!("canonicalize payload: {e}"))?;

    // Detached JWS over the canonical payload.
    let header_b64 = b64url(br#"{"alg":"EdDSA","b64":false,"crit":["b64"]}"#);
    let mut signing_input = Vec::with_capacity(header_b64.len() + 1 + payload_json.len());
    signing_input.extend_from_slice(header_b64.as_bytes());
    signing_input.push(b'.');
    signing_input.extend_from_slice(payload_json.as_bytes());
    let sig = subject_signing_key.sign(&signing_input);
    let proof = format!("{header_b64}..{}", b64url(&sig.to_bytes()));

    Ok(PresentationEnvelope {
        id,
        payload_json,
        proof,
        subject: subject_did.as_str().to_string(),
    })
}

/// Verify an envelope and consume its (audience, nonce) slot. Idempotent
/// for the rejection path; the first acceptance records the pair, so
/// any replay returns `Replayed`.
pub fn verify_presentation_impl(
    conn: &Connection,
    envelope: &PresentationEnvelope,
    expected_audience: &str,
) -> Result<PresentationVerification, String> {
    // Parse the canonical payload to extract audience + nonce.
    let payload: serde_json::Value = match serde_json::from_str(&envelope.payload_json) {
        Ok(v) => v,
        Err(_) => return Ok(PresentationVerification::Malformed),
    };
    let payload_audience = payload.get("audience").and_then(|v| v.as_str());
    let nonce = payload.get("nonce").and_then(|v| v.as_str());
    let (Some(payload_audience), Some(nonce)) = (payload_audience, nonce) else {
        return Ok(PresentationVerification::Malformed);
    };
    if payload_audience != expected_audience {
        return Ok(PresentationVerification::AudienceMismatch);
    }

    // Replay check first — even if signature is bad, an attacker
    // shouldn't be able to probe nonce reuse based on response time.
    // We test seen-state without recording yet; recording happens
    // only after a successful signature check.
    let seen: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM presentations_seen WHERE audience = ?1 AND nonce = ?2",
            params![payload_audience, nonce],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if seen > 0 {
        return Ok(PresentationVerification::Replayed);
    }

    // Resolve subject's verifying key from envelope.subject.
    let subject_did = Did(envelope.subject.clone());
    if parse_did_key(subject_did.as_str()).is_err() {
        return Ok(PresentationVerification::Malformed);
    }
    let vk: VerifyingKey = match resolve_did_key(&subject_did) {
        Ok(k) => k,
        Err(_) => return Ok(PresentationVerification::Malformed),
    };

    // Verify the detached JWS over payload_json.
    let parts: Vec<&str> = envelope.proof.split('.').collect();
    if parts.len() != 3 || !parts[1].is_empty() {
        return Ok(PresentationVerification::BadSignature);
    }
    let sig_bytes = match b64url_decode(parts[2]) {
        Some(b) if b.len() == 64 => b,
        _ => return Ok(PresentationVerification::BadSignature),
    };
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
    let mut signing_input = Vec::with_capacity(parts[0].len() + 1 + envelope.payload_json.len());
    signing_input.extend_from_slice(parts[0].as_bytes());
    signing_input.push(b'.');
    signing_input.extend_from_slice(envelope.payload_json.as_bytes());
    if vk.verify_strict(&signing_input, &sig).is_err() {
        return Ok(PresentationVerification::BadSignature);
    }

    // Record the (audience, nonce) pair. INSERT OR IGNORE in case
    // of a TOCTOU race with a parallel verifier — the first writer
    // wins, the second sees the row on its next call and returns
    // Replayed.
    conn.execute(
        "INSERT OR IGNORE INTO presentations_seen (audience, nonce) VALUES (?1, ?2)",
        params![payload_audience, nonce],
    )
    .map_err(|e| e.to_string())?;
    Ok(PresentationVerification::Accepted)
}

// ---- helpers -------------------------------------------------------------

/// Build a redacted JSON copy keeping only the structural keys plus
/// the dot-separated `reveal` paths. Paths can target nested keys
/// inside objects — `credentialSubject.level` keeps `level` inside
/// `credentialSubject`. Arrays pass through the same path filter
/// element-wise.
fn redact(value: &serde_json::Value, reveal: &[String]) -> serde_json::Value {
    let mut keep_paths: Vec<Vec<String>> = STRUCTURAL_KEYS
        .iter()
        .map(|k| vec![(*k).to_string()])
        .collect();
    // Subject id is always preserved (§10) — without it, the
    // verifier can't bind the presentation to a presenter.
    keep_paths.push(vec!["credentialSubject".into(), "id".into()]);
    for r in reveal {
        keep_paths.push(r.split('.').map(String::from).collect());
    }
    redact_recursive(value, &[], &keep_paths)
}

fn redact_recursive(
    value: &serde_json::Value,
    here: &[String],
    keep: &[Vec<String>],
) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                let mut child = here.to_vec();
                child.push(k.clone());
                if path_is_kept(&child, keep) {
                    out.insert(k.clone(), v.clone());
                } else if path_has_descendant_kept(&child, keep) {
                    out.insert(k.clone(), redact_recursive(v, &child, keep));
                }
            }
            serde_json::Value::Object(out)
        }
        // Arrays inherit the parent's path-filter context.
        serde_json::Value::Array(items) => {
            let kept: Vec<_> = items
                .iter()
                .map(|v| redact_recursive(v, here, keep))
                .collect();
            serde_json::Value::Array(kept)
        }
        _ => value.clone(),
    }
}

fn path_is_kept(path: &[String], keep: &[Vec<String>]) -> bool {
    keep.iter().any(|p| p == path)
}

/// True if any kept path starts with `path` (i.e. `path` is an
/// ancestor of something we want to reveal).
fn path_has_descendant_kept(path: &[String], keep: &[Vec<String>]) -> bool {
    keep.iter()
        .any(|p| p.len() > path.len() && p[..path.len()] == path[..])
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .ok()
}

async fn load_subject_key(state: &State<'_, AppState>) -> Result<(SigningKey, Did), String> {
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(ks_guard);
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    let signing_key = SigningKey::from_bytes(&w.signing_key.to_bytes());
    let did = derive_did_key(&signing_key);
    Ok((signing_key, did))
}

#[tauri::command]
pub async fn create_presentation(
    state: State<'_, AppState>,
    req: CreatePresentationRequest,
) -> Result<PresentationEnvelope, String> {
    let (signing_key, subject_did) = load_subject_key(&state).await?;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    create_presentation_impl(db.conn(), &signing_key, &subject_did, &req)
}

#[tauri::command]
pub async fn verify_presentation(
    state: State<'_, AppState>,
    envelope: PresentationEnvelope,
    audience: String,
) -> Result<PresentationVerification, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    verify_presentation_impl(db.conn(), &envelope, &audience)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::credentials::{issue_credential_impl, IssueCredentialRequest};
    use crate::db::Database;
    use crate::domain::vc::{Claim, CredentialType, SkillClaim};

    const NOW: &str = "2026-04-13T00:00:00Z";

    #[test]
    fn create_presentation_request_round_trips() {
        let req = CreatePresentationRequest {
            credential_ids: vec!["urn:uuid:c1".into(), "urn:uuid:c2".into()],
            reveal: vec!["credentialSubject.level".into()],
            audience: "did:web:hirer.example".into(),
            nonce: "nonce-1234".into(),
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: CreatePresentationRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back.credential_ids, req.credential_ids);
        assert_eq!(back.reveal, req.reveal);
        assert_eq!(back.audience, req.audience);
        assert_eq!(back.nonce, req.nonce);
    }

    #[test]
    fn presentation_envelope_round_trips() {
        let env = PresentationEnvelope {
            id: "pres-1".into(),
            payload_json: "{\"sub\":\"did:key:z\"}".into(),
            proof: "sig".into(),
            subject: "did:key:zSubject".into(),
        };
        let s = serde_json::to_string(&env).unwrap();
        let back: PresentationEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, env.id);
        assert_eq!(back.payload_json, env.payload_json);
        assert_eq!(back.proof, env.proof);
        assert_eq!(back.subject, env.subject);
    }

    fn test_key(role: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let b = role.as_bytes();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = b[i % b.len().max(1)];
        }
        SigningKey::from_bytes(&bytes)
    }

    fn setup_with_credential() -> (Database, SigningKey, Did, String) {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let issuer_key = test_key("issuer");
        let issuer = derive_did_key(&issuer_key);
        let subject_key = test_key("subject");
        let subject = derive_did_key(&subject_key);
        let req = IssueCredentialRequest {
            credential_type: CredentialType::FormalCredential,
            subject: subject.clone(),
            claim: Claim::Skill(SkillClaim {
                skill_id: "skill_pres".into(),
                level: 4,
                score: 0.92,
                evidence_refs: vec!["urn:uuid:e1".into()],
                rubric_version: Some("v1".into()),
                assessment_method: Some("exam".into()),
            }),
            evidence_refs: vec!["urn:uuid:e1".into()],
            expiration_date: None,
            supersedes: None,
        };
        let vc = issue_credential_impl(db.conn(), &issuer_key, &issuer, &req, NOW).unwrap();
        (db, subject_key, subject, vc.id.unwrap())
    }

    #[test]
    fn presentation_redacts_unrevealed_fields() {
        // `reveal = ["credentialSubject.level"]` keeps `level`
        // but drops `score` and `evidenceRefs` from the payload.
        let (db, subject_key, subject, cred_id) = setup_with_credential();
        let req = CreatePresentationRequest {
            credential_ids: vec![cred_id],
            reveal: vec!["credentialSubject.level".into()],
            audience: "did:web:hirer".into(),
            nonce: "n-1".into(),
        };
        let env = create_presentation_impl(db.conn(), &subject_key, &subject, &req).unwrap();
        assert!(!env.payload_json.contains("\"score\""));
        assert!(!env.payload_json.contains("evidenceRefs"));
        assert!(env.payload_json.contains("\"level\""));
    }

    #[test]
    fn presentation_verify_round_trip_accepts() {
        let (db, subject_key, subject, cred_id) = setup_with_credential();
        let req = CreatePresentationRequest {
            credential_ids: vec![cred_id],
            reveal: vec!["credentialSubject.level".into()],
            audience: "did:web:hirer".into(),
            nonce: "n-2".into(),
        };
        let env = create_presentation_impl(db.conn(), &subject_key, &subject, &req).unwrap();
        let outcome = verify_presentation_impl(db.conn(), &env, "did:web:hirer").unwrap();
        assert_eq!(outcome, PresentationVerification::Accepted);
    }

    #[test]
    fn replay_with_same_audience_nonce_is_rejected() {
        let (db, subject_key, subject, cred_id) = setup_with_credential();
        let req = CreatePresentationRequest {
            credential_ids: vec![cred_id],
            reveal: vec!["credentialSubject.level".into()],
            audience: "did:web:hirer".into(),
            nonce: "n-replay".into(),
        };
        let env = create_presentation_impl(db.conn(), &subject_key, &subject, &req).unwrap();
        assert_eq!(
            verify_presentation_impl(db.conn(), &env, "did:web:hirer").unwrap(),
            PresentationVerification::Accepted
        );
        assert_eq!(
            verify_presentation_impl(db.conn(), &env, "did:web:hirer").unwrap(),
            PresentationVerification::Replayed
        );
    }

    #[test]
    fn audience_mismatch_is_rejected() {
        let (db, subject_key, subject, cred_id) = setup_with_credential();
        let req = CreatePresentationRequest {
            credential_ids: vec![cred_id],
            reveal: vec!["credentialSubject.level".into()],
            audience: "did:web:hirer-A".into(),
            nonce: "n-aud".into(),
        };
        let env = create_presentation_impl(db.conn(), &subject_key, &subject, &req).unwrap();
        let outcome = verify_presentation_impl(db.conn(), &env, "did:web:hirer-B").unwrap();
        assert_eq!(outcome, PresentationVerification::AudienceMismatch);
    }

    #[test]
    fn rejects_empty_audience_or_nonce() {
        let (db, subject_key, subject, cred_id) = setup_with_credential();
        let req = CreatePresentationRequest {
            credential_ids: vec![cred_id.clone()],
            reveal: vec!["credentialSubject.level".into()],
            audience: "".into(),
            nonce: "n".into(),
        };
        assert!(create_presentation_impl(db.conn(), &subject_key, &subject, &req).is_err());

        let req2 = CreatePresentationRequest {
            credential_ids: vec![cred_id],
            reveal: vec!["credentialSubject.level".into()],
            audience: "did:web:hirer".into(),
            nonce: "".into(),
        };
        assert!(create_presentation_impl(db.conn(), &subject_key, &subject, &req2).is_err());
    }

    #[test]
    fn refuses_to_present_other_subjects_credential() {
        // §10 non-transferability: the presenter MUST be the subject
        // of every credential they wrap.
        let (db, _other_key, _other_did, cred_id) = setup_with_credential();
        let imposter_key = test_key("imposter");
        let imposter_did = derive_did_key(&imposter_key);
        let req = CreatePresentationRequest {
            credential_ids: vec![cred_id],
            reveal: vec!["credentialSubject.level".into()],
            audience: "did:web:hirer".into(),
            nonce: "n-imposter".into(),
        };
        assert!(create_presentation_impl(db.conn(), &imposter_key, &imposter_did, &req).is_err());
    }
}
