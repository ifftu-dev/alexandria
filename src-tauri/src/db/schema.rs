//! The baseline schema.
//!
//! One migration creates the whole schema. It is not a concatenation of the
//! 94 migrations it replaces: those were replayed into a database, the result
//! was dumped, and the dead tables and columns were removed from that dump, so
//! what follows is what the old chain actually produced rather than a reading
//! of its SQL. Parity was checked table by table — columns with their types,
//! nullability, defaults and primary keys, plus every foreign key, index
//! definition, trigger and view — and differed only by the deliberate
//! omissions below.
//!
//! What is deliberately absent, because the code that gave it authority was
//! deleted in D01 and D02:
//!
//! * the credential challenge and escrow tables, and plugin attestations and
//!   advisories — retired authority, and a stored row granted nothing even
//!   before the tables went;
//! * the Sentinel kill switch, weights blocklist and priors, and integrity
//!   attestations;
//! * the on-chain governance queue and the local governance DAO, proposal and
//!   election tables. `governance_genesis_trust_anchors` stays: genesis trust
//!   anchors are current, not retired;
//! * `bank_questions` and `question_bank_versions`, superseded by
//!   `assessment_items`, which is what an attempt actually draws from;
//! * `local_identity.account_role`, superseded by the `account_roles` set, and
//!   the CIP-68 columns on `reputation_snapshots` — `policy_id`,
//!   `ref_asset_name`, `user_asset_name`, `snapshot_format` and
//!   `snapshot_scope` — whose minting path is deleted.
//!
//! The runner is unchanged and still applies migrations atomically, one
//! transaction each. A fresh baseline is a starting point, not permission to
//! stop managing schema change.
//!
//! An old database is NOT upgraded to this schema. Disposable pre-launch data
//! is what makes that acceptable; the identity check in [`crate::db`] refuses
//! an unrecognised schema family with an actionable message rather than
//! migrating it or deleting it.

/// Every migration, in order. The baseline is number 1 and, for now, the only
/// entry; later schema changes append here and are applied on top.
pub const MIGRATIONS: &[(i64, &str, &str)] = &[(1, "baseline", MIGRATION_001_BASELINE)];

const MIGRATION_001_BASELINE: &str = r#"
CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    scope TEXT NOT NULL DEFAULT 'sync' CHECK (scope IN ('sync', 'device'))
);

CREATE TABLE assessment_attempts (
    id TEXT PRIMARY KEY,
    subject_did TEXT NOT NULL,
    bank_id TEXT NOT NULL REFERENCES question_banks(id),
    skill_id TEXT NOT NULL,
    seed INTEGER NOT NULL,
    question_ids TEXT NOT NULL,                                                                    -- JSON array of served question ids (in served order)
    option_orders TEXT NOT NULL,                                                                   -- JSON: per-question shuffled option index order
    integrity_session_id TEXT,
    score REAL,
    passed INTEGER,
    credential_id TEXT,                                                                            -- issued AssessmentCredential, if passed
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    graded_at TEXT,
    attempt_ordinal INTEGER,
    ended_at TEXT,
    end_reason TEXT CHECK (end_reason IS NULL OR end_reason IN ('diagnostics', 'interrupted')),
    draft_answers_json TEXT CHECK (draft_answers_json IS NULL OR json_valid(draft_answers_json))
);

CREATE TABLE assessment_item_skills (
    item_id TEXT NOT NULL REFERENCES assessment_items(id) ON DELETE CASCADE,
    skill_id TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 1.0,
    PRIMARY KEY (item_id, skill_id)
);

CREATE TABLE assessment_items (
    id TEXT PRIMARY KEY,
    item_kind TEXT NOT NULL CHECK (item_kind IN ('mcq', 'plugin')),
    skill_id TEXT NOT NULL,                                          -- Plugin providing the UI and grader. NULL for `mcq`, which resolves the built-in mcq-grader at grade time (it is installed at startup, so its CID is not knowable when this migration runs).
    plugin_cid TEXT,                                                 -- Safe to send to a client: prompt, options, kind, starter code.
    content_public TEXT NOT NULL,                                    -- NEVER sent to a client. Answer keys, hidden test cases. Merged into the grade envelope host-side as `content.grader_private`.
    grader_private TEXT,
    difficulty INTEGER NOT NULL DEFAULT 2,                           -- 1 (easy) .. 5 (hard) Populated in a follow-up once BloomLevel becomes a real enum; orthogonal to difficulty (an easy "create" item is possible).
    bloom_level TEXT,
    points REAL NOT NULL DEFAULT 1.0,                                -- Provenance: the bank this item came from, when it came from one.
    bank_id TEXT REFERENCES question_banks(id) ON DELETE CASCADE,
    author_did TEXT,
    taxonomy_version TEXT,
    ratified INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE attempt_items (
    attempt_id TEXT NOT NULL REFERENCES assessment_attempts(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,                                                       -- 0-based served order
    item_id TEXT NOT NULL,
    option_order TEXT,                                                              -- JSON: served position -> original index
    submission_json TEXT,                                                           -- what the learner submitted
    grader_cid TEXT,                                                                -- grader that actually produced `score`
    content_cid TEXT,
    submission_cid TEXT,
    score REAL,                                                                     -- [0,1] for this item
    score_details TEXT,                                                             -- grader `details` blob
    theta_after REAL,
    se_after REAL,
    graded_at TEXT,
    PRIMARY KEY (attempt_id, ordinal)
);

CREATE TABLE catalog (
    course_id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    author_address TEXT NOT NULL,
    content_cid TEXT NOT NULL,
    thumbnail_cid TEXT,
    tags TEXT,                                            -- JSON array
    skill_ids TEXT,                                       -- JSON array of skill IDs
    version INTEGER NOT NULL DEFAULT 1,
    published_at TEXT NOT NULL,
    received_at TEXT NOT NULL DEFAULT (datetime('now')),
    pinned INTEGER DEFAULT 0,
    on_chain_tx TEXT,
    signature TEXT NOT NULL,                              -- Author's signature over the record
    kind TEXT NOT NULL DEFAULT 'course'
);

CREATE TABLE chain_submission_members (
    network TEXT NOT NULL,
    member_kind TEXT NOT NULL CHECK (length(member_kind) > 0),
    member_id TEXT NOT NULL CHECK (length(member_id) > 0),
    operation_kind TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    PRIMARY KEY (network, member_kind, member_id),
    FOREIGN KEY (network, operation_kind, operation_id) REFERENCES chain_submissions(network, operation_kind, operation_id)
);

CREATE TABLE chain_submissions (
    network TEXT NOT NULL,
    operation_kind TEXT NOT NULL CHECK (length(operation_kind) > 0),
    operation_id TEXT NOT NULL CHECK (length(operation_id) > 0),
    tx_hash TEXT NOT NULL CHECK (length(tx_hash) = 64),
    signed_cbor BLOB NOT NULL CHECK (length(signed_cbor) > 0),
    context_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'outcome_unknown' CHECK (status IN ('outcome_unknown', 'submitted', 'confirmed', 'failed_on_chain')),
    confirmed_slot INTEGER CHECK (confirmed_slot >= 0),
    applied_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (network, operation_kind, operation_id)
);

CREATE TABLE classroom_calls (
    id TEXT PRIMARY KEY,
    classroom_id TEXT NOT NULL REFERENCES classrooms(id) ON DELETE CASCADE,
    channel_id TEXT REFERENCES classroom_channels(id),
    title TEXT NOT NULL,
    ticket TEXT,
    started_by TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'ended')),
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    ended_at TEXT
);

CREATE TABLE classroom_channels (
    id TEXT PRIMARY KEY,                                                                         -- blake2b(classroom_id + name)
    classroom_id TEXT NOT NULL REFERENCES classrooms(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    channel_type TEXT NOT NULL DEFAULT 'text' CHECK (channel_type IN ('text', 'announcement')),
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (classroom_id, name)
);

CREATE TABLE classroom_group_keys (
    classroom_id TEXT PRIMARY KEY,
    group_key_enc BLOB NOT NULL,
    key_version INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE classroom_join_requests (
    id TEXT PRIMARY KEY,
    classroom_id TEXT NOT NULL REFERENCES classrooms(id) ON DELETE CASCADE,
    stake_address TEXT NOT NULL,
    display_name TEXT,
    message TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'denied')),
    reviewed_by TEXT,
    requested_at TEXT NOT NULL DEFAULT (datetime('now')),
    reviewed_at TEXT
);

CREATE TABLE classroom_members (
    classroom_id TEXT NOT NULL REFERENCES classrooms(id) ON DELETE CASCADE,
    stake_address TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner', 'moderator', 'member')),
    display_name TEXT,
    joined_at TEXT NOT NULL DEFAULT (datetime('now')),
    x25519_public_key BLOB,
    PRIMARY KEY (classroom_id, stake_address)
);

CREATE TABLE classroom_messages (
    id TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL REFERENCES classroom_channels(id) ON DELETE CASCADE,
    classroom_id TEXT NOT NULL,
    sender_address TEXT NOT NULL,
    sender_name TEXT,
    content TEXT NOT NULL,
    edited_at TEXT,
    deleted INTEGER NOT NULL DEFAULT 0,
    sent_at TEXT NOT NULL,
    received_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE classrooms (
    id TEXT PRIMARY KEY,                                                             -- blake2b(owner_address + name + created_at_ms)
    name TEXT NOT NULL,
    description TEXT,
    icon_emoji TEXT,
    owner_address TEXT NOT NULL,                                                     -- Cardano stake address (bech32)
    invite_code TEXT UNIQUE,                                                         -- 8-char alphanumeric join code (optional)
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE completion_claims (
    id TEXT PRIMARY KEY,
    subject_did TEXT NOT NULL,
    course_id TEXT NOT NULL,
    completion_root TEXT NOT NULL,
    credential_ids_json TEXT NOT NULL CHECK (json_valid(credential_ids_json)),
    witness_unavailable INTEGER NOT NULL DEFAULT 0 CHECK (witness_unavailable IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    course_document_cid TEXT CHECK (course_document_cid IS NULL OR length(course_document_cid) = 64),
    course_document_version INTEGER CHECK (course_document_version IS NULL OR course_document_version > 0),
    completion_binding_json TEXT CHECK (completion_binding_json IS NULL OR json_valid(completion_binding_json)),
    enrollment_id TEXT REFERENCES enrollments(id),
    UNIQUE (subject_did, course_id, completion_root)
);

CREATE TABLE completion_observations (
    policy_id TEXT NOT NULL,
    asset_name_hex TEXT NOT NULL,
    tx_hash TEXT NOT NULL,
    subject_pubkey TEXT NOT NULL,                         -- hex, 64 chars (32-byte Ed25519 pubkey)
    course_id TEXT NOT NULL,                              -- hex
    completion_root TEXT NOT NULL,                        -- hex, 64 chars (32-byte blake2b-256)
    completion_time TEXT NOT NULL,                        -- ISO 8601 from CompletionDatum.timestamp
    credential_id TEXT,                                   -- populated once the VC is issued
    observed_at TEXT NOT NULL DEFAULT (datetime('now')),
    issued_at TEXT,
    PRIMARY KEY (policy_id, asset_name_hex)
);

CREATE TABLE completion_witness_requests (
    operation_id TEXT PRIMARY KEY,
    claim_id TEXT NOT NULL UNIQUE REFERENCES completion_claims(id),
    context_json TEXT NOT NULL CHECK (json_valid(context_json)),
    blocked INTEGER NOT NULL DEFAULT 0 CHECK (blocked IN (0, 1)),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE content_mappings (
    external_id TEXT PRIMARY KEY,
    blake3_hash TEXT NOT NULL,
    size_bytes INTEGER,
    mapped_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE course_chapters (
    id TEXT PRIMARY KEY,
    course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE course_completion_endorsements (
    id TEXT PRIMARY KEY CHECK (length(id) = 64),
    claim_id TEXT NOT NULL REFERENCES completion_claims(id) ON DELETE CASCADE,
    attestor_did TEXT NOT NULL,
    endorsement_json TEXT NOT NULL CHECK (json_valid(endorsement_json)),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (claim_id, attestor_did)
);

CREATE TABLE course_elements (
    id TEXT PRIMARY KEY,
    chapter_id TEXT NOT NULL REFERENCES course_chapters(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    element_type TEXT NOT NULL,                                                 -- video|text|quiz|interactive|assessment
    content_cid TEXT,                                                           -- content ID (BLAKE3 hash) of element content
    position INTEGER NOT NULL DEFAULT 0,
    duration_seconds INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    content_inline TEXT,
    plugin_cid TEXT,
    plugin_version TEXT,
    plugin_config_cid TEXT
);

CREATE TABLE course_notes (
    id TEXT PRIMARY KEY,
    enrollment_id TEXT NOT NULL REFERENCES enrollments(id) ON DELETE CASCADE,
    chapter_id TEXT REFERENCES course_chapters(id),
    element_id TEXT REFERENCES course_elements(id),
    content_cid TEXT,                                                          -- content ID (BLAKE3 hash) of note content
    preview_text TEXT,
    video_timestamp_seconds INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE courses (
    id TEXT PRIMARY KEY,                                                                                                         -- blake2b(author_stake_address + content_cid)
    title TEXT NOT NULL,
    description TEXT,
    author_address TEXT NOT NULL,                                                                                                -- Cardano stake address of the author
    content_cid TEXT,                                                                                                            -- content ID (BLAKE3 hash) of course content root
    thumbnail_cid TEXT,
    tags TEXT,                                                                                                                   -- JSON array
    skill_ids TEXT,                                                                                                              -- JSON array of skill IDs
    version INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'draft',                                                                                        -- draft|published|archived
    published_at TEXT,
    on_chain_tx TEXT,                                                                                                            -- Cardano tx hash (if registered on-chain)
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    author_name TEXT,
    thumbnail_svg TEXT,
    kind TEXT NOT NULL DEFAULT 'course' CHECK (kind IN ('course', 'tutorial')),
    provenance TEXT,
    course_document_version INTEGER CHECK (course_document_version IS NULL OR course_document_version > 0),
    completion_policy_json TEXT CHECK (completion_policy_json IS NULL OR json_valid(completion_policy_json)),
    draft_completion_policy_json TEXT CHECK (draft_completion_policy_json IS NULL OR json_valid(draft_completion_policy_json))
);

CREATE TABLE credential_allowlist (
    credential_id TEXT NOT NULL,
    requestor_did TEXT NOT NULL,                         -- or the literal 'public'
    granted_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (credential_id, requestor_did)
);

CREATE TABLE credential_anchors (
    credential_id TEXT PRIMARY KEY REFERENCES credentials(id),
    anchor_tx_hash TEXT,
    anchor_status TEXT NOT NULL DEFAULT 'pending',              -- pending|submitted|confirmed|failed
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    next_attempt_at TEXT,
    enqueued_at TEXT NOT NULL DEFAULT (datetime('now')),
    confirmed_at TEXT
);

CREATE TABLE credential_status_lists (
    list_id TEXT PRIMARY KEY,                            -- issuer's list identifier
    issuer_did TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,                  -- monotonic; older versions ignored
    status_purpose TEXT NOT NULL DEFAULT 'revocation',
    bits BLOB NOT NULL,                                  -- packed little-endian bitmap
    bit_length INTEGER NOT NULL DEFAULT 0,
    signature TEXT,                                      -- issuer signature over (list_id, version, bits)
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE credentials (
    id TEXT PRIMARY KEY,                                  -- e.g. urn:uuid:...
    issuer_did TEXT NOT NULL,
    subject_did TEXT NOT NULL,
    credential_type TEXT NOT NULL,                        -- FormalCredential, etc.
    claim_kind TEXT NOT NULL,                             -- skill | role | custom
    skill_id TEXT,                                        -- NULL for non-skill claims
    issuance_date TEXT NOT NULL,
    expiration_date TEXT,
    signed_vc_json TEXT NOT NULL,                         -- full JSON-LD VC
    integrity_hash TEXT NOT NULL,                         -- hex(blake3(JCS bytes))
    status_list_id TEXT,                                  -- FK to credential_status_lists.list_id
    status_list_index INTEGER,                            -- bit position in the list
    revoked INTEGER NOT NULL DEFAULT 0,                   -- cached from status list for fast queries
    revoked_at TEXT,
    revocation_reason TEXT,
    supersedes TEXT,                                      -- prior credential id, §11.4
    received_at TEXT NOT NULL DEFAULT (datetime('now')),
    suspended INTEGER NOT NULL DEFAULT 0,
    suspended_at TEXT,
    suspended_until TEXT,
    suspended_reason TEXT,
    witness_tx_hash TEXT,
    witness_validator_script_hash TEXT,
    witness_validator_name TEXT,
    auto_issued INTEGER NOT NULL DEFAULT 0,
    provenance TEXT
);

CREATE TABLE credentials_pending_verification (
    id TEXT PRIMARY KEY,
    issuer_did TEXT NOT NULL,
    subject_did TEXT NOT NULL,
    signed_vc_json TEXT NOT NULL,
    received_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE derived_skill_state_history (
    subject_did TEXT NOT NULL,
    skill_id TEXT NOT NULL,
    snapshot_date TEXT NOT NULL,
    raw_score REAL NOT NULL,
    confidence REAL NOT NULL,
    trust_score REAL NOT NULL,
    level INTEGER NOT NULL,
    evidence_mass REAL NOT NULL,
    computed_at TEXT NOT NULL,
    PRIMARY KEY (subject_did, skill_id, snapshot_date)
);

CREATE TABLE derived_skill_states (
    subject_did TEXT NOT NULL,
    skill_id TEXT NOT NULL,
    calculation_version TEXT NOT NULL,
    raw_score REAL NOT NULL,
    confidence REAL NOT NULL,
    trust_score REAL NOT NULL,
    level INTEGER NOT NULL,
    evidence_mass REAL NOT NULL,
    unique_issuer_clusters INTEGER NOT NULL,
    active_evidence_count INTEGER NOT NULL,
    state_json TEXT NOT NULL,                                  -- full DerivedSkillState
    computed_at TEXT NOT NULL,
    dominant_provenance TEXT,
    input_fingerprint TEXT,
    PRIMARY KEY (subject_did, skill_id, calculation_version)
);

CREATE TABLE devices (
    id TEXT PRIMARY KEY,                                 -- Random UUID per device
    device_name TEXT,                                    -- User-assigned label
    platform TEXT,                                       -- macos|windows|linux
    first_seen TEXT NOT NULL DEFAULT (datetime('now')),
    last_synced TEXT,
    is_local INTEGER NOT NULL DEFAULT 0,                 -- 1 = this device
    peer_id TEXT,                                        -- libp2p PeerId (if known)
    stake_address TEXT,
    shared_key BLOB,
    paired INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE dht_records (
    key BLOB PRIMARY KEY,
    value BLOB NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE element_progress (
    id TEXT PRIMARY KEY,
    enrollment_id TEXT NOT NULL REFERENCES enrollments(id) ON DELETE CASCADE,
    element_id TEXT NOT NULL REFERENCES course_elements(id),
    status TEXT NOT NULL DEFAULT 'not_started',                                -- not_started|in_progress|completed
    score REAL,                                                                -- 0.0 to 1.0 for assessments
    time_spent INTEGER DEFAULT 0,                                              -- seconds
    completed_at TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(enrollment_id, element_id)
);

CREATE TABLE element_skill_tags (
    element_id TEXT NOT NULL REFERENCES course_elements(id) ON DELETE CASCADE,
    skill_id TEXT NOT NULL REFERENCES skills(id),
    weight REAL NOT NULL DEFAULT 1.0,
    PRIMARY KEY (element_id, skill_id)
);

CREATE TABLE element_submissions (
    id TEXT PRIMARY KEY,
    element_id TEXT NOT NULL REFERENCES course_elements(id),
    enrollment_id TEXT NOT NULL REFERENCES enrollments(id) ON DELETE CASCADE,
    submission_cid TEXT NOT NULL,                                              -- BLAKE3 of the submission bytes (in iroh store)
    grader_cid TEXT NOT NULL,                                                  -- BLAKE3 of the grader.wasm
    content_cid TEXT NOT NULL,                                                 -- BLAKE3 of the content bytes the grader saw
    score REAL NOT NULL CHECK (score >= 0.0 AND score <= 1.0),
    score_details_json TEXT,                                                   -- plugin-defined `details` payload
    learner_did TEXT NOT NULL,
    signed_attestation BLOB,                                                   -- Ed25519 signature over the bundle (NULL until signed)
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    grader_version TEXT NOT NULL DEFAULT '',
    answers_json TEXT,
    evidence_published INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE enrollments (
    id TEXT PRIMARY KEY,                                                                                       -- blake2b(stake_address + course_id)
    course_id TEXT NOT NULL REFERENCES courses(id),
    enrolled_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    status TEXT NOT NULL DEFAULT 'active',                                                                     -- active|completed|dropped
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    course_document_cid TEXT CHECK (course_document_cid IS NULL OR length(course_document_cid) = 64),
    course_document_version INTEGER CHECK (course_document_version IS NULL OR course_document_version > 0),
    completion_policy_json TEXT CHECK (completion_policy_json IS NULL OR json_valid(completion_policy_json))
);

CREATE TABLE goal_template_versions (
    version INTEGER PRIMARY KEY,
    content_cid TEXT NOT NULL,
    previous_cid TEXT,
    ratified_by TEXT,                                      -- DAO multisig / committee id
    signature TEXT,
    taxonomy_version TEXT,
    published_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE goal_templates (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('exam','curriculum','job_role')),
    key TEXT NOT NULL,                                                    -- stable slug, e.g. 'cbse.grade10', 'jee_main', 'engineering_manager'
    label TEXT NOT NULL,                                                  -- human label, e.g. 'CBSE — Grade 10'
    board TEXT,                                                           -- curriculum only: 'CBSE' | 'ICSE' | 'IB' | ...
    grade TEXT,                                                           -- curriculum only: '10'
    skill_ids TEXT NOT NULL,                                              -- JSON array of target skill ids
    taxonomy_version TEXT,                                                -- skill-graph version these ids were authored against
    dao_id TEXT,                                                          -- ratifying DAO (NULL for genesis-seeded)
    ratified INTEGER NOT NULL DEFAULT 0,
    content_cid TEXT,                                                     -- published version doc CID (NULL for genesis)
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE governance_genesis_trust_anchors (
    dao_id TEXT PRIMARY KEY CHECK (length(dao_id) = 64),
    genesis_hash TEXT NOT NULL UNIQUE CHECK (length(genesis_hash) = 64 AND genesis_hash = dao_id),
    genesis_json BLOB NOT NULL CHECK (length(genesis_json) > 0),
    name TEXT NOT NULL CHECK (length(name) > 0),
    scope_type TEXT NOT NULL CHECK (length(scope_type) > 0),
    scope_id TEXT NOT NULL CHECK (length(scope_id) > 0),
    rules_hash TEXT NOT NULL CHECK (length(rules_hash) = 64),
    pinned_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE guardian_activity_rows (
    link_id TEXT NOT NULL REFERENCES guardian_links(id) ON DELETE CASCADE,
    table_name TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (link_id, table_name, entity_id)
);

CREATE TABLE guardian_links (
    id TEXT PRIMARY KEY,
    side TEXT NOT NULL CHECK (side IN ('ward','guardian')),
    peer_did TEXT NOT NULL,
    peer_stake_address TEXT,
    peer_peer_id TEXT,                                                      -- libp2p PeerId once known
    peer_display_name TEXT,
    shared_key BLOB NOT NULL,                                               -- 32-byte AEAD key (profile DB is vault-scoped)
    status TEXT NOT NULL CHECK (status IN ('pending','active','revoked')),
    guardian_vc_id TEXT,                                                    -- parent-issued RoleCredential(role='guardian')
    invite_code_hash TEXT,                                                  -- guardian side: for retrying Link while pending
    child_birthdate TEXT,                                                   -- guardian side only, from sealed payload
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_sync_at TEXT
);

CREATE TABLE guardian_pending_invites (
    code_hash TEXT PRIMARY KEY,
    shared_key BLOB NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE integrity_evidence (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES integrity_sessions(id) ON DELETE CASCADE,  -- The flagged snapshot this evidence belongs to.
    snapshot_id TEXT REFERENCES integrity_snapshots(id) ON DELETE CASCADE,         -- camera_frame | keystroke | mouse | gaze
    kind TEXT NOT NULL,                                                            -- Opaque payload. Camera frames are stored encoded, not as raw RGBA. The database file is SQLCipher-encrypted at rest under the profile vault key, so this inherits that protection and nothing weaker.
    payload BLOB NOT NULL,
    captured_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE TABLE integrity_evidence_consent (
    session_id TEXT PRIMARY KEY REFERENCES integrity_sessions(id) ON DELETE CASCADE,  -- 1 = learner chose to preserve, 0 = declined. There is no default: a row exists only because a human answered.
    granted INTEGER NOT NULL CHECK (granted IN (0, 1)),
    decided_at TEXT NOT NULL DEFAULT (datetime('now')),                               -- NULL when declined. Absolute deadline, not a duration, so a device that is offline for a month still expires the evidence on next open.
    expires_at TEXT
);

CREATE TABLE integrity_evidence_release (
    session_id TEXT NOT NULL,                             -- The service it went to, as the learner's directory list spells it.
    directory_url TEXT NOT NULL,                          -- Their identifier for the assessment, learned from that person's own export. This device does not otherwise have a name for it.
    run_id TEXT NOT NULL,
    released_at TEXT NOT NULL DEFAULT (datetime('now')),
    item_count INTEGER NOT NULL DEFAULT 0,                -- Set the moment the learner asks for it back. Cleared never; it is the record that they asked.
    revoke_wanted_at TEXT,                                -- Set when the service confirmed. Until then the withdrawal is still owed.
    revoked_at TEXT,
    PRIMARY KEY (session_id, directory_url, run_id)
);

CREATE TABLE integrity_flag_notice (
    directory_url TEXT NOT NULL,                            -- The service's identifier for the assessment.
    run_id TEXT NOT NULL,                                   -- What it was for, kept so the notice can name it without another fetch.
    organisation TEXT NOT NULL DEFAULT '',
    role_label TEXT NOT NULL DEFAULT '',
    first_seen_at TEXT NOT NULL DEFAULT (datetime('now')),  -- When the learner was shown it. NULL means they have not been yet.
    told_at TEXT,
    PRIMARY KEY (directory_url, run_id)
);

CREATE TABLE integrity_sessions (
    id TEXT PRIMARY KEY,
    enrollment_id TEXT REFERENCES enrollments(id),
    status TEXT NOT NULL DEFAULT 'active',                                                      -- active|completed|flagged|suspended
    integrity_score REAL,
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    ended_at TEXT,
    critical_count INTEGER NOT NULL DEFAULT 0,
    warning_count INTEGER NOT NULL DEFAULT 0,
    assurance_level TEXT NOT NULL DEFAULT 'local',
    commitment_root TEXT,
    anchor_ref TEXT,
    purpose TEXT NOT NULL DEFAULT 'assessment' CHECK (purpose IN ('assessment', 'interview'))
);

CREATE TABLE integrity_snapshots (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES integrity_sessions(id) ON DELETE CASCADE,
    typing_score REAL,
    mouse_score REAL,
    human_score REAL,
    tab_score REAL,
    paste_score REAL,
    devtools_score REAL,
    camera_score REAL,
    composite_score REAL,
    captured_at TEXT NOT NULL DEFAULT (datetime('now')),
    anomaly_flags TEXT,
    ai_paste_anomaly REAL,
    gaze_offscreen_ratio REAL,
    commitment_hash TEXT
);

CREATE TABLE interview_criteria (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES interview_sessions(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    position INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'not_covered' CHECK (status IN ('not_covered', 'partial', 'covered')),
    notes TEXT
);

CREATE TABLE interview_followups (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES interview_sessions(id) ON DELETE CASCADE,
    source_segment_id TEXT REFERENCES interview_transcript_segments(id) ON DELETE SET NULL,
    criterion_id TEXT REFERENCES interview_criteria(id) ON DELETE SET NULL,
    question TEXT NOT NULL,
    reason TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'suggested' CHECK (status IN ('suggested', 'asked', 'dismissed')),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE interview_notes (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES interview_sessions(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    is_private INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE interview_participants (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES interview_sessions(id) ON DELETE CASCADE,
    peer_id TEXT,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('interviewer', 'candidate', 'observer')),
    pseudonym TEXT NOT NULL,
    consent_transcription INTEGER NOT NULL DEFAULT 0,
    consent_audio_recording INTEGER NOT NULL DEFAULT 0,
    consent_video_recording INTEGER NOT NULL DEFAULT 0,
    consent_sentinel INTEGER NOT NULL DEFAULT 0,
    consent_camera INTEGER NOT NULL DEFAULT 0,
    consented_at TEXT,
    revoked_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE interview_sessions (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    objective TEXT,
    role_assessment_id TEXT REFERENCES role_assessments(id) ON DELETE SET NULL,
    tutoring_session_id TEXT,
    status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'ready', 'live', 'completed')),
    duration_minutes INTEGER NOT NULL DEFAULT 45 CHECK (duration_minutes > 0),
    retention_days INTEGER NOT NULL DEFAULT 30 CHECK (retention_days BETWEEN 1 AND 365),
    record_audio INTEGER NOT NULL DEFAULT 0,
    record_video INTEGER NOT NULL DEFAULT 0,
    sentinel_enabled INTEGER NOT NULL DEFAULT 1,
    integrity_session_id TEXT REFERENCES integrity_sessions(id) ON DELETE SET NULL,
    summary TEXT,
    conclusion TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    started_at TEXT,
    ended_at TEXT,
    expires_at TEXT NOT NULL
);

CREATE TABLE interview_transcript_segments (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES interview_sessions(id) ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES interview_participants(id) ON DELETE CASCADE,
    speaker_label TEXT NOT NULL,
    text TEXT NOT NULL,
    start_ms INTEGER NOT NULL DEFAULT 0,
    end_ms INTEGER NOT NULL DEFAULT 0,
    is_final INTEGER NOT NULL DEFAULT 1,
    confidence REAL,
    source TEXT NOT NULL DEFAULT 'manual' CHECK (source IN ('local_stt', 'remote_stt', 'manual')),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE key_registry (
    did TEXT NOT NULL,
    key_id TEXT NOT NULL,          -- '<did>#key-N' fragment
    public_key_hex TEXT NOT NULL,  -- raw 32-byte Ed25519 pubkey, hex
    valid_from TEXT NOT NULL,      -- ISO 8601 UTC
    valid_until TEXT,              -- NULL while active
    rotated_by TEXT,               -- DID of successor, if rotated
    PRIMARY KEY (did, key_id)
);

CREATE TABLE local_identity (
    id INTEGER PRIMARY KEY CHECK (id = 1),                                                                      -- Singleton
    stake_address TEXT NOT NULL UNIQUE,
    payment_address TEXT NOT NULL,
    display_name TEXT,
    bio TEXT,
    avatar_cid TEXT,
    mnemonic_enc BLOB,                                                                                          -- Encrypted mnemonic (OS keychain preferred, this is fallback)
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    profile_hash TEXT,
    device_id TEXT,
    x25519_public_key BLOB,
    username TEXT,
    visibility TEXT NOT NULL DEFAULT 'public',
    birthdate TEXT,
    activation_state TEXT NOT NULL DEFAULT 'active' CHECK (activation_state IN ('active','pending_guardian')),
    account_roles TEXT NOT NULL DEFAULT '["learner"]'
);

CREATE TABLE opinions (
    id TEXT PRIMARY KEY,                                           -- blake2b(author_address + video_cid)
    author_address TEXT NOT NULL,                                  -- Cardano stake address
    subject_field_id TEXT NOT NULL REFERENCES subject_fields(id),
    title TEXT NOT NULL,
    summary TEXT,                                                  -- soft limit 280 chars at app layer
    video_cid TEXT NOT NULL,                                       -- iroh BLAKE3 of video blob
    thumbnail_cid TEXT,
    duration_seconds INTEGER,
    credential_proof_ids TEXT NOT NULL,                            -- JSON array of skill_proof IDs the author stakes
    signature TEXT NOT NULL,                                       -- Ed25519 over the canonical payload
    public_key TEXT,                                               -- Ed25519 public key (hex) for verification
    published_at TEXT NOT NULL,
    received_at TEXT NOT NULL DEFAULT (datetime('now')),
    withdrawn INTEGER NOT NULL DEFAULT 0,
    withdrawn_reason TEXT,                                         -- e.g. 'challenge_upheld'
    on_chain_tx TEXT,                                              -- optional: future DAO-attested anchor
    provenance TEXT
);

CREATE TABLE opinions_pending_verification (
    id TEXT PRIMARY KEY,
    author_address TEXT NOT NULL,
    subject_field_id TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT,
    video_cid TEXT NOT NULL,
    thumbnail_cid TEXT,
    duration_seconds INTEGER,
    credential_proof_ids TEXT NOT NULL,
    signature TEXT NOT NULL,
    public_key TEXT,
    published_at TEXT NOT NULL,
    queued_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE organizations (
    id TEXT PRIMARY KEY,                                 -- blake2b(name + owner_address)
    name TEXT NOT NULL,
    owner_address TEXT NOT NULL,                         -- sponsor admin stake address
    did TEXT,                                            -- optional org issuer DID
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE peer_profiles (
    did TEXT PRIMARY KEY,
    username TEXT,
    display_name TEXT,
    bio TEXT,
    avatar_cid TEXT,
    visibility TEXT NOT NULL DEFAULT 'public',
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE peers (
    peer_id TEXT PRIMARY KEY,  -- libp2p PeerId
    stake_address TEXT,        -- Cardano stake address (if known)
    display_name TEXT,
    last_seen TEXT NOT NULL,
    addresses TEXT NOT NULL,   -- JSON array of multiaddrs
    roles TEXT,                -- JSON array: ["instructor", "learner"]
    reputation REAL
);

CREATE TABLE pending_pairings (
    code_hash TEXT PRIMARY KEY,                          -- BLAKE2b hash of the pairing code
    shared_key BLOB NOT NULL,                            -- 32-byte key offered to the acceptor
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL
);

CREATE TABLE pinboard_observations (
    id TEXT PRIMARY KEY,
    pinner_did TEXT NOT NULL,
    subject_did TEXT NOT NULL,
    scope TEXT NOT NULL,                                  -- JSON array of strings
    commitment_since TEXT NOT NULL,
    revoked_at TEXT,
    signature TEXT NOT NULL,
    public_key TEXT NOT NULL,
    received_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE pins (
    cid TEXT PRIMARY KEY,
    pin_type TEXT NOT NULL,                             -- course|evidence|profile|taxonomy
    size_bytes INTEGER,
    last_accessed TEXT,
    auto_unpin INTEGER DEFAULT 0,                       -- 1 = ok to unpin under storage pressure
    pinned_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE plugin_catalog (
    plugin_cid TEXT PRIMARY KEY,                           -- BLAKE3 of manifest.json
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    author_did TEXT NOT NULL,
    description TEXT,
    api_version TEXT NOT NULL,
    kinds_json TEXT NOT NULL,                              -- JSON array
    capabilities_json TEXT NOT NULL,                       -- JSON array
    subject_tags_json TEXT NOT NULL,                       -- JSON array
    platforms_json TEXT NOT NULL,                          -- JSON array
    has_grader INTEGER NOT NULL DEFAULT 0,
    grader_cid TEXT,                                       -- NULL for interactive-only
    source TEXT NOT NULL,                                  -- 'gossip' | 'builtin' | 'local'
    announced_at TEXT NOT NULL,                            -- author-stamped time from announcement
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE plugin_dependencies (
    plugin_cid TEXT NOT NULL REFERENCES plugin_installed(plugin_cid) ON DELETE CASCADE,
    dependency_id TEXT NOT NULL,                                                             -- manifest id: did:key:<author>#<slug>
    dependency_cid TEXT NOT NULL REFERENCES plugin_installed(plugin_cid) ON DELETE CASCADE,
    PRIMARY KEY (plugin_cid, dependency_id)
);

CREATE TABLE plugin_element_state (
    element_id TEXT PRIMARY KEY,
    plugin_cid TEXT NOT NULL,
    state_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE plugin_installed (
    plugin_cid TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    author_did TEXT NOT NULL,
    install_path TEXT NOT NULL,                            -- filesystem path under app_data/plugins/
    source TEXT NOT NULL,                                  -- 'local_file' | 'p2p' | 'builtin'
    manifest_json TEXT NOT NULL,                           -- full manifest at install time
    installed_at TEXT NOT NULL DEFAULT (datetime('now')),
    enabled INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE plugin_irl_submissions (
    id TEXT PRIMARY KEY,
    plugin_cid TEXT NOT NULL REFERENCES plugin_installed(plugin_cid) ON DELETE CASCADE,
    element_id TEXT,
    enrollment_id TEXT,
    learner_did TEXT NOT NULL,
    submission_json TEXT NOT NULL,
    skills_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL CHECK (status IN ('pending','reviewed','rejected')) DEFAULT 'pending',
    reviewer_did TEXT,
    score REAL,
    feedback TEXT,
    skill_ratings_json TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    reviewed_at TEXT,
    course_id TEXT
);

CREATE TABLE plugin_permissions (
    plugin_cid TEXT NOT NULL REFERENCES plugin_installed(plugin_cid) ON DELETE CASCADE,
    capability TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('once','session','always')),
    granted_at TEXT NOT NULL DEFAULT (datetime('now')),
    granted_until TEXT,                                                                  -- NULL for 'always'
    PRIMARY KEY (plugin_cid, capability)
);

CREATE TABLE presentations_seen (
    audience TEXT NOT NULL,
    nonce TEXT NOT NULL,
    seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (audience, nonce)
);

CREATE TABLE question_banks (
    id TEXT PRIMARY KEY,
    skill_id TEXT NOT NULL,
    label TEXT NOT NULL,
    pass_threshold REAL NOT NULL DEFAULT 0.7,                                                    -- fraction correct to pass
    draw_count INTEGER NOT NULL DEFAULT 5,                                                       -- questions per attempt
    taxonomy_version TEXT,
    dao_id TEXT,
    ratified INTEGER NOT NULL DEFAULT 0,
    content_cid TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    max_attempts INTEGER,
    cooldown_hours TEXT NOT NULL DEFAULT '[0,24,72,168]',
    attempt_window_days INTEGER NOT NULL DEFAULT 90,
    score_policy TEXT NOT NULL DEFAULT 'best',
    delivery_mode TEXT NOT NULL DEFAULT 'fixed' CHECK (delivery_mode IN ('fixed', 'adaptive')),
    adaptive_se_target REAL NOT NULL DEFAULT 0.3,
    adaptive_min_items INTEGER NOT NULL DEFAULT 5,
    adaptive_max_items INTEGER NOT NULL DEFAULT 20
);

CREATE TABLE reputation_assertions (
    id TEXT PRIMARY KEY,
    actor_address TEXT NOT NULL,                                                                                            -- Cardano stake address
    role TEXT NOT NULL,                                                                                                     -- instructor|learner|assessor|author|mentor
    skill_id TEXT REFERENCES skills(id),
    proficiency_level TEXT,
    score REAL NOT NULL,
    evidence_count INTEGER NOT NULL DEFAULT 0,
    median_impact REAL,
    impact_p25 REAL,
    impact_p75 REAL,
    learner_count INTEGER,
    impact_variance REAL,
    window_start TEXT,
    window_end TEXT,
    computation_spec TEXT NOT NULL DEFAULT 'v2',
    cid TEXT,                                                                                                               -- content ID (BLAKE3 hash) of reputation proof
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    input_policy_state TEXT NOT NULL DEFAULT 'valid' CHECK (input_policy_state IN ('valid', 'needs_refresh', 'excluded')),
    input_fingerprint TEXT
);

CREATE TABLE reputation_snapshot_inputs (
    snapshot_id TEXT PRIMARY KEY REFERENCES reputation_snapshots(id),
    context_json TEXT NOT NULL CHECK (json_valid(context_json))
);

CREATE TABLE reputation_snapshots (
    id TEXT PRIMARY KEY,
    actor_address TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    role TEXT NOT NULL,
    skill_count INTEGER NOT NULL DEFAULT 0,
    tx_status TEXT NOT NULL DEFAULT 'pending',            -- pending|building|submitted|confirmed|failed
    tx_hash TEXT,
    error_message TEXT,
    snapshot_at TEXT NOT NULL DEFAULT (datetime('now')),
    confirmed_at TEXT,
    computation_spec TEXT,
    credential_id TEXT REFERENCES credentials(id)
);

CREATE TABLE role_assessments (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    role_title TEXT NOT NULL,                                             -- e.g. "SRE L4"
    job_description TEXT,                                                 -- JD text
    course_id TEXT REFERENCES courses(id),                                -- backing assessment
    skill_ids TEXT,                                                       -- JSON array of required skill ids
    issuance_policy_json TEXT,                                            -- serialized IssuancePolicy (P0)
    required_assurance_level TEXT,                                        -- local|anchored|high_assurance
    status TEXT NOT NULL DEFAULT 'draft',                                 -- draft|published|archived
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE sentinel_holdout_refs (
    id TEXT PRIMARY KEY,
    encrypted_cid TEXT NOT NULL,
    model_kind TEXT NOT NULL,                            -- 'keystroke' | 'mouse'
    threshold INTEGER NOT NULL,
    key_policy TEXT NOT NULL,                            -- JSON: sealed-share envelope
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE sentinel_user_models (
    user_address TEXT NOT NULL,
    device_fp_prefix TEXT NOT NULL,
    model_kind TEXT NOT NULL,
    weights_json TEXT NOT NULL,
    train_loss REAL,
    trained_epochs INTEGER NOT NULL DEFAULT 0,
    training_samples INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_address, device_fp_prefix, model_kind)
);

CREATE TABLE skill_prerequisites (
    skill_id TEXT NOT NULL REFERENCES skills(id),
    prerequisite_id TEXT NOT NULL REFERENCES skills(id),
    PRIMARY KEY (skill_id, prerequisite_id),
    CHECK (skill_id != prerequisite_id)
);

CREATE TABLE skill_relations (
    skill_id TEXT NOT NULL REFERENCES skills(id),
    related_skill_id TEXT NOT NULL REFERENCES skills(id),
    relation_type TEXT NOT NULL DEFAULT 'related',         -- related|complementary|alternative
    PRIMARY KEY (skill_id, related_skill_id),
    CHECK (skill_id != related_skill_id)
);

CREATE TABLE skills (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    subject_id TEXT NOT NULL REFERENCES subjects(id),
    bloom_level TEXT NOT NULL DEFAULT 'apply',           -- remember|understand|apply|analyze|evaluate|create
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    synonyms TEXT
);

CREATE TABLE stake_pubkey_registry (
    stake_address TEXT NOT NULL,                                   -- Cardano stake addr (bech32)
    public_key_hex TEXT NOT NULL,                                  -- Ed25519 libp2p pubkey, lowercase hex
    valid_from INTEGER NOT NULL,                                   -- unix secs
    valid_until INTEGER,                                           -- unix secs, NULL = open-ended
    source TEXT NOT NULL CHECK (source IN ('chain', 'snapshot')),
    on_chain_tx TEXT,                                              -- tx hash, NULL for snapshot-only
    snapshot_sig TEXT,                                             -- multisig hex, NULL for chain rows
    last_verified INTEGER NOT NULL DEFAULT 0,                      -- unix secs of last chain re-check
    PRIMARY KEY (stake_address, public_key_hex, valid_from)
);

CREATE TABLE subject_fields (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    icon_emoji TEXT
);

CREATE TABLE subjects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    subject_field_id TEXT NOT NULL REFERENCES subject_fields(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE sync_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL,                          -- evidence|catalog|taxonomy|governance
    entity_id TEXT NOT NULL,
    direction TEXT NOT NULL,                            -- sent|received
    peer_id TEXT,                                       -- Which peer (null = broadcast)
    signature TEXT,
    synced_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE sync_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_name TEXT NOT NULL,
    row_id TEXT NOT NULL,                               -- PK of the changed row
    operation TEXT NOT NULL,                            -- insert|update|delete
    row_data TEXT,                                      -- JSON snapshot of the row (null for delete)
    updated_at TEXT NOT NULL,                           -- Timestamp of the change (LWW tiebreaker)
    queued_at TEXT NOT NULL DEFAULT (datetime('now')),
    delivered_to TEXT DEFAULT '[]'                      -- JSON array of device_ids that received it
);

CREATE TABLE sync_state (
    device_id TEXT NOT NULL REFERENCES devices(id),
    table_name TEXT NOT NULL,                        -- enrollments|element_progress|course_notes|evidence_records|skill_proof_evidence
    last_synced_at TEXT NOT NULL,                    -- ISO 8601 timestamp of last sync
    row_count INTEGER NOT NULL DEFAULT 0,            -- Number of rows synced
    PRIMARY KEY (device_id, table_name)
);

CREATE TABLE taxonomy_versions (
    version INTEGER PRIMARY KEY,
    cid TEXT NOT NULL,                                   -- content ID (BLAKE3 hash) of the full taxonomy document
    previous_cid TEXT,                                   -- CID of the previous version
    ratified_by TEXT,                                    -- DAO committee multisig info
    ratified_at TEXT,
    signature TEXT,                                      -- Ed25519 signature
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE tutoring_sessions (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    ticket TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'ended', 'cancelled')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    ended_at TEXT
);

CREATE TABLE username_claims (
    username TEXT PRIMARY KEY,
    did TEXT NOT NULL,
    claimed_at INTEGER NOT NULL,
    tier INTEGER NOT NULL DEFAULT 0,                     -- 0 bare | 1 receipted | 2 anchored
    claim_json TEXT NOT NULL,                            -- full UsernameClaim
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    anchor_verified INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE video_chapters (
    id TEXT PRIMARY KEY,
    element_id TEXT NOT NULL REFERENCES course_elements(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    start_seconds INTEGER NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);


CREATE INDEX idx_app_settings_scope_updated ON app_settings(scope, updated_at);
CREATE INDEX idx_assessment_attempts_history ON assessment_attempts(subject_did, skill_id, started_at DESC);
CREATE INDEX idx_assessment_attempts_open ON assessment_attempts(subject_did, started_at DESC) WHERE graded_at IS NULL AND ended_at IS NULL;
CREATE INDEX idx_assessment_attempts_subject ON assessment_attempts(subject_did, skill_id);
CREATE INDEX idx_assessment_item_skills_skill ON assessment_item_skills(skill_id);
CREATE INDEX idx_assessment_items_bank ON assessment_items(bank_id);
CREATE INDEX idx_assessment_items_skill ON assessment_items(skill_id, ratified);
CREATE INDEX idx_attempt_items_item ON attempt_items(item_id);
CREATE INDEX idx_calls_classroom ON classroom_calls(classroom_id, status);
CREATE INDEX idx_catalog_author ON catalog(author_address);
CREATE INDEX idx_catalog_kind ON catalog(kind);
CREATE INDEX idx_chain_submissions_recovery ON chain_submissions(network, operation_kind, applied_at, updated_at);
CREATE INDEX idx_chain_submissions_status ON chain_submissions(network, status);
CREATE INDEX idx_channels_classroom ON classroom_channels(classroom_id, position);
CREATE INDEX idx_classrooms_owner ON classrooms(owner_address);
CREATE INDEX idx_classrooms_status ON classrooms(status);
CREATE INDEX idx_completion_obs_pending ON completion_observations(credential_id) WHERE credential_id IS NULL;
CREATE INDEX idx_completion_obs_subject ON completion_observations(subject_pubkey);
CREATE INDEX idx_completion_witness_due ON completion_witness_requests(next_attempt_at, operation_id) WHERE blocked = 0;
CREATE INDEX idx_content_mappings_blake3 ON content_mappings(blake3_hash);
CREATE INDEX idx_course_completion_endorsements_claim ON course_completion_endorsements(claim_id, created_at);
CREATE INDEX idx_courses_author ON courses(author_address);
CREATE INDEX idx_courses_kind ON courses(kind);
CREATE INDEX idx_courses_status ON courses(status);
CREATE INDEX idx_credential_allowlist_cred ON credential_allowlist(credential_id);
CREATE INDEX idx_credential_anchors_status ON credential_anchors(anchor_status, next_attempt_at);
CREATE INDEX idx_credentials_auto_issued ON credentials(auto_issued, subject_did);
CREATE INDEX idx_credentials_issuer ON credentials(issuer_did);
CREATE INDEX idx_credentials_pending_issuer ON credentials_pending_verification(issuer_did);
CREATE INDEX idx_credentials_skill ON credentials(skill_id) WHERE skill_id IS NOT NULL;
CREATE INDEX idx_credentials_status ON credentials(status_list_id, status_list_index) WHERE status_list_id IS NOT NULL;
CREATE INDEX idx_credentials_subject ON credentials(subject_did);
CREATE INDEX idx_credentials_supersedes ON credentials(supersedes) WHERE supersedes IS NOT NULL;
CREATE INDEX idx_credentials_witness_tx ON credentials(witness_tx_hash) WHERE witness_tx_hash IS NOT NULL;
CREATE INDEX idx_derived_skill_states_skill ON derived_skill_states(skill_id);
CREATE INDEX idx_derived_skill_states_subject ON derived_skill_states(subject_did);
CREATE INDEX idx_devices_local ON devices(is_local);
CREATE INDEX idx_dss_history_subject_skill ON derived_skill_state_history(subject_did, skill_id, snapshot_date);
CREATE INDEX idx_element_progress_enrollment ON element_progress(enrollment_id);
CREATE INDEX idx_element_submissions_element ON element_submissions(element_id);
CREATE INDEX idx_element_submissions_enrollment ON element_submissions(enrollment_id);
CREATE INDEX idx_element_submissions_grader ON element_submissions(grader_cid);
CREATE INDEX idx_enrollments_course ON enrollments(course_id);
CREATE INDEX idx_enrollments_course_document ON enrollments(course_id, course_document_cid);
CREATE INDEX idx_evidence_release_owed ON integrity_evidence_release(revoke_wanted_at) WHERE revoke_wanted_at IS NOT NULL AND revoked_at IS NULL;
CREATE INDEX idx_flag_notice_untold ON integrity_flag_notice(told_at) WHERE told_at IS NULL;
CREATE UNIQUE INDEX idx_goal_templates_key ON goal_templates(kind, key);
CREATE INDEX idx_goal_templates_kind ON goal_templates(kind);
CREATE INDEX idx_governance_genesis_trust_scope ON governance_genesis_trust_anchors(scope_type, scope_id);
CREATE INDEX idx_integrity_evidence_expires ON integrity_evidence(expires_at);
CREATE INDEX idx_integrity_evidence_session ON integrity_evidence(session_id);
CREATE INDEX idx_integrity_snapshots_session ON integrity_snapshots(session_id);
CREATE INDEX idx_interview_criteria_session ON interview_criteria(session_id, position);
CREATE INDEX idx_interview_followups_session ON interview_followups(session_id, status, created_at DESC);
CREATE INDEX idx_interview_notes_session ON interview_notes(session_id, created_at);
CREATE INDEX idx_interview_participants_session ON interview_participants(session_id, created_at);
CREATE INDEX idx_interview_sessions_expiry ON interview_sessions(expires_at);
CREATE INDEX idx_interview_sessions_status ON interview_sessions(status, created_at DESC);
CREATE INDEX idx_interview_transcript_session ON interview_transcript_segments(session_id, start_ms, created_at);
CREATE INDEX idx_irl_submissions_course ON plugin_irl_submissions(course_id);
CREATE INDEX idx_irl_submissions_learner ON plugin_irl_submissions(learner_did);
CREATE INDEX idx_irl_submissions_plugin ON plugin_irl_submissions(plugin_cid);
CREATE INDEX idx_irl_submissions_status ON plugin_irl_submissions(status);
CREATE INDEX idx_join_req_address ON classroom_join_requests(stake_address);
CREATE INDEX idx_join_req_classroom ON classroom_join_requests(classroom_id, status);
CREATE UNIQUE INDEX idx_join_requests_unique_pending ON classroom_join_requests(classroom_id, stake_address) WHERE status = 'pending';
CREATE INDEX idx_key_registry_active ON key_registry(did) WHERE valid_until IS NULL;
CREATE INDEX idx_key_registry_did_valid_from ON key_registry(did, valid_from);
CREATE INDEX idx_members_address ON classroom_members(stake_address);
CREATE INDEX idx_members_classroom ON classroom_members(classroom_id);
CREATE INDEX idx_messages_channel ON classroom_messages(channel_id, sent_at);
CREATE INDEX idx_messages_classroom ON classroom_messages(classroom_id, sent_at);
CREATE INDEX idx_opinions_author ON opinions(author_address);
CREATE INDEX idx_opinions_pending_author ON opinions_pending_verification(author_address);
CREATE INDEX idx_opinions_subject ON opinions(subject_field_id, published_at DESC);
CREATE INDEX idx_opinions_withdrawn ON opinions(withdrawn);
CREATE INDEX idx_organizations_owner ON organizations(owner_address);
CREATE INDEX idx_peer_profiles_username ON peer_profiles(username);
CREATE INDEX idx_peers_last_seen ON peers(last_seen);
CREATE INDEX idx_pinboard_observations_active ON pinboard_observations(subject_did) WHERE revoked_at IS NULL;
CREATE INDEX idx_pinboard_observations_pinner ON pinboard_observations(pinner_did);
CREATE INDEX idx_pinboard_observations_subject ON pinboard_observations(subject_did);
CREATE INDEX idx_plugin_catalog_author ON plugin_catalog(author_did);
CREATE INDEX idx_plugin_catalog_source ON plugin_catalog(source);
CREATE INDEX idx_plugin_dependencies_dep ON plugin_dependencies(dependency_cid);
CREATE INDEX idx_plugin_installed_author ON plugin_installed(author_did);
CREATE INDEX idx_presentations_seen_audience ON presentations_seen(audience);
CREATE INDEX idx_question_banks_skill ON question_banks(skill_id);
CREATE INDEX idx_registry_address_window ON stake_pubkey_registry(stake_address, valid_from, valid_until);
CREATE INDEX idx_reputation_actor ON reputation_assertions(actor_address);
CREATE INDEX idx_reputation_role_skill ON reputation_assertions(role, skill_id, proficiency_level);
CREATE INDEX idx_reputation_skill ON reputation_assertions(skill_id);
CREATE UNIQUE INDEX idx_reputation_snapshots_credential ON reputation_snapshots(credential_id) WHERE credential_id IS NOT NULL;
CREATE INDEX idx_role_assessments_org ON role_assessments(org_id);
CREATE INDEX idx_sentinel_holdout_kind ON sentinel_holdout_refs(model_kind);
CREATE INDEX idx_sentinel_user_models_kind ON sentinel_user_models(model_kind);
CREATE INDEX idx_snapshots_actor ON reputation_snapshots(actor_address);
CREATE INDEX idx_snapshots_status ON reputation_snapshots(tx_status);
CREATE INDEX idx_snapshots_subject ON reputation_snapshots(subject_id);
CREATE INDEX idx_status_lists_issuer ON credential_status_lists(issuer_did);
CREATE INDEX idx_sync_log_entity ON sync_log(entity_type, entity_id);
CREATE INDEX idx_sync_queue_queued ON sync_queue(queued_at);
CREATE INDEX idx_sync_queue_table ON sync_queue(table_name);
CREATE INDEX idx_username_claims_did ON username_claims(did);
CREATE INDEX idx_video_chapters_element ON video_chapters(element_id, position);

CREATE VIEW current_reputation_assertions AS
SELECT * FROM reputation_assertions WHERE input_policy_state = 'valid';

CREATE TRIGGER completion_request_existing_journal AFTER INSERT ON completion_witness_requests
WHEN EXISTS (SELECT 1 FROM chain_submissions WHERE network = 'cardano-preprod'
    AND operation_kind = 'completion_witness' AND operation_id = NEW.operation_id)
BEGIN
    UPDATE completion_witness_requests SET blocked = 1 WHERE operation_id = NEW.operation_id;
END;

CREATE TRIGGER completion_request_journal_handoff AFTER INSERT ON chain_submissions
WHEN NEW.network = 'cardano-preprod' AND NEW.operation_kind = 'completion_witness'
BEGIN
    UPDATE completion_witness_requests SET blocked = 1 WHERE operation_id = NEW.operation_id;
END;

CREATE TRIGGER snapshot_submission_checkpoint AFTER INSERT ON chain_submissions
WHEN NEW.network = 'cardano-preprod' AND NEW.operation_kind = 'reputation_snapshot'
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM reputation_snapshot_inputs WHERE snapshot_id = NEW.operation_id
            AND context_json = NEW.context_json
    ) THEN RAISE(ABORT, 'snapshot checkpoint differs from frozen inputs') END;
    SELECT CASE WHEN EXISTS (
        SELECT 1 FROM reputation_snapshots WHERE id = NEW.operation_id
            AND tx_hash IS NOT NULL AND tx_hash != NEW.tx_hash
    ) THEN RAISE(ABORT, 'snapshot already has a different transaction') END;
    UPDATE reputation_snapshots SET tx_status = 'outcome_unknown', tx_hash = NEW.tx_hash,
        error_message = NULL, confirmed_at = NULL WHERE id = NEW.operation_id;
END;
"#;
