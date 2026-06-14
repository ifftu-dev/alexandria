# Multi-User Profiles

**Status:** Implemented on `feat/multi-user-profiles` (backend + frontend landed) · **Date:** 2026-05-19

## Motivation

Alexandria's mission is universal access to education, including for learners in regions where one phone or one laptop is shared by a household, a classroom, or an internet café. The current single-vault architecture forces each device to belong to one identity: switching learners would require wiping the vault and starting over, destroying enrollments, credentials, and reputation.

Multi-user accounts let one device host several learners, each with their own credentials, progress, and reputation — fully isolated cryptographically, fully isolated at rest, and switchable without loss.

## Goals

- One device, many learners — no practical cap.
- A profile picker that is the first thing the user sees once any profile exists.
- Full isolation of identity, vault, database, content cache, and peer ID per profile.
- Painless switch: lock current, pick another, unlock with that profile's password.
- Auto-migrate the existing single-vault layout on first launch.
- Beautiful and clean UI — avatar tiles, large hit targets, friendly enough for a child.

## Non-Goals (this RFC)

- Remote push notifications (covered in [push-notifications-rfc.md](./push-notifications-rfc.md)).
- Deep linking (covered in a follow-up RFC; multi-profile must land first because deep links queue until a profile is unlocked).
- Cross-profile content deduplication. Each profile keeps its own iroh blob cache for now; a shared read-only blob pool can come later.
- Per-profile parental controls / content filtering.

> **Update:** biometric unlock at the picker has since shipped (see _Security
> properties_ below). Selecting a profile offers Touch ID / Face ID, with the
> vault password held in the OS keychain keyed per profile. Fully
> password-free switching across _every_ profile still depends on each having
> enrolled biometrics on the device.

## Architecture

### Per-profile data layout

```
<app_data>/
  profiles/
    <profile-uuid-1>/
      vault/           # Stronghold (desktop) or AES-GCM portable vault (mobile)
      alexandria.db    # SQLCipher, password-derived key
      iroh/            # iroh blob store, per-profile peer id
      plugins/         # installed plugin bundles
      videocache/      # materialized video files for asset:// protocol
    <profile-uuid-2>/
      ...
  profiles_index.json  # PUBLIC sidecar; rendered without unlocking anything
```

### `profiles_index.json` (public metadata)

```json
{
  "version": 1,
  "profiles": [
    {
      "id": "0f5a…",
      "display_name": "Pratyush",
      "avatar": { "kind": "emoji", "value": "🦊" },
      "color": "#7c3aed",
      "created_at": "2026-05-18T09:21:00Z",
      "last_unlocked_at": "2026-05-18T14:02:14Z"
    }
  ]
}
```

- **Public** because the picker must render before any vault is unlocked.
- Holds only what's needed for the tile: display name, avatar (emoji or local image hash), accent color, timestamps.
- Never contains stake addresses, DIDs, keys, or anything cryptographically linked to the user's chain identity. Treat it as user-public listing data.

### Usernames are mandatory

Every profile must have a non-empty `display_name`. This is enforced at
both layers:

- **Backend** (`commands/profile.rs`, `commands/identity.rs`):
  `create_profile`, `restore_profile_with_mnemonic`, `rename_profile`, and
  `update_profile` trim and reject an empty name.
- **Frontend**: onboarding blocks advancing without a username (no silent
  "My Profile" default), and Settings → Account refuses to save a blank
  name.

The username is the primary human label for an identity everywhere in the
app. DIDs (`did:key:…`) are never shown as the primary identifier — the
`resolve_display_names` IPC + `useDisplayNames` composable map a DID to a
username (own DID → your display name; built-in plugin authors →
"Alexandria"; unknown DIDs → a short-DID fallback). Used across plugins,
credentials, IRL Review, and other DID-bearing surfaces.

### `ActiveProfile` (in `AppState`)

```rust
pub struct ActiveProfile {
    pub id:           ProfileId,
    pub root:         PathBuf,        // profiles/<id>/
    pub vault_dir:    PathBuf,        // root/vault
    pub db_path:      PathBuf,        // root/alexandria.db
    pub plugins_dir:  PathBuf,        // root/plugins
    pub video_cache:  PathBuf,        // root/videocache
    pub iroh_dir:     PathBuf,        // root/iroh
    pub keystore:     Keystore,
    pub db:           Database,
    pub content_node: Arc<ContentNode>,
    pub p2p_node:     Arc<Mutex<Option<P2pNode>>>,
}

pub struct AppState {
    pub profile_manager: Arc<ProfileManager>,           // always available
    pub active: Arc<RwLock<Option<ActiveProfile>>>,     // None until unlock
    pub last_activity: Arc<Mutex<Instant>>,
    pub ipc_limiter:   Arc<Mutex<IpcRateLimiter>>,
    #[cfg(desktop)]
    pub grader_runtime: Arc<plugins::wasm_runtime::GraderRuntime>,
}
```

Existing accessors like `state.vault_dir` become `state.active().vault_dir`. A helper `state.active_or_locked()` returns `Result<RwLockReadGuard<ActiveProfile>, String>` so command handlers stay terse.

### `WHERE id = 1` stays untouched

Every `local_identity` query that today reads `WHERE id = 1` keeps working unchanged: each profile has its own SQLCipher database, so row 1 of that database **is** the active profile's identity. We do not multiplex identities inside one DB — that would be brittle and require touching 23 queries. Isolation at the DB-file level is simpler and stronger.

### Peer ID per profile

Each profile gets its own libp2p peer ID and iroh node ID, derived from that profile's keypair. Switching profiles tears down the active swarm and starts a new one. Two profiles on the same device are network-indistinguishable from two devices on the same LAN; nothing in the protocol layer leaks the shared host.

The iroh blob store is a per-device singleton (`AppState::content_node: Arc<ContentNode>`) but is repointed at the active profile's blob directory on every unlock via `ContentNode::set_data_dir`. Lock (`stop_active_profile`) calls both `Router::shutdown` AND `Store::shutdown` — the latter is required so the iroh-blobs Actor exits its private tokio runtime and releases the redb `blobs.db` file lock. Without `Store::shutdown`, a follow-up `FsStore::load` on the same path within the same process hangs indefinitely (observed early on; fixed via the iroh-blobs 0.98 public `Store::shutdown` API — no fork needed).

The cost: in-flight tutoring sessions are terminated on switch. We surface a confirmation dialog when active sessions exist.

### SQLCipher key per profile

Each DB's encryption key is derived from that profile's password via the existing `db_key` derivation. No cross-profile key reuse. Forgetting a profile password means that profile's data is unrecoverable — same trust model as today, scoped per profile.

## Migration

On first launch after upgrade, if `<app_data>/alexandria.db` exists at the legacy path and `<app_data>/profiles/` does not, run the migrator:

1. Generate `new_uuid`.
2. Create `<app_data>/profiles/<new_uuid>/`.
3. **Move** (not copy): `alexandria.db`, `alexandria.db-wal`, `alexandria.db-shm`, `stronghold/` or `vault/`, `iroh/`, `plugins/`, `videocache/` into the new directory.
4. Write `profiles_index.json` with one entry. Display name defaults to `"My Profile"` (renameable later); avatar is a random emoji.
5. If any step fails: rollback by moving any partially-moved files back, leave legacy layout intact, surface a `migration_failed` event the picker can show as a banner.

The migration runs **before** `AppState` is constructed, so the rest of the app boots in the new layout from the very first frame. The user does **not** see a migration spinner — the move is filesystem-rename-fast.

## Frontend flow

```
┌─────────────────┐
│ App.vue init    │
│ list_profiles() │
└────────┬────────┘
         │
   ┌─────┴─────┐
   │ count?    │
   └─────┬─────┘
         │
   ┌─────┴───────────────────────────┐
   │                                 │
 == 0                              >= 1
   │                                 │
   ▼                                 ▼
/onboarding                       /profiles  (Profile Picker)
   │                                 │
   └─────────────┐         ┌─────────┴─────────┐
                 │         │                   │
                 ▼         ▼                   ▼
              (create)  Unlock(id)         Add user
                 │         │                   │
                 └────┬────┘                   ▼
                      ▼                    /onboarding
                  /home                  (creates new profile)
```

### `useProfiles` composable (replaces `useAuth` internals)

```ts
const profiles = ref<ProfileSummary[]>([])
const activeProfileId = ref<string | null>(null)

async function listProfiles(): Promise<void>
async function createProfile(displayName: string, password: string): Promise<ProfileSummary>
async function unlockProfile(id: string, password: string): Promise<void>
async function lockProfile(): Promise<void>                  // returns to /profiles
async function switchProfile(id: string): Promise<void>      // lock + redirect picker preselects id
async function deleteProfile(id: string, password: string): Promise<void>
async function renameProfile(id: string, name: string): Promise<void>
async function setAvatar(id: string, avatar: Avatar): Promise<void>
```

`useAuth` is kept as a thin compatibility shim — it delegates to `useProfiles` and exposes the same `isAuthenticated`/`displayName`/`stakeAddress` reactive properties that 10 components already consume. No mass rewrite.

### Picker UI

- Centered avatar grid, 4 columns desktop / 2 columns mobile.
- Each tile: avatar (emoji or generated identicon), display name, last-active relative time ("Today", "Yesterday", "3 days ago").
- One "+" tile at the end labeled **Add user**.
- Tile hover: subtle lift + accent ring in the profile's color.
- Click → password field slides in over the tile, with a back arrow. ESC dismisses.
- Top-right corner: settings icon (global app settings, not profile-scoped).

### Header switch

`AppTopBar.vue` gets an avatar pill on the right showing the active profile. Click opens a menu:
- `<profile name>` (current, dimmed)
- **Switch user** — locks current, routes to `/profiles`
- **Lock** — locks current, routes to `/profiles`
- **Settings** — navigates to the full-page settings (`/settings`)

Keyboard shortcut: `Cmd/Ctrl + Shift + U` invokes Switch user from anywhere.

## Privacy & security

- **At rest:** each profile's data is encrypted with that profile's password-derived key. A compromised host filesystem reveals only `profiles_index.json` (avatars, display names) plus ciphertext.
- **In memory:** only the active profile's keystore and database connection are held. Lock zeroes them.
- **Auto-lock:** existing inactivity timeout applies, scoped to the active profile.
- **Peer ID rotation:** distinct profiles cannot be linked through libp2p observation. Two profiles on one device look like two separate devices to the network.
- **No "remember me":** the picker shows display names by default but does not auto-fill passwords. Biometric unlock is the password-free path — when a profile has enrolled Touch ID / Face ID, selecting it prompts the OS and retrieves that profile's vault password from the keychain. Credentials are keyed per profile (`vault_password_<profileId>`) so one device can biometric-unlock any enrolled profile; see `useBiometricVault.ts` and `ProfileSelect.vue`.
- **Delete profile:** wipes the profile's directory and removes its index entry. Best-effort overwrite (not cryptographic erasure — the user already has password protection).

## File-surface impact

From the investigation pass:

- `src-tauri/src/lib.rs` — `AppState` refactor + setup() reorganisation.
- `src-tauri/src/profile/` — new module (manager, index, migration).
- `src-tauri/src/commands/identity.rs` — `unlock_vault`/`generate_wallet`/`restore_wallet`/`lock_vault` become `unlock_profile`/`create_profile`/`restore_profile`/`lock_profile`; legacy names kept as `#[deprecated]` shims for one release.
- `src-tauri/src/commands/profile.rs` — new IPC surface (`list_profiles`, `delete_profile`, `rename_profile`, `set_profile_avatar`, `get_active_profile_id`).
- `src-tauri/src/ipfs/node.rs` — `ContentNode::new` takes per-profile dir; already does.
- `src-tauri/src/p2p/sync.rs:66,823` — unchanged (per-profile DB).
- 23 `WHERE id = 1` queries — **unchanged** (per-profile DB).
- `src/composables/useProfiles.ts` — new.
- `src/composables/useAuth.ts` — refactored to delegate to `useProfiles`; public API preserved.
- `src/pages/ProfileSelect.vue` — new picker.
- `src/components/profile/ProfileTile.vue`, `AddProfileTile.vue` — new.
- `src/router/index.ts` — `/profiles` route, updated guards.
- `src/App.vue` — initial routing decision based on profile count.
- `src/components/layout/AppTopBar.vue` — avatar dropdown.

## Out of scope for this worktree

- Deep linking (`tauri-plugin-deep-link`). Requires multi-profile because cold-start deep links must queue per-profile.
- Push notifications. See [push-notifications-rfc.md](./push-notifications-rfc.md) for the researched architecture (relay-mediated APNs + FCM + UnifiedPush + native WS).
- Cross-profile content deduplication.

## Test plan

- `cargo test -p alexandria-node profile::` — unit tests for ProfileManager (create, list, delete, rename, atomic dir creation, index round-trip).
- `cargo test -p alexandria-node migration::` — migration round-trip on tempfile fixture (legacy layout → new layout, with verification that all files moved and SQLCipher still opens).
- `cargo test -p alexandria-node` — full suite (regression).
- `vue-tsc -b --noEmit` — strict-mode type check on the new composable and components.
- Manual UI: onboarding from zero profiles, picker with 1/3/6 profiles, switch with active tutoring session (cancel + confirm dialog), delete profile, rename, change avatar.
