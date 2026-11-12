-- Credential anchors are batched: an issuer's node anchors one Merkle root per
-- batch, and each credential keeps a receipt proving its place in the batch
-- (protocol spec §11.3.1). Pre-launch data is replaced, not migrated.
DROP INDEX IF EXISTS idx_credential_anchors_status;
DROP TABLE credential_anchors;

-- One anchor transaction. `id` is its submission-journal operation id; the
-- journal holds the exact signed bytes.
CREATE TABLE anchor_batches (
    id TEXT PRIMARY KEY,
    issuer_did TEXT NOT NULL,
    root TEXT NOT NULL CHECK (length(root) = 64),
    leaf_count INTEGER NOT NULL CHECK (leaf_count > 0),
    funding TEXT NOT NULL CHECK (funding IN ('sponsor', 'issuer')),
    status TEXT NOT NULL DEFAULT 'outcome_unknown'
        CHECK (status IN ('outcome_unknown', 'submitted', 'confirmed', 'failed', 'expired')),
    tx_hash TEXT NOT NULL CHECK (length(tx_hash) = 64),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    confirmed_at TEXT
);

-- A credential this node issued and queued. Its status follows its batch.
CREATE TABLE credential_anchors (
    credential_id TEXT PRIMARY KEY REFERENCES credentials(id),
    batch_id TEXT REFERENCES anchor_batches(id),
    leaf_index INTEGER,
    anchor_tx_hash TEXT,
    anchor_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (anchor_status IN ('pending', 'outcome_unknown', 'submitted', 'confirmed', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    next_attempt_at TEXT,
    enqueued_at TEXT NOT NULL DEFAULT (datetime('now')),
    confirmed_at TEXT
);
CREATE INDEX idx_credential_anchors_status ON credential_anchors(anchor_status, next_attempt_at);
CREATE INDEX idx_credential_anchors_batch ON credential_anchors(batch_id);

-- Receipts by integrity hash H(c): this node's own once their batch confirms,
-- and receipts imported with credential bundles. A credential may hold several
-- receipts (an imported one can be wrong); the verifier is handed one whose
-- root the ledger shows. `verified` caches whether the receipt anchors a
-- stored credential, refreshed when an observation or a receipt arrives.
CREATE TABLE anchor_receipts (
    credential_hash TEXT NOT NULL CHECK (length(credential_hash) = 64),
    tx_hash TEXT NOT NULL CHECK (length(tx_hash) = 64),
    root TEXT NOT NULL CHECK (length(root) = 64),
    receipt_json TEXT NOT NULL CHECK (json_valid(receipt_json)),
    source TEXT NOT NULL CHECK (source IN ('local', 'imported')),
    verified INTEGER NOT NULL DEFAULT 0 CHECK (verified IN (0, 1)),
    next_observe_at TEXT,
    stored_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (credential_hash, tx_hash)
);
CREATE INDEX idx_anchor_receipts_tx ON anchor_receipts(tx_hash);

-- What the ledger showed for an anchor transaction (an IntegrityAnchor).
CREATE TABLE anchor_observations (
    tx_hash TEXT PRIMARY KEY CHECK (length(tx_hash) = 64),
    root TEXT NOT NULL CHECK (length(root) = 64),
    anchor_json TEXT NOT NULL CHECK (json_valid(anchor_json)),
    observed_at TEXT NOT NULL DEFAULT (datetime('now'))
);
