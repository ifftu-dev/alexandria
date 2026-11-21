-- cmi5 sessions: the launching system's side of a cmi5 package attempt.
-- The unit talks xAPI to an on-device endpoint that exists only while its
-- session is open; these tables are what that endpoint reads and writes.
-- Device-local: none of these tables sync. See `crate::packages::attempts::cmi5`.

-- One launch of one assignable unit. A session left open when the app
-- stopped is abandoned at the next launch of the attempt.
CREATE TABLE cmi5_sessions (
    id TEXT PRIMARY KEY,                                                            -- the session identifier the unit was given (UUID)
    attempt_id TEXT NOT NULL REFERENCES package_attempts(id) ON DELETE CASCADE,
    activity_id TEXT NOT NULL,                                                      -- the unit's identifier in the course structure
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'terminated', 'abandoned')),
    launched_at TEXT NOT NULL,
    last_statement_at TEXT,                                                         -- when the unit last sent a statement that was accepted
    ended_at TEXT
);

CREATE INDEX idx_cmi5_sessions_attempt ON cmi5_sessions(attempt_id, status);

-- Every statement the endpoint accepted or the launching system wrote, as
-- the unit sees it: its actor is the pseudonymous identifier the unit was
-- given. Never sent anywhere; learning records carry what leaves the device.
CREATE TABLE cmi5_statements (
    seq INTEGER PRIMARY KEY,                                                        -- the order statements were stored in
    id TEXT NOT NULL UNIQUE,                                                        -- the statement id (UUID)
    attempt_id TEXT NOT NULL REFERENCES package_attempts(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    activity_id TEXT NOT NULL,                                                      -- the unit, block or course the statement is about, by course structure identifier
    verb TEXT,                                                                      -- the cmi5 defined verb (launched, initialized, completed, passed, failed, terminated, abandoned, satisfied); NULL for a cmi5 allowed statement
    score REAL CHECK (score IS NULL OR (score >= 0 AND score <= 1)),               -- a passed or failed statement's score, normalised
    duration_seconds REAL CHECK (duration_seconds IS NULL OR duration_seconds >= 0),
    statement_json TEXT NOT NULL CHECK (json_valid(statement_json)),
    stored_at TEXT NOT NULL
);

CREATE INDEX idx_cmi5_statements_attempt ON cmi5_statements(attempt_id, activity_id, verb);

-- The xAPI documents cmi5 needs: activity state (LMS.LaunchData and the
-- unit's own), agent profile (cmi5LearnerPreferences) and activity profile.
-- The agent is always the actor a package's units are given, so documents
-- are keyed by the package instead.
CREATE TABLE xapi_documents (
    package_hash TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('state', 'agent_profile', 'activity_profile')),
    activity_id TEXT NOT NULL DEFAULT '',                                           -- '' for an agent profile document
    registration TEXT NOT NULL DEFAULT '',                                          -- state documents only; '' when none was named
    document_id TEXT NOT NULL,                                                      -- the stateId or profileId
    content_type TEXT NOT NULL,
    body BLOB NOT NULL,
    etag TEXT NOT NULL,                                                             -- SHA-256 of the body, quoted, for concurrency control
    updated_at TEXT NOT NULL,
    PRIMARY KEY (package_hash, kind, activity_id, registration, document_id)
) WITHOUT ROWID;
