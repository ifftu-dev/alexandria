//! Plugin catalog — the local cache of plugin announcements seen on the
//! `/alexandria/plugins/1.0` gossip topic.
//!
//! Phase 3. Mirrors the existing course `catalog` table: announcements
//! flow in, get validated, and upsert here. The discovery UI reads from
//! this table; install pulls the actual bundle bytes from the iroh blob
//! store on demand.
//!
//! Built-in plugins are also written to this table at startup with
//! `source = 'builtin'` so the browse UI surfaces them alongside
//! community plugins.

use rusqlite::{params, OptionalExtension};

use crate::db::Database;
use crate::domain::plugin::{
    PluginAnnouncement, PluginCapability, PluginCatalogEntry, PluginKind, PluginManifest,
};

/// Insert or update a plugin catalog row from a parsed announcement.
/// `last_seen_at` always advances; `announced_at` is taken from the
/// announcement so author-stamped time is preserved.
pub fn upsert_announcement(
    db: &Database,
    announcement: &PluginAnnouncement,
    source: &str,
) -> Result<(), String> {
    let kinds_json = serde_json::to_string(&announcement.kinds).map_err(|e| e.to_string())?;
    let caps_json = serde_json::to_string(&announcement.capabilities).map_err(|e| e.to_string())?;
    let tags_json = serde_json::to_string(&announcement.subject_tags).map_err(|e| e.to_string())?;
    let plat_json = serde_json::to_string(&announcement.platforms).map_err(|e| e.to_string())?;

    db.conn()
        .execute(
            "INSERT INTO plugin_catalog \
             (plugin_cid, name, version, author_did, description, api_version, \
              kinds_json, capabilities_json, subject_tags_json, platforms_json, \
              has_grader, grader_cid, source, announced_at, last_seen_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, datetime('now')) \
             ON CONFLICT(plugin_cid) DO UPDATE SET \
               name = excluded.name, \
               version = excluded.version, \
               description = excluded.description, \
               kinds_json = excluded.kinds_json, \
               capabilities_json = excluded.capabilities_json, \
               subject_tags_json = excluded.subject_tags_json, \
               platforms_json = excluded.platforms_json, \
               has_grader = excluded.has_grader, \
               grader_cid = excluded.grader_cid, \
               announced_at = CASE \
                 WHEN excluded.announced_at > plugin_catalog.announced_at \
                 THEN excluded.announced_at ELSE plugin_catalog.announced_at END, \
               last_seen_at = datetime('now')",
            params![
                announcement.plugin_cid,
                announcement.name,
                announcement.version,
                announcement.author_did,
                announcement.description,
                announcement.api_version,
                kinds_json,
                caps_json,
                tags_json,
                plat_json,
                announcement.has_grader as i64,
                announcement.grader_cid,
                source,
                announcement.announced_at,
            ],
        )
        .map_err(|e| format!("failed to upsert plugin catalog row: {e}"))?;
    Ok(())
}

/// Convenience: build an announcement from an installed plugin's parsed
/// manifest. Used to seed the catalog from built-in plugins on startup
/// and to publish the local user's own plugin to the gossip topic.
pub fn announcement_from_manifest(
    plugin_cid: &str,
    manifest: &PluginManifest,
    announced_at: &str,
) -> PluginAnnouncement {
    PluginAnnouncement {
        plugin_cid: plugin_cid.to_string(),
        manifest_cid: plugin_cid.to_string(),
        author_did: manifest.author_did.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        api_version: manifest.api_version.clone(),
        description: manifest.description.clone(),
        kinds: manifest.kinds.clone(),
        capabilities: manifest.capabilities.clone(),
        subject_tags: manifest.subject_tags.clone(),
        platforms: manifest.platforms.clone(),
        has_grader: manifest.grader.is_some(),
        grader_cid: manifest.grader.as_ref().map(|g| g.cid.clone()),
        announced_at: announced_at.to_string(),
    }
}

/// List every catalog entry, newest first by `last_seen_at`.
pub fn list_catalog(db: &Database) -> Result<Vec<PluginCatalogEntry>, String> {
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT plugin_cid, name, version, author_did, description, api_version, \
                    kinds_json, capabilities_json, subject_tags_json, platforms_json, \
                    has_grader, grader_cid, source, announced_at, last_seen_at \
             FROM plugin_catalog ORDER BY last_seen_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_entry)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Look up a single catalog entry by CID.
pub fn get_entry(db: &Database, plugin_cid: &str) -> Result<Option<PluginCatalogEntry>, String> {
    db.conn()
        .query_row(
            "SELECT plugin_cid, name, version, author_did, description, api_version, \
                    kinds_json, capabilities_json, subject_tags_json, platforms_json, \
                    has_grader, grader_cid, source, announced_at, last_seen_at \
             FROM plugin_catalog WHERE plugin_cid = ?1",
            params![plugin_cid],
            row_to_entry,
        )
        .optional()
        .map_err(|e| e.to_string())
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<PluginCatalogEntry> {
    let kinds_json: String = row.get(6)?;
    let caps_json: String = row.get(7)?;
    let tags_json: String = row.get(8)?;
    let plat_json: String = row.get(9)?;
    let kinds: Vec<PluginKind> = serde_json::from_str(&kinds_json).unwrap_or_default();
    let capabilities: Vec<PluginCapability> = serde_json::from_str(&caps_json).unwrap_or_default();
    let subject_tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let platforms: Vec<String> = serde_json::from_str(&plat_json).unwrap_or_default();
    Ok(PluginCatalogEntry {
        plugin_cid: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        author_did: row.get(3)?,
        description: row.get(4)?,
        api_version: row.get(5)?,
        kinds,
        capabilities,
        subject_tags,
        platforms,
        has_grader: row.get::<_, i64>(10)? != 0,
        grader_cid: row.get(11)?,
        source: row.get(12)?,
        announced_at: row.get(13)?,
        last_seen_at: row.get(14)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugin::PluginKind;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("db");
        db.run_migrations().expect("migrations");
        db
    }

    fn sample_announcement(cid: &str) -> PluginAnnouncement {
        PluginAnnouncement {
            plugin_cid: cid.to_string(),
            manifest_cid: cid.to_string(),
            author_did: "did:key:z6Mksample".to_string(),
            name: "Sample Plugin".to_string(),
            version: "1.0.0".to_string(),
            api_version: "1".to_string(),
            description: Some("A test plugin".to_string()),
            kinds: vec![PluginKind::Interactive],
            capabilities: vec![],
            subject_tags: vec!["test".to_string()],
            platforms: vec!["macos".to_string()],
            has_grader: false,
            grader_cid: None,
            announced_at: "2026-04-15T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn upsert_and_list() {
        let db = test_db();
        let a = sample_announcement("cid-a");
        upsert_announcement(&db, &a, "gossip").unwrap();

        let list = list_catalog(&db).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].plugin_cid, "cid-a");
        assert_eq!(list[0].source, "gossip");
    }

    #[test]
    fn upsert_preserves_newer_announced_at() {
        let db = test_db();
        let mut a = sample_announcement("cid-a");
        a.announced_at = "2026-04-15T00:00:00Z".to_string();
        upsert_announcement(&db, &a, "gossip").unwrap();

        // Older announcement should not regress announced_at.
        let mut b = a.clone();
        b.announced_at = "2026-04-10T00:00:00Z".to_string();
        b.version = "0.9.0".to_string();
        upsert_announcement(&db, &b, "gossip").unwrap();

        let entry = get_entry(&db, "cid-a").unwrap().unwrap();
        assert_eq!(entry.announced_at, "2026-04-15T00:00:00Z");
    }
}
