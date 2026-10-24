-- Ward side: a learner under 18 asks a linked guardian to approve a remote
-- model connection. An approval covers one connection at one endpoint; if
-- the endpoint changes, the learner asks again. The latest decision applies.
CREATE TABLE ai_connection_approvals (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL,
    connection_name TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    model TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'declined')),
    decided_by_link TEXT,
    requested_at TEXT NOT NULL,
    decided_at TEXT
);
CREATE INDEX idx_ai_connection_approvals_connection
    ON ai_connection_approvals(connection_id, endpoint, requested_at);

-- Guardian side: requests received from a ward over the sealed guardian
-- link, and the guardian's decision until the ward acknowledges it.
CREATE TABLE guardian_ai_requests (
    id TEXT NOT NULL,
    link_id TEXT NOT NULL REFERENCES guardian_links(id) ON DELETE CASCADE,
    connection_name TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    model TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'declined')),
    received_at TEXT NOT NULL,
    decided_at TEXT,
    -- The current decision reached the ward.
    delivered INTEGER NOT NULL DEFAULT 0 CHECK (delivered IN (0, 1)),
    PRIMARY KEY (id, link_id)
);
