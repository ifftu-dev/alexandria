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
use crate::content_store::storage;
use crate::crypto::hash::{blake2b_256, entity_id};
use crate::crypto::wallet;
use crate::domain::sentinel::SentinelPriorAnnouncement;
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
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Keystroke,
    Mouse,
    /// Trained ONNX classifier weights — DAO-signed, runtime-swappable.
    /// Sample shape is a metadata object (`WeightsBlobMeta`) referencing
    /// the weights CID + eval artifact rather than a samples array.
    PasteClassifierWeights,
}

impl ModelKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelKind::Keystroke => "keystroke",
            ModelKind::Mouse => "mouse",
            ModelKind::PasteClassifierWeights => "paste_classifier_weights",
        }
    }
}

impl FromStr for ModelKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "keystroke" => Ok(ModelKind::Keystroke),
            "mouse" => Ok(ModelKind::Mouse),
            "paste_classifier_weights" => Ok(ModelKind::PasteClassifierWeights),
            "face" => Err("face is not a permitted model_kind for adversarial priors \
                 (see docs/sentinel-federation.md decision 2)"
                .into()),
            other => Err(format!("unknown model_kind: {other}")),
        }
    }
}

/// Minimum eval gates for a classifier-weights prior to be considered
/// "active" by the client. Mirrors the Phase 2b training acceptance
/// criteria; see docs/sentinel.md §AI Models.
pub const WEIGHTS_GATE_MIN_TPR: f64 = 0.92;
pub const WEIGHTS_GATE_MAX_FPR: f64 = 0.03;

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
    /// Only set for `paste_classifier_weights` rows. CID of the raw
    /// ONNX bytes (separate from `cid`, which points to the metadata
    /// envelope blob).
    #[serde(default)]
    pub weights_cid: Option<String>,
    /// Only set for `paste_classifier_weights` rows. References the
    /// CID of the eval JSON artifact.
    #[serde(default)]
    pub eval_cid: Option<String>,
    #[serde(default)]
    pub eval_tpr: Option<f64>,
    #[serde(default)]
    pub eval_fpr: Option<f64>,
    /// Version tag (e.g. `paste-v1`). Only set for weights rows.
    #[serde(default)]
    pub version: Option<String>,
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
/// Two shapes are accepted:
///
/// - Labeled-samples blob (`keystroke`, `mouse`): `samples` is a JSON
///   array of ≥ `MIN_SAMPLES` entries.
/// - Weights blob (`paste_classifier_weights`): `samples` is an object
///   conforming to [`WeightsBlobMeta`] — the on-chain artifact
///   referenced by the proposal carries the ONNX bytes under
///   `weights_cid`, not inline.
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
    let kind = ModelKind::from_str(&blob.model_kind)?;

    if blob.label.trim().is_empty() {
        return Err("label must be a non-empty string".into());
    }

    match kind {
        ModelKind::Keystroke | ModelKind::Mouse => {
            let sample_count = match &blob.samples {
                serde_json::Value::Array(items) => items.len(),
                _ => return Err("samples must be a JSON array".into()),
            };
            if sample_count < MIN_SAMPLES {
                return Err(format!(
                    "at least {MIN_SAMPLES} samples required (got {sample_count})"
                ));
            }
        }
        ModelKind::PasteClassifierWeights => {
            validate_weights_meta(&blob.samples)?;
        }
    }

    Ok(blob)
}

/// Metadata payload carried inside the `samples` field of a
/// `paste_classifier_weights` blob.
///
/// `weights_cid` points to the raw ONNX bytes (pinned separately);
/// `eval_cid` references the JSON eval report so any peer can re-verify
/// the gate numbers against the same holdout artifact.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WeightsBlobMeta {
    pub weights_cid: String,
    pub eval_cid: String,
    pub eval_tpr: f64,
    pub eval_fpr: f64,
    pub version: String,
}

pub fn validate_weights_meta(samples: &serde_json::Value) -> Result<WeightsBlobMeta, String> {
    let meta: WeightsBlobMeta = serde_json::from_value(samples.clone())
        .map_err(|e| format!("weights blob `samples` must be a WeightsBlobMeta object: {e}"))?;

    if meta.weights_cid.trim().is_empty() {
        return Err("weights_cid must be non-empty".into());
    }
    if meta.eval_cid.trim().is_empty() {
        return Err("eval_cid must be non-empty".into());
    }
    if meta.version.trim().is_empty() {
        return Err("version must be non-empty".into());
    }
    if !(0.0..=1.0).contains(&meta.eval_tpr) {
        return Err(format!("eval_tpr out of range [0,1]: {}", meta.eval_tpr));
    }
    if !(0.0..=1.0).contains(&meta.eval_fpr) {
        return Err(format!("eval_fpr out of range [0,1]: {}", meta.eval_fpr));
    }
    Ok(meta)
}

/// Returns true iff a weights row meets the runtime-load gate.
pub fn weights_gate_passes(eval_tpr: Option<f64>, eval_fpr: Option<f64>) -> bool {
    match (eval_tpr, eval_fpr) {
        (Some(tpr), Some(fpr)) => tpr >= WEIGHTS_GATE_MIN_TPR && fpr <= WEIGHTS_GATE_MAX_FPR,
        _ => false,
    }
}

/// Deterministic prior identifier. Same inputs ⇒ same id, so a blob
/// that's already been ratified can't be accidentally re-ratified under
/// a different id.
pub fn compute_prior_id(cid: &str, label: &str, model_kind: &str) -> String {
    entity_id(&[cid, label, model_kind])
}

/// **PLACEHOLDER signature — NOT cryptographically authenticated.**
///
/// This is a public Blake2b digest over `(cid, label, model_kind,
/// schema_version)`. Anyone can compute it; it binds the metadata to
/// the row but does NOT authenticate that the Sentinel DAO ratified the
/// content. Until the real DAO threshold-sig infrastructure ships:
///
/// - Default `sentinel_ai_scoring_enabled = false` in `useSentinel.ts`.
/// - The kill switch + version blocklist (`sentinel_set_kill_switch`,
///   `sentinel_blocklist_version`) are the operator's escape hatch if a
///   malicious row reaches `sentinel_priors`.
/// - Content-addressing transitively binds `weights_cid` / `eval_cid`
///   inside the envelope, so a single envelope CID covers the full
///   bundle — but an attacker with DB write access can still inject any
///   envelope they like. This is mitigated by the `verify_weights_candidate`
///   re-fetch + gate-recheck path (defense in depth).
///
/// Tracked in `docs/sentinel-federation.md` §12 (threshold-sig row).
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
    let kind = ModelKind::from_str(&blob.model_kind)
        .expect("validate_prior_blob already vetted model_kind");

    let (sample_count, weights_meta): (i64, Option<WeightsBlobMeta>) = match kind {
        ModelKind::Keystroke | ModelKind::Mouse => match &blob.samples {
            serde_json::Value::Array(items) => (items.len() as i64, None),
            _ => unreachable!("validate_prior_blob guarantees samples shape for labeled kinds"),
        },
        ModelKind::PasteClassifierWeights => {
            let meta = validate_weights_meta(&blob.samples)?;
            (0, Some(meta))
        }
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

        let weights_cid_val = weights_meta.as_ref().map(|m| m.weights_cid.clone());
        let eval_cid_val = weights_meta.as_ref().map(|m| m.eval_cid.clone());
        let eval_tpr_val = weights_meta.as_ref().map(|m| m.eval_tpr);
        let eval_fpr_val = weights_meta.as_ref().map(|m| m.eval_fpr);
        let version_val = weights_meta.as_ref().map(|m| m.version.clone());

        conn.execute(
            "INSERT OR IGNORE INTO sentinel_priors
                 (id, proposal_id, cid, model_kind, label, schema_version,
                  sample_count, notes, signature,
                  weights_cid, eval_cid, eval_tpr, eval_fpr, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                id,
                proposal_id,
                cid,
                blob.model_kind,
                blob.label,
                blob.schema_version as i64,
                sample_count,
                blob.notes,
                signature,
                weights_cid_val,
                eval_cid_val,
                eval_tpr_val,
                eval_fpr_val,
                version_val,
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
                        sample_count, notes, ratified_at, signature,
                        weights_cid, eval_cid, eval_tpr, eval_fpr, version
                 FROM sentinel_priors WHERE model_kind = ?1
                 ORDER BY ratified_at DESC"
                    .to_string(),
                vec![Box::new(k)],
            )
        } else {
            (
                "SELECT id, proposal_id, cid, model_kind, label, schema_version,
                        sample_count, notes, ratified_at, signature,
                        weights_cid, eval_cid, eval_tpr, eval_fpr, version
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

/// Result shape for [`sentinel_get_active_paste_classifier`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivePasteClassifier {
    pub prior_id: String,
    pub weights_cid: String,
    pub version: String,
    pub eval_tpr: f64,
    pub eval_fpr: f64,
    pub signature: String,
    pub ratified_at: String,
}

/// Returns true if the kill switch for the given model_kind is active.
pub fn kill_switch_active(conn: &rusqlite::Connection, model_kind: &str) -> rusqlite::Result<bool> {
    let active: Option<i64> = conn
        .query_row(
            "SELECT active FROM sentinel_kill_switch WHERE model_kind = ?1",
            params![model_kind],
            |row| row.get(0),
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(active.unwrap_or(0) != 0)
}

/// Returns the set of blocklisted versions for the given model_kind.
pub fn blocklisted_versions(
    conn: &rusqlite::Connection,
    model_kind: &str,
) -> rusqlite::Result<std::collections::HashSet<String>> {
    let mut stmt =
        conn.prepare("SELECT version FROM sentinel_weights_blocklist WHERE model_kind = ?1")?;
    let rows: Result<std::collections::HashSet<String>, _> = stmt
        .query_map(params![model_kind], |row| row.get::<_, String>(0))?
        .collect();
    rows
}

#[derive(Debug, Deserialize)]
pub struct SetKillSwitchRequest {
    pub model_kind: String,
    pub active: bool,
    pub reason: Option<String>,
    pub actor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BlocklistVersionRequest {
    pub model_kind: String,
    pub version: String,
    pub reason: Option<String>,
    pub actor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KillSwitchStatus {
    pub model_kind: String,
    pub active: bool,
    pub reason: Option<String>,
    pub activated_at: Option<String>,
    pub activated_by: Option<String>,
}

/// Toggle the kill switch for a model_kind. Validates the model_kind
/// is a known classifier kind so typos don't silently disable nothing.
///
/// Side effect: when activating the kill switch for
/// `paste_classifier_weights`, any DAO-loaded `tract` session is
/// immediately reverted to the embedded bundled artifact. Otherwise
/// the previously-loaded DAO weights would keep running in-memory
/// until the next process restart.
#[tauri::command]
pub async fn sentinel_set_kill_switch(
    state: State<'_, AppState>,
    req: SetKillSwitchRequest,
) -> Result<KillSwitchStatus, String> {
    let kind = ModelKind::from_str(&req.model_kind)?;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sentinel_kill_switch (model_kind, active, reason, activated_at, activated_by)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(model_kind) DO UPDATE SET
             active = excluded.active,
             reason = excluded.reason,
             activated_at = excluded.activated_at,
             activated_by = excluded.activated_by",
        params![
            req.model_kind,
            req.active as i64,
            req.reason,
            if req.active { Some(now.as_str()) } else { None },
            req.actor,
        ],
    )
    .map_err(|e| e.to_string())?;
    drop(db_guard);

    // Best-effort revert. Failure to revert (e.g. paste_classifier
    // module load issue) doesn't undo the kill-switch persist — the
    // DB row is authoritative, and `sentinel_get_active_paste_classifier`
    // already short-circuits to None on subsequent calls. Logging only.
    if req.active && matches!(kind, ModelKind::PasteClassifierWeights) {
        crate::sentinel::paste_classifier::revert_to_bundled();
        log::warn!(
            "[sentinel] kill switch activated for paste_classifier_weights — reverted to bundled"
        );
    }

    Ok(KillSwitchStatus {
        model_kind: req.model_kind,
        active: req.active,
        reason: req.reason,
        activated_at: if req.active { Some(now) } else { None },
        activated_by: req.actor,
    })
}

/// Inspect the kill-switch state for a model_kind. Returns
/// `{ active: false }` when no row exists.
#[tauri::command]
pub async fn sentinel_get_kill_switch(
    state: State<'_, AppState>,
    model_kind: String,
) -> Result<KillSwitchStatus, String> {
    ModelKind::from_str(&model_kind)?;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let row = conn
        .query_row(
            "SELECT active, reason, activated_at, activated_by
             FROM sentinel_kill_switch WHERE model_kind = ?1",
            params![model_kind],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .ok();

    Ok(match row {
        Some((active, reason, activated_at, activated_by)) => KillSwitchStatus {
            model_kind,
            active: active != 0,
            reason,
            activated_at,
            activated_by,
        },
        None => KillSwitchStatus {
            model_kind,
            active: false,
            reason: None,
            activated_at: None,
            activated_by: None,
        },
    })
}

/// Add a (model_kind, version) pair to the blocklist. Idempotent.
///
/// Side effect: if the currently-loaded paste-classifier session is
/// the blocked version, revert it to the bundled artifact. Without
/// this, the blocked weights would keep scoring in-memory until the
/// next session start (which is when `sentinel_get_active_paste_classifier`
/// re-runs and respects the new blocklist).
#[tauri::command]
pub async fn sentinel_blocklist_version(
    state: State<'_, AppState>,
    req: BlocklistVersionRequest,
) -> Result<(), String> {
    let kind = ModelKind::from_str(&req.model_kind)?;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    conn.execute(
        "INSERT OR IGNORE INTO sentinel_weights_blocklist
             (model_kind, version, reason, blocked_by)
         VALUES (?1, ?2, ?3, ?4)",
        params![req.model_kind, req.version, req.reason, req.actor],
    )
    .map_err(|e| e.to_string())?;
    drop(db_guard);

    if matches!(kind, ModelKind::PasteClassifierWeights) {
        let info = crate::sentinel::paste_classifier::loaded_info();
        if info.version == req.version {
            crate::sentinel::paste_classifier::revert_to_bundled();
            log::warn!(
                "[sentinel] blocklisted version {} was currently loaded — reverted to bundled",
                req.version
            );
        }
    }
    Ok(())
}

/// Remove a (model_kind, version) pair from the blocklist. No-op if
/// the row doesn't exist.
#[tauri::command]
pub async fn sentinel_unblocklist_version(
    state: State<'_, AppState>,
    model_kind: String,
    version: String,
) -> Result<(), String> {
    ModelKind::from_str(&model_kind)?;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();
    conn.execute(
        "DELETE FROM sentinel_weights_blocklist
         WHERE model_kind = ?1 AND version = ?2",
        params![model_kind, version],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Candidate row pulled from `sentinel_priors` before content-blob
/// re-verification. Lives only inside this module.
#[derive(Debug, Clone)]
struct WeightsCandidate {
    prior_id: String,
    envelope_cid: String,
    weights_cid: String,
    version: String,
    eval_tpr: f64,
    eval_fpr: f64,
    signature: String,
    ratified_at: String,
}

impl WeightsCandidate {
    fn into_active(self) -> ActivePasteClassifier {
        ActivePasteClassifier {
            prior_id: self.prior_id,
            weights_cid: self.weights_cid,
            version: self.version,
            eval_tpr: self.eval_tpr,
            eval_fpr: self.eval_fpr,
            signature: self.signature,
            ratified_at: self.ratified_at,
        }
    }
}

const F64_EPSILON: f64 = 1e-6;

/// Max size in bytes for a weights envelope OR eval report JSON blob.
/// Prevents OOM-by-design from a malicious envelope pointing at a huge
/// CID. 1 MiB is generous — real envelopes are a few hundred bytes.
const MAX_WEIGHTS_BLOB_BYTES: usize = 1024 * 1024;

/// Max size in bytes for a ratified ONNX weights binary. 50 MiB caps
/// the largest plausible classifier we'd ever ship via this pipeline.
pub const MAX_WEIGHTS_BYTES: usize = 50 * 1024 * 1024;

/// Resolver round-trip timeout for envelope + eval fetches inside the
/// active-classifier IPC. Prevents a hung Iroh peer from blocking session
/// start indefinitely; the client falls back to bundled on timeout.
const WEIGHTS_RESOLVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Pull ratified `paste_classifier_weights` rows that pass the
/// numeric gate, ordered newest-first. Pure DB query — split out so
/// it's testable with an in-memory database.
fn select_weights_candidates(conn: &rusqlite::Connection) -> Result<Vec<WeightsCandidate>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, cid, weights_cid, version, eval_tpr, eval_fpr, signature, ratified_at
             FROM sentinel_priors
             WHERE model_kind = 'paste_classifier_weights'
               AND weights_cid IS NOT NULL
               AND eval_tpr IS NOT NULL
               AND eval_fpr IS NOT NULL
               AND eval_tpr >= ?1
               AND eval_fpr <= ?2
             ORDER BY ratified_at DESC, version DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![WEIGHTS_GATE_MIN_TPR, WEIGHTS_GATE_MAX_FPR], |row| {
            Ok(WeightsCandidate {
                prior_id: row.get(0)?,
                envelope_cid: row.get(1)?,
                weights_cid: row.get(2)?,
                version: row.get(3)?,
                eval_tpr: row.get(4)?,
                eval_fpr: row.get(5)?,
                signature: row.get(6)?,
                ratified_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// JSON shape of the eval-report artifact that backs each weights row.
///
/// The Python eval harness (`tools/sentinel-train/eval.py`) emits this
/// exact structure. Other keys are tolerated but ignored.
#[derive(Debug, Deserialize)]
struct EvalReport {
    macro_tpr: f64,
    macro_fpr: f64,
}

/// Re-verifies a candidate row against its content-addressed envelope.
///
/// Defense in depth — three layers must agree before a weights row is
/// returned to the client:
///
/// 1. DB columns (what the local `sentinel_priors` row claims)
/// 2. The envelope blob at `cid` (what the DAO ratified, content-addressed)
/// 3. The eval JSON at `eval_cid` (what the holdout actually measured)
///
/// Each transition is bound by content-addressing, so the only way all
/// three can lie consistently is if the DAO itself published a lying
/// envelope/eval pair — at which point the signature check is the last
/// line of defense (currently placeholder; see `compute_prior_signature`).
async fn verify_weights_candidate(
    resolver: &crate::content_store::resolver::ContentResolver,
    c: &WeightsCandidate,
) -> Result<(), String> {
    // ---- Layer 1↔2: envelope must agree with DB ------------------------
    let envelope = tokio::time::timeout(WEIGHTS_RESOLVE_TIMEOUT, resolver.resolve(&c.envelope_cid))
        .await
        .map_err(|_| "envelope resolve timed out".to_string())?
        .map_err(|e| format!("envelope resolve failed: {e}"))?;
    if envelope.bytes.len() > MAX_WEIGHTS_BLOB_BYTES {
        return Err(format!(
            "envelope too large: {} > {} bytes",
            envelope.bytes.len(),
            MAX_WEIGHTS_BLOB_BYTES,
        ));
    }
    let json = std::str::from_utf8(&envelope.bytes)
        .map_err(|_| "envelope is not valid UTF-8 JSON".to_string())?;
    let blob = validate_prior_blob(json)?;
    if blob.model_kind != ModelKind::PasteClassifierWeights.as_str() {
        return Err(format!("envelope model_kind mismatch: {}", blob.model_kind));
    }
    let meta = validate_weights_meta(&blob.samples)?;

    if meta.weights_cid != c.weights_cid {
        return Err(format!(
            "weights_cid mismatch (db={}, envelope={})",
            c.weights_cid, meta.weights_cid,
        ));
    }
    if meta.version != c.version {
        return Err(format!(
            "version mismatch (db={}, envelope={})",
            c.version, meta.version,
        ));
    }
    if (meta.eval_tpr - c.eval_tpr).abs() > F64_EPSILON {
        return Err(format!(
            "eval_tpr mismatch (db={}, envelope={})",
            c.eval_tpr, meta.eval_tpr,
        ));
    }
    if (meta.eval_fpr - c.eval_fpr).abs() > F64_EPSILON {
        return Err(format!(
            "eval_fpr mismatch (db={}, envelope={})",
            c.eval_fpr, meta.eval_fpr,
        ));
    }
    if !weights_gate_passes(Some(meta.eval_tpr), Some(meta.eval_fpr)) {
        return Err(format!(
            "envelope-reported gate fails: tpr={}, fpr={}",
            meta.eval_tpr, meta.eval_fpr,
        ));
    }

    // ---- Layer 2↔3: eval JSON must agree with envelope -----------------
    let eval_blob = tokio::time::timeout(WEIGHTS_RESOLVE_TIMEOUT, resolver.resolve(&meta.eval_cid))
        .await
        .map_err(|_| "eval resolve timed out".to_string())?
        .map_err(|e| format!("eval resolve failed: {e}"))?;
    if eval_blob.bytes.len() > MAX_WEIGHTS_BLOB_BYTES {
        return Err(format!(
            "eval blob too large: {} > {} bytes",
            eval_blob.bytes.len(),
            MAX_WEIGHTS_BLOB_BYTES,
        ));
    }
    let eval_json = std::str::from_utf8(&eval_blob.bytes)
        .map_err(|_| "eval blob is not valid UTF-8 JSON".to_string())?;
    let report: EvalReport =
        serde_json::from_str(eval_json).map_err(|e| format!("eval JSON parse failed: {e}"))?;
    if (report.macro_tpr - meta.eval_tpr).abs() > F64_EPSILON {
        return Err(format!(
            "eval.macro_tpr mismatch (envelope={}, report={})",
            meta.eval_tpr, report.macro_tpr,
        ));
    }
    if (report.macro_fpr - meta.eval_fpr).abs() > F64_EPSILON {
        return Err(format!(
            "eval.macro_fpr mismatch (envelope={}, report={})",
            meta.eval_fpr, report.macro_fpr,
        ));
    }
    if !weights_gate_passes(Some(report.macro_tpr), Some(report.macro_fpr)) {
        return Err(format!(
            "eval report fails gate: tpr={}, fpr={}",
            report.macro_tpr, report.macro_fpr,
        ));
    }
    Ok(())
}

/// Return the highest-ranked ratified paste-classifier weights prior
/// that passes the runtime gate AND content-addressed re-verification,
/// or `None` if no such row exists. The client should fall back to its
/// bundled `paste-v1.onnx` when this returns `None`.
///
/// Ranking: newest `ratified_at` wins. Ties broken by lexical
/// `version` (so explicit semver-style ordering is honored as well).
/// Candidates that fail re-verification are logged and skipped rather
/// than surfaced.
#[tauri::command]
pub async fn sentinel_get_active_paste_classifier(
    state: State<'_, AppState>,
) -> Result<Option<ActivePasteClassifier>, String> {
    let (candidates, blocked): (Vec<WeightsCandidate>, std::collections::HashSet<String>) = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let conn = db.conn();

        // Kill switch short-circuits everything — return None so the
        // client falls back to bundled and the bundled toggle (or
        // disabling AI scoring entirely) is the operator's escape hatch.
        if kill_switch_active(conn, ModelKind::PasteClassifierWeights.as_str())
            .map_err(|e| e.to_string())?
        {
            log::warn!("[sentinel] paste_classifier_weights kill switch active — bundled fallback");
            return Ok(None);
        }

        let raw = select_weights_candidates(conn)?;
        let blocked = blocklisted_versions(conn, ModelKind::PasteClassifierWeights.as_str())
            .map_err(|e| e.to_string())?;
        (raw, blocked)
    };

    let candidates: Vec<WeightsCandidate> = candidates
        .into_iter()
        .filter(|c| !blocked.contains(&c.version))
        .collect();

    if candidates.is_empty() {
        return Ok(None);
    }

    let resolver_opt = { state.resolver.lock().await.as_ref().cloned() };
    let Some(resolver) = resolver_opt else {
        // Resolver not initialized — fall back to gate-only selection
        // for the first candidate. This path matters for tests + early
        // boot before the resolver spins up.
        log::warn!("[sentinel] resolver not initialized — returning gate-only selection");
        return Ok(Some(candidates.into_iter().next().unwrap().into_active()));
    };

    for c in candidates {
        match verify_weights_candidate(&resolver, &c).await {
            Ok(()) => return Ok(Some(c.into_active())),
            Err(e) => log::warn!(
                "[sentinel] weights candidate {} ({}) failed re-verify: {}",
                c.prior_id,
                c.version,
                e,
            ),
        }
    }
    Ok(None)
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
        weights_cid: row.get(10)?,
        eval_cid: row.get(11)?,
        eval_tpr: row.get(12)?,
        eval_fpr: row.get(13)?,
        version: row.get(14)?,
    })
}

fn read_prior_by_proposal(
    conn: &rusqlite::Connection,
    proposal_id: &str,
) -> Result<SentinelPrior, String> {
    conn.query_row(
        "SELECT id, proposal_id, cid, model_kind, label, schema_version,
                sample_count, notes, ratified_at, signature,
                weights_cid, eval_cid, eval_tpr, eval_fpr, version
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

    fn valid_weights_blob() -> serde_json::Value {
        json!({
            "schema_version": 1,
            "model_kind": "paste_classifier_weights",
            "label": "paste-v1",
            "samples": {
                "weights_cid": "blake3-weights-cid",
                "eval_cid": "blake3-eval-cid",
                "eval_tpr": 0.97,
                "eval_fpr": 0.01,
                "version": "paste-v1",
            },
            "notes": "synthetic train, holdout TPR=0.97",
        })
    }

    #[test]
    fn weights_kind_parses() {
        assert_eq!(
            ModelKind::from_str("paste_classifier_weights").unwrap(),
            ModelKind::PasteClassifierWeights,
        );
    }

    #[test]
    fn weights_blob_validates() {
        let blob = validate_prior_blob(&valid_weights_blob().to_string()).unwrap();
        assert_eq!(blob.model_kind, "paste_classifier_weights");
        assert_eq!(blob.label, "paste-v1");
        let meta = validate_weights_meta(&blob.samples).unwrap();
        assert_eq!(meta.weights_cid, "blake3-weights-cid");
        assert_eq!(meta.eval_tpr, 0.97);
    }

    #[test]
    fn weights_blob_rejects_array_samples() {
        let mut bad = valid_weights_blob();
        bad["samples"] = json!([]);
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(
            err.contains("WeightsBlobMeta") || err.contains("weights blob"),
            "got: {err}",
        );
    }

    #[test]
    fn weights_blob_rejects_out_of_range_tpr() {
        let mut bad = valid_weights_blob();
        bad["samples"]["eval_tpr"] = json!(1.5);
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("eval_tpr"), "got: {err}");
    }

    #[test]
    fn weights_blob_rejects_empty_weights_cid() {
        let mut bad = valid_weights_blob();
        bad["samples"]["weights_cid"] = json!("");
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("weights_cid"), "got: {err}");
    }

    // ---- DB integration tests for select_weights_candidates ------------

    use crate::db::Database;

    fn fresh_db() -> Database {
        let db = Database::open_in_memory().expect("open in-memory");
        db.run_migrations().expect("migrations");
        db
    }

    fn insert_proposal(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO governance_proposals
                 (id, dao_id, title, category, status, proposer)
             VALUES (?1, 'sentinel-dao', 'test', 'sentinel_prior', 'approved', 'addr1xxx')",
            params![id],
        )
        .expect("insert proposal");
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_weights_row(
        conn: &rusqlite::Connection,
        id: &str,
        proposal_id: &str,
        cid: &str,
        weights_cid: &str,
        version: &str,
        eval_tpr: f64,
        eval_fpr: f64,
        ratified_at: &str,
    ) {
        insert_proposal(conn, proposal_id);
        conn.execute(
            "INSERT INTO sentinel_priors
                 (id, proposal_id, cid, model_kind, label, schema_version,
                  sample_count, signature, ratified_at,
                  weights_cid, eval_cid, eval_tpr, eval_fpr, version)
             VALUES (?1, ?2, ?3, 'paste_classifier_weights', ?4, 1, 0,
                     'placeholder-sig', ?5,
                     ?6, 'eval-cid', ?7, ?8, ?4)",
            params![
                id,
                proposal_id,
                cid,
                version,
                ratified_at,
                weights_cid,
                eval_tpr,
                eval_fpr
            ],
        )
        .expect("insert weights row");
    }

    #[test]
    fn select_returns_empty_when_no_weights_rows() {
        let db = fresh_db();
        let rows = select_weights_candidates(db.conn()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn select_filters_below_tpr_gate() {
        let db = fresh_db();
        insert_weights_row(
            db.conn(),
            "id-low",
            "prop-low",
            "envelope-low",
            "weights-low",
            "paste-v1",
            0.91, // below 0.92
            0.01,
            "2026-05-01 12:00:00",
        );
        let rows = select_weights_candidates(db.conn()).unwrap();
        assert!(rows.is_empty(), "TPR below gate should not surface");
    }

    #[test]
    fn select_filters_above_fpr_gate() {
        let db = fresh_db();
        insert_weights_row(
            db.conn(),
            "id-fp",
            "prop-fp",
            "envelope-fp",
            "weights-fp",
            "paste-v1",
            0.99,
            0.04, // above 0.03
            "2026-05-01 12:00:00",
        );
        assert!(select_weights_candidates(db.conn()).unwrap().is_empty());
    }

    #[test]
    fn select_returns_passing_rows_newest_first() {
        let db = fresh_db();
        insert_weights_row(
            db.conn(),
            "id-v1",
            "prop-v1",
            "env-v1",
            "weights-v1",
            "paste-v1",
            0.95,
            0.02,
            "2026-04-01 12:00:00",
        );
        insert_weights_row(
            db.conn(),
            "id-v2",
            "prop-v2",
            "env-v2",
            "weights-v2",
            "paste-v2",
            0.98,
            0.01,
            "2026-05-01 12:00:00",
        );
        let rows = select_weights_candidates(db.conn()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].prior_id, "id-v2",
            "newer ratified row should rank first"
        );
        assert_eq!(rows[1].prior_id, "id-v1");
    }

    #[test]
    fn select_excludes_non_weights_kinds() {
        let db = fresh_db();
        // Hand-rolled insert for a keystroke (samples-array) row.
        insert_proposal(db.conn(), "prop-keystroke");
        db.conn()
            .execute(
                "INSERT INTO sentinel_priors
                     (id, proposal_id, cid, model_kind, label, schema_version,
                      sample_count, signature)
                 VALUES ('id-keystroke', 'prop-keystroke', 'cid-keystroke',
                         'keystroke', 'paste_macro', 1, 100, 'sig')",
                [],
            )
            .unwrap();
        assert!(
            select_weights_candidates(db.conn()).unwrap().is_empty(),
            "labeled-samples rows must not appear in the weights selector",
        );
    }

    #[test]
    fn weights_gate_thresholds_match_spec() {
        // Plan §AI Models: TPR ≥ 0.92, FPR ≤ 0.03.
        assert!(weights_gate_passes(Some(0.92), Some(0.03)));
        assert!(weights_gate_passes(Some(1.0), Some(0.0)));
        assert!(!weights_gate_passes(Some(0.91), Some(0.03)));
        assert!(!weights_gate_passes(Some(0.92), Some(0.04)));
        assert!(!weights_gate_passes(None, Some(0.0)));
        assert!(!weights_gate_passes(Some(1.0), None));
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
