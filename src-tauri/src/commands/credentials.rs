//! IPC commands for Verifiable Credentials.
//!
//! The public `#[tauri::command]` handlers are thin adapters — they
//! unlock the keystore, derive the issuer's signing key, and delegate
//! to pure functions that take `&Connection` + `&SigningKey` +
//! `&Did`. This split keeps the business logic unit-testable without
//! constructing a full `State<AppState>`.

use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};
use tauri::State;
use uuid::Uuid;

use crate::crypto::did::{derive_did_key, Did, VerificationMethodRef};
use crate::crypto::wallet;
use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
use crate::domain::vc::{
    Claim, CredentialStatus, CredentialSubject, CredentialType, Proof, VerifiableCredential,
    VerificationResult,
};
use crate::AppState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IssueCredentialRequest {
    pub credential_type: CredentialType,
    pub subject: Did,
    pub claim: Claim,
    pub evidence_refs: Vec<String>,
    pub expiration_date: Option<String>,
}

const STATUS_LIST_BITS: usize = 16_384; // 2 KiB bitmap per list
const STATUS_LIST_TYPE: &str = "RevocationList2020Status";
const W3C_VC_V1: &str = "https://www.w3.org/2018/credentials/v1";
const ALEXANDRIA_V1: &str = "https://alexandria.protocol/context/v1";

/// Pure-function issuance pipeline. Allocates the next status-list
/// slot, builds the VC envelope, signs it, persists both the signed
/// VC and its status-list slot, and returns the signed credential.
pub fn issue_credential_impl(
    conn: &Connection,
    issuer_key: &SigningKey,
    issuer_did: &Did,
    req: &IssueCredentialRequest,
    now: &str,
) -> Result<VerifiableCredential, String> {
    if !req.subject.as_str().starts_with("did:") {
        return Err("subject MUST be a DID (§10 non-transferability)".into());
    }

    let list_id = ensure_status_list(conn, issuer_did)?;
    let index = allocate_status_index(conn, &list_id)?;

    let credential_id = format!("urn:uuid:{}", Uuid::new_v4());
    let type_name = serde_plain_variant(&req.credential_type);

    // Build the VC envelope; sign_credential will stamp proof.jws.
    let mut claim = req.claim.clone();
    if let Claim::Skill(ref mut s) = claim {
        s.evidence_refs = req.evidence_refs.clone();
    }

    let vc = VerifiableCredential {
        context: vec![W3C_VC_V1.into(), ALEXANDRIA_V1.into()],
        id: credential_id.clone(),
        type_: vec!["VerifiableCredential".into(), type_name.clone()],
        issuer: issuer_did.clone(),
        issuance_date: now.to_string(),
        expiration_date: req.expiration_date.clone(),
        credential_subject: CredentialSubject {
            id: req.subject.clone(),
            claim: claim.clone(),
        },
        credential_status: Some(CredentialStatus {
            id: format!("{list_id}#{index}"),
            type_: STATUS_LIST_TYPE.into(),
            status_purpose: "revocation".into(),
            status_list_index: index.to_string(),
            status_list_credential: list_id.clone(),
        }),
        terms_of_use: None,
        proof: Proof {
            type_: "Ed25519Signature2020".into(),
            created: now.to_string(),
            verification_method: VerificationMethodRef(format!("{}#key-1", issuer_did.as_str())),
            proof_purpose: "assertionMethod".into(),
            jws: String::new(),
        },
    };
    let signed = sign_credential(
        UnsignedCredential { credential: vc },
        issuer_key,
        issuer_did,
    )
    .map_err(|e| format!("sign: {e}"))?;

    let signed_json = serde_json::to_string(&signed).map_err(|e| e.to_string())?;
    let integrity_hash = integrity_hash_of(&signed)?;

    let (claim_kind, skill_id) = match &claim {
        Claim::Skill(s) => ("skill", Some(s.skill_id.clone())),
        Claim::Role(_) => ("role", None),
        Claim::Custom(_) => ("custom", None),
    };

    conn.execute(
        "INSERT INTO credentials \
         (id, issuer_did, subject_did, credential_type, claim_kind, skill_id, \
          issuance_date, expiration_date, signed_vc_json, integrity_hash, \
          status_list_id, status_list_index) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            credential_id,
            issuer_did.as_str(),
            req.subject.as_str(),
            type_name,
            claim_kind,
            skill_id,
            now,
            req.expiration_date,
            signed_json,
            integrity_hash,
            list_id,
            index,
        ],
    )
    .map_err(|e| format!("insert credential: {e}"))?;

    Ok(signed)
}

/// Flip the revocation bit in the issuer's status list and mark the
/// local `credentials` row as revoked. Idempotent — calling it twice
/// leaves the bit set and the row flagged.
pub fn revoke_credential_impl(
    conn: &Connection,
    credential_id: &str,
    reason: &str,
    now: &str,
) -> Result<(), String> {
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT status_list_id, status_list_index FROM credentials \
             WHERE id = ?1",
            params![credential_id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<i64>>(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .and_then(|(lid, idx)| match (lid, idx) {
            (Some(l), Some(i)) => Some((l, i)),
            _ => None,
        });

    let (list_id, index) =
        row.ok_or_else(|| format!("credential {credential_id} not found or has no status list"))?;

    // Read current bits, set the revocation bit, write back + bump version.
    let mut bits: Vec<u8> = conn
        .query_row(
            "SELECT bits FROM credential_status_lists WHERE list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("load status list: {e}"))?;
    let byte = (index / 8) as usize;
    let bit = (index % 8) as u8;
    if byte >= bits.len() {
        return Err(format!("status index {index} out of range"));
    }
    bits[byte] |= 1 << bit;

    conn.execute(
        "UPDATE credential_status_lists \
         SET bits = ?2, version = version + 1, updated_at = ?3 \
         WHERE list_id = ?1",
        params![list_id, bits, now],
    )
    .map_err(|e| format!("update status list: {e}"))?;

    conn.execute(
        "UPDATE credentials \
         SET revoked = 1, revoked_at = ?2, revocation_reason = ?3 \
         WHERE id = ?1",
        params![credential_id, now, reason],
    )
    .map_err(|e| format!("update credential: {e}"))?;

    Ok(())
}

pub fn get_credential_impl(
    conn: &Connection,
    credential_id: &str,
) -> Result<Option<VerifiableCredential>, String> {
    let json: Option<String> = conn
        .query_row(
            "SELECT signed_vc_json FROM credentials WHERE id = ?1",
            params![credential_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match json {
        Some(s) => Ok(Some(serde_json::from_str(&s).map_err(|e| e.to_string())?)),
        None => Ok(None),
    }
}

pub fn list_credentials_impl(
    conn: &Connection,
    subject_did: Option<&str>,
    skill_id: Option<&str>,
) -> Result<Vec<VerifiableCredential>, String> {
    let mut sql = String::from("SELECT signed_vc_json FROM credentials WHERE 1=1");
    let mut args: Vec<String> = Vec::new();
    if let Some(s) = subject_did {
        sql.push_str(" AND subject_did = ?");
        args.push(s.to_string());
    }
    if let Some(k) = skill_id {
        sql.push_str(" AND skill_id = ?");
        args.push(k.to_string());
    }
    sql.push_str(" ORDER BY received_at DESC");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args.iter()), |r| {
            r.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        let s = r.map_err(|e| e.to_string())?;
        out.push(serde_json::from_str(&s).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

// --- internal helpers -----------------------------------------------------

fn ensure_status_list(conn: &Connection, issuer_did: &Did) -> Result<String, String> {
    // One list per issuer (MVP). list_id is a stable URN so verifiers
    // can look it up from the credential's credentialStatus.statusListCredential.
    let list_id = format!("urn:alexandria:status-list:{}:1", issuer_did.as_str());
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM credential_status_lists WHERE list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        let bits = vec![0u8; STATUS_LIST_BITS / 8];
        conn.execute(
            "INSERT INTO credential_status_lists \
             (list_id, issuer_did, version, status_purpose, bits, bit_length) \
             VALUES (?1, ?2, 1, 'revocation', ?3, ?4)",
            params![list_id, issuer_did.as_str(), bits, STATUS_LIST_BITS as i64],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(list_id)
}

fn allocate_status_index(conn: &Connection, list_id: &str) -> Result<i64, String> {
    // Next free index = max allocated + 1. We read from `credentials`
    // rather than scanning the bitmap because gaps from revocations
    // shouldn't be reused (the revoked state is permanent evidence).
    let next: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(status_list_index), -1) + 1 FROM credentials \
             WHERE status_list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if next >= STATUS_LIST_BITS as i64 {
        return Err(format!("status list {list_id} is full"));
    }
    Ok(next)
}

fn serde_plain_variant(t: &CredentialType) -> String {
    // CredentialType serializes as PascalCase JSON string like
    // `"FormalCredential"`; strip the quotes to get the bare variant.
    let s = serde_json::to_string(t).unwrap_or_default();
    s.trim_matches('"').to_string()
}

fn integrity_hash_of(vc: &VerifiableCredential) -> Result<String, String> {
    let mut clone = vc.clone();
    clone.proof.jws.clear();
    let value = serde_json::to_value(&clone).map_err(|e| e.to_string())?;
    let bytes = serde_json_canonicalizer::to_vec(&value).map_err(|e| e.to_string())?;
    Ok(hex::encode(blake3::hash(&bytes).as_bytes()))
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

async fn load_issuer_key(state: &State<'_, AppState>) -> Result<(SigningKey, Did), String> {
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(ks_guard);
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    // `Wallet` implements `Drop` (zeroize) so we can't move out — clone
    // the signing key bytes instead.
    let signing_key = SigningKey::from_bytes(&w.signing_key.to_bytes());
    let issuer_did = derive_did_key(&signing_key);
    Ok((signing_key, issuer_did))
}

// --- tauri command handlers ----------------------------------------------

#[tauri::command]
pub async fn issue_credential(
    state: State<'_, AppState>,
    req: IssueCredentialRequest,
) -> Result<VerifiableCredential, String> {
    let (signing_key, issuer_did) = load_issuer_key(&state).await?;
    let now = now_rfc3339();
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    issue_credential_impl(db.conn(), &signing_key, &issuer_did, &req, &now)
}

#[tauri::command]
pub async fn list_credentials(
    state: State<'_, AppState>,
    subject: Option<String>,
    skill_id: Option<String>,
) -> Result<Vec<VerifiableCredential>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    list_credentials_impl(db.conn(), subject.as_deref(), skill_id.as_deref())
}

#[tauri::command]
pub async fn get_credential(
    state: State<'_, AppState>,
    credential_id: String,
) -> Result<Option<VerifiableCredential>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    get_credential_impl(db.conn(), &credential_id)
}

#[tauri::command]
pub async fn revoke_credential(
    state: State<'_, AppState>,
    credential_id: String,
    reason: String,
) -> Result<(), String> {
    let now = now_rfc3339();
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    revoke_credential_impl(db.conn(), &credential_id, &reason, &now)
}

#[tauri::command]
pub async fn verify_credential_cmd(
    state: State<'_, AppState>,
    credential: VerifiableCredential,
) -> Result<VerificationResult, String> {
    let now = now_rfc3339();
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    Ok(crate::domain::vc::verify::verify_credential(
        db.conn(),
        &credential,
        &now,
        &crate::domain::vc::VerificationPolicy::default(),
    ))
}

// ---------------------------------------------------------------------------
// Tests.
//
// Unit-test the pure `*_impl` functions against an in-memory DB — the
// tauri handlers are thin wrappers around the same business logic, so the
// command-level behaviour is fully covered.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::domain::vc::SkillClaim;

    const NOW: &str = "2026-04-13T00:00:00Z";

    fn test_key(role: &str) -> SigningKey {
        let mut bytes = [0u8; 32];
        let b = role.as_bytes();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = b[i % b.len().max(1)];
        }
        SigningKey::from_bytes(&bytes)
    }

    fn setup() -> (Database, SigningKey, Did, Did) {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let issuer_key = test_key("issuer");
        let issuer = derive_did_key(&issuer_key);
        let subject = derive_did_key(&test_key("subject"));
        (db, issuer_key, issuer, subject)
    }

    fn sample_request(subject: Did) -> IssueCredentialRequest {
        IssueCredentialRequest {
            credential_type: CredentialType::FormalCredential,
            subject,
            claim: Claim::Skill(SkillClaim {
                skill_id: "skill_test".into(),
                level: 4,
                score: 0.82,
                evidence_refs: vec![],
                rubric_version: Some("v1".into()),
                assessment_method: Some("exam".into()),
            }),
            evidence_refs: vec!["urn:uuid:e1".into()],
            expiration_date: None,
        }
    }

    #[test]
    fn issue_credential_returns_signed_vc_with_status_slot() {
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        assert!(!vc.proof.jws.is_empty());
        assert!(vc.id.starts_with("urn:uuid:"));
        let status = vc.credential_status.expect("status attached");
        assert_eq!(status.status_list_index, "0");
        assert!(status
            .status_list_credential
            .starts_with("urn:alexandria:status-list:"));
    }

    #[test]
    fn issue_credential_allocates_sequential_indices() {
        // Each new credential from the same issuer gets the next bit
        // in the status list, never reusing an index even after revoke.
        let (db, key, issuer, subject) = setup();
        let a = issue_credential_impl(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();
        let b =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        assert_eq!(a.credential_status.unwrap().status_list_index, "0");
        assert_eq!(b.credential_status.unwrap().status_list_index, "1");
    }

    #[test]
    fn issue_rejects_non_did_subject() {
        let (db, key, issuer, _) = setup();
        let req = sample_request(Did("alice@example.com".into()));
        let err = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("DID"), "got {err}");
    }

    #[test]
    fn revoke_sets_bit_and_marks_row_revoked() {
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        revoke_credential_impl(db.conn(), &vc.id, "superseded", NOW).unwrap();

        let revoked: i64 = db
            .conn()
            .query_row(
                "SELECT revoked FROM credentials WHERE id = ?1",
                params![vc.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revoked, 1);

        // Bit 0 must be flipped in the status list.
        let bits: Vec<u8> = db
            .conn()
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE issuer_did = ?1",
                params![issuer.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bits[0] & 0x01, 0x01);
    }

    #[test]
    fn revoke_is_idempotent() {
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        revoke_credential_impl(db.conn(), &vc.id, "r1", NOW).unwrap();
        revoke_credential_impl(db.conn(), &vc.id, "r2", NOW).unwrap();
        // One bit set; not doubled up.
        let bits: Vec<u8> = db
            .conn()
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE issuer_did = ?1",
                params![issuer.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bits[0], 0x01);
    }

    #[test]
    fn get_credential_returns_none_for_unknown_id() {
        let (db, _, _, _) = setup();
        let got = get_credential_impl(db.conn(), "urn:uuid:missing").unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn list_credentials_filters_by_subject_and_skill() {
        let (db, key, issuer, subject) = setup();
        issue_credential_impl(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();
        // Different skill
        let mut req2 = sample_request(subject.clone());
        if let Claim::Skill(ref mut s) = req2.claim {
            s.skill_id = "other_skill".into();
        }
        issue_credential_impl(db.conn(), &key, &issuer, &req2, NOW).unwrap();

        let all = list_credentials_impl(db.conn(), Some(subject.as_str()), None).unwrap();
        assert_eq!(all.len(), 2);
        let one =
            list_credentials_impl(db.conn(), Some(subject.as_str()), Some("other_skill")).unwrap();
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn revoked_credential_fails_verification() {
        // End-to-end within this test: issue → verify (accept) →
        // revoke → verify (reject) under default policy. This is what
        // PR 5.3 wires into verify_credential; locking it in here lets
        // verify.rs's test module stay focused on sign/verify only.
        use crate::domain::vc::verify::verify_credential;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();

        let accepted = verify_credential(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert_eq!(accepted.acceptance_decision, AcceptanceDecision::Accept);
        assert!(!accepted.revoked);

        revoke_credential_impl(db.conn(), &vc.id, "test", NOW).unwrap();

        let rejected = verify_credential(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(rejected.revoked, "revocation bit must propagate to verify");
        assert_eq!(rejected.acceptance_decision, AcceptanceDecision::Reject);
    }
}
