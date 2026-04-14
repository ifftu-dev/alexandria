# Alexandria Protocol Specification

**Status:** Draft v0.1.0
**Category:** Standards Track
**Created:** 2025
**Updated:** 2026-04-14
**Author:** Pratyush Pundir

---

## Implementation Status

Two credential pipelines coexist in this specification:

- **Legacy path (§4 + §5)** — assessment → skill_proof → optional
  Cardano NFT mint. Still implemented and supported. See
  `evidence::aggregator`, `commands::cardano::mint_skill_proof_nft`.
- **VC-first path (§14)** — `did:key` identity + signed VCs +
  status-list revocation + deterministic aggregation +
  selective-disclosure presentations + offline survivability bundle.

The §14 Verifiable Credentials Protocol is normative and defines the
full credential lifecycle. The §4 reputation system remains active
for backward compatibility with previously-issued skill proofs.

For each section in this document, see the implementation-status
table at `docs/architecture.md` §13 to determine which PR landed
the corresponding feature.

---

## Conventions and Terminology

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in RFC 2119.

The following terms are used throughout this specification:

- **Subject Field** — A top-level domain of knowledge (e.g. "Computer Science", "Mathematics").
- **Subject** (taxonomy) — A discrete area of study contained within a Subject Field (e.g. "Distributed Systems", "Linear Algebra"). Used in §4, §5, §8, §10.
- **Subject** (credential) — In the §14 Verifiable Credentials Protocol, the entity about whom a claim is made, identified by a DID (see §14.4.1). Matches the W3C `credentialSubject` role; disambiguated from the taxonomy sense by surrounding context.
- **Skill** — An atomic, assessable unit of competence within a Subject. Skills form a directed acyclic graph (DAG) with explicit prerequisite edges.
- **SkillProof** — A verifiable, learner-owned credential attesting to demonstrated proficiency in a specific Skill at a specific Bloom's taxonomy level. Legacy artefact of §4; the §14 VC-first path replaces it with a signed `VerifiableCredential` (§14.7).
- **VerifiableCredential** — A W3C-style signed credential issued under §14. Carries a claim about a subject, bound to an issuer DID, verifiable without Alexandria infrastructure.
- **Issuer** — In §14, the authority that signs and originates a credential. Exactly one primary issuer per credential (§14.4.2).
- **ReputationAssertion** — A computed, evidence-derived claim about the instructional, assessment, or authorship impact of an actor within a given scope. Legacy artefact of §4; the §14 aggregation pipeline (§14.14) produces a Derived Skill State instead.
- **DerivedSkillState** — The explainable output of the §14 trust aggregation pipeline: (rawScore Q, confidence C, trustScore T, level L, evidenceMass M, uniqueIssuerClusters U) for a (subject, skill) pair.
- **DAO** — Decentralised Autonomous Organisation; a governance body responsible for decision-making within a defined scope.
- **ProficiencyLevel** — An ordered enumeration of learner capability within a Skill, based on Bloom's Taxonomy: `remember`, `understand`, `apply`, `analyze`, `evaluate`, `create`.
- **Node** — A running instance of the Alexandria application (desktop or mobile), containing a complete database, content store, P2P networking stack, and wallet.
- **Sentinel** — The client-side assessment integrity system that monitors behavioral signals during assessments.

For motivation, design rationale, and stakeholder context, see the companion document: *Alexandria: Free Knowledge, Verified Skills, No Gatekeepers*.

---

## 1. Scope

This specification defines the data models, verifiable credentials, reputation mechanics, query interfaces, governance structures, decentralisation requirements, peer-to-peer protocol, evidence pipeline, assessment integrity, and threat mitigations for the Alexandria protocol. It is intended as a normative reference for implementors, contributors, and governance participants.

---

## 2. Architecture

### 2.1 Overview

Alexandria is a **Tauri v2 application** — a single binary that bundles a Rust backend with a Vue 3 frontend. Every user runs a full node. There are no servers, no Docker containers, and no external databases.

All state lives on the user's device in three locations:

| Store | Purpose |
|-------|---------|
| SQLite | Relational data (courses, skills, evidence, governance, verifiable credentials) — 66 tables, 30 migrations |
| Encrypted vault | Wallet keys and mnemonic — IOTA Stronghold (desktop) or AES-256-GCM + Argon2id (mobile) |
| iroh | Content-addressed blobs (course HTML, profiles) — BLAKE3 hashes |

### 2.2 Design Principles

The architecture MUST satisfy:

- **Offline-first**: Every operation MUST work without network access. Sync is opportunistic.
- **Self-sovereign identity**: The user's 24-word BIP-39 mnemonic IS their account. No email, no password recovery service, no OAuth provider.
- **Trustless verification**: Credentials anchored on Cardano MUST be independently verifiable without contacting the platform.
- **Privacy by architecture**: Raw behavioral data (Sentinel) MUST NOT leave the device. Only derived scores MAY cross the network.

### 2.3 Technology Stack

| Layer | Technology |
|-------|------------|
| Shell | Tauri 2.10, WebKit (macOS/iOS) / WebView2 (Windows) / Android WebView |
| Backend | Rust (2021 edition), tokio async runtime |
| Frontend | Vue 3, TypeScript, Vite, Tailwind CSS v4 |
| Database | SQLite (rusqlite, bundled) |
| Content storage | iroh 0.96 (BLAKE3 content-addressed blobs) |
| P2P networking | libp2p 0.56 (TCP, QUIC, GossipSub, Kademlia, Relay, DCUtR) |
| Wallet (desktop) | BIP-39 + CIP-1852 (pallas), IOTA Stronghold vault |
| Wallet (mobile) | BIP-39 + CIP-1852 (pallas), AES-256-GCM + Argon2id vault |
| Cardano | pallas 0.35 (Conway tx builder), Blockfrost preprod |

### 2.4 IPC Boundary

The frontend communicates with the Rust backend via 194 Tauri IPC commands registered in `tauri::generate_handler!`. Commands are split across 26 IPC modules (classroom, governance, taxonomy, tutoring, identity, credentials, sync, courses, attestation, challenge, opinions, integrity, content, pinning, storage, snapshot, reputation, enrollment, elements, chapters, catalog, p2p, evidence, aggregation, presentation, health, cardano), with `commands/` totalling 30 source files (excluding `mod.rs`; `tutoring_mobile.rs` / `tutoring_stubs.rs` are platform-conditional variants of `tutoring`, and `ratelimit.rs` is an internal helper).

---

## 3. Identity & Wallet

### 3.1 Key Derivation

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

The same Ed25519 key MUST serve as: (1) Cardano payment signing key, (2) libp2p peer identity, (3) GossipSub message signing key, and (4) content/profile document signing key.

### 3.2 Vault Storage

Keys MUST be stored in an encrypted vault. The implementation varies by platform:

**Desktop (IOTA Stronghold)**: Password → Argon2id (64 MB, 3 iterations, 4 lanes) with random salt → derived key. Salt file includes HMAC-SHA256 integrity tag. Mnemonic stored encrypted at a fixed vault path.

**Mobile (Portable AES-256-GCM + Argon2id)**: Password → Argon2id (64 MB, 3 iterations) → 256-bit key. Mnemonic encrypted with AES-256-GCM (random 96-bit nonce). Vault file contains salt + nonce + ciphertext.

Both platforms MUST enforce a minimum 12-character password. The Wallet struct MUST implement `Drop` with zeroization; `Clone` MUST NOT be derived.

### 3.3 Deterministic Entity IDs

All entity IDs MUST be computed as `hex(blake2b_256(parts.join("|")))` instead of server-generated UUIDs. This ensures deterministic, collision-resistant identifiers derived from entity properties.

### 3.4 DID Identity for the VC Layer

For the DID identity model, key-rotation requirements, and historical-key resolution used by the verifiable credentials layer (§14), see §14.5. The wallet-level key derivation defined in §3.1 supplies the underlying Ed25519 signing material; the VC layer binds a `did:key` identifier to that same key by multicodec-wrapping the Ed25519 public key (varint `0xed01`) and encoding it as multibase base58btc.

---

## 4. Reputation System

*§4 describes the legacy evidence-derived reputation system (assessment → skill_proof). The §14 Verifiable Credentials Protocol defines an independent aggregation pipeline (§14.14) that operates over signed VCs rather than `skill_proof` rows. Both pipelines coexist; consumers MAY query either.*

### 4.1 Purpose

The Reputation System defines a verifiable, skill-scoped, evidence-derived mechanism for evaluating the instructional, assessment, or authorship impact of actors within the system. Reputation is not a credential, not a global score, and not platform-owned. It is a computed view derived exclusively from learner-owned SkillProofs and associated evidence.

### 4.2 Design Principles

The Reputation System MUST:

- Be derived solely from verifiable SkillProofs.
- Be scoped to a specific Skill and ProficiencyLevel.
- Be role-specific (e.g. instructor vs assessor).
- Be reproducible from disclosed evidence.
- Avoid global or identity-wide scalar scores.

The Reputation System MUST NOT:

- Assign reputation without evidence.
- Aggregate across unrelated skills.
- Override learner ownership controls.
- Depend on platform-specific trust assumptions.

### 4.3 Reputation Scope Model

Reputation MUST always be defined over the following tuple:

```
(subject, role, skill, proficiency_level)
```

Implementations MUST NOT produce or consume reputation values that omit any element of this tuple.

### 4.4 ReputationAssertion

A conforming ReputationAssertion MUST contain the following fields:

```
ReputationAssertion {
    subject_address: string       // The actor being evaluated (Cardano stake address)
    role: instructor | assessor | author
    subject_id: string            // Subject scope
    skill_id: string              // Skill scope
    proficiency_level: ProficiencyLevel
    confidence: number            // Statistical confidence
    evidence_count: number        // Supporting evidence count
}
```

Stored in `reputation_assertions` with supporting evidence in `reputation_evidence`. Impact deltas stored in `reputation_impact_deltas` with FK to individual evidence records.

Implementations MUST reject any ReputationAssertion where supporting evidence is empty or contains unverifiable references.

### 4.5 Instructor Impact Computation

Implementations MUST compute instructor impact using the following model:

```
Impact(I, S, P) = Σ over learners L [
    ΔConfidence(L, S, P) × Attribution(I, L, S)
]
```

Where:

- `ΔConfidence` = change in skill proof confidence attributable to instruction
- `Attribution` = InstructionWeight / TotalInstructionWeight

Implementations MAY substitute alternative impact functions provided they satisfy the design principles defined in Section 4.2.

### 4.6 Evidence Weighting

Evidence weight MUST be computed as:

```
EvidenceWeight = assessment.weight × difficulty × trust_factor
```

The `trust_factor` is derived from the Sentinel assessment integrity system. Confirmed violations lower `trust_factor` by 0.20 per violation (floor: 0.10). Implementations MUST NOT assign non-zero weight to evidence that lacks a verified assessment source.

### 4.7 Reputation Distribution Model

Reputation MUST be exposed as a distribution, not as a single scalar. A conforming view MUST include: median impact, 25th/75th percentile impact, learner count, and confidence interval.

### 4.8 On-Chain Snapshots

Reputation snapshots MAY be anchored on Cardano as CIP-68 soulbound tokens with CBOR-encoded inline datums (reference + user token pair).

### 4.9 Learner Ownership

Only learner-disclosed SkillProofs MAY be used in reputation computation. Platforms MUST compute reputation on behalf of actors but MUST NOT claim ownership of the resulting assertions.

---

## 5. Evidence Pipeline

*Evidence in §5 is captured as `evidence_records` rows feeding the legacy `skill_proof` aggregator. The verifiable-credentials path (§14) treats evidence differently: each issuance event produces a signed VC (§14.7) whose lifecycle is governed by §14.11 and whose contribution to subject trust is computed by §14.14.*

### 5.1 Flow

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

### 5.2 Evidence Records

Evidence records bind assessments to verifiable outcomes:

```
EvidenceRecord {
    learner_address: string     // Cardano stake address
    skill_id: string
    assessment_id: string
    score: number
    proficiency_level: ProficiencyLevel
    difficulty: number
    trust_factor: number        // Derived from Sentinel integrity scoring
    instructor_address: string
    course_id: string
}
```

IDs are deterministic: `hex(blake2b_256(parts.join("|")))`. Evidence is broadcast over the `/alexandria/evidence/1.0` GossipSub topic with Ed25519 signatures for authenticity.

### 5.3 Aggregation

The aggregator computes a weighted confidence score:

```
confidence = Σ(score_pct × assessment_weight × assessment_difficulty)
           / Σ(assessment_weight × assessment_difficulty)
```

### 5.4 Proficiency Thresholds

The aggregator evaluates Bloom's taxonomy proficiency thresholds from lowest to highest, awarding the highest level for which the learner meets both the minimum evidence count and minimum confidence:

| Level | Min Evidence | Min Confidence | Special Requirement |
|-------|-------------|---------------|---------------------|
| Remember | 1 | 0.60 | — |
| Understand | 2 | 0.65 | — |
| Apply | 2 | 0.70 | — |
| Analyze | 3 | 0.75 | — |
| Evaluate | 3 | 0.80 | — |
| Create | 1 | 0.80 | Requires project-type evidence |

### 5.5 Attestation

Multi-party attestation requirements MAY be defined for high-stakes assessments. The attestation system supports configurable requirements with multiple required attestors.

### 5.6 Challenge Mechanism

Any peer MAY challenge evidence by staking 5 ADA (5,000,000 lovelace). The challenge enters a voting period. A 2/3 supermajority is required to uphold. If upheld, the evidence is deleted and the challenger's stake is returned. If rejected, the challenger loses their stake.

---

## 6. Peer-to-Peer Protocol

### 6.1 Overview

Alexandria uses libp2p 0.56 to build a fully decentralized P2P network where every user runs a local node. The protocol has 8 logical layers:

| Layer | Name | Description |
|:-----:|------|-------------|
| 8 | Application | Tauri IPC commands, frontend events |
| 7 | P2P Events | PeerConnected, GossipMessage, NatChanged |
| 6 | Sync | Cross-device sync (encrypted, LWW/append-only) |
| 5 | Domain | Catalog, Evidence, Taxonomy, Governance, Profiles, Classrooms |
| 4 | Validation | Signature, Identity, Freshness, Dedup, Schema, Authority |
| 3 | GossipSub | 6 global + per-classroom topics, peer scoring, rate limiting |
| 2 | Transport | QUIC, TCP, Kademlia, AutoNAT, Relay, DCUtR |
| 1 | Crypto | Ed25519, Blake2b-256, SHA-256 |

All messages are JSON-encoded, wrapped in a signed envelope, and published via GossipSub v1.1. Every sender is identified by their Cardano stake address and Ed25519 public key — creating a cryptographic link between P2P identity and on-chain identity.

### 6.2 Cryptographic Primitives

| Function | Algorithm | Usage |
|----------|-----------|-------|
| `blake2b_256(data)` | Blake2b, 32-byte digest | Message dedup, entity IDs, sync keys, GossipSub message IDs |
| `sha256(data)` | SHA-256 | Signature pre-hashing |
| `entity_id(parts)` | `hex(blake2b_256(parts.join("\|")))` | Deterministic IDs for courses, evidence, votes |
| Ed25519 | RFC 8032 via ed25519-dalek | All message signing |

### 6.3 Transports

| Transport | Address | Platform |
|-----------|---------|----------|
| QUIC v1 | `/ip4/0.0.0.0/udp/0/quic-v1` | Desktop |
| QUIC v1 (IPv6) | `/ip6/::/udp/0/quic-v1` | Desktop (best-effort) |
| TCP + Noise + Yamux | `/ip4/0.0.0.0/tcp/0` | Mobile (iOS — QUIC unavailable) |
| Relay Circuit | Full relay multiaddr + `/p2p-circuit` | All (NAT traversal) |

### 6.4 Network Behaviour

Seven libp2p protocols compose `AlexandriaBehaviour`:

| Protocol | Version | Purpose |
|----------|---------|---------|
| GossipSub v1.1 | — | Topic-based pub/sub with peer scoring |
| Kademlia | `/alexandria/kad/1.0` | Private DHT — peer discovery via relay bootstrap |
| Identify | `/alexandria/id/1.0` | Peer info exchange, push listen addr updates |
| AutoNAT | — | NAT reachability detection |
| Relay Server | — | Circuit Relay v2 server |
| Relay Client | — | Circuit Relay v2 client (NAT traversal) |
| DCUtR | — | Upgrade relayed connections via hole punching |

### 6.5 GossipSub Topics

| Topic Path | Description |
|------------|-------------|
| `/alexandria/catalog/1.0` | Course announcements |
| `/alexandria/evidence/1.0` | Skill evidence broadcasts |
| `/alexandria/taxonomy/1.0` | DAO-ratified skill graph updates |
| `/alexandria/governance/1.0` | Governance events |
| `/alexandria/profiles/1.0` | User profile updates |
| `/alexandria/opinions/1.0` | Subjective ratings on courses, evidence, peers (Field Commentary, mig 21) |
| `/alexandria/peer-exchange/1.0` | Known peer address propagation |
| `/alexandria/vc-did/1.0` | DID document + key-rotation announcements (§14.5) |
| `/alexandria/vc-status/1.0` | RevocationList2020 status-list snapshots and deltas (§14.11.2) |
| `/alexandria/vc-presentation/1.0` | Opt-in selective-disclosure presentation envelopes (§14.18) |
| `/alexandria/pinboard/1.0` | PinBoard pinning-commitment observations (§14.12, §14.20.4) |

All 11 topics MUST be subscribed on node startup. In addition, a
request-response protocol on `/alexandria/vc-fetch/1.0`
(libp2p `request-response` + CBOR codec) handles authority-respecting
credential pull; this is a 1-to-1 protocol, not a gossip topic, and
is enabled when the node has a `Database` wired into its swarm event
loop.

See §14 for the normative VC payload schemas carried by the
`vc-did`, `vc-status`, `vc-presentation`, and `pinboard` topics, and
by the `vc-fetch/1.0` request-response protocol.

### 6.6 Message Envelope

Every message on the network MUST be wrapped in a `SignedGossipMessage`:

```json
{
    "topic": "/alexandria/catalog/1.0",
    "payload": "<bytes>",
    "signature": "<bytes:64>",
    "public_key": "<bytes:32>",
    "stake_address": "stake_test1u...",
    "timestamp": 1740000000
}
```

| Field | Type | Size | Description |
|-------|------|------|-------------|
| `topic` | String | variable | GossipSub topic this was published on |
| `payload` | `Vec<u8>` | variable | JSON-encoded, topic-specific data |
| `signature` | `Vec<u8>` | 64 bytes | Ed25519 signature over canonical bytes |
| `public_key` | `Vec<u8>` | 32 bytes | Sender's Cardano payment verification key |
| `stake_address` | String | ~60 chars | Sender's Cardano stake address (bech32) |
| `timestamp` | u64 | 8 bytes | Unix timestamp (seconds) of message creation |

The signature MUST cover ALL envelope fields: `SHA-256(topic || timestamp_be_bytes || stake_address || payload)`. The `public_key` is the Cardano payment verification key, NOT the libp2p peer key — this links messages to on-chain identity.

### 6.7 Validation Pipeline

Every incoming gossip message MUST pass through a 6-step validation pipeline. A per-peer token-bucket rate limiter (20 msgs/60s) is applied before the pipeline.

| Step | Check | Rejection Error |
|------|-------|-----------------|
| 1. Signature | Ed25519 verify over canonical bytes | `InvalidSignature` |
| 2. Identity | Verify public key derives the claimed stake address | `InvalidIdentity` |
| 3. Freshness | Timestamp within ±5 minutes of local clock | `ExpiredMessage` |
| 4. Dedup | Blake2b-256 hash not in LRU cache (100,000 entries) | `DuplicateMessage` |
| 5. Schema | Payload deserialises to expected topic-specific type | `InvalidSchema` |
| 6. Authority | Topic-specific permission checks (e.g. committee membership for taxonomy) | `Unauthorized` |

The first failing step rejects the message.

### 6.8 Peer Scoring

#### 6.8.1 Global Parameters

| Parameter | Value | Purpose |
|-----------|-------|---------|
| Topic score cap | 100.0 | Max positive topic contribution |
| IP colocation weight | -10.0 | Anti-Sybil (same IP penalty) |
| IP colocation threshold | 3.0 | Peers on same IP before penalty |
| Behaviour penalty weight | -10.0 | Protocol misbehaviour |
| Decay interval | 1 second | Score decay tick |
| Retain score | 3,600 seconds | Remember scores after disconnect |

#### 6.8.2 Score Thresholds

| Threshold | Value | Effect |
|-----------|-------|--------|
| Gossip | -10.0 | Suppress gossip control messages |
| Publish | -50.0 | Suppress publishing to peer |
| Graylist | -80.0 | Drop all messages from peer |
| Accept PX | 5.0 | Trust for peer exchange |
| Opportunistic graft | 3.0 | Trigger grafting above median |

#### 6.8.3 Per-Topic Parameters

| Topic | Weight | First Delivery | Invalid Penalty | Invalid Decay |
|-------|--------|----------------|-----------------|---------------|
| Catalog | 0.5 | 2.0 | -10.0 | 0.5 |
| Evidence | 0.7 | 3.0 | -15.0 | 0.5 |
| **Taxonomy** | **1.0** | **5.0** | **-50.0** | **0.3** |
| Governance | 0.8 | 3.0 | -30.0 | 0.3 |
| Profiles | 0.3 | 1.0 | -5.0 | 0.5 |
| Peer Exchange | 0.3 | 1.0 | -5.0 | 0.5 |

Taxonomy has the highest weight and strongest invalid message penalty because unauthorized taxonomy updates are the most dangerous attack vector (could corrupt the global skill graph). Governance is second-most sensitive.

### 6.9 NAT Traversal

When behind NAT: (1) Circuit Relay v2 via relay peers — obtain `/p2p/<relay>/p2p-circuit` addresses, (2) DCUtR hole punching to upgrade relayed connections to direct. AutoNAT probes determine reachability with retry interval of 60 seconds and confidence max of 2.

The relay server serves as the bootstrap node and has no special protocol authority. It provides initial contact for Kademlia DHT bootstrapping and Circuit Relay v2.

---

## 7. Cross-Device Sync

### 7.1 Design

Sync messages are encrypted with a key derived from the wallet signing key (AES-256-GCM). Only devices with the same BIP-39 mnemonic can decrypt sync data.

### 7.2 Syncable Tables

| Table | Merge Strategy | Description |
|-------|---------------|-------------|
| `enrollments` | LWW | Course enrollment status |
| `element_progress` | LWW | Learning progress per element |
| `course_notes` | LWW | User's course notes |
| `evidence_records` | Append-only | Skill evidence (never deleted) |
| `skill_proof_evidence` | Append-only | Proof-evidence links |

**Derived tables** (not synced, recomputed locally): `skill_proofs`, `reputation_assertions`.

### 7.3 Merge Strategies

**LWW (Last-Writer-Wins)**: Compare `updated_at` timestamps (ISO 8601 string comparison). Remote wins only if `remote.updated_at > local.updated_at` (strictly newer). Ties: local wins.

**Append-Only Union**: Insert if primary key does not exist. Never update or delete existing records.

### 7.4 Sync Message Protocol

Four message types: Hello (handshake with platform and sync vector), RequestSync (request rows newer than timestamps), SyncData (response with rows and operations), SyncAck (acknowledgement with merge counts).

### 7.5 Security

Table names are validated against an allowlist before use in dynamic SQL queries. Any table name not in `SYNCABLE_TABLES` is rejected.

---

## 8. Skill Taxonomy

### 8.1 Structure

Skills are organised in a three-tier hierarchy: Subject Fields → Subjects → Skills. Skills form a directed acyclic graph (DAG) with explicit prerequisite edges stored in `skill_prerequisites` (composite PK: `skill_id`, `prerequisite_id`). Non-prerequisite relationships (e.g., "related to", "builds on") are stored in `skill_relations`.

The DAG MUST be validated for cycles before inserting any prerequisite edge.

### 8.2 Interoperability

Skills carry references to external taxonomies (ESCO, O*NET) to support interoperability.

### 8.3 Taxonomy Governance

Taxonomy updates are committee-gated via the governance system and propagated over the `/alexandria/taxonomy/1.0` GossipSub topic. All taxonomy changes are versioned (`taxonomy_versions`), and old proofs remain valid across taxonomy versions. Backward-compatible parsing is required.

---

## 9. Query and Consumption Model

### 9.1 Query Principles

Consumers MUST: query by skill (not by identity), use thresholds (not rankings), and respect selective disclosure.

Consumers MUST NOT construct queries that rank individuals globally or that bypass learner disclosure preferences.

### 9.2 Skill Queries

```
SkillQuery {
    skill_id: string
    minimum_proficiency: ProficiencyLevel
    minimum_confidence?: number
}
```

The `minimum_confidence` field is OPTIONAL. When omitted, implementations SHOULD apply a platform-defined default threshold.

### 9.3 Composite Queries

```
CompositeSkillQuery {
    required: SkillQuery[]
    optional?: SkillQuery[]
}
```

A result MUST satisfy all entries in `required`. Entries in `optional` are RECOMMENDED for ranking but MUST NOT be used as exclusion criteria.

### 9.4 Reputation-Aware Queries

```
ReputationConstraint {
    role: instructor | assessor | author
    skill_id: string
    proficiency_level: ProficiencyLevel
    minimum_median_impact?: number
}
```

### 9.5 Query Results

A conforming QueryResult MUST include: `subject_id`, `matched_skills`, and `explanation`. The `explanation` field MUST provide a human-readable justification for the match. Implementations MUST NOT return opaque or unexplained scores.

### 9.6 Anti-Patterns

The following patterns are explicitly non-conforming: global rankings of individuals, hidden or opaque scoring mechanisms, and resume-style keyword inference in lieu of verified SkillProofs.

---

## 10. Governance

### 10.1 Overview

Governance in Alexandria MUST be fully decentralised, meritocratic, and evidence-based. Decision-making authority SHALL derive from demonstrated expertise as measured by scoped reputation. Governance MUST NOT derive authority from seniority, wealth, or title.

### 10.2 DAO Structure

The governance hierarchy mirrors the platform's knowledge taxonomy. When a new Subject Field is created, the system MUST automatically instantiate a corresponding top-level DAO. Each Subject within a Subject Field MUST automatically receive its own Sub-DAO.

### 10.3 Elections

Top-level DAO elections MUST be held every four (4) years for all seats on the governing committee. Sub-DAO elections MUST be held annually for all roles.

### 10.4 Nominations

Nominations for all DAO committee elections MUST be automatic and reputation-based. Each nominee MUST disclose their full reputation proof history to the DAO membership prior to the election. Nominees MUST explicitly accept or decline through active consent. The system MUST NOT enroll any actor as a candidate without their recorded consent.

### 10.5 Proposals and Voting

Each DAO MUST accept proposals from its members. The proposal lifecycle proceeds through: Draft Stage (any member may submit; draft specifies minimum skill level to vote), Committee Review (committee must approve advancement to published state), Published Stage (all qualified members may cast a vote; skill-level gate enforced at vote time).

DAO membership is any actor who holds the relevant skill levels within the scope of that DAO. Membership MUST be computed dynamically from current SkillProofs and MUST NOT be granted through manual assignment.

### 10.6 On-Chain Enforcement

**Current state**: Governance rules are enforced at the application level and P2P validation level. Transaction metadata is submitted to Cardano, but no on-chain validators exist.

**Target state**: 7 Aiken/Plutus v3 validators — DAO registration, election, proposal, committee token minting, vote receipt minting (double-vote prevention), reputation soulbound token, and credential NFT.

### 10.7 Spec Stewardship

The Alexandria specification SHOULD be governed by a neutral, public-interest body. The governing body MUST adhere to the following principles: open participation, transparent decision-making, and no proprietary extensions to core schemas.

### 10.8 Spec Evolution and Compatibility

All changes to this specification MUST be versioned. Breaking changes MUST require a major version increment. Existing SkillProofs and ReputationAssertions MUST remain valid across version boundaries.

Implementations MUST: support backward-compatible parsing of all prior schema versions, ignore unknown fields safely, and fail explicitly on incompatible versions.

---

## 11. Decentralisation

### 11.1 Overview

Alexandria MUST avoid dependence on any single provider for content storage, credential verification, or identity management. Decentralisation applies across three domains: content distribution, credential anchoring, and identity.

### 11.2 Content Distribution

All learning content MUST be stored in the iroh content-addressed blob store and addressed by BLAKE3 hash. Published course content MAY also be available via public URLs with local BLAKE3 caching. Content is propagated across the P2P network via the catalog GossipSub topic.

Once published, content hashes are immutable. Content updates produce a new hash and a new version record.

### 11.3 Credential Anchoring

**Blockchain**: Cardano (Conway era) via pallas 0.35 and Blockfrost.

Two anchoring modes coexist — the legacy NFT-based credential path (§11.3.1) and the W3C-VC integrity-anchoring path (§11.3.2). The two are complementary: the former publishes presentational metadata on-chain; the latter timestamps a verifiable hash of the canonical VC without publishing credential content.

#### 11.3.1 NFT-based credentials (legacy)

Legacy SkillProof credentials MAY be represented as non-fungible tokens minted on-chain with NativeScript signature policy and CIP-25 metadata containing skill, proficiency level, confidence score, and evidence count.

Any third party MUST be able to verify such a credential independently by querying the blockchain for the `policy_id` and `asset_name` and retrieving the metadata. Verification MUST NOT depend on the platform being available.

**Reputation Snapshots**: CIP-68 soulbound tokens with CBOR-encoded inline datums (reference + user token pair).

#### 11.3.2 VC integrity anchoring

The §14 Verifiable Credentials path anchors credential integrity on Cardano by submitting a metadata-only transaction (no minting, no output token) whose metadata label is `1697` (`ALEXANDRIA_ANCHOR_LABEL`). The metadata carries the canonical hash of the VC, the issuer DID, and the issuance timestamp, following the formula from §14.12.3:

```
A(c) = (H(c), t_i, issuer_ref)
```

where `H(c)` is `blake3` of the JCS-canonical bytes of the VC (§14.23.2). The full credential is NOT stored on-chain. Anchoring is an integrity-timestamping mechanism; it neither creates nor transfers a token and does not replace the signature-based verification defined in §14.13.

### 11.4 Decentralised Identity

The user's 24-word BIP-39 mnemonic IS their identity. Key derivation follows CIP-1852 via pallas-wallet. The same Ed25519 key serves as Cardano payment key, libp2p peer identity, and message signing key. There is no email, no OAuth, and no custodial wallet service.

### 11.5 Selective Disclosure

Learners MUST retain full control over which SkillProofs, credentials, and reputation data are disclosed to third parties. No query result SHALL include data from credentials or proofs that the learner has not explicitly chosen to disclose.

---

## 12. Assessment Integrity (Sentinel)

### 12.1 Overview

Sentinel is a client-side anti-cheat system that monitors assessment integrity through multi-signal behavioral fingerprinting. All processing happens on-device. Raw behavioral data MUST NOT leave the client.

### 12.2 Design Principles

1. **Privacy-first** — All behavioral data (keystrokes, mouse movements, video frames) is processed entirely on-device. Only numeric scores and categorical flags are stored and broadcast.
2. **Non-punitive by default** — Sentinel informs rather than punishes. Flagged sessions surface for review; automated suspensions require multiple strong signals.
3. **Dual scoring** — Rule-based and AI-based systems run in parallel. Rule-based is authoritative; AI is advisory until validated with labeled data.
4. **Zero dependencies for AI** — All ML models are hand-written in TypeScript with no external ML frameworks, WASM runtimes, or model downloads.
5. **Incremental trust** — Behavioral profiles build over time. Consistency scoring activates after 10+ samples.

### 12.3 Signal Taxonomy

| Signal | Source | Type | Weight |
|--------|--------|------|--------|
| `typing_consistency` | Rule | 0-1 | 0.20 |
| `mouse_consistency` | Rule | 0-1 | 0.15 |
| `is_human_likely` | Rule | bool | 0.15 |
| `tab_switches` | Rule | count | 0.15 |
| `paste_events` / `pasted_chars` | Rule | count | 0.10 |
| `devtools_detected` | Rule | bool | 0.10 |
| `face_present` / `face_count` | Rule | bool/int | 0.15 |
| `ai_keystroke_anomaly` | AI | 0-1 | advisory |
| `ai_mouse_human_prob` | AI | 0-1 | advisory |
| `ai_face_similarity` | AI | 0-1 | advisory |

### 12.4 ML Models

Three models, all hand-written in TypeScript with zero external dependencies, trained on-device:

- **Keystroke Autoencoder** (4→8→4→8→4) — Digraph timing features, anomaly detection via reconstruction error. Requires 20+ samples. Threshold: 0.65.
- **Mouse Trajectory CNN** (Conv1D(3→8)→Conv1D(8→16)→Dense→Sigmoid) — 50-point segments with dx/dy/dt channels. Human threshold: 0.50. Conv layers use reservoir computing (random feature extractors).
- **Face Embedder** (LBP histograms, 4×4 spatial binning, 944-dimensional) — Cosine similarity for continuous identity verification. Match threshold: 0.70. Uses YCbCr skin-color segmentation.

### 12.5 Session Outcomes

- **Clean**: Default.
- **Flagged**: 1 critical OR 3+ warnings OR integrity < 0.40.
- **Suspended**: 2+ critical OR (1 critical + 2 warnings).

### 12.6 Trust Factor Propagation

Confirmed violations lower `trust_factor` on `skill_assessments` by 0.20 per violation (floor: 0.10). This propagates through the evidence pipeline — flagged assessment evidence carries less weight in skill proof aggregation and instructor reputation attribution.

### 12.7 Privacy Guarantees

These guarantees are architectural — they are enforced by the code structure, not by policy.

- Raw keystrokes never stored: Only anonymized timing features (dwell/flight in ms).
- Raw mouse coordinates never transmitted: Only deltas used for CNN features.
- Video frames never leave the device: Face processing happens on a canvas element.
- AI model weights are not biometric data: Weights encode statistical patterns, not recoverable input data.
- Profile keyed to device: Profiles are device-specific.
- No server-side data: All behavioral processing happens on-device.

---

## 13. Threat Model

### 13.1 Reputation Gaming

**Threat**: Actors attempt to inflate reputation via low-signal instruction, collusion, or selective assessment.

**Mitigations**: Reputation derived only from verified SkillProofs. Reputation skill- and proficiency-scoped. Distribution-based with confidence weighting. Evidence strength requirements enforced.

### 13.2 Assessment Inflation

**Threat**: Instructors or assessors inflate scores to boost downstream reputation.

**Mitigations**: Assessment definitions independent of instructor. Difficulty and assessment type weighting. Sentinel integrity scoring lowers trust_factor on flagged assessments. Variance and confidence penalties for inconsistent outcomes. Stake-based evidence challenges (5 ADA, 2/3 supermajority vote).

### 13.3 Sybil Attacks

**Threat**: Creation of multiple fake identities to manipulate attribution or governance.

**Mitigations**: Instructor reputation grows only via learner SkillProofs. Attribution bounded per learner-skill pair. IP colocation scoring in P2P layer (-10.0 weight above 3 peers threshold). Optional identity verification layers. Graph-based anomaly detection supported by data model.

### 13.4 Message Forgery

**Threat**: Forged or tampered gossip messages.

**Mitigations**: Ed25519 signatures on all gossip messages, covering all envelope fields (topic, timestamp, stake_address, payload). 6-step validation pipeline with per-peer rate limiting.

### 13.5 Taxonomy Corruption

**Threat**: Unauthorized modification of the global skill graph.

**Mitigations**: Highest topic weight (1.0) and strongest invalid penalty (-50.0) in peer scoring. Committee authority verification at validation layer. Full committee membership check at domain handler layer. CommitteeUpdated events replace entire committee (no incremental adds).

### 13.6 Content Tampering

**Threat**: Modification of course content or credential metadata.

**Mitigations**: BLAKE3 content addressing (iroh). Ed25519 signed documents. Published content hashes are immutable.

### 13.7 Replay Attacks

**Threat**: Re-broadcasting valid but expired messages.

**Mitigations**: ±5 minute freshness window. Blake2b-256 dedup cache (100,000 entries, LRU eviction).

### 13.8 Assessment Fraud

**Threat**: Someone else taking assessments on behalf of the learner, or use of automated tools.

**Mitigations**: Sentinel behavioral fingerprinting (keystroke, mouse, face). Client-side ML models for continuous identity verification. Trust factor propagation through evidence pipeline. DevTools detection heuristic.

### 13.9 Key Theft

**Threat**: Compromise of wallet keys or mnemonic.

**Mitigations**: Encrypted vault — Stronghold (desktop) or AES-256-GCM + Argon2id (mobile). 12-character minimum password. Salt file HMAC-SHA256 integrity protection. Wallet struct implements Drop with zeroization; Clone removed. Biometric session password auto-clears after 15 minutes.

### 13.10 Credential Issuer Compromise

**Threat**: Theft of an issuer's signing key enables the attacker to mint arbitrary VCs in the issuer's name.

**Mitigations**: Key rotation is mandatory (§14.5.3); verifiers resolve the verification method valid at the credential's `issuanceDate` via the historical key registry, so rotating a compromised key does not invalidate pre-compromise credentials. The compromised issuer MUST revoke affected credentials via the §14.11.2 status list. §14.14.4 permits verifiers to downgrade the issuer weight for an issuer with a history of compromise or bad-faith revocation.

### 13.11 Status List Poisoning

**Threat**: An adversarial or compromised issuer flips valid credentials to revoked in the status list, de-platforming legitimate subjects.

**Mitigations**: Status lists MUST be issuer-signed and version-pinned (§14.11.2), so a reader can detect forgery and rollback. Subjects MAY retain pre-revocation evidence of good standing by caching signed status-list snapshots locally. Verifiers MAY downgrade issuer trust weight (§14.14.4) when observing statistically anomalous revocation rates from a given issuer.

### 13.12 Aggregation Gaming

**Threat**: An adversarial issuer floods the network with self-attestations, duplicate credentials, or inflated assessment scores to game the §14.14 trust score.

**Mitigations**: §14.15 anti-gaming controls — cluster-based Sybil cap (§14.15.1), self-attestation type-weight floor (§14.15.2), inflation z-score penalty (§14.15.3), and re-issuance spam discount (§14.15.4) — combine to bound the influence of any single issuer cluster and to discount correlated or inflated evidence.

---

## 14. Verifiable Credentials Protocol

*This section defines the credentialing, attestation, verification, revocation, aggregation, and survivability model for Alexandria. It is normative.*

### 14.1 Purpose

The protocol is designed to satisfy the following properties:

1. Credentials MUST remain verifiable without Alexandria-controlled infrastructure.
2. Credentials MUST be non-transferable at the semantic layer.
3. Each credential MUST have exactly one primary issuer.
4. Multiple issuers MAY independently attest to the same skill, competency, or achievement.
5. System-level trust MUST be computed through aggregation, not assumed from any single credential.
6. The protocol MUST degrade gracefully if Alexandria ceases to exist.
7. The protocol SHOULD support privacy-preserving disclosure and future zero-knowledge extensions.

### 14.2 Design Position

#### 14.2.1 Why not NFTs as primary credentials

Primary credentials MUST NOT be modeled as freely transferable NFTs because transferability destroys the semantic meaning of educational attainment.

Let `C` be a credential and `O(C, t)` be the owner of `C` at time `t`. For an educational credential to be meaningful, the following invariant is required:

```
∀t: O(C, t) = S
```

where `S` is the credential subject. If transfer is allowed, then there exist times `t_1, t_2` such that `O(C, t_1) ≠ O(C, t_2)`. This violates identity-binding and invalidates the credential as proof of personal achievement.

Therefore, transferable NFTs MUST NOT be the source of truth for educational or skill credentials.

#### 14.2.2 Why not pure SBTs as the entire model

Pure soulbound-token systems are directionally useful but insufficient as a complete architecture because they typically fail one or more of: revocation flexibility, correction workflows, selective disclosure, portability across wallets and DID methods, and long-term resilience across infrastructure changes.

A credentialing protocol for Alexandria MUST support: issuance, verification, expiration, revocation, selective presentation, key rotation, and issuer continuity (or graceful issuer death).

Therefore, Alexandria adopts a VC-first architecture, optionally with SBT-like semantics at the presentation layer, but NOT as the canonical data model.

### 14.3 System Overview

The protocol consists of six layers:

1. **Identity Layer** — DID-based identifiers for issuers and subjects (§14.5).
2. **Credential Layer** — W3C-style Verifiable Credentials representing claims (§14.6, §14.7).
3. **Status Layer** — Expiration, revocation, suspension, supersession (§14.11).
4. **Anchoring Layer** — Optional public integrity anchors for timestamping and tamper evidence (§14.12.3).
5. **Aggregation Layer** — Deterministic reputation and confidence computation across multiple credentials and attestations (§14.14).
6. **Presentation Layer** — Wallet exports, recruiter APIs, selective disclosure, optional NFT wrappers (§14.18, §14.19).

### 14.4 Core Entities

#### 14.4.1 Subject

The **Subject** is the entity about whom a claim is made. A Subject MUST be identified by a DID or DID-compatible identifier.

```json
{ "id": "did:key:z6MkUser123..." }
```

#### 14.4.2 Primary Issuer

A **Primary Issuer** is the single authority that originates and signs a credential. Each credential MUST have exactly one primary issuer:

```
|PrimaryIssuer(c)| = 1
```

#### 14.4.3 Co-Signer

A **Co-Signer** is an optional secondary signer that endorses the credential payload. A credential MAY have zero or more co-signers:

```
|CoSigners(c)| ≥ 0
```

Co-signers MUST NOT replace or obscure the primary issuer.

#### 14.4.4 Verifier

A **Verifier** is any system or agent that evaluates a credential or derived skill state. A Verifier MUST be able to perform validation without dependence on Alexandria-controlled endpoints.

#### 14.4.5 Attestor

An **Attestor** is an issuer of lightweight attestations such as peer validation, instructor endorsement, employer feedback, or DAO endorsement. Attestations MUST be represented distinctly from formal credentials.

### 14.5 Identity Model

#### 14.5.1 DID Support

Implementations MUST support at least one DID method and SHOULD support multiple. Recommended minimum support: `did:key`, `did:ethr`, equivalent pluggable DID methods.

A DID document resolver MUST yield sufficient public key material to verify signatures.

#### 14.5.2 Subject Binding

All credentials MUST be bound to a subject identifier. For credential `c` with subject `s`:

```
Subject(c) = s
```

This binding MUST NOT be alterable after issuance except through explicit supersession by a newly issued credential (§14.11.4).

#### 14.5.3 Key Rotation

Issuers MUST support key rotation. A verifier evaluating signature `σ` created at issuance time `t_i` MUST use the issuer verification method valid for `t_i`, unless superseded by a cryptographically valid historical key registry.

### 14.6 Credential Taxonomy

The system defines the following credential classes.

#### 14.6.1 FormalCredential

High-weight claim representing successful completion, competency demonstration, or institutionally recognised achievement. Examples: course completion, skill certification, competency badge, research-contribution validation.

#### 14.6.2 AssessmentCredential

Credential issued based on measurable evaluation. SHOULD include score, rubric version, and assessment method.

#### 14.6.3 AttestationCredential

Lightweight claim issued by peers, instructors, employers, or DAOs. MUST be weighted lower than formal credentials by default.

#### 14.6.4 RoleCredential

Credential asserting a governance role, teaching role, maintainer role, reviewer role, or evaluator role.

#### 14.6.5 DerivedCredential

A computed artifact representing system-level aggregation or reputation. A DerivedCredential MUST NOT be confused with source-issued credentials — it is a computation over source evidence, not a replacement for it.

### 14.7 Canonical Credential Structure

Each credential MUST conform to the following logical structure.

```json
{
  "@context": [
    "https://www.w3.org/2018/credentials/v1",
    "https://alexandria.protocol/context/v1"
  ],
  "id": "urn:uuid:credential-id",
  "type": ["VerifiableCredential", "FormalCredential"],
  "issuer": "did:key:z6MkIssuer123",
  "issuanceDate": "2026-04-13T00:00:00Z",
  "expirationDate": "2028-04-13T00:00:00Z",
  "credentialSubject": {
    "id": "did:key:z6MkSubject456",
    "claim": {
      "kind": "skill",
      "skillId": "skill:logistics.network_optimization",
      "level": 4,
      "score": 0.87,
      "evidenceRefs": ["urn:uuid:evidence-1", "urn:uuid:evidence-2"]
    }
  },
  "credentialStatus": {
    "id": "status:revocation-list:abc",
    "type": "RevocationList2020Status",
    "statusPurpose": "revocation",
    "statusListIndex": "271",
    "statusListCredential": "https://example.org/status/1"
  },
  "termsOfUse": {
    "policyVersion": "1.0",
    "usage": "verification-permitted"
  },
  "proof": {
    "type": "Ed25519Signature2020",
    "created": "2026-04-13T00:00:00Z",
    "verificationMethod": "did:key:z6MkIssuer123#key-1",
    "proofPurpose": "assertionMethod",
    "jws": "..."
  }
}
```

### 14.8 Required Credential Fields

Each credential MUST include: `id`, `type`, `issuer`, `issuanceDate`, `credentialSubject.id`, `proof`.

Formal and assessment credentials SHOULD include: `expirationDate`, `credentialStatus`, claim metadata, evidence references, and rubric or evaluation metadata where relevant.

### 14.9 Issuance Rules

#### 14.9.1 Single Primary Issuer Rule

A credential MUST have exactly one primary issuer:

```
∀c: |Issuer(c)| = 1
```

This is required to preserve revocation clarity, provenance, and accountability.

#### 14.9.2 Same Skill, Multiple Independent Issuers

The same underlying skill or competency MAY be attested by multiple issuers. For a skill `k`, subject `s`, and issuer set `I`, there exist credentials `c_1, c_2, …, c_n` such that:

```
Skill(c_i) = k,  Subject(c_i) = s,  Issuer(c_i) ≠ Issuer(c_j) for i ≠ j
```

These credentials are independent attestations, not duplicates of a jointly owned truth.

#### 14.9.3 Co-Signed Credentials

A credential MAY include co-signers. If co-signers are present:

- the primary issuer MUST remain explicit
- each co-signer MUST supply its own proof
- verifiers MAY consider co-signers in trust scoring

#### 14.9.4 Issuance Preconditions

Before issuance, the issuer MUST verify:

1. subject DID validity
2. claim schema validity
3. evaluation completion if assessment-based
4. evidence integrity
5. policy compatibility
6. issuer authorization to issue that credential type

### 14.10 Non-Transferability Semantics

The protocol enforces semantic non-transferability, regardless of storage or wrapper format.

For any credential `c` bound to subject `s`, a verifier MUST reject any presentation where presenter identity `p` does not satisfy:

```
p = s   OR   p cryptographically controls s
```

If a credential is merely copied, wrapped, mirrored, or tokenized but cannot be bound to the presenting subject, it MUST NOT be accepted as valid proof of personal achievement.

### 14.11 Expiration, Revocation, Suspension, and Supersession

#### 14.11.1 Expiration

A credential MAY include an expiration date. For verification time `t_v` and expiration time `t_e`:

```
Expired(c, t_v) = 1  if t_v > t_e
                = 0  otherwise
```

If a credential is expired, the verifier MUST either reject it or downgrade its weight according to verifier policy.

Default protocol behavior: expired formal credentials SHOULD be treated as inactive unless explicitly allowed by policy.

#### 14.11.2 Revocation

Each revocable credential MUST provide a resolvable status reference. A credential is revoked if `Revoked(c) = 1`. Revoked credentials MUST NOT contribute positive weight to the active trust score.

#### 14.11.3 Suspension

Temporary invalidation MAY be supported through status lists. For suspension interval `[t_s, t_r]`, a credential is suspended at verification time `t_v` if `t_s ≤ t_v ≤ t_r`. Suspended credentials MUST be excluded from positive active computations during suspension.

#### 14.11.4 Supersession

A newer credential MAY supersede an older one. For credentials `c_old` and `c_new`, if all four hold — same subject, same claim kind, same issuer, explicit supersession reference — then `c_new` supersedes `c_old`.

Superseded credentials MAY remain historically visible but SHOULD NOT be treated as the current active state.

### 14.12 Storage Model

#### 14.12.1 Subject-Controlled Storage

Credentials MUST be delivered to a subject-controlled wallet or storage agent. Alexandria-controlled storage MAY exist for convenience but MUST NOT be the sole source required for future verification.

#### 14.12.2 Durability Model

Implementations SHOULD support one or more of: local encrypted wallet storage, exportable JSON-LD packages, content-addressed backup, decentralised archival networks, user-controlled cloud backup.

#### 14.12.3 Integrity Anchoring

Credential hashes SHOULD be anchored in a public integrity layer. Let `H(c)` be the canonical hash of credential `c`. If anchored, the anchor record MUST store:

```
A(c) = (H(c), t_i, issuer_ref)
```

The full credential SHOULD NOT be stored on-chain unless explicitly required and privacy-compatible.

### 14.13 Verification Algorithm

#### 14.13.1 Verification Result Structure

The verifier MUST output a structured result:

```json
{
  "credentialId": "urn:uuid:credential-id",
  "validSignature": true,
  "issuerResolved": true,
  "revoked": false,
  "expired": false,
  "subjectBound": true,
  "integrityAnchored": true,
  "verificationTime": "2026-04-13T00:00:00Z",
  "acceptanceDecision": "accept"
}
```

#### 14.13.2 Verification Procedure

For credential `c` at verification time `t_v`, the verifier MUST execute, in order:

1. validate schema
2. canonicalize payload
3. resolve issuer DID
4. verify signature
5. check `issuanceDate` sanity
6. check expiration
7. check status list
8. validate subject binding
9. optionally verify integrity anchor
10. emit decision and metadata

#### 14.13.3 Acceptance Predicate

Define the boolean components:

- `S(c)` ∈ {0,1} — signature valid
- `D(c)` ∈ {0,1} — issuer DID resolvable
- `B(c)` ∈ {0,1} — subject binding valid
- `R(c)` ∈ {0,1} — revoked flag (1 means revoked)
- `E(c)` ∈ {0,1} — expired flag (1 means expired)
- `P(c)` ∈ {0,1} — policy compatibility

Baseline acceptance:

```
Accept(c) = 1  if S(c)=1 ∧ D(c)=1 ∧ B(c)=1 ∧ R(c)=0 ∧ P(c)=1
          = 0  otherwise
```

Expiration handling MAY be policy-specific. The strict default is:

```
AcceptStrict(c) = 1  if Accept(c)=1 ∧ E(c)=0
                = 0  otherwise
```

### 14.14 Trust Aggregation Model

#### 14.14.1 Objective

The aggregation layer computes a **Derived Skill State** from a set of valid credentials and attestations. The protocol's central premise is:

> issuance is local; trust is aggregated.

#### 14.14.2 Inputs

For a subject `s` and skill `k`, let the evidence set be:

```
E_{s,k} = { e_1, e_2, …, e_n }
```

Each evidence item `e_i` has: issuer `I_i`, raw score `q_i ∈ [0,1]`, credential type `τ_i`, issuance time `t_i`, expiration time `t_{e,i}` (if applicable), revocation status, confidence metadata, evidence quality metadata.

Only accepted evidence (per §14.13) enters aggregation.

#### 14.14.3 Evidence Weight

Each accepted evidence item receives an effective weight:

```
w_i = w_issuer,i × w_type,i × w_fresh,i × w_quality,i × w_independence,i
```

Each factor is defined in §14.14.4–§14.14.8.

#### 14.14.4 Issuer Weight

Each issuer has a trust prior:

```
w_issuer,i ∈ [0,1]
```

This is a protocol- or verifier-configurable prior based on issuer credibility, governance legitimacy, auditability, historical accuracy, or role. Examples: a recognized institution carries a higher prior; a new peer attestor carries a lower one.

The protocol MUST keep issuer weighting transparent and explainable.

#### 14.14.5 Type Weight

Credential class default weights SHOULD follow:

| Type | Weight |
|------|-------:|
| FormalCredential | 1.00 |
| AssessmentCredential | 0.90 |
| RoleCredential | 0.60 |
| AttestationCredential | 0.35 |
| SelfAssertion | 0.25 |

Implementations MAY tune these but MUST expose the chosen values.

#### 14.14.6 Freshness Weight

Evidence SHOULD decay over time for fast-changing skills. Let `t_v` be the verification time, `t_i` the issuance time, `Δt_i = t_v - t_i`, and `λ_k` the decay constant for skill `k`. Then:

```
w_fresh,i = exp(-λ_k × Δt_i)
```

A smaller `λ_k` means slow decay; a larger `λ_k` means fast decay. Foundational mathematics: low decay. Fast-moving software framework: higher decay.

If explicit expiration exists and `t_v > t_{e,i}`, the credential SHOULD be treated as inactive unless policy says otherwise.

#### 14.14.7 Quality Weight

Evidence quality weight captures assessment rigor and evidence richness. Let:

- rubric completeness `r_i ∈ [0,1]`
- proctoring or anti-cheat reliability `a_i ∈ [0,1]`
- evidence traceability `x_i ∈ [0,1]`

Then:

```
w_quality,i = α_r × r_i + α_a × a_i + α_x × x_i,   subject to   α_r + α_a + α_x = 1
```

Recommended defaults: `α_r = 0.4, α_a = 0.3, α_x = 0.3` (see §14.25.3).

#### 14.14.8 Independence Weight

Multiple credentials from highly correlated issuers SHOULD NOT scale trust linearly. Let `ρ_ij ∈ [0,1]` be the dependence estimate between evidence items `e_i` and `e_j`, where 0 is independent and 1 is fully dependent (same origin in disguise). A practical independence discount for item `i` is:

```
w_independence,i = 1 / (1 + Σ_{j≠i} ρ_ij)
```

This reduces overweighting of repeated endorsements from the same trust cluster.

#### 14.14.9 Raw Evidence Score

Each evidence item has a normalised score `q_i ∈ [0,1]`. Examples:

- percentage score: `q_i = percent / 100`
- rubric level `L ∈ {1,2,3,4,5}`: `q_i = (L - 1) / 4`
- pass/fail: `1` or `0`

#### 14.14.10 Aggregated Skill Score

The system computes the weighted mean:

```
Q_{s,k} = Σ(w_i × q_i) / Σ(w_i)
```

where `Q_{s,k} ∈ [0,1]`. If `Σ w_i = 0`, no active score exists.

#### 14.14.11 Evidence Mass

The total evidence mass is:

```
M_{s,k} = Σ w_i
```

This is not the same as score — it measures how much weighted evidence exists.

#### 14.14.12 Confidence Function

Confidence SHOULD increase with weighted evidence mass and issuer diversity. Let `M_{s,k}` be the evidence mass, `U_{s,k}` the number of unique effective issuer clusters, `β > 0` the mass-saturation parameter, and `γ > 0` the issuer-diversity parameter. Then:

```
C_{s,k} = (1 - exp(-β × M_{s,k})) × (1 - exp(-γ × U_{s,k}))
```

with `C_{s,k} ∈ [0,1]`. More evidence increases confidence; more diverse issuers increase confidence; confidence saturates rather than growing unbounded.

#### 14.14.13 Final Skill Trust Score

The final trust score combines quality and confidence:

```
T_{s,k} = Q_{s,k} × C_{s,k}
```

`Q_{s,k}` answers "how strong is the evidence?"; `C_{s,k}` answers "how sure are we?".

#### 14.14.14 Level Mapping

Continuous score MAY be mapped to discrete levels. Recommended 5-level mapping:

| Q range | Level |
|---|:---:|
| 0.00 ≤ Q < 0.20 | 1 |
| 0.20 ≤ Q < 0.40 | 2 |
| 0.40 ≤ Q < 0.60 | 3 |
| 0.60 ≤ Q < 0.80 | 4 |
| 0.80 ≤ Q ≤ 1.00 | 5 |

Implementations MAY use alternative thresholds but MUST publish them.

### 14.15 Anti-Gaming Controls

#### 14.15.1 Sybil Resistance for Attestations

Attestation-based trust MUST NOT scale linearly with raw count. If multiple attestations originate from identities with low independence, the system MUST discount them.

A simple cluster-based cap is:

```
W_cluster = min(Σ_{i ∈ cluster} w_i,  κ_cluster)
```

where `κ_cluster` is a maximum influence cap.

#### 14.15.2 Self-Attestation Limits

Self-assertions MAY exist for discovery or portfolio use but MUST have low default type weight (see §14.14.5) and MUST NOT alone generate high-confidence derived states.

#### 14.15.3 Assessment Inflation Control

Issuers with abnormally high pass rates or score inflation SHOULD be downweighted. Let `μ_I` be the issuer average awarded score, `μ_G` the global benchmark average for comparable assessment type, and `σ_G` the benchmark standard deviation. Define the inflation z-score:

```
z_I = (μ_I - μ_G) / σ_G
```

Then optional penalty:

```
p_I = 1                             if z_I ≤ z_max
    = exp(-η × (z_I - z_max))       if z_I > z_max
```

Adjusted issuer weight: `w'_issuer,I = w_issuer,I × p_I`.

#### 14.15.4 Repeated Re-Issuance Spam Control

If the same issuer repeatedly issues nearly identical credentials for the same claim without materially new evidence, only the most recent or highest-quality active credential SHOULD be counted, or earlier ones MUST be strongly discounted.

### 14.16 Derived Skill State Output

The aggregation layer MUST produce an explainable object:

```json
{
  "subject": "did:key:z6MkSubject456",
  "skillId": "skill:logistics.network_optimization",
  "rawScore": 0.81,
  "confidence": 0.74,
  "trustScore": 0.5994,
  "level": 5,
  "evidenceMass": 3.92,
  "uniqueIssuerClusters": 3,
  "activeEvidenceCount": 5,
  "calculationVersion": "1.0",
  "sources": [
    "urn:uuid:credential-1",
    "urn:uuid:credential-2",
    "urn:uuid:credential-3"
  ]
}
```

### 14.17 Recruiter / Consumer Query Semantics

External consumers often do not want raw credentials. They want answers to questions like: Does this person demonstrably possess skill `k`? At what level? With what confidence? Backed by whom? Based on how much evidence?

Therefore the protocol SHOULD support query responses of the form:

```
Response(s, k) = (L_{s,k}, Q_{s,k}, C_{s,k}, T_{s,k}, M_{s,k}, U_{s,k})
```

A consumer MAY define acceptance criteria such as `Q_{s,k} ≥ q_min ∧ C_{s,k} ≥ c_min`. Example hiring threshold: `Q_{s,k} ≥ 0.75 ∧ C_{s,k} ≥ 0.65`.

### 14.18 Privacy and Selective Disclosure

#### 14.18.1 Minimum Requirement

The system SHOULD support partial presentation of credentials. For example, a subject SHOULD be able to prove possession of a qualifying credential, achievement above a threshold, or current non-revoked status — without revealing unnecessary fields.

#### 14.18.2 Presentation Policies

A presentation MAY reveal: credential existence only, issuer only, score only, level only, derived trust state only, or the full credential. Consumers SHOULD request the minimum data needed.

#### 14.18.3 Future ZK Compatibility

Schemas SHOULD be designed so that claim structures can later support zero-knowledge predicates such as `Q_{s,k} ≥ 0.8` or `L_{s,k} ≥ 4` without exposing raw underlying evidence.

### 14.19 NFT Wrapper Rules

An NFT wrapper MAY exist purely as a presentation artifact. If an NFT wrapper is used:

1. it MUST NOT be the canonical credential record
2. it MUST reference the canonical credential hash or presentation artifact
3. it MUST NOT cause a verifier to ignore subject binding
4. transfer of the NFT MUST NOT imply transfer of achievement
5. verifier logic MUST still validate the underlying credential or derived state

NFTs may serve as display objects, but MUST NOT redefine trust semantics.

### 14.20 Failure Mode: Alexandria Ceases to Exist

#### 14.20.1 Required Survivability Property

The protocol MUST remain minimally functional if Alexandria-controlled systems disappear. Let `A = 0` denote Alexandria service death. Then for existing credentials `c`, the protocol aims to preserve:

```
Verify(c | A = 0) = 1
```

provided that: subject still possesses the credential, issuer DID remains resolvable or historically resolvable, signature suite remains supported, and status mechanism remains publicly available (if needed).

#### 14.20.2 What Must Survive

The following MUST survive Alexandria shutdown if the protocol is correctly implemented: credential possession by subjects, signature verification, subject-binding verification, optional integrity-anchor checking, historical evidence portability.

#### 14.20.3 What May Degrade

The following MAY degrade if Alexandria disappears: convenience APIs, hosted dashboards, centralised analytics, reputation recomputation services operated only by Alexandria, any revocation path that was improperly centralised.

Therefore, revocation and aggregation SHOULD be designed to be reproducible by third parties.

#### 14.20.4 Strong Survivability Rule

No credential issued under this protocol SHALL require Alexandria-controlled infrastructure as the sole means of later verification.

### 14.21 System Interfaces

#### 14.21.1 Issue Credential

**Input**: issuer auth context, subject DID, claim payload, evidence refs, expiration policy, status configuration.

**Output**: signed credential, integrity hash, optional anchor receipt.

#### 14.21.2 Verify Credential

**Input**: credential or presentation, verification time, verifier policy.

**Output**: verification result structure (§14.13.1).

#### 14.21.3 Aggregate Skill State

**Input**: subject DID, skill ID, accepted evidence set, weighting configuration, aggregation version.

**Output**: derived skill state (§14.16).

### 14.22 Recommended Pseudocode

#### 14.22.1 Verify Credential

```text
function verifyCredential(c, t_v, policy):
    require schemaValid(c)
    issuerDoc = resolveDID(c.issuer)
    if issuerDoc == null:
        return reject("issuer_unresolved")

    if !verifySignature(c, issuerDoc):
        return reject("bad_signature")

    if !subjectBindingValid(c):
        return reject("bad_subject_binding")

    if isRevoked(c, t_v):
        return reject("revoked")

    if isExpired(c, t_v) and policy.rejectExpired:
        return reject("expired")

    if !policyCompatible(c, policy):
        return reject("policy_incompatible")

    return accept(metadata)
```

#### 14.22.2 Aggregate Skill State

```text
function aggregateSkillState(subject, skill, evidenceSet, t_v, config):
    accepted = []
    for e in evidenceSet:
        if verifyCredential(e, t_v, config.policy).accepted:
            accepted.append(e)

    if len(accepted) == 0:
        return noEvidenceState(subject, skill)

    weightedScores = []
    weights = []

    for e in accepted:
        q = normalizeScore(e)
        wIssuer = issuerWeight(e.issuer, config)
        wType = typeWeight(e.type, config)
        wFresh = freshnessWeight(e, skill, t_v, config)
        wQuality = qualityWeight(e, config)
        wInd = independenceWeight(e, accepted, config)
        w = wIssuer * wType * wFresh * wQuality * wInd

        weightedScores.append(w * q)
        weights.append(w)

    Q = sum(weightedScores) / sum(weights)
    M = sum(weights)
    U = effectiveIssuerClusters(accepted, config)
    C = (1 - exp(-config.beta * M)) * (1 - exp(-config.gamma * U))
    T = Q * C
    L = mapLevel(Q, config)

    return {
        subject, skill,
        rawScore: Q, confidence: C, trustScore: T,
        level: L, evidenceMass: M, uniqueIssuerClusters: U
    }
```

### 14.23 Security Requirements

#### 14.23.1 Private Key Safety

Issuer private keys MUST NOT be exposed to application clients or untrusted agents.

#### 14.23.2 Canonicalization Safety

All signing and verification flows MUST use deterministic canonicalization (e.g., RFC 8785 JCS) to prevent signature mismatch.

#### 14.23.3 Replay Resistance

Credential IDs MUST be unique. Issuance time and proof creation time SHOULD be recorded. Where presentation tokens are used, they SHOULD include nonce and audience binding.

#### 14.23.4 Auditability

Issuers SHOULD log issuance metadata, rubric versions, and evidence references in auditable but privacy-compatible form.

### 14.24 Minimal Implementation Profile

A minimal viable Alexandria-compliant implementation MUST support:

1. DID-backed subject identifiers
2. exactly one primary issuer per credential
3. signed VC issuance
4. status checking
5. expiration handling
6. local verification without Alexandria dependency
7. deterministic aggregation over multiple issuers for the same skill
8. explainable output of score, confidence, and level

### 14.25 Recommended Default Parameters

These defaults are suggested for v1.

#### 14.25.1 Type Weights

| Type | Weight |
|------|-------:|
| FormalCredential | 1.00 |
| AssessmentCredential | 0.90 |
| RoleCredential | 0.60 |
| AttestationCredential | 0.35 |
| SelfAssertion | 0.25 |

#### 14.25.2 Freshness

Recommended decay constants per year-equivalent unit:

| Skill domain | λ |
|---|---:|
| Foundational knowledge | 0.02 |
| Operational practices | 0.08 |
| Technical tools / frameworks | 0.15 |

#### 14.25.3 Confidence

Recommended: `β = 0.6`, `γ = 0.7`. Quality-weight component coefficients: `α_r = 0.4`, `α_a = 0.3`, `α_x = 0.3`.

#### 14.25.4 Inflation Penalty

Recommended threshold: `z_max = 1.5`, `η = 0.5`.

### 14.26 Example Worked Computation

Assume subject `s` and skill `k` have three accepted evidence items.

**Evidence 1** — formal credential, `q_1 = 0.90`:

```
w_1 = w_issuer,1 × w_type,1 × w_fresh,1 × w_quality,1 × w_independence,1
    = 0.95 × 1.00 × 0.90 × 0.92 × 1.00
    = 0.7866
```

**Evidence 2** — assessment credential, `q_2 = 0.78`:

```
w_2 = 0.85 × 0.90 × 0.95 × 0.88 × 0.85 = 0.544221
```

**Evidence 3** — attestation, `q_3 = 0.80`:

```
w_3 = 0.70 × 0.35 × 0.98 × 0.70 × 0.80 = 0.134456
```

**Raw Score**:

```
Q_{s,k} = (w_1 q_1 + w_2 q_2 + w_3 q_3) / (w_1 + w_2 + w_3)
        = (0.7866·0.90 + 0.544221·0.78 + 0.134456·0.80) / 1.465277
        = 1.240 / 1.465277
        ≈ 0.846
```

**Evidence Mass**: `M_{s,k} = 1.465277`. Assume `U_{s,k} = 3`, `β = 0.6`, `γ = 0.7`.

**Confidence**:

```
C_{s,k} = (1 - exp(-0.6 × 1.465277)) × (1 - exp(-0.7 × 3))
        = (1 - exp(-0.8791662)) × (1 - exp(-2.1))
        ≈ (1 - 0.415) × (1 - 0.122)
        ≈ 0.585 × 0.878
        ≈ 0.514
```

**Trust Score**: `T_{s,k} = Q_{s,k} × C_{s,k} ≈ 0.846 × 0.514 ≈ 0.435`.

**Level**: Using the §14.14.14 mapping, `Q_{s,k} ≈ 0.846 ⇒ L_{s,k} = 5`.

This yields strong underlying evidence quality, moderate confidence due to still-limited total evidence mass, and an elite provisional level with room for confidence growth. The reference implementation reproduces these numbers in `tests/e2e_vc/aggregation.rs`.

### 14.27 Summary of Normative Conclusions

1. Primary credentials MUST be VC-first, not NFT-first.
2. The same credential MUST NOT be issued by multiple primary issuers.
3. The same skill MAY be independently attested by multiple issuers.
4. Trust MUST be aggregated from multiple accepted pieces of evidence.
5. Aggregation MUST be deterministic, explainable, and non-destructive to source credentials.
6. Credentials MUST remain verifiable without Alexandria-controlled infrastructure.
7. NFTs MAY exist only as optional presentation wrappers and MUST NOT redefine credential ownership semantics.
8. Attestations MUST be weighted lower than rigorous credentials by default.
9. Freshness, issuer trust, evidence quality, and issuer independence SHOULD all affect aggregation.
10. The system MUST be survivable, portable, and auditable.

---

## 15. Reference Implementation

*This section is informative. It documents the architecture of the reference implementation to aid contributors and adopters. It does not define conformance requirements for the protocol.*

### 15.1 Architecture

The reference implementation is a Tauri v2 application — a single binary that bundles a Rust backend with a Vue 3 frontend. It runs on macOS, Linux, Windows, iOS, and Android.

| Component | Technology | Purpose |
|-----------|------------|---------|
| Backend | Rust (tokio) | Business logic, wallet, P2P, database, evidence, governance |
| Frontend | Vue 3, TypeScript, Tailwind CSS v4 | 30 pages, 34 components, 15 composables |
| Database | SQLite (rusqlite, bundled) | 66 tables, 30 migrations |
| Content | iroh 0.96 | BLAKE3 content-addressed blob store |
| P2P | libp2p 0.56 | Kademlia, GossipSub, Relay, DCUtR, request-response/CBOR for `/alexandria/vc-fetch/1.0` |
| Wallet | pallas 0.35, Stronghold / AES-256-GCM | Conway era transactions, encrypted key storage |
| Cardano | pallas, Blockfrost | NFT minting, reputation snapshots, governance metadata, VC integrity anchoring (label 1697) |
| Integrity | TypeScript ML models | Keystroke autoencoder, mouse CNN, face embedder |
| Tutoring | iroh-live | Video, audio, screenshare (desktop) |
| CLI | Rust, clap 4 | Developer tooling (`alex`) |
| **VC sign/verify** | `domain::vc/{mod,canonicalize,context,sign,verify}` | Ed25519Signature2020 detached JWS over RFC 8785 JCS bytes, §14.7 / §14.13 |
| **Trust aggregation** | `aggregation::{mod,weights,level,independence,antigaming,config}` | §14.14 engine + §14.15 anti-gaming; reproduces the §14.26 worked example |
| **VC P2P layer** | `p2p::{vc_did,vc_status,vc_fetch,presentation,pinboard,archive}` | Wire layer for §14.5, §14.11.2, §14.18, §14.12.2 |
| **VC storage** | `commands::{credentials,presentation,pinning,aggregation}`, `db` migrations 22–30 | Issuance, verification, revocation, suspension, allowlist, presentation envelopes, PinBoard commitments |

### 15.2 Database

**Engine**: SQLite (rusqlite 0.38, bundled). **Tables**: 66 across 30 migrations.

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

Key design decisions: deterministic IDs via `hex(blake2b_256(parts.join("|")))`, singleton identity table with `CHECK (id = 1)`, content stored externally in iroh blobs.

### 15.3 Test Suite

407 backend tests across crypto, database, P2P, evidence, cardano, and domain modules. ~1500 lines of stress tests covering high-volume gossip (200+ messages), concurrent validation (1000 messages / 10 threads), sync conflicts, and adversarial inputs.

---

## 16. References

### Normative References

- **[RFC 2119]** Bradner, S., "Key words for use in RFCs to Indicate Requirement Levels", BCP 14, RFC 2119, March 1997. https://www.rfc-editor.org/rfc/rfc2119
- **[RFC 7797]** Jones, M., "JSON Web Signature (JWS) Unencoded Payload Option", RFC 7797, February 2016 — used for the Ed25519Signature2020 detached-JWS pattern in §14.7. https://www.rfc-editor.org/rfc/rfc7797
- **[RFC 8785]** Rundgren, A., Jordan, B., Erdtman, S., "JSON Canonicalization Scheme (JCS)", RFC 8785, June 2020 — used as the canonicalisation algorithm feeding signing and anchoring in §14.7, §14.12.3, §14.23.2. https://www.rfc-editor.org/rfc/rfc8785
- **[W3C VC-DATA-MODEL]** Sporny, M., Longley, D., Chadwick, D., "Verifiable Credentials Data Model v1.1", W3C Recommendation, March 2022 — model underlying §14.6–§14.8. https://www.w3.org/TR/vc-data-model/
- **[did:key]** Longley, D., Zundel, B., Sporny, M., "The did:key Method v0.7", W3C CCG Draft — DID method used in §14.5.1. https://w3c-ccg.github.io/did-key-spec/
- **[Ed25519Signature2020]** Longley, D., Sporny, M., "Ed25519 Signature 2020", W3C CCG Draft — proof suite referenced in §14.7. https://w3c-ccg.github.io/lds-ed25519-2020/
- **[StatusList2021]** Sporny, M., Longley, D., "Verifiable Credentials Status List v2021", W3C CCG Draft — revocation model in §14.11.2. https://w3c.github.io/vc-status-list-2021/
- **[BIP-39]** Palatinus, M., Rusnak, P., Voisine, A., Bowe, S., "Mnemonic code for generating deterministic keys", Bitcoin Improvement Proposal 39, 2013. https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki
- **[CIP-25]** Cardano Improvement Proposal 25, "Media NFT Metadata Standard", 2021. https://cips.cardano.org/cip/CIP-0025
- **[CIP-30]** Cardano Improvement Proposal 30, "Cardano dApp-Wallet Web Bridge", 2021. https://cips.cardano.org/cip/CIP-0030
- **[CIP-68]** Cardano Improvement Proposal 68, "Datum Metadata Standard", 2022. https://cips.cardano.org/cip/CIP-0068
- **[CIP-1852]** Cardano Improvement Proposal 1852, "HD Wallets for Cardano", 2019. https://cips.cardano.org/cip/CIP-1852
- **[JSON Schema]** Wright, A., Andrews, H., Hutton, B., Dennis, G., "JSON Schema: A Media Type for Describing JSON Documents", Internet-Draft, 2020. https://json-schema.org/draft/2020-12/schema

### Informative References

- **[Bloom's Taxonomy]** Anderson, L.W., Krathwohl, D.R. (Eds.), "A Taxonomy for Learning, Teaching, and Assessing", Longman, 2001.
- **[ESCO]** European Commission, "European Skills, Competences, Qualifications and Occupations". https://esco.ec.europa.eu
- **[O*NET]** U.S. Department of Labor, "O*NET OnLine". https://www.onetonline.org
- **[iroh]** number0, "iroh — Content-addressed data distribution". https://iroh.computer
- **[libp2p]** Protocol Labs, "libp2p — Modular peer-to-peer networking framework". https://libp2p.io
- **[Cardano]** Hoskinson, C., "Cardano", IOHK. https://cardano.org
- **[Tauri]** Tauri Contributors, "Tauri — Build desktop and mobile apps with web frontend". https://tauri.app
- **[pallas]** TxPipe, "Pallas — Cardano primitives in Rust". https://github.com/txpipe/pallas
- **[BBS+]** Decentralized Identity Foundation, "BBS+ Signatures v2023" — referenced from §14.18.3 for future zero-knowledge presentation compatibility. https://identity.foundation/bbs-signature/

---

## Authors

Pratyush Pundir for IFFTU
GitHub: https://github.com/ifftu-dev
Project: https://github.com/ifftu-dev/alexandria
