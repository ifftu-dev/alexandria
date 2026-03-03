# Missing Features — (Mark 3) vs (Mark 2)

> What exists in the (Mark 2) documentation and codebase that has not yet been
> implemented or documented in (Mark 3).

**Last updated**: 2026-03-03

---

## Table of Contents

1. [Documentation Gaps](#1-documentation-gaps)
2. [Smart Contracts](#2-smart-contracts)
3. [Authentication](#3-authentication)
4. [API & Web Companion](#4-api--web-companion)
5. [Monitoring & Observability](#5-monitoring--observability)
6. [Cloud Deployment](#6-cloud-deployment)
7. [Frontend Testing](#7-frontend-testing)
8. [Security Audit](#8-security-audit)
9. [Content Moderation](#9-content-moderation)
10. [Search](#10-search)
11. [Sentinel Server-Side Models](#11-sentinel-server-side-models)
12. [Feature Comparison Matrix](#12-feature-comparison-matrix)

---

## 1. Documentation Gaps

(Mark 2) has 11 documentation files covering the full platform specification.
(Mark 3) now has 8 docs (architecture, database schema, P2P protocol, project
structure, missing features, security audit, performance audit, README), but
several mark2 documents have no mark3 equivalent:

| (Mark 2) Document | (Mark 3) Equivalent | Status |
|-----------------|-------------------|--------|
| `whitepaper-consolidated-v0.0.3.md` | None | **Missing** — The whitepaper covers reputation formulas, governance rules, decentralisation criteria, sustainability model, threat model, and credential schemas. (Mark 3) implements most of this but has no written spec. |
| `api-reference.md` | None | **N/A** — (Mark 3) has no REST API (IPC commands replace it). A command reference could be written. |
| `skills-and-reputation.md` | None | **Missing** — RFC covering skill graph design, evidence model, reputation system, and query/consumption model. The implementation exists in mark3 but the design document doesn't. |
| `sentinel.md` | None | **Missing** — Architecture doc for the Sentinel anti-cheat system. (Mark 3) has `useSentinel.ts` and the ML model utilities, but no documentation. |
| `security-audit-v0.0.1.md` | `security-audit.md` | **Created** — 24 findings (1 critical, 4 high, 8 medium, 6 low, 5 info) |
| `cloud-deployment.md` | None | **N/A** — (Mark 3) is a desktop app; no cloud deployment. |
| `database-architecture.md` | `database-schema.md` | **Created** (this session) |
| `project-structure.md` | `project-structure.md` | **Created** (this session) |
| `architecture-v2.md` | `architecture.md` | **Created** (this session) |

**Recommendation**: Write a mark3 whitepaper, skills/reputation RFC, and Sentinel doc.

---

## 2. Smart Contracts

(Mark 2) had **7 Aiken/Plutus v3 validators** for on-chain governance enforcement:

- DAO registration validator
- Election validator (nomination, voting, finalization)
- Proposal validator (submission, voting, resolution)
- Committee token minting policy
- Vote receipt minting policy (double-vote prevention)
- Reputation soulbound token validator
- Credential NFT validator

**(Mark 3) status**: All governance and credential operations use **transaction metadata only** — no on-chain validators. Transactions are submitted to Cardano with metadata fields, but there is no smart contract enforcement. The `cardano/governance.rs` module builds metadata, not Plutus scripts.

**Impact**: Governance rules (supermajority, committee authority, election lifecycle) are enforced only at the application level and P2P validation level. A malicious node could bypass these checks by submitting raw transactions. On-chain enforcement would require porting the Aiken validators.

**Priority**: Medium — the P2P validation pipeline provides meaningful protection for the network, but on-chain enforcement is the full trust model.

---

## 3. Authentication

(Mark 2) supported 4 authentication methods:

| Method | (Mark 2) | (Mark 3) |
|--------|--------|--------|
| Email/password | Yes | No |
| OAuth (Google, Apple, LinkedIn) | Yes | No |
| CIP-30 wallet (browser extension) | Yes | No |
| BIP-39 mnemonic (self-sovereign) | Yes (custodial) | **Yes** (only method) |

**(Mark 3) status**: The only authentication method is the BIP-39 mnemonic + encrypted vault (Stronghold on desktop, AES-256-GCM + Argon2id on mobile). There is no email, no OAuth, and no browser extension wallet support.

**Impact**: Lower barrier to entry was a mark2 design goal — non-crypto users could sign up with email and get a custodial wallet. (Mark 3) requires users to understand and safeguard a 24-word mnemonic.

**Priority**: Low for the desktop app (self-sovereign is the design goal), but important if a web companion is ever built.

---

## 4. API & Web Companion

(Mark 2) architecture planned for a web companion alongside the desktop app:

- Lightweight web UI for credential verification
- Public skill graph browser
- Read-only course catalog
- No sign-in required for verification

**(Mark 3) status**: No web companion exists. All functionality is in the desktop and mobile app.

**Priority**: Low — the desktop app is the primary interface. A verification-only web page could be built later as a static site that reads from Cardano directly.

---

## 5. Monitoring & Observability

(Mark 2) had:

- **Grafana dashboards** for all services
- **Prometheus alerting** with 25 alert rules across 3 severity levels
- **Structured logging** across all Go services

**(Mark 3) status**: The app uses `tauri-plugin-log` for basic logging. There are no dashboards, no metrics collection, and no alerting. This is expected for a desktop app, but developer-facing diagnostics could be improved.

**Priority**: Low — desktop apps don't need Prometheus. But a diagnostics/debug page in the Settings UI would be useful.

---

## 6. Cloud Deployment

(Mark 2) had **Terraform configurations** for:

- AWS ECS Fargate
- GCP Cloud Run
- Azure ACI
- 3 environments (dev, staging, prod)

**(Mark 3) status**: Not applicable. (Mark 3) is a native desktop application distributed as a binary. Deployment is `cargo tauri build`.

---

## 7. Frontend Testing

(Mark 2) had:

- **Vitest** unit tests for Vue components
- **Playwright** E2E tests for critical flows

**(Mark 3) status**: **No frontend tests exist.** The backend has 309 tests, but the Vue frontend has zero test coverage. There is no Vitest or Playwright configuration.

**Priority**: Medium — the backend is well-tested, but the frontend has complex flows (onboarding, course player, quiz engine, governance UI) that would benefit from testing.

---

## 8. Security Audit

(Mark 2) had a documented security audit (`security-audit-v0.0.1.md`) with 66 findings:

- 13 Critical
- 18 High
- 20 Medium
- 15 Low

**(Mark 3) status**: A security audit has been performed (`security-audit.md`) with 24 findings. Many of the mark2 findings (Apple OAuth unverified, default secrets, no rate limiting, CORS *) don't apply to a desktop app, but new threat vectors exist:

- Stronghold vault password strength enforcement
- Local file permission security (SQLite, vault, iroh store)
- IPC command authorization (any frontend code can call any command)
- P2P message flooding / resource exhaustion
- Mnemonic exposure in memory

**Priority**: High before any mainnet deployment.

---

## 9. Content Moderation

(Mark 2)'s architecture-v2 doc identified content moderation as an open question:

> "Content Moderation — With no central authority, how are spam courses or
> harmful content handled?"

**(Mark 3) status**: No content moderation system exists. Any peer can publish any course to the catalog topic. The peer scoring system penalizes invalid messages, but there is no mechanism for reporting or removing objectionable content.

**Priority**: Medium — important for a public network, but not blocking for development/testnet.

---

## 10. Search

(Mark 2)'s architecture-v2 doc identified search as an open question:

> "How do users discover courses and skills without a centralized search
> index?"

**(Mark 3) status**: Course discovery relies on the P2P catalog topic (GossipSub). The frontend has a course listing page but no full-text search. Skills can be browsed by taxonomy but not searched.

**Priority**: Medium — a local SQLite FTS5 index could provide search without any server.

---

## 11. Sentinel Server-Side Models

(Mark 2)'s Sentinel used a dual architecture:

- **Client-side**: Rule-based scoring (11 signals) — **implemented in mark3**
- **Server-side**: Decision tree ensemble for final adjudication — **not implemented in mark3**

(Mark 3) has the client-side ML models (`keystroke-autoencoder.ts`, `mouse-trajectory-cnn.ts`, `face-embedder.ts`) but there is no server-side aggregation. In a P2P architecture, the "server-side" model would need to run locally or be replaced by peer consensus.

**Priority**: Low — the client-side models provide the core integrity signal. Server-side aggregation was mainly for a centralized architecture.

---

## 12. Feature Comparison Matrix

| Feature | (Mark 2) | (Mark 3) | Notes |
|---------|:------:|:------:|-------|
| **Core Platform** | | | |
| Course content (text, video, quiz) | Y | Y | (Mark 3) uses iroh instead of IPFS |
| Skill taxonomy (DAG) | Y | Y | Same model, SQLite instead of PostgreSQL+Neo4j |
| Evidence pipeline | Y | Y | Fully implemented |
| Skill proofs | Y | Y | Fully implemented |
| Reputation system | Y | Y | Fully implemented |
| **Identity** | | | |
| Email/password auth | Y | - | Not applicable (desktop app) |
| OAuth (Google, Apple, LinkedIn) | Y | - | Not applicable |
| CIP-30 wallet auth | Y | - | Not applicable (embedded wallet) |
| BIP-39 mnemonic wallet | Y | Y | Only auth method in mark3 |
| Encrypted vault (Stronghold / portable) | - | Y | Stronghold on desktop, AES-256-GCM + Argon2id on mobile |
| **Blockchain** | | | |
| Cardano transaction building | Y | Y | Conway era (pallas) |
| NFT credential minting | Y | Y | NativeScript + CIP-25 |
| CIP-68 soulbound tokens | - | Y | New in mark3 |
| Aiken smart contracts | Y | - | **Missing** — metadata only |
| **Networking** | | | |
| REST/gRPC API | Y | - | Replaced by Tauri IPC |
| libp2p P2P | - | Y | New in mark3 |
| GossipSub (6 topics) | - | Y | New in mark3 |
| Cross-device sync | - | Y | New in mark3 |
| Relay-based discovery (Kademlia DHT) | - | Y | New in mark3 (mDNS removed) |
| NAT traversal (relay + DCUtR) | - | Y | New in mark3 |
| iOS mobile node | - | Y | New in mark3 — full node on iPhone |
| **Governance** | | | |
| DAOs | Y | Y | |
| Elections | Y | Y | |
| Proposals | Y | Y | |
| On-chain enforcement | Y | - | **Missing** — app-level only |
| P2P gossip governance | - | Y | New in mark3 |
| **Integrity** | | | |
| Client-side rule engine | Y | Y | |
| Client-side ML models | Y | Y | Autoencoder, CNN, LBP embedder |
| Server-side decision tree | Y | - | **Missing** — no server |
| **UI / Design** | | | |
| Refined Editorial design system | Y | Y | Shadow-only cards, glassmorphism stats, serif greetings, off-white bg |
| Sidebar (collapsible, Live Tutoring, Classrooms) | Y | Y | Inline previews with status dots, marquee, unread badges |
| Sidebar skill graph widget | Y | Y | force-graph canvas with earned/available/locked nodes |
| Course cards (hover lift, thumbnail zoom) | Y | Y | CourseCard component with glassmorphism stats pills |
| TopBar user menu (role badge, icon SVGs) | Y | Y | Mark 2-style rounded-xl dropdown |
| Mobile tab bar (backdrop blur, active indicator) | Y | Y | bg-black/50 backdrop, left bar active indicator |
| Live Tutoring pages | Y | - | **Missing** — preview cards link to /courses placeholder |
| Classrooms pages | Y | - | **Missing** — preview cards link to /courses placeholder |
| **Infrastructure** | | | |
| Docker Compose | Y | - | Not applicable |
| Terraform (AWS/GCP/Azure) | Y | - | Not applicable |
| Grafana + Prometheus | Y | - | Not applicable |
| Developer CLI | Y | Y | Go→Rust, different commands |
| **Testing** | | | |
| Backend tests | Y | Y | 309 tests (mark3), Go tests (mark2) |
| Frontend unit tests (Vitest) | Y | - | **Missing** |
| E2E tests (Playwright) | Y | - | **Missing** |
| Stress tests | - | Y | New in mark3 (~1500 lines) |
| **Documentation** | | | |
| Whitepaper | Y | - | **Missing** |
| Architecture doc | Y | Y | Created this session |
| Database schema doc | Y | Y | Created this session |
| P2P protocol spec | - | Y | New in mark3 |
| API reference | Y | - | N/A (IPC replaces API) |
| Skills & reputation RFC | Y | - | **Missing** |
| Sentinel doc | Y | - | **Missing** |
| Security audit | Y | Y | Created (24 findings) |
| Cloud deployment guide | Y | - | N/A |
| Project structure | Y | Y | Created this session |
