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
const GRADER_FILENAME: &str = "grader.wasm";

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
    // the existing record. Same content ≠ reinstall.
    if let Some(existing) = get_installed(db, &plugin_cid)? {
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
    };

    db.conn()
        .execute(
            "INSERT INTO plugin_installed \
             (plugin_cid, name, version, author_did, install_path, source, manifest_json, installed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
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
    };

    db.conn()
        .execute(
            "INSERT INTO plugin_installed \
             (plugin_cid, name, version, author_did, install_path, source, manifest_json, installed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
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

    Ok(record)
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
            "SELECT plugin_cid, name, version, author_did, install_path, source, manifest_json, installed_at \
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
            "SELECT plugin_cid, name, version, author_did, install_path, source, manifest_json, installed_at \
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
    Ok(InstalledPlugin {
        plugin_cid: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        author_did: row.get(3)?,
        install_path: row.get(4)?,
        source: row.get(5)?,
        manifest_json: row.get(6)?,
        installed_at: row.get(7)?,
    })
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
