//! Completion-attestation IPC (post-VC-first rebuild).
//!
//! Provides CRUD for `completion_attestation_requirements` (DAO-gated)
//! and `completion_attestations` (assessor signatures). See
//! `domain::attestation` for the shape of the records involved.
//!
//! The auto-issuance pipeline (`commands::auto_issuance`) consults
//! [`are_attestations_satisfied`] before emitting a VC for a given
//! observation; unmet requirements keep the observation pending so
//! the UI can nudge assessors.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rusqlite::{params, Connection, OptionalExtension};
use tauri::State;

use crate::crypto::did::{derive_did_key, parse_did_key};
use crate::crypto::wallet;
use crate::domain::attestation::{
    CompletionAttestation, CompletionAttestationRequirement, CompletionAttestationStatus,
    SetCompletionRequirementParams, SubmitCompletionAttestationParams,
};
use crate::AppState;

// ---------- pure helpers ----------

/// Upsert a requirement. Returns the resulting row.
pub fn set_requirement_impl(
    conn: &Connection,
    params: &SetCompletionRequirementParams,
) -> Result<CompletionAttestationRequirement, String> {
    if params.required_attestors <= 0 {
        return Err("required_attestors must be positive".into());
    }

    conn.execute(
        "INSERT INTO completion_attestation_requirements \
         (course_id, required_attestors, dao_id, set_by_proposal) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(course_id) DO UPDATE SET \
             required_attestors = excluded.required_attestors, \
             dao_id              = excluded.dao_id, \
             set_by_proposal     = excluded.set_by_proposal, \
             updated_at          = datetime('now')",
        params![
            params.course_id,
            params.required_attestors,
            params.dao_id,
            params.set_by_proposal,
        ],
    )
    .map_err(|e| e.to_string())?;

    get_requirement(conn, &params.course_id)?
        .ok_or_else(|| "requirement not found after upsert".into())
}

/// Remove a requirement. Returns the number of rows removed (0 or 1).
pub fn remove_requirement_impl(conn: &Connection, course_id: &str) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM completion_attestation_requirements WHERE course_id = ?1",
        params![course_id],
    )
    .map_err(|e| e.to_string())
}

/// Fetch a single requirement, if any.
pub fn get_requirement(
    conn: &Connection,
    course_id: &str,
) -> Result<Option<CompletionAttestationRequirement>, String> {
    conn.query_row(
        "SELECT course_id, required_attestors, dao_id, set_by_proposal, \
                created_at, updated_at \
         FROM completion_attestation_requirements WHERE course_id = ?1",
        params![course_id],
        row_to_requirement,
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// List all requirements, newest first.
pub fn list_requirements(
    conn: &Connection,
) -> Result<Vec<CompletionAttestationRequirement>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT course_id, required_attestors, dao_id, set_by_proposal, \
                    created_at, updated_at \
             FROM completion_attestation_requirements \
             ORDER BY updated_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_requirement)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Persist an attestor signature over the 32-byte witness tx hash.
/// Idempotent: a duplicate (witness_tx_hash, attestor_did) is a no-op.
pub fn submit_attestation_impl(
    conn: &Connection,
    attestor_key: &SigningKey,
    params: &SubmitCompletionAttestationParams,
) -> Result<CompletionAttestation, String> {
    let tx_bytes = hex::decode(&params.witness_tx_hash)
        .map_err(|e| format!("witness_tx_hash must be hex: {e}"))?;
    if tx_bytes.len() != 32 {
        return Err(format!(
            "witness_tx_hash must decode to 32 bytes, got {}",
            tx_bytes.len()
        ));
    }

    let attestor_did = derive_did_key(attestor_key);
    let attestor_pubkey = hex::encode(attestor_key.verifying_key().as_bytes());
    let signature_bytes = attestor_key.sign(&tx_bytes);
    let signature_hex = hex::encode(signature_bytes.to_bytes());

    let id = format!(
        "ca_{}",
        blake3::hash(format!("{}:{}", params.witness_tx_hash, attestor_did.as_str()).as_bytes())
            .to_hex()
    );

    conn.execute(
        "INSERT OR IGNORE INTO completion_attestations \
         (id, witness_tx_hash, attestor_did, attestor_pubkey, signature, note) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            params.witness_tx_hash,
            attestor_did.as_str(),
            attestor_pubkey,
            signature_hex,
            params.note,
        ],
    )
    .map_err(|e| e.to_string())?;

    conn.query_row(
        "SELECT id, witness_tx_hash, attestor_did, attestor_pubkey, signature, \
                note, created_at \
         FROM completion_attestations \
         WHERE witness_tx_hash = ?1 AND attestor_did = ?2",
        params![params.witness_tx_hash, attestor_did.as_str()],
        row_to_attestation,
    )
    .map_err(|e| e.to_string())
}

/// List attestations on a given witness tx, newest first.
pub fn list_attestations(
    conn: &Connection,
    witness_tx_hash: &str,
) -> Result<Vec<CompletionAttestation>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, witness_tx_hash, attestor_did, attestor_pubkey, signature, \
                    note, created_at \
             FROM completion_attestations \
             WHERE witness_tx_hash = ?1 \
             ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![witness_tx_hash], row_to_attestation)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Verify every stored signature on `witness_tx_hash`. Rows that fail
/// verification are filtered out.
pub fn verify_attestations(
    conn: &Connection,
    witness_tx_hash: &str,
) -> Result<Vec<CompletionAttestation>, String> {
    let tx_bytes =
        hex::decode(witness_tx_hash).map_err(|e| format!("witness_tx_hash must be hex: {e}"))?;
    let all = list_attestations(conn, witness_tx_hash)?;
    let mut valid = Vec::new();
    for att in all {
        if verify_one(&tx_bytes, &att).unwrap_or(false) {
            valid.push(att);
        }
    }
    Ok(valid)
}

fn verify_one(tx_bytes: &[u8], att: &CompletionAttestation) -> Result<bool, String> {
    let pub_bytes = hex::decode(&att.attestor_pubkey).map_err(|e| e.to_string())?;
    if pub_bytes.len() != 32 {
        return Ok(false);
    }
    let sig_bytes = hex::decode(&att.signature).map_err(|e| e.to_string())?;
    if sig_bytes.len() != 64 {
        return Ok(false);
    }
    let pub_arr: [u8; 32] = pub_bytes.try_into().map_err(|_| "pubkey len".to_string())?;
    let sig_arr: [u8; 64] = sig_bytes.try_into().map_err(|_| "sig len".to_string())?;

    let vk = match VerifyingKey::from_bytes(&pub_arr) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    // Optional DID consistency check: the on-file attestor_did must
    // match the one we'd derive from the embedded pubkey.
    if let Ok(parsed) = parse_did_key(&att.attestor_did) {
        if parsed != crate::crypto::did::did_from_verifying_key(&vk) {
            return Ok(false);
        }
    }
    let sig = Signature::from_bytes(&sig_arr);
    Ok(vk.verify(tx_bytes, &sig).is_ok())
}

/// Full status view: required vs. present, attested = cryptographically
/// valid signatures.
pub fn attestation_status(
    conn: &Connection,
    witness_tx_hash: &str,
    course_id: Option<&str>,
) -> Result<CompletionAttestationStatus, String> {
    let valid = verify_attestations(conn, witness_tx_hash)?;
    let required = course_id
        .and_then(|cid| get_requirement(conn, cid).ok().flatten())
        .map(|r| r.required_attestors)
        .unwrap_or(0);

    Ok(CompletionAttestationStatus {
        witness_tx_hash: witness_tx_hash.to_string(),
        course_id: course_id.map(|s| s.to_string()),
        required_attestors: required,
        current_attestors: valid.len() as i64,
        is_satisfied: valid.len() as i64 >= required,
        attestations: valid,
    })
}

/// Auto-issuance gate. Returns `true` when the witness tx has
/// gathered enough attestor signatures, OR when no requirement has
/// been configured for the course (the observer issues immediately).
pub fn are_attestations_satisfied(
    conn: &Connection,
    witness_tx_hash: &str,
    course_id: Option<&str>,
) -> Result<bool, String> {
    let Some(cid) = course_id else {
        return Ok(true);
    };
    let Some(req) = get_requirement(conn, cid)? else {
        return Ok(true);
    };
    let valid = verify_attestations(conn, witness_tx_hash)?;
    Ok(valid.len() as i64 >= req.required_attestors)
}

// ---------- row mappers ----------

fn row_to_requirement(row: &rusqlite::Row) -> rusqlite::Result<CompletionAttestationRequirement> {
    Ok(CompletionAttestationRequirement {
        course_id: row.get(0)?,
        required_attestors: row.get(1)?,
        dao_id: row.get(2)?,
        set_by_proposal: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn row_to_attestation(row: &rusqlite::Row) -> rusqlite::Result<CompletionAttestation> {
    Ok(CompletionAttestation {
        id: row.get(0)?,
        witness_tx_hash: row.get(1)?,
        attestor_did: row.get(2)?,
        attestor_pubkey: row.get(3)?,
        signature: row.get(4)?,
        note: row.get(5)?,
        created_at: row.get(6)?,
    })
}

// ---------- Tauri commands ----------

#[tauri::command]
pub async fn set_completion_attestation_requirement(
    state: State<'_, AppState>,
    params: SetCompletionRequirementParams,
) -> Result<CompletionAttestationRequirement, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    set_requirement_impl(db.conn(), &params)
}

#[tauri::command]
pub async fn remove_completion_attestation_requirement(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<usize, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    remove_requirement_impl(db.conn(), &course_id)
}

#[tauri::command]
pub async fn list_completion_attestation_requirements(
    state: State<'_, AppState>,
) -> Result<Vec<CompletionAttestationRequirement>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    list_requirements(db.conn())
}

#[tauri::command]
pub async fn submit_completion_attestation(
    state: State<'_, AppState>,
    params: SubmitCompletionAttestationParams,
) -> Result<CompletionAttestation, String> {
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(ks_guard);
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    submit_attestation_impl(db.conn(), &w.signing_key, &params)
}

#[tauri::command]
pub async fn get_completion_attestation_status(
    state: State<'_, AppState>,
    witness_tx_hash: String,
    course_id: Option<String>,
) -> Result<CompletionAttestationStatus, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    attestation_status(db.conn(), &witness_tx_hash, course_id.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        db
    }

    #[test]
    fn requirement_crud_roundtrip() {
        let db = test_db();
        set_requirement_impl(
            db.conn(),
            &SetCompletionRequirementParams {
                course_id: "course_a".into(),
                required_attestors: 2,
                dao_id: "dao_cs".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        let fetched = get_requirement(db.conn(), "course_a").unwrap().unwrap();
        assert_eq!(fetched.required_attestors, 2);

        // Upsert bumps.
        set_requirement_impl(
            db.conn(),
            &SetCompletionRequirementParams {
                course_id: "course_a".into(),
                required_attestors: 3,
                dao_id: "dao_cs".into(),
                set_by_proposal: Some("prop_1".into()),
            },
        )
        .unwrap();
        let bumped = get_requirement(db.conn(), "course_a").unwrap().unwrap();
        assert_eq!(bumped.required_attestors, 3);
        assert_eq!(bumped.set_by_proposal.as_deref(), Some("prop_1"));

        let removed = remove_requirement_impl(db.conn(), "course_a").unwrap();
        assert_eq!(removed, 1);
        assert!(get_requirement(db.conn(), "course_a").unwrap().is_none());
    }

    #[test]
    fn requirement_rejects_nonpositive_threshold() {
        let db = test_db();
        let err = set_requirement_impl(
            db.conn(),
            &SetCompletionRequirementParams {
                course_id: "c".into(),
                required_attestors: 0,
                dao_id: "d".into(),
                set_by_proposal: None,
            },
        )
        .unwrap_err();
        assert!(err.contains("required_attestors"));
    }

    #[test]
    fn submit_attestation_is_idempotent() {
        let db = test_db();
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let tx_hash = "ab".repeat(32);

        let p = SubmitCompletionAttestationParams {
            witness_tx_hash: tx_hash.clone(),
            note: Some("first-pass".into()),
        };
        let a1 = submit_attestation_impl(db.conn(), &key, &p).unwrap();
        let a2 = submit_attestation_impl(db.conn(), &key, &p).unwrap();
        assert_eq!(a1.id, a2.id, "same composite id on duplicate submit");

        let listed = list_attestations(db.conn(), &tx_hash).unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn verify_filters_tampered_signatures() {
        let db = test_db();
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let tx_hash = "cd".repeat(32);
        submit_attestation_impl(
            db.conn(),
            &key,
            &SubmitCompletionAttestationParams {
                witness_tx_hash: tx_hash.clone(),
                note: None,
            },
        )
        .unwrap();

        // Corrupt the signature.
        db.conn()
            .execute(
                "UPDATE completion_attestations SET signature = ?1 WHERE witness_tx_hash = ?2",
                params!["00".repeat(64), tx_hash.clone()],
            )
            .unwrap();

        assert!(verify_attestations(db.conn(), &tx_hash).unwrap().is_empty());
    }

    #[test]
    fn status_reports_satisfaction() {
        let db = test_db();
        set_requirement_impl(
            db.conn(),
            &SetCompletionRequirementParams {
                course_id: "course_sat".into(),
                required_attestors: 2,
                dao_id: "d".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        let tx_hash = "ef".repeat(32);
        let key_a = SigningKey::from_bytes(&[9u8; 32]);
        let key_b = SigningKey::from_bytes(&[10u8; 32]);

        submit_attestation_impl(
            db.conn(),
            &key_a,
            &SubmitCompletionAttestationParams {
                witness_tx_hash: tx_hash.clone(),
                note: None,
            },
        )
        .unwrap();
        let partial = attestation_status(db.conn(), &tx_hash, Some("course_sat")).unwrap();
        assert!(!partial.is_satisfied);
        assert_eq!(partial.current_attestors, 1);
        assert_eq!(partial.required_attestors, 2);

        submit_attestation_impl(
            db.conn(),
            &key_b,
            &SubmitCompletionAttestationParams {
                witness_tx_hash: tx_hash.clone(),
                note: None,
            },
        )
        .unwrap();
        let full = attestation_status(db.conn(), &tx_hash, Some("course_sat")).unwrap();
        assert!(full.is_satisfied);
        assert_eq!(full.current_attestors, 2);
    }

    #[test]
    fn gate_returns_true_without_requirement() {
        let db = test_db();
        assert!(are_attestations_satisfied(db.conn(), &"00".repeat(32), Some("nope")).unwrap());
        assert!(are_attestations_satisfied(db.conn(), &"00".repeat(32), None).unwrap());
    }
}
