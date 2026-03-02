# Alexandria P2P Protocol Specification v1

> Formal specification of all P2P message types, wire formats, gossip topics,
> validation rules, and state machine transitions for the Alexandria
> decentralized learning platform.

**Protocol version**: 1.0
**Last updated**: 2026-02-21
**Status**: Implementation-complete (Phase 4)

---

## Table of Contents

1. [Protocol Overview](#1-protocol-overview)
2. [Cryptographic Primitives](#2-cryptographic-primitives)
3. [Transport & Network Layer](#3-transport--network-layer)
4. [GossipSub Configuration](#4-gossipsub-configuration)
5. [Message Envelope](#5-message-envelope)
6. [Validation Pipeline](#6-validation-pipeline)
7. [Topic: Catalog](#7-topic-catalog)
8. [Topic: Evidence](#8-topic-evidence)
9. [Topic: Taxonomy](#9-topic-taxonomy)
10. [Topic: Governance](#10-topic-governance)
11. [Topic: Profiles](#11-topic-profiles)
12. [Cross-Device Sync Protocol](#12-cross-device-sync-protocol)
13. [Peer Scoring](#13-peer-scoring)
14. [NAT Traversal](#14-nat-traversal)
15. [Application Events](#15-application-events)
16. [Security Considerations](#16-security-considerations)

---

## 1. Protocol Overview

Alexandria uses libp2p 0.56 to build a fully decentralized P2P network
where every user runs a local node. The protocol has 8 logical layers:

```
Layer 8  Application     Tauri IPC commands, frontend events
Layer 7  P2P Events      PeerConnected, GossipMessage, NatChanged
Layer 6  Sync            Cross-device sync (encrypted, LWW/append-only)
Layer 5  Domain          Catalog, Evidence, Taxonomy, Governance, Profiles
Layer 4  Validation      Signature, Freshness, Dedup, Schema, Authority
Layer 3  GossipSub       6 topics, peer scoring, signed envelopes
Layer 2  Transport       QUIC, TCP, Kademlia, AutoNAT, Relay, DCUtR
Layer 1  Crypto          Ed25519, Blake2b-256, SHA-256
```

All messages are JSON-encoded, wrapped in a signed envelope, and
published via GossipSub v1.1. Every sender is identified by their
Cardano stake address and Ed25519 public key — creating a
cryptographic link between P2P identity and on-chain identity.

---

## 2. Cryptographic Primitives

### 2.1 Hash Functions

| Function | Algorithm | Output | Usage |
|----------|-----------|--------|-------|
| `blake2b_256(data)` | Blake2b, 32-byte digest | `[u8; 32]` | Message dedup, entity IDs, sync keys, GossipSub message IDs |
| `sha256(data)` | SHA-256 | `[u8; 32]` | General-purpose hashing |
| `entity_id(parts)` | `hex(blake2b_256(parts.join("\|")))` | 64-char hex string | Deterministic IDs for courses, evidence, votes |

**Entity ID construction**: Parts are concatenated with `|` separator,
hashed with Blake2b-256, hex-encoded.

Example: `entity_id(["stake1u8abc", "cid123"])` = `hex(blake2b("stake1u8abc|cid123"))`

### 2.2 Digital Signatures

**Algorithm**: Ed25519 (RFC 8032) via `ed25519-dalek`

**Signing**: `sign(payload_bytes, signing_key) -> SignedMessage`
- Signs raw payload bytes with Ed25519
- Returns: `{ payload, signature: [u8; 64], public_key: [u8; 32] }`

**Verification**: `verify(signed_message) -> Result`
- Reconstructs `VerifyingKey` from 32-byte `public_key`
- Reconstructs `Signature` from 64-byte `signature`
- Verifies Ed25519 signature over `payload`

### 2.3 Identity Derivation

The libp2p PeerId is derived from the Cardano payment signing key:

```
cardano_payment_key: [u8; 32]  (Ed25519 scalar from BIP32 derivation)
        |
        v
libp2p_keypair = Keypair::ed25519_from_bytes(payment_key)
        |
        v
peer_id = keypair.public().to_peer_id()
        |
        v
PeerId: base58, prefix "12D3KooW" (Ed25519 multicodec)
```

This creates a 1:1 cryptographic link: PeerId = f(Cardano signing key).

---

## 3. Transport & Network Layer

### 3.1 Transports

| Transport | Address | Platform | Purpose |
|-----------|---------|----------|---------|
| QUIC v1 | `/ip4/0.0.0.0/udp/0/quic-v1` | Desktop | Primary (OS-assigned port) |
| QUIC v1 (IPv6) | `/ip6/::/udp/0/quic-v1` | Desktop | Best-effort, errors ignored |
| TCP + Noise + Yamux | `/ip4/0.0.0.0/tcp/0` | Mobile (iOS) | Primary on mobile (QUIC unavailable on iOS) |
| Relay Circuit | Full relay multiaddr + `/p2p-circuit` | All | Circuit Relay v2 behind NAT |

### 3.2 Network Behaviour

Seven libp2p protocols compose `AlexandriaBehaviour`:

| Protocol | Version | Purpose |
|----------|---------|---------|
| GossipSub v1.1 | — | Topic-based pub/sub with peer scoring |
| Kademlia | `/alexandria/kad/1.0` | Private DHT — peer discovery via relay bootstrap |
| Identify | `/alexandria/id/1.0` | Peer info exchange, push listen addr updates |
| AutoNAT | — | NAT reachability detection |
| Relay Server | — | Circuit Relay v2 server (serve relay for other nodes) |
| Relay Client | — | Circuit Relay v2 client (NAT traversal via relay) |
| DCUtR | — | Upgrade relayed connections via hole punching |

### 3.3 Connection Parameters

| Parameter | Value |
|-----------|-------|
| Idle connection timeout | 60 seconds |
| Max GossipSub message size | 65,536 bytes (64 KB) |
| Command channel buffer | 256 messages |
| GossipSub heartbeat interval | 1 second |
| GossipSub validation mode | Strict |
| GossipSub message ID function | `hex(blake2b_256(msg.data))` |
| GossipSub authenticity | Signed (libp2p keypair) |

### 3.4 Bootstrap Peers

The Alexandria relay serves as the bootstrap node. Four multiaddrs are
configured (DNS TCP, DNS QUIC, IPv4 TCP, IPv4 QUIC):

```
/dns4/alexandria-relay.fly.dev/tcp/4001/p2p/<RELAY_PEER_ID>
/dns4/alexandria-relay.fly.dev/udp/4001/quic-v1/p2p/<RELAY_PEER_ID>
/ip4/168.220.86.30/tcp/4001/p2p/<RELAY_PEER_ID>
/ip4/168.220.86.30/udp/4001/quic-v1/p2p/<RELAY_PEER_ID>
```

Bootstrap nodes have no special protocol authority. They serve only as
initial contact points for Kademlia DHT bootstrapping and Circuit Relay v2.

---

## 4. GossipSub Configuration

### 4.1 Topics

| Constant | Topic Path | Description |
|----------|------------|-------------|
| `TOPIC_CATALOG` | `/alexandria/catalog/1.0` | Course announcements |
| `TOPIC_EVIDENCE` | `/alexandria/evidence/1.0` | Skill evidence broadcasts |
| `TOPIC_TAXONOMY` | `/alexandria/taxonomy/1.0` | DAO-ratified skill graph updates |
| `TOPIC_GOVERNANCE` | `/alexandria/governance/1.0` | Governance events |
| `TOPIC_PROFILES` | `/alexandria/profiles/1.0` | User profile updates |
| `TOPIC_PEER_EXCHANGE` | `/alexandria/peer-exchange/1.0` | Known peer address propagation |

All 6 topics are subscribed on node startup.

---

## 5. Message Envelope

Every message on the network is wrapped in a `SignedGossipMessage`:

```json
{
  "topic": "/alexandria/catalog/1.0",
  "payload": <bytes>,
  "signature": <bytes>,
  "public_key": <bytes>,
  "stake_address": "stake_test1u...",
  "timestamp": 1740000000
}
```

| Field | Type | Size | Description |
|-------|------|------|-------------|
| `topic` | String | variable | GossipSub topic this was published on |
| `payload` | `Vec<u8>` | variable | JSON-encoded, topic-specific data |
| `signature` | `Vec<u8>` | 64 bytes | Ed25519 signature over `payload` |
| `public_key` | `Vec<u8>` | 32 bytes | Sender's Cardano payment verification key |
| `stake_address` | String | ~60 chars | Sender's Cardano stake address (bech32) |
| `timestamp` | u64 | 8 bytes | Unix timestamp (seconds) of message creation |

**Publishing flow**:
1. Serialize domain-specific payload to JSON bytes
2. Sign payload with Cardano Ed25519 signing key
3. Capture current Unix timestamp
4. Construct `SignedGossipMessage` envelope
5. Serialize envelope to JSON
6. Publish via GossipSub

**Important**: The `public_key` is the Cardano payment verification key,
NOT the libp2p peer key. This links messages to on-chain identity.

---

## 6. Validation Pipeline

Every incoming gossip message passes through a 5-step validation
pipeline. The first failing step rejects the message.

### 6.1 Pipeline Steps

| Step | Check | Rejection Error |
|------|-------|-----------------|
| 1. Signature | Ed25519 verify(`payload`, `signature`, `public_key`) | `InvalidSignature` |
| 2. Freshness | `\|timestamp - now\| <= 300s` | `TooOld` or `FromFuture` |
| 3. Dedup | `blake2b_256(payload)` not in seen cache | `Duplicate` |
| 4. Schema | `payload` is valid JSON | `InvalidPayload` |
| 5. Authority | For taxonomy: `stake_address` non-empty | `Unauthorized` |

### 6.2 Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `FRESHNESS_WINDOW_SECS` | 300 (5 min) | Max clock skew tolerance |
| `DEDUP_CACHE_MAX` | 100,000 | Cache entries before full clear (~6.4 MB) |

### 6.3 Dedup Cache Behavior

The dedup cache stores `hex(blake2b_256(payload))` strings in a
`HashSet`. When the cache reaches 100,000 entries, the entire cache is
cleared (simple strategy — the freshness window already limits relevant
messages to +/-5 minutes).

### 6.4 Authority Check

Only the taxonomy topic has an authority check at the validation layer.
Full committee membership verification is deferred to the taxonomy
domain handler, which has database access. The validator performs a
lightweight check: taxonomy messages must have a non-empty
`stake_address`.

---

## 7. Topic: Catalog

**Topic**: `/alexandria/catalog/1.0`

### 7.1 Payload Schema

```json
{
  "course_id": "a1b2c3...",
  "title": "Algorithm Design",
  "description": "An advanced algorithms course",
  "content_cid": "abc123def456",
  "author_address": "stake_test1u...",
  "thumbnail_cid": null,
  "tags": ["algorithms", "graphs"],
  "skill_ids": ["skill_graph_traversal"],
  "version": 1,
  "published_at": 1740000000
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `course_id` | String | Yes | `entity_id([author_address, content_cid])` |
| `title` | String | Yes | Course title |
| `description` | String | No | Short description |
| `content_cid` | String | Yes | BLAKE3 hash of course document on iroh |
| `author_address` | String | Yes | Author's Cardano stake address |
| `thumbnail_cid` | String | No | BLAKE3 hash of thumbnail |
| `tags` | `[String]` | No | Discovery tags |
| `skill_ids` | `[String]` | No | Skills this course covers |
| `version` | i64 | Yes | Monotonically increasing version |
| `published_at` | i64 | Yes | Unix timestamp |

### 7.2 Handling Rules

1. Validate required fields: `course_id`, `title`, `content_cid`, `author_address`
2. Check local catalog for existing version
3. If `local_version >= received_version`: skip (monotonic enforcement)
4. UPSERT into `catalog` table (`ON CONFLICT(course_id) DO UPDATE`)
5. Record `sync_log` entry (direction = 'received')

---

## 8. Topic: Evidence

**Topic**: `/alexandria/evidence/1.0`

### 8.1 Payload Schema

```json
{
  "evidence_id": "ev_abc123",
  "learner_address": "stake_test1u...",
  "skill_id": "graph_traversal",
  "proficiency_level": "apply",
  "assessment_id": "sa_xyz",
  "score": 0.85,
  "difficulty": 0.50,
  "trust_factor": 1.0,
  "course_id": "course_abc",
  "instructor_address": "stake_test1u...",
  "created_at": 1740000000
}
```

| Field | Type | Required | Range | Description |
|-------|------|----------|-------|-------------|
| `evidence_id` | String | Yes | — | Deterministic ID |
| `learner_address` | String | Yes | — | Learner's stake address |
| `skill_id` | String | Yes | — | Assessed skill |
| `proficiency_level` | String | Yes | Bloom's 6 | Bloom's taxonomy level |
| `assessment_id` | String | Yes | — | Source assessment |
| `score` | f64 | Yes | [0.0, 1.0] | Score achieved |
| `difficulty` | f64 | Yes | [0.0, 1.0] | Assessment difficulty |
| `trust_factor` | f64 | Yes | > 0.0 | Assessment trust factor |
| `course_id` | String | No | — | Source course |
| `instructor_address` | String | No | — | For attribution |
| `created_at` | i64 | Yes | — | Unix timestamp |

### 8.2 Proficiency Levels (Bloom's Taxonomy)

Ordered ascending: `remember`, `understand`, `apply`, `analyze`,
`evaluate`, `create`.

### 8.3 Handling Rules

1. Validate required fields and score range [0.0, 1.0]
2. Check if `skill_id` exists locally (taxonomy must be synced)
3. If skill missing: record in `sync_log` only, skip storage
4. Auto-create `skill_assessments` stub if `assessment_id` not in DB
5. Insert with `INSERT OR IGNORE` (idempotent by evidence_id PK)
6. Record `sync_log` entry

**Critical**: Received evidence does NOT trigger local aggregation.
Only the learner's own node evaluates and updates skill proofs.
Peers store evidence solely for reputation computation (instructor
impact, verification).

---

## 9. Topic: Taxonomy

**Topic**: `/alexandria/taxonomy/1.0`

### 9.1 Payload Schema

```json
{
  "version": 2,
  "cid": "blake3_hex_hash_of_taxonomy_document",
  "previous_cid": "blake3_hex_of_v1",
  "ratified_by": ["stake_test1u_chair", "stake_test1u_member1"],
  "ratified_at": 1740000000,
  "changes": {
    "subject_fields": [
      {"id": "sf_cs", "name": "Computer Science", "description": "..."}
    ],
    "subjects": [
      {"id": "sub_algo", "name": "Algorithms", "subject_field_id": "sf_cs"}
    ],
    "skills": [
      {"id": "sk_graph", "name": "Graph Traversal", "subject_id": "sub_algo", "bloom_level": "apply"}
    ],
    "prerequisites": [["sk_graph", "sk_basics"]],
    "removed_prerequisites": []
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `version` | i64 | Yes | Monotonically increasing (>= 1) |
| `cid` | String | Yes | BLAKE3 hash or IPFS CID of full taxonomy document |
| `previous_cid` | String | No | Previous version CID (chain validation) |
| `ratified_by` | `[String]` | Yes | Stake addresses of ratifying committee members |
| `ratified_at` | i64 | Yes | Unix timestamp of ratification |
| `changes` | Object | Yes | Taxonomy changes (see below) |

### 9.2 Changes Schema

| Field | Type | Description |
|-------|------|-------------|
| `subject_fields` | `[{id, name, description?}]` | New/modified subject fields |
| `subjects` | `[{id, name, description?, subject_field_id}]` | New/modified subjects |
| `skills` | `[{id, name, description?, subject_id, bloom_level}]` | New/modified skills |
| `prerequisites` | `[[skill_id, prereq_id]]` | New prerequisite edges |
| `removed_prerequisites` | `[[skill_id, prereq_id]]` | Removed prerequisite edges |

### 9.3 Handling Rules

1. Validate: `cid` non-empty, `version >= 1`
2. Query `MAX(version) FROM taxonomy_versions`
3. If `update.version <= local_version`: skip (idempotent)
4. Apply changes:
   - Subject fields: `INSERT ON CONFLICT DO UPDATE`
   - Subjects: `INSERT ON CONFLICT DO UPDATE`
   - Skills: `INSERT ON CONFLICT DO UPDATE`
   - Prerequisites: `INSERT OR IGNORE`
   - Removed prerequisites: `DELETE`
5. Record in `taxonomy_versions` table
6. Record in `sync_log`

### 9.4 Authority Verification

Full authority check (domain handler): Queries `governance_dao_members`
joined with `governance_daos` to verify the sender has role
`committee` or `chair` in an active DAO.

---

## 10. Topic: Governance

**Topic**: `/alexandria/governance/1.0`

### 10.1 Payload Schema

```json
{
  "event_type": {
    "ProposalCreated": {
      "proposal_id": "prop_123",
      "title": "Add ML Skills",
      "description": "Proposal to add machine learning skills",
      "category": "taxonomy",
      "proposer": "stake_test1u..."
    }
  },
  "dao_id": "dao_cs",
  "timestamp": 1740000000
}
```

### 10.2 Event Types

**ProposalCreated**:
`proposal_id`, `title`, `description?`, `category`, `proposer`

**ProposalResolved**:
`proposal_id`, `status` (approved|rejected), `votes_for`, `votes_against`, `on_chain_tx?`

**CommitteeUpdated**:
`members: [String]` (stake addresses of new committee), `on_chain_tx?`

### 10.3 Handling Rules

- `ProposalCreated`: `INSERT OR IGNORE` into `governance_proposals` (skip if DAO not in local DB)
- `ProposalResolved`: `UPDATE` status/votes/on_chain_tx (skip if proposal not found)
- `CommitteeUpdated`: **DELETE all existing members** for the DAO, **INSERT new members** (critical: controls taxonomy authority)

### 10.4 State Machines

**Election lifecycle**:
```
nomination --> voting --> finalized
                     \--> cancelled
```

**Proposal lifecycle**:
```
draft --> published --> approved
                   \--> rejected
                   \--> cancelled
```

**Supermajority threshold**: 2/3 of votes for approval.

---

## 11. Topic: Profiles

**Topic**: `/alexandria/profiles/1.0`

Profile announcements contain the BLAKE3 hash (iroh content address)
of a signed profile document. Peers can fetch the full profile from
iroh using the CID.

The profile document is a `SignedProfileDocument` containing display
name, bio, avatar CID, skills, and stake address — signed with the
user's Ed25519 key for authenticity verification.

---

## 12. Cross-Device Sync Protocol

### 12.1 Overview

Cross-device sync is a private, encrypted protocol separate from
public gossip. It synchronizes local data between devices owned by
the same user.

**Pairing model**: Both devices import the same BIP-39 mnemonic.
Identical mnemonics produce identical signing keys, which derive
identical sync keys. No out-of-band pairing is needed.

### 12.2 Key Derivation

```
signing_key_bytes: [u8; 32]  (from BIP-39 mnemonic)
        |
        v
material = signing_key_bytes ++ "alexandria-cross-device-sync-v1"
        |
        v
sync_key = blake2b_256(material)  -> [u8; 32]
```

The sync key is deterministic: same mnemonic = same key on every device.

### 12.3 Syncable Tables

| Table | Merge Strategy | Description |
|-------|---------------|-------------|
| `enrollments` | LWW | Course enrollment status |
| `element_progress` | LWW | Learning progress per element |
| `course_notes` | LWW | User's course notes |
| `evidence_records` | Append-only | Skill evidence (never deleted) |
| `skill_proof_evidence` | Append-only | Proof-evidence links |

**Derived tables** (not synced, recomputed locally):
`skill_proofs`, `reputation_assertions`

### 12.4 Merge Strategies

**LWW (Last-Writer-Wins)**:
- Compare `updated_at` timestamps (ISO 8601 string comparison)
- Remote wins only if `remote.updated_at > local.updated_at` (strictly newer)
- Ties: local wins (conservative)
- Delete operations: executed except on append-only tables

**Append-Only Union**:
- Insert if primary key does not exist
- Never update or delete existing records
- Used for evidence records (immutable once created)

### 12.5 Sync Message Protocol

Messages exchanged between paired devices:

**Hello** (handshake):
```json
{
  "msg_type": { "Hello": { "platform": "macos", "sync_vector": [...] } },
  "device_id": "uuid",
  "device_name": "MacBook Pro",
  "timestamp": 1740000000
}
```

**RequestSync** (request rows newer than timestamps):
```json
{
  "msg_type": { "RequestSync": { "requests": [["enrollments", "2025-01-01T00:00:00Z"]] } }
}
```

**SyncData** (response with rows):
```json
{
  "msg_type": {
    "SyncData": {
      "table_name": "enrollments",
      "rows": [
        {
          "row_id": "enr_123",
          "operation": "update",
          "data": {"status": "completed", "updated_at": "2025-06-01T00:00:00Z"},
          "updated_at": "2025-06-01T00:00:00Z"
        }
      ]
    }
  }
}
```

**SyncAck** (acknowledgement):
```json
{
  "msg_type": { "SyncAck": { "merged": [["enrollments", 5], ["evidence_records", 12]] } }
}
```

### 12.6 Sync Queue

Local changes are enqueued for outbound delivery:

1. `enqueue_change(table, row_id, operation, row_data, updated_at)`
2. `get_pending_queue_items(device_id, limit)` — items not yet delivered
3. `mark_delivered(queue_ids, device_id)` — records delivery
4. `prune_delivered_queue()` — removes items delivered to ALL known devices

### 12.7 SQL Injection Prevention

Table names are validated against an allowlist before use in dynamic
SQL queries. Any table name not in `SYNCABLE_TABLES` is rejected.

---

## 13. Peer Scoring

### 13.1 Global Parameters

| Parameter | Value | Purpose |
|-----------|-------|---------|
| Topic score cap | 100.0 | Max positive topic contribution |
| IP colocation weight | -10.0 | Anti-Sybil (same IP penalty) |
| IP colocation threshold | 3.0 | Peers on same IP before penalty |
| Behaviour penalty weight | -10.0 | Protocol misbehaviour |
| Decay interval | 1 second | Score decay tick |
| Retain score | 3,600 seconds | Remember scores after disconnect |

### 13.2 Score Thresholds

| Threshold | Value | Effect |
|-----------|-------|--------|
| Gossip | -10.0 | Suppress gossip control messages |
| Publish | -50.0 | Suppress publishing to peer |
| Graylist | -80.0 | Drop all messages from peer |
| Accept PX | 5.0 | Trust for peer exchange |
| Opportunistic graft | 3.0 | Trigger grafting above median |

### 13.3 Per-Topic Parameters

| Topic | Weight | First Delivery | Invalid Penalty | Invalid Decay |
|-------|--------|----------------|-----------------|---------------|
| Catalog | 0.5 | 2.0 | -10.0 | 0.5 |
| Evidence | 0.7 | 3.0 | -15.0 | 0.5 |
| **Taxonomy** | **1.0** | **5.0** | **-50.0** | **0.3** |
| Governance | 0.8 | 3.0 | -30.0 | 0.3 |
| Profiles | 0.3 | 1.0 | -5.0 | 0.5 |
| Peer Exchange | 0.3 | 1.0 | -5.0 | 0.5 |

**Design rationale**: Taxonomy has the highest weight and strongest
invalid message penalty because unauthorized taxonomy updates are the
most dangerous attack vector (could corrupt the global skill graph).
Governance is second-most sensitive. Profiles are most permissive.

---

## 14. NAT Traversal

### 14.1 AutoNAT Configuration

| Parameter | Value | Default | Rationale |
|-----------|-------|---------|-----------|
| Retry interval | 60s | 15s | Reduce probe overhead |
| Confidence max | 2 | 3 | Faster NAT determination |
| Throttle server period | 30s | — | Rate-limit inbound probes |
| Max peer addresses | 3 | — | Limit probe targets per cycle |

### 14.2 NAT State Machine

```
Unknown ──[2 successful probes]──> Public(address)
Unknown ──[probes fail]──────────> Private
Public  ──[re-probe fails]──────> Private
Private ──[re-probe succeeds]───> Public(address)
```

### 14.3 Fallback Strategy

When behind NAT (`Private` state):
1. **Circuit Relay v2**: Connect to relay peers, obtain
   `/p2p/<relay>/p2p-circuit` addresses
2. **DCUtR**: Attempt to upgrade relayed connections to direct
   connections via UDP hole punching

---

## 15. Application Events

The swarm event loop forwards events to the application via
`mpsc::Sender<P2pEvent>`:

| Event | Trigger | Data |
|-------|---------|------|
| `PeerConnected` | Connection established | `peer_id: String` |
| `PeerDisconnected` | Connection closed | `peer_id: String` |
| `GossipMessage` | Validated gossip received | `topic, message: SignedGossipMessage` |
| `StatusChanged` | Network status change | `NetworkStatus` |
| `NatStatusChanged` | AutoNAT transition | `NatState` |
| `RelayReservation` | Relay reservation accepted | `relay_peer: String` |
| `DirectConnectionUpgraded` | DCUtR hole punch succeeded | `peer_id: String` |

---

## 16. Security Considerations

### 16.1 Message Authenticity

All messages are Ed25519-signed with the sender's Cardano payment key.
The signature covers only the `payload` bytes. The `timestamp` and
`stake_address` are not signed — they serve as metadata for freshness
and routing, but cannot be relied upon for authentication.

### 16.2 Sybil Resistance

- **IP colocation scoring**: Peers running multiple nodes on the same
  IP are penalized (-10.0 weight above 3 peers threshold)
- **Stake-backed challenges**: Evidence challenges require a minimum
  5 ADA stake (5,000,000 lovelace)
- **Reputation is evidence-derived**: Cannot be manufactured without
  actual learner outcomes

### 16.3 Taxonomy Protection

Taxonomy updates are the most sensitive message type:
- Highest topic weight (1.0) and strongest invalid penalty (-50.0)
- Authority check at validation layer (non-empty stake_address)
- Full committee membership verification at domain handler layer
- CommitteeUpdated events replace entire committee (no incremental adds)

### 16.4 Privacy

- **Gossip is public**: All messages on the 6 gossip topics are visible
  to every peer. Evidence records, catalog entries, and governance
  events are inherently public.
- **Sync is private**: Cross-device sync uses encryption derived from
  the wallet signing key. Only devices with the same mnemonic can
  decrypt sync messages.
- **Behavioral data**: Per the Sentinel anti-cheat design, raw
  behavioral data (keystrokes, mouse movements) never leaves the
  client. Only derived integrity scores travel over the network.

### 16.5 Clock Skew

The freshness window of +/-5 minutes tolerates reasonable clock skew.
Messages outside this window are silently dropped. NTP synchronization
is recommended but not enforced.

### 16.6 Dedup Cache Limitations

The dedup cache uses a simple clear-all strategy at 100,000 entries.
This means a message that was previously seen could be re-accepted
after a cache clear. The freshness window (5 minutes) limits the
practical impact — messages older than 5 minutes are rejected
regardless of dedup cache state.
