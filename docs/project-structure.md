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
    ├── lib.rs              # Tauri setup, registers 194 IPC commands, startup tasks
    ├── diag.rs             # File-based diagnostic logger + panic hook (iOS/desktop)
    │
    ├── commands/           # IPC command handlers (30 source files; 26 register IPC; 194 cmds total)
    │   ├── mod.rs          # Re-exports all command modules
    │   ├── ratelimit.rs    # Internal helper (not registered as IPC)
    │   ├── classroom.rs    # 24 cmds — classrooms, members, channels, messages, calls
    │   ├── governance.rs   # 19 cmds — DAOs, proposals, elections, votes
    │   ├── tutoring.rs     # 15 cmds — rooms, video/audio toggle, chat (desktop)
    │   ├── tutoring_mobile.rs # platform variant of tutoring (mobile)
    │   ├── tutoring_stubs.rs  # platform variant of tutoring (not-yet-supported stubs)
    │   ├── taxonomy.rs     # 15 cmds — skills, subjects, taxonomy graph
    │   ├── identity.rs     # 13 cmds — wallet, vault, profile
    │   ├── credentials.rs  # 10 cmds — VC issue/verify/revoke/suspend/reinstate/allow/disallow/export/verify-bundle (PR 5, 12, 13, 16, 19b, 19c)
    │   ├── sync.rs         #  8 cmds — cross-device sync
    │   ├── courses.rs      #  8 cmds — CRUD, publishing
    │   ├── attestation.rs  #  8 cmds — multi-party attestation
    │   ├── challenge.rs    #  7 cmds — evidence challenges, voting
    │   ├── opinions.rs     #  6 cmds — Field Commentary opinions (mig 21)
    │   ├── integrity.rs    #  6 cmds — Sentinel sessions, snapshots
    │   ├── content.rs      #  6 cmds — iroh blob operations
    │   ├── pinning.rs      #  5 cmds — PinBoard commitment declare/revoke/list/quota (PR 10)
    │   ├── storage.rs      #  4 cmds — quota, cache prune, settings
    │   ├── snapshot.rs     #  4 cmds — CIP-68 reputation anchoring + soulbound minting
    │   ├── reputation.rs   #  4 cmds — assertions, impact
    │   ├── enrollment.rs   #  4 cmds — enroll, progress
    │   ├── elements.rs     #  4 cmds — element CRUD
    │   ├── chapters.rs     #  4 cmds — chapter CRUD
    │   ├── catalog.rs      #  4 cmds — search, browse, bootstrap, hydrate
    │   ├── p2p.rs          #  4 cmds — network status, peers
    │   ├── evidence.rs     #  3 cmds — submit, query, broadcast
    │   ├── aggregation.rs  #  3 cmds — get/list derived skill state, recompute_all (PR 13)
    │   ├── presentation.rs #  2 cmds — create/verify selective-disclosure presentation (PR 11)
    │   ├── health.rs       #  2 cmds — health check, diag log
    │   └── cardano.rs      #  2 cmds — NFT minting, course registration
    │
    ├── crypto/             # Cryptographic primitives
    │   ├── mod.rs
    │   ├── wallet.rs       # BIP-39, CIP-1852, pallas key derivation
    │   ├── keystore.rs     # IOTA Stronghold vault — desktop (#[cfg(desktop)])
    │   ├── keystore_portable.rs  # AES-256-GCM + Argon2id vault — iOS/Android (#[cfg(mobile)])
    │   ├── signing.rs      # Ed25519 sign/verify
    │   ├── did.rs          # did:key derivation/parsing/resolving + key_registry historical resolution (PR 3)
    │   └── hash.rs         # Blake2b-256, SHA-256, entity_id
    │
    ├── db/                 # Database layer
    │   ├── mod.rs          # Database struct, migration runner
    │   ├── schema.rs       # 30 migrations, 66 tables (full DDL)
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
    │   ├── opinions.rs     # Field Commentary opinion types
    │   ├── profile.rs      # Signed profile document format
    │   ├── reputation.rs   # Assertion, impact delta types
    │   ├── sync.rs         # Sync message types
    │   ├── taxonomy.rs     # Skill, subject, taxonomy update types
    │   ├── challenge.rs    # Challenge, vote types
    │   ├── attestation.rs  # Attestation requirement types
    │   ├── classroom.rs    # Classroom, member, channel, message, call types
    │   └── vc/             # Verifiable Credentials (PR 4)
    │       ├── mod.rs           # VerifiableCredential, Claim variants, Proof
    │       ├── canonicalize.rs  # JCS canonicalisation (RFC 8785) via serde_json_canonicalizer
    │       ├── context.rs       # W3C + Alexandria @context constants
    │       ├── sign.rs          # Ed25519Signature2020 detached JWS
    │       └── verify.rs        # Verification pipeline (§13.2 acceptance predicate)
    │
    ├── aggregation/        # §14 trust aggregation engine (PR 6 + 7)
    │   ├── mod.rs          # aggregate_skill_state — reproduces §26 worked example
    │   ├── weights.rs      # Weighted-mean confidence
    │   ├── level.rs        # Bloom-level mapping
    │   ├── independence.rs # Issuer-cluster independence
    │   ├── antigaming.rs   # §15 cluster cap + inflation z-score
    │   └── config.rs       # Tunable parameters
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
    │   ├── profile.rs      # Signed profile publish/resolve
    │   └── pinboard.rs     # PinBoard pinning declarations + 5-tier eviction (PR 10)
    │
    ├── p2p/                # Peer-to-peer networking
    │   ├── mod.rs
    │   ├── network.rs      # libp2p swarm (7+ protocols), relay logic, event loop, vc-fetch request-response (PR 19d)
    │   ├── types.rs        # 11 topics in ALL_TOPICS, SignedGossipMessage, PeerExchangeMessage, events
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
    │   ├── vc_did.rs       # /alexandria/vc-did/1.0 handler — DID doc + key rotation (PR 9)
    │   ├── vc_status.rs    # /alexandria/vc-status/1.0 handler — status list snapshots/deltas (PR 9)
    │   ├── vc_fetch.rs     # /alexandria/vc-fetch/1.0 request-response — authority-respecting pull (PR 9, 19c, 19d)
    │   ├── presentation.rs # /alexandria/vc-presentation/1.0 — selective disclosure envelopes (PR 11)
    │   ├── pinboard.rs     # /alexandria/pinboard/1.0 — pinning commitment observations (PR 10)
    │   ├── archive.rs      # Archive/replay utilities for VC propagation (PR 10)
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
        ├── blockfrost.rs   # Blockfrost REST client (preprod, 6 endpoints)
        ├── types.rs        # UTxO, protocol params, chain tip
        ├── policy.rs       # NativeScript policies, asset names
        ├── tx_builder.rs   # Conway-era tx building — NFT minting (pallas)
        ├── gov_tx_builder.rs  # Plutus V3 governance tx builders (6 actions)
        ├── soulbound_tx_builder.rs  # CIP-68 soulbound token minting tx builder
        ├── plutus_data.rs  # Plutus Data CBOR encoding (datums + redeemers)
        ├── snapshot.rs     # CIP-68 asset names, datum encoding, metadata
        ├── governance.rs   # On-chain governance metadata (CIP-25 label 1694)
        ├── onchain_queue.rs  # Persistent governance tx queue (pending → submitted → confirmed)
        ├── anchor_queue.rs   # VC integrity-anchor tick processor (PR 8, §12.3)
        ├── anchor_tx.rs      # Metadata-only Cardano tx builder (label 1697 = ALEXANDRIA_ANCHOR_LABEL, PR 8/16)
        └── script_refs.rs  # Deployed validator script hashes + reference UTxO locations
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
├── composables/            # Shared reactive state (15 composables)
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
│   ├── useTutoringRoom.ts  # Tutoring session management
│   ├── useCredentials.ts   # VC IPC wrapper (issue/list/verify/revoke/suspend/export, PR 14)
│   ├── useOpinions.ts      # Field Commentary opinion IPC wrapper
│   └── usePinning.ts       # PinBoard commitment IPC wrapper
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
    │       ├── AppTopBar.vue        # Top bar with user menu dropdown (role badge, icon SVGs)
    │       ├── MobileTabBar.vue     # Bottom tab bar for mobile (iOS/Android), backdrop blur, active indicator
    │       ├── SidebarSkillGraph.vue # force-graph canvas widget — earned/available/locked skill nodes with glow
    │       └── TutoringPiP.vue      # Picture-in-picture call overlay
│
├── layouts/
│   ├── AppLayout.vue       # Sidebar + content area
│   └── BlankLayout.vue     # Full-screen (onboarding, unlock)
│
├── pages/                  # 30 route views
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
