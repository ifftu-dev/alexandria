# Headless dependency map

H01 step 1. Which state and code require Tauri, UI events, media or
process-global state, and what that decides about how a headless node is built.
Every claim below was checked against the code at `bd8a971`; line numbers refer
to that revision.

## Decision

**Extract a `crates/alexandria-core` crate.** A facade inside `app_lib` cannot
produce a headless binary that runs on a Linux host without a desktop stack.

This is the condition H01 step 4 names — "if APP's crate dependencies force
GUI/native initialization/linkage" — and it is met on linkage alone:

- `tauri` is a plain, unconditional dependency (`src-tauri/Cargo.toml:51`), with
  eight `tauri-plugin-*` crates beside it (`:52–59`).
- On `x86_64-unknown-linux-gnu`, with no features enabled, `cargo tree` resolves
  `webkit2gtk-sys`, `javascriptcore-rs-sys`, `gtk-sys`, `gdk-sys`, `soup3-sys`,
  `x11`, `alsa-sys` and `v4l2-sys-mit`.
- `alsa-sys` arrives through `live` → `moq-media` → `firewheel` → `cpal`, and
  `firewheel` is not optional (`crates/moq-media/Cargo.toml:19`), so no feature
  flag removes it. `v4l2-sys-mit` arrives through the desktop `nokhwa`
  dependency, which is target-gated rather than feature-gated.

These are shared libraries resolved when the process loads. A binary that links
`app_lib` fails in the dynamic linker on a host without them, before any of its
own code runs, whether or not it ever touches a window. Cargo cannot drop a
`[dependencies]` entry for one consumer, and gating `tauri` behind a feature
would mean conditionalising every call site across 359 commands.

Initialization is *not* the obstacle. The CLI already links `app_lib` and never
opens a display, because nothing GUI- or media-related runs unless
`app_lib::run()` is called. Linkage is the obstacle, and only a crate boundary
removes it.

## What is already free of Tauri

More than the size of the crate suggests.

- **`AppState` holds no Tauri types.** None of its fields is an `AppHandle`,
  window or tray handle (`lib.rs:73–111`).
- **Runtime construction is a function of one path.** Everything from resolving
  the data directory (`lib.rs:748`) to assembling `AppState` (`:1054–1075`) —
  the profile manager, database executor, keystore slot, content node, resolver,
  discovery, P2P slot and the background on-chain queue task — needs only
  `app_dir: PathBuf`. Tauri is strictly required from `app.manage(app_state)`
  (`:1081`) onward.
- **Most commands already delegate.** Of 359 `#[tauri::command]` handlers, the
  dominant shape is a thin wrapper around a Tauri-free `*_db` / `*_impl`
  function run on the database executor; about 101 such functions exist.
- **Proven in practice.** The CLI drives credentials, VCs, DIDs, crypto and the
  database with no `AppState`, and `tests/guardian_e2e.rs` runs the full
  guardian-link protocol between two in-process persons with no Tauri at all.
- **Whole subsystems are clean**: `db/`, `crypto/`, `content_store/`,
  `cardano/`, `domain/`, `p2p/network.rs`, `profile/manager.rs` and the
  non-command parts of `sentinel/` and `plugins/` have no Tauri references.

## What is coupled to Tauri

| Coupling | Where | Headless replacement |
|---|---|---|
| Data directory | `lib.rs:752`, and again in `commands/p2p.rs:65` instead of reading `state.app_data_dir` | an explicit path; the second lookup should read the state it already has |
| Session-scoped command access | `profile/scope.rs` — `ProfileState` is a `tauri::ipc::CommandArg` extractor that reads a session header and admits a `ProfileLease` | a headless session/lease API over the same `ProfileOperations` |
| Gossip UI events | `p2p/inbound.rs:239–244` | already shaped correctly: `gossip_ingest` takes a generic closure and only the command binds it to an `AppHandle` |
| Tutoring events | `tutoring/manager.rs` stores an `AppHandle` per session and threads it through ~15 methods, used only to emit | an event-sink trait; lazy for headless, which does not start media |
| Progress events mid-operation | `commands/plugins.rs` `install_course_plugins` emits between install steps | event sink |
| Logger | `tauri_plugin_log` inside `.setup()` (`lib.rs:701`) | an ordinary logger set once per process, with node-tagged records |
| Plugin asset protocol | `register_uri_scheme_protocol("plugin", …)` (`lib.rs:1218`) | none needed — it serves the webview |

Commands that mix business logic, several `Mutex<Option<T>>` state fields and
event emission in one body need both a state facade and an event sink before
they can move: `p2p_start`, `guardian_create_invite`, `pairing_generate_code`,
`install_course_plugins`, and reputation snapshot creation. `guardian.rs`,
`pairing.rs` and `plugins.rs` have no extracted delegate functions at all.

## Process-global state

Two persona nodes in one process — which H01's required tests assume — collide
on the following.

**Blocks two nodes, in business code:**

| Global | Location | Effect |
|---|---|---|
| `DIAGNOSTICS_ENABLED: AtomicBool` | `commands/diagnostics.rs:20` | enabling diagnostics for one persona flips it for every persona's assessment attempts (`assessment.rs:312`, `adaptive.rs:93`) |
| `EXTRA_RELAYS: RwLock<Vec<_>>` | `p2p/discovery.rs:20` | each node's `p2p_start` overwrites the other's relay set |
| `ONCHAIN_ISSUERS: RwLock<Vec<_>>` | `p2p/relay_registry.rs:39` | shared authorized-issuer set with a write-write race; nodes cannot observe different registry states |
| `DIAG_PATH: OnceLock<PathBuf>` | `diag.rs:17` | the second node's `init` is silently ignored, so its diagnostics and panics land in the first node's log |

`DIAGNOSTICS_ENABLED` is worth fixing independently of headless work: it is an
assessment-integrity property, and it is process-global.

**Blocks only through Tauri setup**, so a headless node that never enters
`.setup()` avoids them: the `tauri_plugin_log` logger, `ALEXANDRIA_DATA_DIR`
(read once, process-wide), and `tauri_plugin_single_instance`.

**Per-node values forced to be shared:** `ALEXANDRIA_COMPLETION_POLICY_ID` (read
each tick, `lib.rs:933`) and `ALEXANDRIA_DEVICE_LABEL` (the Identify agent
version on Linux, Android and Windows).

**Shared and harmless:** the embedded network profile and qualification
policies (one network per build, by design), the bundled ONNX classifiers
(read-only after first load), the GF(256) tables, the treasury and Blockfrost
defaults (one payer by design; Blockfrost already takes a per-database
override), and one-time Objective-C class registration.

**No contention** on ports or sockets: libp2p binds `/tcp/0` and `/udp/0`, and
iroh binds ephemerally. Nothing installs a global rustls provider in this
source tree; that is worth confirming at runtime once two nodes start.

## Media stays lazy

No camera, microphone or display code runs at startup. Camera enumeration,
permission prompts, Secure Input, Sentinel focus tracking and the Sentinel ML
models are all reached only through `#[tauri::command]` dispatch or window
events. `TutoringManager::new()` constructs state without opening devices. A
headless node can omit media entirely rather than disabling it.

## Proposed slice order for step 4

Each slice compiles, passes the full gate, and is committed on its own. App and
CLI import paths stay stable throughout by re-exporting moved modules from
`app_lib`.

1. **Make the four blocking globals per-node**, inside `app_lib`, before any
   crate moves. Diagnostics mode, the relay set, the issuer set and the diag log
   path become fields owned by the node's runtime. This fixes the integrity
   issue immediately and removes the hardest cross-node coupling before code
   starts moving.
2. **Lift runtime construction out of `.setup()`** into a plain
   `build_runtime(app_dir) -> AppState`, and make `p2p_start` read
   `state.app_data_dir` instead of asking Tauri again.
3. **Introduce an event-sink trait** with a Tauri adapter, and route the gossip
   closure, tutoring manager and plugin-install progress through it.
4. **Create `crates/alexandria-core`** and move leaf modules first — `domain`,
   `db`, `crypto`, then `content_store`, `cardano` and `p2p` — checking before
   each move that the module does not reach back into `commands`. Diagnostics
   mode is the known case: assessment code consults a flag that lives in
   `commands/`, which slice 1 relocates.
5. **Add the headless `Node` facade** in core with owned profile resources,
   executor, lifecycle, network configuration, clock and event sink, and prove
   two isolated nodes on Linux in a CI job that installs no GTK, WebKit or ALSA
   packages. That job is the only real proof that linkage is solved.

## Open questions

- **Tutoring on headless nodes.** World serving needs blobs, catalog,
  attestations and addressed protocols, not video. Keeping `live`/`moq-media`
  out of core entirely is simplest, and is what removes ALSA; it means tutoring
  stays an app-only capability until someone needs it headless.
- **Keystore on headless Linux.** Desktop uses Stronghold and mobile uses the
  portable AES-GCM/Argon2id keystore, whose tests now run on desktop. Either
  could serve a headless node; the portable one has fewer native dependencies.
- **Logging with two nodes.** `log` allows one global logger per process, so
  per-node separation has to come from tagging records with a node identity
  rather than from separate loggers.
