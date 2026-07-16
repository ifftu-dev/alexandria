# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project loosely follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **iroh 1.0 P2P storage layer** — the blob/content stack moved to the iroh 1.0 endpoint + router API, with the MoQ live-media crates now owned in-tree.
- **Onboarding roles + birthdate** (migration 66) — the node owner declares learner / instructor / parent-guardian; learners self-assert a birthdate that is stored local-only and never published or gossiped.
- **Guardian links** (migration 67) — a minor learner's profile stays gated in `pending_guardian` until a parent/guardian on their own device accepts a sealed cross-device invite over the `/alexandria/guardian/1.0` protocol; guardian tables never join sync or gossip.
- **Governance on-chain bridge** (migrations 58–59) — an on-chain state machine plus off-chain signed votes and a Cardano-anchored tally; operator-signed create-DAO / proposal / vote / outcome anchoring, verified on preprod.
- **Stake-pubkey registry** (migration 52) — a persistent stake-address → libp2p Ed25519 pubkey registry seeded from a multisig-signed bootstrap and reconciled against on-chain registration txs, replacing in-memory TOFU.
- **Username registry** (migrations 54–57) — claimable usernames + profile visibility distributed over the DHT, with on-chain anchor verification and a local record mirror for desktop DHT servers.
- **Role / job-description assessments** (migration 62) — enterprise sponsors fund role-specific assessments tied to a JD and issuance policy; passing with a satisfying integrity session yields a gated RoleCredential.
- **Goal templates** (migration 69) — learner goals (exams, board-grade curricula, job roles, or parsed JDs) resolve to a target "ideal skill graph"; curated maps are DAO-ratified and distributed over `/alexandria/goal-templates/1.0`.
- **Assessment question banks** (migration 70) — DAO-ratified banks draw randomized, difficulty-stratified, host-graded attempts (correct answers never leave the node); passing issues a Sentinel-gated AssessmentCredential.
- **Skill-claim provenance tiers** (migration 68) — evidence quality is weighted by provenance (self-declared < document-backed < accredited-institution < distinct-issuer VC) instead of a flat 1.0, and the highest tier badges the skill in the UI.
- **Skill-graph bootstrap from documents** — onboarding can ingest an uploaded resume/transcript to seed the learner's current skill graph.
- **Editor / code plugins** — graded in-browser code editors that run locally in sandboxed WebAssembly (JavaScript/TypeScript on Boa, Python on RustPython, C/C++ via JSCPP inside Boa); graded submissions are re-run by the host's deterministic grader and passing issues a signed credential.
- **Desktop auto-updater** — in-app self-update (macOS signed path, then Windows/Linux) with a mobile update notice.
- **Deep linking** on all platforms (`alexandria://` + `https` app-links).
- **Localization** — UI translated into 9 languages with a plain-language copy rewrite.
- **Sentinel gaze / second-device detection** (migration 60) and automated integrity attestation with an assurance ladder (migration 61), feeding an Integrity → VC issuance gate.
- Auto-mint on course completion with a celebration modal, a derived-credentials page with evidence drill-down, and persisted/read-only assessment responses.

### Changed

- **Bumped iroh 0.x → 1.0.2, iroh-blobs 0.98 → 0.103, iroh-gossip → 0.101**; the minimum supported Rust version (MSRV) is now 1.91.0.
- **Vendored `iroh-live`, `iroh-moq`, and `moq-media` in-tree** under `crates/`, ported to iroh 1.0 and referenced by path — no git dependency or patch.
- Completion-credential mints are paid by the treasury; the learner only signs.
- Renamed the "Targets" feature to **Goals**.
- Seed taxonomy + goals on every platform and stopped auto-enrolling fresh users.
- Raised the minimum iOS version to 16.4.

### Fixed

- **Plutus-script txs were invalid on-chain** (Spend redeemer with no `script_data_hash`); all governance flows and challenge-escrow settlement now build valid txs and are verified on preprod.
- macOS biometric-unlock entitlement restored, and the mobile sync-status bar is hidden.
- iOS P2P crash after wallet unlock fixed by patching `netdev` for iroh 1.0.

### Launch-readiness UI pass + biometric unlock

User-facing polish for the alpha, plus a fix that restored biometric
unlock after the multi-user picker landed.

**UI:**

- Preview/Alpha badge in the top bar (exact version shown in the tooltip).
- Dashboard: a hero "next action" card (resume course / set a target /
  start a course) and accent-iconed stat tiles.
- New reusable `InfoTip` popover (touch-safe, no popover library) with
  explanations on the reputation metrics.
- Mobile horizontal rails get correct right-edge padding (WebKit drops a
  scroll container's `padding-right`).
- Course player: the mobile chapter `<select>` is replaced by a bottom-sheet
  chapter navigator (accordion + per-chapter progress).
- Profile selection screen gains the onboarding Starfield background.
- The diagnostic overlay is gated behind `import.meta.env.DEV`.

**Auth:**

- Biometric unlock restored. The multi-user picker (`ProfileSelect`) replaced
  the old unlock screen but never wired up biometric retrieval; Touch ID /
  Face ID now fires at the picker again.
- Keychain credentials are keyed per profile (`vault_password_<profileId>`),
  so any enrolled profile on a multi-user device can biometric-unlock — not
  just the last one enabled.

**Build:**

- `+fullfp16` enabled for `aarch64-apple-ios` so `gemm-f16` compiles for iOS
  device builds (its fp16 NEON intrinsics require the target-feature; the
  app's candle use is F32-only so the kernels never execute).

### Unified per-profile settings store (branch `feat/synced-settings`)

Every user-controlled preference now lives in one place — the
per-profile `app_settings` table — and propagates to the user's
other devices via the existing cross-device sync. See
[`docs/settings.md`](docs/settings.md) for the architecture.

**Highlights:**

- New `src-tauri/src/settings/` module with a typed registry
  (`registry::keys`), a validated R/W store (`SettingsStore`), and
  three IPC commands (`list_settings`, `set_setting`, `reset_setting`).
- Migration `048_app_settings_scope` adds a `scope` column (`sync` /
  `device`). Existing `storage_quota_bytes` becomes `device`-scoped
  (per-device disk capacity); every other key defaults to `sync`.
- 19 settings registered, covering everything previously scattered
  across `localStorage`, ad-hoc DB rows, hardcoded seed data, and
  env vars: `ui.theme`, `ui.sidebar_collapsed`, `ui.sidebar_sections`,
  `input.keyboard_shortcuts`, `ui.omni_recents`,
  `sentinel.{ai_scoring_enabled,paste_classifier_enabled,camera_enabled,keyboard_enabled}`,
  `notifications.enabled`, `sync.auto`, `user.language`,
  `video.{default_volume,default_muted}`,
  `cardano.{blockfrost_project_id,completion_policy_id}`,
  `device.label`, `storage.quota_bytes`, `ui.window_geometry`.
- Sync hooks in `p2p/sync.rs`: `settings_outbound_snapshot` +
  `settings_apply_inbound` carry only `scope='sync'` rows, LWW on
  `updated_at`, refuses inbound writes for unknown keys or
  device-scoped keys.
- Frontend `useSettings` composable (reactive entries + listens for
  `settings-changed` events from other windows or inbound sync).
  Per-key two-way ref via `useSetting<T>(key)`.
- Migrated callers: `useTheme`, `useKeyboardShortcuts`, `useOmniSearch`,
  `useSentinel`, `AppLayout` sidebar, `AppSidebar` sections,
  `VideoPlayer` mute/volume defaults. `localStorage` is kept as a
  synchronous cache so pre-unlock paint matches; reconciled with
  the per-profile store via `init*FromSettings` hooks fired from
  `App.vue` after profile unlock.
- New "All settings" panel in `SettingsModal` renders every
  registered setting grouped by category, with the right widget
  (toggle / textbox / number / JSON), a sync/device chip, and a
  Reset button when the value differs from the default.
- Dropped 6 unused `app_settings` seed rows that the app never
  read.

**Profile-switch bugfixes** (shipped in the same branch):

- **Theme bled across profiles.** `init*FromSettings` was copying
  the previously-active profile's `localStorage` value into the
  new profile's settings on every unlock. Removed; each profile
  starts at the registry default. `useTheme` rewritten to use
  `useSetting<string>('ui.theme')` so explicit toggles, profile
  switches, and inbound sync deliveries all repaint immediately.
  Added `useProfiles::onProfileLocked` callback + `clearSettingsCache()`
  so the picker (and the next-unlocked profile) does not flash
  the prior profile's preferences. `useKeyboardShortcuts` +
  `useOmniSearch` reset their singleton state at the start of
  every hydrate.
- **iroh blob store deadlocked on second unlock within a
  process.** `ContentNode::shutdown` called only
  `Router::shutdown`; iroh-blobs spawns the blob store on its own
  tokio runtime whose `Actor` only exits when `Store::shutdown`
  is invoked, so the redb `blobs.db` file lock persisted across
  profile switches. The follow-up unlock hit `FsStore::load` on
  a locked path and hung. Fixed by calling
  `node.store.shutdown().await` after `Router::shutdown` in
  `ContentNode::shutdown` (no fork — `Store::shutdown` is the
  public iroh-blobs 0.98 API). Lock → unlock cycle now works.

**Tests:** 674 backend unit + 39 integration pass; 16 new settings
module tests; `vue-tsc -b --noEmit` clean.

### Multi-user profiles (branch `feat/multi-user-profiles`)

One device can now host any number of fully-isolated learner profiles —
the headline change for shared-device contexts (households, classrooms,
internet cafés). Backend, frontend, and migration land together.

**Data layout** — `<app_data>/profiles/<uuid>/{vault, alexandria.db,
iroh, plugins, videocache}` instead of single-vault-per-device. Each
profile owns its own SQLCipher key, iroh node secret, libp2p peer id,
and plugin directory. A public sidecar `profiles_index.json` (display
names + avatars only — no crypto material) lets the picker render
before any vault is unlocked.

**Backend** (`src-tauri/src/`):
- New `profile/{mod, index, manager, migration}.rs` — ProfileManager
  (list / create / rename / delete / touch), public sidecar index,
  and an idempotent first-launch auto-migrator that atomically moves
  the legacy single-vault layout into `profiles/<new-uuid>/` named
  "My Profile". 18 unit tests on tempfile fixtures.
- `AppState` refactored: vault/db/plugin/videocache/iroh paths become
  methods that read from the active profile. New
  `start_active_profile` / `stop_active_profile` lifecycle methods
  bring per-profile services up and down on every switch. The
  singleton iroh `ContentNode` is repointable via new `set_data_dir` /
  `clear_content_key` methods so the 20+ existing
  `&state.content_node` call sites stay unchanged.
- New IPC commands in `commands/profile.rs`: `list_profiles`,
  `get_active_profile_id`, `create_profile`,
  `restore_profile_with_mnemonic`, `unlock_profile`, `lock_profile`,
  `rename_profile`, `set_profile_avatar`, `delete_profile` (delete
  re-verifies the profile's vault password).
- Removed identity commands superseded by the profile lifecycle:
  `check_vault_exists`, `unlock_vault`, `generate_wallet`,
  `restore_wallet`, `lock_vault`, `reset_local_wallet`. Remaining
  identity commands (`export_mnemonic`, `is_biometric_available`,
  `get_wallet_info`, `get_local_did`, `get_profile`, `update_profile`,
  `publish_profile`, `resolve_profile`) operate on the active profile.
- `tauri.conf.json` asset scope broadened from `$APPDATA/videocache/**`
  to also include `$APPDATA/profiles/**/videocache/**` so the
  webview's `<video>` element still resolves materialized blobs after
  migration.
- 658 backend unit tests + 39 integration tests pass; `cargo fmt`
  clean; clippy clean.

**Frontend** (`src/`):
- New canonical composable `composables/useProfiles.ts` —
  `profiles`, `activeProfile`, `unlock/lock/create/rename/delete`,
  `setAvatar`. `composables/useAuth.ts` becomes a thin shim over it;
  removed lifecycle methods throw on call so any stale invocation
  surfaces loudly.
- New `pages/ProfileSelect.vue` — avatar grid picker with slide-in
  password panel and `Esc` to back out. Composed from new
  `components/profile/{ProfileTile, AddProfileTile, ProfileAvatar}.vue`.
- `pages/Onboarding.vue` collects a display name in addition to the
  password and calls `createProfile` / `restoreProfileWithMnemonic`.
- `pages/Unlock.vue` deleted; `/unlock` is now a router redirect to
  `/profiles` so any cached deep link still routes somewhere sane.
- `App.vue` initial routing maps `onboarding | picker | ready` to
  `/onboarding` / `/profiles` / no-op.
- `AppTopBar` avatar pill renders the active profile's color +
  emoji; dropdown gets **Switch user** and **Lock profile** entries;
  `Cmd/Ctrl + Shift + U` is registered as the `switch-profile`
  shortcut.
- New types in `src/types/index.ts`: `Avatar`, `ProfileSummary`,
  `CreateProfileResponse`, `UnlockProfileResponse`.
- `vue-tsc -b --noEmit` clean.

**Out of scope** (deferred to follow-up RFCs):
- Push notifications — researched architecture (relay-mediated APNs +
  FCM + UnifiedPush + native WS) lives in
  `docs/push-notifications-rfc.md`.
- Deep linking.
- Quick-switch via biometric (would build on the per-profile vault).
- Cross-profile read-only blob deduplication.

See `docs/multi-user-profiles.md` for the full design.

### Sentinel — backend ML rewrite (PRs #166, #170, #171, #172)

- **#166** — Synthetic adversarial-prior data generator (`alex
  synth-sentinel`) + Python training kit (`tools/sentinel-train/`).
  Deterministic ChaCha20 generators for six attack archetypes
  (paste_macro, typing_bot_constant / _jitter, llm_paste_edit,
  remote_control, human_baseline). Per-label blake2b-512 golden
  hashes pin SYNTH_VERSION = "v1" against drift in CI.
- **#170** — Backend ML moved into the Rust crate. `tract-onnx` runs
  the frozen paste classifier (5 KB ONNX embedded via
  `include_bytes!`); `candle` trains + scores the per-user keystroke
  autoencoder + mouse-trajectory CNN. New `sentinel_ml` IPC surface
  (11 commands) + DAO-weights distribution hardening (three-layer
  re-verify, 256-node + 50 MiB caps, 5 s resolver timeout, atomic
  revert on kill-switch / blocklist). Migrations 044–047 add
  `ai_paste_anomaly`, weights columns on `sentinel_priors`,
  `sentinel_kill_switch`, `sentinel_weights_blocklist`,
  `sentinel_user_models`. 637 backend tests pass (+28 sentinel module
  tests).
- **#171** — `useSentinel.ts` strips every JS ML class and dispatches
  to the new backend IPCs; module-scope refs source from the backend
  on session start. New `/dashboard/sentinel/cheat-test` page drives
  the bundled tract classifier with six synthetic streams. Wizard
  gains a welcome explainer (per-user models vs always-on classifier),
  per-step skip buttons that preserve existing models, and a
  retrained/kept-existing review badge. Mobile gate retired —
  pure-Rust ML runs on iOS WKWebView + Android WebView with no CSP /
  WASM constraints. `onnxruntime-web`, `vite-plugin-static-copy`,
  and `'wasm-unsafe-eval'` all removed. Mac DMG: 30 MB → 16 MB.
- **#172** — `docs/sentinel-runbook.md` (5 operator procedures + threat
  model + incident response template); post-rewrite doc sweep on
  `sentinel.md`, `sentinel-adversarial-priors.md`,
  `sentinel-federation.md`, `project-structure.md`, `architecture.md`.
  CI integrity step now verifies the backend `paste-v1.onnx.sha256`
  pin.

## [0.1.0-alpha] - 2026-04-14

### Added — VC-first credential migration (PRs 2–19)

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
| 14 | 576 | 0  | 24 | 14 |
| 15 | 576 | 0  | 24 | 14 |
| 16 | 579 | 0  | 25 | 13 |
| 17 | 579 | 0  | **38** | **0** |
| 18 | 579 | 0  | 38 | 0  |
| 19a | 579 | 0  | 38 | 0  |
| 19b | 583 | 0  | 38 | 0  |
| 19c | 587 | 0  | 39 | 0  |
| 19d | **589** | 0  | **40** | **0** |

- **PR 14** — Full rewrite of `Credentials.vue` against the new
  VC-first backend: list/issue/verify/revoke/export/present flows,
  new `CredentialDetail.vue`, `useCredentials` composable over
  every VC IPC command, and 14 new domain types in
  `src/types/index.ts`. Drops the legacy skill-proof + NFT-mint UI.
- **PR 15** — `alex credentials {list, get, export, verify}` CLI
  subcommand. Delegates to `app_lib::commands::credentials::*_impl`
  so GUI + CLI share one source of truth. 5 clap-derive parsing
  tests.
- **PR 16** — Real Cardano anchor metadata tx builder +
  `tick` processor with exponential-backoff retries (capped at
  5 attempts / 60 min). Auto-enqueue on `issue_credential_impl`.
  Metadata label 1697 registered in `cardano::script_refs`.
- **PR 17** — Two-node libp2p fixture lifted into
  `tests/e2e_vc/common.rs` + all four VC gossip topics added to
  `ALL_TOPICS` + 5-tier pinning eviction + per-tier byte accounting
  in `quota_breakdown_impl` + migration 28 for
  `credentials_pending_verification` + 13 new e2e test bodies.
  E2E ignored count: **14 → 0**.
- **PR 18** — Preprod anchor submission validated end-to-end
  against a funded testnet wallet. Exposed and fixed a real bug
  in `cardano::types::UTxO` deserialization: Blockfrost's live
  API returns BOTH `tx_index` and `output_index` on each UTxO,
  and `#[serde(alias)]` rejects with a duplicate-field error on
  both-present. Replaced with a manual `Deserialize` via a
  shadow struct (prefers `tx_index`, falls back to `output_index`).
  Live tx: `0e5ee75…93dd9f25` on preprod, metadata under label
  1697 confirmed via Blockfrost.
- **PR 19a** — Two pre-existing clippy-on-tests violations
  fixed: a helper (`get_course_by_id`) that landed inside the
  test module in `commands/courses.rs` is hoisted above the
  `mod tests`, and a `len() >= 1` check in
  `commands/reputation.rs` is rewritten to `!is_empty()`. No
  behaviour changes — pure clippy hygiene against
  `cargo clippy --tests`.
- **PR 19b** — Implements §11.3 suspension and §11.4 supersession.
  New `credentials.suspended`, `suspended_at`, `suspended_until`,
  `suspension_reason` columns (migration 29). New IPC
  `suspend_credential` and `reinstate_credential`. Verifier
  treats suspended credentials as not-currently-valid (distinct
  from `revoked` which is permanent). `IssueCredentialRequest`
  gains an optional `supersedes` field that enforces the §11.4
  invariant (issuer + subject + claim_kind + skill_id must
  match). Promotes the inbound-pending queue
  (`credentials_pending_verification`, migration 28) to a
  first-class table.
- **PR 19c** — Subject-controlled allowlist for the pull-based
  fetch protocol. New `credential_allowlist` table (migration 30)
  + `allow_credential_fetch` / `disallow_credential_fetch` IPCs.
  The literal string `"public"` in `requestor_did` makes a
  credential world-fetchable. Fetch handler now consults the
  allowlist alongside the existing subject-self check.
- **PR 19d** — Wires `/alexandria/vc-fetch/1.0` end-to-end.
  Adds `request-response` + `cbor` to libp2p features; the
  `Behaviour` now includes a `request_response::cbor::Behaviour`
  for the protocol. New `start_node_with_db` variant lets the
  swarm event loop synchronously consult a `Database` to answer
  inbound fetch requests; `P2pNode::fetch_credential(peer_id, req)`
  is the outbound counterpart. Adds an integration test
  (`tests/e2e_vc/p2p_vc_fetch.rs::two_node_round_trip_over_vc_fetch_protocol`)
  that boots two real libp2p nodes, fires a fetch over the wire,
  and asserts the deserialised response.

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
