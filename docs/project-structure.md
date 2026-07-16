# Project Structure

> Alexandria — Tauri v2 desktop and mobile application (macOS, iOS, Android)

---

## Top Level

```
alexandria/
├── Cargo.toml              # Workspace root (members: src-tauri, cli, crates/*)
├── package.json            # npm scripts (dev, build, preview)
├── vite.config.ts          # Vite + Vue + Tailwind plugins
├── tsconfig.json           # TypeScript project references
├── index.html              # Vite entry point (FOUC prevention script)
│
├── src-tauri/              # Rust backend (Tauri v2 app)
├── src/                    # Vue 3 frontend
├── cli/                    # Developer CLI (alex)
├── crates/                 # Workspace member crates (iroh-live, iroh-moq, moq-media)
├── patches/                # Local crate patches (netdev, if-watch, audiopus_sys, webrtc-audio-processing-sys, ffmpeg-sys-next, ffmpeg-next)
├── docs/                   # Documentation
├── bootstrap/              # Seed data (public_courses.json)
└── scripts/                # Build/dev scripts (incl. check-tauri-commands.mjs: CI guard that every registered command has a frontend caller or is allowlisted)
```

---

## Rust Backend (`src-tauri/`)

This is a guided map, not a byte-for-byte manifest. `commands/`, `p2p/`,
and the platform-gated tutoring/cardano code are the fastest-moving
areas, so the filesystem is the final source of truth.

```
src-tauri/
├── Cargo.toml              # alexandria-node crate
├── tauri.conf.json         # Tauri window, build, bundle config
├── capabilities/
│   └── default.json        # IPC permissions
├── Info.plist              # macOS permissions (camera, microphone)
│
└── src/
    ├── main.rs             # Thin binary entry point
    ├── lib.rs              # Tauri setup, startup tasks, ~300 command registrations
    ├── diag.rs             # File-based diagnostic logger + panic hook
    │
    ├── commands/           # Domain-oriented Tauri IPC handlers
    │   ├── mod.rs          # Module exports + platform gating
    │   ├── ratelimit.rs    # Shared helper (not exposed as IPC)
    │   ├── classroom.rs    # Classrooms, members, channels, messages, calls
    │   ├── governance.rs   # DAOs, proposals, elections, votes
    │   ├── tutoring.rs     # Desktop tutoring/session commands
    │   ├── tutoring_mobile.rs # Mobile tutoring command variant
    │   ├── tutoring_stubs.rs  # Stubbed tutoring surface for unsupported builds
    │   ├── taxonomy.rs     # Subject fields, subjects, skills, taxonomy graph
    │   ├── users.rs        # Public profile fetch + name resolution
    │   ├── username_registry.rs # DHT username claims (claim/resolve/availability)
    │   ├── profile.rs      # Multi-user profile lifecycle: list/create/unlock/lock/rename/avatar/delete + restore-from-mnemonic
    │   ├── settings.rs     # Per-profile settings IPC: list_settings, set_setting, reset_setting (typed registry)
    │   ├── identity.rs     # Active-profile identity ops: export_mnemonic, get_profile, update_profile, publish_profile, resolve_profile, get_wallet_info, get_local_did
    │   ├── credentials.rs  # VC issue/list/verify/revoke/suspend/export/allowlist
    │   ├── sync.rs         # Cross-device sync
    │   ├── courses.rs      # Course/tutorial CRUD and publish flows
    │   ├── attestation.rs  # Multi-party attestation
    │   ├── challenge.rs    # Evidence challenges and voting
    │   ├── opinions.rs     # Field Commentary opinions
    │   ├── integrity.rs    # Sentinel sessions and snapshots
    │   ├── assessment.rs   # Dynamic Sentinel-gated assessments: start attempt, host-side grade
    │   ├── goal_templates.rs # Resolve learner goals → skill graph; list/get DAO goal templates
    │   ├── skill_bootstrap.rs # Bootstrap skill graph from uploaded resume/transcript
    │   ├── content_governance.rs # DAO propose/publish for goal templates + question banks
    │   ├── role_assessment.rs # Enterprise sponsor role/JD assessments
    │   ├── content.rs      # iroh blob operations
    │   ├── pinning.rs      # PinBoard commitments
    │   ├── storage.rs      # Quota and cache management
    │   ├── snapshot.rs     # Reputation snapshot + soulbound entry points
    │   ├── reputation.rs   # Reputation assertions and impact
    │   ├── enrollment.rs   # Enrollment and progress
    │   ├── elements.rs     # Course element CRUD
    │   ├── chapters.rs     # Chapter CRUD
    │   ├── catalog.rs      # Search, browse, bootstrap, hydrate
    │   ├── p2p.rs          # Network status, peers, diagnostics
    │   ├── evidence.rs     # Evidence submission/query
    │   ├── aggregation.rs  # Derived skill-state queries
    │   ├── presentation.rs # Selective-disclosure presentation create/verify
    │   ├── health.rs       # Health check + diag log access
    │   └── graph.rs      # Skill-graph fetch + learning-path helpers
    │
    ├── crypto/             # BIP-39 wallet, keystore, Ed25519, did:key
    ├── db/                 # SQLite, migrations, seed data
    ├── domain/             # Core types and VC domain models
    ├── aggregation/        # Trust aggregation / anti-gaming pipeline (provenance-weighted)
    ├── evidence/           # Reputation, challenge, taxonomy, thresholds logic
    ├── goals/              # Goal → skill-graph resolver + on-device JD/resume parser
    │   ├── mod.rs
    │   └── jd_parser.rs    # Pure n-gram matcher over skill names + synonyms
    ├── assessment/         # Dynamic assessment engine (pure)
    │   ├── mod.rs          # SplitMix64 PRNG + option shuffle
    │   ├── randomizer.rs   # Difficulty-stratified question draw (per-attempt seed)
    │   └── grader.rs       # Host-side grading against the withheld answer key
    ├── ipfs/               # iroh node + resolver + gateway fallback
    │
    ├── profile/            # Multi-user profile manager
    │   ├── mod.rs          # Module exports
    │   ├── index.rs        # profiles_index.json sidecar (public — names/avatars only)
    │   ├── manager.rs      # ProfileManager: list/create/rename/delete/touch + ProfilePaths
    │   └── migration.rs    # First-launch auto-migrator from legacy single-vault layout
    │
    ├── settings/           # Unified per-profile settings (sync + device scope)
    │   ├── mod.rs          # Module exports
    │   ├── registry.rs     # Typed SettingKey<T> registry — single source of truth for valid keys + defaults
    │   └── store.rs        # SettingsStore: get/set/reset/list_all/list_syncable/apply_sync_row
    │
    ├── p2p/                # libp2p network stack
    │   ├── network.rs      # Swarm, relay bootstrap, event loop
    │   ├── types.rs        # 13 gossip topics + shared message types
    │   ├── gossip.rs       # Typed publish helpers
    │   ├── signing.rs      # Gossip envelope signing/verification
    │   ├── validation.rs   # Signature, identity (via registry), freshness, dedup, schema, authority
    │   ├── registry.rs     # stake_pubkey_registry lookups + bootstrap snapshot loader
    │   ├── registry_chain.rs # Background refresh of registry from on-chain registrations
    │   ├── scoring.rs      # Per-topic GossipSub peer scoring (12 scored topics)
    │   ├── discovery.rs    # Relay bootstrap + namespace discovery
    │   ├── catalog.rs      # Catalog topic handler
    │   ├── taxonomy.rs     # Taxonomy topic handler
    │   ├── governance.rs   # Governance topic handler
    │   ├── opinions.rs     # Opinions topic handler
    │   ├── sync.rs         # Cross-device sync
    │   ├── vc_did.rs       # DID doc + key rotation gossip
    │   ├── vc_status.rs    # Status-list snapshots/deltas
    │   ├── vc_fetch.rs     # `/alexandria/vc-fetch/1.0` pull protocol
    │   ├── graph_fetch.rs  # Public skill-graph fetch protocol
    │   ├── profile_fetch.rs # Public profile fetch protocol
    │   ├── username_reg.rs # Relay receipt client + verification
    │   ├── presentation.rs # Inbound presentation parse/accept path
    │   ├── pinboard.rs     # PinBoard commitment observations
    │   ├── archive.rs      # Replay/archive helpers
    │   └── stress.rs       # High-volume P2P stress tests
    │
    ├── classroom/          # Classroom manager + gossip/types
    ├── sentinel/           # Backend Sentinel ML (tract + candle)
    │   ├── types.rs        # KeystrokeEvent / MousePoint / DigraphFeatures
    │   ├── features.rs     # 12-dim windowed feature extractor (paste classifier)
    │   ├── paste_classifier.rs  # tract ONNX inference; embedded paste-v1.onnx
    │   ├── keystroke_ae.rs # Per-user autoencoder (candle autograd)
    │   └── mouse_cnn.rs    # Reservoir-style trajectory CNN (candle dense head)
    │
    ├── tutoring/           # iroh-live integration
    │   ├── mod.rs
    │   ├── manager.rs      # Desktop + Android tutoring manager
    │   └── manager_mobile.rs # iOS/mobile tutoring manager
    │
    └── cardano/            # Cardano integration
        ├── blockfrost.rs   # REST client (preprod)
        ├── tx_builder.rs   # Native-script/NFT tx building
        ├── username_anchor.rs # Batched label-1698 username-claim anchoring
        ├── gov_tx_builder.rs # Governance tx builders
        ├── soulbound_tx_builder.rs # Soulbound/reputation tx builder path
        ├── snapshot.rs     # Asset names, datum encoding, metadata
        ├── governance.rs   # Metadata labels and payloads
        ├── onchain_queue.rs  # Persistent governance tx queue
        ├── anchor_queue.rs   # VC integrity-anchor queue
        ├── anchor_tx.rs      # Metadata-only anchor transactions
        └── script_refs.rs    # Reference-script hashes/UTXOs (deployed to preprod, block 4736927)
```

---

## Vue Frontend (`src/`)

This section is exhaustive for route views and composables, but only
representative for components. UI components move around more often than
feature routes do.

```
src/
├── main.ts                 # Vue app entry, router, CSS import
├── App.vue                 # Root component (auth init, theme init, window show)
│
├── assets/
│   ├── css/
│   │   └── main.css        # Tailwind CSS v4 + design tokens
│   ├── fonts.css           # @font-face declarations
│   └── fonts/
│       ├── Inter.woff2
│       └── JetBrainsMono.woff2
│
├── composables/            # Shared singletons
│   ├── useProfiles.ts      # Canonical multi-user surface (list/unlock/lock/create/rename/delete/avatar) + onProfileReady / onProfileLocked fan-out hooks
│   ├── useSettings.ts      # Reactive mirror of the per-profile settings registry; `useSetting<T>(key)` two-way ref
│   ├── useGraphPrefs.ts    # Skill-graph visibility prefs
│   ├── useAuth.ts          # Compat shim over useProfiles — removed lifecycle methods throw
│   ├── useBiometricVault.ts
│   ├── useClassroom.ts
│   ├── useContentSync.ts
│   ├── useCredentials.ts
│   ├── useKeyboardShortcuts.ts # Includes the `switch-profile` shortcut (Cmd/Ctrl+Shift+U); bindings persisted via settings store
│   ├── useLocalApi.ts
│   ├── useOmniSearch.ts    # Recents synced via `ui.omni_recents`
│   ├── useP2P.ts
│   ├── usePlatform.ts
│   ├── useSentinel.ts      # AI / paste-classifier toggles synced via `sentinel.*` settings
│   ├── useSettingsModal.ts # Type-only now: the `SettingsSectionId` union (settings is a full page, not a modal)
│   ├── useDisplayNames.ts  # DID→username resolver (cached) — usernames shown app-wide instead of DIDs
│   ├── useSkillGraphHover.ts
│   ├── useSkillGraphState.ts
│   ├── useTheme.ts         # Theme bound to `ui.theme` via `useSetting<string>` — reacts to sync deliveries
│   └── useTutoringRoom.ts
│
├── components/             # Vue components across feature folders
│   ├── ui/                 # Barrel-exported primitives
│   ├── auth/               # Onboarding visuals (Starfield, etc.)
│   ├── profile/            # Multi-user picker tiles + avatar widget
│   │   ├── ProfileTile.vue       # Picker tile for an existing profile
│   │   ├── AddProfileTile.vue    # "+" tile that routes to /onboarding
│   │   └── ProfileAvatar.vue     # Emoji / identicon / image avatar
│   ├── course/             # Content renderers + quiz widgets
│   ├── governance/         # Governance badges/gates/countdowns
│   ├── integrity/          # Sentinel training/calibration UI
│   ├── layout/             # Sidebar, top bar (avatar dropdown), bottom bar, PiP, ticker, modal shell
│   ├── omni/               # Omni search surface
│   ├── settings/           # Settings-page panels (AdvancedSettingsPanel, PluginsPanel)
│   ├── goals/              # GoalPicker (exam/curriculum/job-role/JD tabs + confirm suggestions)
│   └── skills/             # Skill graph + SkillBootstrapPanel (resume/transcript upload)
│
├── layouts/
│   ├── AppLayout.vue       # Sidebar + content area
│   └── BlankLayout.vue     # Full-screen (profile picker, onboarding)
│
├── pages/                  # Route views
│   ├── Home.vue
│   ├── ProfileSelect.vue   # Multi-user picker (`/profiles`)
│   ├── Onboarding.vue      # New-profile creation / mnemonic restore
│   ├── classrooms/
│   │   ├── Classroom.vue
│   │   ├── Index.vue
│   │   ├── JoinRequests.vue
│   │   └── Settings.vue
│   ├── ProfileMe.vue        # Own profile page (/profile)
│   ├── goals/               # Learning goals + path view (/goals)
│   │   └── Index.vue
│   ├── u/                   # Public user profiles (/u/:id)
│   ├── courses/
│   │   ├── Detail.vue
│   │   └── Index.vue
│   ├── dashboard/
│   │   ├── Courses.vue
│   │   ├── CredentialDetail.vue
│   │   ├── Credentials.vue
│   │   ├── Network.vue
│   │   ├── Reputation.vue
│   │   ├── Sentinel.vue
│   │   └── Sync.vue
│   ├── governance/
│   │   ├── DaoDetail.vue
│   │   └── Index.vue
│   ├── instructor/
│   │   ├── Composer.vue
│   │   ├── CourseLearners.vue
│   │   ├── Dashboard.vue
│   │   ├── Inbox.vue
│   │   ├── MyCourses.vue
│   │   └── SubmissionReview.vue
│   ├── learn/
│   │   ├── Player.vue
│   │   └── AssessmentRunner.vue # Sentinel-gated dynamic assessment (/assessment/:skillId)
│   ├── opinions/
│   │   ├── Detail.vue
│   │   ├── Index.vue
│   │   └── New.vue
│   ├── skills/
│   │   ├── Detail.vue
│   │   ├── Index.vue
│   │   └── BootstrapUpload.vue # Resume/transcript skill bootstrap (/skills/bootstrap)
│   └── tutoring/
│       ├── Index.vue
│       └── Session.vue
│
├── router/
│   └── index.ts            # All routes with layout meta
│
├── types/
│   └── index.ts            # All TypeScript types
│
└── utils/
    ├── sanitize.ts         # DOMPurify wrappers for HTML and SVG sanitization
    └── sentinel/           # Frontend Sentinel helpers
        └── face-embedder.ts  # LBP face embedding (pure pixel math)
                              # Keystroke AE, mouse CNN, paste classifier
                              # all moved to the backend Rust crate -- see
                              # src-tauri/src/sentinel/ and docs/sentinel.md
```

---

## Developer CLI (`cli/`)

```
cli/
├── Cargo.toml              # alex binary crate
└── src/
    ├── main.rs             # clap CLI entry point
    ├── context.rs          # Project root detection, app data paths
    ├── output.rs           # Colored terminal output (owo-colors)
    ├── runner.rs           # Command execution, spinners (indicatif)
    └── commands/
        ├── mod.rs
        ├── dev.rs          # dev run/check/test/clippy/fmt/all
        ├── run.rs          # Run on desktop / iOS / Android (device + emulator selection)
        ├── db.rs           # db status/reset
        ├── build.rs        # build check/release
        ├── config.rs       # config show/path
        ├── credentials.rs  # List/show/verify local credentials + survivability bundle export
        ├── synth_sentinel.rs # Generate synthetic Sentinel training/holdout data blobs
        ├── health.rs       # process + data health check
        └── clean.rs        # clean build/data/all
```
