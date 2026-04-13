# Alexandria Protocol Specification

**Status:** Draft v0.0.1
**Category:** Standards Track
**Created:** 2025
**Updated:** 2026
**Author:** Pratyush Pundir

---

## Implementation Status

The §4 reputation system has been substantially extended by the
**Alexandria Credential & Reputation Protocol v1** (separate spec
document) which lands a W3C-style Verifiable Credential model
alongside the legacy skill-proof + NFT pipeline described in §4
below. Both models coexist:

- **Legacy path (§4 + §5)** — assessment → skill_proof → optional
  Cardano NFT mint. Still implemented and supported. See
  `evidence::aggregator`, `commands::cardano::mint_skill_proof_nft`.
- **VC-first path (PRs 2–13)** — `did:key` identity + signed VCs +
  status-list revocation + deterministic aggregation +
  selective-disclosure presentations + offline survivability bundle.
  See `docs/architecture.md` §13 for the layer overview and the
  v1 credential-reputation spec for the normative protocol.

For each section in this document, see the implementation-status
table at `docs/architecture.md` §13 to determine which PR landed
the corresponding feature.

---

## Conventions and Terminology

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in RFC 2119.

The following terms are used throughout this specification:

- **Subject Field** — A top-level domain of knowledge (e.g. "Computer Science", "Mathematics").
- **Subject** — A discrete area of study contained within a Subject Field (e.g. "Distributed Systems", "Linear Algebra").
- **Skill** — An atomic, assessable unit of competence within a Subject. Skills form a directed acyclic graph (DAG) with explicit prerequisite edges.
- **SkillProof** — A verifiable, learner-owned credential attesting to demonstrated proficiency in a specific Skill at a specific Bloom's taxonomy level.
- **ReputationAssertion** — A computed, evidence-derived claim about the instructional, assessment, or authorship impact of an actor within a given scope.
- **DAO** — Decentralised Autonomous Organisation; a governance body responsible for decision-making within a defined scope.
- **ProficiencyLevel** — An ordered enumeration of learner capability within a Skill, based on Bloom's Taxonomy: `remember`, `understand`, `apply`, `analyze`, `evaluate`, `create`.
- **Node** — A running instance of the Alexandria application (desktop or mobile), containing a complete database, content store, P2P networking stack, and wallet.
- **Sentinel** — The client-side assessment integrity system that monitors behavioral signals during assessments.

For motivation, design rationale, and stakeholder context, see the companion document: *Alexandria: Free Knowledge, Verified Skills, No Gatekeepers*.

---

## 1. Scope

This specification defines the data models, reputation mechanics, query interfaces, governance structures, decentralisation requirements, peer-to-peer protocol, evidence pipeline, assessment integrity, and threat mitigations for the Alexandria protocol. It is intended as a normative reference for implementors, contributors, and governance participants.

---

## 2. Architecture

### 2.1 Overview

Alexandria is a **Tauri v2 application** — a single binary that bundles a Rust backend with a Vue 3 frontend. Every user runs a full node. There are no servers, no Docker containers, and no external databases.

All state lives on the user's device in three locations:

| Store | Purpose |
|-------|---------|
| SQLite | Relational data (courses, skills, evidence, governance) — 53 tables, 19 migrations |
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

The frontend communicates with the Rust backend via approximately 160 Tauri IPC commands across 22 modules (classroom, governance, taxonomy, tutoring, identity, attestation, sync, challenge, courses, content, integrity, catalog, enrollment, p2p, reputation, snapshot, chapters, elements, evidence, cardano, health, storage).

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

---

## 4. Reputation System

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
| `/alexandria/peer-exchange/1.0` | Known peer address propagation |

All 6 topics MUST be subscribed on node startup.

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

**NFT-Based Credentials**: Each credential MUST be represented as a non-fungible token minted on-chain with NativeScript signature policy and CIP-25 metadata containing skill, proficiency level, confidence score, and evidence count.

**Credential Verification**: Any third party MUST be able to verify a credential independently by querying the blockchain for the `policy_id` and `asset_name`, retrieving the metadata. Verification MUST NOT depend on the platform being available.

**Reputation Snapshots**: CIP-68 soulbound tokens with CBOR-encoded inline datums (reference + user token pair).

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

---

## 14. Reference Implementation

*This section is informative. It documents the architecture of the reference implementation to aid contributors and adopters. It does not define conformance requirements for the protocol.*

### 14.1 Architecture

The reference implementation is a Tauri v2 application — a single binary that bundles a Rust backend with a Vue 3 frontend. It runs on macOS, Linux, Windows, iOS, and Android.

| Component | Technology | Purpose |
|-----------|------------|---------|
| Backend | Rust (tokio) | Business logic, wallet, P2P, database, evidence, governance |
| Frontend | Vue 3, TypeScript, Tailwind CSS v4 | 26 pages, 32 components, 12 composables |
| Database | SQLite (rusqlite, bundled) | 53 tables, 19 migrations |
| Content | iroh 0.96 | BLAKE3 content-addressed blob store |
| P2P | libp2p 0.56 | Kademlia, GossipSub, Relay, DCUtR |
| Wallet | pallas 0.35, Stronghold / AES-256-GCM | Conway era transactions, encrypted key storage |
| Cardano | pallas, Blockfrost | NFT minting, reputation snapshots, governance metadata |
| Integrity | TypeScript ML models | Keystroke autoencoder, mouse CNN, face embedder |
| Tutoring | iroh-live | Video, audio, screenshare (desktop) |
| CLI | Rust, clap 4 | Developer tooling (`alex`) |

### 14.2 Database

**Engine**: SQLite (rusqlite 0.38, bundled). **Tables**: 53 across 19 migrations.

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

### 14.3 Test Suite

407 backend tests across crypto, database, P2P, evidence, cardano, and domain modules. ~1500 lines of stress tests covering high-volume gossip (200+ messages), concurrent validation (1000 messages / 10 threads), sync conflicts, and adversarial inputs.

---

## 15. References

### Normative References

- **[RFC 2119]** Bradner, S., "Key words for use in RFCs to Indicate Requirement Levels", BCP 14, RFC 2119, March 1997. https://www.rfc-editor.org/rfc/rfc2119
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

---

## Authors

Pratyush Pundir for IFFTU
GitHub: https://github.com/ifftu-dev
Project: https://github.com/ifftu-dev/alexandria
