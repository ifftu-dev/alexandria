# 0004. The Tauri-free runtime lives behind a crate boundary

Status: active
Date: 2026-09-16 (reassessed 2026-09-19)

## Context

A headless Alexandria node (for example a persona node serving a demo world on a Linux host) must run without a desktop stack. The app library, `app_lib` in `src-tauri`, can't provide one:

- `tauri` and the `tauri-plugin-*` crates are unconditional dependencies.
- On `x86_64-unknown-linux-gnu`, with no features enabled, the dependency tree pulls in WebKitGTK, JavaScriptCore, GTK/GDK, libsoup, X11, ALSA and V4L2 system libraries. ALSA arrives through `live` → `moq-media` → `firewheel` → `cpal`, and `firewheel` isn't optional. V4L2 arrives through `nokhwa`, which is target-gated, not feature-gated.
- These are shared libraries resolved at load time. A binary that links `app_lib` fails in the dynamic linker on a host without them, before any of its code runs.

The obstacle is linkage, not initialisation: the CLI links `app_lib` and never opens a display. Much is already Tauri-free. `AppState` holds no Tauri types, runtime construction needs only a data directory, most commands wrap a Tauri-free `*_db`/`*_impl` function, and `db/`, `crypto/`, `content_store/`, `cardano/`, `domain/` and `profile/manager.rs` have no Tauri references.

Options considered:

1. Install the desktop libraries in the headless image. No code change, but a larger image and attack surface, and not actually headless.
2. Make `tauri` and the media crates optional behind a default-on feature. The Tauri-free helpers inside `commands/` would still have to move, and a wrong `cfg` only shows up in the headless build.
3. Keep the Tauri-free runtime in a crate whose manifest has no `tauri`, so the compiler rejects an accidental dependency.

## Decision

Option 3, using the crate that already exists: `crates/alexandria-studio`. Don't create a second core crate.

- `alexandria-studio` and `alexandria-mcp` link neither Tauri nor `app_lib`.
- `alexandria_studio::broker::BrokerHost` is the host boundary (lifecycle gate, grants, profile epoch, clock, database access, blob fetch). The app, the `fixture_host` example and the MCP test profile implement it.
- Shared logic moves into the crate and the app re-exports it. Duplicates are deleted, not kept in sync: `src-tauri/src/goals/jd_parser.rs` now re-exports `alexandria_studio::jd_parser`.
- A headless `Node` facade, when built, sits beside `BrokerHost` and reuses its gate, epoch and clock.

## Consequences

- The compiler enforces the boundary. The real proof is a CI job that builds and runs a node on Linux with no GTK, WebKit or ALSA installed, and that job doesn't exist yet.
- Tutoring (`live`/`moq-media`) stays app-only, because keeping it out of core is what removes ALSA.
- Process-global state blocks two nodes in one process and still has to become per-node: `DIAGNOSTICS_ENABLED` (`commands/diagnostics.rs`), `EXTRA_RELAYS` (`p2p/discovery.rs`), `ONCHAIN_ISSUERS` (`p2p/relay_registry.rs`) and `DIAG_PATH` (`diag.rs`). Diagnostics mode is also an assessment-integrity concern on its own.
- Still to do: lift runtime construction out of Tauri `.setup()`, add an event-sink trait for UI events (gossip, tutoring, plugin-install progress), move leaf modules (`domain`, `db`, `crypto`, then `content_store`, `cardano`, `p2p`) into the crate, and pick one profile-lifecycle mechanism (the app's `ProfileOperations` leases or the studio's epoch and gate).
- Still open: whether the crate keeps the `alexandria-studio` name or is split into a general core and studio-specific parts.
