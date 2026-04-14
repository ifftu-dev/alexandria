# alexandria/src-tauri/src/

**Generated:** 2026-04-15

## Standing Instructions

- **Documentation review after code changes**: After completing any code changes, always assess whether README and other docs need updating. Ask the user for permission before modifying any documentation files.

## Overview

Rust backend for the Tauri v2 desktop/mobile app. Core responsibilities include 194 registered Tauri commands, a 66-table SQLite schema, libp2p networking, iroh content storage, and Cardano integration.

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Tauri commands | `commands/` | Domain-oriented IPC handlers plus platform-specific tutoring variants |
| Domain models | `domain/` | Core app types plus the `vc/` protocol submodule |
| P2P networking | `p2p/` | Swarm, gossip, validation, scoring, discovery, vc-fetch, sync, stress |
| Database | `db/` | SQLite + versioned migrations |
| Tutoring | `tutoring/` | Platform-conditional (`desktop`, `mobile`, `ios`, `android`) |
| Cardano | `cardano/` | Pallas wallet/tx building; reference-script deployment still pending in-tree |
| Content storage | `ipfs/` | iroh blobs integration |
| Cryptography | `crypto/` | Ed25519, Blake2b, keystore |

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

| File | Lines | Risk |
|------|-------|------|
| `tutoring/manager_mobile.rs` | 2918 | Mobile tutoring logic |
| `p2p/stress.rs` | 1775 | Stress testing utilities |
| `tutoring/manager.rs` | 1939 | Desktop tutoring |
| `evidence/reputation.rs` | 1442 | Reputation system |
| `commands/governance.rs` | 1734 | Governance command surface |
| `p2p/sync.rs` | 1203 | P2P sync protocol |
