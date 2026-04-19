# Sentinel Adversarial Priors — Implementation Plan (Option B)

> **Scope:** ship the first federated-learning capability for Sentinel without federating any per-user data. The Sentinel DAO curates labeled attack patterns; each client pulls them and trains locally. See [sentinel-federation.md](sentinel-federation.md) for the design rationale.
>
> **Target timeline:** 3–4 focused weeks. Pre-req: Sentinel DAO scaffolding (phase 1).
>
> **Non-goals:** Option A (per-user gradient sharing), DP-SGD, secure aggregation, Cardano stake thresholds for contributors. Those stay dormant and become live only if/when we revisit Option A.

---

## 1. Architecture at a glance

```
┌─ Proposer (any learner / researcher / instructor) ──────────────┐
│   1. Capture or synthesize a cheat pattern                      │
│   2. Upload as IPFS blob + create governance_proposal(          │
│        dao_id = Sentinel DAO, category = 'sentinel_prior')      │
└────────────────────────────────┬────────────────────────────────┘
                                 │
┌─ Sentinel DAO committee ───────▼────────────────────────────────┐
│   3. Review proposal, vote per existing governance pipeline     │
│   4. On ratification → insert into sentinel_priors table        │
│   5. Republish under Sentinel DAO-signed CID                    │
└────────────────────────────────┬────────────────────────────────┘
                                 │ Iroh blob + catalog entry
┌─ Every Sentinel client ────────▼────────────────────────────────┐
│   6. Periodic pull of ratified priors (daily)                   │
│   7. Verify Sentinel DAO signature + hash                       │
│   8. Cache locally, indexed by (model_kind, label)              │
│   9. Fold into local training when user calibrates / retrains   │
└─────────────────────────────────────────────────────────────────┘

Holdout set (not shown): encrypted-at-rest, accessible to a multi-sig
subset of Sentinel DAO members. Used for measuring false-positive rate
against fresh Sentinel builds; never broadcast.
```

Everything the client consumes is public and signed. Nothing the client produces is published (unless the learner *proposes* a pattern, which is an explicit user action — not ambient telemetry).

---

## 2. Phases

### Phase 1 — Sentinel DAO scaffolding · ~1 week

**Goal:** a Sentinel DAO exists with voting membership and the ability to ratify `sentinel_prior` proposals. Reuses `governance_daos` / `governance_proposals` with a new scope_type and category.

- **Schema migration 037 — `sentinel_dao`:**
  - Extend `governance_daos.scope_type` to accept `'sentinel'` (no schema change; already TEXT)
  - Seed one row: `id = 'sentinel-dao'`, `scope_type = 'sentinel'`, `scope_id = 'sentinel-global'`
  - Add `'sentinel_prior'` as a valid `governance_proposals.category` value (no schema change)
- **New IPC commands** (`commands/sentinel_dao.rs`):
  - `sentinel_dao_get_info() -> SentinelDaoInfo` (committee members, election schedule, etc.)
  - Reuses existing `governance_propose`, `governance_vote`, `governance_ratify` with dao_id scoped to sentinel-dao
- **Bootstrapping committee:** initial seed members — open question, out of scope for this phase. Propose: the existing Alexandria DAO elects the first Sentinel DAO committee.
- **No UI in this phase** — governance UI already exists under `/dashboard/governance`; Sentinel DAO shows up as just another DAO.

**Deliverable:** can create, vote on, and ratify `sentinel_prior` proposals via existing governance commands.

### Phase 2 — `sentinel_priors` table + blob schema · ~3 days

**Goal:** ratified priors are first-class entities with a versioned on-device schema.

- **Schema migration 038 — `sentinel_priors`:**
  ```sql
  CREATE TABLE IF NOT EXISTS sentinel_priors (
      id              TEXT PRIMARY KEY,  -- blake2b(cid + label + model_kind)
      proposal_id     TEXT NOT NULL REFERENCES governance_proposals(id),
      cid             TEXT NOT NULL,     -- IPFS CID of the labeled example blob
      model_kind      TEXT NOT NULL,     -- 'keystroke'|'mouse'
      label           TEXT NOT NULL,     -- 'bot_script'|'paste_macro'|'remote_control'|'teleport'|etc.
      schema_version  INTEGER NOT NULL,  -- blob format version
      sample_count    INTEGER NOT NULL,  -- how many examples the blob contains
      notes           TEXT,              -- freeform curator notes
      ratified_at     TEXT NOT NULL,
      signature       TEXT NOT NULL      -- Sentinel DAO threshold sig over (cid + label + model_kind + schema_version)
  );

  CREATE INDEX IF NOT EXISTS idx_sentinel_priors_kind
      ON sentinel_priors(model_kind);
  ```
- **Blob schema (JSON, v1):**
  ```json
  {
    "schema_version": 1,
    "model_kind": "keystroke",
    "label": "paste_macro",
    "samples": [
      { "dwellMs": [...], "flightMs": [...], "digraphs": [...] },
      ...
    ],
    "notes": "Captured from Selenium script running paste at 50 chars/sec",
    "contributor_attribution": "stake1..."
  }
  ```
  Face model kind is **not** present — decision 2.
- **Forfeiture hook (decision 3):** proposal submission blocks if `source_session_id` (optional field on the proposal) references an `integrity_sessions` row with status `flagged` or `suspended`. Enforced at `sentinel_propose_prior` entry point.
- **Validation** at ratification time: blob parses, required fields present, sample count ≥ 20, no face kind.

**Deliverable:** a ratified prior is queryable via `sentinel_list_priors(model_kind)` and the blob is verifiably signed.

### Phase 3 — Client fetch + cache · ~3 days

**Goal:** each Sentinel client keeps a local mirror of all ratified priors.

- **New IPC commands** (`commands/sentinel_priors.rs`):
  - `sentinel_priors_sync() -> SyncResult` — pulls new ratified rows, fetches missing CIDs, verifies signatures, updates local cache
  - `sentinel_priors_list(kind: String) -> Vec<PriorMetadata>` — client-facing listing
  - `sentinel_priors_load(id: String) -> PriorBlob` — lazy-load a specific blob (returns parsed JSON)
- **Cache location:** separate IPFS pin set under `pin_type = 'sentinel_prior'` (add to existing `pins` enum).
- **Sync cadence:** on app start + once daily while running. Cheap because blobs are small and content-addressed.
- **Version handling:** clients ignore priors whose `schema_version` they don't understand, logged but non-fatal.

**Deliverable:** client maintains a local index of ratified priors and can retrieve blob contents on demand.

### Phase 4 — Training integration · ~3–5 days

**Goal:** `useSentinel.ts` local training incorporates ratified priors as negative examples.

- **Keystroke AE (`utils/sentinel/keystroke-autoencoder.ts`):**
  - Already trains as an anomaly detector (unsupervised reconstruction of user data).
  - Integration: during `train()`, fold in labeled paste_macro / bot_script digraphs as additional high-loss-target samples (teach the AE to reconstruct human patterns poorly when given known-attack patterns). This is a minor loss function tweak; no architecture change.
- **Mouse CNN (`utils/sentinel/mouse-trajectory-cnn.ts`):**
  - Already uses 5 hard-coded synthetic bot patterns as negative class (reservoir-computing design).
  - Integration: replace/extend the hard-coded list with ratified `mouse/bot_script` and similar priors. Keep the 5 synthetic patterns as a fallback when priors haven't synced yet.
- **Face embedder:** unchanged. No face priors ever.
- **When training fires:** unchanged — training wizard and `saveTrainingProfile()`. The difference is that `trainAIModels()` now accepts a `priorsForKind(kind)` function and the wizard hydrates it from the local prior cache before training.

**Deliverable:** keystroke AE and mouse CNN train against curated attack data as well as user data. Anomaly detection improves without any per-user data leaving the device.

### Phase 5 — Proposal UX · ~3 days

**Goal:** learners / researchers can submit a cheat-pattern proposal from the Sentinel dashboard.

- **New route:** `/dashboard/sentinel/propose-prior`
- Upload flow:
  1. Choose `model_kind` (keystroke | mouse)
  2. Choose `label` from a known-label dropdown + freeform
  3. Upload JSON blob conforming to the schema, or record one live (later phase)
  4. Attach optional `source_session_id` (forfeiture check runs)
  5. Preview summary + submit
- Blob is pinned locally and referenced in the governance_proposal as `content_cid`.
- Proposal appears in the Sentinel DAO voting queue (existing governance UI).

**Deliverable:** end-to-end path from "I have a labeled attack pattern" → "DAO can vote" → "ratified prior in the library."

### Phase 6 — Holdout evaluation set · ~3 days

**Goal:** Sentinel DAO can measure classifier false-positive / accuracy against an unpublished holdout.

- **Schema migration 039 — `sentinel_holdout`:**
  ```sql
  CREATE TABLE IF NOT EXISTS sentinel_holdout_refs (
      id              TEXT PRIMARY KEY,
      encrypted_cid   TEXT NOT NULL,   -- CID of encrypted holdout blob
      key_policy      TEXT NOT NULL,   -- multi-sig policy for decrypt
      model_kind      TEXT NOT NULL,
      created_at      TEXT NOT NULL
  );
  ```
- Blob is encrypted with a committee threshold key; only a multi-sig subset of Sentinel DAO members can decrypt.
- Evaluation harness: `sentinel_evaluate_classifier(builder_signature)` runs client-side classifier against decrypted holdout, reports aggregate accuracy + false-positive rate. Only runs when the caller holds the decryption share.
- Results surface in `Sentinel.vue` dashboard as a weekly "Classifier health: 98% true-positive, 3% false-positive" card.
- **Role separation (decision 7 note):** the DAO members authorized to curate priors must not overlap with the members authorized to decrypt the holdout, so no one can leak holdout examples back into training. Enforced via the key policy.

**Deliverable:** DAO has a standing evaluation signal it can use to decide whether Option B's curated approach is keeping false-positives under control (revisit-A threshold: >5% for six months).

### Phase 7 — Privacy guarantee doc update · ~0.5 day

Apply the sentinel.md #4 / #6 hunks already drafted in [sentinel-federation.md §9](sentinel-federation.md#9-privacy-guarantee-rewrite-for-sentinelmd). These are the Option-B-only versions; guarantee #4 stays an absolute claim, guarantee #6 gains a note about the read-only prior library.

**Deliverable:** public docs accurately reflect what the code does.

---

## 3. Data model summary

| Artifact | Kind | Lives where |
|----------|------|-------------|
| Sentinel DAO row | `governance_daos` | SQLite, replicated |
| Prior proposal | `governance_proposals` (category `sentinel_prior`) | SQLite, replicated |
| Ratified prior metadata | `sentinel_priors` (new, migration 038) | SQLite, replicated |
| Labeled example blob | JSON blob | IPFS, pinned under `pin_type='sentinel_prior'` |
| Holdout set | Encrypted JSON blob | IPFS, pinned by DAO committee members only |
| Client cache of priors | Same as replicated tables | Local SQLite |
| User behavioral profile + AI weights | Unchanged | Device localStorage |

Nothing per-user is new. The only new writes are *publications* — cheat patterns proposed by someone who intentionally clicked "propose."

---

## 4. New IPC commands

| Command | Phase | Description |
|---------|-------|-------------|
| `sentinel_dao_get_info` | 1 | Read sentinel-dao row + committee |
| `sentinel_propose_prior` | 2 | Create a proposal of category `sentinel_prior`. Runs forfeiture check. |
| `sentinel_ratify_prior` | 2 | Hook called on proposal ratification; inserts into `sentinel_priors`. |
| `sentinel_priors_sync` | 3 | Pull missing CIDs, verify sigs, populate cache |
| `sentinel_priors_list` | 3 | List cached priors, optionally by kind |
| `sentinel_priors_load` | 3 | Return blob JSON for a given prior id |
| `sentinel_evaluate_classifier` | 6 | Run classifier against holdout; requires decrypt share |

All follow existing `integrity_*` command conventions (AppState + rusqlite + serde).

---

## 5. Testing strategy

- **Unit tests (Rust):** severity-of-classifier-on-prior set, signature verification, forfeiture check, schema migrations apply cleanly on existing DB.
- **Unit tests (TS):** autoencoder / CNN trained with synthetic + ratified priors has expected anomaly-threshold shift; no runtime regressions in `useSentinel.computeScores()`.
- **Integration tests:** end-to-end propose → vote → ratify → client sync → train. Lives in a new `tests/sentinel_priors.rs`.
- **Attack simulation:** pre-ratification, the DAO curator runs the proposed blob through the current classifier to confirm it's detectable as anomalous *before* adding it — prevents ratifying priors that the model already correctly classifies as human.

---

## 6. Risks & open technical questions

| Risk | Mitigation |
|------|-----------|
| Ratified priors cause real-user false-positive spikes | Holdout evaluation (phase 6). Every new prior is benchmarked before it ships to all clients. Roll back via governance if the holdout metric regresses. |
| Prior-library format evolves, old clients break | `schema_version` field in every blob + metadata row. Old clients ignore unknown versions, logged. Migrations happen via new ratified priors at a higher version, not via rewrites. |
| Prior library becomes too large for weak devices | Per-kind quotas (e.g., max 1000 priors per model_kind); oldest unused evicted client-side. DAO can mark older priors as `superseded` to guide eviction. |
| Sentinel DAO committee captured or goes offline | Pinning-forever (decision 8) lets clients stay on last-known-good prior set. Alexandria main DAO can intervene to re-seat the Sentinel DAO. |
| Holdout leaks | Split duties (curators ≠ holdout-key-holders) + multi-sig threshold. If leak is suspected, rotate holdout set; old leaked examples can still be used in training since they're now in-distribution. |
| Attacker proposes priors that model legitimate human behavior (anti-proposal) | DAO review + holdout pre-check will catch this. The pre-ratification classifier test is specifically designed to block priors that get classified as "human" by the current model. |

---

## 7. Definition of done

- Sentinel DAO exists in the governance UI and can ratify proposals
- At least three seed priors exist (one keystroke paste-macro, one mouse scripted-bot, one mouse teleport)
- Clients sync the seed priors within 24h of app start
- Training wizard surfaces "trained against N curated patterns" in its result screen
- Holdout evaluation card visible in `Sentinel.vue` for any user who is a Sentinel DAO committee member
- `sentinel.md` guarantees #4 and #6 updated per [sentinel-federation.md §9](sentinel-federation.md#9-privacy-guarantee-rewrite-for-sentinelmd)
- New unit + integration tests pass; `cargo fmt --check`, `cargo clippy -- -D warnings`, `vue-tsc -b --noEmit` all green

---

## 8. Open governance questions (non-blocking for this plan, blocking for ship)

1. **Sentinel DAO bootstrap** — who are the initial committee members? Proposal: Alexandria DAO elects them in a one-off proposal before this lands.
2. **Sentinel DAO token** — does it share Alexandria's governance token, or issue its own? Technical design is indifferent; user experience is not.
3. **Proposer incentive** — zero today. Worth thinking about whether contributors of high-quality priors deserve reputation credit in the Alexandria reputation system.
4. **Pattern retirement** — when an attack is so old it's no longer seen in the wild, do we mark its priors as `archived` to keep training sets fresh? Proposing: yes, via a lightweight DAO vote; archived priors stay pinned but stop being distributed to new clients.

These are governance/product questions, not engineering blockers. Phases 1–7 can ship without resolving them.
