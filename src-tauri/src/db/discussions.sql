CREATE TABLE discussion_events (
 id TEXT PRIMARY KEY, entity_id TEXT NOT NULL, actor_did TEXT NOT NULL,
 signed_json TEXT NOT NULL, accepted INTEGER NOT NULL DEFAULT 0,
 received_at INTEGER NOT NULL DEFAULT (unixepoch()), last_shared INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_discussion_events_retry ON discussion_events(accepted,last_shared);
CREATE TABLE discussion_items (
 id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, parent_id TEXT,
 subject_field_id TEXT NOT NULL REFERENCES subject_fields(id), author_did TEXT NOT NULL,
 content_json TEXT, body TEXT NOT NULL, created_at INTEGER NOT NULL,
 revision INTEGER NOT NULL, event_id TEXT NOT NULL, deleted INTEGER NOT NULL DEFAULT 0,
 credential_proof_ids TEXT NOT NULL
);
CREATE INDEX idx_discussion_thread ON discussion_items(thread_id,created_at);
CREATE TABLE discussion_votes (
 item_id TEXT NOT NULL REFERENCES discussion_items(id), actor_did TEXT NOT NULL,
 value INTEGER NOT NULL CHECK(value BETWEEN -1 AND 1), revision INTEGER NOT NULL, event_id TEXT NOT NULL,
 PRIMARY KEY(item_id,actor_did)
);
CREATE TABLE discussion_reports (
 item_id TEXT NOT NULL REFERENCES discussion_items(id), actor_did TEXT NOT NULL,
 reason TEXT NOT NULL, PRIMARY KEY(item_id,actor_did)
);

ALTER TABLE demo_opinion_examples ADD COLUMN thumbnail_cid TEXT;
