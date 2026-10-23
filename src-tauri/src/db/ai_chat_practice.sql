-- One validated practice question, produced by an answer in Practice mode.
-- The question (including its checked answer) is stored as JSON so the
-- learner's answers are checked here, never by asking a model.
CREATE TABLE chat_practice_items (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
    message_id TEXT NOT NULL UNIQUE REFERENCES chat_messages(id) ON DELETE CASCADE,
    question_json TEXT NOT NULL CHECK (json_valid(question_json)),
    hints_used INTEGER NOT NULL DEFAULT 0 CHECK (hints_used >= 0),
    revealed INTEGER NOT NULL DEFAULT 0 CHECK (revealed IN (0, 1)),
    solved INTEGER NOT NULL DEFAULT 0 CHECK (solved IN (0, 1)),
    -- Solved with no hint and before the answer was shown.
    solved_independently INTEGER NOT NULL DEFAULT 0 CHECK (solved_independently IN (0, 1)),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_chat_practice_thread ON chat_practice_items(thread_id);

CREATE TABLE chat_practice_attempts (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL REFERENCES chat_practice_items(id) ON DELETE CASCADE,
    answer TEXT NOT NULL,
    correct INTEGER NOT NULL CHECK (correct IN (0, 1)),
    -- A hint had been used, or the answer shown, before this attempt.
    assisted INTEGER NOT NULL CHECK (assisted IN (0, 1)),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_chat_practice_attempts_item ON chat_practice_attempts(item_id, created_at);
