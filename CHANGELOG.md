# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project loosely follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added — VC-first credential migration (PRs 2–13)

End-to-end implementation of the Alexandria Credential & Reputation
Protocol v1 (`alexandria-credential-reputation-protocol-v1.md`)
alongside the existing skill-proof + NFT pipeline. See
`docs/architecture.md` §13 for the full layer breakdown.

- **PR 2** — Unit-test scaffolding across every stub introduced in
  PR 1 (20 active wire-shape tests + 73 `#[ignore]`'d behaviour
  specs un-ignored as their landing PRs arrive).
- **PR 3** — `did:key` self-resolving DIDs over Ed25519
  (multicodec 0xed + multibase base58btc) plus a historical
  `key_registry` (migration 22) for §5.3 historical-key
  verification across rotation.
- **PR 4** — VC sign/verify pipeline. JCS canonicalization
  (`serde_json_canonicalizer`), embedded W3C + Alexandria
  JSON-LD contexts, Ed25519Signature2020 detached JWS over
  JCS bytes, full §13.2 verification algorithm with the §13.3
  acceptance predicate.
- **PR 5** — Canonical credential storage (`credentials` table,
  migration 23) + RevocationList2020-style status lists
  (`credential_status_lists`). New IPC: `issue_credential`,
  `list_credentials`, `get_credential`, `revoke_credential`,
  `verify_credential_cmd`. Verify hot-path consults the status
  list bitmap.
- **PR 6** — Deterministic aggregation engine (§14, §16, §22.2):
  weighted mean Q, evidence mass M, unique issuer clusters U,
  saturating confidence C, trust score T = Q·C, level mapping.
  §25 default parameters baked in; §26 worked example
  (Q ≈ 0.846, C ≈ 0.514, L = 5) reproduced in e2e tests.
- **PR 7** — Anti-gaming controls (§15): cluster cap,
  inflation z-score from the credentials table, exponential
  penalty above z_max. Per-DID issuer clustering as the v1
  baseline.
- **PR 8** — Cardano integrity-anchor queue scaffolding
  (`credential_anchors`, migration 24) with idle-node contract:
  `tick` silently no-ops without `BLOCKFROST_PROJECT_ID` /
  wallet credentials. Real on-chain submission scheduled for
  a follow-up PR with testnet validation.
- **PR 9** — P2P propagation handlers for four new gossip
  topics: `vc-did`, `vc-status`, `vc-presentation`, `pinboard`,
  plus the `vc-fetch/1.0` request-response handler. Authority-
  respecting fetch (subject = self-allow, others = unauthorized).
- **PR 10** — PinBoard storage layer (`pinboard_observations`,
  migration 25) + IPC commands. DHT archive discovery surface
  with idle-node contract.
- **PR 11** — Selective-disclosure presentations (§18 + §23.3).
  Redact-and-resign envelope: subject signs a JCS-canonical
  bundle of redacted credentials bound to (audience, nonce);
  `presentations_seen` (migration 26) provides per-verifier
  replay protection.
- **PR 12** — Survivability bundle export + offline verifier
  (§20.4). `export_credentials_bundle` IPC produces a JCS-
  canonical JSON document carrying credentials + key registry
  + status lists; `verify_bundle_offline_impl` re-loads into a
  fresh ephemeral DB and runs the full §13.2 pipeline. Bundles
  are byte-identical for same inputs (content-addressable).
- **PR 13** — IPC command registration (lib.rs `invoke_handler!`
  block now exposes all 16 new VC commands) + wired-up
  aggregation IPC handlers backed by a new `derived_skill_states`
  cache (migration 27). Documentation sweep across
  `docs/architecture.md` (§13 added), `docs/database-schema.md`
  (migrations 22–27 + new VC domain section),
  `docs/protocol-specification.md` (implementation-status
  preamble), and `docs/skills-and-reputation.md` (PR cross-refs).

#### Migration scoreboard

| PR | Lib unit tests | Lib ignored | E2E pass | E2E ignored |
|----|----|----|----|----|
| 2  | 470 | 73 | 0  | 37 |
| 3  | 483 | 63 | 3  | 34 |
| 4  | 500 | 49 | 7  | 30 |
| 5  | 506 | 49 | 8  | 29 |
| 6  | 526 | 29 | 12 | 25 |
| 7  | 537 | 20 | 16 | 21 |
| 8  | 540 | 17 | 18 | 19 |
| 9  | 551 | 7  | 18 | 19 |
| 10 | 560 | **0** | 18 | 19 |
| 11 | 566 | 0  | 21 | 16 |
| 12 | 571 | 0  | 24 | 14 |
| 13 | 576 | 0  | 24 | 14 |

The 14 still-ignored e2e tests have `unimplemented!()` bodies and
need a real two-node libp2p fixture (`p2p_did_status`,
`p2p_vc_fetch`, `p2p_survival`, `pinning` 5-tier eviction). They
land in PR 17.

## [2026-03-25]

### Added
- **Classrooms**: Full classroom/cohort management with 24 Tauri commands.
  - Create/archive classrooms, manage members with role-based access (owner/moderator/member).
  - Text and announcement channels with real-time P2P messaging via per-classroom gossip topics.
  - Join request workflow (request, approve, deny).
  - Voice/video calls via iroh-live integration (desktop), with mobile stubs.
  - 4 frontend pages: classroom list, detail/messages, settings, join requests.
  - `useClassroom` composable with singleton state and real-time Tauri event listeners.
- **Live Tutoring**: Video/audio/screenshare sessions via iroh-live.
  - TutoringManager with platform-specific variants (desktop, mobile, Android).
  - 14 commands: create/join/leave rooms, toggle video/audio/screenshare, chat.
  - TutoringPiP component for picture-in-picture call overlay.
  - Database tables: tutoring_sessions, tutoring_peers, tutoring_chat.
- **Security remediation**: 21 of 27 audit findings fixed.
  - DOMPurify sanitization on all `v-html` sites (XSS prevention).
  - Restrictive Content Security Policy enabled in Tauri.
  - Wallet struct implements Drop with zeroization; Clone removed.
  - 12-character minimum password enforcement.
  - Salt file HMAC-SHA256 integrity protection (desktop + mobile keystores).
  - Per-peer token-bucket rate limiter for gossip messages.
  - Committee updates wrapped in transactions.
  - Proposal status validated against allowlist.
  - Sync column names validated against injection.
  - Raw `p2p_publish` command removed from IPC surface.
  - SSRF blocklist for IPFS content resolver.
  - Biometric session password auto-clears after 15 minutes.
  - Mutex `.unwrap()` replaced with `.map_err()` across all command handlers.
- **CI**: Security audit job with `cargo-audit` and updater key placeholder check.
- **Fonts**: Inter and JetBrains Mono bundled locally (removed Google Fonts CDN dependency).

### Fixed
- Mobile tab bar "Classrooms" linked to `/courses` instead of `/classrooms`.
- Onboarding password hint said "8 characters" but backend enforced 12.

## [2026-03-03]

### Added
- Public content availability flow for fresh installs and cross-device access:
  - URL-based content IDs and resolver support with local BLAKE3 caching.
  - `bootstrap_public_catalog` and `hydrate_catalog_courses` commands.
  - Bundled public catalog dataset (`bootstrap/public_courses.json`).
- Home post-unlock content sync attempt with bottom-bar status messaging and completion stats.

### Changed
- Mobile tab bar remains fixed to the required four tabs: Home, Live Tutoring, Classrooms, Skill Graph.

### Notes
- PR #43 was an intermediate stacked PR that closed when its base branch merged.
- Equivalent changes were merged through PR #44.
