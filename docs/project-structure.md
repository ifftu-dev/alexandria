# Project Structure

> Alexandria (Mark 3) — Tauri v2 desktop and mobile application (macOS, iOS, Android)

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

```
src-tauri/
├── Cargo.toml              # alexandria-node crate
├── tauri.conf.json         # Tauri window, build, bundle config
├── capabilities/
│   └── default.json        # IPC permissions (core:default, core:window:allow-show)
├── Info.plist              # macOS permissions (camera, microphone for Sentinel)
│
└── src/
    ├── main.rs             # Binary entry point (calls app_lib::run)
    ├── lib.rs              # Tauri setup, registers ~160 IPC commands, startup tasks
    ├── diag.rs             # File-based diagnostic logger + panic hook (iOS/desktop)
    │
    ├── commands/           # IPC command handlers (frontend ↔ backend)
    │   ├── mod.rs          # Re-exports all command modules
    │   ├── identity.rs     # 11 cmds — wallet, vault, profile
    │   ├── governance.rs   # 18 cmds — DAOs, proposals, elections, votes
    │   ├── taxonomy.rs     # 14 cmds — skills, subjects, taxonomy graph
    │   ├── courses.rs      #  7 cmds — CRUD, publishing
    │   ├── attestation.rs  #  8 cmds — multi-party attestation
    │   ├── challenge.rs    #  7 cmds — evidence challenges, voting
    │   ├── content.rs      #  6 cmds — iroh blob operations
    │   ├── integrity.rs    #  6 cmds — Sentinel sessions, snapshots
    │   ├── sync.rs         #  8 cmds — cross-device sync
    │   ├── p2p.rs          #  4 cmds — network status, peers
    │   ├── enrollment.rs   #  4 cmds — enroll, progress
    │   ├── reputation.rs   #  4 cmds — assertions, impact
    │   ├── snapshot.rs     #  4 cmds — CIP-68 reputation anchoring
    │   ├── chapters.rs     #  4 cmds — chapter CRUD
    │   ├── elements.rs     #  4 cmds — element CRUD
    │   ├── evidence.rs     #  3 cmds — submit, query, broadcast
    │   ├── classroom.rs    # 26 cmds — classrooms, members, channels, messages, calls
    │   ├── tutoring.rs     # 14 cmds — rooms, video/audio toggle, chat (desktop)
    │   ├── tutoring_stubs.rs # 14 cmds — mobile stubs (not-yet-supported errors)
    │   ├── catalog.rs      #  4 cmds — search, browse, bootstrap, hydrate
    │   ├── cardano.rs      #  2 cmds — UTxOs, tx submit
    │   └── health.rs       #  2 cmds — health check, diag log
    │
    ├── crypto/             # Cryptographic primitives
    │   ├── mod.rs
    │   ├── wallet.rs       # BIP-39, CIP-1852, pallas key derivation
    │   ├── keystore.rs     # IOTA Stronghold vault — desktop (#[cfg(desktop)])
    │   ├── keystore_portable.rs  # AES-256-GCM + Argon2id vault — iOS/Android (#[cfg(mobile)])
    │   ├── signing.rs      # Ed25519 sign/verify
    │   └── hash.rs         # Blake2b-256, SHA-256, entity_id
    │
    ├── db/                 # Database layer
    │   ├── mod.rs          # Database struct, migration runner
    │   ├── schema.rs       # 16 migrations, 50 tables (full DDL)
    │   ├── seed.rs         # Taxonomy, courses, governance seed data
    │   ├── seed_content.rs # Uses include!() to load seed_content_data.rs
    │   └── seed_content_data.rs  # HTML content + quiz JSON for all 82 seed elements
    │
    ├── domain/             # Business logic (pure, no I/O)
    │   ├── mod.rs
    │   ├── identity.rs     # Profile types
    │   ├── catalog.rs      # Catalog entry types
    │   ├── course.rs       # Course, chapter, element types
    │   ├── course_document.rs # Signed course document format
    │   ├── evidence.rs     # Evidence record types
    │   ├── enrollment.rs   # Enrollment types
    │   ├── governance.rs   # DAO, proposal, election types
    │   ├── profile.rs      # Signed profile document format
    │   ├── reputation.rs   # Assertion, impact delta types
    │   ├── sync.rs         # Sync message types
    │   ├── taxonomy.rs     # Skill, subject, taxonomy update types
    │   ├── challenge.rs    # Challenge, vote types
    │   ├── attestation.rs  # Attestation requirement types
    │   └── classroom.rs    # Classroom, member, channel, message, call types
    │
    ├── evidence/           # Evidence processing pipeline
    │   ├── mod.rs
    │   ├── aggregator.rs   # Weighted confidence aggregation → skill proofs
    │   ├── attestation.rs  # Multi-party attestation logic
    │   ├── challenge.rs    # Stake-based challenge resolution
    │   ├── reputation.rs   # Instructor impact computation
    │   ├── taxonomy.rs     # Bloom's level traversal, prerequisites
    │   └── thresholds.rs   # Proof confidence thresholds
    │
    ├── ipfs/               # Content-addressed storage
    │   ├── mod.rs
    │   ├── node.rs         # iroh node lifecycle (start, shutdown)
    │   ├── content.rs      # Blob operations (add, get, has)
    │   ├── resolver.rs     # Resolution chain (local → mapping → gateway)
    │   ├── gateway.rs      # IPFS gateway HTTP client (3 fallbacks)
    │   ├── cid.rs          # CID detection (BLAKE3, CIDv0, CIDv1)
    │   ├── course.rs       # Signed course document publish/resolve
    │   └── profile.rs      # Signed profile publish/resolve
    │
    ├── p2p/                # Peer-to-peer networking
    │   ├── mod.rs
    │   ├── network.rs      # libp2p swarm (7 protocols), relay logic, event loop
    │   ├── types.rs        # 6 topics, SignedGossipMessage, PeerExchangeMessage, events
    │   ├── gossip.rs       # High-level publish methods
    │   ├── signing.rs      # Gossip envelope signing/verification
    │   ├── validation.rs   # 6-step validation pipeline (signature, identity, freshness, dedup, schema, authority)
    │   ├── scoring.rs      # Per-topic GossipSub peer scoring
    │   ├── nat.rs          # AutoNAT configuration
    │   ├── discovery.rs    # Relay bootstrap addrs, namespace key, relay PeerId
    │   ├── catalog.rs      # Catalog topic handler
    │   ├── evidence.rs     # Evidence topic handler
    │   ├── taxonomy.rs     # Taxonomy topic handler (committee-gated)
    │   ├── governance.rs   # Governance topic handler
    │   ├── sync.rs         # Cross-device sync (encrypted, LWW + append-only)
    │   ├── rate_limit.rs   # Per-peer token-bucket gossip rate limiter
    │   └── stress.rs       # Stress tests (~1500 lines)
    │
    ├── classroom/          # Classroom feature
    │   ├── mod.rs
    │   ├── manager.rs      # ClassroomManager, message/meta handlers, authz gates
    │   ├── gossip.rs       # Per-classroom gossip publish (message + meta topics)
    │   └── types.rs        # ClassroomMessagePayload, ClassroomMetaEvent
    │
    ├── tutoring/           # Live tutoring (iroh-live integration)
    │   ├── mod.rs
    │   ├── manager.rs      # TutoringManager — desktop (video/audio/screenshare)
    │   ├── manager_mobile.rs  # Mobile variant (audio-only)
    │   └── manager_android.rs # Android variant
    │
    └── cardano/            # Cardano blockchain integration
        ├── mod.rs
        ├── blockfrost.rs   # Blockfrost REST client (preprod)
        ├── types.rs        # UTxO, protocol params, chain tip
        ├── policy.rs       # NativeScript policies, asset names
        ├── tx_builder.rs   # Conway-era tx building (pallas)
        ├── snapshot.rs     # CIP-68 soulbound reputation tokens
        └── governance.rs   # On-chain governance metadata
```

---

## Vue Frontend (`src/`)

```
src/
├── main.ts                 # Vue app entry, router, CSS import
├── App.vue                 # Root component (auth init, theme init, window show)
│
├── assets/
│   ├── css/
│   │   └── main.css        # Tailwind CSS v4 + design system (light/dark vars, components)
│   ├── fonts.css            # @font-face declarations for bundled fonts
│   └── fonts/
│       ├── Inter.woff2      # Inter variable font (400-700)
│       └── JetBrainsMono.woff2  # JetBrains Mono (400-500)
│
├── composables/            # Shared reactive state (12 composables)
│   ├── useAuth.ts          # Wallet/vault lifecycle, identity state
│   ├── useTheme.ts         # Theme toggle (light/dark/system), localStorage persistence
│   ├── useLocalApi.ts      # Tauri invoke wrapper
│   ├── useP2P.ts           # P2P status polling
│   ├── useSkillGraphState.ts # Module-level reactive singleton for shared skill graph state
│   ├── useSkillGraphHover.ts # Skill graph hover state
│   ├── useSentinel.ts      # Sentinel integrity sessions
│   ├── useContentSync.ts   # Content sync status and progress
│   ├── usePlatform.ts      # Platform detection (iOS, Android, macOS)
│   ├── useBiometricVault.ts # Biometric unlock with session timeout
│   ├── useClassroom.ts     # Classroom state, real-time message/meta listeners
│   └── useTutoringRoom.ts  # Tutoring session management
│
├── components/
    │   ├── ui/                 # Barrel-exported UI primitives (12 components)
    │   │   ├── index.ts
    │   │   ├── AppButton.vue
    │   │   ├── AppBadge.vue
    │   │   ├── AppModal.vue
    │   │   ├── AppAlert.vue
    │   │   ├── AppSpinner.vue
    │   │   ├── AppInput.vue
    │   │   ├── AppTextarea.vue
    │   │   ├── AppTabs.vue
    │   │   ├── EmptyState.vue
    │   │   ├── StatusBadge.vue
    │   │   ├── ConfirmDialog.vue
    │   │   └── DataRow.vue
    │   ├── auth/
    │   │   └── Starfield.vue   # 3-layer parallax SVG starfield (onboarding/unlock bg)
    │   ├── course/
    │   │   ├── CourseCard.vue  # Borderless shadow card with glassmorphism stats, hover lift
    │   │   ├── TextContent.vue # Rich HTML renderer
    │   │   ├── QuizEngine.vue  # Interactive quiz with scoring
    │   │   ├── McqQuestion.vue # Multiple-choice question component
    │   │   ├── EssayInput.vue  # Essay/long-form input component
    │   │   ├── PdfViewer.vue   # PDF element viewer
    │   │   └── VideoPlayer.vue # Video element player
    │   ├── skills/
    │   │   └── SkillGraph.vue  # Interactive skill prerequisite graph
    │   ├── integrity/
    │   │   └── SentinelTrainingWizard.vue  # 6-step integrity calibration wizard
    │   └── layout/
    │       ├── AppSidebar.vue       # Desktop sidebar — collapsible Live Tutoring/Classrooms previews, skill graph
    │       ├── AppTopBar.vue        # Top bar with Mark 2-style user menu dropdown (role badge, icon SVGs)
    │       ├── MobileTabBar.vue     # Bottom tab bar for mobile (iOS/Android), backdrop blur, active indicator
    │       ├── SidebarSkillGraph.vue # force-graph canvas widget — earned/available/locked skill nodes with glow
    │       └── TutoringPiP.vue      # Picture-in-picture call overlay
│
├── layouts/
│   ├── AppLayout.vue       # Sidebar + content area
│   └── BlankLayout.vue     # Full-screen (onboarding, unlock)
│
├── pages/                  # 26 route views
│   ├── Home.vue
│   ├── Onboarding.vue      # Multi-step wallet creation + mnemonic backup
│   ├── Unlock.vue          # Password entry + vault progress
│   ├── courses/
│   │   ├── Index.vue       # Course catalog
│   │   └── Detail.vue      # Course detail + enrollment
│   ├── instructor/
│   │   ├── CourseNew.vue   # Create course
│   │   └── CourseEdit.vue  # Edit course content
│   ├── learn/
│   │   └── Player.vue      # Content player (text, video, quiz)
│   ├── skills/
│   │   ├── Index.vue       # Skill taxonomy browser
│   │   └── Detail.vue      # Skill detail + prerequisites + proofs
│   ├── governance/
│   │   ├── Index.vue       # DAO list
│   │   └── DaoDetail.vue   # DAO detail + proposals + elections
│   ├── classrooms/
│   │   ├── Index.vue       # Classroom list (joined classrooms)
│   │   ├── Classroom.vue   # Channels, messages, active calls
│   │   ├── Settings.vue    # Role management, archive
│   │   └── JoinRequests.vue # Review pending join requests
│   ├── tutoring/
│   │   ├── Index.vue       # Tutoring sessions list
│   │   └── Session.vue     # Active video/audio session
│   └── dashboard/
│       ├── Courses.vue     # My enrolled courses
│       ├── Credentials.vue # Minted NFT credentials
│       ├── Reputation.vue  # Reputation assertions
│       ├── Network.vue     # P2P status, peers
│       ├── Sync.vue        # Cross-device sync
│       ├── Sentinel.vue    # Integrity dashboard + training wizard
│       └── Settings.vue    # Theme, profile, app config
│
├── router/
│   └── index.ts            # All routes with layout meta
│
├── types/
│   └── index.ts            # All TypeScript types
│
└── utils/
    ├── sanitize.ts          # DOMPurify wrappers for HTML and SVG sanitization
    └── sentinel/            # Client-side ML models
        ├── keystroke-autoencoder.ts  # 4→8→4→8→4 autoencoder
        ├── mouse-trajectory-cnn.ts   # Trajectory analysis CNN
        └── face-embedder.ts          # LBP face embedding
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
