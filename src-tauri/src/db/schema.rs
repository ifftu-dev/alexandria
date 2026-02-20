/// Database migrations, ordered by version.
/// Each entry: (version, name, SQL).
///
/// The schema mirrors the v1 PostgreSQL schema with adjustments for
/// local-first operation (see architecture-v2.md §4.3):
///   - Deterministic IDs (blake2b-based) instead of server-generated UUIDs
///   - Only self + known peers in users table
///   - Local-only tables: peers, pins, sync_log, catalog
///   - No server-side tables: refresh_tokens, oauth_accounts
pub const MIGRATIONS: &[(i64, &str, &str)] = &[(1, "initial_schema", MIGRATION_001)];

const MIGRATION_001: &str = r#"
-- ============================================================
-- Migration 001: Initial Schema
-- Alexandria Node — Local-first SQLite database
-- ============================================================

-- ---- Identity ----

-- The local user's wallet and profile.
-- In a local-first model, there is exactly ONE row: the node owner.
CREATE TABLE IF NOT EXISTS local_identity (
    id              INTEGER PRIMARY KEY CHECK (id = 1),  -- Singleton
    stake_address   TEXT NOT NULL UNIQUE,
    payment_address TEXT NOT NULL,
    display_name    TEXT,
    bio             TEXT,
    avatar_cid      TEXT,
    mnemonic_enc    BLOB,          -- Encrypted mnemonic (OS keychain preferred, this is fallback)
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ---- Skill Taxonomy ----

CREATE TABLE IF NOT EXISTS subject_fields (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS subjects (
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    description      TEXT,
    subject_field_id TEXT NOT NULL REFERENCES subject_fields(id),
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS skills (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    subject_id  TEXT NOT NULL REFERENCES subjects(id),
    bloom_level TEXT NOT NULL DEFAULT 'apply',  -- remember|understand|apply|analyze|evaluate|create
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS skill_prerequisites (
    skill_id        TEXT NOT NULL REFERENCES skills(id),
    prerequisite_id TEXT NOT NULL REFERENCES skills(id),
    PRIMARY KEY (skill_id, prerequisite_id),
    CHECK (skill_id != prerequisite_id)
);

CREATE TABLE IF NOT EXISTS skill_relations (
    skill_id        TEXT NOT NULL REFERENCES skills(id),
    related_skill_id TEXT NOT NULL REFERENCES skills(id),
    relation_type   TEXT NOT NULL DEFAULT 'related',  -- related|complementary|alternative
    PRIMARY KEY (skill_id, related_skill_id),
    CHECK (skill_id != related_skill_id)
);

-- Taxonomy version tracking (signed by DAO)
CREATE TABLE IF NOT EXISTS taxonomy_versions (
    version      INTEGER PRIMARY KEY,
    cid          TEXT NOT NULL,       -- IPFS CID of the full taxonomy document
    previous_cid TEXT,                -- CID of the previous version
    ratified_by  TEXT,                -- DAO committee multisig info
    ratified_at  TEXT,
    signature    TEXT,                -- Ed25519 signature
    applied_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ---- Courses ----

CREATE TABLE IF NOT EXISTS courses (
    id              TEXT PRIMARY KEY,  -- blake2b(author_stake_address + content_cid)
    title           TEXT NOT NULL,
    description     TEXT,
    author_address  TEXT NOT NULL,     -- Cardano stake address of the author
    content_cid     TEXT,              -- IPFS CID of course content root
    thumbnail_cid   TEXT,
    tags            TEXT,              -- JSON array
    skill_ids       TEXT,              -- JSON array of skill IDs
    version         INTEGER NOT NULL DEFAULT 1,
    status          TEXT NOT NULL DEFAULT 'draft',  -- draft|published|archived
    published_at    TEXT,
    on_chain_tx     TEXT,              -- Cardano tx hash (if registered on-chain)
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Course content structure (chapters, elements)
CREATE TABLE IF NOT EXISTS course_chapters (
    id          TEXT PRIMARY KEY,
    course_id   TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    description TEXT,
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS course_elements (
    id          TEXT PRIMARY KEY,
    chapter_id  TEXT NOT NULL REFERENCES course_chapters(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    element_type TEXT NOT NULL,  -- video|text|quiz|interactive|assessment
    content_cid TEXT,            -- IPFS CID of element content
    position    INTEGER NOT NULL DEFAULT 0,
    duration_seconds INTEGER,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Skill tags on elements (for evidence pipeline)
CREATE TABLE IF NOT EXISTS element_skill_tags (
    element_id TEXT NOT NULL REFERENCES course_elements(id) ON DELETE CASCADE,
    skill_id   TEXT NOT NULL REFERENCES skills(id),
    weight     REAL NOT NULL DEFAULT 1.0,
    PRIMARY KEY (element_id, skill_id)
);

-- ---- Enrollments & Progress ----

CREATE TABLE IF NOT EXISTS enrollments (
    id          TEXT PRIMARY KEY,  -- blake2b(stake_address + course_id)
    course_id   TEXT NOT NULL REFERENCES courses(id),
    enrolled_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    status      TEXT NOT NULL DEFAULT 'active',  -- active|completed|dropped
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS element_progress (
    id           TEXT PRIMARY KEY,
    enrollment_id TEXT NOT NULL REFERENCES enrollments(id) ON DELETE CASCADE,
    element_id   TEXT NOT NULL REFERENCES course_elements(id),
    status       TEXT NOT NULL DEFAULT 'not_started',  -- not_started|in_progress|completed
    score        REAL,             -- 0.0 to 1.0 for assessments
    time_spent   INTEGER DEFAULT 0,  -- seconds
    completed_at TEXT,
    updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(enrollment_id, element_id)
);

-- ---- Course Notes ----

CREATE TABLE IF NOT EXISTS course_notes (
    id            TEXT PRIMARY KEY,
    enrollment_id TEXT NOT NULL REFERENCES enrollments(id) ON DELETE CASCADE,
    chapter_id    TEXT REFERENCES course_chapters(id),
    element_id    TEXT REFERENCES course_elements(id),
    content_cid   TEXT,           -- IPFS CID of note content
    preview_text  TEXT,
    video_timestamp_seconds INTEGER,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ---- Assessments & Evidence ----

CREATE TABLE IF NOT EXISTS skill_assessments (
    id              TEXT PRIMARY KEY,
    skill_id        TEXT NOT NULL REFERENCES skills(id),
    course_id       TEXT REFERENCES courses(id),
    assessment_type TEXT NOT NULL DEFAULT 'quiz',  -- quiz|project|peer_review|exam
    proficiency_level TEXT NOT NULL DEFAULT 'apply',
    difficulty      REAL NOT NULL DEFAULT 0.50,
    trust_factor    REAL NOT NULL DEFAULT 1.0,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS evidence_records (
    id                    TEXT PRIMARY KEY,  -- blake2b(learner + assessment + timestamp)
    skill_assessment_id   TEXT NOT NULL REFERENCES skill_assessments(id),
    skill_id              TEXT NOT NULL REFERENCES skills(id),
    proficiency_level     TEXT NOT NULL,
    score                 REAL NOT NULL,     -- 0.0 to 1.0
    difficulty            REAL NOT NULL,
    trust_factor          REAL NOT NULL DEFAULT 1.0,
    course_id             TEXT REFERENCES courses(id),
    instructor_address    TEXT,              -- Cardano stake address
    integrity_session_id  TEXT,
    integrity_score       REAL,
    cid                   TEXT,              -- IPFS CID of evidence document
    signature             TEXT,              -- Ed25519 signature
    created_at            TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ---- Skill Proofs ----

CREATE TABLE IF NOT EXISTS skill_proofs (
    id                TEXT PRIMARY KEY,  -- blake2b(learner + skill + level)
    skill_id          TEXT NOT NULL REFERENCES skills(id),
    proficiency_level TEXT NOT NULL,
    confidence        REAL NOT NULL,
    evidence_count    INTEGER NOT NULL DEFAULT 0,
    cid               TEXT,              -- IPFS CID of proof document
    nft_policy_id     TEXT,              -- Cardano NFT policy ID
    nft_asset_name    TEXT,              -- Cardano NFT asset name
    nft_tx_hash       TEXT,              -- Minting transaction hash
    computed_at       TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS skill_proof_evidence (
    proof_id    TEXT NOT NULL REFERENCES skill_proofs(id) ON DELETE CASCADE,
    evidence_id TEXT NOT NULL REFERENCES evidence_records(id),
    PRIMARY KEY (proof_id, evidence_id)
);

-- ---- Reputation ----

CREATE TABLE IF NOT EXISTS reputation_assertions (
    id                TEXT PRIMARY KEY,
    actor_address     TEXT NOT NULL,      -- Cardano stake address
    role              TEXT NOT NULL,       -- instructor|learner|assessor|author|mentor
    skill_id          TEXT REFERENCES skills(id),
    proficiency_level TEXT,
    score             REAL NOT NULL,
    evidence_count    INTEGER NOT NULL DEFAULT 0,
    median_impact     REAL,
    impact_p25        REAL,
    impact_p75        REAL,
    learner_count     INTEGER,
    impact_variance   REAL,
    window_start      TEXT,
    window_end        TEXT,
    computation_spec  TEXT NOT NULL DEFAULT 'v2',
    cid               TEXT,              -- IPFS CID of reputation proof
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ---- Integrity (Sentinel) ----

CREATE TABLE IF NOT EXISTS integrity_sessions (
    id              TEXT PRIMARY KEY,
    enrollment_id   TEXT REFERENCES enrollments(id),
    status          TEXT NOT NULL DEFAULT 'active',  -- active|completed|flagged|suspended
    integrity_score REAL,
    started_at      TEXT NOT NULL DEFAULT (datetime('now')),
    ended_at        TEXT
);

CREATE TABLE IF NOT EXISTS integrity_snapshots (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES integrity_sessions(id) ON DELETE CASCADE,
    typing_score    REAL,
    mouse_score     REAL,
    human_score     REAL,
    tab_score       REAL,
    paste_score     REAL,
    devtools_score  REAL,
    camera_score    REAL,
    composite_score REAL,
    captured_at     TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ---- P2P Network (local-only) ----

CREATE TABLE IF NOT EXISTS peers (
    peer_id       TEXT PRIMARY KEY,    -- libp2p PeerId
    stake_address TEXT,                -- Cardano stake address (if known)
    display_name  TEXT,
    last_seen     TEXT NOT NULL,
    addresses     TEXT NOT NULL,       -- JSON array of multiaddrs
    roles         TEXT,                -- JSON array: ["instructor", "learner"]
    reputation    REAL
);

-- IPFS content pinning state
CREATE TABLE IF NOT EXISTS pins (
    cid           TEXT PRIMARY KEY,
    pin_type      TEXT NOT NULL,       -- course|evidence|profile|taxonomy
    size_bytes    INTEGER,
    last_accessed TEXT,
    auto_unpin    INTEGER DEFAULT 0,   -- 1 = ok to unpin under storage pressure
    pinned_at     TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Sync log: track what's been broadcast / received
CREATE TABLE IF NOT EXISTS sync_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL,         -- evidence|catalog|taxonomy|governance
    entity_id   TEXT NOT NULL,
    direction   TEXT NOT NULL,         -- sent|received
    peer_id     TEXT,                  -- Which peer (null = broadcast)
    signature   TEXT,
    synced_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Course catalog from the P2P network
CREATE TABLE IF NOT EXISTS catalog (
    course_id       TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    description     TEXT,
    author_address  TEXT NOT NULL,
    content_cid     TEXT NOT NULL,
    thumbnail_cid   TEXT,
    tags            TEXT,              -- JSON array
    skill_ids       TEXT,              -- JSON array of skill IDs
    version         INTEGER NOT NULL DEFAULT 1,
    published_at    TEXT NOT NULL,
    received_at     TEXT NOT NULL DEFAULT (datetime('now')),
    pinned          INTEGER DEFAULT 0,
    on_chain_tx     TEXT,
    signature       TEXT NOT NULL       -- Author's signature over the record
);

-- ---- Governance ----

CREATE TABLE IF NOT EXISTS governance_daos (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    scope_type  TEXT NOT NULL,         -- subject_field|subject
    scope_id    TEXT NOT NULL,         -- FK to subject_fields or subjects
    status      TEXT NOT NULL DEFAULT 'active',
    on_chain_tx TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS governance_proposals (
    id          TEXT PRIMARY KEY,
    dao_id      TEXT NOT NULL REFERENCES governance_daos(id),
    title       TEXT NOT NULL,
    description TEXT,
    category    TEXT NOT NULL,         -- taxonomy_change|policy|funding|content_moderation
    status      TEXT NOT NULL DEFAULT 'draft',
    proposer    TEXT NOT NULL,         -- stake address
    votes_for   INTEGER DEFAULT 0,
    votes_against INTEGER DEFAULT 0,
    on_chain_tx TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    resolved_at TEXT
);

-- ---- Indexes ----

CREATE INDEX IF NOT EXISTS idx_courses_author ON courses(author_address);
CREATE INDEX IF NOT EXISTS idx_courses_status ON courses(status);
CREATE INDEX IF NOT EXISTS idx_enrollments_course ON enrollments(course_id);
CREATE INDEX IF NOT EXISTS idx_element_progress_enrollment ON element_progress(enrollment_id);
CREATE INDEX IF NOT EXISTS idx_evidence_skill ON evidence_records(skill_id);
CREATE INDEX IF NOT EXISTS idx_evidence_course ON evidence_records(course_id);
CREATE INDEX IF NOT EXISTS idx_skill_proofs_skill ON skill_proofs(skill_id);
CREATE INDEX IF NOT EXISTS idx_reputation_actor ON reputation_assertions(actor_address);
CREATE INDEX IF NOT EXISTS idx_reputation_skill ON reputation_assertions(skill_id);
CREATE INDEX IF NOT EXISTS idx_catalog_author ON catalog(author_address);
CREATE INDEX IF NOT EXISTS idx_peers_last_seen ON peers(last_seen);
CREATE INDEX IF NOT EXISTS idx_sync_log_entity ON sync_log(entity_type, entity_id);
"#;
