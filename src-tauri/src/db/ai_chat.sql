CREATE TABLE chat_threads (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT '',
    mode TEXT NOT NULL DEFAULT 'ask' CHECK (mode IN ('ask', 'learn', 'practice')),
    connection_id TEXT,
    model_id TEXT,
    -- Bumped by every change that alters what the next request would contain:
    -- new messages and attachment changes. A prepared request is bound to it.
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    -- Digest of the destination and attachment scope the learner last
    -- reviewed. A different scope needs a fresh review before sending.
    reviewed_scope TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_chat_threads_updated ON chat_threads(updated_at DESC);

CREATE TABLE chat_messages (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
    -- One position per user turn; its assistant versions share it.
    position INTEGER NOT NULL CHECK (position >= 0),
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    reply_to TEXT REFERENCES chat_messages(id) ON DELETE CASCADE,
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    content TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK (status IN ('complete', 'streaming', 'interrupted', 'failed', 'cancelled')),
    error TEXT,
    connection_id TEXT,
    connection_name TEXT,
    model_id TEXT,
    location TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    -- Sources the request that produced this answer actually carried, so a
    -- citation can be checked against what was sent.
    sources_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(sources_json)),
    created_at TEXT NOT NULL,
    completed_at TEXT,
    CHECK ((role = 'user') = (reply_to IS NULL))
);
CREATE UNIQUE INDEX idx_chat_messages_turn ON chat_messages(thread_id, position, role, version);

CREATE TABLE chat_bindings (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('lesson')),
    course_id TEXT NOT NULL,
    element_id TEXT NOT NULL,
    label TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (thread_id, kind, course_id, element_id)
);

CREATE TABLE chat_excerpts (
    hash TEXT PRIMARY KEY,
    text TEXT NOT NULL
);

CREATE TABLE chat_snapshots (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
    digest TEXT NOT NULL,
    manifest_json TEXT NOT NULL CHECK (json_valid(manifest_json)),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed_at TEXT
);
CREATE INDEX idx_chat_snapshots_thread ON chat_snapshots(thread_id);

CREATE TABLE chat_snapshot_excerpts (
    snapshot_id TEXT NOT NULL REFERENCES chat_snapshots(id) ON DELETE CASCADE,
    hash TEXT NOT NULL REFERENCES chat_excerpts(hash),
    PRIMARY KEY (snapshot_id, hash)
);

CREATE TABLE chat_runs (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
    client_request_id TEXT NOT NULL UNIQUE,
    user_message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    assistant_message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    snapshot_id TEXT NOT NULL REFERENCES chat_snapshots(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('streaming', 'complete', 'failed', 'cancelled', 'interrupted')),
    started_at TEXT NOT NULL,
    finished_at TEXT
);
-- One generation at a time per thread keeps message order unambiguous.
CREATE UNIQUE INDEX idx_chat_runs_active ON chat_runs(thread_id) WHERE state = 'streaming';

CREATE TABLE chat_drafts (
    -- A thread id, or 'new' for the composer before a thread exists.
    scope TEXT PRIMARY KEY,
    text TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
