-- Probes: short follow-ups produced from a learner's own submission after an
-- attempt is submitted, inside its integrity session, and the verification
-- results they leave in that session's commitment chain.

-- An assessment's probe policy: none | reseed | authored | plugin. Checked in
-- Rust (`assessment::probes::ProbePolicy`) so a new policy needs no table
-- rebuild. A role's policy lives in its issuance policy JSON.
ALTER TABLE question_banks ADD COLUMN probe_policy TEXT NOT NULL DEFAULT 'none';

-- Instructor-written follow-ups and seeded variants of an item. Same content
-- split as the item: `content_public` may reach a client, `grader_private`
-- never does. Graded by the parent item's grader. A probe written by the
-- learner being probed is never served to them.
CREATE TABLE assessment_item_probes (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL REFERENCES assessment_items(id) ON DELETE CASCADE,
    probe_kind TEXT NOT NULL CHECK (probe_kind IN ('follow_up', 'variant')),
    content_public TEXT NOT NULL CHECK (json_valid(content_public)),
    grader_private TEXT CHECK (grader_private IS NULL OR json_valid(grader_private)),
    time_box_ms INTEGER CHECK (time_box_ms IS NULL OR time_box_ms > 0),
    author_did TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_assessment_item_probes_item ON assessment_item_probes(item_id, probe_kind);

-- Probes served for an attempt. The question and expectation are copied in
-- when the probe is generated, so editing an item or an authored probe later
-- cannot change what was asked. Times are milliseconds since the epoch on the
-- host clock; the time box starts at `rendered_at_ms`.
CREATE TABLE assessment_probes (
    id TEXT PRIMARY KEY,
    attempt_id TEXT NOT NULL REFERENCES assessment_attempts(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    policy TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('reseed', 'variant', 'authored', 'plugin')),
    source_id TEXT,
    item_kind TEXT NOT NULL CHECK (item_kind IN ('mcq', 'plugin')),
    plugin_cid TEXT,
    question_json TEXT NOT NULL CHECK (json_valid(question_json)),
    expectation_json TEXT CHECK (expectation_json IS NULL OR json_valid(expectation_json)),
    option_order TEXT,
    time_box_ms INTEGER NOT NULL CHECK (time_box_ms > 0),
    pass_score REAL NOT NULL,
    rendered_at_ms INTEGER,
    answered_at_ms INTEGER,
    first_input_ms INTEGER,
    response_json TEXT,
    score REAL,
    grader_cid TEXT,
    content_cid TEXT,
    submission_cid TEXT,
    outcome TEXT CHECK (outcome IS NULL OR outcome IN ('satisfied', 'failed', 'skipped')),
    UNIQUE (attempt_id, ordinal)
);

-- Verification results, each folded into its session's commitment chain.
-- `step` is what was verified (`probe` today); `policy` is how. Skipped never
-- counts against the learner.
CREATE TABLE integrity_verifications (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES integrity_sessions(id) ON DELETE CASCADE,
    attempt_id TEXT,
    step TEXT NOT NULL,
    policy TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('satisfied', 'failed', 'skipped')),
    reason TEXT,
    subject_ref TEXT,
    detail_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(detail_json)),
    commitment_hash TEXT NOT NULL,
    recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_integrity_verifications_session ON integrity_verifications(session_id);
