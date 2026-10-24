-- Course notes carry their own text, so a note syncs whole between the
-- learner's devices (the table syncs every column). `origin` is 'chat' for a
-- note saved from chat. A deleted note keeps its row as a tombstone with its
-- text cleared: sync sends full snapshots without deletions, so removing the
-- row would let another device's copy bring it back.
ALTER TABLE course_notes ADD COLUMN body TEXT;
ALTER TABLE course_notes ADD COLUMN origin TEXT;
ALTER TABLE course_notes ADD COLUMN deleted_at TEXT;
CREATE INDEX idx_course_notes_enrollment ON course_notes(enrollment_id, updated_at);
