//! Per-plugin user settings.
//!
//! A manifest may declare a `settings` array (see
//! [`PluginSettingSpec`](crate::domain::plugin::PluginSettingSpec)). The
//! Settings page renders it as a form; the values reach the plugin in its
//! `init` payload as `settings`.
//!
//! Values are stored per plugin **manifest id** (stable across plugin
//! upgrades, unlike the CID) in the profile's `app_settings` table under
//! `plugin_setting:<manifest id>:<key>`, scope `'device'`, as JSON text. They
//! are not part of the typed settings registry, so the registry's list and the
//! device-sync snapshot (which only carries `scope = 'sync'`) never see them.
//!
//! Every read re-validates against the current manifest: a stored value that
//! no longer fits (an option removed in an upgrade, a narrowed range) falls
//! back to the default instead of reaching the plugin.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::db::Database;
use crate::domain::plugin::{PluginManifest, PluginSettingSpec, PluginSettingType};
use crate::plugins::{manifest, registry};

/// Most settings one manifest may declare.
pub const MAX_SETTINGS: usize = 32;
/// Longest setting key.
pub const MAX_KEY_LEN: usize = 40;
/// Longest label (setting or option), in characters.
pub const MAX_LABEL_LEN: usize = 120;
/// Longest setting description, in characters.
pub const MAX_DESCRIPTION_LEN: usize = 500;
/// Most search keywords per setting, and the longest one.
pub const MAX_KEYWORDS: usize = 16;
pub const MAX_KEYWORD_LEN: usize = 40;
/// Most options per `select`, and the longest option value.
pub const MAX_OPTIONS: usize = 50;
pub const MAX_OPTION_VALUE_LEN: usize = 64;
/// `text` length limit when the manifest gives none, and the largest it may give.
pub const DEFAULT_TEXT_MAX_LENGTH: u32 = 200;
pub const MAX_TEXT_MAX_LENGTH: u32 = 2000;

const STORAGE_PREFIX: &str = "plugin_setting:";

/// Validate a manifest's `settings` array. Called from
/// [`manifest::validate`], so a manifest with malformed settings does not
/// install.
pub fn validate_specs(specs: &[PluginSettingSpec]) -> Result<(), String> {
    if specs.len() > MAX_SETTINGS {
        return Err(format!(
            "plugin manifest declares {} settings (at most {MAX_SETTINGS})",
            specs.len()
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for spec in specs {
        validate_spec(spec)?;
        if !seen.insert(spec.key.as_str()) {
            return Err(format!("duplicate setting key '{}'", spec.key));
        }
    }
    Ok(())
}

fn validate_spec(spec: &PluginSettingSpec) -> Result<(), String> {
    let key = &spec.key;
    if key.is_empty()
        || key.len() > MAX_KEY_LEN
        || !key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(format!(
            "setting key '{key}' must match [a-z0-9_]{{1,{MAX_KEY_LEN}}}"
        ));
    }
    if spec.setting_type.is_empty() || spec.setting_type.len() > MAX_KEY_LEN {
        return Err(format!("setting '{key}' has an invalid type"));
    }
    check_text_field(key, "label", &spec.label, MAX_LABEL_LEN)?;
    if spec.label.trim().is_empty() {
        return Err(format!("setting '{key}' must have a non-empty label"));
    }
    if let Some(description) = &spec.description {
        check_text_field(key, "description", description, MAX_DESCRIPTION_LEN)?;
    }
    if spec.keywords.len() > MAX_KEYWORDS {
        return Err(format!(
            "setting '{key}' has more than {MAX_KEYWORDS} keywords"
        ));
    }
    for keyword in &spec.keywords {
        check_text_field(key, "keyword", keyword, MAX_KEYWORD_LEN)?;
    }

    // Unknown types are accepted (forward compatibility) but carry no
    // type-specific checks: the host never renders them or hands them to the
    // plugin.
    let Some(kind) = spec.known_type() else {
        return Ok(());
    };
    match kind {
        PluginSettingType::Toggle => {}
        PluginSettingType::Select => {
            if spec.options.is_empty() {
                return Err(format!("select setting '{key}' must declare options"));
            }
            if spec.options.len() > MAX_OPTIONS {
                return Err(format!(
                    "select setting '{key}' has more than {MAX_OPTIONS} options"
                ));
            }
            let mut values = std::collections::HashSet::new();
            for option in &spec.options {
                if option.value.is_empty() || option.value.chars().count() > MAX_OPTION_VALUE_LEN {
                    return Err(format!(
                        "select setting '{key}' has an option value that is empty or longer than {MAX_OPTION_VALUE_LEN} characters"
                    ));
                }
                check_text_field(key, "option label", &option.label, MAX_LABEL_LEN)?;
                if option.label.trim().is_empty() {
                    return Err(format!(
                        "select setting '{key}' has an option with no label"
                    ));
                }
                if !values.insert(option.value.as_str()) {
                    return Err(format!(
                        "select setting '{key}' repeats option '{}'",
                        option.value
                    ));
                }
            }
        }
        PluginSettingType::Number => {
            for (name, bound) in [("min", spec.min), ("max", spec.max), ("step", spec.step)] {
                if bound.is_some_and(|v| !v.is_finite()) {
                    return Err(format!("number setting '{key}' has a non-finite {name}"));
                }
            }
            if let (Some(min), Some(max)) = (spec.min, spec.max) {
                if min > max {
                    return Err(format!("number setting '{key}' has min greater than max"));
                }
            }
            if spec.step.is_some_and(|s| s <= 0.0) {
                return Err(format!("number setting '{key}' must have a positive step"));
            }
        }
        PluginSettingType::Text => {
            if let Some(max_length) = spec.max_length {
                if max_length == 0 || max_length > MAX_TEXT_MAX_LENGTH {
                    return Err(format!(
                        "text setting '{key}' must have max_length between 1 and {MAX_TEXT_MAX_LENGTH}"
                    ));
                }
            }
        }
        PluginSettingType::SpeechPacks => {}
    }

    // An explicit default must itself be a valid value.
    if kind.has_value() {
        if let Some(default) = &spec.default {
            check_value(spec, default)
                .map_err(|e| format!("setting '{key}' has an invalid default: {e}"))?;
        }
    }
    Ok(())
}

fn check_text_field(key: &str, field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.chars().count() > max {
        return Err(format!(
            "setting '{key}' {field} is longer than {max} characters"
        ));
    }
    if value.chars().any(|c| c.is_control()) {
        return Err(format!(
            "setting '{key}' {field} contains control characters"
        ));
    }
    Ok(())
}

/// The value-bearing type of a spec, or `None` for host widgets and unknown
/// types (which store nothing and never reach the plugin).
fn value_type(spec: &PluginSettingSpec) -> Option<PluginSettingType> {
    spec.known_type().filter(|k| k.has_value())
}

/// The effective default for a value-bearing setting.
pub fn default_value(spec: &PluginSettingSpec) -> Option<Value> {
    let kind = value_type(spec)?;
    if let Some(default) = &spec.default {
        if check_value(spec, default).is_ok() {
            return Some(default.clone());
        }
    }
    Some(match kind {
        PluginSettingType::Toggle => Value::Bool(false),
        PluginSettingType::Select => Value::String(spec.options.first()?.value.clone()),
        PluginSettingType::Text => Value::String(String::new()),
        PluginSettingType::Number => {
            let mut n = 0.0_f64;
            if let Some(min) = spec.min {
                n = n.max(min);
            }
            if let Some(max) = spec.max {
                n = n.min(max);
            }
            serde_json::Number::from_f64(n).map(Value::Number)?
        }
        PluginSettingType::SpeechPacks => return None,
    })
}

/// Check that `value` is valid for `spec`. Host widgets and unknown types
/// accept nothing.
pub fn check_value(spec: &PluginSettingSpec, value: &Value) -> Result<(), String> {
    let Some(kind) = value_type(spec) else {
        return Err(format!("setting '{}' does not store a value", spec.key));
    };
    match kind {
        PluginSettingType::Toggle => {
            if !value.is_boolean() {
                return Err("expected true or false".into());
            }
        }
        PluginSettingType::Select => {
            let Some(s) = value.as_str() else {
                return Err("expected one of the listed options".into());
            };
            if !spec.options.iter().any(|o| o.value == s) {
                return Err(format!("'{s}' is not one of the listed options"));
            }
        }
        PluginSettingType::Number => {
            let Some(n) = value.as_f64().filter(|n| n.is_finite()) else {
                return Err("expected a number".into());
            };
            if spec.min.is_some_and(|min| n < min) || spec.max.is_some_and(|max| n > max) {
                return Err(format!("{n} is outside the allowed range"));
            }
        }
        PluginSettingType::Text => {
            let Some(s) = value.as_str() else {
                return Err("expected text".into());
            };
            let max = spec.max_length.unwrap_or(DEFAULT_TEXT_MAX_LENGTH) as usize;
            if s.chars().count() > max {
                return Err(format!("text is longer than {max} characters"));
            }
        }
        PluginSettingType::SpeechPacks => unreachable!("filtered by value_type"),
    }
    Ok(())
}

/// `app_settings` key holding one plugin setting.
pub fn storage_key(manifest_id: &str, key: &str) -> String {
    format!("{STORAGE_PREFIX}{manifest_id}:{key}")
}

/// Every value-bearing setting with its current value: the stored value when
/// it still validates against the manifest, the default otherwise. Host
/// widgets and unknown types are left out.
pub fn get_settings(conn: &Connection, manifest: &PluginManifest) -> Map<String, Value> {
    let mut out = Map::new();
    for spec in &manifest.settings {
        let Some(default) = default_value(spec) else {
            continue;
        };
        let stored = read_stored(conn, &storage_key(&manifest.id, &spec.key))
            .filter(|v| check_value(spec, v).is_ok());
        out.insert(spec.key.clone(), stored.unwrap_or(default));
    }
    out
}

fn read_stored(conn: &Connection, storage_key: &str) -> Option<Value> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![storage_key],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or_else(|e| {
            log::warn!("plugin settings: failed to read {storage_key}: {e}");
            None
        });
    raw.and_then(|s| serde_json::from_str(&s).ok())
}

/// Store one setting after validating it against the manifest. Unknown keys,
/// host widgets and unknown types are refused.
pub fn set_setting(
    conn: &Connection,
    manifest: &PluginManifest,
    key: &str,
    value: &Value,
) -> Result<(), String> {
    let spec = manifest
        .settings
        .iter()
        .find(|s| s.key == key)
        .ok_or_else(|| format!("plugin has no setting '{key}'"))?;
    check_value(spec, value).map_err(|e| format!("invalid value for '{key}': {e}"))?;
    let encoded = serde_json::to_string(value).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO app_settings (key, value, scope, updated_at)
         VALUES (?1, ?2, 'device', datetime('now'))
         ON CONFLICT(key) DO UPDATE SET
             value      = excluded.value,
             scope      = 'device',
             updated_at = excluded.updated_at",
        params![storage_key(&manifest.id, key), encoded],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Remove every stored setting for this plugin id, restoring the defaults.
pub fn reset_settings(conn: &Connection, manifest: &PluginManifest) -> Result<(), String> {
    // Prefix compare rather than LIKE: plugin ids may contain `_`, a LIKE
    // wildcard.
    let prefix = format!("{STORAGE_PREFIX}{}:", manifest.id);
    conn.execute(
        "DELETE FROM app_settings WHERE substr(key, 1, length(?1)) = ?1",
        params![prefix],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// One setting as the global Settings search sees it.
#[derive(Debug, Clone, Serialize)]
pub struct PluginSettingsIndexSetting {
    pub key: String,
    pub label: String,
    pub description: Option<String>,
    pub keywords: Vec<String>,
}

/// One installed plugin as the global Settings search sees it.
#[derive(Debug, Clone, Serialize)]
pub struct PluginSettingsIndexEntry {
    pub plugin_cid: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_path: Option<String>,
    pub subject_tags: Vec<String>,
    pub enabled: bool,
    /// Settings the page renders (value-bearing types and host widgets);
    /// unknown types are left out.
    pub settings: Vec<PluginSettingsIndexSetting>,
}

/// Search index over every installed plugin and its renderable settings.
/// Plugins whose stored manifest no longer parses are skipped.
pub fn settings_index(db: &Database) -> Result<Vec<PluginSettingsIndexEntry>, String> {
    let mut out = Vec::new();
    for plugin in registry::list_installed(db)? {
        let Ok(m) = manifest::parse_and_validate(plugin.manifest_json.as_bytes()) else {
            continue;
        };
        out.push(PluginSettingsIndexEntry {
            plugin_cid: plugin.plugin_cid,
            name: plugin.name,
            description: m.description,
            icon_path: m.icon_path,
            subject_tags: m.subject_tags,
            enabled: plugin.enabled,
            settings: m
                .settings
                .into_iter()
                .filter(|s| s.known_type().is_some())
                .map(|s| PluginSettingsIndexSetting {
                    key: s.key,
                    label: s.label,
                    description: s.description,
                    keywords: s.keywords,
                })
                .collect(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DID: &str = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK";

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("db");
        db.run_migrations().expect("migrations");
        db
    }

    fn manifest_json(version: &str, settings: Value) -> String {
        json!({
            "id": format!("{DID}#language_lab"),
            "version": version,
            "api_version": "1",
            "host_min_version": "0.1.0",
            "name": "Language Lab",
            "description": "Practise speaking.",
            "author_did": DID,
            "kinds": ["interactive"],
            "subject_tags": ["english"],
            "settings": settings,
        })
        .to_string()
    }

    fn sample_settings() -> Value {
        json!([
            {"key": "ui_language", "type": "select", "label": "Instruction language",
             "description": "Language used for instructions.", "default": "hi",
             "options": [{"value": "hi", "label": "हिंदी (Hindi)"}, {"value": "en", "label": "English"}],
             "keywords": ["hindi"]},
            {"key": "speech_rate", "type": "number", "label": "Listening speed",
             "default": 0.9, "min": 0.5, "max": 1.5, "step": 0.1},
            {"key": "show_hints", "type": "toggle", "label": "Show Hindi hints", "default": true},
            {"key": "nickname", "type": "text", "label": "Nickname", "default": "", "max_length": 80},
            {"key": "pronunciation_packs", "type": "speech_packs", "label": "Pronunciation language packs",
             "keywords": ["pronunciation"]},
            {"key": "future_thing", "type": "colour_wheel", "label": "From a newer host"}
        ])
    }

    fn sample_manifest() -> PluginManifest {
        manifest::parse_and_validate(manifest_json("1.0.0", sample_settings()).as_bytes())
            .expect("sample manifest validates")
    }

    fn validate_with(settings: Value) -> Result<PluginManifest, String> {
        manifest::parse_and_validate(manifest_json("1.0.0", settings).as_bytes())
    }

    #[test]
    fn sample_settings_validate_and_unknown_types_parse() {
        let m = sample_manifest();
        assert_eq!(m.settings.len(), 6);
        assert!(m.settings[5].known_type().is_none());
    }

    #[test]
    fn manifests_without_settings_still_parse() {
        let mut value: Value = serde_json::from_str(&manifest_json("1.0.0", json!([]))).unwrap();
        value.as_object_mut().unwrap().remove("settings");
        let m = manifest::parse_and_validate(value.to_string().as_bytes()).unwrap();
        assert!(m.settings.is_empty());
    }

    #[test]
    fn rejects_bad_keys() {
        let long = "k".repeat(41);
        for key in ["", "Has-Caps", "spaced key", "dash-key", long.as_str()] {
            let err =
                validate_with(json!([{"key": key, "type": "toggle", "label": "X"}])).unwrap_err();
            assert!(err.contains("must match"), "{key}: {err}");
        }
        assert!(
            validate_with(json!([{"key": &"k".repeat(40), "type": "toggle", "label": "X"}]))
                .is_ok()
        );
    }

    #[test]
    fn rejects_duplicate_keys() {
        let err = validate_with(json!([
            {"key": "a", "type": "toggle", "label": "A"},
            {"key": "a", "type": "text", "label": "B"}
        ]))
        .unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
    }

    #[test]
    fn rejects_empty_label_and_oversized_strings() {
        assert!(validate_with(json!([{"key": "a", "type": "toggle", "label": "  "}])).is_err());
        assert!(
            validate_with(json!([{"key": "a", "type": "toggle", "label": "x".repeat(121)}]))
                .is_err()
        );
        assert!(validate_with(
            json!([{"key": "a", "type": "toggle", "label": "A", "description": "x".repeat(501)}])
        )
        .is_err());
        assert!(validate_with(
            json!([{"key": "a", "type": "toggle", "label": "A", "keywords": vec!["k"; 17]}])
        )
        .is_err());
        assert!(
            validate_with(json!([{"key": "a", "type": "toggle", "label": "A\u{0007}"}])).is_err()
        );
    }

    #[test]
    fn rejects_too_many_settings() {
        let many: Vec<Value> = (0..33)
            .map(|i| json!({"key": format!("k{i}"), "type": "toggle", "label": "T"}))
            .collect();
        let err = validate_with(Value::Array(many)).unwrap_err();
        assert!(err.contains("at most 32"), "{err}");
    }

    #[test]
    fn rejects_select_default_outside_options() {
        let err = validate_with(
            json!([{"key": "lang", "type": "select", "label": "L", "default": "fr",
            "options": [{"value": "hi", "label": "Hindi"}]}]),
        )
        .unwrap_err();
        assert!(err.contains("invalid default"), "{err}");
        assert!(validate_with(
            json!([{"key": "lang", "type": "select", "label": "L", "options": []}])
        )
        .is_err());
        assert!(
            validate_with(json!([{"key": "lang", "type": "select", "label": "L",
            "options": [{"value": "a", "label": "A"}, {"value": "a", "label": "B"}]}]))
            .is_err()
        );
    }

    #[test]
    fn rejects_number_default_outside_range_and_bad_bounds() {
        let err = validate_with(json!([{"key": "rate", "type": "number", "label": "R",
            "default": 2.0, "min": 0.5, "max": 1.5}]))
        .unwrap_err();
        assert!(err.contains("invalid default"), "{err}");
        assert!(validate_with(
            json!([{"key": "rate", "type": "number", "label": "R", "min": 2, "max": 1}])
        )
        .is_err());
        assert!(
            validate_with(json!([{"key": "rate", "type": "number", "label": "R", "step": 0}]))
                .is_err()
        );
    }

    #[test]
    fn rejects_text_default_longer_than_max_length() {
        assert!(
            validate_with(json!([{"key": "t", "type": "text", "label": "T",
            "default": "abcdef", "max_length": 5}]))
            .is_err()
        );
        assert!(validate_with(
            json!([{"key": "t", "type": "text", "label": "T", "max_length": 0}])
        )
        .is_err());
        assert!(validate_with(
            json!([{"key": "t", "type": "text", "label": "T", "max_length": 2001}])
        )
        .is_err());
    }

    #[test]
    fn get_fills_defaults_and_skips_widgets_and_unknown_types() {
        let db = test_db();
        let settings = get_settings(db.conn(), &sample_manifest());
        assert_eq!(
            Value::Object(settings),
            json!({"ui_language": "hi", "speech_rate": 0.9, "show_hints": true, "nickname": ""})
        );
    }

    #[test]
    fn implicit_defaults_follow_the_type() {
        let m = validate_with(json!([
            {"key": "t", "type": "toggle", "label": "T"},
            {"key": "s", "type": "select", "label": "S", "options": [{"value": "b", "label": "B"}, {"value": "a", "label": "A"}]},
            {"key": "n", "type": "number", "label": "N", "min": 3, "max": 9},
            {"key": "x", "type": "text", "label": "X"}
        ]))
        .unwrap();
        let settings = get_settings(test_db().conn(), &m);
        assert_eq!(
            Value::Object(settings),
            json!({"t": false, "s": "b", "n": 3.0, "x": ""})
        );
    }

    #[test]
    fn set_then_get_round_trips_and_reset_restores_defaults() {
        let db = test_db();
        let m = sample_manifest();
        set_setting(db.conn(), &m, "ui_language", &json!("en")).unwrap();
        set_setting(db.conn(), &m, "speech_rate", &json!(1.2)).unwrap();
        set_setting(db.conn(), &m, "show_hints", &json!(false)).unwrap();
        set_setting(db.conn(), &m, "nickname", &json!("Asha")).unwrap();
        let settings = get_settings(db.conn(), &m);
        assert_eq!(
            Value::Object(settings),
            json!({"ui_language": "en", "speech_rate": 1.2, "show_hints": false, "nickname": "Asha"})
        );

        let scope: String = db
            .conn()
            .query_row(
                "SELECT scope FROM app_settings WHERE key = ?1",
                params![storage_key(&m.id, "nickname")],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(scope, "device");

        reset_settings(db.conn(), &m).unwrap();
        assert_eq!(
            Value::Object(get_settings(db.conn(), &m)),
            json!({"ui_language": "hi", "speech_rate": 0.9, "show_hints": true, "nickname": ""})
        );
    }

    #[test]
    fn reset_leaves_other_plugins_and_app_settings_alone() {
        let db = test_db();
        let m = sample_manifest();
        let other = manifest::parse_and_validate(
            manifest_json("1.0.0", sample_settings())
                .replace("#language_lab", "#language_lab_2")
                .as_bytes(),
        )
        .unwrap();
        set_setting(db.conn(), &m, "nickname", &json!("A")).unwrap();
        set_setting(db.conn(), &other, "nickname", &json!("B")).unwrap();
        crate::settings::SettingsStore::set_raw(db.conn(), "ui.theme", "dark").unwrap();

        reset_settings(db.conn(), &m).unwrap();
        assert_eq!(get_settings(db.conn(), &m)["nickname"], json!(""));
        assert_eq!(get_settings(db.conn(), &other)["nickname"], json!("B"));
        assert_eq!(
            crate::settings::SettingsStore::get(
                db.conn(),
                crate::settings::registry::keys::UI_THEME
            ),
            "dark"
        );
    }

    #[test]
    fn set_rejects_unknown_keys_widgets_unknown_types_and_bad_values() {
        let db = test_db();
        let m = sample_manifest();
        assert!(set_setting(db.conn(), &m, "nope", &json!(true)).is_err());
        assert!(set_setting(db.conn(), &m, "pronunciation_packs", &json!(true)).is_err());
        assert!(set_setting(db.conn(), &m, "future_thing", &json!("x")).is_err());
        assert!(set_setting(db.conn(), &m, "ui_language", &json!("fr")).is_err());
        assert!(set_setting(db.conn(), &m, "ui_language", &json!(1)).is_err());
        assert!(set_setting(db.conn(), &m, "speech_rate", &json!(3)).is_err());
        assert!(set_setting(db.conn(), &m, "speech_rate", &json!("1.0")).is_err());
        assert!(set_setting(db.conn(), &m, "show_hints", &json!("yes")).is_err());
        assert!(set_setting(db.conn(), &m, "nickname", &json!("x".repeat(81))).is_err());
    }

    #[test]
    fn values_survive_an_upgrade_and_invalid_ones_fall_back() {
        let db = test_db();
        let v1 = sample_manifest();
        set_setting(db.conn(), &v1, "ui_language", &json!("en")).unwrap();
        set_setting(db.conn(), &v1, "speech_rate", &json!(1.4)).unwrap();

        // v2 keeps the id but drops the English option and narrows the range.
        let mut settings = sample_settings();
        settings[0]["options"] =
            json!([{"value": "hi", "label": "Hindi"}, {"value": "mr", "label": "Marathi"}]);
        settings[1]["max"] = json!(1.5);
        let v2 = manifest::parse_and_validate(manifest_json("2.0.0", settings).as_bytes()).unwrap();
        let got = get_settings(db.conn(), &v2);
        assert_eq!(got["ui_language"], json!("hi"), "removed option falls back");
        assert_eq!(
            got["speech_rate"],
            json!(1.4),
            "still-valid value carries over"
        );

        // A corrupt row also falls back.
        db.conn()
            .execute(
                "UPDATE app_settings SET value = 'not json' WHERE key = ?1",
                params![storage_key(&v2.id, "speech_rate")],
            )
            .unwrap();
        assert_eq!(get_settings(db.conn(), &v2)["speech_rate"], json!(0.9));
    }

    #[test]
    fn index_lists_installed_plugins_with_renderable_settings() {
        let db = test_db();
        db.conn()
            .execute(
                "INSERT INTO plugin_installed (plugin_cid, name, version, author_did, install_path, source, manifest_json) \
                 VALUES ('cid1', 'Language Lab', '1.0.0', ?1, '/tmp/x', 'builtin', ?2)",
                params![DID, manifest_json("1.0.0", sample_settings())],
            )
            .unwrap();
        let index = settings_index(&db).unwrap();
        assert_eq!(index.len(), 1);
        let entry = &index[0];
        assert_eq!(entry.plugin_cid, "cid1");
        assert_eq!(entry.subject_tags, vec!["english".to_string()]);
        let keys: Vec<&str> = entry.settings.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "ui_language",
                "speech_rate",
                "show_hints",
                "nickname",
                "pronunciation_packs"
            ]
        );
        assert_eq!(entry.settings[0].keywords, vec!["hindi".to_string()]);
    }
}
