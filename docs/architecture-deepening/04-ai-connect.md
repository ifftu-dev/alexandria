# One preamble to a model: connect in the AI crate

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/architecture-deepening-spec.md

## Problem Statement

Three places build the path from a saved connection to a usable adapter, each in its own way: the chat run resolves the endpoint, fetches the bearer, builds the adapter, applies limits, and maps errors; the tutor and Studio runs fetch the bearer by hand and call a Studio helper that resolves and times out internally; provider sign-in resolves and builds the adapter directly. Saving a signed-in connection skips the credential terms check that saving a keyed connection applies, so sign-in checks by hand. Adding a protocol touches the AI crate, the Studio model, and the webview's protocol list.

## Solution

One `connect` operation in the AI crate turns a saved connection plus its credential into a ready adapter: it resolves the endpoint under the endpoint policy, admits the credential under the vendor terms, applies limits, and returns typed errors. Every caller uses it. The Studio helper and the hand-wired bearer go away.

## User Stories

1. As a learner, I want a consumer sign-in token refused wherever a connection is made or used, so that no path lets me break a vendor's terms.
2. As a learner, I want the same timeout, limits, and error wording whether I use the chat, the tutor, or a Studio run, so that a model behaves the same everywhere.
3. As a learner, I want a connection that worked in chat to work in the tutor, so that I never configure a model twice.
4. As a maintainer, I want one place that knows how to go from a connection to an adapter, so that a new protocol is one change in one crate.
5. As a maintainer, I want the terms check impossible to skip, so that a new call site cannot forget it.
6. As an AI agent working on the codebase, I want one constructor, so that "how do we reach a model" has one place to read.

## Implementation Decisions

- New operation in the AI crate: `connect(connection, credential, limits)` returning an adapter or a typed error. It owns endpoint resolution with the connection's location, credential admission under the terms rules, and limits. The existing protocol enum dispatch stays as the adapter's internal shape.
- The credential is passed in (the AI crate does not read the database); the app's existing executor-based bearer lookup is the one way to obtain it, and the Studio run path uses it instead of its hand-wired copy.
- The Studio provider's complete helper is deleted; its two callers call `connect` then the adapter.
- Saving a signed-in connection applies the same credential admission as saving a keyed one; the sign-in path's own check is deleted.
- The Optional AI checks client is not changed: it deliberately builds a pinned HTTP client for a non-chat protocol in a crate that must stay MIT/Apache (recorded in the chat spec). It keeps sharing endpoint resolution only.
- Error mapping to the chat's string codes and to Studio's error type happens once, in each caller's adapter to its own error vocabulary, from the typed error.

## Testing Decisions

- Test `connect` in the AI crate against a loopback server: resolves and pins; refuses a consumer token for a consumer host; refuses a private address for a remote location; applies limits; returns typed errors for unreachable, timeout, and HTTP status. Assert on what the adapter will send and on the error variant.
- App-side: existing chat streaming tests, sign-in tests, and tutor tests stay green through the new preamble.
- Prior art: the AI crate's endpoint and SSE tests; the terms tests.

## Out of Scope

- Changing any protocol adapter's wire behaviour.
- The Optional AI checks transport.
- Model listing (stays a chat command; a shared lister is a possible follow-on).

## Further Notes

- A trait instead of the enum is not required; the enum dispatch is not the friction, the preamble is.

## Notes

- 2026-10-26: landed. `alexandria_ai::connect(&Connection, Option<&str>, Limits) -> Result<Adapter>` lives in `crates/alexandria-ai/src/connection.rs` with `Connection { endpoint, location, protocol }` and `connection::admit(base, protocol, credential)`, the one credential admission rule (a sign-in token is admitted only over `responses`). `Adapter::new`, the per-protocol constructors, and `with_limits` are crate-private, so `connect` is the only way to an adapter; the per-request `Adapter::admit` went with them, since the credential is admitted at `connect` and every caller builds the adapter per request. Callers: `chat_list_models` and the chat run's `Stream::drive` (bearer, then `connect`, errors through `ai_chat::error_code`); the tutor and `execute_run` through one private `complete` in `commands/studio.rs` (enabled, `validate_connection`, remote-needs-key, `connect`, `provider::request`, `provider::limits`, errors through `provider::map_error` once); `execute_run` now takes the starting session's `ProfileLease` and uses `credential::bearer_for`; the ChatGPT sign-in lists models through `connect`. `alexandria_studio::provider::complete` is deleted; `provider` keeps `location` and `validate_connection` and gains `connection`, `request`, `limits`, `map_error`. `save_signed_in_connection` admits through `connection::admit` like `save_connection`; the OpenRouter save's hand check is deleted. Six seam tests in the AI crate; the three protocol contract test files build their adapter through `connect`; the two Studio provider tests exercise `provider::connection` + `connect`. Not done, as scoped: a shared model lister; the Optional AI checks client is untouched and still uses `endpoint::resolve` + `Endpoint::http_client` directly.
