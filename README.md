# Alexandria

**Decentralized learning platform — desktop node.**

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
- **Peer-to-Peer** — Fully decentralized via libp2p (GossipSub, Kademlia, mDNS, AutoNAT, Relay, DCUtR). No central server required.
- **Offline-First** — Local SQLite database, iroh content store, and Stronghold encrypted vault. Everything works without connectivity.

## Architecture

Alexandria Mark 3 is a **Tauri v2 desktop application** — a single binary that bundles a Rust backend with a Vue 3 frontend. There are no servers, no Docker containers, and no external databases.

```
alexandria-mark3/
├── src-tauri/        # Rust backend (Tauri v2)
│   └── src/
│       ├── cardano/  # Blockfrost client, Conway tx building, NFT policies
│       ├── commands/ # 118 IPC command handlers (frontend ↔ backend)
│       ├── crypto/   # BIP-39 wallet, Stronghold vault, Ed25519 signing
│       ├── db/       # SQLite (43 tables, 12 migrations, seed data)
│       ├── domain/   # Business logic (courses, evidence, governance, ...)
│       ├── evidence/ # Aggregation, attestation, challenges, reputation
│       ├── ipfs/     # iroh node, IPFS gateway fallback, CID resolution
│       └── p2p/      # libp2p swarm (14 submodules), cross-device sync
├── src/              # Vue 3 + TypeScript frontend
│   ├── pages/        # 19 pages (onboarding, courses, skills, governance, ...)
│   ├── components/   # UI components + auth + course + layout
│   ├── composables/  # useAuth, useTheme, useP2P, useSentinel, useLocalApi
│   └── assets/       # Tailwind CSS v4 design system
├── cli/              # Developer CLI (alex) — Rust + clap
└── docs/             # This documentation
```

| Layer | Technology |
|-------|------------|
| Desktop shell | Tauri 2.10, WebKit/WebView2 |
| Backend | Rust (2021 edition), tokio async runtime |
| Frontend | Vue 3, TypeScript, Vite, Tailwind CSS 4 |
| Database | SQLite (rusqlite, bundled) |
| Content storage | iroh 0.96 (BLAKE3 content-addressed blobs) |
| P2P networking | libp2p 0.56 (QUIC, GossipSub, Kademlia, mDNS) |
| Wallet | BIP-39 + CIP-1852 (pallas), IOTA Stronghold vault |
| Cardano | pallas 0.35 (Conway tx builder), Blockfrost preprod |
| Developer CLI | Rust, clap 4, owo-colors |

For the full architecture breakdown, see [Architecture](docs/architecture.md).

## Getting Started

### Prerequisites

- **Rust 1.83+** with `cargo`
- **Node.js 22+** with `npm`
- **Tauri CLI**: `cargo install tauri-cli`

### Development

```bash
git clone git@github.com:ifftu-dev/alexandria.git
cd alexandria
npm install
cargo tauri dev
```

The app launches a native window backed by a local webview. First launch generates the SQLite database, runs 12 migrations, seeds taxonomy/courses/governance data, and starts the iroh content store.

### Using the Developer CLI

All common dev tasks are available through the `alex` CLI:

```bash
cargo install --path cli

alex dev run          # cargo tauri dev
alex dev test         # cargo test (309 tests)
alex dev clippy       # cargo clippy -- -D warnings
alex dev fmt          # cargo fmt --check
alex dev check        # vue-tsc type check
alex dev all          # fmt + clippy + test + check

alex db status        # Table row counts, migration version, data sizes
alex db reset --force # Delete all app data (SQLite + vault + iroh)

alex build check      # cargo check + vue-tsc
alex build release    # cargo tauri build (full release bundle)

alex config show      # Project paths, Tauri config, tool versions
alex config path      # Print app data directory

alex health           # Check if app process is running

alex clean build      # Remove target/, dist/, .vite cache
alex clean data --force  # Remove app data
alex clean all --force   # Remove everything
```

### First-Time Onboarding

1. Launch the app — you see the onboarding screen
2. Create a password — this encrypts the Stronghold vault
3. A 24-word BIP-39 mnemonic is generated (CIP-1852 derivation)
4. Payment and stake addresses are derived (preprod testnet)
5. Back up the mnemonic — it is your identity and wallet

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
| `vault.stronghold` | IOTA Stronghold encrypted vault (keys, mnemonic) |
| `iroh/` | Content-addressed blob store (course content, profiles) |

Use `alex config path` to print this directory on any platform.

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture](docs/architecture.md) | System design — desktop-first, offline-first, trustless |
| [Database Schema](docs/database-schema.md) | All 43 tables, 12 migrations, relationships |
| [P2P Protocol](docs/protocol-spec-v1.md) | Wire formats, gossip topics, validation, peer scoring |
| [Project Structure](docs/project-structure.md) | Directory layouts, module responsibilities |
| [Missing Features](docs/missing-features.md) | Comparison with mark2 — what's not yet ported |

## License

TBD
