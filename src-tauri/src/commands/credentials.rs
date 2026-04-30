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
    Claim, CredentialStatus, CredentialType, Proof, VerifiableCredential, VerificationResult,
};
use crate::AppState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IssueCredentialRequest {
    pub credential_type: CredentialType,
    pub subject: Did,
    pub claim: Claim,
    pub evidence_refs: Vec<String>,
    pub expiration_date: Option<String>,
    /// §11.4 supersession: if set, the new credential declares that
    /// it replaces the credential with this id. The issuer/subject
    /// /claim-kind invariants from §11.4 are enforced at insert time.
    #[serde(default)]
    pub supersedes: Option<String>,
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
    // For skill claims we fold the request's evidence_refs into the
    // claim so the inline subject properties carry them.
    let mut claim = req.claim.clone();
    if let Claim::Skill(ref mut s) = claim {
        s.evidence_refs = req.evidence_refs.clone();
    }
    let claim_kind = claim.kind_str();
    let skill_id = claim.skill_id().map(str::to_string);

    let vc = VerifiableCredential {
        context: vec![W3C_VC_V1.into(), ALEXANDRIA_V1.into()],
        id: Some(credential_id.clone()),
        type_: vec!["VerifiableCredential".into(), type_name.clone()],
        issuer: issuer_did.clone(),
        valid_from: now.to_string(),
        valid_until: req.expiration_date.clone(),
        credential_subject: claim.into_subject(req.subject.clone()),
        credential_status: Some(CredentialStatus {
            id: format!("{list_id}#{index}"),
            type_: STATUS_LIST_TYPE.into(),
            status_purpose: "revocation".into(),
            status_list_index: index.to_string(),
            status_list_credential: list_id.clone(),
        }),
        terms_of_use: None,
        witness: None,
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

    // §11.4 supersession invariants: a newer credential may
    // supersede an older only when the same subject, claim kind, and
    // issuer match (otherwise we'd let an arbitrary issuer "retire"
    // someone else's credentials). Enforce here at the insert site.
    if let Some(prior_id) = &req.supersedes {
        let prior: Option<(String, String, String)> = conn
            .query_row(
                "SELECT issuer_did, subject_did, claim_kind FROM credentials WHERE id = ?1",
                params![prior_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let (prior_issuer, prior_subject, prior_kind) =
            prior.ok_or_else(|| format!("supersedes target {prior_id} not found locally"))?;
        if prior_issuer != issuer_did.as_str() {
            return Err("§11.4: supersession requires same issuer as the prior credential".into());
        }
        if prior_subject != req.subject.as_str() {
            return Err("§11.4: supersession requires same subject as the prior credential".into());
        }
        if prior_kind != claim_kind {
            return Err(
                "§11.4: supersession requires same claim kind as the prior credential".into(),
            );
        }
    }

    conn.execute(
        "INSERT INTO credentials \
         (id, issuer_did, subject_did, credential_type, claim_kind, skill_id, \
          issuance_date, expiration_date, signed_vc_json, integrity_hash, \
          status_list_id, status_list_index, supersedes) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
            req.supersedes,
        ],
    )
    .map_err(|e| format!("insert credential: {e}"))?;

    // Auto-enqueue for §12.3 integrity anchoring. Failure here is
    // non-fatal — anchoring is a survivability convenience, not the
    // critical issuance path. A retry will re-enqueue on next issuance
    // or via an explicit IPC.
    if let Err(e) = crate::cardano::anchor_queue::enqueue(conn, &credential_id) {
        log::warn!("auto-enqueue anchor failed for {credential_id}: {e}");
    }

    // Reputation feedback. Skill-kind credentials feed the learner's
    // score on (skill, level); third-party-issued credentials also
    // credit the instructor. Soft-fail: reputation is a derived view.
    if let Err(e) = crate::evidence::reputation::on_credential_accepted(conn, &credential_id) {
        log::warn!("reputation update failed for {credential_id}: {e}");
    }

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

/// §11.3 suspension. Set `suspended = 1` plus optional
/// `suspended_until` for automatic reinstatement at verify time.
/// Idempotent — re-suspending updates the until window.
pub fn suspend_credential_impl(
    conn: &Connection,
    credential_id: &str,
    until: Option<&str>,
    reason: Option<&str>,
    now: &str,
) -> Result<(), String> {
    let updated = conn
        .execute(
            "UPDATE credentials \
             SET suspended = 1, suspended_at = ?2, \
                 suspended_until = ?3, suspended_reason = ?4 \
             WHERE id = ?1",
            params![credential_id, now, until, reason],
        )
        .map_err(|e| format!("suspend credential: {e}"))?;
    if updated == 0 {
        return Err(format!("credential {credential_id} not found"));
    }
    Ok(())
}

/// §11.3 reinstatement — clear the suspension flag. Idempotent.
pub fn reinstate_credential_impl(conn: &Connection, credential_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE credentials \
         SET suspended = 0, suspended_at = NULL, \
             suspended_until = NULL, suspended_reason = NULL \
         WHERE id = ?1",
        params![credential_id],
    )
    .map_err(|e| format!("reinstate credential: {e}"))?;
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
pub async fn suspend_credential(
    state: State<'_, AppState>,
    credential_id: String,
    until: Option<String>,
    reason: Option<String>,
) -> Result<(), String> {
    let now = now_rfc3339();
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    suspend_credential_impl(
        db.conn(),
        &credential_id,
        until.as_deref(),
        reason.as_deref(),
        &now,
    )
}

#[tauri::command]
pub async fn reinstate_credential(
    state: State<'_, AppState>,
    credential_id: String,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    reinstate_credential_impl(db.conn(), &credential_id)
}

/// Add a (credential_id, requestor_did) entry to the per-credential
/// vc-fetch allowlist. Pass the literal string `"public"` to mark
/// the credential as world-fetchable.
#[tauri::command]
pub async fn allow_credential_fetch(
    state: State<'_, AppState>,
    credential_id: String,
    requestor_did: String,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    crate::p2p::vc_fetch::allow_fetch(db.conn(), &credential_id, &requestor_did)
}

/// Remove a (credential_id, requestor_did) entry from the
/// allowlist. Idempotent.
#[tauri::command]
pub async fn disallow_credential_fetch(
    state: State<'_, AppState>,
    credential_id: String,
    requestor_did: String,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    crate::p2p::vc_fetch::disallow_fetch(db.conn(), &credential_id, &requestor_did)
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
// Survivability — credential bundle export + offline verification (§20.4).
//
// The export bundle is a single JSON document carrying everything a
// third-party verifier needs to re-check the credentials without any
// Alexandria infrastructure: the signed VCs themselves, the historical
// key registry, and the revocation status lists.
//
// Determinism comes from JCS canonicalization — same inputs ⇒
// byte-identical bundle, which is what the survivability tests assert
// and what archival storage relies on for content-addressing.
// ---------------------------------------------------------------------------

/// Bundle wire shape. Keys sort under JCS so the canonical bytes are
/// stable across implementations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CredentialBundle {
    pub format_version: String,
    pub credentials: Vec<VerifiableCredential>,
    pub key_registry: Vec<KeyRegistryRow>,
    pub status_lists: Vec<StatusListRow>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyRegistryRow {
    pub did: String,
    pub key_id: String,
    pub public_key_hex: String,
    pub valid_from: String,
    pub valid_until: Option<String>,
    pub rotated_by: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StatusListRow {
    pub list_id: String,
    pub issuer_did: String,
    pub version: i64,
    pub status_purpose: String,
    /// Base64-encoded bitmap.
    pub bits_b64: String,
    pub bit_length: i64,
}

const BUNDLE_FORMAT_VERSION: &str = "alexandria-credential-bundle/1.0";

/// Build a JCS-canonical export bundle of every credential, key
/// registry row, and status list known to this node.
pub fn export_bundle_impl(conn: &Connection) -> Result<String, String> {
    use base64::Engine;

    // Credentials, ordered deterministically by id so ad-hoc ordering
    // in the credentials table doesn't leak into the bundle.
    let mut stmt = conn
        .prepare("SELECT signed_vc_json FROM credentials ORDER BY id")
        .map_err(|e| e.to_string())?;
    let cred_rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut credentials = Vec::new();
    for r in cred_rows {
        let json = r.map_err(|e| e.to_string())?;
        credentials.push(serde_json::from_str(&json).map_err(|e| e.to_string())?);
    }

    let mut stmt = conn
        .prepare(
            "SELECT did, key_id, public_key_hex, valid_from, valid_until, rotated_by \
             FROM key_registry ORDER BY did, key_id",
        )
        .map_err(|e| e.to_string())?;
    let key_rows = stmt
        .query_map([], |r| {
            Ok(KeyRegistryRow {
                did: r.get(0)?,
                key_id: r.get(1)?,
                public_key_hex: r.get(2)?,
                valid_from: r.get(3)?,
                valid_until: r.get(4)?,
                rotated_by: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut key_registry = Vec::new();
    for r in key_rows {
        key_registry.push(r.map_err(|e| e.to_string())?);
    }

    let mut stmt = conn
        .prepare(
            "SELECT list_id, issuer_did, version, status_purpose, bits, bit_length \
             FROM credential_status_lists ORDER BY list_id",
        )
        .map_err(|e| e.to_string())?;
    let list_rows = stmt
        .query_map([], |r| {
            let bits: Vec<u8> = r.get(4)?;
            Ok(StatusListRow {
                list_id: r.get(0)?,
                issuer_did: r.get(1)?,
                version: r.get(2)?,
                status_purpose: r.get(3)?,
                bits_b64: base64::engine::general_purpose::STANDARD.encode(&bits),
                bit_length: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut status_lists = Vec::new();
    for r in list_rows {
        status_lists.push(r.map_err(|e| e.to_string())?);
    }

    let bundle = CredentialBundle {
        format_version: BUNDLE_FORMAT_VERSION.into(),
        credentials,
        key_registry,
        status_lists,
    };
    serde_json_canonicalizer::to_string(&bundle).map_err(|e| format!("canonicalize bundle: {e}"))
}

/// Verify a bundle with no dependence on the calling node's state —
/// loads the bundle into a fresh in-memory DB and runs each VC
/// through the full §13.2 verification pipeline. Returns
/// `(accepted, total)`.
///
/// This is the in-process analogue of "shell out to digitalbazaar/
/// vc-js" — same offline guarantee, no Alexandria infrastructure
/// required, except the verifier itself.
pub fn verify_bundle_offline_impl(
    bundle_json: &str,
    verification_time: &str,
) -> Result<(u32, u32), String> {
    use crate::db::Database;
    use crate::domain::vc::verify::verify_credential;
    use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};
    use base64::Engine;

    let bundle: CredentialBundle =
        serde_json::from_str(bundle_json).map_err(|e| format!("parse bundle: {e}"))?;
    if bundle.format_version != BUNDLE_FORMAT_VERSION {
        return Err(format!(
            "unsupported bundle format_version: {}",
            bundle.format_version
        ));
    }

    // Spin up a clean DB so the verifier can't see any local state.
    let db = Database::open_in_memory().map_err(|e| format!("open ephemeral db: {e}"))?;
    db.run_migrations()
        .map_err(|e| format!("ephemeral migrations: {e}"))?;

    for entry in &bundle.key_registry {
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO key_registry \
                 (did, key_id, public_key_hex, valid_from, valid_until, rotated_by) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    entry.did,
                    entry.key_id,
                    entry.public_key_hex,
                    entry.valid_from,
                    entry.valid_until,
                    entry.rotated_by,
                ],
            )
            .map_err(|e| e.to_string())?;
    }
    for list in &bundle.status_lists {
        let bits = base64::engine::general_purpose::STANDARD
            .decode(list.bits_b64.as_bytes())
            .map_err(|e| format!("decode list bits: {e}"))?;
        db.conn()
            .execute(
                "INSERT INTO credential_status_lists \
                 (list_id, issuer_did, version, status_purpose, bits, bit_length) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    list.list_id,
                    list.issuer_did,
                    list.version,
                    list.status_purpose,
                    bits,
                    list.bit_length,
                ],
            )
            .map_err(|e| e.to_string())?;
    }

    let total = bundle.credentials.len() as u32;
    let mut accepted = 0u32;
    let policy = VerificationPolicy::default();
    for vc in &bundle.credentials {
        let result = verify_credential(db.conn(), vc, verification_time, &policy);
        if result.acceptance_decision == AcceptanceDecision::Accept {
            accepted += 1;
        }
    }
    Ok((accepted, total))
}

#[tauri::command]
pub async fn export_credentials_bundle(state: State<'_, AppState>) -> Result<String, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    export_bundle_impl(db.conn())
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
            supersedes: None,
        }
    }

    #[test]
    fn issue_credential_returns_signed_vc_with_status_slot() {
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        assert!(!vc.proof.jws.is_empty());
        assert!(vc.id.as_deref().unwrap().starts_with("urn:uuid:"));
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
        revoke_credential_impl(db.conn(), vc.id.as_deref().unwrap(), "superseded", NOW).unwrap();

        let revoked: i64 = db
            .conn()
            .query_row(
                "SELECT revoked FROM credentials WHERE id = ?1",
                params![vc.id.as_deref().unwrap()],
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
        revoke_credential_impl(db.conn(), vc.id.as_deref().unwrap(), "r1", NOW).unwrap();
        revoke_credential_impl(db.conn(), vc.id.as_deref().unwrap(), "r2", NOW).unwrap();
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

        revoke_credential_impl(db.conn(), vc.id.as_deref().unwrap(), "test", NOW).unwrap();

        let rejected = verify_credential(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(rejected.revoked, "revocation bit must propagate to verify");
        assert_eq!(rejected.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn export_bundle_is_deterministic_for_same_inputs() {
        // §20.4: same credential set + same fixed clock + same key
        // ⇒ byte-identical bundle. This is what lets the bundle
        // round-trip through content-addressed archival.
        let (db, key, issuer, subject) = setup();
        let _ =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        let a = export_bundle_impl(db.conn()).unwrap();
        let b = export_bundle_impl(db.conn()).unwrap();
        assert_eq!(a, b, "bundle MUST be byte-identical");
    }

    #[test]
    fn export_bundle_includes_credentials_and_status_lists() {
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        let json = export_bundle_impl(db.conn()).unwrap();
        let bundle: CredentialBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(bundle.format_version, BUNDLE_FORMAT_VERSION);
        assert_eq!(bundle.credentials.len(), 1);
        assert_eq!(bundle.credentials[0].id, vc.id);
        assert_eq!(bundle.status_lists.len(), 1);
        assert_eq!(bundle.status_lists[0].issuer_did, issuer.as_str());
    }

    #[test]
    fn offline_verifier_accepts_a_well_signed_bundle() {
        // §20: bundle survives Alexandria shutdown — verify uses an
        // ephemeral DB with no shared state.
        let (db, key, issuer, subject) = setup();
        let _ =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        let json = export_bundle_impl(db.conn()).unwrap();
        let (accepted, total) = verify_bundle_offline_impl(&json, NOW).unwrap();
        assert_eq!(total, 1);
        assert_eq!(accepted, 1, "round-tripped credential must verify");
    }

    #[test]
    fn offline_verifier_rejects_revoked_credential_in_bundle() {
        // The status list inside the bundle carries the revocation
        // bit, so the offline verifier sees the same Reject as the
        // local one.
        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        revoke_credential_impl(db.conn(), vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        let json = export_bundle_impl(db.conn()).unwrap();
        let (accepted, total) = verify_bundle_offline_impl(&json, NOW).unwrap();
        assert_eq!(total, 1);
        assert_eq!(accepted, 0, "revoked VC must not be accepted offline");
    }

    #[test]
    fn offline_verifier_rejects_unsupported_format_version() {
        let bundle = serde_json::json!({
            "format_version": "alexandria-credential-bundle/0.0",
            "credentials": [],
            "key_registry": [],
            "status_lists": []
        });
        assert!(
            verify_bundle_offline_impl(&bundle.to_string(), NOW).is_err(),
            "must reject unknown format_version"
        );
    }

    // ---- §11.3 suspension --------------------------------------------------

    #[test]
    fn suspension_round_trip_flips_verify_decision() {
        use crate::domain::vc::verify::verify_credential;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();

        // Pre-suspension: accepted.
        let pre = verify_credential(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert_eq!(pre.acceptance_decision, AcceptanceDecision::Accept);
        assert!(!pre.suspended);

        // Suspend with no upper bound — indefinite suspension.
        suspend_credential_impl(
            db.conn(),
            vc.id.as_deref().unwrap(),
            None,
            Some("under review"),
            NOW,
        )
        .unwrap();
        let mid = verify_credential(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(mid.suspended);
        assert_eq!(mid.acceptance_decision, AcceptanceDecision::Reject);

        // Reinstate.
        reinstate_credential_impl(db.conn(), vc.id.as_deref().unwrap()).unwrap();
        let after = verify_credential(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(!after.suspended);
        assert_eq!(after.acceptance_decision, AcceptanceDecision::Accept);
    }

    #[test]
    fn suspension_with_until_in_past_is_no_longer_active() {
        use crate::domain::vc::verify::verify_credential;
        use crate::domain::vc::VerificationPolicy;

        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();

        // Suspended until a time before NOW — verifier sees the
        // suspension as auto-expired and treats the credential as
        // active again.
        suspend_credential_impl(
            db.conn(),
            vc.id.as_deref().unwrap(),
            Some("2026-01-01T00:00:00Z"),
            None,
            NOW,
        )
        .unwrap();
        let result = verify_credential(db.conn(), &vc, NOW, &VerificationPolicy::default());
        assert!(!result.suspended);
    }

    #[test]
    fn permissive_policy_can_accept_suspended() {
        use crate::domain::vc::verify::verify_credential;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let vc =
            issue_credential_impl(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        suspend_credential_impl(db.conn(), vc.id.as_deref().unwrap(), None, None, NOW).unwrap();

        let permissive = VerificationPolicy {
            reject_suspended: false,
            ..Default::default()
        };
        let result = verify_credential(db.conn(), &vc, NOW, &permissive);
        assert!(result.suspended);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Accept);
    }

    // ---- §11.4 supersession ------------------------------------------------

    #[test]
    fn supersession_marks_old_credential_superseded() {
        use crate::domain::vc::verify::verify_credential;
        use crate::domain::vc::{AcceptanceDecision, VerificationPolicy};

        let (db, key, issuer, subject) = setup();
        let old = issue_credential_impl(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();

        // Issue a newer credential that supersedes the old one.
        let mut new_req = sample_request(subject);
        new_req.supersedes = old.id.clone();
        let _new = issue_credential_impl(db.conn(), &key, &issuer, &new_req, NOW).unwrap();

        let result = verify_credential(db.conn(), &old, NOW, &VerificationPolicy::default());
        assert!(result.superseded);
        assert_eq!(result.acceptance_decision, AcceptanceDecision::Reject);
    }

    #[test]
    fn supersession_rejects_cross_issuer() {
        // §11.4: same subject, claim kind, and issuer required.
        let (db, key, issuer, subject) = setup();
        let old = issue_credential_impl(
            db.conn(),
            &key,
            &issuer,
            &sample_request(subject.clone()),
            NOW,
        )
        .unwrap();

        let other_key = test_key("other-issuer");
        let other_issuer = derive_did_key(&other_key);
        let mut bad_req = sample_request(subject);
        bad_req.supersedes = old.id.clone();
        let err =
            issue_credential_impl(db.conn(), &other_key, &other_issuer, &bad_req, NOW).unwrap_err();
        assert!(err.contains("issuer"), "got {err}");
    }

    #[test]
    fn supersession_rejects_unknown_prior_id() {
        let (db, key, issuer, subject) = setup();
        let mut req = sample_request(subject);
        req.supersedes = Some("urn:uuid:does-not-exist".into());
        let err = issue_credential_impl(db.conn(), &key, &issuer, &req, NOW).unwrap_err();
        assert!(err.contains("not found"), "got {err}");
    }
}
