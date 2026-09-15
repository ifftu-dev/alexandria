CREATE TABLE studio_documents (
    kind TEXT NOT NULL CHECK(kind IN ('course','settings','workflow','connection','run')),
    id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision > 0),
    value TEXT NOT NULL,
    PRIMARY KEY(kind,id)
);
CREATE TABLE studio_secrets (
    connection_id TEXT PRIMARY KEY,
    secret TEXT NOT NULL
);
CREATE TABLE course_tutor_policies (
    course_id TEXT PRIMARY KEY REFERENCES courses(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 0,
    guidance TEXT NOT NULL DEFAULT 'socratic'
        CHECK (guidance IN ('socratic', 'balanced', 'direct')),
    initial_prompt TEXT NOT NULL DEFAULT ''
);
CREATE TABLE studio_tutor_threads (
    id TEXT PRIMARY KEY,
    course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    element_id TEXT NOT NULL REFERENCES course_elements(id) ON DELETE CASCADE,
    connection_id TEXT NOT NULL,
    messages TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(course_id, element_id)
);
CREATE TABLE course_lesson_feedback (
    id TEXT PRIMARY KEY,
    enrollment_id TEXT NOT NULL REFERENCES enrollments(id) ON DELETE CASCADE,
    course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    element_id TEXT NOT NULL REFERENCES course_elements(id) ON DELETE CASCADE,
    rating INTEGER NOT NULL CHECK(rating BETWEEN 1 AND 5),
    comment TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(enrollment_id, element_id)
);
CREATE INDEX idx_course_lesson_feedback_course_element
    ON course_lesson_feedback(course_id, element_id, created_at DESC);
