# Community Plugin System

**Status:** Phase 1–2 implemented (iframe-sandboxed interactive plugins,
WASM graders, capability consent, management UI). Phase 3 (P2P discovery +
DAO attestation) types and gossip topics are in place; discovery UI is
partial.

## Overview

A plugin is a content-addressed bundle — a signed `manifest.json` plus a
`ui/` directory rendered inside a sandboxed iframe, optionally with a
deterministic `grader.wasm` for credential-bearing assessments. Plugins
provide new **learning element types**: an element with
`element_type = 'plugin'` and a non-null `plugin_cid` dispatches to the
plugin host instead of a built-in renderer.

First-party element types (MCQ, etc.) are themselves plugins — the host's
own UI flows through the same contract, which keeps it production-grade.

### Identity & addressing

- `plugin_cid` = BLAKE3 of the canonical `manifest.json` bytes. Identity.
- Manifest `id` is `did:key:<author>#<slug>`; the manifest is signed by the
  author's Ed25519 DID-Key. Built-ins skip signature verification — the
  host binary is their trust root.
- The manifest shape is frozen at `api_version = "1"`. New fields are
  additive and optional so a 2026 manifest still parses in 2046.

### Dependencies

A manifest may declare other plugins it requires via an optional
`dependencies` array of plugin **ids** (`did:key:<author>#<slug>`):

```json
"dependencies": [
  "did:key:z6Mk…#editor-javascript",
  "did:key:z6Mk…#editor-typescript"
]
```

When a plugin is installed the host resolves each dependency to an
already-installed bundle (a built-in, or one installed earlier / by the
user) and records the edge in `plugin_dependencies` (migration 063). A
dependency that can't be resolved fails the install and rolls it back —
Phase 1 has no on-demand bundle fetch, so dependencies must already be
present (built-ins always are). Resolution is by id, not CID, so it
survives a dependency being reinstalled at a new version.

Two guarantees follow:

- **Auto-install** — installing a plugin pulls its dependencies in first.
  For built-ins this just means registering a plugin *after* the plugins
  it depends on in `builtins::BUILTIN_PLUGINS` (the umbrella
  `editors` is registered last, after its language plugins).
- **Uninstall guard** — `plugin_uninstall` refuses to remove a plugin that
  other installed plugins still depend on ("still required by …"); the
  dependents must be removed first.

The Settings → Plugins UI shows both directions on each card (*Requires* /
*Required by*) via `plugin_list_dependencies` + the manifest. Edges cascade
away automatically when either endpoint is uninstalled.

## Architecture

### Backend (Rust, `src-tauri/src/plugins/`)

| Module | Responsibility |
|--------|----------------|
| `manifest` | Parse + semantically validate `manifest.json` |
| `verifier` | Ed25519 signature verify; compute the BLAKE3 `plugin_cid` |
| `registry` | On-disk bundle store + SQLite install/list/uninstall, capability grant/revoke, enable/disable, README read, in-bundle asset read |
| `asset_protocol` | `plugin://<cid>/` URI handler; injects the bootstrap script + per-plugin nonce CSP |
| `builtins` | First-party bundles embedded via `include_bytes!`, installed (and refreshed) at startup; prunes stale builtin rows when a builtin's manifest/CID changes |
| `catalog` | Discovery cache for the `/alexandria/plugins/1.0` gossip topic |
| `attestation` | Plugin DAO multi-sig attestation verify + store |
| `wasm_runtime` | Wasmtime grader sandbox (desktop only) |
| `irl_review` | Local instructor-review inbox (see below) |

IPC commands live in `src-tauri/src/commands/plugins.rs`: install /
uninstall / list / get-manifest, capability grant/revoke/list,
`plugin_set_enabled`, `plugin_get_docs`, `plugin_read_asset_data_url`
(icons + README images as `data:` URLs), `plugin_submit_and_grade`
(desktop), the `irl_*` inbox commands, and the Phase-3 catalog +
attestation commands. DID→username resolution is
`resolve_display_names` in `commands/identity.rs`.

### Frontend (Vue)

- **`components/plugin/PluginHost.vue`** — loads manifest + grants, mounts
  the iframe, drives the permission prompt, proxies plugin events, routes
  `submit`/`irl_refresh` to the right IPC. Refuses to mount a disabled
  plugin; passes `content_inline` + theme tokens through.
- **`components/plugin/PluginIframe.vue`** — the sandbox + host↔plugin
  postMessage bridge (protocol v1). Snapshots host theme tokens; supports
  deferred host responses (`resolveSubmit` / `resolveEvent`).
- **`components/plugin/PermissionPrompt.vue`** — capability consent dialog
  (shows the author as a username via `useDisplayNames`).
- **`components/settings/PluginsPanel.vue`** — the management UI (grid of
  cards + search + IRL tabs), embedded in the Settings page.
- **`pages/PluginDocs.vue`** — full-page README viewer.
- **`utils/markdown.ts`** — sanitized Markdown renderer for README docs.
- **`components/course/elementRegistry.ts`** — maps `element_type` →
  renderer; `'plugin'` → `PluginHost`.

### Sandbox & security

- `sandbox="allow-scripts allow-same-origin"`. Each plugin loads from its
  own `plugin://<cid>/` origin, so the browser's same-origin policy gives
  cross-plugin isolation. `allow-same-origin` is required for
  `getUserMedia` to work under the iframe's Permissions Policy.
- Per-plugin CSP: `connect-src 'none'` (no network of any kind),
  `script-src 'self' plugin://<cid> 'nonce-…' 'wasm-unsafe-eval'`. The
  injected bootstrap script carries the per-response nonce; plugin-author
  inline scripts remain blocked.
- The `allow` attribute (microphone, camera, midi, …) is built from the
  manifest's **declared** capabilities; runtime gating is enforced by the
  host through the consent flow, not by toggling the attribute (WKWebView
  reads `allow` once at load).
- On macOS the host installs a WKUIDelegate
  (`src-tauri/src/macos_media_delegate.rs`) that auto-grants WebKit-level
  media-capture requests — the plugin's own consent already ran through
  `PermissionPrompt`, so the redundant native prompt is suppressed.

## The `window.alex` API (protocol v1)

Injected by `bootstrap.js`. The only surface a plugin needs:

| Method | Purpose |
|--------|---------|
| `ready(declaredCaps)` | Handshake. Host replies with an `init` message (content, state, granted caps, locale, theme) |
| `requestCapability(name, reason)` | Prompt for consent → `{ granted }` |
| `complete(progressFraction, score?)` | Mark the element complete |
| `submit(submission, metadata)` | Credential / review submission |
| `persistState(blob)` | Opaque per-element state |
| `emitEvent(type, payload)` | Telemetry / host-mediated events |
| `onHost(handler)` | Receive `init`, `capability_granted/revoked`, `theme_changed` |

### Capabilities

Declarable in v1: `microphone`, `camera`, `midi`, `fullscreen`,
`clipboard`, `storage`, `ml_inference`. Consent is per-plugin with scope
`once` / `session` / `always`; `always` persists in `plugin_permissions`.

### Theming

At init the host sends a `theme` map of `--app-*` and `--theme-*` CSS
tokens (background, foreground, border, accent, success, warning, error…);
`bootstrap.js` applies them to the plugin's `documentElement`. Plugins
should style with `var(--theme-*)`. A future `theme_changed` message
hot-swaps tokens for the planned custom-accent picker.

## Management UI — Settings → Plugins

Settings is a full page (`src/pages/Settings.vue`, route `/settings/:section?`);
the Plugins section renders `PluginsPanel.vue` at `/settings/plugins`. Three
tabs:

- **Installed** — a responsive grid of plugin cards (auto-fills columns by
  width) with a search box (matches name / description / capability / tag).
  Each card shows a thumbnail (manifest `icon_path` inlined as a `data:`
  URL, or a deterministic gradient + monogram fallback), badges, the
  attestation status, and clickable capability chips (click → revoke).
  Card actions: enable/disable toggle, donate (manifest `donate_url`),
  uninstall (built-ins protected). Clicking the card body opens the
  full-page docs viewer at `/settings/plugins/:cid/docs`
  (`pages/PluginDocs.vue`) — rendered README Markdown with screenshots
  inlined from the bundle.
- **Instructor Inbox** — pending IRL Review submissions on this device,
  with a review form (score slider, per-skill ratings, feedback, file
  previews). Submitters shown by username.
- **My Submissions** — the learner's own IRL Review submissions with their
  returned scores/feedback.

### README docs + screenshots

The docs viewer renders the plugin's bundled `README.md` as sanitized
Markdown (`utils/markdown.ts` + DOMPurify). Relative image references
(e.g. `![](screenshots/x.svg)`) are resolved to `data:` URLs via
`plugin_read_asset_data_url` — the main window's CSP forbids the
`plugin://` scheme for `<img>`, so icons and screenshots are inlined.

### Usernames, not DIDs

DIDs are opaque, so author / learner / reviewer identities render as
usernames via the `useDisplayNames` composable (backed by
`resolve_display_names`): the active profile's own DID → its display name,
built-in authors → "Alexandria", everything else → a short-DID fallback.
See [`settings.md`](settings.md) — usernames are mandatory for every
profile.

## Auto-advance

The player suppresses auto-advance for `element_type = 'plugin'` — plugins
are interactive (replay, retry, review results), so the learner clicks
**Next** themselves. See `shouldAutoAdvance` in `Player.vue`.

## Built-in plugins

| Slug | Kind | Notes |
|------|------|-------|
| `mcq` | graded | Multiple-choice, deterministic WASM grader |
| `music-trainer` | interactive | Capability-prompt demo (mic + amplitude meter) |
| `music-reviews` | interactive | Scrolling-timeline pitch trainer; see its `README.md`. Pitch detection unit-tested via `ui/pitch.test.js` |
| `irl-review` | interactive | Upload work for human instructor review |
| `editor-javascript` | graded | Write and run JavaScript with live evaluation; runs locally in a sandboxed Boa engine (pure-Rust JS → WebAssembly) against test cases; graded submissions are re-run by the host's deterministic grader |
| `editor-typescript` | graded | Same, TypeScript — types are stripped in-engine (sucrase) then run on Boa |
| `editor-python` | graded | Same, Python on the RustPython engine (compiled to sandboxed WebAssembly); teaching subset (builtins only) |
| `editor-cpp` | graded | Write and run C/C++, interpreted locally by the bundled JSCPP engine inside the Boa WebAssembly runtime; intro subset (no STL/templates) |
| `editors` | umbrella | Umbrella that `depends on` the editor language plugins; installing it auto-installs them |

The editor plugins' in-browser runtimes (the Boa pure-Rust JS engine
compiled to wasm for JS/TS, RustPython for Python, JSCPP interpreted inside
Boa for C/C++) and CodeMirror live in each bundle's `ui/vendor/` and
**are committed**, so the app and CI build with no extra step. Regenerate
them with `plugins/builtin/editor-shared/build.sh` only after bumping a
runtime version; they are embedded via `include_bytes!`.

## IRL Review flow

Local, no-network instructor review:

1. **Learner submits** — the plugin base64-encodes files client-side and
   calls `alex.submit(submission, { type: 'irl_review', skills })`.
   `PluginHost` routes this to `irl_submit_for_review`, queuing a `pending`
   row in `plugin_irl_submissions`.
2. **Instructor reviews** — Settings → Plugins → Instructor Inbox lists
   pending rows across the node; posting a review (`irl_post_review`) writes
   score + feedback + per-skill ratings and flips status to `reviewed`.
3. **Learner sees the result** — the plugin polls `irl_list_my_submissions`
   (via an `irl_refresh` event) and renders the reply.

Cross-device / federated review routing is a later phase.

## Phases

- **Phase 1** — interactive plugins, iframe sandbox, capability consent,
  local-file install.
- **Phase 2** — deterministic WASM graders, submit-and-grade, the
  `element_submissions` reproducibility bundle.
- **Phase 3** — P2P discovery (`/alexandria/plugins/1.0`) and Plugin DAO
  attestation (`/alexandria/plugin-attestations/1.0`).

## Related

- Schema: [`database-schema.md`](database-schema.md) — `plugin_*` tables.
- Gossip topics + identity binding: [`architecture.md`](architecture.md).
