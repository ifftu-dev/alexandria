# Project Structure

> Alexandria — Tauri v2 desktop and mobile application (macOS, iOS, Android)

---

## Top Level

```
alexandria/
├── Cargo.toml              # Workspace root (members: src-tauri, cli)
├── package.json            # npm scripts (dev, build, preview)
├── vite.config.ts          # Vite + Vue + Tailwind plugins
├── tsconfig.json           # TypeScript project references
├── index.html              # Vite entry point (FOUC prevention script)
│
├── src-tauri/              # Rust backend (Tauri v2 app)
├── src/                    # Vue 3 frontend
├── cli/                    # Developer CLI (alex)
├── patches/                # Local crate patches (if-watch iOS/Android fix)
├── docs/                   # Documentation
├── bootstrap/              # Seed data (public_courses.json)
└── scripts/                # Build/dev scripts
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
    ├── lib.rs              # Tauri setup, startup tasks, 194 command registrations
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
    │   ├── identity.rs     # Wallet, vault, profile lifecycle
    │   ├── credentials.rs  # VC issue/list/verify/revoke/suspend/export/allowlist
    │   ├── sync.rs         # Cross-device sync
    │   ├── courses.rs      # Course/tutorial CRUD and publish flows
    │   ├── attestation.rs  # Multi-party attestation
    │   ├── challenge.rs    # Evidence challenges and voting
    │   ├── opinions.rs     # Field Commentary opinions
    │   ├── integrity.rs    # Sentinel sessions and snapshots
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
    │   └── cardano.rs      # NFT minting + course registration helpers
    │
    ├── crypto/             # BIP-39 wallet, keystore, Ed25519, did:key
    ├── db/                 # SQLite, migrations, seed data
    ├── domain/             # Core types and VC domain models
    ├── aggregation/        # Trust aggregation / anti-gaming pipeline
    ├── evidence/           # Reputation, attestation, challenge logic
    ├── ipfs/               # iroh node + resolver + gateway fallback
    │
    ├── p2p/                # libp2p network stack
    │   ├── network.rs      # Swarm, relay bootstrap, event loop
    │   ├── types.rs        # 11 gossip topics + shared message types
    │   ├── gossip.rs       # Typed publish helpers
    │   ├── signing.rs      # Gossip envelope signing/verification
    │   ├── validation.rs   # Freshness/schema/authority checks
    │   ├── scoring.rs      # Per-topic GossipSub peer scoring
    │   ├── discovery.rs    # Relay bootstrap + namespace discovery
    │   ├── catalog.rs      # Catalog topic handler
    │   ├── evidence.rs     # Evidence topic handler
    │   ├── taxonomy.rs     # Taxonomy topic handler
    │   ├── governance.rs   # Governance topic handler
    │   ├── opinions.rs     # Opinions topic handler
    │   ├── sync.rs         # Cross-device sync
    │   ├── vc_did.rs       # DID doc + key rotation gossip
    │   ├── vc_status.rs    # Status-list snapshots/deltas
    │   ├── vc_fetch.rs     # `/alexandria/vc-fetch/1.0` pull protocol
    │   ├── presentation.rs # Inbound presentation parse/accept path
    │   ├── pinboard.rs     # PinBoard commitment observations
    │   ├── archive.rs      # Replay/archive helpers
    │   └── stress.rs       # High-volume P2P stress tests
    │
    ├── classroom/          # Classroom manager + gossip/types
    ├── tutoring/           # iroh-live integration
    │   ├── mod.rs
    │   ├── manager.rs      # Desktop + Android tutoring manager
    │   └── manager_mobile.rs # iOS/mobile tutoring manager
    │
    └── cardano/            # Cardano integration
        ├── blockfrost.rs   # REST client (preprod)
        ├── tx_builder.rs   # Native-script/NFT tx building
        ├── gov_tx_builder.rs # Governance tx builders
        ├── soulbound_tx_builder.rs # Soulbound/reputation tx builder path
        ├── snapshot.rs     # Asset names, datum encoding, metadata
        ├── governance.rs   # Metadata labels and payloads
        ├── onchain_queue.rs  # Persistent governance tx queue
        ├── anchor_queue.rs   # VC integrity-anchor queue
        ├── anchor_tx.rs      # Metadata-only anchor transactions
        └── script_refs.rs    # Reference-script hashes/UTXOs (currently DEPLOY_PENDING in-tree)
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
├── composables/            # 14 shared singletons
│   ├── useAuth.ts
│   ├── useBiometricVault.ts
│   ├── useClassroom.ts
│   ├── useContentSync.ts
│   ├── useCredentials.ts
│   ├── useLocalApi.ts
│   ├── useOmniSearch.ts
│   ├── useP2P.ts
│   ├── usePlatform.ts
│   ├── useSentinel.ts
│   ├── useSkillGraphHover.ts
│   ├── useSkillGraphState.ts
│   ├── useTheme.ts
│   └── useTutoringRoom.ts
│
├── components/             # 34 Vue components across feature folders
│   ├── ui/                 # Barrel-exported primitives (12 total)
│   ├── auth/               # Onboarding/unlock visuals
│   ├── course/             # Content renderers + quiz widgets
│   ├── governance/         # Governance badges/gates/countdowns
│   ├── integrity/          # Sentinel training/calibration UI
│   ├── layout/             # Sidebar, top bar, bottom bar, PiP, ticker, modal shell
│   ├── omni/               # Omni search surface
│   └── skills/             # Skill graph
│
├── layouts/
│   ├── AppLayout.vue       # Sidebar + content area
│   └── BlankLayout.vue     # Full-screen (onboarding, unlock)
│
├── pages/                  # 30 route views
│   ├── Home.vue
│   ├── Onboarding.vue
│   ├── Unlock.vue
│   ├── classrooms/
│   │   ├── Classroom.vue
│   │   ├── Index.vue
│   │   ├── JoinRequests.vue
│   │   └── Settings.vue
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
│   │   ├── Settings.vue
│   │   └── Sync.vue
│   ├── governance/
│   │   ├── DaoDetail.vue
│   │   └── Index.vue
│   ├── instructor/
│   │   ├── CourseEdit.vue
│   │   ├── CourseNew.vue
│   │   └── TutorialNew.vue
│   ├── learn/
│   │   └── Player.vue
│   ├── opinions/
│   │   ├── Detail.vue
│   │   ├── Index.vue
│   │   └── New.vue
│   ├── skills/
│   │   ├── Detail.vue
│   │   └── Index.vue
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
    └── sentinel/           # Client-side ML models
        ├── keystroke-autoencoder.ts
        ├── mouse-trajectory-cnn.ts
        └── face-embedder.ts
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
        ├── db.rs           # db status/reset
        ├── build.rs        # build check/release
        ├── config.rs       # config show/path
        ├── health.rs       # process + data health check
        └── clean.rs        # clean build/data/all
```
