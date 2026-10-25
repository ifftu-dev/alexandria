# Backend-owned vocabularies generated for the webview

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/architecture-deepening-spec.md

## Problem Statement

Several facts the backend decides are retyped by hand in the webview and have drifted: the vendor list whose terms restrict apps for minors exists in Rust with no caller and again in TypeScript; the fenced block tags a reply may contain are hidden while streaming by a TypeScript list that omits the artifact tag, so a streaming note or goal proposal is briefly visible as raw text; the chat error codes are produced as string prefixes in Rust and matched against three separate hand lists in the webview; the location names differ between Studio and the AI crate and are re-mapped in a component, which also knows a location the backend does not; the proposal kinds are a Rust enum mirrored as a TypeScript union switched on in a dozen places.

## Solution

Make these vocabularies typed Rust values and generate the TypeScript from them through the existing generated-file seam that already carries the command names and is checked in CI. The webview imports the generated file; hand lists are deleted.

## User Stories

1. As a learner, I want a note or goal proposal never to flash as raw fenced text while the answer streams, so that cards feel intentional.
2. As a learner, I want every refusal to show a translated message rather than a raw code, so that a new backend code never leaks.
3. As an adult connecting a vendor whose terms restrict apps minors use, I want the warning to track the backend's list, so that the warning is never stale.
4. As a maintainer, I want to add an error code once, so that the webview cannot fall out of step.
5. As a maintainer, I want the type checker to catch an unhandled proposal kind, so that a new kind cannot render as nothing.
6. As a maintainer, I want CI to fail when the generated file is stale, so that drift is impossible.
7. As an AI agent working on the codebase, I want one generated vocabulary file, so that "what codes can the backend send" has one place to read.

## Implementation Decisions

- Rust owns: chat error codes (an enum with its string form), proposal kinds, fence tags that may appear in a reply, connection locations and protocols, the restricts-apps-for-minors host list. Each is a serialisable enum or constant with a stable string form equal to today's wire strings.
- The existing command-list generator gains a second output: a generated TypeScript module exporting these vocabularies as const arrays and union types. The CI check that compares the generated command list to the handler macro also compares this file.
- The webview replaces: the three error code lists and the string-prefix parsers with one typed parser over the generated enum; the provider terms host list with the generated one; the streaming fence stripper's list with the generated tags; the component-level location re-map with the generated location names; the hand-written proposal body union with the generated kinds (bodies keep their shapes).
- The webview's "external" location, which the backend does not know, is either removed or added to the backend's location enum; decide by whether any saved connection carries it (none should).
- Type-check strictness already in place makes a missing case in a switch over the generated union a build error.
- No change to wire strings; no schema change.

## Testing Decisions

- The generator's own test (prior art: the command-check script's test) covers the new output.
- A webview unit test for the fence stripper runs over every generated tag, including artifact.
- A webview unit test for error code parsing round-trips every generated code to a translation key.
- Existing component tests (connection form, sign-in, offers) stay green.

## Out of Scope

- Full type generation for every command payload (a larger codegen effort).
- Changing any code's meaning or translation.

## Further Notes

- The restricts-apps-for-minors Rust function has no caller today; after this ticket its list is the single source and the function may be used by the connection form through a command, or deleted. Prefer generation plus deletion.

## Notes

- 2026-10-26: landed on `refactor/arch-06-vocab`. `src-tauri/src/ai_chat/vocab.rs` owns the chat error codes, proposal kinds, reply fence tags, connection and chat locations, protocols, and the restricts-apps-for-minors host list, each with its existing wire string; `error_code` and `Body::kind` return those values, `memory::split_block` takes a `ReplyFence`, and `alexandria_ai::Location` gained `as_str`. The `chat_vocab_snapshot_is_current` test writes `scripts/chat-vocab.json` when stale and fails; `scripts/check-tauri-commands.mjs --write` renders it into `src/generated/chat-vocab.ts` and the check mode fails when that is stale, so both guards already run in CI (`cargo test --workspace` and the Tauri command guard step; no workflow edit needed). A source scan in the same test checks every `"code: reason"` literal in chat-owned code against the enum; it found `no_lesson` (course notes saved from chat), which no webview list knew. The webview's three error lists and two prefix parsers are now `src/utils/chatErrors.ts` (one parser, one exhaustive message table, a typed sign-in table); the fence stripper is `src/utils/chatFences.ts` and hides a streaming `artifact` block; the proposal body union, `StudioConnection.location`/`protocol`, and `ChatLocation` derive from the generated kinds; `src/utils/providerTerms.ts` reads the generated host list and `terms::restricts_apps_for_minors` (no caller) was deleted. "external": no saved connection can carry it (`save_connection` validates through `provider::location`), but `alexandria-studio/store.rs` writes it on the synthetic run-step connection of an external-assistant proposal, which the webview renders in the Studio run panel, so it is in the backend vocabulary as `ConnectionLocation::External` with no chat location and the chat filters now ask for a chat location instead of excluding one name. Side effect of the single message table: the chat panel now shows a code's own message where it used to fall back to the generic one (for example `interrupted`, `usage_limit`); wire strings and translations are unchanged.
