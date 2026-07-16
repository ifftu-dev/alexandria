# Stake-Address → Public-Key Registry

**Status:** Implemented — schema (migration 052), registry module,
chain-side fetcher with witness verification, registration tx
builder, validator integration, embedded multisig-signed bootstrap
snapshot, and refresh-task spawn all live in the codebase. First
real preprod registration confirmed on 2026-05-25. Operator runbook:
[`stake-pubkey-registry-runbook.md`](./stake-pubkey-registry-runbook.md).
**Date:** 2026-05-25
**Spec link:** §7.3 (authority on privileged gossip topics)
**Replaces:** the in-memory TOFU binding that previously lived in
`src-tauri/src/p2p/validation.rs`.

## 1. Problem (historical — kept for context)

The earlier `check_identity_binding()` bound `stake_address →
public_key` the first time the running process saw a gossip message
from that stake address, and rejected later messages that disagreed.
The binding lived in a `Mutex<HashMap>` and lasted only for the
process lifetime.

Consequences:

- An attacker who wins the **first-seen race** for a committee member's
  stake address can issue privileged messages as that member for the
  remainder of the process. The bound key is the attacker's, not the
  legitimate member's.
- Restarts forget all bindings, so the race is repeatable on every
  reboot.
- Downstream `is_committee_member(db, stake_address)` only checks DAO
  membership of the *address*, not that the *public key* signing the
  envelope belongs to that address. The combined TOFU + membership check
  is not safe for taxonomy, governance, or Sentinel-prior messages.

## 2. Goals

1. Replace the in-memory TOFU binding with a persistent, evidence-backed
   one.
2. Keep first-boot startup fast — no synchronous Blockfrost call gating
   the app.
3. Survive Blockfrost outages without grinding to a halt.
4. Support ad-hoc committee rotation without requiring an app release.
5. Preserve auditability: every binding must be traceable to either an
   on-chain transaction or a signed bootstrap snapshot.

## 3. Non-Goals

- Replacing DID-based VC signing identities. This registry covers
  **gossip envelope** authority only; the VC `proof.jws` path is
  unchanged.
- Anonymous/unlinkable committee membership. Stake-address bindings are
  intentionally public.
- Solving general PKI for end users. Only privileged-topic authors
  (committee, chair, Sentinel co-signers) need a registry entry.

## 4. Design

### 4.1 Sources of truth (hybrid)

- **Canonical:** an on-chain `StakePubkeyRegistration` tx on Cardano,
  carrying `(stake_address, libp2p_pubkey, valid_from, valid_to)` in a
  Plutus datum and signed by the stake key (so possession of the stake
  key is proven at registration time).
- **Cache / bootstrap:** a JSON `bootstrap_registry.json` shipped in the
  release, signed by a 2-of-3 multisig of the org founders, listing the
  same `(stake_address, pubkey, valid_from, valid_to)` tuples plus the
  on-chain tx that produced them.

Conflict resolution: on-chain always wins. If the cache and chain
disagree for an entry within its validity window, the chain entry
overwrites the cached one and the discrepancy is logged.

### 4.2 SQLite schema (migration 052)

```sql
CREATE TABLE IF NOT EXISTS stake_pubkey_registry (
    stake_address   TEXT NOT NULL,                 -- Cardano stake addr (bech32)
    public_key_hex  TEXT NOT NULL,                 -- Ed25519 libp2p pubkey, hex
    valid_from      INTEGER NOT NULL,              -- unix secs
    valid_until     INTEGER,                       -- unix secs, NULL = open-ended
    source          TEXT NOT NULL,                 -- 'chain' | 'snapshot'
    on_chain_tx     TEXT,                          -- tx hash, NULL for snapshot-only
    snapshot_sig    TEXT,                          -- multisig hex, NULL for chain rows
    last_verified   INTEGER NOT NULL,              -- unix secs of last chain re-check
    PRIMARY KEY (stake_address, public_key_hex, valid_from)
);

CREATE INDEX IF NOT EXISTS idx_registry_address_window
    ON stake_pubkey_registry(stake_address, valid_from, valid_until);
```

Validity windows let a committee member rotate their libp2p key without
purging history — old messages signed by the old key during its window
remain verifiable, while new traffic uses the new key.

### 4.3 Bootstrap flow

1. First boot: read `bootstrap_registry.json` (currently bundled at
   build time via `include_bytes!`; the on-disk
   `<app_resources>/bootstrap_registry.json` loader exists in code
   for ops use but is not on the default boot path), verify the
   2-of-3 multisig over its canonical JSON bytes, insert all entries
   with `source='snapshot'` and `last_verified = 0`.
2. If verification fails, the current implementation logs a `WARN`
   and continues — privileged-topic gossip is then gated entirely by
   on-chain refresh entries until the snapshot is fixed. (Earlier
   drafts of this doc called for refusing privileged-topic
   subscription on snapshot-verify failure; that stricter posture
   is on the roadmap but is not what ships today.)
3. The background refresh task starts at the same time and begins
   reconciling against Blockfrost (§4.4).

### 4.4 Background refresh

- Tokio task spawned at startup. Tick interval = `registry.refresh_secs`
  setting (default 3600, clamped up to 60s to avoid hammering
  Blockfrost). The setting and the Blockfrost project id are both
  re-resolved fresh **on every tick** via factory closures handed
  to `spawn_refresh_task`, so:
  - the task survives the operator unlocking a profile or setting
    `cardano.blockfrost_project_id` after boot,
  - tuning `registry.refresh_secs` is live.
- Each tick:
  1. List all current UTxOs at the `stake_pubkey_registration`
     script address via Blockfrost
     `GET /addresses/{script_addr}/utxos`. This is a full scan, not
     a "since `last_verified`" delta — Blockfrost has no such
     filter, and the cost is bounded by the number of *active*
     committee bindings, which stays small. An incremental fetch
     keyed off the indexed `last_verified` column is on the roadmap
     once registration volume justifies the complexity.
  2. For each UTxO, fetch the creating transaction's inline datum
     (`GET /txs/{tx_hash}/utxos`) and the raw tx CBOR
     (`GET /txs/{tx_hash}/cbor`).
  3. Decode the Plutus-Data datum to
     `(stake_key_hash, pubkey, valid_from, valid_to)`.
  4. Verify the tx witness set contains a vkey witness whose
     Blake2b-224 hash equals `stake_key_hash`. Entries that fail
     witness verification are dropped with a `WARN`.
  5. Convert the stake-key-hash to its bech32 stake address and
     UPSERT into `stake_pubkey_registry` with `source='chain'` and
     `last_verified = now`.
  6. For any pre-existing snapshot row whose
     `(stake_address, pubkey, valid_from)` triple matches the
     chain-confirmed binding, the same UPSERT upgrades `source`
     in-place to `'chain'`.
  7. For any snapshot row whose binding **contradicts** the chain
     (different pubkey for an overlapping window), evict the
     snapshot row and emit a `WARN`. Chain wins.
- Failures (Blockfrost outage, network error, lock poisoning,
  missing profile DB) are logged and the next tick retries.
  Registry state is unchanged across a failed tick.

### 4.5 Validation lookup

`check_identity_binding(message)` becomes:

```rust
let now = unix_secs_now();
let pubkey_hex = hex::encode(&message.public_key);
let allowed: bool = conn.query_row(
    "SELECT EXISTS(
       SELECT 1 FROM stake_pubkey_registry
       WHERE stake_address = ?1
         AND public_key_hex = ?2
         AND valid_from <= ?3
         AND (valid_until IS NULL OR valid_until > ?3)
     )",
    params![&message.stake_address, &pubkey_hex, now],
    |r| r.get(0),
)?;
if !allowed {
    return Err(ValidationError::IdentityMismatch { … });
}
```

No in-memory binding. No TOFU. Unregistered stake addresses fail closed.

### 4.6 Topic scope

The registry guards messages whose stake address is checked downstream
for authority. Today that's exactly the set returned by
`p2p::registry::is_privileged_topic`:

- `/alexandria/taxonomy/1.0` — DAO-ratified skill graph updates,
  authority checked by `p2p::taxonomy::is_committee_member`.
- `/alexandria/governance/1.0` — committee changes / proposal events.
- `/alexandria/sentinel-priors/1.0` — Sentinel DAO threshold-signed
  adversarial-prior announcements.
- `/alexandria/plugin-attestations/1.0` — Plugin DAO threshold-signed
  `(plugin_cid, grader_cid)` approvals.
- `/alexandria/goal-templates/1.0` — DAO-ratified goal-template
  publications.
- `/alexandria/question-banks/1.0` — DAO-ratified question-bank
  publications.

Non-privileged topics (`profiles`, `opinions`, catalog, plugin
announcements, VC-layer topics, classroom messages, etc.) skip the
registry check — they're authored by arbitrary peers, and their
integrity is enforced by the per-message signature + content-level
rules. When adding a new privileged topic, extend `is_privileged_topic`
in `p2p/registry.rs` AND list it here so the two stay in sync.

### 4.7 Snapshot file format

```json
{
  "version": 1,
  "issued_at": "2026-05-25T00:00:00Z",
  "entries": [
    {
      "stake_address": "stake1u...",
      "public_key_hex": "ab12...",
      "valid_from": 1748131200,
      "valid_until": null,
      "on_chain_tx": "deadbeef..."
    }
  ],
  "signatures": [
    {"signer": "founder_a", "sig_hex": "..."},
    {"signer": "founder_b", "sig_hex": "..."}
  ]
}
```

Multisig verification: collect the three founder public keys from a
constant in code (`p2p::registry::SNAPSHOT_VERIFIERS`), accept the
snapshot iff ≥ 2 of the signatures over `canonical_json(entries +
issued_at + version)` verify.

### 4.8 Settings

- **`registry.refresh_secs`** *(implemented; per-device, default 3600)* —
  cadence at which the background refresh task reconciles against
  on-chain registrations. Clamped up to 60s at runtime to avoid
  hot-looping Blockfrost. Lives in the standard
  `app_settings`/registry plumbing (migration 048 scope) and is
  re-read on every tick, so changes take effect on the next
  reconciliation without a restart.
- `registry.require_chain_verification` *(not implemented; on the
  roadmap)* — would reject any binding that hasn't been confirmed
  against the chain since app start. Tracked as a follow-up.

## 5. Migration story (pre-launch)

No live users. No data migration needed. Steps:

1. Land migration 052 + registry module.
2. Ship `bootstrap_registry.json` w/ initial committee in
   `src-tauri/resources/`.
3. Flip the validation call site to the new lookup.
4. Delete `identity_bindings: Mutex<HashMap<…>>` from
   `GossipValidator` and the TOFU code in `validation.rs:108-144`.

## 6. Testing plan

- Unit:
  - Registry lookup honors validity windows (inside, on edge, outside).
  - Snapshot verifier rejects 0-of-3, 1-of-3; accepts 2-of-3, 3-of-3.
  - Snapshot verifier rejects forged sigs.
  - UPSERT: chain row overwrites snapshot row when they disagree.
  - UPSERT: chain row upgrades matching snapshot row's `source` field
    without inserting a duplicate.
- Integration:
  - Mock Blockfrost: bg task pulls 1 new registration, table reflects
    it inside one tick.
  - Mock Blockfrost: outage at tick time, registry unchanged, no
    retries blocked.
  - Privileged-topic replay attack: forged envelope with unregistered
    pubkey for a known committee stake address → rejected at
    `check_identity_binding`.
- End-to-end:
  - Fresh node, no network: bootstrap snapshot loaded, taxonomy update
    from a snapshot-listed committee member accepted.
  - Fresh node, no snapshot file: privileged topics fail closed,
    non-privileged topics still work.

## 7. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Org multisig key compromise | Snapshot only seeds initial state; chain entries override. Compromised snapshot can't outlast next chain refresh. |
| Blockfrost extended outage | Snapshot remains authoritative for its windows; node keeps working with stale-but-valid state. `last_verified` lets ops detect stale data. |
| Committee rotation race (member's old key revoked before new key signs) | `valid_until` overlap handled by leaving both rows valid during the overlap window. Operationally: rotators publish new tx before old tx's `valid_until`. |
| `bootstrap_registry.json` tampering in release pipeline | Multisig requires 2 of 3 founder keys; one compromise insufficient. Release SBOM should hash the file. |
| Backward compatibility for in-flight messages mid-rotation | Validity windows are inclusive on `valid_from`, exclusive on `valid_until`. Message freshness window (±5 min) is already smaller than expected rotation overlap, so verification at message timestamp avoids races. |

## 8. Open questions

- Do we add the on-chain `StakePubkeyRegistration` script in this PR, or
  ship the snapshot-only path first and add the script in a follow-up?
  Recommendation: snapshot-only for PR B, on-chain script in PR B'
  before public-beta — chain refresh code can be stubbed but disabled.
- Snapshot file location: `src-tauri/resources/` (bundled, requires
  rebuild) vs `<app_data>/bootstrap_registry.json` (replaceable). Pick
  bundled for tamper resistance; add a TOFU-free update path later.
- Founder verifier pubkeys: hardcoded constants vs config. Hardcoded
  for v1 — config opens a key-substitution attack vector.

## 9. Implementation order (PR B impl)

1. Schema migration 052.
2. `p2p::registry` module: types, snapshot loader, multisig verifier.
3. `p2p::registry::lookup(conn, stake_addr, pubkey, ts)` helper +
   tests.
4. Wire `check_identity_binding` to call the helper. Delete the
   `identity_bindings` field and its initialization.
5. Background refresh task (stubbed Blockfrost client first, real
   client second).
6. Ship a stub `bootstrap_registry.json` with the project's initial
   committee.
7. Tests as per §6.
