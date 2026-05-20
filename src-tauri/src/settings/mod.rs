//! Unified per-profile user settings.
//!
//! Every user-controlled preference (theme, sidebar collapsed,
//! keyboard shortcuts, sentinel toggles, video defaults, window
//! geometry, etc.) lives in the `app_settings` table of the active
//! profile's SQLCipher database. The schema is dirt-simple:
//!
//! ```text
//! app_settings(key TEXT PRIMARY KEY, value TEXT, scope TEXT, updated_at TEXT)
//! ```
//!
//! What makes this useful is the typed [`registry`]: every setting
//! is declared once at compile time with its scope, default,
//! category, and human-readable label. Reading and writing settings
//! goes through that registry, which means:
//!
//! * The frontend never needs to mirror string literals — it
//!   discovers settings via [`commands::settings::list_settings`].
//! * Adding a new setting is one line in [`registry::keys`].
//! * Defaults live in code, not seed SQL, so removing a setting
//!   does not require a destructive migration.
//!
//! Two scopes:
//!
//! * **`Sync`** — propagated across every device of the same user
//!   via the existing cross-device sync (see `p2p::sync`). Last-write-wins
//!   on `updated_at`. Default for new keys.
//! * **`Device`** — stays on this device only. Use sparingly: window
//!   geometry, device label, per-device disk quota.
//!
//! Per-session ephemeral state (filter chips, modal open flags)
//! does NOT belong here — keep those in component-local refs.

pub mod registry;
pub mod store;

pub use registry::{Scope, SettingEntry, SettingKey, SettingValue};
pub use store::SettingsStore;
