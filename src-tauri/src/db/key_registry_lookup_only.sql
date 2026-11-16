-- Personal key rotation is deleted: a did:key resolves to the key in its
-- identifier, always. The registry stays only as the lookup for identifiers
-- that are not self-resolving, so its validity windows and rotated-by link
-- go, and a row must carry a 32-byte Ed25519 key (64 hex characters). Its
-- rows were written only by the retired rotation and announcement paths;
-- they are dropped.
DROP INDEX idx_key_registry_active;
DROP INDEX idx_key_registry_did_valid_from;
DROP TABLE key_registry;
CREATE TABLE key_registry (
    did TEXT NOT NULL,
    key_id TEXT NOT NULL,
    public_key_hex TEXT NOT NULL CHECK (length(public_key_hex) = 64),
    PRIMARY KEY (did, key_id)
);
