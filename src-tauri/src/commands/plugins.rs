//! IPC commands for the community plugin system.
//!
//! Phase 1 — install, uninstall, list, inspect, capability consent.
//! Phase 2 — submit-and-grade against deterministic WASM graders.
//! Phase 3 (later) — discovery/gossip commands alongside these.

use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;
use rusqlite::params;
use tauri::State;

use crate::crypto::did::{derive_did_key, Did};
use crate::crypto::hash::entity_id;
use crate::crypto::wallet;
use crate::db::Database;
use crate::domain::plugin::{
    InstalledPlugin, PluginCapability, PluginManifest, PluginPermissionRecord,
};
use crate::plugins::registry;
#[cfg(desktop)]
use crate::plugins::wasm_runtime::{GraderBudgets, ScoreRecord};
use crate::AppState;

/// Install a plugin from a directory on the user's local filesystem.
/// The directory must contain `manifest.json`, `manifest.sig`, and the
/// `entry` HTML file the manifest references. Phase 3 will add P2P
/// install from a gossip-announced CID.
#[tauri::command]
pub async fn plugin_install_from_file(
    state: State<'_, AppState>,
    directory: String,
) -> Result<InstalledPlugin, String> {
    check_rate_limit(&state, "plugin_install_from_file")?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let src = PathBuf::from(&directory);
    registry::install_from_directory(db, &state.plugins_dir, &src)
}

/// Uninstall a plugin by CID. Removes the bundle directory and cascades
/// deletion of `plugin_permissions`. Does *not* affect any course elements
/// that reference the plugin — they will simply fail to mount with a
/// "plugin not installed" message until the user re-installs it.
#[tauri::command]
pub async fn plugin_uninstall(
    state: State<'_, AppState>,
    plugin_cid: String,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::uninstall(db, &state.plugins_dir, &plugin_cid)
}

/// List every plugin installed on this node, newest first.
#[tauri::command]
pub async fn plugin_list(state: State<'_, AppState>) -> Result<Vec<InstalledPlugin>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::list_installed(db)
}

/// Return the parsed manifest for an installed plugin.
#[tauri::command]
pub async fn plugin_get_manifest(
    state: State<'_, AppState>,
    plugin_cid: String,
) -> Result<PluginManifest, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::get_manifest(db, &plugin_cid)
}

/// Grant a capability to a plugin. Scope is `"once"`, `"session"`, or
/// `"always"`. Calling twice for the same `(plugin_cid, capability)` pair
/// overwrites the previous grant. The frontend is expected to call this
/// *after* presenting a consent prompt to the user.
#[tauri::command]
pub async fn plugin_grant_capability(
    state: State<'_, AppState>,
    plugin_cid: String,
    capability: String,
    scope: String,
) -> Result<(), String> {
    let cap = PluginCapability::parse(&capability)
        .ok_or_else(|| format!("unknown capability '{capability}'"))?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::grant_capability(db, &plugin_cid, cap, &scope, None)
}

/// Revoke a previously-granted capability. Safe to call when no grant
/// exists (no-op).
#[tauri::command]
pub async fn plugin_revoke_capability(
    state: State<'_, AppState>,
    plugin_cid: String,
    capability: String,
) -> Result<(), String> {
    let cap = PluginCapability::parse(&capability)
        .ok_or_else(|| format!("unknown capability '{capability}'"))?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::revoke_capability(db, &plugin_cid, cap)
}

/// List all current capability grants for a plugin. Used by the Installed
/// Plugins page to show revoke buttons.
#[tauri::command]
pub async fn plugin_list_permissions(
    state: State<'_, AppState>,
    plugin_cid: String,
) -> Result<Vec<PluginPermissionRecord>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::list_permissions(db, &plugin_cid)
}

/// Run a graded plugin's WASM grader against a learner's submission and
/// persist the reproducibility bundle to `element_submissions`. Phase 2.
///
/// The host loads `grader.wasm` from the installed plugin bundle, hands
/// `(content, submission)` to the deterministic Wasmtime runtime, and
/// returns the resulting `ScoreRecord`. Any verifier — anywhere on the
/// network, any time later — can fetch the persisted CIDs and re-run
/// the grader to confirm the score reproduces.
///
/// `content_json` and `submission_json` are UTF-8 JSON strings. They are
/// stored as bytes (currently in-memory only — Phase 2 of the iroh blob
/// integration adds CID-based persistence; for now we hash for the bundle
/// but don't pin the bytes).
///
/// `learner_did` is taken from the caller — Phase 2 keeps the Ed25519
/// signing of the bundle deferred (the keystore is unlocked in the host
/// already, but signing a verifiable attestation is best added with the
/// VC-issuance code in §10.x). The persisted row carries `signed_attestation
/// = NULL` until that lands.
#[cfg(desktop)]
#[tauri::command]
pub async fn plugin_submit_and_grade(
    state: State<'_, AppState>,
    plugin_cid: String,
    element_id: String,
    enrollment_id: String,
    content_json: String,
    submission_json: String,
) -> Result<ScoreRecord, String> {
    check_rate_limit(&state, "plugin_submit_and_grade")?;

    // Derive the learner's DID from the unlocked keystore. The keystore
    // must be open — graded plugin submissions only make sense for an
    // enrolled learner whose vault is unlocked anyway.
    let (_signing_key, learner_did) = load_learner_did(&state).await?;
    let learner_did_str = learner_did.0;

    // Resolve manifest + grader path in a short DB-lock scope so we don't
    // hold the lock across grader execution.
    let (manifest, grader_path, grader_cid) = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;

        let installed = registry::get_installed(db, &plugin_cid)?
            .ok_or_else(|| format!("plugin not installed: {plugin_cid}"))?;
        let manifest = registry::get_manifest(db, &plugin_cid)?;
        let grader = manifest
            .grader
            .as_ref()
            .ok_or_else(|| {
                "plugin manifest has no grader (Phase 2 graded path requires one)".to_string()
            })?
            .clone();
        let grader_path = Path::new(&installed.install_path).join("grader.wasm");
        (manifest, grader_path, grader.cid)
    };

    let wasm_bytes =
        std::fs::read(&grader_path).map_err(|e| format!("failed to read grader.wasm: {e}"))?;

    // Sanity check: the bytes on disk must match the cid in the manifest.
    // The plugin install flow already verified the manifest signature, but
    // a tampered grader.wasm sitting next to a valid manifest would slip
    // through without this re-check.
    let computed = blake3::hash(&wasm_bytes).to_hex().to_string();
    if computed != grader_cid {
        return Err(format!(
            "grader.wasm hash mismatch: manifest declared {grader_cid}, on-disk is {computed}"
        ));
    }

    // Build the input envelope. Pre-canonicalize via serde_json::Value
    // so the bytes the grader sees match what we hash for content_cid /
    // submission_cid — the verifier needs to feed identical bytes back.
    let content_value: serde_json::Value = serde_json::from_str(&content_json)
        .map_err(|e| format!("content_json is not valid JSON: {e}"))?;
    let submission_value: serde_json::Value = serde_json::from_str(&submission_json)
        .map_err(|e| format!("submission_json is not valid JSON: {e}"))?;
    let envelope = serde_json::json!({
        "version": "1",
        "content": &content_value,
        "submission": &submission_value,
    });
    let envelope_bytes = serde_json::to_vec(&envelope)
        .map_err(|e| format!("failed to encode grade envelope: {e}"))?;

    let content_bytes = serde_json::to_vec(&content_value)
        .map_err(|e| format!("failed to re-encode content: {e}"))?;
    let submission_bytes = serde_json::to_vec(&submission_value)
        .map_err(|e| format!("failed to re-encode submission: {e}"))?;
    let content_cid = blake3::hash(&content_bytes).to_hex().to_string();
    let submission_cid = blake3::hash(&submission_bytes).to_hex().to_string();

    let record = state.grader_runtime.grade(
        &grader_cid,
        &wasm_bytes,
        &envelope_bytes,
        GraderBudgets::default(),
    )?;

    // Persist the reproducibility bundle. `signed_attestation` stays NULL
    // for now; the VC-signing path lands separately so this code doesn't
    // need to touch the keystore yet.
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    persist_submission(
        db,
        &SubmissionRow {
            element_id: &element_id,
            enrollment_id: &enrollment_id,
            submission_cid: &submission_cid,
            grader_cid: &grader_cid,
            content_cid: &content_cid,
            score: record.score,
            details: &record.details,
            learner_did: &learner_did_str,
        },
    )?;

    log::info!(
        "plugin grade: cid={plugin_cid} element={element_id} score={} ({})",
        record.score,
        manifest.name,
    );

    Ok(record)
}

/// Bundle of fields the host writes to `element_submissions` per grade.
/// Bundled into a struct so the helper has one parameter (clippy
/// complains about the original 9-arg form, and this makes the caller
/// site readable too).
struct SubmissionRow<'a> {
    element_id: &'a str,
    enrollment_id: &'a str,
    submission_cid: &'a str,
    grader_cid: &'a str,
    content_cid: &'a str,
    score: f64,
    details: &'a serde_json::Value,
    learner_did: &'a str,
}

fn persist_submission(db: &Database, row: &SubmissionRow<'_>) -> Result<(), String> {
    let id = entity_id(&[
        row.element_id,
        row.enrollment_id,
        row.submission_cid,
        row.grader_cid,
        row.content_cid,
    ]);
    let details_json = serde_json::to_string(row.details)
        .map_err(|e| format!("failed to serialize score details: {e}"))?;
    db.conn()
        .execute(
            "INSERT INTO element_submissions \
             (id, element_id, enrollment_id, submission_cid, grader_cid, content_cid, \
              score, score_details_json, learner_did) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                row.element_id,
                row.enrollment_id,
                row.submission_cid,
                row.grader_cid,
                row.content_cid,
                row.score,
                details_json,
                row.learner_did,
            ],
        )
        .map_err(|e| format!("failed to record element submission: {e}"))?;
    Ok(())
}

/// Load the local node's signing key + DID-Key. The keystore must be
/// unlocked. Mirrors the pattern in `commands/pinning.rs::load_pinner_key`.
async fn load_learner_did(state: &State<'_, AppState>) -> Result<(SigningKey, Did), String> {
    let ks_guard = state.keystore.lock().await;
    let ks = ks_guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(ks_guard);
    let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    let signing_key = SigningKey::from_bytes(&w.signing_key.to_bytes());
    let did = derive_did_key(&signing_key);
    Ok((signing_key, did))
}

fn check_rate_limit(state: &State<'_, AppState>, command: &str) -> Result<(), String> {
    let mut limiter = state
        .ipc_limiter
        .lock()
        .map_err(|_| "rate limiter poisoned".to_string())?;
    limiter.check(command)
}
