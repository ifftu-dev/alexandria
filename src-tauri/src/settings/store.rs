//! Read/write layer over `app_settings`.
//!
//! All paths go through this module so that the typed registry is
//! the only source of truth for valid keys + scopes.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

use super::registry::{self, Scope, SettingEntry, SettingKey, SettingValue};

#[derive(Error, Debug)]
pub enum SettingsError {
    #[error("unknown setting key: {0}")]
    UnknownKey(String),
    #[error("invalid value for {key} ({kind}): {value}")]
    InvalidValue {
        key: String,
        kind: &'static str,
        value: String,
    },
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
}

pub struct SettingsStore;

impl SettingsStore {
    /// Read a typed setting, falling back to the registry default.
    /// Never errors — corrupt rows yield the default + a log line.
    pub fn get<T: SettingValue + 'static>(conn: &Connection, key: SettingKey<T>) -> T {
        match conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                params![key.key],
                |row| row.get::<_, String>(0),
            )
            .optional()
        {
            Ok(Some(s)) => T::from_setting_string(&s).unwrap_or_else(|| {
                log::warn!(
                    "settings: {} stored as invalid `{}` for type {} — falling back to default",
                    key.key,
                    s,
                    T::kind()
                );
                (key.default)()
            }),
            Ok(None) => (key.default)(),
            Err(e) => {
                log::warn!("settings: failed to read {}: {e}", key.key);
                (key.default)()
            }
        }
    }

    /// Persist a typed setting. The registry-declared scope is
    /// always used — callers cannot lie about scope.
    pub fn set<T: SettingValue + 'static>(
        conn: &Connection,
        key: SettingKey<T>,
        value: T,
    ) -> Result<(), SettingsError> {
        conn.execute(
            "INSERT INTO app_settings (key, value, scope, updated_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET
                 value      = excluded.value,
                 scope      = excluded.scope,
                 updated_at = excluded.updated_at",
            params![key.key, value.to_setting_string(), key.scope.as_str()],
        )?;
        Ok(())
    }

    /// Persist a setting by string key (used by IPC). Validates the
    /// key against the registry and rejects unknown keys; that's the
    /// only protection against a stale frontend smuggling arbitrary
    /// rows into `app_settings`.
    pub fn set_raw(conn: &Connection, key: &str, value: &str) -> Result<(), SettingsError> {
        let (scope, kind) =
            registry::lookup_meta(key).ok_or_else(|| SettingsError::UnknownKey(key.to_string()))?;

        // Best-effort type validation so the frontend learns of bad
        // values now rather than after the next get(). The actual
        // parse round-trips through SettingValue impls.
        let ok = match kind {
            "bool" => bool::from_setting_string(value).is_some(),
            "int" => i64::from_setting_string(value).is_some(),
            "float" => f64::from_setting_string(value).is_some(),
            "string" => true,
            "json" => super::registry::JsonSetting::from_setting_string(value).is_some(),
            _ => true,
        };
        if !ok {
            return Err(SettingsError::InvalidValue {
                key: key.to_string(),
                kind,
                value: value.to_string(),
            });
        }

        conn.execute(
            "INSERT INTO app_settings (key, value, scope, updated_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET
                 value      = excluded.value,
                 scope      = excluded.scope,
                 updated_at = excluded.updated_at",
            params![key, value, scope.as_str()],
        )?;
        Ok(())
    }

    /// Delete the override for a key, restoring the registry default.
    pub fn reset(conn: &Connection, key: &str) -> Result<(), SettingsError> {
        if registry::lookup_meta(key).is_none() {
            return Err(SettingsError::UnknownKey(key.to_string()));
        }
        conn.execute("DELETE FROM app_settings WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// Walk the registry and return every setting with its current
    /// value and metadata. Used by the settings panel.
    pub fn list_all(conn: &Connection) -> Result<Vec<SettingEntry>, SettingsError> {
        let overrides = read_overrides(conn)?;
        Ok(registry::all_entries(&overrides))
    }

    /// All key/value pairs the sync layer should replicate
    /// (rows where `scope = 'sync'`). The values are returned as
    /// strings; the sync layer carries them verbatim.
    pub fn list_syncable(
        conn: &Connection,
    ) -> Result<Vec<(String, String, String)>, SettingsError> {
        let mut stmt = conn.prepare(
            "SELECT key, value, updated_at
               FROM app_settings
              WHERE scope = 'sync'
              ORDER BY key",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Apply an incoming syncable row using last-write-wins on
    /// `updated_at`. Used by the sync receiver.
    pub fn apply_sync_row(
        conn: &Connection,
        key: &str,
        value: &str,
        remote_updated_at: &str,
    ) -> Result<bool, SettingsError> {
        let (scope, _) = match registry::lookup_meta(key) {
            Some(m) => m,
            None => return Ok(false), // forward-compat: ignore unknown keys
        };
        if scope != Scope::Sync {
            return Ok(false);
        }

        let local_updated_at: Option<String> = conn
            .query_row(
                "SELECT updated_at FROM app_settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(local) = local_updated_at {
            if local.as_str() >= remote_updated_at {
                return Ok(false);
            }
        }

        conn.execute(
            "INSERT INTO app_settings (key, value, scope, updated_at)
             VALUES (?1, ?2, 'sync', ?3)
             ON CONFLICT(key) DO UPDATE SET
                 value      = excluded.value,
                 scope      = 'sync',
                 updated_at = excluded.updated_at",
            params![key, value, remote_updated_at],
        )?;
        Ok(true)
    }
}

fn read_overrides(conn: &Connection) -> Result<HashMap<String, (String, Scope)>, SettingsError> {
    let mut stmt = conn.prepare("SELECT key, value, scope FROM app_settings")?;
    let mut out = HashMap::new();
    for row in stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (k, v, s) = row?;
        out.insert(k, (v, Scope::parse(&s)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::registry::keys;
    use super::*;
    use crate::db::Database;

    fn db() -> Database {
        let db = Database::open_in_memory().expect("open in-memory");
        db.run_migrations().expect("migrate");
        db
    }

    #[test]
    fn get_returns_default_when_unset() {
        let db = db();
        assert_eq!(SettingsStore::get(db.conn(), keys::UI_THEME), "system");
        assert!(SettingsStore::get(db.conn(), keys::SENTINEL_AI_SCORING));
    }

    #[test]
    fn set_then_get_round_trip() {
        let db = db();
        SettingsStore::set(db.conn(), keys::UI_THEME, "dark".to_string()).unwrap();
        assert_eq!(SettingsStore::get(db.conn(), keys::UI_THEME), "dark");

        SettingsStore::set(db.conn(), keys::SENTINEL_AI_SCORING, false).unwrap();
        assert!(!SettingsStore::get(db.conn(), keys::SENTINEL_AI_SCORING));
    }

    #[test]
    fn set_raw_rejects_unknown_keys() {
        let db = db();
        let err = SettingsStore::set_raw(db.conn(), "totally.bogus", "x").unwrap_err();
        assert!(matches!(err, SettingsError::UnknownKey(_)));
    }

    #[test]
    fn set_raw_rejects_invalid_type() {
        let db = db();
        let err =
            SettingsStore::set_raw(db.conn(), "sentinel.ai_scoring_enabled", "maybe").unwrap_err();
        assert!(matches!(err, SettingsError::InvalidValue { .. }));
    }

    #[test]
    fn reset_clears_override() {
        let db = db();
        SettingsStore::set(db.conn(), keys::UI_THEME, "dark".to_string()).unwrap();
        SettingsStore::reset(db.conn(), keys::UI_THEME.key).unwrap();
        assert_eq!(SettingsStore::get(db.conn(), keys::UI_THEME), "system");
    }

    #[test]
    fn list_all_marks_default_vs_overridden() {
        let db = db();
        SettingsStore::set(db.conn(), keys::UI_THEME, "dark".to_string()).unwrap();
        let entries = SettingsStore::list_all(db.conn()).unwrap();
        let theme = entries.iter().find(|e| e.key == "ui.theme").unwrap();
        assert_eq!(theme.current_value, "dark");
        assert!(!theme.is_default);

        let lang = entries.iter().find(|e| e.key == "user.language").unwrap();
        assert_eq!(lang.current_value, "en");
        assert!(lang.is_default);
    }

    #[test]
    fn list_syncable_excludes_device_scope() {
        let db = db();
        SettingsStore::set(db.conn(), keys::UI_THEME, "dark".to_string()).unwrap();
        SettingsStore::set(db.conn(), keys::DEVICE_LABEL, "laptop".to_string()).unwrap();

        let sync = SettingsStore::list_syncable(db.conn()).unwrap();
        let keys: Vec<_> = sync.iter().map(|(k, _, _)| k.as_str()).collect();
        assert!(keys.contains(&"ui.theme"));
        assert!(!keys.contains(&"device.label"));
    }

    #[test]
    fn apply_sync_row_lww_keeps_newer_local_value() {
        let db = db();
        // Local write — gets `datetime('now')`.
        SettingsStore::set(db.conn(), keys::UI_THEME, "dark".to_string()).unwrap();
        // Remote tries with an earlier timestamp — should lose.
        let applied =
            SettingsStore::apply_sync_row(db.conn(), "ui.theme", "light", "1999-01-01T00:00:00")
                .unwrap();
        assert!(!applied);
        assert_eq!(SettingsStore::get(db.conn(), keys::UI_THEME), "dark");
    }

    #[test]
    fn apply_sync_row_lww_overwrites_with_newer_remote() {
        let db = db();
        SettingsStore::set(db.conn(), keys::UI_THEME, "dark".to_string()).unwrap();
        let applied =
            SettingsStore::apply_sync_row(db.conn(), "ui.theme", "light", "2999-01-01T00:00:00")
                .unwrap();
        assert!(applied);
        assert_eq!(SettingsStore::get(db.conn(), keys::UI_THEME), "light");
    }

    #[test]
    fn apply_sync_row_refuses_device_scoped_keys() {
        let db = db();
        let applied = SettingsStore::apply_sync_row(
            db.conn(),
            "device.label",
            "stolen",
            "2999-01-01T00:00:00",
        )
        .unwrap();
        assert!(!applied);
    }

    #[test]
    fn apply_sync_row_ignores_unknown_keys() {
        let db = db();
        let applied =
            SettingsStore::apply_sync_row(db.conn(), "totally.bogus", "x", "2999-01-01T00:00:00")
                .unwrap();
        assert!(!applied);
    }
}
