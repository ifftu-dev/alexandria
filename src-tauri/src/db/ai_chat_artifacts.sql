-- Study artifacts from chat: notes, flashcard sets, and goal proposals.
--
-- A proposal waits for the learner and is removed with its chat. A saved
-- artifact outlives the chat; its thread and message links become NULL and
-- it keeps the sources the originating answer carried. A goal proposal is
-- never saved here: once the learner adds it through the Goals service it
-- is marked 'added' (the goal itself lives in the learner's settings) and is
-- removed with its chat like any proposal.
CREATE TABLE chat_artifacts (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('note', 'flashcards', 'goal')),
    status TEXT NOT NULL CHECK (status IN ('proposed', 'saved', 'added')),
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
