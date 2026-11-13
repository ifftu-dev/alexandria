-- Generated probes (`llm` policy): a sponsor-configured model connection
-- writes a follow-up from the learner's own answer. See
-- `crate::assessment::llm_probe`.

-- Whether the learner agreed, at attempt start, that their answers and the
-- skill description may leave the device to generate follow-ups. Without it
-- a generated probe is skipped and nothing is sent.
ALTER TABLE assessment_attempts ADD COLUMN llm_probe_consent INTEGER NOT NULL DEFAULT 0
    CHECK (llm_probe_consent IN (0, 1));

-- `assessment_probes.source` gains 'llm', and each generated probe keeps how
-- it was generated (model id, prompt hash, latency, format) so every result
-- recorded for it carries the same record into the commitment chain. The
-- source CHECK cannot be altered in place, so the table is rebuilt.
CREATE TABLE assessment_probes_next (
    id TEXT PRIMARY KEY,
    attempt_id TEXT NOT NULL REFERENCES assessment_attempts(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    policy TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('reseed', 'variant', 'authored', 'plugin', 'llm')),
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
    generation_json TEXT CHECK (generation_json IS NULL OR json_valid(generation_json)),
    UNIQUE (attempt_id, ordinal)
);
INSERT INTO assessment_probes_next
    (id, attempt_id, ordinal, item_id, policy, source, source_id, item_kind, plugin_cid,
     question_json, expectation_json, option_order, time_box_ms, pass_score, rendered_at_ms,
     answered_at_ms, first_input_ms, response_json, score, grader_cid, content_cid,
     submission_cid, outcome)
SELECT id, attempt_id, ordinal, item_id, policy, source, source_id, item_kind, plugin_cid,
       question_json, expectation_json, option_order, time_box_ms, pass_score, rendered_at_ms,
       answered_at_ms, first_input_ms, response_json, score, grader_cid, content_cid,
       submission_cid, outcome
  FROM assessment_probes;
DROP TABLE assessment_probes;
ALTER TABLE assessment_probes_next RENAME TO assessment_probes;
