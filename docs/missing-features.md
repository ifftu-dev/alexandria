# Missing Features

> Features and documentation that have not yet been implemented.

**Last updated**: 2026-04-08

---

## Table of Contents

1. [Documentation Gaps](#1-documentation-gaps)
2. [Smart Contracts](#2-smart-contracts)
3. [Authentication](#3-authentication)
4. [Web Companion](#4-web-companion)
5. [Monitoring & Observability](#5-monitoring--observability)
6. [Frontend Testing](#6-frontend-testing)
7. [Content Moderation](#7-content-moderation)
8. [Search](#8-search)
9. [Sentinel Server-Side Models](#9-sentinel-server-side-models)
10. [Feature Status Matrix](#10-feature-status-matrix)

---

## 1. Documentation Gaps

Alexandria has 10 documentation files (architecture, database schema, P2P protocol, project structure, skills & reputation, sentinel, missing features, security audit, performance audit, README), but several areas lack written specs:

| Document | Status |
|----------|--------|
| Whitepaper | **Missing** — Reputation formulas, governance rules, decentralisation criteria, sustainability model, threat model, and credential schemas are all implemented but have no written spec. The mark2 whitepaper (v0.0.3) contains the conceptual framework but references a Go/Docker/PostgreSQL architecture; a mark3 whitepaper should be written fresh. |
| IPC command reference | **Missing** — A reference for the ~160 IPC commands could be written. |
| Skills & reputation RFC | **Created** — Skill graph design, evidence model, reputation system, and query/consumption model. |
| Sentinel architecture doc | **Created** — Client-side behavioral fingerprinting, ML models, signal taxonomy, and privacy guarantees. |
| Security audit | **Created** — 32 findings (1 critical, 7 high, 10 medium, 9 low, 5 informational). |
| Database schema | **Created** |
| Project structure | **Created** |
| Architecture | **Created** |

**Recommendation**: Write a whitepaper.

---

## 2. Smart Contracts

All 7 Aiken/Plutus v3 governance validators have been **written and compiled** (`cardano/governance/`). The Rust transaction builders in `src-tauri/src/cardano/gov_tx_builder.rs` and `soulbound_tx_builder.rs` are implemented and dispatch correctly from the on-chain governance queue.

| Validator | Status |
|-----------|--------|
| `dao_registry` (DAO state) | Written, compiled |
| `dao_minting` (DAO state tokens) | Written, compiled |
| `election` (lifecycle) | Written, compiled |
| `proposal` (lifecycle) | Written, compiled |
| `vote_minting` (double-vote prevention) | Written, compiled |
| `reputation_minting` (CIP-68 soulbound) | Written, compiled |
| `soulbound` (non-transferable spending) | Written, compiled |

**Remaining work**: Deploy the 7 validators as reference scripts on Cardano preprod testnet (`cardano/governance/deploy_reference_scripts.sh`), then update `script_refs.rs` with the deployment tx hashes. Once deployed, the governance queue automatically submits Plutus transactions.

**Priority**: High — the validators are ready, deployment is the last step.

---

## 3. Authentication

The only authentication method is the BIP-39 mnemonic + encrypted vault (Stronghold on desktop, AES-256-GCM + Argon2id on mobile). There is no email, no OAuth, and no browser extension wallet support.

| Method | Status |
|--------|--------|
| BIP-39 mnemonic (self-sovereign) | **Implemented** (only method) |
| Email/password | Not implemented |
| OAuth (Google, Apple, LinkedIn) | Not implemented |
| CIP-30 wallet (browser extension) | Not implemented |

**Impact**: Users must understand and safeguard a 24-word mnemonic. Non-crypto users have no custodial onramp.

**Priority**: Low for the desktop app (self-sovereign is the design goal), but important if a web companion is ever built.

---

## 4. Web Companion

No web companion exists. All functionality is in the desktop and mobile app. A lightweight web UI could provide:

- Credential verification (read-only, from Cardano)
- Public skill graph browser
- Read-only course catalog
- No sign-in required for verification

**Priority**: Low — the desktop app is the primary interface. A verification-only web page could be built later as a static site that reads from Cardano directly.

---

## 5. Monitoring & Observability

The app uses `tauri-plugin-log` for basic logging. There are no dashboards, no metrics collection, and no alerting. This is expected for a desktop app, but developer-facing diagnostics could be improved.

**Priority**: Low — desktop apps don't need Prometheus. But a diagnostics/debug page in the Settings UI would be useful.

---

## 6. Frontend Testing

**No frontend tests exist.** The backend has 407 tests, but the Vue frontend has zero test coverage. There is no Vitest or Playwright configuration.

**Priority**: Medium — the backend is well-tested, but the frontend has complex flows (onboarding, course player, quiz engine, governance UI) that would benefit from testing.

---

## 7. Content Moderation

No content moderation system exists. Any peer can publish any course to the catalog topic. The peer scoring system penalizes invalid messages, but there is no mechanism for reporting or removing objectionable content.

**Priority**: Medium — important for a public network, but not blocking for development/testnet.

---

## 8. Search

Course discovery relies on the P2P catalog topic (GossipSub). The frontend has a course listing page but no full-text search. Skills can be browsed by taxonomy but not searched.

**Priority**: Medium — a local SQLite FTS5 index could provide search without any server.

---

## 9. Sentinel Server-Side Models

The Sentinel anti-cheat system uses a client-side rule-based scoring engine (11 signals) with ML models (`keystroke-autoencoder.ts`, `mouse-trajectory-cnn.ts`, `face-embedder.ts`). A server-side decision tree ensemble for final adjudication is not implemented. In a P2P architecture, this would need to run locally or be replaced by peer consensus.

**Priority**: Low — the client-side models provide the core integrity signal. Server-side aggregation was mainly for a centralized architecture.

---

## 10. Feature Status Matrix

| Feature | Status | Notes |
|---------|:------:|-------|
| **Core Platform** | | |
| Course content (text, video, quiz) | Y | iroh content-addressed storage |
| Skill taxonomy (DAG) | Y | SQLite-backed |
| Evidence pipeline | Y | Fully implemented |
| Skill proofs | Y | Fully implemented |
| Reputation system | Y | Fully implemented |
| **Identity** | | |
| BIP-39 mnemonic wallet | Y | Only auth method |
| Encrypted vault (Stronghold / portable) | Y | Stronghold on desktop, AES-256-GCM + Argon2id on mobile |
| Biometric unlock (Face ID / Touch ID) | Y | via tauri-plugin-biometry |
| Auto-updater | Y | via tauri-plugin-updater |
| Email/password auth | - | Not applicable (desktop app) |
| OAuth (Google, Apple, LinkedIn) | - | Not applicable |
| CIP-30 wallet auth | - | Not applicable (embedded wallet) |
| **Blockchain** | | |
| Cardano transaction building | Y | Conway era (pallas) |
| NFT credential minting | Y | NativeScript + CIP-25 |
| CIP-68 soulbound tokens | Y | |
| Aiken smart contracts | Y | 7 validators compiled, tx builders implemented — awaiting preprod deployment |
| **Networking** | | |
| libp2p P2P | Y | |
| GossipSub (6 global + per-classroom topics) | Y | |
| Cross-device sync | Y | |
| Relay-based discovery (Kademlia DHT) | Y | |
| NAT traversal (relay + DCUtR) | Y | |
| Mobile nodes (iOS + Android) | Y | Full node on iPhone and Android |
| **Governance** | | |
| DAOs | Y | |
| Elections | Y | |
| Proposals | Y | |
| On-chain enforcement | Y | Plutus v3 tx builders + on-chain queue — awaiting preprod deployment |
| P2P gossip governance | Y | |
| **Integrity** | | |
| Client-side rule engine | Y | |
| Client-side ML models | Y | Autoencoder, CNN, LBP embedder |
| Server-side decision tree | - | **Missing** — no server |
| **UI / Design** | | |
| Refined Editorial design system | Y | Shadow-only cards, glassmorphism stats, serif greetings, off-white bg |
| Sidebar (collapsible, Live Tutoring, Classrooms) | Y | Inline previews with status dots, marquee, unread badges |
| Sidebar skill graph widget | Y | force-graph canvas with earned/available/locked nodes |
| Course cards (hover lift, thumbnail zoom) | Y | CourseCard component with glassmorphism stats pills |
| TopBar user menu (role badge, icon SVGs) | Y | Rounded-xl dropdown |
| Mobile tab bar (backdrop blur, active indicator) | Y | bg-black/50 backdrop, left bar active indicator |
| Live Tutoring pages | Y | iroh-live video/audio/screenshare with desktop + mobile variants |
| Classrooms pages | Y | 26 commands, channels, messages, calls, role-based auth |
| **Infrastructure** | | |
| Developer CLI | Y | Rust + clap (`alex`) |
| **Testing** | | |
| Backend tests | Y | 437 tests |
| Frontend unit tests (Vitest) | - | **Missing** |
| E2E tests (Playwright) | - | **Missing** |
| Stress tests | Y | ~1500 lines |
| **Documentation** | | |
| Whitepaper | - | **Missing** |
| Architecture doc | Y | |
| Database schema doc | Y | |
| P2P protocol spec | Y | |
| IPC command reference | - | **Missing** |
| Skills & reputation RFC | Y | |
| Sentinel doc | Y | |
| Security audit | Y | 32 findings |
| Project structure | Y | |
