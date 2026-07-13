//! On-disk plugin bundle store + SQLite-backed install registry.
//!
//! Install, list, uninstall, and capability grant/revoke operations. All
//! paths are rooted at `plugins_dir` (typically `app_data_dir/plugins/`).
//! Installed bundles live at `plugins_dir/<plugin_cid>/` where the cid is
//! the hex-encoded BLAKE3 of the manifest bytes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension};

use crate::db::Database;
use crate::domain::plugin::{
    InstalledPlugin, PluginCapability, PluginManifest, PluginPermissionRecord,
};
use crate::plugins::{manifest, verifier};

const MANIFEST_FILENAME: &str = "manifest.json";
const SIGNATURE_FILENAME: &str = "manifest.sig";
pub const GRADER_FILENAME: &str = "grader.wasm";
/// Precompiled (`Engine::precompile_module`) sibling of `grader.wasm`, written
/// at install time so the first grade of a session is a fast `deserialize`
/// instead of a cranelift compile. Host/arch/wasmtime-version specific and
/// always regenerable from `grader.wasm`, so it is never shipped or verified.
pub const GRADER_CWASM_FILENAME: &str = "grader.cwasm";

pub const SOURCE_LOCAL_FILE: &str = "local_file";
pub const SOURCE_BUILTIN: &str = "builtin";

/// Aggregate result of an `install_all` builtin pass. Logs only — the
/// caller doesn't take action on these counts in v1.
#[derive(Debug, Default, Clone, Copy)]
pub struct InstallStats {
    pub installed: usize,
    pub failed: usize,
}

/// Install a plugin from an already-extracted directory on disk. In Phase 1
/// the caller hands over the directory path directly (the app file picker
/// in `plugins/Installed.vue` is what talks to this). Phase 3 adds a P2P
/// fetch path that resolves a CID to bytes and then calls through to here.
/// Best-effort: ahead-of-time compile a grader's wasm into its `.cwasm`
/// sibling so the first grade of a session is a fast `deserialize` rather than
/// a cranelift compile. Never fails the install — a precompile error just means
/// grading falls back to JIT (and rewrites the `.cwasm`) on first use.
fn write_precompiled_grader(dest_dir: &Path, grader_bytes: &[u8]) {
    match crate::plugins::wasm_runtime::precompile_grader(grader_bytes) {
        Ok(cwasm) => {
            if let Err(e) = fs::write(dest_dir.join(GRADER_CWASM_FILENAME), &cwasm) {
                log::warn!("failed to write precompiled grader: {e}");
            }
        }
        Err(e) => log::warn!("failed to precompile grader (will JIT on first grade): {e}"),
    }
}

pub fn install_from_directory(
    db: &Database,
    plugins_dir: &Path,
    src_dir: &Path,
) -> Result<InstalledPlugin, String> {
    if !src_dir.is_dir() {
        return Err(format!(
            "plugin source is not a directory: {}",
            src_dir.display()
        ));
    }

    let manifest_path = src_dir.join(MANIFEST_FILENAME);
    let signature_path = src_dir.join(SIGNATURE_FILENAME);

    let manifest_bytes =
        fs::read(&manifest_path).map_err(|e| format!("cannot read manifest.json: {e}"))?;
    let signature_bytes =
        fs::read(&signature_path).map_err(|e| format!("cannot read manifest.sig: {e}"))?;

    let manifest = manifest::parse_and_validate(&manifest_bytes)?;

    verifier::verify_manifest_signature(&manifest_bytes, &signature_bytes, &manifest.author_did)?;

    let plugin_cid = verifier::compute_plugin_cid(&manifest_bytes);

    // Entry file must exist and be within the bundle.
    let entry_path = src_dir.join(&manifest.entry);
    if !entry_path.is_file() {
        return Err(format!(
            "plugin bundle missing entry file '{}'",
            manifest.entry
        ));
    }

    // Idempotent install: if the CID is already present, just return
    // the existing record. Same content ≠ reinstall — but re-resolve
    // dependency edges in case they were cleared (CASCADE) or are new.
    if let Some(existing) = get_installed(db, &plugin_cid)? {
        resolve_and_record_dependencies(db, &plugin_cid, &manifest.dependencies)?;
        return Ok(existing);
    }

    let dest_dir = plugins_dir.join(&plugin_cid);
    if dest_dir.exists() {
        // Disk has leftovers from a failed previous install — remove
        // and reinstall fresh.
        fs::remove_dir_all(&dest_dir)
            .map_err(|e| format!("failed to clean stale plugin dir: {e}"))?;
    }
    fs::create_dir_all(&dest_dir).map_err(|e| format!("failed to create plugin dir: {e}"))?;

    copy_tree(src_dir, &dest_dir).map_err(|e| format!("failed to copy plugin bundle: {e}"))?;

    // Safety net: if the manifest we just parsed ever diverged from what
    // we copied, abort. This catches races where the source dir changed
    // mid-install.
    let copied_manifest = fs::read(dest_dir.join(MANIFEST_FILENAME))
        .map_err(|e| format!("failed to re-read copied manifest: {e}"))?;
    if verifier::compute_plugin_cid(&copied_manifest) != plugin_cid {
        let _ = fs::remove_dir_all(&dest_dir);
        return Err("plugin bundle changed during install".into());
    }

    // Precompile the grader (if any) from the CID-verified copy on disk. Never
    // trust a `.cwasm` a community bundle might ship — regenerate it locally.
    if manifest.grader.is_some() {
        if let Ok(grader_bytes) = fs::read(dest_dir.join(GRADER_FILENAME)) {
            write_precompiled_grader(&dest_dir, &grader_bytes);
        }
    }

    let record = InstalledPlugin {
        plugin_cid: plugin_cid.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        author_did: manifest.author_did.clone(),
        install_path: dest_dir.to_string_lossy().to_string(),
        source: SOURCE_LOCAL_FILE.to_string(),
        manifest_json: String::from_utf8(manifest_bytes.clone())
            .map_err(|e| format!("manifest is not valid UTF-8: {e}"))?,
        installed_at: chrono::Utc::now().to_rfc3339(),
        enabled: true,
    };

    db.conn()
        .execute(
            "INSERT INTO plugin_installed \
             (plugin_cid, name, version, author_did, install_path, source, manifest_json, installed_at, enabled) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
            params![
                record.plugin_cid,
                record.name,
                record.version,
                record.author_did,
                record.install_path,
                record.source,
                record.manifest_json,
                record.installed_at,
            ],
        )
        .map_err(|e| {
            // If the DB insert fails after we copied files, tidy up so
            // the on-disk state can't diverge from the DB.
            let _ = fs::remove_dir_all(&dest_dir);
            format!("failed to record plugin install: {e}")
        })?;

    // Record dependency edges now that the dependent row exists. A missing
    // dependency rolls the whole install back (row + bundle dir) so we never
    // leave a plugin installed whose dependencies aren't satisfied.
    if let Err(e) = resolve_and_record_dependencies(db, &plugin_cid, &manifest.dependencies) {
        let _ = db.conn().execute(
            "DELETE FROM plugin_installed WHERE plugin_cid = ?1",
            params![plugin_cid],
        );
        let _ = fs::remove_dir_all(&dest_dir);
        return Err(e);
    }

    Ok(record)
}

/// Install a built-in plugin from in-memory bytes. Phase 2 — the host
/// binary embeds first-party plugin bundles via `include_bytes!` and
/// installs them at startup so they're available without P2P fetch.
///
/// Distinct from [`install_from_directory`] in two ways:
///  * No `manifest.sig` — the binary itself is the trust root for
///    built-ins, so signature verification would be tautological.
///  * Source is recorded as `SOURCE_BUILTIN` so the UI can distinguish.
///
/// Idempotent: same `plugin_cid` (BLAKE3 of manifest bytes) → no-op.
pub fn install_builtin(
    db: &Database,
    plugins_dir: &Path,
    bundle: &BuiltinBundle<'_>,
) -> Result<InstalledPlugin, String> {
    let manifest = manifest::parse_and_validate(bundle.manifest_json)?;
    let plugin_cid = verifier::compute_plugin_cid(bundle.manifest_json);

    if let Some(existing) = get_installed(db, &plugin_cid)? {
        // Dev iteration: the manifest (and therefore the CID) often stays
        // stable across edits to ui/*.js / *.html / README, so the early
        // return would leave stale files on disk. Refresh the embedded
        // bytes every startup for builtins — they're authoritative.
        refresh_builtin_files(plugins_dir, &plugin_cid, bundle)?;
        resolve_and_record_dependencies(db, &plugin_cid, &manifest.dependencies)?;
        return Ok(existing);
    }

    let dest_dir = plugins_dir.join(&plugin_cid);
    if dest_dir.exists() {
        fs::remove_dir_all(&dest_dir)
            .map_err(|e| format!("failed to clean stale builtin dir: {e}"))?;
    }
    fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("failed to create builtin plugin dir: {e}"))?;

    fs::write(dest_dir.join(MANIFEST_FILENAME), bundle.manifest_json)
        .map_err(|e| format!("failed to write builtin manifest: {e}"))?;

    if let Some(grader_bytes) = bundle.grader_wasm {
        // The manifest's grader.cid must match the actual bytes — same
        // sanity check the grade IPC enforces. A mismatch here means
        // the manifest is stale relative to the embedded grader.
        let computed = blake3::hash(grader_bytes).to_hex().to_string();
        let declared = manifest
            .grader
            .as_ref()
            .map(|g| g.cid.as_str())
            .unwrap_or("");
        if !declared.is_empty() && declared != computed {
            return Err(format!(
                "builtin grader hash mismatch: manifest declared {declared}, embedded is {computed}"
            ));
        }
        fs::write(dest_dir.join(GRADER_FILENAME), grader_bytes)
            .map_err(|e| format!("failed to write builtin grader: {e}"))?;
        write_precompiled_grader(&dest_dir, grader_bytes);
    }

    for (rel_path, bytes) in bundle.ui_files {
        let dest_path = dest_dir.join(rel_path);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create builtin ui subdir: {e}"))?;
        }
        // Same path-traversal guard as install_from_directory.
        if rel_path.starts_with('/') || rel_path.contains("..") {
            return Err(format!("invalid builtin ui path '{rel_path}'"));
        }
        fs::write(&dest_path, bytes)
            .map_err(|e| format!("failed to write builtin ui file '{rel_path}': {e}"))?;
    }

    let record = InstalledPlugin {
        plugin_cid: plugin_cid.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        author_did: manifest.author_did.clone(),
        install_path: dest_dir.to_string_lossy().to_string(),
        source: SOURCE_BUILTIN.to_string(),
        manifest_json: String::from_utf8(bundle.manifest_json.to_vec())
            .map_err(|e| format!("builtin manifest is not UTF-8: {e}"))?,
        installed_at: chrono::Utc::now().to_rfc3339(),
        enabled: true,
    };

    db.conn()
        .execute(
            "INSERT INTO plugin_installed \
             (plugin_cid, name, version, author_did, install_path, source, manifest_json, installed_at, enabled) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
            params![
                record.plugin_cid,
                record.name,
                record.version,
                record.author_did,
                record.install_path,
                record.source,
                record.manifest_json,
                record.installed_at,
            ],
        )
        .map_err(|e| {
            let _ = fs::remove_dir_all(&dest_dir);
            format!("failed to record builtin install: {e}")
        })?;

    if let Err(e) = resolve_and_record_dependencies(db, &plugin_cid, &manifest.dependencies) {
        let _ = db.conn().execute(
            "DELETE FROM plugin_installed WHERE plugin_cid = ?1",
            params![plugin_cid],
        );
        let _ = fs::remove_dir_all(&dest_dir);
        return Err(e);
    }

    Ok(record)
}

/// Overwrite the on-disk copy of a builtin's bundle files with the
/// embedded bytes. Used at startup when the DB row for a builtin
/// already exists (idempotent CID install) but the embedded UI may
/// have changed in this build. Manifest CID is recomputed from the
/// embedded bytes by the caller, so paths line up.
fn refresh_builtin_files(
    plugins_dir: &Path,
    plugin_cid: &str,
    bundle: &BuiltinBundle<'_>,
) -> Result<(), String> {
    let dest_dir = plugins_dir.join(plugin_cid);
    if !dest_dir.exists() {
        fs::create_dir_all(&dest_dir)
            .map_err(|e| format!("failed to recreate builtin dir: {e}"))?;
    }
    fs::write(dest_dir.join(MANIFEST_FILENAME), bundle.manifest_json)
        .map_err(|e| format!("failed to refresh builtin manifest: {e}"))?;

    if let Some(grader_bytes) = bundle.grader_wasm {
        fs::write(dest_dir.join(GRADER_FILENAME), grader_bytes)
            .map_err(|e| format!("failed to refresh builtin grader: {e}"))?;
        // The grader bytes are keyed by the (stable) manifest CID, so the
        // `.cwasm` from the initial install is still valid — only (re)compile
        // when it's missing, so we don't pay a multi-MB cranelift compile on
        // every startup. A wasmtime-version-stale artifact is caught + rebuilt
        // lazily at grade time instead.
        if !dest_dir.join(GRADER_CWASM_FILENAME).exists() {
            write_precompiled_grader(&dest_dir, grader_bytes);
        }
    }

    for (rel_path, bytes) in bundle.ui_files {
        if rel_path.starts_with('/') || rel_path.contains("..") {
            return Err(format!("invalid builtin ui path '{rel_path}'"));
        }
        let dest_path = dest_dir.join(rel_path);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create builtin ui subdir: {e}"))?;
        }
        fs::write(&dest_path, bytes)
            .map_err(|e| format!("failed to refresh builtin ui file '{rel_path}': {e}"))?;
    }
    Ok(())
}

/// Static descriptor of a built-in plugin bundle. The host embeds these
/// via `include_bytes!`; see `src/plugins/builtins.rs`.
pub struct BuiltinBundle<'a> {
    /// Identifier for logs/errors only — the canonical id is the manifest's.
    pub slug: &'a str,
    pub manifest_json: &'a [u8],
    pub grader_wasm: Option<&'a [u8]>,
    pub ui_files: &'a [(&'a str, &'a [u8])],
}

/// Look up a single installed plugin by CID.
pub fn get_installed(db: &Database, plugin_cid: &str) -> Result<Option<InstalledPlugin>, String> {
    db.conn()
        .query_row(
            "SELECT plugin_cid, name, version, author_did, install_path, source, manifest_json, installed_at, enabled \
             FROM plugin_installed WHERE plugin_cid = ?1",
            params![plugin_cid],
            row_to_installed,
        )
        .optional()
        .map_err(|e| e.to_string())
}

/// List every installed plugin, newest first.
pub fn list_installed(db: &Database) -> Result<Vec<InstalledPlugin>, String> {
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT plugin_cid, name, version, author_did, install_path, source, manifest_json, installed_at, enabled \
             FROM plugin_installed ORDER BY installed_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_installed)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Return the parsed manifest for an installed plugin. The manifest is
/// re-parsed from the stored JSON each call — that way if we evolve
/// `PluginManifest` backward-compatibly, old installs pick up new fields
/// with no schema migration needed.
pub fn get_manifest(db: &Database, plugin_cid: &str) -> Result<PluginManifest, String> {
    let record = get_installed(db, plugin_cid)?
        .ok_or_else(|| format!("plugin not installed: {plugin_cid}"))?;
    manifest::parse_and_validate(record.manifest_json.as_bytes())
}

/// Find an installed plugin by its manifest `id` (`did:key:<author>#<slug>`)
/// rather than by CID. Dependencies are declared by id (stable across
/// reinstalls), so resolution maps id → the concrete installed bundle.
pub fn get_installed_by_id(
    db: &Database,
    plugin_id: &str,
) -> Result<Option<InstalledPlugin>, String> {
    for p in list_installed(db)? {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&p.manifest_json) {
            if value.get("id").and_then(|v| v.as_str()) == Some(plugin_id) {
                return Ok(Some(p));
            }
        }
    }
    Ok(None)
}

/// Resolve a freshly-installed plugin's declared dependencies and record the
/// edges. Each dependency must already be installed (a built-in, or installed
/// earlier in the same pass / by the user). Phase 1 has no on-demand bundle
/// fetch, so an unresolved dependency is a hard error listing what's missing.
///
/// Idempotent: edges are upserted, so re-running on an already-recorded
/// plugin is a no-op. Call *after* the dependent's `plugin_installed` row
/// exists (both columns are FKs into it).
pub fn resolve_and_record_dependencies(
    db: &Database,
    dependent_cid: &str,
    dependencies: &[String],
) -> Result<(), String> {
    let mut missing = Vec::new();
    let mut resolved: Vec<(String, String)> = Vec::new();
    for dep_id in dependencies {
        match get_installed_by_id(db, dep_id)? {
            Some(dep) => resolved.push((dep_id.clone(), dep.plugin_cid)),
            None => missing.push(dep_id.clone()),
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "missing plugin dependencies (install them first): {}",
            missing.join(", ")
        ));
    }
    for (dep_id, dep_cid) in resolved {
        db.conn()
            .execute(
                "INSERT INTO plugin_dependencies (plugin_cid, dependency_id, dependency_cid) \
                 VALUES (?1, ?2, ?3) \
                 ON CONFLICT(plugin_cid, dependency_id) DO UPDATE SET dependency_cid = excluded.dependency_cid",
                params![dependent_cid, dep_id, dep_cid],
            )
            .map_err(|e| format!("failed to record plugin dependency: {e}"))?;
    }
    Ok(())
}

/// The installed plugins that `plugin_cid` depends on.
pub fn list_dependencies(db: &Database, plugin_cid: &str) -> Result<Vec<InstalledPlugin>, String> {
    let dep_cids: Vec<String> = {
        let conn = db.conn();
        let mut stmt = conn
            .prepare("SELECT dependency_cid FROM plugin_dependencies WHERE plugin_cid = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![plugin_cid], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    let mut out = Vec::new();
    for cid in dep_cids {
        if let Some(p) = get_installed(db, &cid)? {
            out.push(p);
        }
    }
    Ok(out)
}

/// The installed plugins that depend on `dependency_cid` (reverse edges).
/// Used to refuse uninstalling a plugin others still need.
pub fn list_dependents(
    db: &Database,
    dependency_cid: &str,
) -> Result<Vec<InstalledPlugin>, String> {
    let dependent_cids: Vec<String> = {
        let conn = db.conn();
        let mut stmt = conn
            .prepare("SELECT plugin_cid FROM plugin_dependencies WHERE dependency_cid = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![dependency_cid], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    let mut out = Vec::new();
    for cid in dependent_cids {
        if let Some(p) = get_installed(db, &cid)? {
            out.push(p);
        }
    }
    Ok(out)
}

/// Remove a plugin bundle from disk and its rows from the DB.
pub fn uninstall(db: &Database, plugins_dir: &Path, plugin_cid: &str) -> Result<(), String> {
    // DB side first — if it fails, disk is untouched.
    let rows = db
        .conn()
        .execute(
            "DELETE FROM plugin_installed WHERE plugin_cid = ?1",
            params![plugin_cid],
        )
        .map_err(|e| e.to_string())?;
    if rows == 0 {
        return Err(format!("plugin not installed: {plugin_cid}"));
    }

    // Disk side. CASCADE on plugin_permissions has already cleared consent.
    let dir = plugins_dir.join(plugin_cid);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("failed to remove plugin dir: {e}"))?;
    }
    Ok(())
}

/// Grant a capability on a plugin. Upserts — calling twice with different
/// scopes overwrites the previous grant.
pub fn grant_capability(
    db: &Database,
    plugin_cid: &str,
    capability: PluginCapability,
    scope: &str,
    granted_until: Option<&str>,
) -> Result<(), String> {
    if !matches!(scope, "once" | "session" | "always") {
        return Err(format!("invalid permission scope '{scope}'"));
    }
    // The plugin must actually be installed — enforced by the FK, but
    // a nicer error than a cryptic constraint failure is worth the pre-check.
    if get_installed(db, plugin_cid)?.is_none() {
        return Err(format!("plugin not installed: {plugin_cid}"));
    }

    db.conn()
        .execute(
            "INSERT INTO plugin_permissions \
             (plugin_cid, capability, scope, granted_at, granted_until) \
             VALUES (?1, ?2, ?3, datetime('now'), ?4) \
             ON CONFLICT(plugin_cid, capability) DO UPDATE SET \
               scope = excluded.scope, \
               granted_at = excluded.granted_at, \
               granted_until = excluded.granted_until",
            params![plugin_cid, capability.as_str(), scope, granted_until],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn revoke_capability(
    db: &Database,
    plugin_cid: &str,
    capability: PluginCapability,
) -> Result<(), String> {
    db.conn()
        .execute(
            "DELETE FROM plugin_permissions WHERE plugin_cid = ?1 AND capability = ?2",
            params![plugin_cid, capability.as_str()],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_permissions(
    db: &Database,
    plugin_cid: &str,
) -> Result<Vec<PluginPermissionRecord>, String> {
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT plugin_cid, capability, scope, granted_at, granted_until \
             FROM plugin_permissions WHERE plugin_cid = ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![plugin_cid], |row| {
            Ok(PluginPermissionRecord {
                plugin_cid: row.get(0)?,
                capability: row.get(1)?,
                scope: row.get(2)?,
                granted_at: row.get(3)?,
                granted_until: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn row_to_installed(row: &rusqlite::Row<'_>) -> rusqlite::Result<InstalledPlugin> {
    let enabled_int: i64 = row.get(8)?;
    Ok(InstalledPlugin {
        plugin_cid: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        author_did: row.get(3)?,
        install_path: row.get(4)?,
        source: row.get(5)?,
        manifest_json: row.get(6)?,
        installed_at: row.get(7)?,
        enabled: enabled_int != 0,
    })
}

/// Remove every `source = 'builtin'` install whose CID is not in
/// `keep_cids` — used at startup to drop stale builtin rows left behind
/// when a builtin's manifest (and therefore its CID) changes between
/// releases. Community plugins are never touched. Returns the count
/// pruned.
pub fn prune_builtins_except(
    db: &Database,
    plugins_dir: &Path,
    keep_cids: &[String],
) -> Result<usize, String> {
    let stale: Vec<String> = {
        let conn = db.conn();
        let mut stmt = conn
            .prepare("SELECT plugin_cid FROM plugin_installed WHERE source = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![SOURCE_BUILTIN], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows.into_iter()
            .filter(|cid| !keep_cids.iter().any(|k| k == cid))
            .collect()
    };

    let mut pruned = 0;
    for cid in &stale {
        match uninstall(db, plugins_dir, cid) {
            Ok(()) => pruned += 1,
            Err(e) => log::warn!("failed to prune stale builtin {cid}: {e}"),
        }
    }
    Ok(pruned)
}

/// Enable or disable a plugin. Disabled plugins remain installed but the
/// player refuses to mount them. Capabilities are preserved across the
/// flip — re-enabling restores grants without prompting again.
pub fn set_enabled(db: &Database, plugin_cid: &str, enabled: bool) -> Result<(), String> {
    let rows = db
        .conn()
        .execute(
            "UPDATE plugin_installed SET enabled = ?1 WHERE plugin_cid = ?2",
            params![enabled as i64, plugin_cid],
        )
        .map_err(|e| e.to_string())?;
    if rows == 0 {
        return Err(format!("plugin not installed: {plugin_cid}"));
    }
    Ok(())
}

/// Read the plugin bundle's README markdown if one exists. Returns an
/// empty string if the bundle ships no README. The asset-path traversal
/// guard in [`resolve_asset`] is reused.
pub fn read_docs(plugins_dir: &Path, plugin_cid: &str) -> Result<String, String> {
    for candidate in ["README.md", "readme.md", "README.txt"] {
        let path = match resolve_asset(plugins_dir, plugin_cid, candidate) {
            Ok(p) => p,
            Err(_) => continue,
        };
        match fs::read_to_string(&path) {
            Ok(s) => return Ok(s),
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("failed to read plugin docs: {e}")),
        }
    }
    Ok(String::new())
}

/// Recursive directory copy that refuses symlinks (Phase 1 can't verify
/// their targets stay within the bundle). Regular files and directories
/// are allowed; everything else is skipped with a logged warning.
fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_symlink() {
            log::warn!(
                "plugin bundle contains a symlink, skipping: {}",
                src_path.display()
            );
            continue;
        }
        if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_tree(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Public helper for the asset protocol handler: resolve
/// `plugins_dir/<cid>/<relative_path>` with path-traversal guard.
pub fn resolve_asset(
    plugins_dir: &Path,
    plugin_cid: &str,
    relative_path: &str,
) -> Result<PathBuf, String> {
    // Reject traversal and absolute paths outright. Mirrors the
    // manifest::validate_relative_path check, but we don't trust the
    // request URL.
    if relative_path.starts_with('/') || relative_path.contains("..") {
        return Err("invalid plugin asset path".into());
    }

    let root = plugins_dir.join(plugin_cid);
    let requested = root.join(relative_path);

    // Canonicalize to resolve any `.` segments and confirm the resolved
    // path is still inside the plugin root. If canonicalize fails the
    // file probably doesn't exist — return the error unchanged.
    let canonical_requested =
        fs::canonicalize(&requested).map_err(|e| format!("plugin asset not found: {e}"))?;
    let canonical_root =
        fs::canonicalize(&root).map_err(|e| format!("plugin root not found: {e}"))?;

    if !canonical_requested.starts_with(&canonical_root) {
        return Err("plugin asset path escapes bundle root".into());
    }

    Ok(canonical_requested)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::did_from_verifying_key;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use tempfile::TempDir;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("db");
        db.run_migrations().expect("migrations");
        db
    }

    fn build_bundle(dir: &Path) -> (String, SigningKey) {
        let sk = SigningKey::generate(&mut OsRng);
        let did = did_from_verifying_key(&sk.verifying_key());

        let manifest_json = format!(
            r#"{{
                "id": "{}#demo",
                "version": "0.1.0",
                "api_version": "1",
                "host_min_version": "0.1.0",
                "name": "Demo",
                "author_did": "{}",
                "kinds": ["interactive"],
                "entry": "ui/index.html"
            }}"#,
            did.as_str(),
            did.as_str()
        );

        fs::write(dir.join(MANIFEST_FILENAME), &manifest_json).unwrap();
        let sig = sk.sign(manifest_json.as_bytes());
        fs::write(dir.join(SIGNATURE_FILENAME), sig.to_bytes()).unwrap();

        fs::create_dir_all(dir.join("ui")).unwrap();
        fs::write(dir.join("ui/index.html"), "<html></html>").unwrap();

        (manifest_json, sk)
    }

    /// Build a signed bundle with a given slug + declared dependencies.
    /// Returns the plugin's manifest id (`did:key:...#slug`).
    fn build_bundle_slug(dir: &Path, slug: &str, deps: &[String]) -> String {
        let sk = SigningKey::generate(&mut OsRng);
        let did = did_from_verifying_key(&sk.verifying_key());
        let id = format!("{}#{}", did.as_str(), slug);
        let deps_json = serde_json::to_string(deps).unwrap();
        let manifest_json = format!(
            r#"{{
                "id": "{id}",
                "version": "0.1.0",
                "api_version": "1",
                "host_min_version": "0.1.0",
                "name": "Plugin {slug}",
                "author_did": "{did}",
                "kinds": ["interactive"],
                "entry": "ui/index.html",
                "dependencies": {deps_json}
            }}"#,
            did = did.as_str()
        );
        fs::write(dir.join(MANIFEST_FILENAME), &manifest_json).unwrap();
        let sig = sk.sign(manifest_json.as_bytes());
        fs::write(dir.join(SIGNATURE_FILENAME), sig.to_bytes()).unwrap();
        fs::create_dir_all(dir.join("ui")).unwrap();
        fs::write(dir.join("ui/index.html"), "<html></html>").unwrap();
        id
    }

    #[test]
    fn install_with_satisfied_dependency_records_edges() {
        let db = test_db();
        let plugins_dir = TempDir::new().unwrap();

        let dep_src = TempDir::new().unwrap();
        let dep_id = build_bundle_slug(dep_src.path(), "dep", &[]);
        let dep = install_from_directory(&db, plugins_dir.path(), dep_src.path()).unwrap();

        let parent_src = TempDir::new().unwrap();
        build_bundle_slug(parent_src.path(), "parent", std::slice::from_ref(&dep_id));
        let parent = install_from_directory(&db, plugins_dir.path(), parent_src.path()).unwrap();

        let deps = list_dependencies(&db, &parent.plugin_cid).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].plugin_cid, dep.plugin_cid);

        let dependents = list_dependents(&db, &dep.plugin_cid).unwrap();
        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0].plugin_cid, parent.plugin_cid);

        // Resolving by id maps to the concrete installed bundle.
        let by_id = get_installed_by_id(&db, &dep_id).unwrap().unwrap();
        assert_eq!(by_id.plugin_cid, dep.plugin_cid);
    }

    #[test]
    fn install_with_missing_dependency_is_rejected_and_rolled_back() {
        let db = test_db();
        let plugins_dir = TempDir::new().unwrap();

        let parent_src = TempDir::new().unwrap();
        let bogus = "did:key:z6MkubM4drVzMMYqS5wyWo2tqtWgLrGCMY4qNsEaUjHbLbAN#nope".to_string();
        build_bundle_slug(parent_src.path(), "parent", &[bogus]);

        let err = install_from_directory(&db, plugins_dir.path(), parent_src.path());
        assert!(err.is_err());
        // Rolled back: nothing installed, no bundle dir left behind.
        assert_eq!(list_installed(&db).unwrap().len(), 0);
    }

    #[test]
    fn dependency_edges_cascade_when_dependency_removed() {
        let db = test_db();
        let plugins_dir = TempDir::new().unwrap();

        let dep_src = TempDir::new().unwrap();
        let dep_id = build_bundle_slug(dep_src.path(), "dep", &[]);
        let dep = install_from_directory(&db, plugins_dir.path(), dep_src.path()).unwrap();

        let parent_src = TempDir::new().unwrap();
        build_bundle_slug(parent_src.path(), "parent", &[dep_id]);
        let parent = install_from_directory(&db, plugins_dir.path(), parent_src.path()).unwrap();

        // Mechanical uninstall of the dependency cascades its edges away.
        uninstall(&db, plugins_dir.path(), &dep.plugin_cid).unwrap();
        assert!(list_dependencies(&db, &parent.plugin_cid)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn install_then_list() {
        let db = test_db();
        let plugins_dir = TempDir::new().unwrap();
        let src = TempDir::new().unwrap();
        build_bundle(src.path());

        let installed =
            install_from_directory(&db, plugins_dir.path(), src.path()).expect("install");
        assert_eq!(installed.name, "Demo");
        assert_eq!(installed.source, SOURCE_LOCAL_FILE);

        let list = list_installed(&db).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].plugin_cid, installed.plugin_cid);
    }

    #[test]
    fn reinstall_is_idempotent() {
        let db = test_db();
        let plugins_dir = TempDir::new().unwrap();
        let src = TempDir::new().unwrap();
        build_bundle(src.path());

        let a = install_from_directory(&db, plugins_dir.path(), src.path()).unwrap();
        let b = install_from_directory(&db, plugins_dir.path(), src.path()).unwrap();
        assert_eq!(a.plugin_cid, b.plugin_cid);

        assert_eq!(list_installed(&db).unwrap().len(), 1);
    }

    #[test]
    fn uninstall_removes_row_and_dir() {
        let db = test_db();
        let plugins_dir = TempDir::new().unwrap();
        let src = TempDir::new().unwrap();
        build_bundle(src.path());

        let installed = install_from_directory(&db, plugins_dir.path(), src.path()).unwrap();
        let dir = plugins_dir.path().join(&installed.plugin_cid);
        assert!(dir.exists());

        uninstall(&db, plugins_dir.path(), &installed.plugin_cid).unwrap();
        assert!(!dir.exists());
        assert_eq!(list_installed(&db).unwrap().len(), 0);
    }

    #[test]
    fn rejects_bad_signature() {
        let db = test_db();
        let plugins_dir = TempDir::new().unwrap();
        let src = TempDir::new().unwrap();
        build_bundle(src.path());
        // Corrupt the signature.
        let mut sig = fs::read(src.path().join(SIGNATURE_FILENAME)).unwrap();
        sig[0] ^= 0xFF;
        fs::write(src.path().join(SIGNATURE_FILENAME), sig).unwrap();

        let err = install_from_directory(&db, plugins_dir.path(), src.path());
        assert!(err.is_err());
    }

    #[test]
    fn grant_and_revoke() {
        let db = test_db();
        let plugins_dir = TempDir::new().unwrap();
        let src = TempDir::new().unwrap();
        build_bundle(src.path());
        let installed = install_from_directory(&db, plugins_dir.path(), src.path()).unwrap();

        grant_capability(
            &db,
            &installed.plugin_cid,
            PluginCapability::Microphone,
            "always",
            None,
        )
        .unwrap();
        let perms = list_permissions(&db, &installed.plugin_cid).unwrap();
        assert_eq!(perms.len(), 1);
        assert_eq!(perms[0].capability, "microphone");
        assert_eq!(perms[0].scope, "always");

        revoke_capability(&db, &installed.plugin_cid, PluginCapability::Microphone).unwrap();
        assert!(list_permissions(&db, &installed.plugin_cid)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn resolve_asset_rejects_traversal() {
        let plugins_dir = TempDir::new().unwrap();
        let cid_dir = plugins_dir.path().join("abc123");
        fs::create_dir_all(cid_dir.join("ui")).unwrap();
        fs::write(cid_dir.join("ui/index.html"), "<html></html>").unwrap();

        assert!(resolve_asset(plugins_dir.path(), "abc123", "ui/index.html").is_ok());
        assert!(resolve_asset(plugins_dir.path(), "abc123", "../outside").is_err());
        assert!(resolve_asset(plugins_dir.path(), "abc123", "/etc/passwd").is_err());
    }
}
