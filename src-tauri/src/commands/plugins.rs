//! IPC commands for the community plugin system.
//!
//! Phase 1 — install, uninstall, list, inspect, capability consent.
//! Phase 2 — submit-and-grade against deterministic WASM graders.
//! Phase 3 (later) — discovery/gossip commands alongside these.

use std::path::{Path, PathBuf};

use std::collections::HashSet;

use ed25519_dalek::SigningKey;
use rusqlite::params;
use tauri::{AppHandle, Emitter, State};

use crate::crypto::did::{derive_did_key, Did};
use crate::crypto::hash::entity_id;
use crate::crypto::wallet;
use crate::db::Database;
use crate::domain::plugin::{
    InstalledPlugin, IrlSubmission, PluginAttestationEvent, PluginAttestationStatus,
    PluginCapability, PluginCatalogEntry, PluginManifest, PluginPermissionRecord,
};
#[cfg(grader)]
use crate::plugins::wasm_runtime::{GraderBudgets, ScoreRecord};
use crate::plugins::{attestation, builtins, catalog, irl_review, manifest, registry, verifier};
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
    let plugins_dir = state.plugins_dir()?;
    registry::install_from_directory(db, &plugins_dir, &src)
}

/// A plugin a course requires, plus whether it is already installed on this
/// machine. Powers the enrollment pre-flight dialog: the user sees which
/// plugins a course uses and which will be installed before continuing.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RequiredPlugin {
    pub plugin_cid: String,
    pub name: String,
    pub icon_path: Option<String>,
    /// `"global"` or `"course"`.
    pub scope: String,
    pub installed: bool,
}

/// Per-plugin progress emitted on the `plugin-install-progress` event while a
/// course's plugins install on enrollment. `step` is one of `installing`,
/// `done`, or `failed`.
#[derive(Debug, Clone, serde::Serialize)]
struct PluginInstallProgress {
    plugin_cid: String,
    name: String,
    /// 1-based position of this plugin in the install batch.
    index: usize,
    total: usize,
    step: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// The distinct plugin CIDs a course's elements reference.
fn course_plugin_cids(db: &Database, course_id: &str) -> Result<Vec<String>, String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT e.plugin_cid FROM course_elements e \
             JOIN course_chapters c ON e.chapter_id = c.id \
             WHERE c.course_id = ?1 AND e.element_type = 'plugin' \
               AND e.plugin_cid IS NOT NULL AND e.plugin_cid <> ''",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![course_id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Expand a set of directly-required builtin CIDs into the full ordered list of
/// builtin bundles to install — including transitive builtin dependencies, and
/// ordered so a dependency always precedes the plugin that needs it (the
/// `BUILTIN_PLUGINS` declaration order is topological).
fn builtin_install_plan(direct_cids: &[String]) -> Vec<&'static registry::BuiltinBundle<'static>> {
    let mut wanted: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = direct_cids.to_vec();
    while let Some(cid) = stack.pop() {
        if !wanted.insert(cid.clone()) {
            continue;
        }
        if let Some(bundle) = builtins::find_bundle_by_cid(&cid) {
            if let Ok(m) = manifest::parse_and_validate(bundle.manifest_json) {
                for dep_id in &m.dependencies {
                    if let Some(dep) = builtins::find_bundle_by_id(dep_id) {
                        stack.push(verifier::compute_plugin_cid(dep.manifest_json));
                    }
                }
            }
        }
    }
    builtins::BUILTIN_PLUGINS
        .iter()
        .filter(|b| wanted.contains(&verifier::compute_plugin_cid(b.manifest_json)))
        .collect()
}

/// List the plugins a course requires and whether each is already installed.
/// Called before enrollment so the UI can show a pre-flight dialog.
#[tauri::command]
pub async fn course_required_plugins(
    state: State<'_, AppState>,
    course_id: String,
) -> Result<Vec<RequiredPlugin>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let cids = course_plugin_cids(db, &course_id)?;
    let mut out = Vec::with_capacity(cids.len());
    for cid in cids {
        let installed = registry::get_installed(db, &cid)?;
        // Prefer the installed manifest; fall back to the embedded builtin
        // manifest so not-yet-installed plugins still show a name + icon.
        let manifest = match &installed {
            Some(_) => registry::get_manifest(db, &cid).ok(),
            None => builtins::find_bundle_by_cid(&cid)
                .and_then(|b| manifest::parse_and_validate(b.manifest_json).ok()),
        };
        let (name, icon_path, scope) = match manifest {
            Some(m) => {
                let scope = match m.scope {
                    crate::domain::plugin::PluginScope::Global => "global",
                    crate::domain::plugin::PluginScope::Course => "course",
                };
                (m.name, m.icon_path, scope.to_string())
            }
            None => (cid.clone(), None, "global".to_string()),
        };
        out.push(RequiredPlugin {
            plugin_cid: cid,
            name,
            icon_path,
            scope,
            installed: installed.is_some(),
        });
    }
    Ok(out)
}

/// Install every not-yet-installed builtin plugin a course requires (plus their
/// dependencies), precompiling graders as part of each install. Emits a
/// `plugin-install-progress` event per plugin so the UI can render a live
/// progress bar. Called on enrollment *before* `enroll`; if it returns an error
/// the caller must not enroll.
///
/// Only builtin plugins install here — community (non-builtin) plugins a course
/// might reference are a follow-up (courses today ship only builtins).
#[tauri::command]
pub async fn install_course_plugins(
    app: AppHandle,
    state: State<'_, AppState>,
    course_id: String,
) -> Result<(), String> {
    let plugins_dir = state.plugins_dir()?;

    // Resolve the ordered install plan (not-installed builtins + their deps)
    // under a short DB lock; the bundles are `'static` so we can hold them
    // across the install loop without borrowing the DB.
    let plan: Vec<&'static registry::BuiltinBundle<'static>> = {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;

        let required = course_plugin_cids(db, &course_id)?;
        let mut candidates = builtin_install_plan(&required);
        // Drop anything already installed (a dependency may already be present).
        let mut keep = Vec::new();
        for bundle in candidates.drain(..) {
            let cid = verifier::compute_plugin_cid(bundle.manifest_json);
            if registry::get_installed(db, &cid)?.is_none() {
                keep.push(bundle);
            }
        }
        keep
    };

    let total = plan.len();
    if total == 0 {
        return Ok(());
    }

    for (i, bundle) in plan.iter().enumerate() {
        let cid = verifier::compute_plugin_cid(bundle.manifest_json);
        let name = manifest::parse_and_validate(bundle.manifest_json)
            .map(|m| m.name)
            .unwrap_or_else(|_| cid.clone());

        let _ = app.emit(
            "plugin-install-progress",
            PluginInstallProgress {
                plugin_cid: cid.clone(),
                name: name.clone(),
                index: i + 1,
                total,
                step: "installing".to_string(),
                error: None,
            },
        );

        // install_builtin writes the bundle AND precompiles the grader (the
        // slow part), so a single "installing" step covers both.
        let result = {
            let db_guard = state
                .db
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            let db = db_guard.as_ref().ok_or("database not initialized")?;
            registry::install_builtin(db, &plugins_dir, bundle)
        };

        match result {
            Ok(_) => {
                let _ = app.emit(
                    "plugin-install-progress",
                    PluginInstallProgress {
                        plugin_cid: cid,
                        name,
                        index: i + 1,
                        total,
                        step: "done".to_string(),
                        error: None,
                    },
                );
            }
            Err(e) => {
                let _ = app.emit(
                    "plugin-install-progress",
                    PluginInstallProgress {
                        plugin_cid: cid,
                        name,
                        index: i + 1,
                        total,
                        step: "failed".to_string(),
                        error: Some(e.clone()),
                    },
                );
                return Err(e);
            }
        }
    }

    Ok(())
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

    // Refuse to remove a plugin that other installed plugins still depend
    // on — otherwise the dependents would silently break. The user must
    // uninstall the dependents first.
    let dependents = registry::list_dependents(db, &plugin_cid)?;
    if !dependents.is_empty() {
        let names: Vec<String> = dependents.into_iter().map(|p| p.name).collect();
        return Err(format!(
            "cannot uninstall: still required by {}",
            names.join(", ")
        ));
    }

    let plugins_dir = state.plugins_dir()?;
    registry::uninstall(db, &plugins_dir, &plugin_cid)
}

/// Persist a plugin's opaque per-element state (the `alex.persistState` blob —
/// e.g. a code editor's unsubmitted source). Upserts one row per element
/// so the latest write wins; the state is returned to the plugin in its `init`
/// payload by [`plugin_load_element_state`].
#[tauri::command]
pub async fn plugin_save_element_state(
    state: State<'_, AppState>,
    element_id: String,
    plugin_cid: String,
    state_json: String,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    db.conn()
        .execute(
            "INSERT INTO plugin_element_state (element_id, plugin_cid, state_json, updated_at) \
             VALUES (?1, ?2, ?3, datetime('now')) \
             ON CONFLICT(element_id) DO UPDATE SET \
               plugin_cid = excluded.plugin_cid, \
               state_json = excluded.state_json, \
               updated_at = excluded.updated_at",
            params![element_id, plugin_cid, state_json],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Load a plugin's saved per-element state, if any. Returns the opaque
/// `state_json` string the plugin last persisted for this element.
#[tauri::command]
pub async fn plugin_load_element_state(
    state: State<'_, AppState>,
    element_id: String,
) -> Result<Option<String>, String> {
    use rusqlite::OptionalExtension;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    db.conn()
        .query_row(
            "SELECT state_json FROM plugin_element_state WHERE element_id = ?1",
            params![element_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())
}

/// List the plugins that an installed plugin depends on (its resolved
/// dependency bundles).
#[tauri::command]
pub async fn plugin_list_dependencies(
    state: State<'_, AppState>,
    plugin_cid: String,
) -> Result<Vec<InstalledPlugin>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::list_dependencies(db, &plugin_cid)
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

/// Enable or disable an installed plugin. Disabled plugins remain on
/// disk and keep their capability grants, but the player refuses to
/// mount them. Used by Settings → Plugins.
#[tauri::command]
pub async fn plugin_set_enabled(
    state: State<'_, AppState>,
    plugin_cid: String,
    enabled: bool,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    registry::set_enabled(db, &plugin_cid, enabled)
}

/// Return the README markdown bundled with a plugin (empty string if
/// none). The Settings page renders this in the docs viewer.
#[tauri::command]
pub async fn plugin_get_docs(
    state: State<'_, AppState>,
    plugin_cid: String,
) -> Result<String, String> {
    let plugins_dir = state.plugins_dir()?;
    registry::read_docs(&plugins_dir, &plugin_cid)
}

/// Read an arbitrary in-bundle asset (icon, README screenshot, …) and
/// return it as a `data:` URL. The main app window's CSP forbids the
/// `plugin://` scheme for `<img>`, so thumbnails and README images are
/// inlined as data URLs instead. Path-traversal is guarded by
/// `registry::resolve_asset`. Returns an empty string if the path is
/// missing.
#[tauri::command]
pub async fn plugin_read_asset_data_url(
    state: State<'_, AppState>,
    plugin_cid: String,
    path: String,
) -> Result<String, String> {
    use base64::Engine as _;

    let plugins_dir = state.plugins_dir()?;
    let resolved = match registry::resolve_asset(&plugins_dir, &plugin_cid, &path) {
        Ok(p) => p,
        Err(_) => return Ok(String::new()),
    };
    let bytes = match std::fs::read(&resolved) {
        Ok(b) => b,
        Err(_) => return Ok(String::new()),
    };
    // Cap at 8 MiB so a hostile bundle can't blow up the webview with a
    // giant data URL.
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("plugin asset too large to inline (8 MiB cap)".into());
    }
    let mime = mime_for_path(&path);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

fn mime_for_path(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".avif") {
        "image/avif"
    } else {
        "application/octet-stream"
    }
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
#[cfg(grader)]
#[tauri::command]
pub async fn plugin_submit_and_grade(
    state: State<'_, AppState>,
    plugin_cid: String,
    element_id: String,
    enrollment_id: String,
    content_json: String,
    submission_json: String,
    // Active Sentinel integrity session for this attempt (if any). When set, the
    // issued credential is bound to it, so its assurance reflects how closely the
    // learner was monitored while solving.
    integrity_session_id: Option<String>,
) -> Result<ScoreRecord, String> {
    check_rate_limit(&state, "plugin_submit_and_grade")?;

    // Derive the learner's DID from the unlocked keystore. The keystore
    // must be open — graded plugin submissions only make sense for an
    // enrolled learner whose vault is unlocked anyway.
    let (signing_key, learner_did) = load_learner_did(&state).await?;
    let learner_did_str = learner_did.0.clone();

    // Resolve manifest + grader path in a short DB-lock scope so we don't
    // hold the lock across grader execution.
    let (manifest, grader_path, cwasm_path, grader_cid) = {
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
        let grader_path = Path::new(&installed.install_path).join(registry::GRADER_FILENAME);
        let cwasm_path = Path::new(&installed.install_path).join(registry::GRADER_CWASM_FILENAME);
        (manifest, grader_path, cwasm_path, grader.cid)
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
        Some(cwasm_path.as_path()),
        &envelope_bytes,
        GraderBudgets::default(),
    )?;

    // Durability: pin the exact grader-input bundle to the content store so the
    // deterministic grade can be re-derived later. Soft-fail — a blob-store
    // hiccup must never fail an otherwise-valid grade. `add_bytes` is async, so
    // this runs BEFORE we take the (non-Send) DB lock below. Note: if a content
    // key is set the bytes are AES-encrypted and the returned hash is of the
    // ciphertext (device-local key), so this provides durability + local
    // reproducibility, not public remote re-verification (that arrives with the
    // unencrypted-evidence path in a later phase).
    let bundle_pin = crate::ipfs::content::add_bytes(&state.content_node, &envelope_bytes)
        .await
        .map_err(|e| log::warn!("plugin grade: failed to pin submission bundle: {e}"))
        .ok();

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
            grader_version: &manifest.version,
        },
    )?;

    // Promote the bundle blob to a permanent pin so eviction never reclaims a
    // submission's reproducibility bytes.
    if let Some(pin) = &bundle_pin {
        crate::ipfs::storage::upsert_pin(db.conn(), &pin.hash, "submission", pin.size, false);
    }

    // Issue a signed Verifiable Credential for a passing grade — one per skill
    // the element is tagged with (`element_skill_tags`). Self-issued (subject ==
    // learner), same pipeline as the assessment path. Best-effort: a failure to
    // issue must not fail the grade itself, and each skill is independent.
    if record.score >= PLUGIN_PASS_THRESHOLD {
        let skills = element_skill_ids(db, &element_id).unwrap_or_default();
        if !skills.is_empty() {
            let now = crate::commands::credentials::now_rfc3339();
            let evidence = vec![
                submission_cid.clone(),
                content_cid.clone(),
                grader_cid.clone(),
            ];
            for skill_id in skills {
                let claim = crate::domain::vc::SkillClaim {
                    skill_id: skill_id.clone(),
                    level: crate::aggregation::level::map_level(record.score),
                    score: record.score,
                    evidence_refs: evidence.clone(),
                    rubric_version: Some(manifest.version.clone()),
                    assessment_method: Some("plugin_grader".to_string()),
                    provenance: None,
                };
                let req = crate::commands::credentials::IssueCredentialRequest {
                    credential_type: crate::domain::vc::CredentialType::AssessmentCredential,
                    subject: learner_did.clone(),
                    claim: crate::domain::vc::Claim::Skill(claim),
                    evidence_refs: vec![submission_cid.clone()],
                    expiration_date: None,
                    supersedes: None,
                    integrity_session_id: integrity_session_id.clone(),
                    integrity_policy: None,
                };
                match crate::commands::credentials::issue_credential_impl(
                    db.conn(),
                    &signing_key,
                    &learner_did,
                    &req,
                    &now,
                ) {
                    Ok(vc) => log::info!(
                        "plugin grade: issued credential {:?} for skill {skill_id}",
                        vc.id
                    ),
                    Err(e) => {
                        log::warn!("plugin grade: credential issuance failed for {skill_id}: {e}")
                    }
                }
            }
        }
    }

    log::info!(
        "plugin grade: cid={plugin_cid} element={element_id} score={} ({})",
        record.score,
        manifest.name,
    );

    Ok(record)
}

/// iOS stub for [`plugin_submit_and_grade`]. The wasmtime grader runtime runs
/// wherever native codegen is allowed — desktop and Android — but iOS forbids
/// JIT, so there it is compiled out (`wasm_runtime` is `#[cfg(grader)]`) and
/// this stub returns a stable, catchable `GraderUnavailable` marker instead of
/// failing as an unknown command. The editor plugin UIs match on this marker to
/// show a clean "runs elsewhere" message rather than a raw error. In-browser
/// "Run tests" still works on iOS — only graded submission (which relies on the
/// grader's hidden tests) is unavailable here.
#[cfg(not(grader))]
#[tauri::command]
pub async fn plugin_submit_and_grade() -> Result<serde_json::Value, String> {
    Err(
        "GraderUnavailable: graded submission runs on the desktop app; \
         this device can run tests but not submit for a grade"
            .to_string(),
    )
}

/// Minimum grade fraction (0.0–1.0) that earns a skill credential from a graded
/// plugin. A single challenge is coarse evidence, so the bar is a strong-but-
/// not-perfect pass; the aggregation layer weighs it by provenance afterward.
const PLUGIN_PASS_THRESHOLD: f64 = 0.7;

/// Skills a graded element is tagged with (`element_skill_tags`), highest weight
/// first. Drives which skill(s) a passing grade credentials.
fn element_skill_ids(db: &Database, element_id: &str) -> Result<Vec<String>, String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare(
            "SELECT skill_id FROM element_skill_tags WHERE element_id = ?1 \
             ORDER BY weight DESC, skill_id ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![element_id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
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
    grader_version: &'a str,
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
              score, score_details_json, learner_did, grader_version) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                row.grader_version,
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

// ---- IRL Review inbox -----------------------------------------------------

/// Submit a learner's work to the IRL Review local instructor inbox.
/// `submission_json` is the opaque plugin-defined payload (files +
/// comment); `skills_json` is the JSON array of self-declared skill tags.
/// Returns the new submission id.
#[tauri::command]
pub async fn irl_submit_for_review(
    state: State<'_, AppState>,
    plugin_cid: String,
    element_id: Option<String>,
    enrollment_id: Option<String>,
    submission_json: String,
    skills_json: String,
) -> Result<String, String> {
    check_rate_limit(&state, "irl_submit_for_review")?;

    let (_sk, learner_did) = load_learner_did(&state).await?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    let course_id =
        resolve_submission_course_id(db, enrollment_id.as_deref(), element_id.as_deref());

    irl_review::submit(
        db,
        &plugin_cid,
        element_id.as_deref(),
        enrollment_id.as_deref(),
        course_id.as_deref(),
        &learner_did.0,
        &submission_json,
        &skills_json,
    )
}

/// Resolve the course a review submission belongs to: prefer the enrollment's
/// course, else the element's chapter/course. `None` if neither resolves.
fn resolve_submission_course_id(
    db: &Database,
    enrollment_id: Option<&str>,
    element_id: Option<&str>,
) -> Option<String> {
    if let Some(eid) = enrollment_id {
        if let Ok(cid) = db.conn().query_row(
            "SELECT course_id FROM enrollments WHERE id = ?1",
            params![eid],
            |r| r.get::<_, String>(0),
        ) {
            return Some(cid);
        }
    }
    if let Some(elid) = element_id {
        if let Ok(cid) = db.conn().query_row(
            "SELECT ch.course_id FROM course_elements ce \
             JOIN course_chapters ch ON ce.chapter_id = ch.id WHERE ce.id = ?1",
            params![elid],
            |r| r.get::<_, String>(0),
        ) {
            return Some(cid);
        }
    }
    None
}

/// List the caller's own IRL Review submissions, newest first. Optionally
/// filtered to a single plugin CID.
#[tauri::command]
pub async fn irl_list_my_submissions(
    state: State<'_, AppState>,
    plugin_cid: Option<String>,
) -> Result<Vec<IrlSubmission>, String> {
    let (_sk, learner_did) = load_learner_did(&state).await?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    irl_review::list_for_learner(db, &learner_did.0, plugin_cid.as_deref())
}

/// List IRL Review submissions awaiting an instructor review. Optional
/// filter by plugin CID. Used by the instructor inbox UI in Settings.
#[tauri::command]
pub async fn irl_list_pending(
    state: State<'_, AppState>,
    plugin_cid: Option<String>,
) -> Result<Vec<IrlSubmission>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    // Single-user local node: the local user is the instructor and sees every
    // pending row, so course scoping stays disabled here (`None`). The
    // course-scoped path exists for multi-instructor / federated review later.
    irl_review::list_pending(db, plugin_cid.as_deref(), None)
}

/// Fetch a single submission by id (for instructors to open the review
/// form, or learners to view their feedback).
#[tauri::command]
pub async fn irl_get_submission(
    state: State<'_, AppState>,
    submission_id: String,
) -> Result<Option<IrlSubmission>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    irl_review::get(db, &submission_id)
}

/// Post a review on a pending submission. Score is 0..=1; feedback is
/// freeform; `skill_ratings_json` maps each declared skill to its rating.
#[tauri::command]
pub async fn irl_post_review(
    state: State<'_, AppState>,
    submission_id: String,
    score: f64,
    feedback: String,
    skill_ratings_json: String,
) -> Result<(), String> {
    check_rate_limit(&state, "irl_post_review")?;

    let (_sk, reviewer_did) = load_learner_did(&state).await?;

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    irl_review::post_review(
        db,
        &submission_id,
        &reviewer_did.0,
        score,
        &feedback,
        &skill_ratings_json,
    )
}

// ---- Phase 3: discovery + DAO attestation IPC -----------------------------

/// List every plugin known to this node — built-ins + locally-installed +
/// any plugins seen on the `/alexandria/plugins/1.0` gossip topic. The
/// browse UI reads from this.
#[tauri::command]
pub async fn plugin_browse_catalog(
    state: State<'_, AppState>,
) -> Result<Vec<PluginCatalogEntry>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    catalog::list_catalog(db)
}

/// Look up the Plugin DAO attestation status for a single plugin CID.
/// Returns `attested = true` when a multi-sig committee attestation row
/// exists in `plugin_attestations`. Active advisory notes are surfaced
/// as well — they don't affect attestation status, but the UI should
/// display them prominently.
#[tauri::command]
pub async fn plugin_attestation_status(
    state: State<'_, AppState>,
    plugin_cid: String,
) -> Result<PluginAttestationStatus, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;

    attestation::status_for(db, &plugin_cid)
}

/// Submit a fully-formed attestation event for verification + storage.
/// The host validates the multi-sig threshold against the embedded
/// committee pubkeys before persisting. Used by the gossip handler when
/// a new attestation arrives on `/alexandria/plugin-attestations/1.0`,
/// and by tests / CLI tooling. Idempotent — duplicates are no-ops.
#[tauri::command]
pub async fn plugin_ingest_attestation(
    state: State<'_, AppState>,
    event: PluginAttestationEvent,
) -> Result<(), String> {
    attestation::verify_event(&event, &attestation::AttestationPolicy::default())?;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    attestation::persist_event(db, &event)
}

#[cfg(test)]
mod grade_credential_tests {
    use super::*;
    use crate::crypto::did::derive_did_key;
    use crate::db::Database;
    use ed25519_dalek::SigningKey;

    /// Seed a minimal skill taxonomy plus one plugin element tagged with two
    /// skills at different weights, so the credential path has real
    /// `element_skill_tags`. Built from raw inserts (not the `dev-seed` seeder)
    /// so the test runs under the default feature set CI checks.
    fn seed_tagged_element(db: &Database) {
        let c = db.conn();
        c.execute(
            "INSERT INTO subject_fields (id, name) VALUES ('sf_cs', 'Computer Science')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub_lang', 'Languages', 'sf_cs')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO skills (id, name, subject_id) VALUES \
             ('skill_javascript', 'JavaScript', 'sub_lang'), \
             ('skill_python', 'Python', 'sub_lang')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO courses (id, title, author_address) VALUES ('c_grade', 'Grade test', 'addr1')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO course_chapters (id, course_id, title, position) VALUES ('ch_grade', 'c_grade', 'Ch', 0)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
             VALUES ('el_grade', 'ch_grade', 'Double', 'plugin', 0)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO element_skill_tags (element_id, skill_id, weight) \
             VALUES ('el_grade', 'skill_python', 0.5), ('el_grade', 'skill_javascript', 1.0)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn element_skill_ids_orders_by_weight_desc() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_tagged_element(&db);
        let skills = element_skill_ids(&db, "el_grade").unwrap();
        assert_eq!(
            skills,
            vec!["skill_javascript".to_string(), "skill_python".to_string()]
        );
        // Untagged element yields nothing (no spurious credentials).
        assert!(element_skill_ids(&db, "el_missing").unwrap().is_empty());
    }

    #[test]
    fn passing_grade_issues_one_credential_per_tagged_skill() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        seed_tagged_element(&db);

        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let learner = derive_did_key(&signing_key);
        let now = "2026-07-13T00:00:00Z";
        let score = 0.9_f64;
        assert!(score >= PLUGIN_PASS_THRESHOLD);

        let skills = element_skill_ids(&db, "el_grade").unwrap();
        for skill_id in &skills {
            let claim = crate::domain::vc::SkillClaim {
                skill_id: skill_id.clone(),
                level: crate::aggregation::level::map_level(score),
                score,
                evidence_refs: vec!["sub_cid".into()],
                rubric_version: Some("0.1.0".into()),
                assessment_method: Some("plugin_grader".into()),
                provenance: None,
            };
            let req = crate::commands::credentials::IssueCredentialRequest {
                credential_type: crate::domain::vc::CredentialType::AssessmentCredential,
                subject: learner.clone(),
                claim: crate::domain::vc::Claim::Skill(claim),
                evidence_refs: vec!["sub_cid".into()],
                expiration_date: None,
                supersedes: None,
                integrity_session_id: None,
                integrity_policy: None,
            };
            let vc = crate::commands::credentials::issue_credential_impl(
                db.conn(),
                &signing_key,
                &learner,
                &req,
                now,
            )
            .expect("issue credential");
            // Self-issued: subject == issuer == learner.
            assert_eq!(vc.credential_subject.id.0, learner.0);
        }

        let n: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, skills.len() as i64);
    }
}
