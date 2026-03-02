# Alexandria

**Decentralized learning platform — desktop and mobile node.**

<p>
  <a href="docs/architecture.md">Architecture</a> &middot;
  <a href="docs/database-schema.md">Database Schema</a> &middot;
  <a href="docs/protocol-spec-v1.md">P2P Protocol</a> &middot;
  <a href="docs/project-structure.md">Project Structure</a> &middot;
  <a href="docs/missing-features.md">Roadmap / Missing Features</a>
</p>

> **This software is in active development. It is NOT production-ready. Do not use with real credentials, real funds, or sensitive data.**

## What Alexandria Does

- **Courses & Assessments** — Rich HTML, video, and interactive quiz content with per-element progress tracking, notes, and skill tagging.
- **Skill Proofs** — Learners earn verifiable credentials scoped to individual skills at Bloom's taxonomy levels (remember through create), aggregated from weighted evidence.
- **Reputation** — Instructor impact derived from learner outcomes, scoped to `(subject, role, skill, proficiency_level)`. Distribution-based with confidence bounds — no global scores.
- **Blockchain Credentials** — NFTs minted on Cardano (Conway era) with CIP-25 metadata. Independently verifiable on-chain without the platform.
- **Governance** — DAOs mirror the knowledge taxonomy. Elections and proposals with 2/3 supermajority. Committee-gated taxonomy updates.
- **Assessment Integrity** — Sentinel anti-cheat with keystroke autoencoder, mouse trajectory CNN, and face embedder. All processing client-side — only derived scores cross the network.
- **Peer-to-Peer** — Fully decentralized via libp2p with a private Alexandria Kademlia DHT, GossipSub, Circuit Relay v2, AutoNAT, and DCUtR. Devices discover each other through a relay bootstrap node — no central server required.
- **Offline-First** — Local SQLite database, iroh content store, and encrypted vault (Stronghold on desktop, AES-256-GCM + Argon2id on mobile). Everything works without connectivity.
- **Mobile Node** — iOS is a first-class target. The mobile app is a fully functional node — same P2P networking, content storage, and wallet as desktop. Multi-device support via shared BIP-39 mnemonic.

## Architecture

Alexandria (Mark 3) is a **Tauri v2 application** — a single binary that bundles a Rust backend with a Vue 3 frontend. It runs on macOS, Linux, Windows, and iOS. There are no servers, no Docker containers, and no external databases.

```
alexandria/
├── src-tauri/        # Rust backend (Tauri v2)
│   └── src/
│       ├── cardano/  # Blockfrost client, Conway tx building, NFT policies
│       ├── commands/ # 118 IPC command handlers (frontend ↔ backend)
│       ├── crypto/   # BIP-39 wallet, vault (Stronghold / portable), Ed25519
│       ├── db/       # SQLite (43 tables, 14 migrations, seed data)
│       ├── diag.rs   # File-based diagnostic logger + panic hook
│       ├── domain/   # Business logic (courses, evidence, governance, ...)
│       ├── evidence/ # Aggregation, attestation, challenges, reputation
│       ├── ipfs/     # iroh node, IPFS gateway fallback, CID resolution
│       └── p2p/      # libp2p swarm — DHT, relay, gossip, peer exchange
├── src/              # Vue 3 + TypeScript frontend
│   ├── pages/        # 19 pages (onboarding, courses, skills, governance, ...)
│   ├── components/   # UI components + auth + course + layout
│   ├── composables/  # useAuth, useTheme, useP2P, useSentinel, useLocalApi
│   └── assets/       # Tailwind CSS v4 design system
├── cli/              # Developer CLI (alex) — Rust + clap
├── patches/          # Local crate patches (if-watch iOS fix)
└── docs/             # Architecture, schema, protocol, structure docs
```

| Layer | Technology |
|-------|------------|
| Shell | Tauri 2.10, WebKit (macOS/iOS) / WebView2 (Windows) |
| Backend | Rust (2021 edition), tokio async runtime |
| Frontend | Vue 3, TypeScript, Vite, Tailwind CSS 4 |
| Database | SQLite (rusqlite, bundled) |
| Content storage | iroh 0.96 (BLAKE3 content-addressed blobs) |
| P2P networking | libp2p 0.56 (TCP, QUIC, GossipSub, Kademlia, Relay, DCUtR) |
| Wallet (desktop) | BIP-39 + CIP-1852 (pallas), IOTA Stronghold vault |
| Wallet (mobile) | BIP-39 + CIP-1852 (pallas), AES-256-GCM + Argon2id vault |
| Cardano | pallas 0.35 (Conway tx builder), Blockfrost preprod |
| Developer CLI | Rust, clap 4, owo-colors |

For the full architecture breakdown, see [Architecture](docs/architecture.md).

## P2P Network

All nodes connect to a private Alexandria Kademlia DHT (`/alexandria/kad/1.0`) using a relay server as the bootstrap node. The relay provides:

- **Circuit Relay v2** — NAT traversal for mobile and firewalled nodes
- **Kademlia DHT** — Peer discovery via `get_providers` on the namespace key `SHA256("ifftu.alexandria")`
- **Identify** — Protocol negotiation and agent version exchange

After connecting to the relay, each node:

1. Requests a circuit relay reservation (reachable address for NAT'd peers)
2. Bootstraps the Kademlia DHT
3. Registers as a provider on the namespace key
4. Discovers other providers and dials them directly (or via relay hop)

Known peer addresses are persisted to a `peers` table in SQLite and reloaded on subsequent launches. GossipSub peer exchange messages propagate addresses across the mesh.

The P2P node auto-starts after wallet unlock — users see only the live peer count.

The relay server lives in a [separate repository](https://github.com/ifftu-dev/alexandria-relay).

## Getting Started

### Prerequisites

- **Rust 1.83+** with `cargo`
- **Node.js 22+** with `npm`
- **Tauri CLI**: `cargo install tauri-cli`

For iOS builds:

- **Xcode 15+** with command-line tools
- **Rust iOS targets**: `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`
- **Apple Development Team** configured in `src-tauri/tauri.conf.json`

### Development (Desktop)

```bash
git clone git@github.com:ifftu-dev/alexandria.git
cd alexandria
npm install
cargo tauri dev
```

The app launches a native window backed by a local webview. First launch generates the SQLite database, runs migrations, seeds taxonomy/courses/governance data, and starts the iroh content store.

### Building for iOS

**Simulator (Apple Silicon):**

```bash
cargo tauri ios build --target aarch64-sim
```

**Physical device:**

```bash
cargo tauri ios build --target aarch64
```

**Deploy to simulator:**

```bash
xcrun simctl install <SIMULATOR_UDID> src-tauri/gen/apple/build/arm64-sim/Alexandria.app
xcrun simctl launch <SIMULATOR_UDID> org.alexandria.node
```

**Deploy to device:**

```bash
xcrun devicectl device install app --device <DEVICE_UDID> src-tauri/gen/apple/build/arm64/Alexandria.ipa
xcrun devicectl device process launch --device <DEVICE_UDID> org.alexandria.node
```

Clean build artifacts before rebuilding to avoid "Directory not empty" errors:

```bash
rm -rf src-tauri/gen/apple/build
```

### Developer CLI (`alex`)

The `alex` CLI wraps common dev tasks, multi-platform builds, and data management into a single tool.

```bash
cargo install --path cli    # install globally as `alex`
```

#### Running

```bash
# Desktop (hot-reload dev server)
alex run desktop                              # cargo tauri dev

# iOS (interactive device/simulator picker)
alex run ios                                  # pick from available simulators + devices
alex run ios --device "iPhone 17 Pro"         # skip picker, run on specific device
alex run ios --open                           # open in Xcode instead

# Android (interactive emulator/device picker)
alex run android                              # pick from available AVDs + devices
alex run android --device "Pixel_9_Pro"       # skip picker, run on specific emulator
alex run android --open                       # open in Android Studio instead

# All run commands support --release for release mode
alex run ios --release
```

#### Code Quality

```bash
alex dev test         # cargo test (309 tests)
alex dev clippy       # cargo clippy -- -D warnings
alex dev fmt          # cargo fmt --check
alex dev check        # vue-tsc type check
alex dev all          # fmt + clippy + test + check (full CI pass)
```

#### Building

```bash
# Quick builds
alex build check      # cargo check + vue-tsc (fast, no codegen)
alex build release    # Full desktop release (cargo tauri build)

# Platform-specific builds
alex build desktop                        # Current host (e.g. mac-arm64)
alex build desktop --target mac-arm64 mac-x64   # Multiple targets
alex build android                        # arm64 (default)
alex build android --target arm64 armv7   # Multiple Android ABIs
alex build ios                            # Simulator for current arch
alex build ios --target device            # iOS device (needs signing)
alex build ios --target device sim-arm64  # Device + simulator

# All platforms at once (uses host defaults for each)
alex build all
alex build all --debug

# Debug builds (faster compile, larger binary)
alex build desktop --debug
alex build android --debug
alex build ios --debug

# Interactive wizard (prompts for platform, targets, profile)
alex build platform

# List all available build targets
alex build list
```

**Available build targets:**

| Platform | Target | Description | Rust triple |
|----------|--------|-------------|-------------|
| Desktop | `mac-arm64` | macOS Apple Silicon | `aarch64-apple-darwin` |
| Desktop | `mac-x64` | macOS Intel | `x86_64-apple-darwin` |
| Desktop | `mac-universal` | macOS Universal | `universal-apple-darwin` |
| Desktop | `linux-x64` | Linux x86_64 | `x86_64-unknown-linux-gnu` |
| Desktop | `win-x64` | Windows x86_64 | `x86_64-pc-windows-msvc` |
| Android | `arm64` | arm64-v8a | `aarch64-linux-android` |
| Android | `armv7` | armeabi-v7a | `armv7-linux-androideabi` |
| Android | `x86_64` | x86_64 (emulator) | `x86_64-linux-android` |
| iOS | `device` | iOS device | `aarch64-apple-ios` |
| iOS | `sim-arm64` | Simulator (ARM) | `aarch64-apple-ios-sim` |
| iOS | `sim-x64` | Simulator (Intel) | `x86_64-apple-ios` |

Prerequisites are checked automatically before each build. Android requires `ANDROID_HOME`, NDK, and Java 21. iOS requires Xcode. Device builds require code signing.

#### Database & Data

```bash
alex db status        # Table row counts, migration version, data sizes
alex db reset --force # Delete all app data (SQLite + vault + iroh)
```

#### Configuration

```bash
alex config show      # Project paths, Tauri config, tool versions
alex config path      # Print app data directory (for scripting)
```

#### Health & Cleanup

```bash
alex health           # Check if the app process is running

alex clean build      # Remove target/, dist/, .vite cache
alex clean data --force  # Remove app data
alex clean all --force   # Remove everything
```

### First-Time Onboarding

1. Launch the app — you see the onboarding screen
2. Create a password — this encrypts the vault
3. A 24-word BIP-39 mnemonic is generated (CIP-1852 derivation)
4. Payment and stake addresses are derived (preprod testnet)
5. Back up the mnemonic — it is your identity and wallet
6. The P2P node starts automatically and connects to the network

To use the same identity on another device, restore the mnemonic on that device.

To reset and start fresh:

```bash
alex db reset --force   # Or manually: rm -rf ~/Library/Application\ Support/org.alexandria.node/
```

## Testing

```bash
# All 309 Rust tests (unit + integration + stress)
cargo test -p alexandria-node

# Individual module tests
cargo test -p alexandria-node wallet
cargo test -p alexandria-node p2p
cargo test -p alexandria-node evidence

# Frontend type check
npx vue-tsc -b
```

The test suite includes:
- **283 synchronous tests** across crypto, database, P2P, evidence, cardano, and domain modules
- **26 async tests** (tokio) for iroh content operations, P2P swarm lifecycle, and network integration
- **~1500 lines of stress tests** covering high-volume gossip (200+ messages), concurrent validation (1000 messages / 10 threads), sync conflicts, and adversarial inputs

## Data Storage

All data lives in `~/Library/Application Support/org.alexandria.node/` (macOS):

| File/Directory | Purpose |
|----------------|---------|
| `alexandria.db` | SQLite database (43 tables) |
| `vault.stronghold` | IOTA Stronghold encrypted vault — desktop only |
| `vault.enc` | AES-256-GCM + Argon2id encrypted vault — mobile only |
| `iroh/` | Content-addressed blob store (course content, profiles) |

Use `alex config path` to print this directory on any platform.

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture](docs/architecture.md) | System design — offline-first, trustless, multi-platform |
| [Database Schema](docs/database-schema.md) | All 43 tables, 14 migrations, relationships |
| [P2P Protocol](docs/protocol-spec-v1.md) | Wire formats, 6 gossip topics, validation, peer scoring |
| [Project Structure](docs/project-structure.md) | Directory layouts, module responsibilities |
| [Security Audit](docs/security-audit.md) | 24 findings (1 critical, 4 high, 8 medium, 6 low, 5 info) |
| [Performance Audit](docs/performance-audit.md) | 23 findings (2 critical, 5 high, 8 medium, 5 low, 3 info) |
| [Missing Features](docs/missing-features.md) | Comparison with mark2 — what's not yet ported |

## License

TBD
