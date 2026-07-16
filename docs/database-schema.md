# Database Schema

> Alexandria — SQLite (local-first)

> **⚠️ Post-VC-first cutover (migration 040, 2026-04-24):** The
> following tables are **dropped** and should be treated as absent
> when cross-referencing this document:
> `skill_proofs`, `skill_proof_evidence`, `evidence_records`,
> `skill_assessments`, `reputation_evidence`,
> `reputation_impact_deltas`, `evidence_challenges`,
> `challenge_votes`, `attestation_requirements`,
> `evidence_attestations`.
> The `credentials` table gains four columns:
> `witness_tx_hash`, `witness_validator_script_hash`,
> `witness_validator_name`, `auto_issued`. See
> [`vc-migration.md`](./vc-migration.md) for the full diff.

**Engine**: SQLite (rusqlite 0.38, bundled)
**Migrations**: 71

---

## Table of Contents

1. [Design Principles](#design-principles)
2. [Migration History](#migration-history)
3. [Tables by Domain](#tables-by-domain)
4. [Entity Relationship Summary](#entity-relationship-summary)

---

## Design Principles

- **Deterministic IDs**: Most application entities use `hex(blake2b_256(parts.join("|")))` instead of server-generated UUIDs.
- **Singleton identity per profile**: `local_identity` is a one-row table with `CHECK (id = 1)`. Each user profile has its own SQLCipher database (see [`multi-user-profiles.md`](multi-user-profiles.md)), so "singleton" is scoped per-profile, not per-device — a device with three profiles has three independent `local_identity` rows, each in its own encrypted DB file.
- **No server tables**: No hosted auth/session model exists; the app is profile-based and local-first.
- **External content**: Course content, published profiles, evidence bundles, and other large artifacts live in iroh/IPFS-addressed blobs. SQLite stores metadata, references, and caches.
- **Text timestamps**: Time values are stored as ISO-8601-ish `TEXT` for portability and easy inspection.
- **Canonical source**: The exact DDL, defaults, `CHECK` constraints, indexes, and migration bodies live in `src-tauri/src/db/schema.rs`.

---

## Migration History

| Version | Name | Description |
|---------|------|-------------|
| 1 | `initial_schema` | Core tables: identity, taxonomy, courses, learning, evidence, integrity, P2P, governance |
| 2 | `profile_hash` | Add `profile_hash` to `local_identity` |
| 3 | `content_mappings` | Bidirectional CID↔BLAKE3 mapping for the iroh/IPFS bridge |
| 4 | `assessment_columns` | Add `weight` and `source_element_id` to `skill_assessments` |
| 5 | `governance_members` | DAO committee membership |
| 6 | `reputation_engine` | Reputation evidence and impact-delta tables |
| 7 | `governance_elections` | Elections, nominees, proposal voting, election voting |
| 8 | `reputation_snapshots` | On-chain reputation snapshot records |
| 9 | `taxonomy_ratification` | Add `ratified_by` and `ratified_at` to `taxonomy_versions` |
| 10 | `cross_device_sync` | Devices, sync state, sync queue, local device metadata |
| 11 | `evidence_challenges` | Challenge and challenge-vote tables |
| 12 | `multi_party_attestation` | Attestation requirements and attestation records |
| 13 | `visual_assets` | Add display/image fields such as `author_name`, `thumbnail_svg`, and `icon_emoji` |
| 14 | `inline_content` | Add `content_inline` to `course_elements` |
| 15 | `tutoring_sessions` | Live tutoring session metadata |
| 16 | `classrooms` | Classrooms, members, join requests, channels, messages, calls |
| 17 | `storage_settings` | Persistent app settings (`app_settings`) |
| 18 | `onchain_governance_queue` | Async governance submission queue |
| 19 | `classroom_encryption` | Classroom group keys plus X25519 key material |
| 20 | `tutorials_and_video_chapters` | Course/tutorial discriminator and per-video chapter markers |
| 21 | `opinions` | Field Commentary opinions, pending verification, DAO withdrawals |
| 22 | `vc_key_registry` | Historical DID key registry for VC verification |
| 23 | `vc_credentials_and_status_lists` | Canonical VC store and status-list bitmaps |
| 24 | `vc_credential_anchors` | Cardano integrity-anchor queue for credential hashes |
| 25 | `vc_pinboard_observations` | PinBoard commitment observations |
| 26 | `vc_presentations_seen` | Replay-protection log for selective-disclosure presentations |
| 27 | `vc_derived_skill_states` | Cached aggregation outputs |
| 28 | `vc_credentials_pending_verification` | Queue for inbound credentials awaiting issuer DID resolution |
| 29 | `vc_credential_suspension` | Add credential suspension metadata and supersession index |
| 30 | `vc_credential_allowlist` | Subject-controlled allowlist for `/alexandria/vc-fetch/1.0` |
| … | (migrations 31-39: content provenance, plugin system, plugin catalog/attestations, sentinel flags/priors/holdout) | |
| 40 | `vc_first_cutover` | Hard cut to VC-first. Drops the SkillProof/evidence pipeline (`skill_proofs`, `skill_proof_evidence`, `evidence_records`, `skill_assessments`, `reputation_evidence`, `reputation_impact_deltas`, `evidence_challenges`, `challenge_votes`, `attestation_requirements`, `evidence_attestations`). Adds witness columns to `credentials`. |
| 41 | `completion_observer` | `completion_observations` — observer memo for Cardano completion-mint events that auto-issue VCs |
| 42 | `completion_attestation` | `completion_attestation_requirements` + `completion_attestations` — VC-first replacement for evidence cosigning |
| 43 | `credential_challenges` | `credential_challenges` + `credential_challenge_votes` — VC-first replacement for evidence challenges |
| 44 | `integrity_paste_anomaly` | Add `ai_paste_anomaly` column to `integrity_snapshots` |
| 45 | `sentinel_priors_model_weights` | Add DAO-ratified model-weights columns (`weights_cid`, `eval_cid`, `eval_tpr`, `eval_fpr`, `version`) to `sentinel_priors` |
| 46 | `sentinel_kill_switch_and_blocklist` | `sentinel_kill_switch` + `sentinel_weights_blocklist` — operator safety valves for the paste classifier |
| 47 | `sentinel_user_models` | `sentinel_user_models` — per-user keystroke/mouse weights moved from browser localStorage into the encrypted DB |
| 48 | `app_settings_scope` | Add `scope` column (`sync` / `device`) to `app_settings`. Reclassifies `storage_quota_bytes` as `device`-scoped. Powers the unified per-profile settings store; see [`settings.md`](settings.md). |
| 49 | `device_pairing` | Add `stake_address` / `shared_key` / `paired` to `devices`; new `pending_pairings` table for explicit device pairing |
| 50 | `challenge_stake_lifecycle` | Add `stake_status` + `settle_tx_hash` to `credential_challenges` for stake-escrow settlement |
| 51 | `element_submission_grader_version` | Add `grader_version` column to `element_submissions` |
| 52 | `stake_pubkey_registry` | `stake_pubkey_registry` — persistent stake-address → libp2p Ed25519 pubkey bindings (chain + multisig-signed snapshot rows). Replaces the in-memory TOFU binding; see [`stake-pubkey-registry.md`](stake-pubkey-registry.md). |
| 53 | `plugin_enabled_and_irl_review` | Add `enabled` flag to `plugin_installed` (disabled plugins stay installed but the player refuses to mount them). New `plugin_irl_submissions` table — the local instructor-review inbox backing the `irl-review` builtin plugin. See [`plugins.md`](plugins.md). |
| 54 | `usernames_profile_visibility` | `username` + `visibility` on `local_identity`; `peer_profiles` cache (filled by `/alexandria/profile-fetch/1.0`) |
| 55 | `username_claim_cache` | `username_claims` — verified DHT registry winners (`tier` 0 bare / 1 receipted / 2 anchored) |
| 56 | `username_anchor_verified` | `anchor_verified` flag gating tier 2 in conflict ordering |
| 57 | `dht_record_mirror` | `dht_records` — local mirror of signed DHT registry records |
| 58 | `governance_vote_signatures` | Add `signature` / `public_key` to `governance_election_votes` and `governance_proposal_votes` (off-chain signed votes for the lean on-chain governance bridge) |
| 59 | `governance_dao_onchain_links` | Add on-chain link columns to `governance_daos` (`state_token_policy`, `state_token_name`, `reputation_policy`, `membership_subjects_json`, `dao_state_utxo`) |
| 60 | `integrity_gaze_offscreen_ratio` | Add `gaze_offscreen_ratio` to `integrity_snapshots` (Sentinel gaze / second-device signal) |
| 61 | `integrity_attestation` | Assurance ladder: add `assurance_level` / `commitment_root` / `anchor_ref` to `integrity_sessions`, `commitment_hash` to `integrity_snapshots`, plus `integrity_attestations` (automated attestation records) |
| 62 | `org_role_assessments` | `organizations` + `role_assessments` — enterprise sponsors and role/JD-based skill assessments |
| 63 | `plugin_dependencies` | `plugin_dependencies` — declared inter-plugin dependencies (e.g. codejudge language plugins on a shared parent) |
| 64 | `plugin_element_state` | `plugin_element_state` — per-element plugin state persisted across navigation and restart |
| 65 | `element_submission_answers` | Add `answers_json` to `element_submissions` (persisted learner responses) |
| 66 | `account_role_birthdate_activation` | Add `account_role` (`learner`/`instructor`/`parent`), `birthdate` (ISO-8601, on-device only), and `activation_state` (`active`/`pending_guardian`) to `local_identity`. Age is **recomputed** from `birthdate` each unlock — never stored — so turning 18 resolves automatically. |
| 67 | `guardian_links` | Cross-device parental oversight: `guardian_links` (ward↔guardian pairing, vault-sealed shared key, W3C VC ids, `status`), `guardian_pending_invites` (single-use `code_hash` PK, mirrors the pairing-code pattern), `guardian_activity_rows` (sealed activity the child pushes to the guardian). See [`protocol-specification.md`](protocol-specification.md#guardian-link-protocol). **Never** added to device-sync `SYNCABLE_TABLES` or gossip. |
| 68 | `skill_provenance` | Add `provenance` to `credentials` (denormalized `ProvenanceTier` mirror of `credentialSubject.provenance`) and `dominant_provenance` to `derived_skill_states` (highest provenance tier backing the skill) |
| 69 | `goal_templates` | `goal_templates` + `goal_template_versions` (DAO-ratified exam/curriculum/job-role → ideal skill graph); add `synonyms` to `skills` for on-device JD/resume matching |
| 70 | `assessment_question_banks` | `question_banks`, `bank_questions` (answer key `correct_indices` **never** sent to the client), `question_bank_versions`, `assessment_attempts` — dynamic Sentinel-gated community assessments |
| 71 | `plugin_review_course_scope` | Add `course_id` to `plugin_irl_submissions`, generalizing it into the shared submit-for-review store for any plugin with the `instructor_review` capability; backfills course scope so the instructor inbox can be scoped to owned courses (legacy unresolved rows stay NULL / globally visible) |

---

## Tables by Domain

This section is a domain summary, not a copy of the full DDL. For exact
columns and indexes, use `src-tauri/src/db/schema.rs`.

### Identity

- **`local_identity`** — Singleton row for the *active profile's* owner.
  Each profile's SQLCipher DB has exactly one row at `id = 1`. Stores
  wallet/profile metadata such as `stake_address`, `payment_address`,
  `display_name`, `bio`, `avatar_cid`, `profile_hash`, encrypted
  mnemonic fallback, device metadata, and X25519 public key material.
  The public-facing profile picker metadata (name shown on the
  picker, avatar, accent color) lives separately in the unencrypted
  `profiles_index.json` sidecar — see
  [`multi-user-profiles.md`](multi-user-profiles.md).
  Migration 66 adds `account_role`, `birthdate` (kept on-device, excluded
  from the public `SignedProfile`), and `activation_state` — a minor
  learner starts `pending_guardian` and flips to `active` only after a
  guardian link is established.

### Guardianship (3 tables)

> Parental oversight is **cross-device and cross-user** — a minor and their
> parent are separate identities on separate devices, linked over the sealed
> `/alexandria/guardian/1.0` protocol. These tables and the birthdate are
> deliberately excluded from device-sync and gossip; a unit test enforces that
> guardian tables never appear in `SYNCABLE_TABLES`.

- **`guardian_links`** — One row per link, on both sides (`side` = `ward` or
  `guardian`). Holds the peer's DID / stake / peer id, the per-link shared key
  (vault-sealed), the issued guardianship + ward VC ids, `status`
  (`pending`/`active`/`revoked`), and the child's birthdate on the guardian side.
- **`guardian_pending_invites`** — Single-use invites keyed by `code_hash`
  (PK), with the sealed shared key and an `expires_at` (~7 days — the code
  waits for an offline parent, not a live connection). Mirrors the single-use
  pending-pairing pattern.
- **`guardian_activity_rows`** — Sealed activity the child pushes to the
  guardian (enrollments, progress, submissions), keyed by
  `(link_id, table_name, entity_id)` and merged last-write-wins.

### Taxonomy (6 tables)

- **`subject_fields`** — Top-level domains, including optional `icon_emoji`.
- **`subjects`** — Child subjects linked to a `subject_field_id`.
- **`skills`** — Skill records tied to a subject and Bloom level, plus
  `synonyms` (comma-separated aliases for on-device JD/resume matching,
  migration 069).
- **`skill_prerequisites`** — Directed prerequisite edges.
- **`skill_relations`** — Non-prerequisite skill relationships.
- **`taxonomy_versions`** — Signed taxonomy version history with `cid`,
  `previous_cid`, `ratified_by`, `ratified_at`, `signature`, and `applied_at`.

### Courses and Learning (10 tables)

- **`courses`** — Course/tutorial metadata. Important fields include
  `title`, `description`, `author_address`, `author_name`, `content_cid`,
  `thumbnail_cid`, `thumbnail_svg`, `tags`, `skill_ids`, `kind`,
  `version`, `status`, `published_at`, and `on_chain_tx`.
- **`course_chapters`** — Ordered chapter rows per course.
- **`course_elements`** — Element rows with `title`, `element_type`,
  `content_cid`, optional `content_inline`, `position`, and `duration_seconds`.
- **`element_skill_tags`** — Element-to-skill mapping with `weight`.
- **`video_chapters`** — Timestamp markers for video elements.
- **`enrollments`** — Enrollment rows with `course_id`, `enrolled_at`,
  `completed_at`, `status`, and `updated_at`.
- **`element_progress`** — Per-element progress with `status`, `score`,
  `time_spent`, `completed_at`, and `updated_at`.
- **`course_notes`** — Notes scoped to an enrollment/chapter/element,
  with `content_cid`, `preview_text`, and `video_timestamp_seconds`.
- **`element_submissions`** — Plugin-graded element submissions, keyed
  to an `element_id`/`enrollment_id`, with `submission_cid`,
  `grader_cid`, `content_cid`, `score`, `score_details_json`,
  `learner_did`, an optional `signed_attestation`, and the grader's
  self-declared `grader_version` (migration 051; folded into the
  completion Merkle leaf so the on-chain witness is reproducible), and
  `answers_json` (migration 065; persisted learner responses so a
  submission resumes read-only after navigation or restart).
- **`catalog`** — Network-discovered course metadata mirroring the
  publishable subset of `courses`.

### Community Plugins (7 tables)

The community plugin system (see [`plugins.md`](plugins.md)). Plugins are
content-addressed iframe bundles; built-ins ship embedded in the host
binary, community plugins install from a local directory (Phase 1) or P2P
discovery (Phase 3).

- **`plugin_installed`** — One row per installed plugin CID: `name`,
  `version`, `author_did`, `install_path`, `source`
  (`local_file` / `builtin` / `p2p`), the full `manifest_json` captured at
  install, `installed_at`, and an `enabled` flag (migration 053 — disabled
  plugins remain installed but the player refuses to mount them).
- **`plugin_permissions`** — Per-plugin, per-capability consent grants
  (`scope` ∈ `once` / `session` / `always`). Cascades on uninstall.
- **`plugin_catalog`** — Discovery cache of plugin announcements seen on
  the `/alexandria/plugins/1.0` gossip topic (plus built-ins seeded at
  startup). A row means "heard of", not "installed".
- **`plugin_attestations`** + **`plugin_advisories`** — Plugin DAO
  multi-sig attestations binding a `(plugin_cid, grader_cid)` pair as
  credential-eligible, and advisory-only notes (deprecated / known-flawed)
  that surface in the UI without affecting recognition.
- **`plugin_irl_submissions`** — Local instructor-review inbox for the
  `irl-review` builtin (migration 053). A learner's submission queues a
  `pending` row (`submission_json` = files + comment, `skills_json` =
  declared skills); an instructor posts back `score`, `feedback`,
  `skill_ratings_json`, and flips `status` to `reviewed`. No network —
  review stays on this node.
- **`plugin_dependencies`** (migration 063) — Declared inter-plugin
  dependencies (e.g. the `codejudge-multilang` umbrella depends on the
  per-language judge plugins), so installing one auto-installs its deps.
- **`plugin_element_state`** (migration 064) — Per-element plugin state
  persisted across navigation and app restart.

### Reputation (2 tables)

> The SkillProof/evidence pipeline (`skill_assessments`,
> `evidence_records`, `skill_proofs`, `skill_proof_evidence`,
> `reputation_evidence`, `reputation_impact_deltas`) was dropped in
> migration 040. Reputation now derives directly from the
> `credentials` VC store.

- **`reputation_assertions`** — Reputation rows keyed by
  `actor_address`/`role`/`skill_id`/`proficiency_level` and a
  `window_start`/`window_end`, with `score`, `evidence_count`,
  `computation_spec`, `cid`, and distribution metrics computed over
  the actor's credentials: `median_impact`, `impact_p25`,
  `impact_p75`, `learner_count`, and `impact_variance`.
- **`reputation_snapshots`** — Snapshot/anchoring records for
  reputation assertions, keyed by actor with `tx_status` and subject.

### Integrity (Sentinel) (7 tables)

- **`integrity_sessions`** — Sentinel sessions with `status`,
  `integrity_score`, `critical_count` / `warning_count` (migration 040),
  `started_at`, and `ended_at`. `enrollment_id` is **nullable** — standalone
  assessment attempts run with a NULL enrollment (migration 070). The
  assurance ladder (migration 061) adds `assurance_level` (`'local'` default
  / `'anchored'` / `'high_assurance'`), `commitment_root`, and `anchor_ref`.
- **`integrity_snapshots`** — Snapshot rows keyed by `session_id`, with
  per-signal scores (`typing_score`, `mouse_score`, `human_score`,
  `tab_score`, `paste_score`, `devtools_score`, `camera_score`),
  `composite_score`, `anomaly_flags` (migration 040), `captured_at`, the
  ONNX paste/typing-bot classifier output `ai_paste_anomaly` (nullable),
  `gaze_offscreen_ratio` (migration 060), and `commitment_hash` (the running
  chained commitment, migration 061).
- **`integrity_attestations`** (migration 061) — committee co-signatures per
  session for the assurance ladder: `session_id`, `attestor_address`,
  `public_key`, `signature`.
- **`sentinel_priors`** — DAO-ratified training samples and model
  weights for the paste classifier. Weights rows carry `weights_cid`,
  `eval_cid`, `eval_tpr`, `eval_fpr`, and `version`; a client only
  auto-loads a weights row whose gate passes (`eval_tpr >= 0.92 AND
  eval_fpr <= 0.03`).
- **`sentinel_kill_switch`** — Single row per `model_kind`; when
  `active = 1` the client treats that classifier as disabled even if a
  ratified row exists.
- **`sentinel_weights_blocklist`** — `(model_kind, version)` pairs the
  active-classifier selector must skip, for rolling back a faulty
  ratified model without amending governance history.
- **`sentinel_user_models`** — Per-user keystroke autoencoder
  (`keystroke_ae`), mouse CNN (`mouse_cnn`), and gaze-calibration MLP
  (`gaze_calib`, a per-user 5→16→2 net) weights, keyed by
  `(user_address, device_fp_prefix, model_kind)`. Moved out of browser
  localStorage into the encrypted DB.

### P2P, Content, and Sync Support (8 tables)

- **`peers`** — Known libp2p peers with `addresses`, `roles`, and local `reputation`.
- **`pins`** — Local iroh pin state, including `size_bytes`,
  `last_accessed`, `auto_unpin`, and `pinned_at`.
- **`sync_log`** — Broadcast/receive audit trail for gossip-synced entities.
- **`content_mappings`** — IPFS CID ↔ iroh BLAKE3 bridge table.
- **`devices`** — Known devices for cross-device sync (`id`, `device_name`,
  `platform`, `peer_id`, `is_local`, timestamps). Explicit pairing
  (migration 049) adds `stake_address` (sync only proceeds when it
  matches the local identity), `shared_key` (per-pair AES-256-GCM key,
  NULL until paired), and `paired` (1 once the two-way handshake
  completed).
- **`pending_pairings`** — Short-lived pairing codes generated by this
  device and awaiting acceptance, keyed by `code_hash` with a
  `shared_key` and `expires_at`.
- **`sync_state`** — Per-device per-table watermarks plus `row_count`.
- **`sync_queue`** — Outbound row-change queue with `row_data`,
  `updated_at`, `queued_at`, and `delivered_to`.

### Governance (7 tables + 1 queue)

- **`governance_daos`** — DAO metadata scoped by `scope_type` and `scope_id`.
- **`governance_proposals`** — Proposal lifecycle rows with category,
  vote tallies, and optional `on_chain_tx`.
- **`governance_dao_members`** — DAO committee membership.
- **`governance_elections`** — Election cycles keyed by `phase`,
  proficiency gates, timing windows, and `on_chain_tx`.
- **`governance_election_nominees`** — Election nominees and results.
- **`governance_election_votes`** — Individual election votes.
- **`governance_proposal_votes`** — Individual proposal votes.
- **`onchain_governance_queue`** — Persistent queue for async governance
  submissions, with `attempts`, `last_error`, and status transitions.

### Challenges, Attestations, and Opinions (7 tables)

> The evidence-based challenge/attestation tables (`evidence_challenges`,
> `challenge_votes`, `attestation_requirements`, `evidence_attestations`)
> were dropped in migration 040 and rebuilt against the `credentials`
> VC store in migrations 042–043.

- **`completion_attestation_requirements`** — Per-course gate (keyed by
  `course_id`) for how many attestor signatures a learner's
  completion-witness tx needs before the observer auto-issues a VC, with
  `required_attestors`, `dao_id`, and optional `set_by_proposal`.
- **`completion_attestations`** — Individual attestor signatures over a
  `witness_tx_hash` (`attestor_did`, `attestor_pubkey`, `signature`,
  optional `note`; unique per `(witness_tx_hash, attestor_did)`).
- **`credential_challenges`** — Stake-based challenges against a
  `credential_id`, with `challenger`, `reason`, `stake_lovelace`,
  `stake_tx_hash`, `status` (pending/reviewing/upheld/rejected/expired),
  `dao_id`, `resolution_tx`, `signature`, and the stake-escrow lifecycle
  fields `stake_status` (none/locked/returned/forfeited) and
  `settle_tx_hash` (migration 050).
- **`credential_challenge_votes`** — Committee votes on a `challenge_id`
  (`voter`, `upheld`, optional `reason`; unique per `(challenge_id, voter)`).
- **`opinions`** — Field Commentary video takes scoped to a `subject_field_id`,
  with staked `credential_proof_ids`, signature, publication timestamps,
  and withdrawal state.
- **`opinions_pending_verification`** — Queue for opinions whose referenced
  proofs have not synced locally yet.
- **`opinion_withdrawals`** — DAO-signed withdrawal records.

### Tutoring, Classrooms, and Settings (9 tables)

- **`tutoring_sessions`** — Live tutoring session metadata:
  `title`, `ticket`, `status`, `created_at`, `ended_at`.
- **`classrooms`** — Group-space metadata with `owner_address`,
  `invite_code`, and `status`.
- **`classroom_members`** — Membership rows; migration 19 adds
  `x25519_public_key`.
- **`classroom_join_requests`** — Join request queue with review state.
- **`classroom_channels`** — Text/announcement channels per classroom.
- **`classroom_messages`** — Persisted messages with edit/delete flags.
- **`classroom_calls`** — Live classroom A/V calls backed by iroh-live tickets.
- **`classroom_group_keys`** — Encrypted per-classroom group keys for E2E messaging.
- **`app_settings`** — Unified per-profile settings KV store
  (`key TEXT PRIMARY KEY`, `value TEXT`, `scope TEXT NOT NULL`,
  `updated_at TEXT`). `scope` is one of `'sync'` (replicated across
  the user's other devices via cross-device sync; LWW on
  `updated_at`) or `'device'` (stays on this device only). The
  Rust-side typed registry (`settings::registry::keys`) is the
  source of truth for valid keys + defaults — the table only
  stores values the user has actually changed. See
  [`settings.md`](settings.md) for the architecture and the
  current list of registered settings.

### Verifiable Credentials Layer

These tables back the VC-first protocol described in
`docs/protocol-specification.md`.

- **`key_registry`** — Historical `(did, key_id)` public-key bindings with
  validity windows.
- **`credentials`** — Canonical signed VC store, with searchable mirrors
  for issuer/subject/type/skill plus revocation, suspension, and
  supersession state. Migration 040 adds on-chain witness metadata:
  `witness_tx_hash`, `witness_validator_script_hash`,
  `witness_validator_name`, and `auto_issued` (1 when the credential was
  auto-issued by the completion observer rather than manually). Migration
  068 adds `provenance` — a denormalized `ProvenanceTier` mirror of
  `credentialSubject.provenance` (`self_declared` / `document_backed` /
  `accredited_document` / `issuer_signed`) feeding the aggregation quality
  weight.
- **`credential_status_lists`** — Versioned RevocationList2020-style status bitmaps.
- **`credential_anchors`** — Per-credential integrity-anchor queue.
- **`pinboard_observations`** — Local and remote PinBoard commitments.
- **`presentations_seen`** — `(audience, nonce)` replay-protection log.
- **`derived_skill_states`** — Materialized aggregation cache for
  recruiter/consumer queries, plus `dominant_provenance` (the highest
  provenance tier among the credentials backing the skill, migration 068).
- **`credentials_pending_verification`** — Queue for VCs that arrive
  before the issuer DID document.
- **`credential_allowlist`** — Per-credential fetch policy for
  `/alexandria/vc-fetch/1.0`.
- **`completion_observations`** — The completion observer's persistent
  memo (migration 041). Keyed by `(policy_id, asset_name_hex)`, it
  records each witnessed Cardano completion mint (`tx_hash`,
  `subject_pubkey`, `course_id`, `completion_root`, `completion_time`)
  and the `credential_id` populated once the VC is auto-issued, so the
  observer neither re-issues nor misses mints that occurred while
  offline.
- **Migration 29 additions on `credentials`** — `suspended`,
  `suspended_at`, `suspended_until`, `suspended_reason`, plus an index
  on `supersedes`.

### Goals (2 tables, migration 069)

- **`goal_templates`** — DAO-ratified maps from a goal to an ideal skill
  graph: `id`, `kind` (`CHECK(exam|curriculum|job_role)`), `key`, `label`,
  optional `board` / `grade`, `skill_ids` (JSON), `taxonomy_version`,
  `dao_id`, `ratified`, `content_cid`. Genesis-seeded so day-one offline
  resolution works.
- **`goal_template_versions`** — Signed version history mirroring
  `taxonomy_versions` (`version`, `content_cid`, `ratified_by`,
  `signature`, `taxonomy_version`, `published_at`).

### Assessments (4 tables, migration 070)

- **`question_banks`** — DAO-ratified banks: `id`, `skill_id`, `label`,
  `difficulty_profile`, `taxonomy_version`, `dao_id`, `ratified`,
  `content_cid`.
- **`bank_questions`** — `id`, `bank_id`, `prompt`, `options` (JSON),
  `correct_indices` (JSON) — the answer key, held locally and **never**
  sent to the client or gossiped — `difficulty`, `points`, `rubric_version`.
- **`question_bank_versions`** — Signed version history (as above).
- **`assessment_attempts`** — Per-attempt record: `id`, `subject_did`,
  `bank_id`, `seed`, `question_ids` (JSON), `option_orders` (JSON),
  `integrity_session_id`, `score`, `passed`, `started_at`, `graded_at`.

### Organizations and Role Assessments (2 tables, migration 062)

- **`organizations`** — Enterprise sponsors: `id`, `name`, `owner_address`,
  `did`.
- **`role_assessments`** — Sponsor role/JD assessments: `id`, `org_id`,
  `role_title`, `job_description`, `course_id`, `skill_ids`,
  `issuance_policy_json`, `required_assurance_level`, `status`. See
  [`protocol-specification.md`](protocol-specification.md) §14.9.6.

## Entity Relationship Summary

```mermaid
erDiagram
    subject_fields ||--o{ subjects : contains
    subjects ||--o{ skills : contains
    skills ||--o{ skill_prerequisites : "DAG edges"
    skills ||--o{ skill_relations : relates

    courses ||--o{ course_chapters : contains
    course_chapters ||--o{ course_elements : contains
    course_elements ||--o{ element_skill_tags : tagged
    course_elements ||--o{ video_chapters : chapters
    skills ||--o{ element_skill_tags : tagged

    courses ||--o{ enrollments : has
    enrollments ||--o{ element_progress : tracks
    enrollments ||--o{ course_notes : has
    enrollments ||--o{ element_submissions : graded

    credentials ||--o{ credential_challenges : challenged
    credential_challenges ||--o{ credential_challenge_votes : votes
    completion_observations ||--o| credentials : "auto-issues"
    completion_attestation_requirements ||--o{ completion_attestations : "gated by"
    reputation_assertions ||--o{ reputation_snapshots : anchors

    integrity_sessions ||--o{ integrity_snapshots : snapshots

    governance_daos ||--o{ governance_proposals : has
    governance_daos ||--o{ governance_dao_members : members
    governance_daos ||--o{ governance_elections : runs
    governance_proposals ||--o{ governance_proposal_votes : votes
    governance_elections ||--o{ governance_election_nominees : nominees
    governance_elections ||--o{ governance_election_votes : votes

    classrooms ||--o{ classroom_members : members
    classrooms ||--o{ classroom_join_requests : requests
    classrooms ||--o{ classroom_channels : channels
    classrooms ||--o{ classroom_calls : calls
    classroom_channels ||--o{ classroom_messages : messages
    classrooms ||--o| classroom_group_keys : encryption

    devices ||--o{ sync_state : tracks

    credentials ||--o| credential_anchors : anchors
    credentials ||--o{ credential_allowlist : grants
```
