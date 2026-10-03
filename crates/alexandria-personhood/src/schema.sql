CREATE TABLE personhood_private_challenges (
    nonce TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    subject_did TEXT NOT NULL,
    network_id TEXT NOT NULL,
    challenge_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending', 'cancelled', 'consumed')),
    submission_digest TEXT,
    receipt_json TEXT,
    CHECK ((state = 'consumed') = (receipt_json IS NOT NULL)),
    CHECK ((state = 'consumed') = (submission_digest IS NOT NULL))
);
CREATE INDEX personhood_private_challenges_account ON personhood_private_challenges(subject_did, network_id, created_at);
