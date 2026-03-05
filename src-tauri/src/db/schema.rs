/// Database migrations, ordered by version.
/// Each entry: (version, name, SQL).
///
/// The schema mirrors the v1 PostgreSQL schema with adjustments for
/// local-first operation (see architecture-v2.md §4.3):
///   - Deterministic IDs (blake2b-based) instead of server-generated UUIDs
///   - Only self + known peers in users table
///   - Local-only tables: peers, pins, sync_log, catalog
///   - No server-side tables: refresh_tokens, oauth_accounts
pub const MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "initial_schema", MIGRATION_001),
    (2, "profile_hash", MIGRATION_002),
    (3, "content_mappings", MIGRATION_003),
    (4, "assessment_columns", MIGRATION_004),
    (5, "governance_members", MIGRATION_005),
    (6, "reputation_engine", MIGRATION_006),
    (7, "governance_elections", MIGRATION_007),
    (8, "reputation_snapshots", MIGRATION_008),
    (9, "taxonomy_ratification", MIGRATION_009),
    (10, "cross_device_sync", MIGRATION_010),
    (11, "evidence_challenges", MIGRATION_011),
    (12, "multi_party_attestation", MIGRATION_012),
    (13, "visual_assets", MIGRATION_013),
    (14, "inline_content", MIGRATION_014),
    (15, "tutoring_sessions", MIGRATION_015),
];

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

const MIGRATION_002: &str = r#"
-- ============================================================
-- Migration 002: Profile Hash
-- Stores the iroh BLAKE3 hash of the user's published profile
-- document. The profile is a signed JSON blob on iroh.
-- ============================================================

ALTER TABLE local_identity ADD COLUMN profile_hash TEXT;
"#;

const MIGRATION_003: &str = r#"
-- ============================================================
-- Migration 003: Content Mappings
-- Bridges IPFS CIDs (SHA-256, used by v1 content on Blockfrost)
-- to iroh BLAKE3 hashes (used natively by v2). When content is
-- fetched from an IPFS gateway, it's cached in iroh and the
-- CID↔BLAKE3 mapping is recorded here for future lookups.
-- ============================================================

CREATE TABLE IF NOT EXISTS content_mappings (
    ipfs_cid    TEXT PRIMARY KEY,
    blake3_hash TEXT NOT NULL,
    size_bytes  INTEGER,
    mapped_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_content_mappings_blake3 ON content_mappings(blake3_hash);
"#;

const MIGRATION_004: &str = r#"
-- ============================================================
-- Migration 004: Assessment Columns
-- Adds weight and source_element_id to skill_assessments.
-- weight: used in evidence weighting (default 1.0, matches v1)
-- source_element_id: enables (course, element, skill) lookups
--   for auto-creating assessments when elements are completed.
-- ============================================================

ALTER TABLE skill_assessments ADD COLUMN weight REAL NOT NULL DEFAULT 1.0;
ALTER TABLE skill_assessments ADD COLUMN source_element_id TEXT;
"#;

const MIGRATION_005: &str = r#"
-- ============================================================
-- Migration 005: Governance DAO Members
-- Tracks DAO committee members for authority checks on taxonomy
-- updates. Per spec §7.3: "For taxonomy updates, verify the
-- signer is a DAO committee member."
-- Also tracks the most recent taxonomy version applied locally.
-- ============================================================

CREATE TABLE IF NOT EXISTS governance_dao_members (
    dao_id          TEXT NOT NULL REFERENCES governance_daos(id),
    stake_address   TEXT NOT NULL,           -- Cardano stake address (bech32)
    role            TEXT NOT NULL DEFAULT 'member', -- member|committee|chair
    joined_at       TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (dao_id, stake_address)
);

CREATE INDEX IF NOT EXISTS idx_dao_members_address ON governance_dao_members(stake_address);
"#;

const MIGRATION_006: &str = r#"
-- ============================================================
-- Migration 006: Reputation Engine
-- Adds tables for full whitepaper reputation computation:
--   - reputation_evidence: links assertions to skill proofs
--   - reputation_impact_deltas: per-learner impact deltas for
--     distribution metrics (median, percentiles, variance)
-- Also adds an index on reputation_assertions for role+skill
-- lookups used by instructor ranking queries.
-- ============================================================

-- Links a reputation assertion to the skill proofs that contributed
-- to it, recording the delta confidence and attribution weight per §2.7.
CREATE TABLE IF NOT EXISTS reputation_evidence (
    assertion_id       TEXT NOT NULL REFERENCES reputation_assertions(id) ON DELETE CASCADE,
    proof_id           TEXT NOT NULL REFERENCES skill_proofs(id),
    delta_confidence   REAL NOT NULL DEFAULT 0.0,
    attribution_weight REAL NOT NULL DEFAULT 1.0,
    PRIMARY KEY (assertion_id, proof_id)
);

-- Per-learner impact deltas for computing distribution metrics per §2.8.
-- Each row = one learner's proof update contributing to an instructor assertion.
-- Stored separately so we can compute median, p25, p75, variance.
CREATE TABLE IF NOT EXISTS reputation_impact_deltas (
    id              TEXT PRIMARY KEY,
    assertion_id    TEXT NOT NULL REFERENCES reputation_assertions(id) ON DELETE CASCADE,
    learner_address TEXT NOT NULL,
    delta           REAL NOT NULL,
    attribution     REAL NOT NULL,
    proof_id        TEXT REFERENCES skill_proofs(id),
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_impact_deltas_assertion
    ON reputation_impact_deltas(assertion_id);

CREATE INDEX IF NOT EXISTS idx_impact_deltas_learner
    ON reputation_impact_deltas(learner_address);

-- Composite index for instructor ranking queries:
-- GET /v1/skills/:id/instructors
CREATE INDEX IF NOT EXISTS idx_reputation_role_skill
    ON reputation_assertions(role, skill_id, proficiency_level);
"#;

const MIGRATION_007: &str = r#"
-- ============================================================
-- Migration 007: Governance Elections
-- Adds full election lifecycle tables (nomination → voting →
-- finalized), proposal voting, and extended DAO/proposal columns
-- matching the v1 schema and whitepaper §4.
-- ============================================================

-- ---- Elections ----

CREATE TABLE IF NOT EXISTS governance_elections (
    id                      TEXT PRIMARY KEY,
    dao_id                  TEXT NOT NULL REFERENCES governance_daos(id),
    title                   TEXT NOT NULL,
    description             TEXT,
    phase                   TEXT NOT NULL DEFAULT 'nomination',  -- nomination|voting|finalized|cancelled
    seats                   INTEGER NOT NULL DEFAULT 5,
    nominee_min_proficiency TEXT NOT NULL DEFAULT 'apply',       -- Bloom's level for nominees
    voter_min_proficiency   TEXT NOT NULL DEFAULT 'remember',    -- Bloom's level for voters
    nomination_start        TEXT NOT NULL DEFAULT (datetime('now')),
    nomination_end          TEXT,
    voting_end              TEXT,
    on_chain_tx             TEXT,
    created_at              TEXT NOT NULL DEFAULT (datetime('now')),
    finalized_at            TEXT
);

CREATE INDEX IF NOT EXISTS idx_elections_dao ON governance_elections(dao_id);
CREATE INDEX IF NOT EXISTS idx_elections_phase ON governance_elections(phase);

-- ---- Election Nominees ----

CREATE TABLE IF NOT EXISTS governance_election_nominees (
    id              TEXT PRIMARY KEY,
    election_id     TEXT NOT NULL REFERENCES governance_elections(id) ON DELETE CASCADE,
    stake_address   TEXT NOT NULL,
    accepted        INTEGER NOT NULL DEFAULT 0,   -- 0 = pending, 1 = accepted
    votes_received  INTEGER NOT NULL DEFAULT 0,
    is_winner       INTEGER NOT NULL DEFAULT 0,   -- 1 = elected
    nominated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(election_id, stake_address)
);

CREATE INDEX IF NOT EXISTS idx_nominees_election ON governance_election_nominees(election_id);

-- ---- Election Votes ----

CREATE TABLE IF NOT EXISTS governance_election_votes (
    id           TEXT PRIMARY KEY,
    election_id  TEXT NOT NULL REFERENCES governance_elections(id) ON DELETE CASCADE,
    voter        TEXT NOT NULL,       -- stake address
    nominee_id   TEXT NOT NULL REFERENCES governance_election_nominees(id),
    on_chain_tx  TEXT,
    voted_at     TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(election_id, voter)        -- one vote per voter per election
);

CREATE INDEX IF NOT EXISTS idx_election_votes_election ON governance_election_votes(election_id);

-- ---- Proposal Votes ----

CREATE TABLE IF NOT EXISTS governance_proposal_votes (
    id           TEXT PRIMARY KEY,
    proposal_id  TEXT NOT NULL REFERENCES governance_proposals(id) ON DELETE CASCADE,
    voter        TEXT NOT NULL,       -- stake address
    in_favor     INTEGER NOT NULL,    -- 1 = for, 0 = against
    on_chain_tx  TEXT,
    voted_at     TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(proposal_id, voter)        -- one vote per voter per proposal
);

CREATE INDEX IF NOT EXISTS idx_proposal_votes_proposal ON governance_proposal_votes(proposal_id);

-- ---- Extended DAO columns ----

ALTER TABLE governance_daos ADD COLUMN committee_size INTEGER NOT NULL DEFAULT 5;
ALTER TABLE governance_daos ADD COLUMN election_interval_days INTEGER NOT NULL DEFAULT 365;

-- ---- Extended Proposal columns ----

ALTER TABLE governance_proposals ADD COLUMN voting_deadline TEXT;
ALTER TABLE governance_proposals ADD COLUMN min_vote_proficiency TEXT NOT NULL DEFAULT 'remember';
"#;

const MIGRATION_008: &str = r#"
-- ============================================================
-- Migration 008: Reputation Snapshots
-- Tracks CIP-68 soulbound token minting for on-chain reputation
-- anchoring. Each snapshot records the status of anchoring a
-- subject+role reputation to the Cardano blockchain.
-- ============================================================

CREATE TABLE IF NOT EXISTS reputation_snapshots (
    id              TEXT PRIMARY KEY,
    actor_address   TEXT NOT NULL,
    subject_id      TEXT NOT NULL,
    role            TEXT NOT NULL,
    skill_count     INTEGER NOT NULL DEFAULT 0,
    tx_status       TEXT NOT NULL DEFAULT 'pending',  -- pending|building|submitted|confirmed|failed
    tx_hash         TEXT,
    policy_id       TEXT,
    ref_asset_name  TEXT,   -- CIP-68 reference token asset name (hex)
    user_asset_name TEXT,   -- CIP-68 user token asset name (hex)
    error_message   TEXT,
    snapshot_at     TEXT NOT NULL DEFAULT (datetime('now')),
    confirmed_at    TEXT
);

CREATE INDEX IF NOT EXISTS idx_snapshots_actor ON reputation_snapshots(actor_address);
CREATE INDEX IF NOT EXISTS idx_snapshots_status ON reputation_snapshots(tx_status);
CREATE INDEX IF NOT EXISTS idx_snapshots_subject ON reputation_snapshots(subject_id);
"#;

const MIGRATION_009: &str = r#"
-- ============================================================
-- Migration 009: Taxonomy Ratification
-- Adds content_cid and taxonomy_version columns to governance
-- proposals, enabling the DAO taxonomy ratification workflow:
--   propose → gossip → submit → vote → resolve+publish → apply
-- content_cid stores the serialized taxonomy changes JSON
-- (replaced with the IPFS CID on publish). taxonomy_version
-- records the target version number for the ratified taxonomy.
-- ============================================================

ALTER TABLE governance_proposals ADD COLUMN content_cid TEXT;
ALTER TABLE governance_proposals ADD COLUMN taxonomy_version INTEGER;
"#;

const MIGRATION_010: &str = r#"
-- ============================================================
-- Migration 010: Cross-Device Sync
-- Adds tables for multi-device synchronization:
--   - devices: registered devices sharing the same wallet
--   - sync_state: per-table last-synced timestamps (LWW vector)
--   - sync_queue: outbound changes queued for replication
-- Pairing = importing the same mnemonic on both devices.
-- Encryption: XChaCha20-Poly1305 with HKDF-derived key.
-- ============================================================

-- Known devices sharing this wallet identity.
-- Each device has a unique device_id (random UUID) and an
-- optional user-assigned name.
CREATE TABLE IF NOT EXISTS devices (
    id              TEXT PRIMARY KEY,      -- Random UUID per device
    device_name     TEXT,                  -- User-assigned label
    platform        TEXT,                  -- macos|windows|linux
    first_seen      TEXT NOT NULL DEFAULT (datetime('now')),
    last_synced     TEXT,
    is_local        INTEGER NOT NULL DEFAULT 0,  -- 1 = this device
    peer_id         TEXT                   -- libp2p PeerId (if known)
);

CREATE INDEX IF NOT EXISTS idx_devices_local ON devices(is_local);

-- Per-table sync state tracking (LWW vector clock).
-- Records the latest updated_at timestamp received from each
-- remote device per table, so we only send newer rows on sync.
CREATE TABLE IF NOT EXISTS sync_state (
    device_id       TEXT NOT NULL REFERENCES devices(id),
    table_name      TEXT NOT NULL,         -- enrollments|element_progress|course_notes|evidence_records|skill_proof_evidence
    last_synced_at  TEXT NOT NULL,          -- ISO 8601 timestamp of last sync
    row_count       INTEGER NOT NULL DEFAULT 0,  -- Number of rows synced
    PRIMARY KEY (device_id, table_name)
);

-- Outbound sync queue — changes that need to be sent to peers.
-- Items are dequeued after successful delivery to all known devices.
CREATE TABLE IF NOT EXISTS sync_queue (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    table_name      TEXT NOT NULL,
    row_id          TEXT NOT NULL,          -- PK of the changed row
    operation       TEXT NOT NULL,          -- insert|update|delete
    row_data        TEXT,                   -- JSON snapshot of the row (null for delete)
    updated_at      TEXT NOT NULL,          -- Timestamp of the change (LWW tiebreaker)
    queued_at       TEXT NOT NULL DEFAULT (datetime('now')),
    delivered_to    TEXT DEFAULT '[]'       -- JSON array of device_ids that received it
);

CREATE INDEX IF NOT EXISTS idx_sync_queue_table ON sync_queue(table_name);
CREATE INDEX IF NOT EXISTS idx_sync_queue_queued ON sync_queue(queued_at);

-- Add device_id to local_identity so we know which device we are.
ALTER TABLE local_identity ADD COLUMN device_id TEXT;
"#;

const MIGRATION_011: &str = r#"
-- ============================================================
-- Migration 011: Evidence Challenges
-- Adds tables for the evidence challenge mechanism. Any P2P
-- observer can dispute evidence or credentials by staking ADA.
-- DAO committee reviews; outcome is burn (upheld) or slash
-- (rejected). Challenges reuse the /alexandria/governance/1.0
-- gossip topic.
-- ============================================================

CREATE TABLE IF NOT EXISTS evidence_challenges (
    id              TEXT PRIMARY KEY,
    challenger      TEXT NOT NULL,
    target_type     TEXT NOT NULL,          -- evidence|skill_proof
    target_ids      TEXT NOT NULL,          -- JSON array of IDs
    evidence_cids   TEXT NOT NULL,          -- JSON array of IPFS CIDs
    reason          TEXT NOT NULL,
    stake_lovelace  INTEGER NOT NULL,
    stake_tx_hash   TEXT,
    status          TEXT NOT NULL DEFAULT 'pending',  -- pending|reviewing|upheld|rejected|expired
    dao_id          TEXT NOT NULL,
    learner_address TEXT NOT NULL,
    reviewed_by     TEXT DEFAULT '[]',      -- JSON array of reviewer addresses
    resolution_tx   TEXT,
    signature       TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    resolved_at     TEXT,
    expires_at      TEXT
);

CREATE INDEX IF NOT EXISTS idx_challenges_status ON evidence_challenges(status);
CREATE INDEX IF NOT EXISTS idx_challenges_learner ON evidence_challenges(learner_address);
CREATE INDEX IF NOT EXISTS idx_challenges_dao ON evidence_challenges(dao_id);
CREATE INDEX IF NOT EXISTS idx_challenges_challenger ON evidence_challenges(challenger);

CREATE TABLE IF NOT EXISTS challenge_votes (
    id              TEXT PRIMARY KEY,
    challenge_id    TEXT NOT NULL REFERENCES evidence_challenges(id) ON DELETE CASCADE,
    voter           TEXT NOT NULL,
    upheld          INTEGER NOT NULL,       -- 1 = uphold, 0 = reject
    reason          TEXT,
    voted_at        TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(challenge_id, voter)
);

CREATE INDEX IF NOT EXISTS idx_challenge_votes_challenge ON challenge_votes(challenge_id);
"#;

const MIGRATION_012: &str = r#"
-- ============================================================
-- Migration 012: Multi-Party Attestation
-- Adds governance-gated multi-party attestation for high-stakes
-- skills. When a skill is marked as high-stakes by the DAO,
-- evidence records require assessor co-signatures before they
-- count toward skill proof aggregation.
-- ============================================================

-- Skills that require multi-party attestation (set by DAO governance).
CREATE TABLE IF NOT EXISTS attestation_requirements (
    skill_id            TEXT NOT NULL REFERENCES skills(id),
    proficiency_level   TEXT NOT NULL,
    required_attestors  INTEGER NOT NULL DEFAULT 1,
    dao_id              TEXT NOT NULL,
    set_by_proposal     TEXT,
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at          TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (skill_id, proficiency_level)
);

CREATE INDEX IF NOT EXISTS idx_attest_req_dao ON attestation_requirements(dao_id);

-- Assessor attestations on evidence records.
CREATE TABLE IF NOT EXISTS evidence_attestations (
    id                  TEXT PRIMARY KEY,
    evidence_id         TEXT NOT NULL REFERENCES evidence_records(id) ON DELETE CASCADE,
    attestor_address    TEXT NOT NULL,
    attestor_role       TEXT NOT NULL DEFAULT 'assessor',
    attestation_type    TEXT NOT NULL DEFAULT 'co_sign',
    integrity_score     REAL,
    session_cid         TEXT,
    signature           TEXT NOT NULL,
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(evidence_id, attestor_address)
);

CREATE INDEX IF NOT EXISTS idx_attestations_evidence ON evidence_attestations(evidence_id);
CREATE INDEX IF NOT EXISTS idx_attestations_attestor ON evidence_attestations(attestor_address);
"#;

const MIGRATION_013: &str = r#"
-- ============================================================
-- Migration 013: Visual Assets
-- Adds columns for richer visual presentation:
--   - Course author display name and thumbnail SVG
--   - DAO and subject_field emoji icons
-- ============================================================

-- Course author display name (avoids showing raw addresses in the UI).
ALTER TABLE courses ADD COLUMN author_name TEXT;

-- Inline SVG thumbnail stored as a data URI string.
-- Avoids the IPFS/iroh dependency for seed thumbnails while keeping
-- the existing `thumbnail_cid` column for user-uploaded images.
ALTER TABLE courses ADD COLUMN thumbnail_svg TEXT;

-- Emoji icon for governance DAOs (displayed in cards and headers).
ALTER TABLE governance_daos ADD COLUMN icon_emoji TEXT;

-- Emoji icon for subject fields (displayed in taxonomy browser).
ALTER TABLE subject_fields ADD COLUMN icon_emoji TEXT;
"#;

const MIGRATION_014: &str = r#"
-- ============================================================
-- Migration 014: Inline Content
-- Adds a content_inline column to course_elements for storing
-- text/HTML/JSON content directly in the database. This allows
-- content to be available without an iroh/IPFS node (essential
-- for mobile and seed data).
-- ============================================================

ALTER TABLE course_elements ADD COLUMN content_inline TEXT;
"#;

const MIGRATION_015: &str = r#"
-- ============================================================
-- Migration 015: Tutoring Sessions
-- Stores live tutoring session metadata. The iroh-live room
-- ticket is persisted so sessions can be re-joined (while the
-- gossip topic is still alive) and for history/analytics.
-- ============================================================

CREATE TABLE IF NOT EXISTS tutoring_sessions (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    ticket      TEXT,
    status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'ended', 'cancelled')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    ended_at    TEXT
);
"#;
