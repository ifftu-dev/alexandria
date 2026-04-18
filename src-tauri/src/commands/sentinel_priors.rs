//! Sentinel adversarial-prior library — propose, ratify, list.
//!
//! Flow:
//!
//!   1. A contributor builds a labeled-samples blob (JSON, schema v1) and
//!      pins it locally (`content_add` → CID).
//!   2. They call [`sentinel_propose_prior`], which validates the blob,
//!      runs the forfeiture check, and submits a governance_proposal
//!      under the Sentinel DAO with category='sentinel_prior'.
//!   3. The Sentinel DAO votes via the generic governance pipeline
//!      (`resolve_proposal` → status='approved').
//!   4. Anyone calls [`sentinel_ratify_prior`] to finalize an approved
//!      proposal: the blob metadata is extracted and a row is inserted
//!      into `sentinel_priors`. Clients mirror this table locally.
//!
//! The face model kind is forbidden by design (see decision 2 in
//! `docs/sentinel-federation.md`). Both the blob validator and the
//! propose entrypoint reject anything labeled `face`.

use std::str::FromStr;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::sentinel_dao::{SENTINEL_DAO_ID, SENTINEL_PRIOR_CATEGORY};
use crate::crypto::hash::{blake2b_256, entity_id};
use crate::crypto::wallet;
use crate::domain::sentinel::SentinelPriorAnnouncement;
use crate::ipfs::storage;
use crate::AppState;

/// Pin type for ratified adversarial-prior blobs. Distinct from 'cache'
/// so they survive eviction — priors have to stay resident to be usable
/// during training.
const PIN_TYPE_SENTINEL_PRIOR: &str = "sentinel_prior";

// ============================================================================
// Constants
// ============================================================================

/// Current blob schema version. Older versions are accepted for
/// backwards-compatibility reads; new proposals must use the latest.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Minimum labeled samples per blob. Below this, gradient inversion
/// becomes trivially successful and the prior adds little signal anyway.
pub const MIN_SAMPLES: usize = 20;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    Keystroke,
    Mouse,
}

impl ModelKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelKind::Keystroke => "keystroke",
            ModelKind::Mouse => "mouse",
        }
    }
}

impl FromStr for ModelKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "keystroke" => Ok(ModelKind::Keystroke),
            "mouse" => Ok(ModelKind::Mouse),
            "face" => Err("face is not a permitted model_kind for adversarial priors \
                 (see docs/sentinel-federation.md decision 2)"
                .into()),
            other => Err(format!("unknown model_kind: {other}")),
        }
    }
}

/// Parsed shape of a labeled-samples blob.
///
/// `samples` is left as an opaque JSON value so the blob can evolve its
/// inner shape (keystroke digraphs vs. mouse trajectories) independently
/// of this metadata envelope.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PriorBlob {
    pub schema_version: u32,
    pub model_kind: String,
    pub label: String,
    pub samples: serde_json::Value,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub contributor_attribution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentinelPrior {
    pub id: String,
    pub proposal_id: String,
    pub cid: String,
    pub model_kind: String,
    pub label: String,
    pub schema_version: i64,
    pub sample_count: i64,
    pub notes: Option<String>,
    pub ratified_at: String,
    pub signature: String,
}

#[derive(Debug, Deserialize)]
pub struct ProposePriorRequest {
    /// BLAKE3 content hash of the labeled-samples blob. Must already be
    /// pinned locally via `content_add` before calling this command.
    pub blob_cid: String,
    /// Short title for the proposal (shown to voters).
    pub title: String,
    /// Optional proposal body.
    pub description: Option<String>,
    /// If the blob was sourced from a live assessment session, attach
    /// that session ID. The backend will reject the proposal if the
    /// session ended in `flagged` or `suspended` state (decision 3 in
    /// `docs/sentinel-federation.md`: a cheater's data must not shape
    /// the classifier that's supposed to catch them).
    pub source_session_id: Option<String>,
}

// ============================================================================
// Validation (pure, unit-testable)
// ============================================================================

/// Parse + validate a blob JSON string.
///
/// Enforces the envelope contract: known schema version, permitted model
/// kind, non-empty label, sample array length ≥ MIN_SAMPLES.
pub fn validate_prior_blob(json: &str) -> Result<PriorBlob, String> {
    let blob: PriorBlob =
        serde_json::from_str(json).map_err(|e| format!("blob parse failed: {e}"))?;

    if blob.schema_version == 0 || blob.schema_version > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported schema_version {} (client knows up to {})",
            blob.schema_version, CURRENT_SCHEMA_VERSION
        ));
    }

    // Reject face kind loudly, even before generic validation — this is
    // a hard-line architectural invariant per the threat model.
    ModelKind::from_str(&blob.model_kind)?;

    if blob.label.trim().is_empty() {
        return Err("label must be a non-empty string".into());
    }

    let sample_count = match &blob.samples {
        serde_json::Value::Array(items) => items.len(),
        _ => return Err("samples must be a JSON array".into()),
    };

    if sample_count < MIN_SAMPLES {
        return Err(format!(
            "at least {MIN_SAMPLES} samples required (got {sample_count})"
        ));
    }

    Ok(blob)
}

/// Deterministic prior identifier. Same inputs ⇒ same id, so a blob
/// that's already been ratified can't be accidentally re-ratified under
/// a different id.
pub fn compute_prior_id(cid: &str, label: &str, model_kind: &str) -> String {
    entity_id(&[cid, label, model_kind])
}

/// Placeholder signature: blake2b digest over the canonical metadata.
///
/// Real Sentinel-DAO threshold signatures will replace this in a later
/// patch; the field stays in the schema so downstream code can keep
/// treating it as authoritative.
pub fn compute_prior_signature(
    cid: &str,
    label: &str,
    model_kind: &str,
    schema_version: u32,
) -> String {
    let combined = format!("{cid}|{label}|{model_kind}|{schema_version}");
    let digest = blake2b_256(combined.as_bytes());
    hex_encode(&digest)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

// ============================================================================
// Commands
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ProposePriorResponse {
    pub proposal_id: String,
}

/// Submit a new adversarial-prior candidate for Sentinel DAO ratification.
///
/// The blob at `blob_cid` must already be pinned locally. The backend
/// fetches, validates, runs the forfeiture check, and — on success —
/// files a governance_proposal of category `sentinel_prior` on the
/// Sentinel DAO.
#[tauri::command]
pub async fn sentinel_propose_prior(
    state: State<'_, AppState>,
    req: ProposePriorRequest,
) -> Result<ProposePriorResponse, String> {
    // Fetch the blob first (async, no DB lock held).
    let resolver = {
        let guard = state.resolver.lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "content resolver not initialized".to_string())?
    };
    let bytes = resolver
        .resolve(&req.blob_cid)
        .await
        .map_err(|e| format!("blob resolve failed: {e}"))?
        .bytes;
    let json =
        std::str::from_utf8(&bytes).map_err(|_| "blob is not valid UTF-8 JSON".to_string())?;
    let blob = validate_prior_blob(json)?;

    // DB-side checks + insert happen under one lock.
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    // Forfeiture: a flagged or suspended session cannot source priors.
    if let Some(ref session_id) = req.source_session_id {
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM integrity_sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .ok();
        match status.as_deref() {
            Some("flagged") | Some("suspended") => {
                return Err(format!(
                    "source session {session_id} is {} — cannot propose priors from it",
                    status.as_deref().unwrap_or("unknown")
                ));
            }
            Some(_) | None => {}
        }
    }

    // Proposer identity + DAO gate (scope_type='sentinel' skips proficiency).
    let proposer: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no local identity: {e}"))?;

    let dao_status: String = conn
        .query_row(
            "SELECT status FROM governance_daos WHERE id = ?1",
            params![SENTINEL_DAO_ID],
            |row| row.get(0),
        )
        .map_err(|_| "Sentinel DAO not seeded (run migration 037)".to_string())?;
    if dao_status != "active" {
        return Err(format!("Sentinel DAO is not active (status: {dao_status})"));
    }

    let proposal_id = entity_id(&[SENTINEL_DAO_ID, &req.blob_cid, &proposer]);

    conn.execute(
        "INSERT INTO governance_proposals
             (id, dao_id, title, description, category, status, proposer,
              min_vote_proficiency, content_cid)
         VALUES (?1, ?2, ?3, ?4, ?5, 'draft', ?6, 'remember', ?7)",
        params![
            proposal_id,
            SENTINEL_DAO_ID,
            req.title,
            req.description,
            SENTINEL_PRIOR_CATEGORY,
            proposer,
            req.blob_cid,
        ],
    )
    .map_err(|e| format!("proposal insert failed: {e}"))?;

    // Silence unused-warning on the validated blob — we'll need it again
    // at ratify time, but blobs are content-addressed so we can re-read.
    let _ = blob;

    Ok(ProposePriorResponse { proposal_id })
}

/// Finalize an approved Sentinel prior proposal into the `sentinel_priors`
/// table.
///
/// Anyone can call this once the governance pipeline has marked the
/// proposal `approved`. Idempotent via `UNIQUE(proposal_id)` — a second
/// call is a no-op that returns the existing row.
#[tauri::command]
pub async fn sentinel_ratify_prior(
    state: State<'_, AppState>,
    proposal_id: String,
) -> Result<SentinelPrior, String> {
    // Pull the proposal + blob CID (need the CID before fetching the blob,
    // and we want to release the DB lock before async work).
    let (status, category, content_cid, on_chain_tx) = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();

        // Short-circuit if already ratified (idempotent).
        if let Ok(existing) = read_prior_by_proposal(conn, &proposal_id) {
            return Ok(existing);
        }

        conn.query_row(
            "SELECT status, category, content_cid, on_chain_tx
             FROM governance_proposals WHERE id = ?1",
            params![proposal_id],
            |row| {
                Ok::<_, rusqlite::Error>((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .map_err(|e| format!("proposal not found: {e}"))?
    };

    if category != SENTINEL_PRIOR_CATEGORY {
        return Err(format!(
            "proposal {proposal_id} is not a sentinel_prior (category: {category})"
        ));
    }
    if status != "approved" {
        return Err(format!(
            "proposal must be approved to ratify (status: {status})"
        ));
    }
    let cid =
        content_cid.ok_or_else(|| "sentinel_prior proposal is missing content_cid".to_string())?;

    // Re-fetch + re-validate the blob. Content-addressing means the bytes
    // are immutable given the CID, but defense in depth is cheap.
    let resolver = {
        let guard = state.resolver.lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "content resolver not initialized".to_string())?
    };
    let bytes = resolver
        .resolve(&cid)
        .await
        .map_err(|e| format!("blob resolve failed: {e}"))?
        .bytes;
    let json =
        std::str::from_utf8(&bytes).map_err(|_| "blob is not valid UTF-8 JSON".to_string())?;
    let blob = validate_prior_blob(json)?;

    let sample_count = match &blob.samples {
        serde_json::Value::Array(items) => items.len(),
        _ => unreachable!("validate_prior_blob guarantees samples is an array"),
    };

    let id = compute_prior_id(&cid, &blob.label, &blob.model_kind);
    let signature = on_chain_tx.unwrap_or_else(|| {
        compute_prior_signature(&cid, &blob.label, &blob.model_kind, blob.schema_version)
    });
    let blob_size = bytes.len() as u64;

    let row = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();

        conn.execute(
            "INSERT OR IGNORE INTO sentinel_priors
                 (id, proposal_id, cid, model_kind, label, schema_version,
                  sample_count, notes, signature)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                proposal_id,
                cid,
                blob.model_kind,
                blob.label,
                blob.schema_version as i64,
                sample_count as i64,
                blob.notes,
                signature,
            ],
        )
        .map_err(|e| format!("prior insert failed: {e}"))?;

        // Upgrade the pin from 'cache' (what content_resolve_bytes would
        // have left) to 'sentinel_prior' with auto_unpin=false so it
        // survives storage-pressure eviction.
        storage::upsert_pin(conn, &cid, PIN_TYPE_SENTINEL_PRIOR, blob_size, false);

        read_prior_by_proposal(conn, &proposal_id)?
    };

    // Best-effort gossip broadcast. Failures are logged, never fatal —
    // the prior is already persisted locally and will be re-broadcast
    // next time a committee member ratifies or syncs (future sweeper
    // work). Gated on an unlocked keystore; if locked, the node is
    // running read-only and gossip will catch up once unlocked.
    broadcast_sentinel_prior(&state, &row).await;

    Ok(row)
}

/// Fire-and-forget broadcast of a freshly ratified prior onto the
/// Sentinel priors gossip topic. All error paths log + continue.
async fn broadcast_sentinel_prior(state: &State<'_, AppState>, row: &SentinelPrior) {
    let mnemonic = {
        let keystore = state.keystore.lock().await;
        match keystore.as_ref() {
            Some(ks) => match ks.retrieve_mnemonic() {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("sentinel broadcast: retrieve_mnemonic failed: {e}");
                    return;
                }
            },
            None => {
                log::debug!("sentinel broadcast: keystore locked — skipping gossip");
                return;
            }
        }
    };
    let w = match wallet::wallet_from_mnemonic(&mnemonic) {
        Ok(w) => w,
        Err(e) => {
            log::warn!("sentinel broadcast: wallet_from_mnemonic failed: {e}");
            return;
        }
    };

    let ann = SentinelPriorAnnouncement {
        prior_id: row.id.clone(),
        proposal_id: row.proposal_id.clone(),
        cid: row.cid.clone(),
        model_kind: row.model_kind.clone(),
        label: row.label.clone(),
        schema_version: row.schema_version as u32,
        sample_count: row.sample_count,
        notes: row.notes.clone(),
        signature: row.signature.clone(),
        ratified_at: row.ratified_at.clone(),
    };
    let payload = match serde_json::to_vec(&ann) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("sentinel broadcast: serialize failed: {e}");
            return;
        }
    };

    let node_guard = state.p2p_node.lock().await;
    match node_guard.as_ref() {
        Some(node) => {
            if let Err(e) = node
                .publish_sentinel_prior(payload, &w.signing_key, &w.stake_address)
                .await
            {
                log::warn!("sentinel broadcast: publish failed: {e}");
            }
        }
        None => log::debug!("sentinel broadcast: p2p node not running — skipping gossip"),
    }
}

#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub priors_known: usize,
    pub blobs_pinned: usize,
    pub errors: Vec<String>,
}

/// Ensure every ratified prior's blob is resolvable locally.
///
/// Iterates the `sentinel_priors` table and calls the content resolver
/// on each CID. Anything resolvable gets upgraded to `pin_type =
/// 'sentinel_prior'` (auto_unpin=false) so it survives eviction.
/// Errors are collected per-CID so a partial failure doesn't abort the
/// rest of the run. Safe to call repeatedly — idempotent by design.
///
/// Wire from the frontend: once on app start + once per 24h while
/// running. Cheap if everything is already cached; any misses pull the
/// missing blob across the content-resolve path.
#[tauri::command]
pub async fn sentinel_priors_sync(state: State<'_, AppState>) -> Result<SyncResult, String> {
    // Snapshot (id, cid) out of the DB so we don't hold the lock across
    // awaits. `rows` is bound explicitly inside the block so `stmt` lives
    // long enough to drive the iterator.
    let priors: Vec<(String, String)> = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let mut stmt = db
            .conn()
            .prepare("SELECT id, cid FROM sentinel_priors")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    let resolver = {
        let guard = state.resolver.lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "content resolver not initialized".to_string())?
    };

    let mut pinned = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for (id, cid) in &priors {
        match resolver.resolve(cid).await {
            Ok(result) => {
                if let Ok(guard) = state.db.lock() {
                    if let Some(db) = guard.as_ref() {
                        storage::upsert_pin(
                            db.conn(),
                            &result.blake3_hash,
                            PIN_TYPE_SENTINEL_PRIOR,
                            result.size,
                            false,
                        );
                    }
                }
                pinned += 1;
            }
            Err(e) => errors.push(format!("{id}: {e}")),
        }
    }

    Ok(SyncResult {
        priors_known: priors.len(),
        blobs_pinned: pinned,
        errors,
    })
}

/// Lazy-load a single prior's parsed blob by prior id.
///
/// Used by the training pipeline when it needs the actual labeled
/// samples to fold into local training. Returns the validated
/// `PriorBlob`; the blob is content-addressed so parsing can trust the
/// bytes match the advertised metadata.
#[tauri::command]
pub async fn sentinel_priors_load(
    state: State<'_, AppState>,
    prior_id: String,
) -> Result<PriorBlob, String> {
    let cid: String = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        db.conn()
            .query_row(
                "SELECT cid FROM sentinel_priors WHERE id = ?1",
                params![prior_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| format!("prior not found: {e}"))?
    };

    let resolver = {
        let guard = state.resolver.lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "content resolver not initialized".to_string())?
    };
    let bytes = resolver
        .resolve(&cid)
        .await
        .map_err(|e| format!("blob resolve failed: {e}"))?
        .bytes;
    let json =
        std::str::from_utf8(&bytes).map_err(|_| "blob is not valid UTF-8 JSON".to_string())?;
    validate_prior_blob(json)
}

/// List ratified priors, optionally filtered by model kind.
#[tauri::command]
pub async fn sentinel_priors_list(
    state: State<'_, AppState>,
    model_kind: Option<String>,
) -> Result<Vec<SentinelPrior>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    // Validate the filter so callers can't probe for `face` priors
    // expecting silence.
    if let Some(ref k) = model_kind {
        ModelKind::from_str(k)?;
    }

    let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(k) = model_kind {
            (
                "SELECT id, proposal_id, cid, model_kind, label, schema_version,
                        sample_count, notes, ratified_at, signature
                 FROM sentinel_priors WHERE model_kind = ?1
                 ORDER BY ratified_at DESC"
                    .to_string(),
                vec![Box::new(k)],
            )
        } else {
            (
                "SELECT id, proposal_id, cid, model_kind, label, schema_version,
                        sample_count, notes, ratified_at, signature
                 FROM sentinel_priors
                 ORDER BY ratified_at DESC"
                    .to_string(),
                vec![],
            )
        };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_ref.as_slice(), map_prior_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

// ============================================================================
// Row mapping helpers
// ============================================================================

fn map_prior_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SentinelPrior> {
    Ok(SentinelPrior {
        id: row.get(0)?,
        proposal_id: row.get(1)?,
        cid: row.get(2)?,
        model_kind: row.get(3)?,
        label: row.get(4)?,
        schema_version: row.get(5)?,
        sample_count: row.get(6)?,
        notes: row.get(7)?,
        ratified_at: row.get(8)?,
        signature: row.get(9)?,
    })
}

fn read_prior_by_proposal(
    conn: &rusqlite::Connection,
    proposal_id: &str,
) -> Result<SentinelPrior, String> {
    conn.query_row(
        "SELECT id, proposal_id, cid, model_kind, label, schema_version,
                sample_count, notes, ratified_at, signature
         FROM sentinel_priors WHERE proposal_id = ?1",
        params![proposal_id],
        map_prior_row,
    )
    .map_err(|e| e.to_string())
}

// ============================================================================
// Tests — validation and hashing helpers are pure, so they're trivially
// covered without a DB.
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn synth_samples(n: usize) -> serde_json::Value {
        serde_json::Value::Array(
            (0..n)
                .map(|i| {
                    json!({
                        "dwellMs1": 80 + (i % 5),
                        "dwellMs2": 75 + (i % 4),
                        "flightMs": 120,
                        "speedRatio": 1.5,
                    })
                })
                .collect(),
        )
    }

    fn valid_keystroke_blob() -> serde_json::Value {
        json!({
            "schema_version": 1,
            "model_kind": "keystroke",
            "label": "paste_macro",
            "samples": synth_samples(25),
            "notes": "test fixture",
        })
    }

    #[test]
    fn model_kind_parse_matches_case() {
        assert_eq!(
            ModelKind::from_str("keystroke").unwrap(),
            ModelKind::Keystroke
        );
        assert_eq!(ModelKind::from_str("mouse").unwrap(), ModelKind::Mouse);
        assert!(ModelKind::from_str("Keystroke").is_err());
        assert!(ModelKind::from_str("").is_err());
    }

    #[test]
    fn face_kind_is_rejected_with_explanatory_error() {
        let err = ModelKind::from_str("face").unwrap_err();
        assert!(err.contains("face"), "error should mention face: {err}");
        assert!(
            err.contains("decision 2"),
            "error should cite the decision: {err}"
        );
    }

    #[test]
    fn blob_validates_when_well_formed() {
        let json = valid_keystroke_blob().to_string();
        let blob = validate_prior_blob(&json).unwrap();
        assert_eq!(blob.model_kind, "keystroke");
        assert_eq!(blob.label, "paste_macro");
        assert_eq!(blob.schema_version, 1);
    }

    #[test]
    fn blob_validation_rejects_too_few_samples() {
        let mut bad = valid_keystroke_blob();
        bad["samples"] = synth_samples(MIN_SAMPLES - 1);
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("samples required"), "got {err}");
    }

    #[test]
    fn blob_validation_rejects_face_kind() {
        let mut bad = valid_keystroke_blob();
        bad["model_kind"] = json!("face");
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("face"), "got {err}");
    }

    #[test]
    fn blob_validation_rejects_empty_label() {
        let mut bad = valid_keystroke_blob();
        bad["label"] = json!("   ");
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("label"), "got {err}");
    }

    #[test]
    fn blob_validation_rejects_unknown_schema_version() {
        let mut bad = valid_keystroke_blob();
        bad["schema_version"] = json!(999);
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("schema_version"), "got {err}");
    }

    #[test]
    fn blob_validation_rejects_non_array_samples() {
        let mut bad = valid_keystroke_blob();
        bad["samples"] = json!("not an array");
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("array"), "got {err}");
    }

    #[test]
    fn prior_id_is_deterministic() {
        let a = compute_prior_id("cid-x", "paste_macro", "keystroke");
        let b = compute_prior_id("cid-x", "paste_macro", "keystroke");
        assert_eq!(a, b);
        let c = compute_prior_id("cid-y", "paste_macro", "keystroke");
        assert_ne!(a, c);
    }

    #[test]
    fn prior_signature_is_deterministic_and_64_hex_chars() {
        let sig = compute_prior_signature("cid-x", "paste_macro", "keystroke", 1);
        assert_eq!(sig.len(), 64, "blake2b-256 hex should be 64 chars");
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
        let sig2 = compute_prior_signature("cid-x", "paste_macro", "keystroke", 1);
        assert_eq!(sig, sig2);
    }

    #[test]
    fn prior_signature_changes_with_any_input() {
        let base = compute_prior_signature("cid-x", "paste_macro", "keystroke", 1);
        assert_ne!(
            base,
            compute_prior_signature("cid-y", "paste_macro", "keystroke", 1)
        );
        assert_ne!(
            base,
            compute_prior_signature("cid-x", "other_label", "keystroke", 1)
        );
        assert_ne!(
            base,
            compute_prior_signature("cid-x", "paste_macro", "mouse", 1)
        );
        assert_ne!(
            base,
            compute_prior_signature("cid-x", "paste_macro", "keystroke", 2)
        );
    }
}
