# App Settings

**Status:** Implemented on `feat/synced-settings`
**Date:** 2026-05-19

Every user-controlled preference Alexandria has — theme, sidebar
collapsed, keyboard shortcuts, sentinel toggles, video defaults,
storage quota, window geometry — lives in one place: the per-profile
`app_settings` table. The store has two scopes:

- **`sync`** — replicated to the user's other **explicitly paired**
  devices. Pairing is a one-time exchange of a pairing code that
  carries a 32-byte shared key; sync payloads are then AES-256-GCM
  sealed over the `/alexandria/sync/1.0` request-response protocol and
  merged last-write-wins on `updated_at`. Default for new keys.
- **`device`** — stays on this device only. Use sparingly: storage
  quota, window geometry, per-device API key overrides.

## How it works

```
┌──────────────────────────────────────────────────────────────┐
│  Frontend                                                    │
│  ┌────────────────┐  ┌────────────────┐                      │
│  │ useSettings()  │  │ useSetting<T>()│                      │
│  │  - entries[]   │  │  - reactive    │                      │
│  │  - setSetting  │  │    Ref<T>      │                      │
│  └───────┬────────┘  └───────┬────────┘                      │
│          │ IPC               │                               │
│          ▼                   ▼                               │
└──────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│  Rust backend (commands/settings.rs)                         │
│   list_settings | set_setting | reset_setting                │
│          │                                                   │
│          ▼                                                   │
│   settings::SettingsStore (registry-validated R/W)           │
│          │                                                   │
│          ▼                                                   │
│   app_settings(key, value, scope, updated_at)                │
│          │                                                   │
│          │ scope='sync' rows only                            │
│          ▼                                                   │
│   p2p::sync::settings_outbound_snapshot  ───►  Other devices │
│   p2p::sync::settings_apply_inbound      ◄───   (LWW merge)  │
└──────────────────────────────────────────────────────────────┘
```

The typed registry at `src-tauri/src/settings/registry.rs` is the
**single source of truth** for valid keys. The frontend never owns a
list of keys — it mirrors the registry via `list_settings` and
listens for `settings-changed` events so multiple windows + sync
deliveries stay coherent.

## Adding a new setting

Add one line to `keys` in `src-tauri/src/settings/registry.rs`:

```rust
pub const NOTIFICATIONS_SOUND: SettingKey<bool> = SettingKey {
    key: "notifications.sound",
    scope: Scope::Sync,
    category: "Notifications",
    label: "Play sound on notification",
    description: "When enabled, in-app notifications include a soft chime.",
    default: || true,
};
```

Then add it to `all_entries()` and `lookup_meta()` in the same file
(both are exhaustive macros against the keys list — a CI check could
be added, but for now they are reviewed manually).

That's it:

- The setting appears in the "All settings" section of the settings
  page automatically, with the right widget (toggle / textbox /
  number / JSON viewer based on `T::kind()`).
- The backend gains a typed accessor: `SettingsStore::get(conn,
  keys::NOTIFICATIONS_SOUND)`.
- The frontend gains a reactive accessor: `useSetting<boolean>('notifications.sound')`.
- The setting propagates to the user's other devices (if scope is `sync`).

## Where settings used to live

Before this work, preferences were scattered:

| Old home | Keys | Problems |
|---|---|---|
| `localStorage` | theme, sidebar collapsed, sidebar sections, keyboard shortcuts, omni recents, sentinel AI / paste toggles | Per-device only, lost on profile switch, never synced |
| `app_settings` table (ad hoc) | `storage_quota_bytes` | One-off SQL, no schema |
| Hardcoded seed | `theme`, `language`, `notifications_enabled`, `auto_sync`, `sentinel_camera_enabled`, `sentinel_keyboard_enabled` | Written by `db/seed.rs` but never read |
| Module-level refs | video volume / mute | Lost on remount |
| Env vars | `BLOCKFROST_PROJECT_ID`, `ALEXANDRIA_COMPLETION_POLICY_ID`, `ALEXANDRIA_DEVICE_LABEL` | Process-wide fallback. `BLOCKFROST_PROJECT_ID` is now overridden by the per-device `cardano.blockfrost_project_id` setting (resolved via `cardano::blockfrost::resolve_project_id`); the env var stays as a CI / dev-script convenience. |

All of these now live in one table, with one API, with sync.

## What does **not** belong here

- **Per-session ephemera**: open dropdown flags, the active settings
  section (now a route param — `/settings/:section?`), page filter chips
  (`courses-kind-filter`, `opinions-field-filter`). Keep these in
  component-local refs, the route, or `sessionStorage`.
- **Per-element drafts**: e.g. `essay_draft_${elementId}`. These are
  content-scoped, not user-preference, and may eventually move into a
  dedicated `drafts` table.
- **Secrets**: vault passwords, mnemonics, API tokens that the user
  could not safely share with another device. `cardano.blockfrost_project_id`
  is a borderline case — declared as `Scope::Device` so it does *not*
  sync.

## Sync semantics

- Only rows with `scope='sync'` are eligible for replication.
- Conflict resolution: last-write-wins on `updated_at` (the column is
  populated server-side via `datetime('now')` so clocks are local;
  callers do not control timestamps).
- The receiver refuses inbound rows for unknown keys (forward-compat:
  a peer running a newer build can include settings we don't know
  about, and we ignore them).
- The receiver refuses inbound rows for keys whose registered scope
  is `Device`. A peer cannot smuggle device-scoped settings.

## Code references

- Migration `048_app_settings_scope` — `src-tauri/src/db/schema.rs`
- Registry — `src-tauri/src/settings/registry.rs`
- Store — `src-tauri/src/settings/store.rs`
- IPC commands — `src-tauri/src/commands/settings.rs`
- Sync helpers — `src-tauri/src/p2p/sync.rs` (`settings_outbound_snapshot`,
  `settings_apply_inbound`, `SettingsSyncRow`)
- Frontend composable — `src/composables/useSettings.ts`
- Frontend hydration hooks (called after profile unlock) —
  `useTheme.initThemeFromSettings`, `useKeyboardShortcuts.initShortcutsFromSettings`,
  `useOmniSearch.initOmniRecentsFromSettings`, `useSentinel.initSentinelFlagsFromSettings`
- "All settings" panel — `src/components/settings/AdvancedSettingsPanel.vue`
