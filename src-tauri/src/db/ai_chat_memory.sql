-- What the learner chose to have chats remember. Proposals come from a
-- model and wait for the learner; only approved, active items are sent.
-- Authoritative records (proven skills, results) are never stored here.
CREATE TABLE learning_memory (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('preference', 'observation')),
    content TEXT NOT NULL CHECK (length(content) BETWEEN 1 AND 1200),
    status TEXT NOT NULL CHECK (status IN ('proposed', 'approved')),
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    -- Where a proposal came from. Unapproved proposals are deleted with
    -- their chat; approved items outlive it.
    thread_id TEXT REFERENCES chat_threads(id) ON DELETE SET NULL,
    source_message_id TEXT REFERENCES chat_messages(id) ON DELETE SET NULL,
    -- Bumped on every change, so a prepared request that carried the old
    -- text is refused as stale.
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_learning_memory_status ON learning_memory(status, updated_at);
