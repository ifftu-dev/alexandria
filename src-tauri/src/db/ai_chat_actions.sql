-- Chat actions. Besides notes, flashcards, and goal proposals, a chat may
-- propose a note for the attached lesson's course notes ('course_note') or
-- enrolling in a course on an attached goal's path ('enroll'). Every
-- proposal waits for the learner to allow or decline it. A declined one stays,
-- marked 'declined', so the chat keeps a record of what was asked and what
-- was decided; it goes with its chat like the others.
--
-- SQLite cannot change a CHECK constraint, so the table is rebuilt.
CREATE TABLE chat_artifacts_next (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('note', 'flashcards', 'goal', 'goal_change', 'course_note', 'enroll')),
    status TEXT NOT NULL CHECK (status IN ('proposed', 'saved', 'added', 'declined')),
    title TEXT NOT NULL,
    body_json TEXT NOT NULL CHECK (json_valid(body_json)),
    sources_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(sources_json)),
    thread_id TEXT REFERENCES chat_threads(id) ON DELETE SET NULL,
    message_id TEXT REFERENCES chat_messages(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
INSERT INTO chat_artifacts_next
    SELECT id, kind, status, title, body_json, sources_json, thread_id, message_id, created_at, updated_at
      FROM chat_artifacts;
DROP TABLE chat_artifacts;
ALTER TABLE chat_artifacts_next RENAME TO chat_artifacts;
CREATE INDEX idx_chat_artifacts_status ON chat_artifacts(status, updated_at);
CREATE INDEX idx_chat_artifacts_thread ON chat_artifacts(thread_id);
