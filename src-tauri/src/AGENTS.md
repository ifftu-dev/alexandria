# alexandria/src-tauri/src/

**Generated:** 2026-09-15

## Standing Instructions

- **Documentation review after code changes**: After completing any code changes, always assess whether README and other docs need updating. Ask the user for permission before modifying any documentation files.

## Overview

Rust backend for the Tauri v2 desktop/mobile app. Core responsibilities include a network-bound per-profile data model (each user gets their own vault + SQLCipher DB + iroh blob store under `<app_data>/profiles/<uuid>/`), domain-oriented Tauri commands, a 90-migration SQLite schema per profile, libp2p networking, iroh content storage, and Cardano integration. Inspect the handler list and schema directly for command/table counts.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Network identity | `network_profile.rs`, `../resources/networks/preprod.json` | Strict network/service/trust-root profile plus embedded-resource identity checks. Currently supplies DHT provider-record namespacing; remaining wire IDs await the coordinated app/relay/monitoring N01 slice. |
| Profile lifecycle | `profile/` | ProfileManager, network-bound format-v2 `profiles_index.json` sidecar, first-launch auto-migrator from the legacy single-vault layout |
| Settings | `settings/` | Typed registry (`registry::keys`) + R/W store. Drives the unified per-profile `app_settings` table; `scope='sync'` rows propagate via cross-device sync (`p2p::sync::settings_outbound_snapshot` / `settings_apply_inbound`). See [`docs/settings.md`](../../docs/settings.md). |
| Tauri commands | `commands/` | Domain-oriented IPC handlers plus platform-specific tutoring variants. `commands/profile.rs` owns multi-user lifecycle; `commands/identity.rs` is active-profile-only; `commands/settings.rs` owns the per-profile settings store IPC. |
| Domain models | `domain/` | Core app types plus the `vc/` protocol submodule |
| P2P networking | `p2p/` | Swarm, gossip, validation, scoring, discovery, vc-fetch, graph-fetch (public skill graphs), profile-fetch, username-reg (registry receipts), sync, stress. `sync.rs` also fans settings rows out/in. |
| Database | `db/` | SQLite + 94 versioned migrations (one DB per profile). Retired challenge/plugin-attestation tables remain legacy storage until D03 and grant no authority. |
| Tutoring | `tutoring/` | Platform-conditional (`desktop`, `mobile`, `ios`, `android`) |
| Cardano | `cardano/` | Pallas wallet/tx building; reference scripts deployed on preprod (UTxOs in `cardano/script_refs.rs`) |
| Content storage | `content_store/` | iroh blobs integration. `ContentNode::set_data_dir` repoints the singleton at the active profile's blob dir on each unlock; `ContentNode::shutdown` calls both `Router::shutdown` and `Store::shutdown` so the redb lock releases between profile switches. |
| Cryptography | `crypto/` | Ed25519, Blake2b, per-profile keystore (Stronghold desktop / portable AES-256-GCM mobile) |
| AppState lifecycle | `lib.rs` | `start_active_profile` / `stop_active_profile` bring per-profile services up and down on switch |

Credential revocation, suspension, and reinstatement are issuer-bound. The
challenge/escrow command and builder paths are retired. Plugin credential
issuance accepts exact bundled manifest and grader bytes only; the reserved
plugin-attestation topic has no inbound persistence handler or authority.

## CONVENTIONS (Rust)

```rust
// Domain structs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Course { pub name: String, pub description: Option<String> }

// Import order
use std::sync::Arc;           // 1. std
use rusqlite::params;          // 2. external
use crate::domain::course::*;   // 3. crate-local

// Tauri command signature
#[tauri::command]
pub async fn list_courses(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<Course>, String> {
    let db = state.db.lock().unwrap();
    // ... SQL
    Ok(courses)
}
```

- **Never** `.unwrap()` in command handlers — use `.map_err(...)?`
- **Allowed**: `state.db.lock().unwrap()` (poisoned mutex = unrecoverable)
- Platform gates: `#[cfg(desktop)]`, `#[cfg(target_os = "ios")]`, etc.

## TESTING

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    #[test]
    fn get_course_by_id_returns_course() {
        let db = test_db();
        let result = get_course_by_id(db.conn(), "course-1");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn async_test() { /* tokio runtime */ }
}
```

- Test helpers: `test_db()`, `setup_db()`, `setup_identity()`, `insert_course()`
- `Database::open_in_memory()` for all DB tests
- FK constraint order: `subject_fields → subjects → skills → ...`

## COMPLEXITY HOTSPOTS

Line counts drift with every PR; regenerate with `wc -l` before relying on them.

| File | Lines | Risk |
|------|-------|------|
| `p2p/network.rs` | 3382 | Swarm behavior and protocol orchestration |
| `tutoring/manager_mobile.rs` | 3286 | Mobile tutoring logic |
| `db/schema.rs` | 3781 | 94 migrations |
| `tutoring/manager.rs` | 2136 | Desktop/Android tutoring |
| `evidence/reputation.rs` | 830 | Credential-backed reputation system |
| `p2p/sync.rs` | 1498 | P2P sync protocol |
| `p2p/stress.rs` | 11 | Stub (retired in VC-first cutover; placeholder) |
