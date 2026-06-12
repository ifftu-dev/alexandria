//! Typed registry of every user setting.
//!
//! Add a new setting in [`keys`] below. The registry drives the
//! settings panel UI, the typed accessors, and the sync filter.

use serde::{Deserialize, Serialize};

/// Whether a setting propagates to the user's other devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Replicated across every device of the same user (LWW on `updated_at`).
    Sync,
    /// Stays on this device only.
    Device,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Sync => "sync",
            Scope::Device => "device",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "device" => Scope::Device,
            _ => Scope::Sync,
        }
    }
}

/// Settings value that can round-trip through a TEXT column.
///
/// All variants serialize to a single string for storage. The
/// `kind()` discriminator drives the picker UI on the frontend
/// (toggle / textbox / number stepper / JSON editor).
pub trait SettingValue: Sized + Clone {
    fn to_setting_string(&self) -> String;
    fn from_setting_string(s: &str) -> Option<Self>;
    /// One of: `"bool"`, `"int"`, `"float"`, `"string"`, `"json"`.
    fn kind() -> &'static str;
}

impl SettingValue for bool {
    fn to_setting_string(&self) -> String {
        if *self {
            "true".into()
        } else {
            "false".into()
        }
    }
    fn from_setting_string(s: &str) -> Option<Self> {
        match s {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        }
    }
    fn kind() -> &'static str {
        "bool"
    }
}

impl SettingValue for i64 {
    fn to_setting_string(&self) -> String {
        self.to_string()
    }
    fn from_setting_string(s: &str) -> Option<Self> {
        s.parse().ok()
    }
    fn kind() -> &'static str {
        "int"
    }
}

impl SettingValue for u64 {
    fn to_setting_string(&self) -> String {
        self.to_string()
    }
    fn from_setting_string(s: &str) -> Option<Self> {
        s.parse().ok()
    }
    fn kind() -> &'static str {
        "int"
    }
}

impl SettingValue for f64 {
    fn to_setting_string(&self) -> String {
        self.to_string()
    }
    fn from_setting_string(s: &str) -> Option<Self> {
        s.parse().ok()
    }
    fn kind() -> &'static str {
        "float"
    }
}

impl SettingValue for String {
    fn to_setting_string(&self) -> String {
        self.clone()
    }
    fn from_setting_string(s: &str) -> Option<Self> {
        Some(s.to_string())
    }
    fn kind() -> &'static str {
        "string"
    }
}

/// JSON-typed setting backed by `serde_json::Value`. Useful for
/// keyboard-shortcut maps, omni-search recents, etc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonSetting(pub serde_json::Value);

impl SettingValue for JsonSetting {
    fn to_setting_string(&self) -> String {
        self.0.to_string()
    }
    fn from_setting_string(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok().map(JsonSetting)
    }
    fn kind() -> &'static str {
        "json"
    }
}

/// Compile-time declaration for one setting key.
///
/// Generic over the runtime value type. Listed centrally in
/// [`keys`] so the registry can be walked at startup.
#[derive(Debug, Clone, Copy)]
pub struct SettingKey<T: SettingValue + 'static> {
    pub key: &'static str,
    pub scope: Scope,
    pub category: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// Function pointer to the default value — closures cannot be
    /// stored in `const`s.
    pub default: fn() -> T,
}

/// Type-erased view of a [`SettingKey`] for runtime enumeration.
#[derive(Debug, Clone, Serialize)]
pub struct SettingEntry {
    pub key: &'static str,
    pub scope: Scope,
    pub category: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// `"bool" | "int" | "float" | "string" | "json"`.
    pub kind: &'static str,
    /// Default value, already string-encoded for the wire.
    pub default_value: String,
    /// Currently-effective value (the user's override if present,
    /// otherwise `default_value`).
    pub current_value: String,
    /// `true` if the row is absent from `app_settings` and the user
    /// is still seeing the registry default.
    pub is_default: bool,
}

/// Every setting the app supports. Adding a new line here is the
/// only step required to expose a new user-tunable preference.
pub mod keys {
    use super::{JsonSetting, Scope, SettingKey};

    // ── UI / appearance ────────────────────────────────────────
    pub const UI_THEME: SettingKey<String> = SettingKey {
        key: "ui.theme",
        scope: Scope::Sync,
        category: "Appearance",
        label: "Theme",
        description: "Light, dark, or follow the operating system.",
        default: || "system".to_string(),
    };

    pub const UI_SIDEBAR_COLLAPSED: SettingKey<bool> = SettingKey {
        key: "ui.sidebar_collapsed",
        scope: Scope::Sync,
        category: "Appearance",
        label: "Collapse navigation sidebar",
        description: "When enabled, the main sidebar starts collapsed on launch.",
        default: || false,
    };

    pub const UI_SIDEBAR_SECTIONS: SettingKey<JsonSetting> = SettingKey {
        key: "ui.sidebar_sections",
        scope: Scope::Sync,
        category: "Appearance",
        label: "Sidebar section expand state",
        description: "Which sidebar sections (tutoring, classrooms, ...) are expanded.",
        default: || JsonSetting(serde_json::json!({})),
    };

    // ── Input ──────────────────────────────────────────────────
    pub const UI_KEYBOARD_SHORTCUTS: SettingKey<JsonSetting> = SettingKey {
        key: "input.keyboard_shortcuts",
        scope: Scope::Sync,
        category: "Input",
        label: "Custom keyboard shortcuts",
        description: "User-overridden bindings, keyed by action id.",
        default: || JsonSetting(serde_json::json!({})),
    };

    // ── Search ─────────────────────────────────────────────────
    pub const UI_OMNI_RECENTS: SettingKey<JsonSetting> = SettingKey {
        key: "ui.omni_recents",
        scope: Scope::Sync,
        category: "Search",
        label: "Omni-search recent queries",
        description: "Recently used search terms shown when the omni-search opens.",
        default: || JsonSetting(serde_json::json!([])),
    };

    // ── Sentinel (integrity ML) ────────────────────────────────
    pub const SENTINEL_AI_SCORING: SettingKey<bool> = SettingKey {
        key: "sentinel.ai_scoring_enabled",
        scope: Scope::Sync,
        category: "Integrity",
        label: "Enable Sentinel AI scoring",
        description: "Run the per-user keystroke + mouse models during sessions.",
        default: || true,
    };

    pub const SENTINEL_PASTE_CLASSIFIER: SettingKey<bool> = SettingKey {
        key: "sentinel.paste_classifier_enabled",
        scope: Scope::Sync,
        category: "Integrity",
        label: "Enable paste classifier",
        description: "Run the bundled tract ONNX paste classifier on text input.",
        default: || true,
    };

    pub const SENTINEL_CAMERA_ENABLED: SettingKey<bool> = SettingKey {
        key: "sentinel.camera_enabled",
        scope: Scope::Sync,
        category: "Integrity",
        label: "Allow webcam access during assessments",
        description: "If disabled, Sentinel skips face-embedding signals.",
        default: || true,
    };

    pub const SENTINEL_KEYBOARD_ENABLED: SettingKey<bool> = SettingKey {
        key: "sentinel.keyboard_enabled",
        scope: Scope::Sync,
        category: "Integrity",
        label: "Allow keystroke capture during assessments",
        description: "If disabled, Sentinel skips per-user keystroke autoencoder scoring.",
        default: || true,
    };

    // ── Notifications / sync / locale ──────────────────────────
    pub const NOTIFICATIONS_ENABLED: SettingKey<bool> = SettingKey {
        key: "notifications.enabled",
        scope: Scope::Sync,
        category: "Notifications",
        label: "Show in-app notifications",
        description: "Surface badges and toasts for new credentials, classroom invites, etc.",
        default: || true,
    };

    pub const SYNC_AUTO: SettingKey<bool> = SettingKey {
        key: "sync.auto",
        scope: Scope::Sync,
        category: "Sync",
        label: "Automatic cross-device sync",
        description: "If disabled, the user must trigger sync manually.",
        default: || true,
    };

    pub const USER_LANGUAGE: SettingKey<String> = SettingKey {
        key: "user.language",
        scope: Scope::Sync,
        category: "Locale",
        label: "Display language",
        description: "BCP-47 language tag (e.g. `en`, `pt-BR`).",
        default: || "en".to_string(),
    };

    // ── Video playback defaults ────────────────────────────────
    pub const VIDEO_DEFAULT_VOLUME: SettingKey<f64> = SettingKey {
        key: "video.default_volume",
        scope: Scope::Sync,
        category: "Video",
        label: "Default playback volume",
        description: "Initial volume (0.0–1.0) applied when a video element mounts.",
        default: || 1.0,
    };

    pub const VIDEO_DEFAULT_MUTED: SettingKey<bool> = SettingKey {
        key: "video.default_muted",
        scope: Scope::Sync,
        category: "Video",
        label: "Start videos muted",
        description: "When enabled, new video elements start muted by default.",
        default: || false,
    };

    // ── Cardano integration overrides ──────────────────────────
    pub const CARDANO_BLOCKFROST_KEY: SettingKey<String> = SettingKey {
        key: "cardano.blockfrost_project_id",
        scope: Scope::Device,
        category: "Cardano",
        label: "Blockfrost project id",
        description:
            "Optional per-device Blockfrost API key. When set, overrides the BLOCKFROST_PROJECT_ID env var.",
        default: || String::new(),
    };

    pub const CARDANO_COMPLETION_POLICY: SettingKey<String> = SettingKey {
        key: "cardano.completion_policy_id",
        scope: Scope::Device,
        category: "Cardano",
        label: "Completion validator policy id",
        description:
            "Cardano policy id observed by the auto-issuance observer. Empty disables the observer.",
        default: || String::new(),
    };

    pub const REGISTRY_REFRESH_SECS: SettingKey<u64> = SettingKey {
        key: "registry.refresh_secs",
        scope: Scope::Device,
        category: "Cardano",
        label: "Stake-pubkey registry refresh interval (seconds)",
        description: "Cadence at which the stake-pubkey registry reconciles against on-chain \
             stake_pubkey_registration UTxOs. Clamped up to 60s at runtime to avoid \
             hammering Blockfrost. Default 3600 (1 hour).",
        default: || crate::p2p::registry_chain::DEFAULT_REFRESH_SECS,
    };

    // ── Per-device ────────────────────────────────────────────
    pub const DEVICE_LABEL: SettingKey<String> = SettingKey {
        key: "device.label",
        scope: Scope::Device,
        category: "Device",
        label: "Device label",
        description: "Friendly name shown for this device in cross-device sync.",
        default: || String::new(),
    };

    pub const STORAGE_QUOTA_BYTES: SettingKey<u64> = SettingKey {
        key: "storage.quota_bytes",
        scope: Scope::Device,
        category: "Storage",
        label: "Local content quota (bytes)",
        description: "Maximum bytes Alexandria may pin on this device. 0 means unlimited.",
        default: || 0,
    };

    pub const WINDOW_GEOMETRY: SettingKey<JsonSetting> = SettingKey {
        key: "ui.window_geometry",
        scope: Scope::Device,
        category: "Window",
        label: "Window position and size",
        description: "Persisted across launches as {x, y, width, height} JSON.",
        default: || JsonSetting(serde_json::json!(null)),
    };

    // ── Skill graph / reputation ───────────────────────────────
    /// Per-skill visibility + teaching preferences for the owner's
    /// skill graph. Shape: `{ "<skill_id>": { "public": bool,
    /// "teaching": bool } }`. Earned skills are public by default; an
    /// absent entry means public + not-teaching.
    pub const INSTRUCTOR_GRAPH_PREFS: SettingKey<JsonSetting> = SettingKey {
        key: "instructor.graph_prefs",
        scope: Scope::Sync,
        category: "Skill graph",
        label: "Skill graph visibility",
        description: "Which earned skills are public and which you teach, keyed by skill id.",
        default: || JsonSetting(serde_json::json!({})),
    };

    /// The learner's targeted skill graphs. Shape: array of
    /// `{ id, label, source_did?, goal_skill_ids: [..], created_at }`.
    pub const LEARNER_TARGETS: SettingKey<JsonSetting> = SettingKey {
        key: "learner.targets",
        scope: Scope::Sync,
        category: "Skill graph",
        label: "Learning targets",
        description: "Skill graphs you are working toward, with their goal skills.",
        default: || JsonSetting(serde_json::json!([])),
    };

    /// Additional relay nodes (federation): `[{ peer_id, host, port }]`.
    /// Merged into bootstrap, circuit, receipt, and availability
    /// surfaces at p2p start. Device-scoped — relay trust is a
    /// per-device decision.
    pub const P2P_EXTRA_RELAYS: SettingKey<JsonSetting> = SettingKey {
        key: "p2p.extra_relays",
        scope: Scope::Device,
        category: "Network",
        label: "Additional relay nodes",
        description: "Extra alexandria-relay servers to bootstrap through, as JSON: [{\"peer_id\", \"host\", \"port\"}].",
        default: || JsonSetting(serde_json::json!([])),
    };

    /// Desktop-only: serve the Kademlia DHT (server mode + persistent
    /// record mirror). Mobile is always a pure client — backgrounded
    /// apps churn the routing table and degrade everyone's lookups.
    pub const P2P_DHT_SERVER: SettingKey<bool> = SettingKey {
        key: "p2p.dht_server",
        scope: Scope::Device,
        category: "Network",
        label: "Serve the DHT (desktop only)",
        description: "Run Kademlia in server mode and persist records locally, strengthening the network's storage layer.",
        default: || false,
    };

    /// Device-local cache of the active profile's `did:key`. Written by
    /// `get_local_did` so the swarm event loop (which has no keystore
    /// access) can answer graph-fetch requests for its own owner.
    pub const IDENTITY_LOCAL_DID: SettingKey<String> = SettingKey {
        key: "identity.local_did",
        scope: Scope::Device,
        category: "Identity",
        label: "Local DID cache",
        description: "Cached did:key of the active profile (internal).",
        default: String::new,
    };
}

/// Walk every declared setting and produce its type-erased entry.
/// The order is the declaration order in [`keys`].
pub fn all_entries(
    overrides: &std::collections::HashMap<String, (String, super::Scope)>,
) -> Vec<SettingEntry> {
    use keys::*;

    macro_rules! entry {
        ($key:expr) => {{
            let k = $key;
            let default_value = (k.default)().to_setting_string();
            let (current_value, is_default, scope) = match overrides.get(k.key) {
                Some((v, s)) => (v.clone(), false, *s),
                None => (default_value.clone(), true, k.scope),
            };
            SettingEntry {
                key: k.key,
                scope,
                category: k.category,
                label: k.label,
                description: k.description,
                kind: kind_of_default(&k),
                default_value,
                current_value,
                is_default,
            }
        }};
    }

    vec![
        entry!(UI_THEME),
        entry!(UI_SIDEBAR_COLLAPSED),
        entry!(UI_SIDEBAR_SECTIONS),
        entry!(UI_KEYBOARD_SHORTCUTS),
        entry!(UI_OMNI_RECENTS),
        entry!(SENTINEL_AI_SCORING),
        entry!(SENTINEL_PASTE_CLASSIFIER),
        entry!(SENTINEL_CAMERA_ENABLED),
        entry!(SENTINEL_KEYBOARD_ENABLED),
        entry!(NOTIFICATIONS_ENABLED),
        entry!(SYNC_AUTO),
        entry!(USER_LANGUAGE),
        entry!(VIDEO_DEFAULT_VOLUME),
        entry!(VIDEO_DEFAULT_MUTED),
        entry!(CARDANO_BLOCKFROST_KEY),
        entry!(CARDANO_COMPLETION_POLICY),
        entry!(REGISTRY_REFRESH_SECS),
        entry!(DEVICE_LABEL),
        entry!(STORAGE_QUOTA_BYTES),
        entry!(WINDOW_GEOMETRY),
        entry!(P2P_EXTRA_RELAYS),
        entry!(P2P_DHT_SERVER),
        entry!(INSTRUCTOR_GRAPH_PREFS),
        entry!(LEARNER_TARGETS),
        entry!(IDENTITY_LOCAL_DID),
    ]
}

/// Lookup the registered scope + kind for a key, used by the
/// `set_setting` IPC to refuse unknown keys (so a stale frontend
/// cannot smuggle arbitrary rows into `app_settings`).
pub fn lookup_meta(key: &str) -> Option<(Scope, &'static str)> {
    use keys::*;
    macro_rules! check {
        ($k:expr) => {
            if $k.key == key {
                return Some(($k.scope, kind_of_default(&$k)));
            }
        };
    }
    check!(UI_THEME);
    check!(UI_SIDEBAR_COLLAPSED);
    check!(UI_SIDEBAR_SECTIONS);
    check!(UI_KEYBOARD_SHORTCUTS);
    check!(UI_OMNI_RECENTS);
    check!(SENTINEL_AI_SCORING);
    check!(SENTINEL_PASTE_CLASSIFIER);
    check!(SENTINEL_CAMERA_ENABLED);
    check!(SENTINEL_KEYBOARD_ENABLED);
    check!(NOTIFICATIONS_ENABLED);
    check!(SYNC_AUTO);
    check!(USER_LANGUAGE);
    check!(VIDEO_DEFAULT_VOLUME);
    check!(VIDEO_DEFAULT_MUTED);
    check!(CARDANO_BLOCKFROST_KEY);
    check!(CARDANO_COMPLETION_POLICY);
    check!(REGISTRY_REFRESH_SECS);
    check!(DEVICE_LABEL);
    check!(STORAGE_QUOTA_BYTES);
    check!(WINDOW_GEOMETRY);
    check!(P2P_EXTRA_RELAYS);
    check!(P2P_DHT_SERVER);
    check!(INSTRUCTOR_GRAPH_PREFS);
    check!(LEARNER_TARGETS);
    check!(IDENTITY_LOCAL_DID);
    None
}

fn kind_of_default<T: SettingValue + 'static>(_: &SettingKey<T>) -> &'static str {
    T::kind()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_round_trip() {
        assert_eq!(bool::from_setting_string("true"), Some(true));
        assert_eq!(bool::from_setting_string("false"), Some(false));
        assert_eq!(true.to_setting_string(), "true");
    }

    #[test]
    fn scope_round_trip() {
        assert_eq!(Scope::parse("sync"), Scope::Sync);
        assert_eq!(Scope::parse("device"), Scope::Device);
        assert_eq!(Scope::parse("nonsense"), Scope::Sync);
    }

    #[test]
    fn json_round_trip() {
        let v = JsonSetting(serde_json::json!({"a": 1, "b": [2, 3]}));
        let s = v.to_setting_string();
        let back = JsonSetting::from_setting_string(&s).unwrap();
        assert_eq!(back.0, v.0);
    }

    #[test]
    fn all_entries_includes_every_registered_key() {
        let overrides = std::collections::HashMap::new();
        let entries = all_entries(&overrides);
        assert!(entries.iter().any(|e| e.key == "ui.theme"));
        assert!(entries
            .iter()
            .any(|e| e.key == "sentinel.ai_scoring_enabled"));
        assert!(entries.iter().any(|e| e.key == "storage.quota_bytes"));
        // Default values surface even with no overrides.
        let theme = entries.iter().find(|e| e.key == "ui.theme").unwrap();
        assert_eq!(theme.current_value, "system");
        assert!(theme.is_default);
    }

    #[test]
    fn lookup_unknown_key_returns_none() {
        assert!(lookup_meta("totally.made_up").is_none());
        assert!(lookup_meta("ui.theme").is_some());
    }
}
