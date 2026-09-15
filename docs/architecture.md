# Alexandria — Architecture

> Offline-first, trustless, multi-platform.

> **⚠️ Post-VC-first cutover (migration 040, 2026-04-24):** The
> SkillProof pipeline described in §5–§7 (`skill_proofs`,
> `evidence_records`, `skill_assessments`, the NFT wrapper, the
> aggregator) has been retired. Credentials are now W3C Verifiable
> Credentials, including offline learner self-claims and optional
> confirmed Cardano completion witnesses. See
> [`vc-migration.md`](./vc-migration.md) for what replaces what.
> Reputation and exact course-version completion endorsement were rebuilt on
> the VC-first model. The later credential-challenge experiment and its Cardano
> escrow path have now also been retired: no challenge commands are
> registered and no challenge transaction builder is compiled. Their
> schema remains only as pre-launch legacy storage pending migration D03.

**Status**: In progress — core local/P2P flows are implemented, with some on-chain and VC presentation surfaces still partial
**Last updated**: 2026-09-15 (legacy authority retirement, versioned preprod network profile, immutable profile network identity, exact course enrollment/completion binding and endorsement persistence, snapshot credential anchoring, shared migration path, profile cleanup ownership, staged database executor, release governance gating, and genesis core identity)

---

## Table of Contents

1. [Design Philosophy](#1-design-philosophy)
2. [System Overview](#2-system-overview)
3. [Identity & Wallet](#3-identity--wallet)
4. [Database](#4-database)
5. [Content Storage (iroh)](#5-content-storage-iroh)
6. [P2P Networking](#6-p2p-networking)
7. [Cardano Integration](#7-cardano-integration)
8. [Evidence Pipeline](#8-evidence-pipeline)
9. [Governance](#9-governance)
10. [Frontend](#10-frontend)
11. [IPC Boundary](#11-ipc-boundary)
12. [Security Model](#12-security-model)
13. [Verifiable Credentials Layer](#13-verifiable-credentials-layer)

---

## 1. Design Philosophy

Alexandria eliminates all servers. Every user runs a full node — a native
application (desktop or mobile) that contains the entire platform:
database, content store, P2P networking, wallet, and UI. There is no
central API, no hosted database, and no Docker infrastructure.

**Core principles**:

- **Offline-first**: Every operation works without network access. Sync
  is opportunistic, not required.
- **Self-sovereign identity**: Your 24-word mnemonic IS your account.
  No email, no password recovery service, no OAuth provider.
- **Trustless verification**: Credentials can be verified locally and
  offline today. Optional Cardano anchoring and validator-backed
  enforcement exist in the design, but some deployment paths remain
  incomplete.
- **Privacy by architecture**: Raw behavioral data (Sentinel) never
  leaves the device. Signed evidence travels over P2P, but only
  derived scores — never biometrics.

---

## 2. System Overview

```
+-------------------------------------------------------+
|                    Tauri v2 Shell                      |
|                                                       |
|  +----------------+         +----------------------+  |
|  |   Vue 3 UI     |--IPC--->|    Rust Backend      |  |
|  |   (WebView)    |  IPC    |                      |  |
|  |                | cmds    |  +----------------+  |  |
|  |  Vue pages     |         |  |   SQLite DB    |  |  |
|  |  + components |         |  |  local schema  |  |  |
|  |  + composables|         |  |   91 migrations|  |  |
|  +----------------+         |  +----------------+  |  |
|                             |                      |  |
|                             |  +----------------+  |  |
|                             |  |  iroh store    |  |  |
|                             |  |  BLAKE3 blobs  |  |  |
|                             |  +----------------+  |  |
|                             |                      |  |
|                             |  +----------------+  |  |
|                             |  | Encrypted Vault|  |  |
|                             |  | (Stronghold or |  |  |
|                             |  |  portable)     |  |  |
|                             |  +----------------+  |  |
|                             |                      |  |
|                             |  +----------------+  |  |
|                             |  | libp2p swarm   |--+-->  P2P Network
|                             |  +----------------+  |  |
|                             |                      |  |
|                             |  +----------------+  |  |
|                             |  | Blockfrost     |--+-->  Cardano (preprod)
|                             |  +----------------+  |  |
|                             +----------------------+  |
+-------------------------------------------------------+
```

All state lives on the user's machine, organised **per profile** so a single device can host multiple isolated learners (see [`multi-user-profiles.md`](multi-user-profiles.md)):

| Store | Path (relative to app data dir) | Purpose |
|-------|--------------------------------|---------|
| Profile index | `profiles_index.json` | **Public** sidecar — display names, avatars, colors, timestamps. Rendered by the picker before any vault is unlocked. Holds no keys, DIDs, or stake addresses. |
| SQLite | `profiles/<uuid>/alexandria.db` | Relational data (courses, skills, evidence, governance) — per profile |
| Vault (desktop) | `profiles/<uuid>/vault/alexandria.stronghold` | IOTA Stronghold encrypted wallet keys and mnemonic — per profile |
| Vault (mobile) | `profiles/<uuid>/vault/vault.enc` | AES-256-GCM + Argon2id encrypted wallet keys and mnemonic — per profile |
| iroh | `profiles/<uuid>/iroh/` | Content-addressed blobs (course HTML, profiles) + per-profile node secret |
| Plugins | `profiles/<uuid>/plugins/` | Installed plugin bundles — per profile |
| Video cache | `profiles/<uuid>/videocache/` | Transient plaintext media for the asset protocol — per profile and purged during successful lock cleanup |

Default data directory: `~/Library/Application Support/org.alexandria.node/` (macOS).

On first launch after upgrading from a single-vault install, the legacy top-level layout is atomically migrated into a freshly-created `profiles/<uuid>/` slot named "My Profile" (see `src-tauri/src/profile/migration.rs`).

---

## 3. Per-Profile Identity & Wallet

Each user profile owns an independent identity. Switching profiles tears down all per-profile services (vault, DB, iroh, libp2p) and rebuilds them for the newly-unlocked profile — see `AppState::start_active_profile` / `stop_active_profile` in `src-tauri/src/lib.rs`. Two profiles on the same device are network-indistinguishable from two devices on the same LAN.

### Key Derivation

```
24-word BIP-39 mnemonic
    |
    v
BIP32-Ed25519 master key (Icarus / CIP-1852 via pallas-wallet)
    |
    +-- m/1852'/1815'/0'/0/0 --> payment key (signing + verification)
    |                              +-- bech32: addr_test1...
    |                              +-- libp2p Ed25519 keypair
    |                                    +-- PeerId: 12D3KooW...
    |
    +-- m/1852'/1815'/0'/2/0 --> stake key
                                   +-- bech32: stake_test1...
```

The payment key (m/1852'/1815'/0'/0/0) serves as:
1. Cardano payment signing key
2. GossipSub message signing key (envelope signature in `SignedGossipMessage`)
3. Content/profile document signing key
4. DID-key signing material for the VC layer

The libp2p peer identity is derived **per device** via `HKDF(payment_key, device_id)` so the same mnemonic on two devices produces distinct `PeerId`s. The stake key (m/1852'/1815'/0'/2/0) is used to sign `stake_pubkey_registration` transactions that bind a stake address to a gossip-envelope public key — see [`docs/stake-pubkey-registry.md`](./stake-pubkey-registry.md).

### Vault Storage

Keys are stored in an encrypted vault. The implementation varies by platform:

**Desktop (IOTA Stronghold)**:
- Password → Argon2id (64 MB, 3 iterations, 4 lanes) with random salt → derived key
- Salt file includes HMAC-SHA256 integrity tag
- Mnemonic stored encrypted at a fixed vault path
- Vault file: `profiles/<uuid>/vault/alexandria.stronghold` (binary, encrypted at rest)

**Mobile (Portable AES-256-GCM + Argon2id)**:
- Password → Argon2id (memory-hard KDF, 64 MB, 3 iterations) → 256-bit key
- Mnemonic encrypted with AES-256-GCM (random 96-bit nonce)
- Vault file: `profiles/<uuid>/vault/vault.enc` (salt + nonce + ciphertext)

Both share the same lock/unlock cycle: lock clears in-memory keys, unlock re-derives from mnemonic. Locking the active profile also tears down its iroh node and libp2p swarm; unlocking a different profile rebuilds them rooted at the new profile's directory.

### Lifecycle ownership and cleanup limits

The frontend hides private content immediately when locking starts and displays
`Locking…`. It finishes required frontend cleanup, including Sentinel's final
session write with the original profile token, before requesting backend lock.
A cleanup failure leaves the lock screen visible with retry; another unlock is
not admitted until cleanup succeeds. Late activation responses cannot restore
the hidden profile. Hiding content is not proof that backend resources have
already stopped.

Backend profile transitions are serialized. Sensitive commands and background
passes hold generation-bound operation leases (`profile::scope`); backend lock
closes admission and drains those leases before replacing profile resources.
The periodic Cardano/device-sync/guardian worker opts into
`ProfileLease::run_until_closed`: closing admission drops its cancellable pass
future before the lease is released. This applies to the audited provider/P2P
awaits and owned local work in that pass, not arbitrary detached native jobs or
`spawn_blocking` tasks. Ordinary admitted IPC calls retain their leases until
they finish. Synchronous work cannot be interrupted while it is executing.
After tutoring releases its native media handles, backend cleanup purges the
active profile's materialized plaintext video-cache contents. A purge failure is
a lock failure and keeps new unlocks disabled; the cache directory itself is
retained for reuse. The asset-protocol scope is not expanded to PDFs or images.

Journal-integrated Cardano submissions persist the exact signed bytes, hash,
operation identity, and recovery context before the POST. Cancellation or a lost
response does not prove rejection: the durable operation remains outcome unknown
and is reconciled as the same transaction when that profile is next available.
A signed operation is never automatically rebuilt as a replacement. Unsigned
completion intents keep their frozen evidence and persisted retry schedule.

Verification includes a loopback provider stalled during submission, production
lock cleanup using temporary encrypted profile databases/vaults, switching to a
second profile before the late response, and reopening the original profile to
reconcile the same transaction. It is not real-camera, mobile-device, live-chain,
or end-to-end teardown latency acceptance. The approved 10-second connection and
30-second total provider limits are not yet globally enabled; remaining raw
submission paths and native teardown still need review. Clearing frontend private
references is not guaranteed memory zeroization, and legacy behavioral/face
calibration in localStorage has not been migrated to encrypted storage.

### Roles, Modes & Guardianship

Every profile is a **learner**; onboarding lets a person add **instructor**
and/or **parent** on top, in any combination. The set lives in
`local_identity.account_roles` (JSON array, migration 81; always contains
`learner`), and `domain::identity::normalize_roles` is the one place it is
validated — it re-inserts `learner` whatever a caller sends. The older
single-valued `account_role` (migration 66) is still written as the first
extra role for builds that predate the set; nothing new reads it. Roles shape
the navigation and tools and drive a visual accent on the shell (amber for
instructor mode, teal when the parent role is present).

- **Role vs. mode.** An account with the instructor role gets a
  Learner↔Instructor **mode** switch (`active_mode` per-profile setting;
  `useMode` composable; `Cmd/Ctrl+Shift+M`). Parent is a **role, not a mode**:
  a parent keeps the learner home and navigation, with the guardian dashboard
  added on top (`/guardian`). A single `router.beforeEach` guard enforces
  `requiresInstructorMode` on `/instructor/*` and `requiresRole: 'parent'`
  (checked against the role set via `hasRole`) on `/guardian/*`.
- **Age gating.** Everybody provides a `birthdate` (ISO-8601, kept on-device and
  excluded from the public `SignedProfile`). Age is **recomputed** via
  `domain::identity::is_minor(birthdate, today)` on every unlock — never stored —
  so turning 18 resolves automatically. A self-asserted birthdate VC is issued
  so "store in records" is satisfied VC-first.
- **Activation.** A minor learner starts `activation_state = pending_guardian`
  and is funnelled to a holding screen (`GuardianGate.vue`) by the router guard.
  The profile flips to `active` — unblocking the app — only once a guardian link
  is established over `/alexandria/guardian/1.0` (see
  [`protocol-specification.md`](protocol-specification.md#guardian-link-protocol)).
  This is deliberately **cross-device and cross-user**: the parent is a separate
  identity on their own device, so oversight can never be forged locally.
- **Authoring & oversight.** Instructors get a unified course/tutorial
  **composer** (`/instructor/composer`, superseding the old
  `CourseNew`/`CourseEdit`/`TutorialNew` pages) plus a dashboard (per-course
  learner progress) and a submissions **inbox**. Parents get a guardian
  dashboard listing each linked child and their synced activity.

Privacy: guardianship VCs, birthdates, and `guardian_*` tables never enter
device-sync (`SYNCABLE_TABLES`) or gossip — an invariant covered by unit tests.

---

## 4. Database

**Engine**: SQLCipher (rusqlite 0.38, `bundled-sqlcipher`) — per-profile DBs are encrypted, opened with `PRAGMA key`

**Schema**: 91 versioned migrations in `src-tauri/src/db/schema.rs`. The domain table below is a selected map, not a complete live-table inventory.

| Domain | Tables |
|--------|--------|
| Identity | `local_identity`, `peer_profiles`, `username_claims` |
| Taxonomy | `subject_fields`, `subjects`, `skills`, `skill_prerequisites`, `skill_relations`, `taxonomy_versions` |
| Courses | `courses`, `course_chapters`, `course_elements`, `element_skill_tags` |
| Learning | `enrollments`, `element_progress`, `course_notes` |
| Credentials | `credentials`, `credential_status_lists`, `key_registry` |
| Reputation | `reputation_assertions`, `derived_skill_states`, `derived_skill_state_history`, `reputation_snapshots`; scoring uses filtered views |
| Issuer recognition (local-only) | `public_derived_issuers`, `derived_skill_refresh_queue` (migration 085) |
| Completion persistence (local-only) | `completion_claims`, `completion_witness_requests`, `course_completion_endorsements`; enrollments and claims freeze exact course-document identity/policy (migrations 086, 091) |
| Integrity | `integrity_sessions`, `integrity_snapshots` |
| P2P | `peers`, `pins`, `sync_log`, `catalog` |
| Governance | `governance_daos`, `governance_proposals`, `governance_dao_members`, `governance_elections`, `governance_election_nominees`, `governance_election_votes`, `governance_proposal_votes` |
| Content | `content_mappings` |
| Sync | `devices`, `sync_state`, `sync_queue` |
| Retired challenge experiment | `credential_challenges`, `credential_challenge_votes` (legacy tables; no active command or authority path) |
| Tutoring | `tutoring_sessions` |
| Classrooms | `classrooms`, `classroom_members`, `classroom_join_requests`, `classroom_channels`, `classroom_messages`, `classroom_calls`, `classroom_group_keys` |
| Governance (on-chain) | `onchain_governance_queue` |
| Settings | `app_settings` |

### Key Design Decisions

- **Deterministic IDs**: `hex(blake2b_256(parts.join("|")))` instead of server-generated UUIDs
- **Singleton identity**: `local_identity` has `CHECK (id = 1)` — exactly one row, the active profile's owner. Because each profile has its own SQLCipher database, "singleton" is scoped per profile, not per device.
- **No server tables**: No `refresh_tokens`, `oauth_accounts`, or session management
- **Content stored externally**: Course HTML and profiles live in iroh blobs, referenced by BLAKE3 hash
- **Settings live in `app_settings`**: One unified per-profile key-value table with a `scope` discriminator (`'sync'` propagates to the user's other devices; `'device'` stays here). The Rust-side typed registry (`settings::registry::keys`) is the source of truth for valid keys + defaults — the table only stores user-overridden values. See [Settings](settings.md).

### Network identity and configuration

The app embeds `src-tauri/resources/networks/preprod.json` and validates it
before opening the profile manager. The strict version-1 profile owns the
network ID, Cardano network and magic, relay PeerIds and DNS names, public
fallback IPs, HTTPS registry origins, receipt issuers, stake-registry founder
keys, the signed bootstrap-registry SHA-256 identity, optional service
identities, and the intended protocol namespace. Unknown fields, duplicate JSON
keys, placeholders, malformed trust roots, inconsistent optional-service
settings, and a bootstrap-registry digest mismatch prevent application setup.

`profiles_index.json` is format version 2 and records an immutable `network_id`
for every profile. The manager rejects a version-1 index, a future index format,
or any profile whose network differs from the embedded profile. New-profile and
mnemonic-restore IPC requests must name the expected network.

The profile currently supplies relay discovery, relay receipt trust, bootstrap
founder keys, HTTPS relay-registry origins, the optional governance anchor, and
the namespace used in DHT provider-record keys. Its
`protocol_namespace = "/alexandria/preprod"` is the target for all wire protocol
IDs, but GossipSub and request-response paths remain the unscoped paths below
until the app, relay, and monitoring services migrate in lockstep.

See [Database Schema](database-schema.md) for the full DDL.

### Profile-fenced database executor

Interactive commands already migrated from direct SQLite locking submit work to
one bounded executor thread that owns access to the existing single connection.
Synchronous SQL therefore does not block a Tokio runtime worker. Every production
job carries the active profile lease: work still waiting when profile admission
closes is rejected before it can acquire the database, while a transaction that
has started retains its lease and finishes before lock cleanup may reuse the
shared slot. Network, provider, and vault operations are kept outside executor
closures. A job that panics while holding the connection is caught inside the
guard: any transaction it left open is rolled back and the caller receives an
error, so the shared database mutex is not poisoned and later jobs still run.

The waiting queues reserve 16 learner, 8 instructor, and 8 background slots.
Filling one lane cannot consume another lane's reservation. A full lane rejects
new work immediately with a retryable busy error instead of allocating an
unbounded backlog. When all lanes remain backlogged, dispatch follows an exact
8 learner : 4 instructor : 1 background schedule; empty lanes are skipped so
the connection does not idle. Warnings report queue time, shared-slot lock wait,
and SQL/closure execution separately against 250 ms learner, 500 ms instructor,
and 2 s background diagnostic thresholds.

This is a staged cutover, not a claim that every database caller has migrated or
that the thresholds are device SLAs. Lifecycle/startup code and command modules
with more complex external-I/O boundaries still use direct exclusive access.
The executor has no read pool, and representative mobile measurements, lock
latency, battery cost, backlog recovery, and end-to-end user-facing overload
handling remain release acceptance work.

### Shared migrations and issuer recognition

The app and CLI use `db::run_migrations_on_connection`: applied `(version, name)` records must be a supported schema prefix, and each migration's DDL/data changes and tracking record commit together. A failed migration rolls back before retry; the CLI no longer maintains an independent runner. Both app and CLI connection-opening paths install `legacy_course_authority_did`, a deterministic, side-effect-free SQL function, after encryption-key configuration and before schema-dependent writes. The function reproduces a historical public-derived DID; it does not confer issuer trust.

Migration 085 stores exact public author-address/DID matches in `public_derived_issuers`, with a schema check on the derivation. Course insert/update triggers maintain recognition; normal course edits/removal do not remove earlier matches. `scoring_credentials` excludes only stored payload issuers matching that table. It does not classify credentials from assessment labels or missing metadata, and it does not modify credential bytes or revocation state.

Recognition atomically removes affected current skill caches, queues reconstruction, marks affected reputation rows for in-place repair, and invalidates affected historical score points. `current_reputation_assertions` hides pending/excluded rows. Skill/reputation readers repair the affected derived state; failed repair remains retryable and cannot reinstate an old inflated cache. Historical rows remain stored with validity metadata (normal same-day skill-history replacement still applies). Expression/partial indexes support exact issuer lookup and pending reputation repair; workload latency and memory budgets still require the planned device benchmarks.

---

## 5. Content Storage (iroh)

**Engine**: iroh 1.0.2 / iroh-blobs 0.103 with `fs-store` backend

iroh provides a BLAKE3 content-addressed blob store. Content is
identified by its hash, ensuring integrity and deduplication.

### Two transports, and which does what

Alexandria runs **two independent networking stacks**. They are not redundant
and neither can currently do the other's job:

| | libp2p (§6) | iroh (this section) |
|---|---|---|
| Carries | mesh: discovery, gossip, sync, pairing, guardian, username receipts | content blobs, tutoring media (MoQ), room presence |
| Relays | Circuit Relay v2 + DCUtR, self-hosted | iroh relays |
| Discovery | private Kademlia DHT | DNS / pkarr |
| Identity | libp2p `PeerId` | iroh `EndpointId` |

Each device therefore holds **two Ed25519 identities**, generated and persisted
independently, and there are **two gossip implementations** in the binary
(`libp2p::gossipsub` in `p2p/network.rs`, `iroh_gossip` in
`content_store/node.rs`).

### Relay and discovery dependency

`ContentNode::start` builds its endpoint with
`Endpoint::builder(iroh::endpoint::presets::N0)`, which is iroh's default
preset: **n0's public relay servers and n0's DNS discovery**.

This matters and is easy to miss. The libp2p relays are self-hosted, are
listed in an on-chain registry, and anyone can run one. The iroh side is not
equivalent — when a direct connection cannot be established, blob transfer and
tutoring media fall back to relays operated by a third party (N0, INC), and
node discovery resolves through their DNS. Traffic stays end-to-end encrypted,
so a relay cannot read content, but it does observe connection metadata: which
endpoints talk to each other, when, and how much.

Self-hosting iroh relays and pointing the endpoint at a custom `RelayMap`
removes the dependency. Until that happens, "no central server" is true of
the libp2p mesh and of application state, but not of iroh's fallback path.

### MoQ is registered unconditionally

The router accepts three ALPNs — `iroh_blobs`, `iroh_gossip`, and `live::ALPN`
(Media over QUIC). The MoQ handler is **not** gated behind the
`tutoring-video*` features or any `cfg`, so every node accepts MoQ connections
whether or not the user ever joins a tutoring session. The feature flags gate
only the codec layer (ffmpeg, VideoToolbox, Opus), not the protocol
registration.

### Operations

| Operation | Description |
|-----------|-------------|
| `add_bytes(data)` → hash | Store content, get BLAKE3 hash |
| `get_bytes(hash)` → data | Retrieve by hash |
| `has(hash)` → bool | Check existence |

### Resolution Chain

When resolving content by CID or hash:

1. **Local iroh store** — instant, offline
2. **iroh peer fetch** — pull from PinBoard pinners over the P2P network
3. **Public URL fallback** — fetch from the mapped public URL origin over HTTP, then cache into the iroh store

### Content Types

- **Course documents**: Signed JSON with chapters, elements, content hashes
- **User profiles**: Signed JSON with display name, bio, avatar CID, skills

Both use Ed25519 signatures for authenticity verification.

### Per-profile lifecycle

The iroh node is a per-device **singleton** (`AppState::content_node:
Arc<ContentNode>`) but is repointed at the active profile's blob
directory on every unlock via `ContentNode::set_data_dir`. Lock
(`stop_active_profile`) calls both `Router::shutdown` AND
`Store::shutdown` — the latter is required so the blob-store actor
exits its private tokio runtime and releases the redb file lock.
Without `Store::shutdown`, a follow-up `FsStore::load` on the same
path within the same process hangs indefinitely.

---

## 6. P2P Networking

**Stack**: libp2p 0.56 via rust-libp2p

### Protocols

| Protocol | Purpose |
|----------|---------|
| GossipSub v1.1 | Topic-based pub/sub with peer scoring |
| Kademlia | Private DHT (`/alexandria/kad/1.0`) — peer discovery via relay bootstrap |
| Identify | Peer info exchange, agent version |
| AutoNAT | NAT reachability detection |
| Relay Server | Circuit Relay v2 server (for nodes that can serve as relays) |
| Relay Client | Circuit Relay v2 client (NAT traversal via relay) |
| DCUtR | Direct connection upgrade (hole punching) |

### Topics

| Topic | Path | Content |
|-------|------|---------|
| Catalog | `/alexandria/catalog/1.0` | Course announcements |
| Taxonomy | `/alexandria/taxonomy/1.0` | DAO-ratified skill graph updates |
| Governance | `/alexandria/governance/1.0` | Proposals, elections, committee updates |
| Profiles | `/alexandria/profiles/1.0` | User profile announcements |
| Opinions | `/alexandria/opinions/1.0` | Subjective ratings on courses, peers |
| Peer Exchange | `/alexandria/peer-exchange/1.0` | Known peer address propagation |
| VC DID | `/alexandria/vc-did/1.0` | DID document + key-rotation announcements |
| VC Status | `/alexandria/vc-status/1.0` | RevocationList2020 status-list snapshots and deltas |
| VC Presentation | `/alexandria/vc-presentation/1.0` | Opt-in selective-disclosure presentation envelopes |
| PinBoard | `/alexandria/pinboard/1.0` | PinBoard pinning commitment observations |
| Plugins | `/alexandria/plugins/1.0` | Community plugin announcements |
| Plugin Attestations | `/alexandria/plugin-attestations/1.0` | Reserved compatibility topic; subscribed and scored, but grants no authority and has no inbound persistence handler |
| Sentinel Priors | `/alexandria/sentinel-priors/1.0` | Ratified Sentinel adversarial-prior metadata |
| Goal Templates | `/alexandria/goal-templates/1.0` | DAO-ratified goal → skill-graph templates |
| Question Banks | `/alexandria/question-banks/1.0` | DAO-ratified assessment question banks |

The taxonomy, governance, goal-template, and question-bank topics are still subscribed and validated, but release builds reject every inbound message on them before any database read or write (no rows, sync-log entry, or UI event) until handlers consume verified committee outcome certificates. The legacy apply paths compile only in debug builds with `legacy-taxonomy-ratification`, `legacy-local-governance`, or `legacy-content-ratification`.

Six request-response protocols (libp2p `request-response` + CBOR
codec) run alongside the gossip mesh and are not part of the
gossip-topic set: `/alexandria/vc-fetch/1.0` handles
authority-respecting credential pull, `/alexandria/sync/1.0`
carries AES-256-GCM-sealed cross-device sync payloads between paired
devices, and `/alexandria/guardian/1.0`, `/alexandria/graph-fetch/1.0`,
`/alexandria/profile-fetch/1.0`, and `/alexandria/username-reg/1.0` carry
the guardian link, public skill-graph, public profile, and username-receipt
exchanges.

### Message Flow

1. Serialize domain payload to JSON
2. Sign with Cardano Ed25519 key
3. Wrap in `SignedGossipMessage` envelope (payload + signature + public_key + stake_address + timestamp)
4. Publish to GossipSub topic

### Validation Pipeline (6 steps)

1. **Signature** — Ed25519 verify (covers all envelope fields: topic, timestamp, stake_address, payload)
2. **Identity Binding** — for privileged topics (taxonomy, governance, Sentinel priors, goal templates, question banks, and the reserved plugin-attestation compatibility topic) the `(stake_address, public_key)` pair MUST appear in the local `stake_pubkey_registry` within the current validity window; non-privileged topics skip this step. Registry validation of the reserved topic does not grant domain authority. See [`docs/stake-pubkey-registry.md`](./stake-pubkey-registry.md).
3. **Freshness** — within ±5 minutes
4. **Dedup** — Blake2b-256 hash in LRU cache (100K entries, least-recently-used eviction)
5. **Schema** — valid JSON
6. **Authority** — in release builds the taxonomy, governance, Sentinel-prior, and content-governance (goal-template, question-bank) handlers reject every message pending verified committee outcome certificates; their legacy committee-membership checks against local governance tables compile only in debug builds with the matching `legacy-*` feature

Validation outcomes feed directly into gossipsub peer scoring: `Reject` on signature, envelope-parse, or identity-binding failure penalises the source through the per-topic `invalid_message_deliveries` weight (see `p2p/scoring.rs`); `Accept` rewards first-delivery scoring for valid messages.

### Rate Limiting

Per-peer token-bucket rate limiter (20 messages per 60 seconds, 1 refill per 3 seconds) applied before the validation pipeline. Peer state is cleaned up on disconnect.

### Dynamic Topics

In addition to the 15 global topics, classrooms use per-classroom dynamic topics:
- Message topic: `/alexandria/classroom/{id}/1.0`
- Meta topic: `/alexandria/classroom/{id}/meta/1.0`

Nodes subscribe/unsubscribe as they join/leave classrooms.

### Classroom End-to-End Encryption

Classroom messages are encrypted with AES-256-GCM using a per-classroom group key:

1. **Key generation** — Owner generates a random AES-256 group key when the classroom is created.
2. **Key distribution** — When a member is approved, the owner/moderator encrypts the group key for the new member's X25519 public key via ECDH and broadcasts a `KeyDistribution` meta event (base64-encoded `nonce || ciphertext`).
3. **Message encryption** — Senders encrypt message content with the group key. The `ClassroomMessagePayload` carries `encrypted: true` and `key_version` to identify the key used.
4. **Key rotation** — On member removal (kick/leave), the group key is rotated and redistributed to remaining members. The `key_version` field is incremented.

Database tables: `classroom_group_keys` (encrypted key + version per classroom), with `x25519_public_key` columns on `classroom_members` and `local_identity`.

### Cross-Device Sync

Private encrypted sync between devices sharing the same mnemonic:

- Sync key: `blake2b_256(signing_key_bytes ++ "alexandria-cross-device-sync-v1")`
- LWW (Last-Writer-Wins) merge for enrollments, progress, notes
- Append-only merge for evidence records
- SQL injection prevention via table name allowlist

See [Protocol Specification](protocol-specification.md) for full wire formats.

---

## 7. Cardano Integration

**Network**: Preprod testnet
**Client**: Blockfrost REST API
**Tx builder**: pallas 0.35 (Conway era)

### Capabilities

| Feature | Implementation |
|---------|---------------|
| UTxO queries | Blockfrost REST (`/addresses/{addr}/utxos`) |
| Protocol parameters | Blockfrost REST (`/epochs/latest/parameters`) |
| Transaction submission | Blockfrost REST (`/tx/submit`) |
| Fee estimation | Linear fee model from protocol params |
| VC integrity anchoring | Metadata-only tx (label 1697) timestamping the canonical VC hash |
| Completion-witness minting | Merkle-root completion witness; validator deployed |
| Reputation snapshots | Signed as-of `DerivedCredential`; optional metadata-only anchor of its canonical VC hash |
| Governance metadata | DAO/election/proposal tx builders and queue records; validator-backed enforcement is still pending |
| Coin selection | Greedy UTxO selection with min-ADA enforcement |

### Transaction Types

1. **VC Integrity Anchor** — Metadata-only transaction (label 1697) that timestamps the canonical hash of a W3C Verifiable Credential without publishing credential content. (The legacy SkillProof NFT and course-registration mints were retired in migration 040.)
2. **Completion Witness** — Mints a completion witness keyed to the Merkle root of a learner's graded element submissions; the completion validator is deployed.
3. **Reputation Snapshot** — Local signed `DerivedCredential` over all currently eligible evidence; optional metadata-only VC-hash anchor uses the credential queue
4. **Governance Actions** — Metadata-bearing transactions and queue entries for DAO ops, elections, proposals, votes

---

## 8. Credential Pipeline

> Post-VC-first cutover (migration 040): the legacy evidence →
> aggregator → SkillProof-NFT pipeline (`evidence_records`,
> `skill_proofs`, the `evidence/aggregator`, the on-chain NFT mint, and
> the `/alexandria/evidence/1.0` broadcast) was retired. Credentials are
> W3C Verifiable Credentials.

### Flow

```
Assessment completion (plugin grader → element_submissions row:
    score, grader_cid, submission_cid, grader_version)
    |
    v
claim_course_completion: assemble passing element submissions in
    course-template order → completion Merkle root
    → atomic local receipt + learner-signed SelfAssertion per skill
      + optional durable witness request (no instructor signature)
    (content-only courses with no gradeable elements issue at a
     baseline proficiency, no witness)
    |
    v
Profile-scoped background worker → Completion-witness tx (completion.ak) as an
    on-chain ANCHOR — treasury-funded when ALEXANDRIA_TREASURY_* is
    configured (learner still signs for validator identity), else
    learner-funded; failure does not block the local credential
    |
    v
Confirmed successful ledger receipt → matching completion observation
    → optional witnessed completion VC, signed by the learner
    |
    v
on_credential_accepted → distribution-based reputation
    (median/p25/p75/variance per skill, from `scoring_credentials`)
```

The completion command performs no Cardano I/O. Migration 086 atomically persists local claims, their reusable receipt, enrollment completion, and an optional witness intent. The same learner/course/root returns the original credential IDs on retry. Only a completion requested with Cardano configured queues an intent; later configuration alone does not enqueue earlier local-only claims. Frozen evidence survives restart, and changed evidence never inherits an earlier transaction's witness.

`cardano::completion_queue` uses the existing background worker's profile lease and unlocked wallet, verifies the saved wallet binding, and attempts at most one unsigned request per pass. Retry scheduling is durable (30-second increments, capped at five minutes, subject to the worker's cadence). Journal handoff removes signed requests from the dispatch index atomically. Receipt reconciliation and witnessed-VC issuance remain separate; backend locking cancels the audited worker pass before releasing its lease, rather than switching a profile underneath it. The UI queries indexed local witness status separately from local credential progress and clears private state on locking. See [Lifecycle ownership and cleanup limits](#lifecycle-ownership-and-cleanup-limits) for implemented cancellation and remaining deadline/native-device checks; this is not a measured device-latency guarantee.

The optional witness transaction is durably checkpointed before submission. A timeout or lost response retains the same signed transaction for reconciliation; neither acknowledgement nor an uncertain outcome becomes a witnessed credential. A genuine instructor endorsement is separately signed by an attestor authorized in the exact author-signed course document. The backend exports, signs, verifies, imports, and counts exact-binding endorsements; the user-facing review and transport flow remains pending.

### Components

| Module | Responsibility |
|--------|---------------|
| `evidence/reputation` | Distribution-based reputation from `scoring_credentials`, plus in-place repair of invalidated rows |
| `evidence/taxonomy` | Bloom's level thresholds and skill graph traversal |
| `evidence/thresholds` | Configurable proof thresholds per proficiency level |

Exact course-version endorsement policy and artifacts are handled by
`commands::attestation`, outside the retired `skill_proof` aggregator. The
credential-challenge command module, evidence challenge module, challenge domain
types, escrow transaction builders, recovery worker, validator source, and
frontend surfaces have been removed. Credential revocation, suspension, and
reinstatement are issuer actions: the active profile must derive the same issuer
DID named by both the credential and its status list. A subject, committee, or
arbitrary peer cannot mutate another issuer's credential status.

---

## 9. Governance

### Structure

- One DAO per subject field or subject
- Each DAO identity is the domain-separated BLAKE2b-256 digest of its founding-genesis core; it identifies a DAO only once all seven founders' acceptances verify
- A founding genesis names seven independently controlled committee members; every member accepts the same core with its identity, consensus, and governance keys, and each member ID must be the `did:key` of that member's identity key
- Operational submission receipts and final outcomes require five of the seven committee members
- Committees gate taxonomy updates and DAO-ratified content (goal templates, question banks); until verified committee certificates are wired in, release builds disable the legacy local paths for both (see [Features](#features))

### Trust bootstrap and import

The canonical JSON founding-genesis envelope is the authoritative bootstrap artifact. It is limited to 256 KiB, contains no self-referential DAO ID, and is accepted only after canonical decoding, validation of all seven founders' three key-control signatures, bounded display and identifier text, a strict CometBFT chain-ID grammar, and integers no larger than 2^53−1. The DAO ID is derived from the canonical core alone, so re-signing an unchanged core cannot mint a second ID, while verification still requires every acceptance. The exact verified bytes are persisted when a learner explicitly pins that DAO; pinning is a single conflict-safe insert, and a differently signed valid envelope for an already pinned core succeeds without replacing the stored bytes (`newly_pinned: false`, `stored_envelope_differs: true`); a matching name or scope does not confer authority, and sync, discovery, Cardano, or an application update cannot create or replace the pin.

Portable QR codes and deep links carry discovery metadata only. A locator is limited to 2 KiB and contains the DAO ID, the BLAKE3 content hash of the canonical JSON, and two to eight content-addressed sources on distinct origins. Sources are either `iroh://<BLAKE3>` or HTTPS URLs whose unsent `#blake3=<BLAKE3>` fragment binds the expected bytes. HTTPS sources must use the default port and a public DNS name: plain `http`, userinfo, IP literals, trailing-dot hosts, and special-use names are refused. Sources are normalized and counted per origin (the iroh network or one HTTPS host), so different paths or spellings of one host count once. Only the `alexandria://governance/genesis/<dao-id>` custom scheme and the corresponding `https://alexandria.ifftu.dev/governance/genesis/<dao-id>` app link are accepted.

Import is deliberately split into three user-visible steps:

1. Opening or pasting a locator parses, normalizes, and displays its identifiers without network access or state mutation. Retrieval and the QR code use the reviewed canonical encoding, not the typed text.
2. An explicit retrieve action fetches only the reviewed sources, recomputes the BLAKE3 hash, verifies the canonical envelope and derived DAO ID, and displays every material trust fact, including the core hash and the BLAKE3 envelope hash.
3. An explicit pin action requires the learner to enter the complete derived DAO ID and stores the exact reviewed bytes.

Neither locator review nor retrieval auto-pins content. Retrieval races at most three sources, with a 10 s limit per source and 30 s overall. It performs no blocking DNS lookup inside that budget: reviewed hosts are checked without DNS, the HTTP client's connect-time resolver rejects non-public addresses, redirects are not followed, and a missing iroh blob does not fall back to any other URL mapped to the same hash.

### Features

| Feature | Status |
|---------|--------|
| Founding-genesis verification and explicit local pinning | Implemented; governance activation is not yet wired to it |
| Locator/deep-link review, verified retrieval, and QR display | Implemented; locator publishing/export and retrieval scheduling remain open |
| Legacy operator DAO creation | Development-only behind `legacy-governance-bootstrap`; not a production authority path |
| Legacy local elections, proposals, committee install, and operator governance transactions | Development-only behind `legacy-local-governance`; the twelve state-changing commands return a disabled error in release, inbound governance gossip is rejected, and the operator queue builds no governance transactions |
| Legacy goal-template and question-bank ratification | Development-only behind `legacy-content-ratification`; the five content commands return a disabled error in release and inbound version documents are rejected |
| Committee management | Legacy implementation, development-only (above); a committee install fails in every build unless every elected winner resolves to a registered key |
| Proposal lifecycle (draft → published → approved/rejected) | Legacy implementation, development-only (off-chain; outcome anchored) |
| Election lifecycle (nomination → voting → finalized) | Legacy implementation, development-only (off-chain; finalized election published on-chain) |
| 2/3 supermajority voting | Legacy implementation, development-only (off-chain tally over signed gossiped votes) |
| Signed-vote + full-lifecycle P2P gossip | Legacy implementation, development-only; release rejects inbound events |
| On-chain (operator-signed): DAO create, election finalize, committee install, proposal-outcome anchor | Legacy implementation, development-only; release reconciles and confirms journaled submissions only |
| Per-vote / per-transition on-chain Plutus spends | Not used (lean model — validators deployed as the upgrade path) |

Governance runs a **lean** on-chain model: the live state machine is local
SQLite, votes and the election lifecycle propagate as signed P2P gossip, and
only the four operator-signed facts above are written to Cardano (the proposal
anchor carries the tally plus a Merkle root over the signed votes, so the
off-chain tally is auditable). The full per-transition Plutus spend validators
are deployed + verified on preprod but are not on the live path.

That lean model is not the approved five-of-seven committee model, so release
builds disable its authority as listed above. Listing and reading DAOs,
elections, proposals, and on-chain queue status still work. Release seeding
keeps the neutral DAO rows other features depend on but creates no
committees, elections, proposals, or votes. The operator key only pays for and
signs transactions; it is never installed as a committee in place of
unresolvable winners.

---

## 10. Frontend

**Stack**: Vue 3 + TypeScript + Vite + Tailwind CSS v4

### Pages

| Page | Route | Description |
|------|-------|-------------|
| Profile Select | `/profiles` | Multi-user picker — avatar grid + slide-in password panel. Cmd/Ctrl+Shift+U from anywhere. |
| Onboarding | `/onboarding` | Display name + password → wallet creation (new) or mnemonic restore (existing). Creates a new profile. |
| Home | `/home` | Dashboard overview |
| Courses Index | `/courses` | Browse course catalog |
| Course Detail | `/courses/:id` | Course info, chapters, enrollment |
| Course Player | `/learn/:id` | Content player (text, video, quiz) |
| Composer (new) | `/instructor/composer/new` | Create a course or tutorial (supersedes the old course editor) |
| Composer (edit) | `/instructor/composer/:id` | Edit an existing course or tutorial |
| Skills & Credentials | `/skills` | Combined surface — the user's credentials (default tab), the skill graph, and taxonomy browsing |
| Skill Detail | `/skills/:id` | Skill info, prerequisites, proofs, and related content (courses teaching it + opinions in its field, ranked by goal alignment) |
| Governance Index | `/governance` | Browse DAOs |
| Governance Trust Import | `/community/import` | Review a bounded genesis locator, explicitly retrieve and verify the canonical founding document, then explicitly pin its full DAO ID |
| DAO Detail | `/governance/:id` | DAO info, proposals, elections |
| Classrooms Index | `/classrooms` | List joined classrooms |
| Classroom Detail | `/classrooms/:id` | Channels, messages, active calls (voice/video desktop only; mobile stubs) |
| Classroom Settings | `/classrooms/:id/settings` | Role management, archive |
| Join Requests | `/classrooms/:id/requests` | Review pending join requests |
| Tutoring Index | `/tutoring` | Live tutoring sessions list |
| Tutoring Session | `/tutoring/:id` | Active video/audio/screen session |
| My Courses | `/dashboard/courses` | Enrolled courses, progress |
| Credentials | `/dashboard/credentials` | W3C Verifiable Credentials — the same view is embedded as the default tab of `/skills` |
| Reputation | `/dashboard/reputation` | Distribution-based reputation (median/p25/p75) |
| Network | `/dashboard/network` | P2P status, connected peers |
| Sync | `/dashboard/sync` | Cross-device sync status |
| Sentinel | `/dashboard/sentinel` | Integrity training, sessions |

### Design System

CSS custom properties with light/dark mode via `.dark` class on `<html>`:

- Custom component classes: `.btn`, `.card`, `.card-interactive`, `.input`, `.badge`, `.prose`
- Color system: space-separated RGB triplets (e.g., `--color-primary: 79 70 229`)
- Tailwind v4 `@custom-variant dark (&:is(.dark *))` for class-based dark mode
- FOUC prevention: inline `<script>` in `index.html` applies theme before CSS loads

---

## 11. IPC Boundary

The frontend communicates with the Rust backend through the handlers registered
in `tauri::generate_handler!`. The `commands/` directory also contains internal
helpers and platform-conditional tutoring variants, so source-file or attribute
counts do not equal the command surface. The table below is a non-exhaustive
sample of command modules; inspect `lib.rs` for the authoritative registration
list.

| Module | Commands | Examples |
|--------|----------|---------|
| classroom | 24 | `classroom_create`, `classroom_approve_member`, `classroom_send_message`, `classroom_start_call` |
| governance | 20 | `list_daos`, `submit_proposal`, `cast_proposal_vote`, `open_election`, `finalize_election` (in release builds `create_dao` and the twelve state-changing election/proposal commands return a disabled error; list/get and queue-status commands work) |
| tutoring | 15 | `tutoring_create_room`, `tutoring_join_room`, `tutoring_toggle_video` |
| taxonomy | 15 | `list_skills`, `list_subjects`, `propose_taxonomy_change`, `list_skill_graph_edges` |
| profile | 9 | `list_profiles`, `get_active_profile_id`, `create_profile`, `restore_profile_with_mnemonic`, `unlock_profile`, `lock_profile`, `rename_profile`, `set_profile_avatar`, `delete_profile` |
| identity | 8 | `export_mnemonic`, `is_biometric_available`, `get_wallet_info`, `get_local_did`, `get_profile`, `update_profile`, `publish_profile`, `resolve_profile` (lifecycle commands moved to `profile` module) |
| settings | 3 | `list_settings`, `set_setting`, `reset_setting` — drives the unified per-profile settings store. See [`settings.md`](settings.md). |
| credentials | 10 | `issue_credential`, `list_credentials`, `get_credential`, `verify_credential_cmd`, `revoke_credential`, `suspend_credential`, `reinstate_credential`, `allow_credential_fetch`, `disallow_credential_fetch`, `export_credentials_bundle` |
| sync | 8 | `sync_status`, `sync_now`, `sync_set_auto`, `sync_list_devices` |
| courses | 9 | `create_course`, `get_course`, `list_courses`, `set_course_completion_policy`, `publish_course` |
| attestation | 4 | `get_course_completion_endorsement_request`, `sign_course_completion_endorsement`, `import_course_completion_endorsement`, `get_course_completion_endorsement_status` |
| opinions | 6 | `publish_opinion`, `list_opinions`, `withdraw_own_opinion` |
| integrity | 6 | `integrity_start_session`, `integrity_submit_snapshot`, `integrity_get_session` |
| sentinel_ml | 11 | `sentinel_score_paste`, `sentinel_train_keystroke_ae`, `sentinel_score_keystroke_ae`, `sentinel_train_mouse_cnn`, `sentinel_score_mouse_cnn`, `sentinel_user_models_status`, `sentinel_load_dao_classifier`, `sentinel_paste_classifier_info`, `sentinel_revert_classifier_to_bundled`, `sentinel_extract_digraphs`, `sentinel_reset_user_models` |
| sentinel_priors | 9 | `sentinel_propose_prior`, `sentinel_ratify_prior`, `sentinel_priors_list`, `sentinel_priors_sync`, `sentinel_priors_load`, `sentinel_get_active_paste_classifier`, `sentinel_set_kill_switch`, `sentinel_blocklist_version`, `sentinel_unblocklist_version` |
| content | 6 | `content_add`, `content_get`, `content_resolve` |
| pinning | 5 | `declare_pinboard_commitment`, `revoke_pinboard_commitment`, `list_my_commitments`, `list_incoming_commitments`, `get_quota_breakdown` |
| storage | 4 | `storage_stats`, `storage_get_quota`, `storage_set_quota`, `storage_evict_now` |
| snapshot | 4 | `snapshot_reputation`, `submit_snapshot_tx`, `list_snapshots`, `get_snapshot` |
| reputation | 3 | `list_reputation_rows`, `get_reputation`, `recompute_reputation_for_subject` |
| enrollment | 4 | `enroll`, `update_progress`, `get_progress`, `list_enrollments` |
| elements | 4 | `list_elements`, `create_element`, `update_element`, `delete_element` |
| chapters | 4 | `list_chapters`, `create_chapter`, `update_chapter`, `delete_chapter` |
| catalog | 4 | `search_catalog`, `get_catalog_entry`, `bootstrap_public_catalog` |
| p2p | 4 | `p2p_start`, `p2p_stop`, `p2p_status`, `p2p_peers` |
| evidence | 1 | `list_reputation` (legacy read surface; `skill_proofs`/`evidence` listings retired in migration 040 — use `list_credentials`) |
| aggregation | 3 | `get_derived_skill_state`, `list_derived_states`, `recompute_all` |
| presentation | 2 | `create_presentation`, `verify_presentation` |
| plugins | 24 | `plugin_install_from_file`, `plugin_submit_and_grade`, `plugin_browse_catalog`, `plugin_list_dependencies`, capability grant/revoke, and the `irl_*` review inbox. Legacy attestation ingest/status IPC is retired. |
| health | 4 | `check_health`, `read_diag_log`, `frontend_log`, `release_secure_input` |

Note: `tutoring` has platform-specific variants. Desktop and Android share the full manager (`tutoring/manager.rs`); iOS has its own AVFoundation/VideoToolbox manager (`tutoring/manager_mobile.rs`); only targets that are neither desktop, iOS, nor Android fall back to `commands/tutoring_stubs.rs`. Counts reflect the unique commands registered for the current build; tally is approximate and shifts with each PR.

---

## 12. Security Model

### Threat Mitigations

| Threat | Mitigation |
|--------|-----------|
| Key theft | Per-profile encrypted vault — Stronghold (desktop) or AES-256-GCM + Argon2id (mobile). Compromising one profile's vault does not expose any other profile on the same device. |
| Message forgery | Ed25519 signatures on all gossip messages |
| Sybil attacks | IP colocation scoring, signed messages, subject-scoped aggregation, and independence penalties |
| Taxonomy corruption | Committee authority verification, strongest peer scoring penalty |
| Evidence inflation | Exact author-signed completion policy, allowlisted distinct-attestor thresholds, issuer/provenance weighting, anti-gaming aggregation, and behavioral integrity |
| Replay attacks | ±5 minute freshness window, Blake2b-256 dedup cache |
| Content tampering | BLAKE3 content addressing (iroh), Ed25519 signed documents |

### Privacy Guarantees

- Raw biometric data (keystrokes, mouse movements, face embeddings) **leaves the device only
  when the learner sends it themselves** — a flagged session's evidence, released by them to
  contest the flag. Nothing can request it and no automatic path exists. See
  [`sentinel.md`](sentinel.md#review-and-adjudication).
- Only derived integrity scores (0.0-1.0) are stored and transmitted otherwise
- Cross-device sync is encrypted with a key derived from the wallet signing key
- Public gossip contains only evidence scores and governance actions — no personal data beyond stake addresses

---

## 13. Verifiable Credentials Layer

VC-first credential model implemented in PRs 2–13 from the normative
spec at [`docs/protocol-specification.md`](protocol-specification.md).
The layer is local-first, signature-verifiable without
Alexandria infrastructure (§20.4 survivability). The legacy
skill-proof + NFT pipeline it replaced has been deleted (no
`mint_skill_proof_nft` / `register_course_onchain` remain in the code).

### Six layers

1. **Identity** — `did:key` self-resolving DIDs over Ed25519 keys
   (`crypto::did`, PR 3). Historical key rotation tracked in
   `key_registry` so credentials signed under a pre-rotation key
   still verify.
2. **Credential** — W3C-style Verifiable Credentials with
   `Ed25519Signature2020` detached JWS proofs over JCS-canonical
   bytes (`domain::vc`, PR 4). Stored in `credentials` (PR 5).
3. **Status** — RevocationList2020-style bitmap per issuer in
   `credential_status_lists`. Versioned to prevent rollback on
   gossip propagation. PR 5 + PR 9.
4. **Anchoring** — Per-credential integrity anchor queue
   (`cardano::anchor_queue`) writes BLAKE3(JCS bytes) into
   metadata-only Cardano txs. Idle-node contract: silently
   no-ops without Blockfrost / wallet credentials. PR 8 (queue),
   PR 16 (real submission).
5. **Aggregation** — Deterministic, explainable trust scores via
   the §14 weighted-mean + saturating-confidence pipeline
   (`aggregation::aggregate_skill_state`, PR 6) + anti-gaming
   (cluster cap, inflation z-score, PR 7). App scoring inputs exclude
   exact-match public-derived issuers (migration 085; §4), without
   quarantining unmatched historical attestations.
6. **Presentation** — Selective-disclosure envelopes signed by the
   subject (`commands::presentation`). JCS-canonical payload bound
   to (audience, nonce); replay-protected via `presentations_seen`.
   PR 11.

### Survivability (§20.4)

The `commands::credentials::export_credentials_bundle` IPC produces
a JCS-canonical JSON bundle with credentials + key registry +
status lists. `verify_bundle_offline_impl` re-loads the bundle
into a fresh ephemeral DB and runs the full §13.2 verification
pipeline — proving the bundle is self-contained and survives
Alexandria shutdown. PR 12.

### P2P propagation

Four new gossip topics carry VC-layer messages:
- `/alexandria/vc-did/1.0` → DID doc + rotation announcements
- `/alexandria/vc-status/1.0` → status list snapshots / deltas
- `/alexandria/vc-presentation/1.0` → opt-in selective presentations
- `/alexandria/pinboard/1.0` → PinBoard pinning commitments

Plus a request-response protocol on `/alexandria/vc-fetch/1.0` for
authority-respecting credential pull. Handlers in `p2p::vc_did`,
`p2p::vc_status`, `p2p::vc_fetch`, `p2p::presentation`,
`p2p::pinboard`. PR 9 + PR 10. Dispatched from the gossip event
loop in `commands/p2p.rs` alongside `catalog`, `evidence`,
`taxonomy`, `governance`, `opinions`, and classroom topics (PR 144).

### Worked example (§26)

The aggregation engine reproduces the spec's worked example
end-to-end: `Q ≈ 0.846`, `C ≈ 0.514`, `L = 5`, `T ≈ 0.435`. See
`tests/e2e_vc/aggregation.rs` for the four assertions that pin
this on every test run.

### Implementation status

| Spec section | PR | Status |
|--------------|----|----|
| §4–§5 (DID identity, key rotation) | PR 3 | Implemented |
| §6–§7 (credential taxonomy + canonical structure) | PR 4 | Implemented |
| §8–§10 (required fields, issuance, non-transferability) | PR 4–5 | Implemented |
| §11 (expiration, revocation, suspension, supersession) | PR 5 | Implemented |
| §12 (storage, durability, integrity anchoring) | PR 8 + PR 10 + PR 145 | Storage + anchor queue + PinBoard implemented. Anchor tick runs every 60s with wallet derived from the unlocked keystore; metadata-only txs submit to Cardano preprod when a Blockfrost project id is configured (Settings → Cardano, or `BLOCKFROST_PROJECT_ID` env var). Mainnet reference-script deployment still pending. |
| §13 (verification algorithm + acceptance predicate) | PR 4–5 | Implemented |
| §14 (trust aggregation, weights, confidence, levels) | PR 6 | Implemented |
| §15 (anti-gaming controls) | PR 7 | Cluster cap + inflation penalty implemented; cluster_issuers is per-DID until governance signals land |
| §16 (derived skill state output) | PR 6 + PR 13 | Implemented + cached |
| §17 (recruiter/consumer queries) | PR 13 | Cached lookups via `get_derived_skill_state` |
| §18 (selective disclosure) | PR 11 | Redact-and-resign MVP; BBS+/zk follow-up |
| §19 (NFT wrapper rules) | — | Legacy skill-proof NFT pipeline deleted (retired in migration 040); no VC-NFT wrapper yet |
| §20 (survivability) | PR 12 | Bundle export + offline verifier |
| §21–§22 (interfaces + pseudocode) | PR 4–11 | Implemented |
| §23 (security requirements) | PR 4–11 | Canonicalization, replay resistance, audit logging |
| §24 (minimal implementation profile) | PR 12 | Complete |
