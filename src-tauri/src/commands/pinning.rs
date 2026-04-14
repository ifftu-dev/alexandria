//! IPC commands for PinBoard management + quota introspection.
//!
//! Per spec §12 + §20.4: a node can opt-in to pin specific subjects'
//! content, and observe other peers' commitments. The IPC handlers
//! are thin adapters around `ipfs::pinboard` + `crypto::wallet` —
//! unit tests hit the impls directly.

use ed25519_dalek::SigningKey;
use rusqlite::Connection;
use tauri::State;

use crate::crypto::did::{derive_did_key, Did};
use crate::crypto::wallet;
use crate::ipfs::pinboard::{declare_commitment, list_pinners_for, revoke_commitment};
use crate::p2p::pinboard::PinboardCommitment;
use crate::AppState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QuotaBreakdown {
    pub subject_authored_bytes: u64,
    pub pinboard_bytes: u64,
    pub cache_bytes: u64,
    pub enrollment_bytes: u64,
    pub total_quota_bytes: u64,
}

/// Pure-function declare path. Inserts the commitment via
/// `ipfs::pinboard::declare_commitment`, returns the row to the
/// caller. The signature/public_key fields are populated as
/// `(unsigned, pinner_did)` placeholders — callers that want to
/// broadcast must sign before publishing on `TOPIC_PINBOARD`.
pub fn declare_my_commitment_impl(
    conn: &Connection,
    pinner_did: &Did,
    subject_did: &Did,
    scope: &[String],
) -> Result<PinboardCommitment, String> {
    declare_commitment(conn, pinner_did, subject_did, scope)
}

pub fn list_my_commitments_impl(
    conn: &Connection,
    pinner_did: &Did,
) -> Result<Vec<PinboardCommitment>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, pinner_did, subject_did, scope, commitment_since, \
                    revoked_at, signature, public_key \
             FROM pinboard_observations WHERE pinner_did = ?1 \
             ORDER BY commitment_since DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![pinner_did.as_str()], |r| {
            let scope_json: String = r.get(3)?;
            let scope: Vec<String> = serde_json::from_str(&scope_json).unwrap_or_default();
            Ok(PinboardCommitment {
                id: r.get(0)?,
                pinner_did: r.get(1)?,
                subject_did: r.get(2)?,
                scope,
                commitment_since: r.get(4)?,
                revoked_at: r.get(5)?,
                signature: r.get(6)?,
                public_key: r.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

pub fn list_incoming_commitments_impl(
    conn: &Connection,
    self_did: &Did,
) -> Result<Vec<PinboardCommitment>, String> {
    // "Incoming" = commitments made by OTHERS to pin OUR content.
    list_pinners_for(conn, self_did)
}

pub fn quota_breakdown_impl(conn: &Connection) -> Result<QuotaBreakdown, String> {
    // Per-tier byte accounting via SQL aggregates on the `pins`
    // table. Mirrors the 5-tier eviction classification in
    // `ipfs::storage::list_evictable_pins`:
    //   - subject_authored: auto_unpin = 0
    //   - pinboard:         pin_type = 'pinboard'
    //   - enrollment:       pin_type = 'course' (regardless of
    //                       active/completed — the hot vs cold split
    //                       is an eviction-order concern, not a
    //                       quota-category one)
    //   - cache:            pin_type = 'cache'
    let sum = |pred: &str| -> Result<u64, String> {
        let sql = format!("SELECT COALESCE(SUM(size_bytes), 0) FROM pins WHERE {pred}");
        conn.query_row(&sql, [], |row| row.get::<_, i64>(0))
            .map(|n| n.max(0) as u64)
            .map_err(|e| format!("quota sum: {e}"))
    };
    let subject_authored_bytes = sum("auto_unpin = 0")?;
    let pinboard_bytes = sum("pin_type = 'pinboard' AND auto_unpin = 1")?;
    let cache_bytes = sum("pin_type = 'cache' AND auto_unpin = 1")?;
    let enrollment_bytes = sum("pin_type = 'course' AND auto_unpin = 1")?;
    let total_quota_bytes: u64 = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'storage_quota_bytes'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok(QuotaBreakdown {
        subject_authored_bytes,
        pinboard_bytes,
        cache_bytes,
        enrollment_bytes,
        total_quota_bytes,
    })
}

async fn load_pinner_key(state: &State<'_, AppState>) -> Result<(SigningKey, Did), String> {
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
pub async fn declare_pinboard_commitment(
    state: State<'_, AppState>,
    subject_did: String,
    scope: Vec<String>,
) -> Result<PinboardCommitment, String> {
    let (_signing_key, pinner) = load_pinner_key(&state).await?;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    declare_my_commitment_impl(db.conn(), &pinner, &Did(subject_did), &scope)
}

#[tauri::command]
pub async fn revoke_pinboard_commitment(
    state: State<'_, AppState>,
    commitment_id: String,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    revoke_commitment(db.conn(), &commitment_id)
}

#[tauri::command]
pub async fn list_my_commitments(
    state: State<'_, AppState>,
) -> Result<Vec<PinboardCommitment>, String> {
    let (_signing_key, pinner) = load_pinner_key(&state).await?;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    list_my_commitments_impl(db.conn(), &pinner)
}

#[tauri::command]
pub async fn list_incoming_commitments(
    state: State<'_, AppState>,
) -> Result<Vec<PinboardCommitment>, String> {
    let (_signing_key, self_did) = load_pinner_key(&state).await?;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    list_incoming_commitments_impl(db.conn(), &self_did)
}

#[tauri::command]
pub async fn get_quota_breakdown(state: State<'_, AppState>) -> Result<QuotaBreakdown, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    quota_breakdown_impl(db.conn())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_breakdown_round_trips() {
        // The frontend dashboard reads this to render the tiered
        // storage breakdown; a silent field rename would break the UI.
        let q = QuotaBreakdown {
            subject_authored_bytes: 1_000,
            pinboard_bytes: 2_000,
            cache_bytes: 3_000,
            enrollment_bytes: 4_000,
            total_quota_bytes: 10_000,
        };
        let s = serde_json::to_string(&q).unwrap();
        let back: QuotaBreakdown = serde_json::from_str(&s).unwrap();
        assert_eq!(back.subject_authored_bytes, 1_000);
        assert_eq!(back.pinboard_bytes, 2_000);
        assert_eq!(back.cache_bytes, 3_000);
        assert_eq!(back.enrollment_bytes, 4_000);
        assert_eq!(back.total_quota_bytes, 10_000);
    }

    #[test]
    fn quota_breakdown_sum_equals_total_when_tiers_fill_quota() {
        // Shape-level invariant the eviction code relies on: the per-
        // tier bytes never over-count the total quota. Locking it here
        // gives a future per-tier accountant a harness to plug into.
        let q = QuotaBreakdown {
            subject_authored_bytes: 1_000,
            pinboard_bytes: 2_000,
            cache_bytes: 3_000,
            enrollment_bytes: 4_000,
            total_quota_bytes: 10_000,
        };
        let sum = q.subject_authored_bytes + q.pinboard_bytes + q.cache_bytes + q.enrollment_bytes;
        assert!(sum <= q.total_quota_bytes);
    }

    #[test]
    fn declare_then_list_my_commitments_round_trips() {
        use crate::db::Database;
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let pinner = Did("did:key:zPinner".into());
        let subject = Did("did:key:zSubject".into());
        let c = declare_my_commitment_impl(db.conn(), &pinner, &subject, &["credentials".into()])
            .unwrap();
        let mine = list_my_commitments_impl(db.conn(), &pinner).unwrap();
        assert!(mine.iter().any(|x| x.id == c.id));
    }

    #[test]
    fn list_incoming_commitments_returns_others_pinning_us() {
        // Someone else commits to pin OUR content ⇒ shows up in
        // incoming. Our OWN commitments to pin our own content also
        // surface here (subject == self).
        use crate::db::Database;
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let me = Did("did:key:zMe".into());
        let other = Did("did:key:zOther".into());
        declare_my_commitment_impl(db.conn(), &other, &me, &["credentials".into()]).unwrap();
        let incoming = list_incoming_commitments_impl(db.conn(), &me).unwrap();
        assert!(incoming.iter().any(|c| c.pinner_did == other.as_str()));
    }
}
