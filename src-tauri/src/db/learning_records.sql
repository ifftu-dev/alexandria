-- Learning records: what the app records about the learner's own activity,
-- kept as learning events and built into xAPI statements for whoever reads
-- them (the learner's activity log, a record destination, a record share).
-- Device-local: none of these tables sync. See `crate::learning_records`.

-- The DID that created a course, from its verified document. Activity
-- identifiers in learning records are anchored to it. NULL for a course this
-- profile authored, whose creator is the profile's own DID.
ALTER TABLE courses ADD COLUMN creator_did TEXT;

CREATE TABLE learning_records (
    seq INTEGER PRIMARY KEY,                                                        -- write order; the oldest record has the lowest
    id TEXT NOT NULL UNIQUE,                                                        -- the record's stable local id; each recipient's statement id is a keyed hash of it
    learner_did TEXT NOT NULL,                                                      -- the learner the record is about, named by DID on the device
    kind TEXT NOT NULL CHECK (kind IN ('completion', 'interaction')),               -- interaction: a response a package reported, sent only under a scope that names it
    verb TEXT NOT NULL,                                                             -- the event's verb (registered, launched, progressed, completed, passed, failed, scored, responded)
    course_id TEXT NOT NULL,
    element_id TEXT,                                                                -- NULL for a course-level record
    registration TEXT,                                                              -- the local enrolment or package attempt the record belongs to
    activity_id TEXT NOT NULL,                                                      -- the activity's identifier (a DID URL under the course's creator)
    summary INTEGER NOT NULL DEFAULT 0 CHECK (summary IN (0, 1)),                   -- 1: kept as the one summary of its attempt at this activity when the log was over its limit
    event_json TEXT NOT NULL CHECK (json_valid(event_json)),                        -- the learning event, built into a statement per recipient
    bytes INTEGER NOT NULL CHECK (bytes >= 0),                                      -- size counted against the log's limit
    recorded_at TEXT NOT NULL                                                       -- when the event happened, RFC 3339 UTC
);

CREATE INDEX idx_learning_records_kind ON learning_records(kind, seq);

-- Per recipient, the records queued for it and how sending them went. Filled
-- and drained by the sending code; a recipient is a record destination or a
-- record share, named by an identifier that code owns.
CREATE TABLE learning_record_outbox (
    recipient_id TEXT NOT NULL,
    record_id TEXT NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
    state TEXT NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'sent', 'failed')), -- failed: the last try failed; retried at next_attempt_at
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),                      -- tries so far
    last_error TEXT,                                                                -- why the last try failed, shown in the activity log
    next_attempt_at TEXT,                                                           -- when to try again after a failure
    sent_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (recipient_id, record_id)
) WITHOUT ROWID;

CREATE INDEX idx_learning_record_outbox_record ON learning_record_outbox(record_id, state);
