-- Study artifacts from chat: notes and flashcard sets.
--
-- A proposal waits for the learner and is removed with its chat. A saved
-- artifact outlives the chat; its thread and message links become NULL and
-- it keeps the sources the originating answer carried.
CREATE TABLE chat_artifacts (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('note', 'flashcards')),
    status TEXT NOT NULL CHECK (status IN ('proposed', 'saved')),
    title TEXT NOT NULL,
    body_json TEXT NOT NULL CHECK (json_valid(body_json)),
    sources_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(sources_json)),
    thread_id TEXT REFERENCES chat_threads(id) ON DELETE SET NULL,
    message_id TEXT REFERENCES chat_messages(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_chat_artifacts_status ON chat_artifacts(status, updated_at);
CREATE INDEX idx_chat_artifacts_thread ON chat_artifacts(thread_id);
