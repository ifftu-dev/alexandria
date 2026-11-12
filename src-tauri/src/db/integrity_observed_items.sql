-- Response-process coverage by item: each item id the response tracker
-- reported a visit to, once per session. The `process` rung compares these
-- with the attempt's served items, so a repeated or unserved id cannot stand
-- in for an item the learner was never seen working.
-- `integrity_sessions.response_items` becomes the count of these rows.
CREATE TABLE integrity_observed_items (
    session_id TEXT NOT NULL REFERENCES integrity_sessions(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    PRIMARY KEY (session_id, item_id)
) WITHOUT ROWID;
