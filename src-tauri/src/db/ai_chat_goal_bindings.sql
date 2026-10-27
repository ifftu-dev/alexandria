-- Goals can be attached to a chat as well as lessons. SQLite cannot change
-- a CHECK constraint in place, so the table is rebuilt. Nothing references
-- chat_bindings by key.
--
-- A goal binding stores the goal's id (from the learner.targets setting)
-- in element_id and leaves course_id empty.
CREATE TABLE chat_bindings_next (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('lesson', 'goal')),
    course_id TEXT NOT NULL,
    element_id TEXT NOT NULL,
    label TEXT NOT NULL,
    created_at TEXT NOT NULL,
    CHECK ((kind = 'goal') = (course_id = '')),
    UNIQUE (thread_id, kind, course_id, element_id)
);
INSERT INTO chat_bindings_next (id, thread_id, kind, course_id, element_id, label, created_at)
SELECT id, thread_id, kind, course_id, element_id, label, created_at FROM chat_bindings;
DROP TABLE chat_bindings;
ALTER TABLE chat_bindings_next RENAME TO chat_bindings;
