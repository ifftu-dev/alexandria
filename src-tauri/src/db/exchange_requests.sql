-- Credential requests whose organisation's domain vouched for them when this
-- device pulled them (see `crate::commands::exchange`). An attempt started
-- from one takes the request's issuance policy (trust profile floor, probe
-- policy) from here, never from the client.
CREATE TABLE exchange_requests (
    id TEXT PRIMARY KEY,
    directory_url TEXT NOT NULL,
    request_json TEXT NOT NULL CHECK (json_valid(request_json)),
    expires_at INTEGER NOT NULL,
    verified_at TEXT NOT NULL
);
-- The request's issuance policy as the attempt was started under it.
ALTER TABLE assessment_attempts ADD COLUMN request_policy_json TEXT
    CHECK (request_policy_json IS NULL OR json_valid(request_policy_json));
