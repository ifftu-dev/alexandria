# Alexandria (Mark 3) -- Performance Audit

**Date**: 2026-02-24 (updated 2026-03-25)
**Scope**: Full Rust backend (`src-tauri/src/`), CLI (`cli/`), frontend config, Cargo workspace
**Files audited**: Every file in `commands/` (20), `db/` (4), `evidence/` (7), `p2p/` (15), `ipfs/` (8), plus `lib.rs`, both `Cargo.toml` files, `package.json`, `vite.config.ts`

**Summary**: 2 critical, 5 high, 8 medium, 5 low, 3 informational findings. The codebase is generally well-structured with good patterns (WAL mode, `spawn_blocking` for crypto, proper lock scoping in several places), but has a systemic architectural bottleneck in the global DB mutex and several targeted issues worth addressing.

---

## CRITICAL

### C-1: Global DB mutex serializes all commands

**File**: `src-tauri/src/lib.rs:43`

`AppState.db` is `Arc<std::sync::Mutex<Database>>` (blocking mutex, not tokio):

```rust
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
    ...
}
```

Every single Tauri command acquires this lock for the duration of its DB access. SQLite in WAL mode supports concurrent readers, but this mutex serializes all reads against each other and all writes.

**Impact**: Under concurrent frontend calls (e.g., loading a dashboard that calls `list_enrollments`, `list_courses`, `list_skill_proofs`, `list_reputation` simultaneously), all commands queue behind one another. Latency scales linearly with concurrency.

**Fix**: Replace `Mutex<Database>` with an `r2d2::Pool<SqliteConnectionManager>` (or `deadpool-sqlite`). This lets readers proceed in parallel while writes serialize naturally via SQLite's WAL lock. Alternatively, use `tokio::sync::RwLock` as a simpler intermediate step (readers share, writers exclusive).

---

### C-2: DB lock held during entire gossip message processing loop

**File**: `src-tauri/src/commands/p2p.rs:76-130`

The gossip event handler acquires `db_for_events.lock().await` at line 76 and holds it through the entire if/else-if chain handling catalog, evidence, taxonomy, and governance messages. Every incoming gossip message holds the global DB mutex:

```rust
crate::p2p::types::P2pEvent::GossipMessage { topic, message } => {
    log::debug!("P2P event: gossip message on {topic}");
    let db = db_for_events.lock().await;  // Lock acquired here
    if topic == TOPIC_CATALOG {
        ...  // DB operations
    } else if topic == TOPIC_EVIDENCE {
        ...  // DB operations
    } else if topic == TOPIC_TAXONOMY {
        ...  // DB operations
    } else if topic == TOPIC_GOVERNANCE {
        ...  // DB operations
    }
    // Lock released here -- held for entire chain
}
```

**Impact**: During gossip bursts (e.g., joining a busy network with many peers), the DB lock is held continuously, blocking all frontend commands (`list_*`, `get_*`, `enroll`, etc.) until gossip processing completes. With rapid messages, this can cause multi-second UI freezes.

**Fix**: Move DB operations into a dedicated channel-based worker: gossip events push to an `mpsc` channel, a worker task batch-processes them. Or at minimum, scope the lock to each individual `if` branch so the lock is dropped between messages.

---

## HIGH

### H-1: N+1 query in `publish_course`

**File**: `src-tauri/src/commands/courses.rs:303-327`

Iterates `chapter_rows` and runs a separate SELECT per chapter inside the loop:

```rust
for (ch_id, ch_title, ch_desc, ch_pos) in &chapter_rows {
    let elements: Vec<DocumentElement> = {
        let mut el_stmt = db.conn().prepare(
            "SELECT id, title, element_type, content_cid, position, duration_seconds \
             FROM course_elements WHERE chapter_id = ?1 ORDER BY position ASC",
        )...;
        // ... maps rows ...
    };
    chapters.push(DocumentChapter { ... });
}
```

**Impact**: For a course with N chapters, this executes N+1 queries (1 for chapters + N for elements). A course with 20 chapters = 21 queries. Each query holds the DB mutex for its duration.

**Fix**: Single JOIN query: `SELECT ce.*, cc.id as chapter_id FROM course_elements ce JOIN course_chapters cc ON ce.chapter_id = cc.id WHERE cc.course_id = ?1 ORDER BY cc.position, ce.position`. Group results in Rust.

---

### H-2: Content resolver mutex held during gateway HTTP fetch

**File**: `src-tauri/src/commands/content.rs:96-104`

`state.resolver.lock().await` is held while `resolver.resolve(&identifier).await` executes. The resolver chain includes IPFS gateway fallback (`gateway.rs`) which makes HTTP requests with a 30-second timeout.

**Impact**: If a gateway is slow or unreachable, the resolver mutex is held for up to 30 seconds. All other `content_resolve` / `content_resolve_bytes` calls queue behind it. One slow resolution blocks all content fetching.

**Fix**: Clone the resolver (or wrap in `Arc`) so the mutex is only held to extract a reference, not during the actual I/O. Or use `RwLock` and take a read guard.

---

### H-3: ContentNode inner mutex serializes all blob operations

**File**: `src-tauri/src/ipfs/node.rs:55`

`ContentNode.inner` is `Arc<Mutex<Option<RunningNode>>>`. Every blob operation (`store_bytes`, `read_bytes`, `list_blobs`, etc. in `content.rs`) must acquire this lock to access the `FsStore`.

**Impact**: Concurrent content operations (e.g., seeding multiple blobs, resolving content while storing) are serialized. The iroh `FsStore` itself is thread-safe, so the outer mutex is unnecessary after startup.

**Fix**: After startup, move to `Arc<RwLock<Option<RunningNode>>>` or store the `FsStore` handle separately in an `Arc` that does not need locking for reads.

---

### H-4: Repeated PBKDF2 key derivation on every signing command

**File**: `src-tauri/src/commands/courses.rs:276`, `enrollment.rs:304`, `identity.rs:79,215,427`, `p2p.rs:46`, `cardano.rs:36,108`

`wallet_from_mnemonic()` is called on every command that needs signing. BIP-39 mnemonic-to-seed derivation uses PBKDF2 with 2048 rounds. This is called in at least 8 different command paths.

**Impact**: ~2-5ms per derivation (varies by CPU). On `update_progress` (called after every element completion), the user experiences unnecessary latency. On `publish_course`, it compounds with other heavy operations.

**Fix**: Cache the derived `Wallet` in `AppState` (behind a `Mutex<Option<Wallet>>`) after first unlock. Clear on lock/logout.

---

### ~~H-5: Dedup cache full-clear creates reprocessing window~~ — **FIXED**

**Status**: Resolved. The dedup cache now uses `lru::LruCache` with capacity 100,000. Least-recently-used entries are evicted individually — no full-clear replay window.

---

## MEDIUM

### M-1: No pagination on list queries

**Files**:
- `commands/evidence.rs:22` -- `list_skill_proofs` (no LIMIT)
- `commands/evidence.rs:58,66` -- `list_evidence` (no LIMIT)
- `commands/evidence.rs:112,120` -- `list_reputation` (no LIMIT)
- `commands/enrollment.rs:28,35` -- `list_enrollments` (no LIMIT)
- `commands/courses.rs:22` -- `list_courses` (no LIMIT)
- `commands/governance.rs:70` -- `list_daos` (no LIMIT)
- `commands/governance.rs:256` -- `list_elections` (no LIMIT)
- `commands/governance.rs:723` -- `list_proposals` (no LIMIT)
- `commands/snapshot.rs:143` -- `list_snapshots` (no LIMIT)

None of these queries have `LIMIT` clauses. They return all matching rows.

**Impact**: Local-first means data grows with usage. A power user with 500 evidence records and 100 courses would transfer increasingly large JSON payloads over the IPC bridge. Each list call also holds the DB mutex for the full scan duration.

**Fix**: Add `LIMIT ?N OFFSET ?M` parameters with sensible defaults (e.g., 50). The catalog search already does this correctly (LIMIT 200 cap).

---

### M-2: Missing index on `enrollments.status`

**File**: `src-tauri/src/db/schema.rs` (table definition around line 152), `commands/enrollment.rs:28`

The `enrollments` table has `idx_enrollments_course` on `course_id` but no index on `status`. Queries like `WHERE status = 'active'` and `WHERE course_id = ?1 AND status = 'active'` do a full table scan on the `status` column.

**Impact**: Low row counts currently, but grows linearly with courses. Full scan on every `list_enrollments(status="active")` call.

**Fix**: Add migration: `CREATE INDEX idx_enrollments_status ON enrollments(status);` or composite `(course_id, status)`.

---

### M-3: Missing index on `skill_assessments(course_id, source_element_id, skill_id)`

**File**: `src-tauri/src/evidence/aggregator.rs:166-168`

`find_or_create_assessment` queries `WHERE course_id = ?1 AND source_element_id = ?2 AND skill_id = ?3` with no covering index. This is called for every evidence record aggregation.

**Impact**: Table scan on `skill_assessments` for each evidence submission. As assessments accumulate, this slows the evidence pipeline.

**Fix**: Add migration: `CREATE INDEX idx_assessments_lookup ON skill_assessments(course_id, source_element_id, skill_id);`

---

### M-4: Missing index on `evidence_records.skill_assessment_id`

**File**: `src-tauri/src/db/schema.rs` (evidence_records table, around line 200)

`evidence_records` has indexes on `skill_id` and `course_id` but not on `skill_assessment_id`. This column is used in JOINs in the aggregator and reputation engine.

**Impact**: Any query joining `evidence_records` on `skill_assessment_id` does a table scan.

**Fix**: Add migration: `CREATE INDEX idx_evidence_assessment ON evidence_records(skill_assessment_id);`

---

### M-5: Sequential lock acquisitions in `update_progress`

**File**: `src-tauri/src/commands/enrollment.rs:295-310`

Acquires `p2p_node` lock (line 295), then `keystore` lock (line 299), then `db` lock again (line 310) -- three sequential mutex acquisitions. The DB lock was already acquired and dropped earlier in the same function.

**Impact**: Each lock acquisition waits for any current holder. Under concurrent progress updates, the sequential pattern maximizes contention. The second DB lock acquisition is especially wasteful.

**Fix**: Collect all needed data (wallet, broadcast payload) under a single DB lock scope, then broadcast without re-acquiring.

---

### M-6: Release profile uses `opt-level = "s"` (size optimization)

**File**: `Cargo.toml:14`

```toml
[profile.release]
codegen-units = 1
lto = true
opt-level = "s"
panic = "abort"
strip = true
```

`opt-level = "s"` optimizes for binary size rather than speed. For a desktop app, CPU performance matters more than a few MB of binary size difference.

**Impact**: Estimated 5-15% slower execution of CPU-bound code (crypto operations, evidence aggregation, reputation computation).

**Fix**: Change to `opt-level = 3` for maximum speed. Or use `opt-level = 2` as a compromise.

---

### M-7: Reputation computation is CPU-intensive with no caching

**File**: `src-tauri/src/evidence/reputation.rs` (full file, ~1440 lines)

The reputation engine performs complex statistical computation (percentiles, impact deltas, Bayesian confidence intervals) across all evidence records. It is called from commands with the DB mutex held.

**Impact**: As evidence accumulates, reputation recomputation gets progressively slower. No incremental or cached computation -- always recomputes from scratch.

**Fix**: Use `spawn_blocking` for the heavy computation (the pattern already exists in the codebase for crypto). Consider incremental updates or caching the last computed values with dirty flags.

---

### M-8: `tokio = { features = ["full"] }` includes unnecessary features

**File**: `src-tauri/Cargo.toml:28`

`features = ["full"]` includes `io-util`, `io-std`, `signal`, `process`, `test-util`, etc. The app only needs a subset.

**Impact**: Marginal compile-time increase (~5-10s) and slightly larger binary. Not a runtime issue.

**Fix**: Replace with explicit feature list: `features = ["rt-multi-thread", "macros", "sync", "time", "net", "fs"]`.

---

## LOW

### L-1: `list_courses` returns all columns including large text fields

**File**: `src-tauri/src/commands/courses.rs:22`

List query selects `description`, `tags`, `skill_ids` -- all potentially large text/JSON fields -- even when only titles and IDs might be needed for a list view.

**Impact**: Larger IPC payloads than necessary for list views.

**Fix**: Provide a lightweight `list_courses_summary` command that returns only `id, title, author_address, status, published_at`.

---

### L-2: String cloning in chapter iteration

**File**: `src-tauri/src/commands/courses.rs:331-334`

`ch_id.clone()`, `ch_title.clone()`, `ch_desc.clone()` inside the loop. These allocate new strings per chapter.

**Impact**: Negligible for typical course sizes (< 30 chapters). Would only matter for extremely large courses.

**Fix**: Use references or consume the vectors with `into_iter()`.

---

### L-3: Gossip message routing uses if/else-if chain

**File**: `src-tauri/src/commands/p2p.rs:77-130`

The topic routing uses a chain of `if topic == TOPIC_CATALOG ... else if topic == TOPIC_EVIDENCE ...` instead of a `match`. Not a performance issue, but makes adding new topics error-prone.

**Impact**: Readability/maintainability, not runtime performance.

**Fix**: Refactor to `match topic.as_str() { ... }` or a dispatch table.

---

### L-4: `list_skill_graph_edges` returns all edges with no filtering

**File**: `src-tauri/src/commands/taxonomy.rs:401`

Returns all prerequisite and relation edges in the entire skill taxonomy. No filtering by subject or field.

**Impact**: For a large taxonomy (1000+ skills), this could be a large result set. Currently manageable with the seeded data.

**Fix**: Add optional `subject_id` filter parameter.

---

### L-5: Error strings allocated on every `.map_err(|e| e.to_string())`

**Files**: Every command file throughout the codebase.

Every DB and crypto operation maps errors with `.map_err(|e| e.to_string())`, allocating a new String.

**Impact**: Negligible -- these only execute on error paths. This is idiomatic for Tauri commands.

**Fix**: Not worth changing.

---

## INFO (Positive findings)

### I-1: WAL mode and foreign keys enabled at connection time

**File**: `src-tauri/src/db/mod.rs:29-31`

WAL mode and `PRAGMA foreign_keys = ON` are set at connection open. This is correct and enables concurrent reads at the SQLite level (though the Rust Mutex negates this benefit -- see C-1).

---

### I-2: `spawn_blocking` used for crypto operations

**File**: `src-tauri/src/commands/identity.rs`

Stronghold vault operations use `spawn_blocking`, avoiding blocking the async runtime for CPU-intensive key derivation (scrypt under the hood, optimized via `[profile.dev.package.scrypt] opt-level = 3`).

---

### I-3: `seed_content` properly scopes DB lock

**File**: `src-tauri/src/db/seed_content.rs`

The content seeding function acquires the DB lock, reads what is needed, drops the lock, then does iroh blob operations without holding the DB mutex. This is the correct pattern that other commands should follow.

---

## Remediation priority

| # | Finding | Effort | Impact |
|---|---------|--------|--------|
| 1 | C-1: Replace DB Mutex with connection pool | Medium | Unblocks all concurrent reads |
| 2 | C-2: Channel-based gossip processing | Medium | Prevents gossip from blocking UI |
| 3 | H-2: Resolver mutex scoping | Low | Unblocks content fetching |
| 4 | H-1: Fix N+1 in publish_course | Low | Single query instead of N+1 |
| 5 | M-2 -- M-4: Add missing indexes | Low | Prevents table scans |
| 6 | M-1: Add pagination to list commands | Medium | Prevents unbounded growth |
| 7 | H-4: Cache derived wallet | Low | Eliminates repeated PBKDF2 |
| 8 | H-3: ContentNode lock scoping | Medium | Parallel blob operations |
| 9 | M-6: opt-level = 3 | Trivial | 5-15% faster CPU-bound code |
| ~~10~~ | ~~H-5: LRU dedup cache~~ | ~~Low~~ | **FIXED** |
| 11 | M-5: Fix sequential lock acquisitions | Low | Reduces lock contention |
| 12 | M-7: Spawn reputation computation | Medium | Prevents UI blocking on recompute |
