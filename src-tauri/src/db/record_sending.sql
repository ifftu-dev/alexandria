-- Record sending: who the learner sends learning records to, what each
-- recipient may receive, the stores the learner named and the credentials
-- they issued, and the learner-readable log of every send. Device-local:
-- none of these tables sync. See `crate::record_sending`.

CREATE TABLE record_recipients (
    id TEXT PRIMARY KEY,                                                            -- the recipient id in learning_record_outbox
    kind TEXT NOT NULL CHECK (kind IN ('destination', 'share')),                    -- destination: a store the learner named; share: a record share through the hosted service
    label TEXT NOT NULL UNIQUE,                                                     -- every identifier the recipient sees is derived from this label; never changes
    identifier TEXT NOT NULL CHECK (identifier IN ('pairwise', 'real_did', 'proposed')), -- the actor the recipient sees: one made for it, the learner's DID, or an account it proposed
    proposed_home_page TEXT,                                                        -- the proposed account, when identifier = proposed
    proposed_name TEXT,
    actor_name TEXT NOT NULL,                                                       -- the account name records carry at this recipient, as the learner saw it before approving
    course_ids TEXT NOT NULL CHECK (json_valid(course_ids)),                        -- scope: JSON array of the course ids whose records the recipient receives
    include_interactions INTEGER NOT NULL DEFAULT 0 CHECK (include_interactions IN (0, 1)), -- scope: 1 also sends interaction-level records (individual responses)
    created_at TEXT NOT NULL                                                        -- when the learner approved it
);

-- A store the learner named and contacts directly, with no hosted service.
CREATE TABLE record_destinations (
    recipient_id TEXT PRIMARY KEY REFERENCES record_recipients(id) ON DELETE CASCADE,
    endpoint TEXT NOT NULL,                                                         -- the store's xAPI base address, normalised to end in a slash
    location TEXT NOT NULL CHECK (location IN ('device', 'private_network', 'remote')), -- where the address resolved when the learner approved it; sending stops if it resolves elsewhere
    username TEXT NOT NULL,                                                         -- the key the store issued to this learner
    xapi_version TEXT NOT NULL                                                      -- the version negotiated with the store (2.0.0 or 1.0.3)
);

-- The secret half of a store's credentials, kept apart like other learner
-- secrets and never returned to the interface.
CREATE TABLE record_destination_secrets (
    recipient_id TEXT PRIMARY KEY REFERENCES record_destinations(recipient_id) ON DELETE CASCADE,
    secret TEXT NOT NULL
);

-- One row per send, in the learner-readable access log. Rows outlive the
-- recipient: removing a destination keeps the history of what it was sent.
CREATE TABLE record_access_log (
    seq INTEGER PRIMARY KEY,                                                        -- write order
    at TEXT NOT NULL,                                                               -- when, RFC 3339 UTC
    recipient_id TEXT NOT NULL,
    recipient_kind TEXT NOT NULL CHECK (recipient_kind IN ('destination', 'share')),
    recipient_name TEXT NOT NULL,                                                   -- what the learner knows the recipient as (a store's address, an organisation)
    action TEXT NOT NULL CHECK (action IN ('send', 'void')),                        -- void: statements withdrawing records already sent, on removal
    course_ids TEXT NOT NULL CHECK (json_valid(course_ids)),                        -- the scope in force
    include_interactions INTEGER NOT NULL CHECK (include_interactions IN (0, 1)),
    record_count INTEGER NOT NULL CHECK (record_count >= 0),
    record_ids TEXT NOT NULL CHECK (json_valid(record_ids)),                        -- local ids of the records sent or voided; voiding on removal reads them
    identifier TEXT NOT NULL,                                                       -- the account name the records carried
    outcome TEXT NOT NULL CHECK (outcome IN ('sent', 'failed')),
    detail TEXT                                                                     -- why it failed
);

CREATE INDEX idx_record_access_log_recipient ON record_access_log(recipient_id, action);
