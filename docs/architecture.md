# Alexandria — Architecture

> Offline-first, trustless, multi-platform.

**Status**: Implementation-complete
**Last updated**: 2026-03-25

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
- **Trustless verification**: Credentials are anchored on Cardano.
  Anyone can verify a skill proof without contacting the platform.
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
|  |   (WebView)    | ~160    |                      |  |
|  |                | cmds    |  +----------------+  |  |
|  |  25 pages      |         |  |   SQLite DB    |  |  |
|  |  32 components |         |  |   53 tables    |  |  |
|  |  12 composables|         |  |   19 migrations|  |  |
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

All state lives on the user's machine in three locations:

| Store | File/Directory | Purpose |
|-------|----------------|---------|
| SQLite | `alexandria.db` | Relational data (courses, skills, evidence, governance) |
| Vault (desktop) | `vault.stronghold` | IOTA Stronghold encrypted wallet keys and mnemonic |
| Vault (mobile) | `vault.enc` | AES-256-GCM + Argon2id encrypted wallet keys and mnemonic |
| iroh | `iroh/` | Content-addressed blobs (course HTML, profiles) |

Default data directory: `~/Library/Application Support/org.alexandria.node/` (macOS).

---

## 3. Identity & Wallet

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

The same Ed25519 key serves as:
1. Cardano payment signing key
2. libp2p peer identity
3. GossipSub message signing key
4. Content/profile document signing key

### Vault Storage

Keys are stored in an encrypted vault. The implementation varies by platform:

**Desktop (IOTA Stronghold)**:
- Password → Argon2id (64 MB, 3 iterations, 4 lanes) with random salt → derived key
- Salt file includes HMAC-SHA256 integrity tag
- Mnemonic stored encrypted at a fixed vault path
- Vault file: `vault.stronghold` (binary, encrypted at rest)

**Mobile (Portable AES-256-GCM + Argon2id)**:
- Password → Argon2id (memory-hard KDF, 64 MB, 3 iterations) → 256-bit key
- Mnemonic encrypted with AES-256-GCM (random 96-bit nonce)
- Vault file: `vault.enc` (salt + nonce + ciphertext)

Both share the same lock/unlock cycle: lock clears in-memory keys, unlock re-derives from mnemonic.

---

## 4. Database

**Engine**: SQLite (rusqlite 0.38, bundled)

**Tables**: 53 across 19 migrations

| Domain | Tables |
|--------|--------|
| Identity | `local_identity` |
| Taxonomy | `subject_fields`, `subjects`, `skills`, `skill_prerequisites`, `skill_relations`, `taxonomy_versions` |
| Courses | `courses`, `course_chapters`, `course_elements`, `element_skill_tags` |
| Learning | `enrollments`, `element_progress`, `course_notes` |
| Evidence | `skill_assessments`, `evidence_records`, `skill_proofs`, `skill_proof_evidence` |
| Reputation | `reputation_assertions`, `reputation_evidence`, `reputation_impact_deltas`, `reputation_snapshots` |
| Integrity | `integrity_sessions`, `integrity_snapshots` |
| P2P | `peers`, `pins`, `sync_log`, `catalog` |
| Governance | `governance_daos`, `governance_proposals`, `governance_dao_members`, `governance_elections`, `governance_election_nominees`, `governance_election_votes`, `governance_proposal_votes` |
| Content | `content_mappings` |
| Sync | `devices`, `sync_state`, `sync_queue` |
| Challenges | `evidence_challenges`, `challenge_votes` |
| Attestation | `attestation_requirements`, `evidence_attestations` |
| Tutoring | `tutoring_sessions` |
| Classrooms | `classrooms`, `classroom_members`, `classroom_join_requests`, `classroom_channels`, `classroom_messages`, `classroom_calls`, `classroom_group_keys` |
| Governance (on-chain) | `onchain_governance_queue` |
| Settings | `app_settings` |

### Key Design Decisions

- **Deterministic IDs**: `hex(blake2b_256(parts.join("|")))` instead of server-generated UUIDs
- **Singleton identity**: `local_identity` has `CHECK (id = 1)` — exactly one row, the node owner
- **No server tables**: No `refresh_tokens`, `oauth_accounts`, or session management
- **Content stored externally**: Course HTML and profiles live in iroh blobs, referenced by BLAKE3 hash

See [Database Schema](database-schema.md) for the full DDL.

---

## 5. Content Storage (iroh)

**Engine**: iroh 0.96 with `fs-store` backend

iroh provides a BLAKE3 content-addressed blob store. Content is
identified by its hash, ensuring integrity and deduplication.

### Operations

| Operation | Description |
|-----------|-------------|
| `add_bytes(data)` → hash | Store content, get BLAKE3 hash |
| `get_bytes(hash)` → data | Retrieve by hash |
| `has(hash)` → bool | Check existence |

### Resolution Chain

When resolving content by CID or hash:

1. **Local iroh store** — instant, offline
2. **CID↔BLAKE3 mapping table** — bidirectional lookup in SQLite
3. **IPFS gateway fallback** — Blockfrost → ipfs.io → dweb.link (HTTP, with caching)

### Content Types

- **Course documents**: Signed JSON with chapters, elements, content hashes
- **User profiles**: Signed JSON with display name, bio, avatar CID, skills

Both use Ed25519 signatures for authenticity verification.

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
| Evidence | `/alexandria/evidence/1.0` | Skill evidence broadcasts |
| Taxonomy | `/alexandria/taxonomy/1.0` | DAO-ratified skill graph updates |
| Governance | `/alexandria/governance/1.0` | Proposals, elections, committee updates |
| Profiles | `/alexandria/profiles/1.0` | User profile announcements |
| Peer Exchange | `/alexandria/peer-exchange/1.0` | Known peer address propagation |

### Message Flow

1. Serialize domain payload to JSON
2. Sign with Cardano Ed25519 key
3. Wrap in `SignedGossipMessage` envelope (payload + signature + public_key + stake_address + timestamp)
4. Publish to GossipSub topic

### Validation Pipeline (6 steps)

1. **Signature** — Ed25519 verify (covers all envelope fields: topic, timestamp, stake_address, payload)
2. **Identity Binding** — TOFU: binds stake_address to public_key on first encounter, rejects mismatches
3. **Freshness** — within ±5 minutes
4. **Dedup** — Blake2b-256 hash in LRU cache (100K entries, least-recently-used eviction)
5. **Schema** — valid JSON
6. **Authority** — taxonomy messages require committee membership

### Rate Limiting

Per-peer token-bucket rate limiter (20 messages per 60 seconds, 1 refill per 3 seconds) applied before the validation pipeline. Peer state is cleaned up on disconnect.

### Dynamic Topics

In addition to the 6 global topics, classrooms use per-classroom dynamic topics:
- Message topic: `/alexandria/classroom/{id}/1.0`
- Meta topic: `/alexandria/classroom/{id}/meta/1.0`

Nodes subscribe/unsubscribe as they join/leave classrooms.

### Cross-Device Sync

Private encrypted sync between devices sharing the same mnemonic:

- Sync key: `blake2b_256(signing_key_bytes ++ "alexandria-cross-device-sync-v1")`
- LWW (Last-Writer-Wins) merge for enrollments, progress, notes
- Append-only merge for evidence records
- SQL injection prevention via table name allowlist

See [P2P Protocol Specification](protocol-spec-v1.md) for full wire formats.

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
| NFT minting | NativeScript signature policy + CIP-25 metadata |
| Course registration | Mint token with course metadata |
| Reputation snapshots | CIP-68 soulbound tokens with Plutus inline datums |
| Governance metadata | DAO registration, elections, proposals, vote receipts |
| Coin selection | Greedy UTxO selection with min-ADA enforcement |

### Transaction Types

1. **SkillProof NFT** — Mints a token with CIP-25 metadata containing skill, proficiency level, confidence score, and evidence count
2. **Course Registration** — Mints a token recording course enrollment on-chain
3. **Reputation Snapshot** — CIP-68 soulbound token with CBOR-encoded datum (reference + user token pair)
4. **Governance Actions** — Metadata-bearing transactions for DAO ops, elections, proposals, votes

---

## 8. Evidence Pipeline

### Flow

```
Assessment completion
    |
    v
Evidence record created (score, difficulty, trust_factor, bloom level)
    |
    v
Aggregation: weighted confidence per (skill, proficiency_level)
    |
    v
Skill proof updated (if threshold met)
    |
    v
Reputation impact computed (instructor attribution)
    |
    +---> (optional) Broadcast via P2P evidence topic
    +---> (optional) Mint SkillProof NFT on Cardano
```

### Components

| Module | Responsibility |
|--------|---------------|
| `evidence/aggregator` | Weighted evidence → skill proof confidence |
| `evidence/attestation` | Multi-party attestation requirements and verification |
| `evidence/challenge` | Stake-based evidence challenges with voting and resolution |
| `evidence/reputation` | Instructor impact computation, distribution-based scoring |
| `evidence/taxonomy` | Bloom's level thresholds and skill graph traversal |
| `evidence/thresholds` | Configurable proof thresholds per proficiency level |

### Challenge Mechanism

- Any peer can challenge evidence by staking 5 ADA (5,000,000 lovelace)
- Challenge enters voting period
- 2/3 supermajority required to uphold
- Upheld: evidence deleted, challenger's stake returned
- Rejected: challenger loses stake

---

## 9. Governance

### Structure

- One DAO per subject field or subject
- DAOs have committees (chair + members)
- Committees gate taxonomy updates

### Features

| Feature | Status |
|---------|--------|
| DAO creation | Implemented |
| Committee management | Implemented |
| Proposal lifecycle (draft → published → approved/rejected) | Implemented |
| Election lifecycle (nomination → voting → finalized) | Implemented |
| 2/3 supermajority voting | Implemented |
| P2P gossip for governance events | Implemented |
| On-chain metadata transactions | Implemented |
| Aiken/Plutus smart contract enforcement | **Not implemented** |

---

## 10. Frontend

**Stack**: Vue 3 + TypeScript + Vite + Tailwind CSS v4

### Pages (26 routes)

| Page | Route | Description |
|------|-------|-------------|
| Onboarding | `/onboarding` | Wallet creation, mnemonic backup, import |
| Unlock | `/unlock` | Password entry, vault unlock |
| Home | `/home` | Dashboard overview |
| Courses Index | `/courses` | Browse course catalog |
| Course Detail | `/courses/:id` | Course info, chapters, enrollment |
| Course Player | `/learn/:id` | Content player (text, video, quiz) |
| Course New | `/instructor/courses/new` | Create a new course |
| Course Edit | `/instructor/courses/:id` | Edit existing course |
| Skills Index | `/skills` | Browse skill taxonomy |
| Skill Detail | `/skills/:id` | Skill info, prerequisites, proofs |
| Governance Index | `/governance` | Browse DAOs |
| DAO Detail | `/governance/:id` | DAO info, proposals, elections |
| Classrooms Index | `/classrooms` | List joined classrooms |
| Classroom Detail | `/classrooms/:id` | Channels, messages, active calls |
| Classroom Settings | `/classrooms/:id/settings` | Role management, archive |
| Join Requests | `/classrooms/:id/requests` | Review pending join requests |
| Tutoring Index | `/tutoring` | Live tutoring sessions list |
| Tutoring Session | `/tutoring/:id` | Active video/audio/screen session |
| My Courses | `/dashboard/courses` | Enrolled courses, progress |
| Credentials | `/dashboard/credentials` | Minted NFT credentials |
| Reputation | `/dashboard/reputation` | Reputation assertions, impact |
| Network | `/dashboard/network` | P2P status, connected peers |
| Sync | `/dashboard/sync` | Cross-device sync status |
| Sentinel | `/dashboard/sentinel` | Integrity training, sessions |
| Settings | `/dashboard/settings` | Theme, profile, app config |

### Design System

CSS custom properties with light/dark mode via `.dark` class on `<html>`:

- Custom component classes: `.btn`, `.card`, `.card-interactive`, `.input`, `.badge`, `.prose`
- Color system: space-separated RGB triplets (e.g., `--color-primary: 79 70 229`)
- Tailwind v4 `@custom-variant dark (&:is(.dark *))` for class-based dark mode
- FOUC prevention: inline `<script>` in `index.html` applies theme before CSS loads

---

## 11. IPC Boundary

The frontend communicates with the Rust backend via **~160 Tauri IPC commands** across 22 modules:

| Module | Commands | Examples |
|--------|----------|---------|
| classroom | 26 | `classroom_create`, `classroom_send_message`, `classroom_start_call` |
| governance | 17 | `create_dao`, `submit_proposal`, `cast_vote`, `run_election` |
| taxonomy | 15 | `get_skills`, `get_subjects`, `update_taxonomy`, `get_skill_graph` |
| tutoring | 14 | `tutoring_create_room`, `tutoring_join_room`, `tutoring_toggle_video` |
| identity | 11 | `generate_wallet`, `unlock_vault`, `lock_vault`, `get_profile` |
| attestation | 8 | `create_attestation_requirement`, `submit_attestation` |
| sync | 8 | `register_device`, `trigger_sync`, `get_sync_status` |
| challenge | 7 | `submit_challenge`, `cast_challenge_vote`, `resolve_challenge` |
| courses | 7 | `create_course`, `get_course`, `list_courses` |
| content | 6 | `store_content`, `get_content`, `resolve_cid` |
| integrity | 6 | `start_session`, `submit_snapshot`, `get_session_score` |
| catalog | 4 | `search_catalog`, `get_catalog_entry`, `bootstrap_public_catalog` |
| enrollment | 4 | `enroll`, `update_progress`, `get_enrollment` |
| p2p | 4 | `p2p_start`, `p2p_stop`, `p2p_status`, `p2p_peers` |
| reputation | 4 | `get_reputation`, `compute_impact`, `get_assertions` |
| snapshot | 4 | `build_snapshot_tx`, `submit_snapshot_tx` |
| chapters | 4 | `get_chapters`, `create_chapter`, `update_chapter` |
| elements | 4 | `get_elements`, `create_element`, `update_element` |
| evidence | 3 | `submit_evidence`, `get_evidence`, `broadcast_evidence` |
| cardano | 2 | `get_utxos`, `submit_transaction` |
| health | 2 | `health_check`, `read_diag_log` |

Note: tutoring has platform-specific variants (desktop with video, mobile stubs). The ~160 count reflects unique commands registered per platform.

---

## 12. Security Model

### Threat Mitigations

| Threat | Mitigation |
|--------|-----------|
| Key theft | Encrypted vault — Stronghold (desktop) or AES-256-GCM + Argon2id (mobile) |
| Message forgery | Ed25519 signatures on all gossip messages |
| Sybil attacks | IP colocation scoring, stake-based challenges |
| Taxonomy corruption | Committee authority verification, strongest peer scoring penalty |
| Evidence inflation | Multi-party attestation, stake-based challenges, behavioral integrity |
| Replay attacks | ±5 minute freshness window, Blake2b-256 dedup cache |
| Content tampering | BLAKE3 content addressing (iroh), Ed25519 signed documents |

### Privacy Guarantees

- Raw biometric data (keystrokes, mouse movements, face embeddings) **never leaves the device**
- Only derived integrity scores (0.0-1.0) are stored and transmitted
- Cross-device sync is encrypted with a key derived from the wallet signing key
- Public gossip contains only evidence scores and governance actions — no personal data beyond stake addresses

