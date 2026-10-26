# Commands depend down, never sideways

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/architecture-deepening-spec.md
Blocked by: alexandria/docs/architecture-deepening/01-chat-turn-module.md, alexandria/docs/architecture-deepening/03-ai-access-policy.md

## Problem Statement

Shared behaviour lives wherever its first command did, so domain modules reach into the commands layer: chat actions call course note insertion and course enrolment out of commands files; goal handling calls the plugin cid lookup out of a commands file; Studio commands call the chat commands file for the age rule. Each commands file declares its own database helper and its own error prefix convention. An AI agent or maintainer looking for "how a learner enrols" finds it under a command, not under enrolment.

## Solution

Commands depend on domain modules; domain modules never depend on commands. Behaviour that other modules need moves under the module named for it. One shared database helper and one error convention serve every commands file. A cheap check keeps the direction from regressing.

## User Stories

1. As a maintainer, I want enrolment, course notes, and plugin lookups callable without a command, so that chat actions and future callers use one implementation.
2. As a maintainer, I want one database helper for commands, so that lease, rate limit, and error handling are identical everywhere.
3. As a maintainer, I want a check that domain modules do not import the commands layer, so that the direction cannot silently regress.
4. As an AI agent working on the codebase, I want the module named for a concept to own it, so that I never have to search the commands layer for domain behaviour.
5. As a maintainer, I want the handler registration list to stop being the file every feature edits, so that merge conflicts in app wiring are rare.

## Implementation Decisions

- Move out of commands files into their domain modules: course note insertion (into a course notes module), course enrolment (into enrolment), plugin cid lookup for a course (into plugins), and anything tickets 01 and 03 leave behind. Commands become wrappers over them.
- One shared commands database helper (profile lease, activity touch, rate limit label, executor run, error mapping) replaces the per-file copies; per-file copies are deleted.
- One error convention for command results: a typed error with a stable string code, mapped once in the shared helper. Existing string codes are preserved.
- A dependency direction check: a test or script that fails if any non-commands module imports the commands layer. Lightweight (grep-level) is acceptable.
- Handler registration: investigate whether each commands module can expose its own handler list that the app's registration composes; adopt only if the Tauri macro allows it without losing the generated command list check. If not, record why in this ticket's notes and leave registration as is.
- No schema change; no command renames.

## Testing Decisions

- The moved behaviour keeps its existing tests, now under the domain module (prior art: chat action tests that enrol and insert course notes).
- The dependency direction check runs in CI alongside the commit message and command list checks.
- No new behaviour tests; this ticket is structure.

## Out of Scope

- Splitting every large commands file; only the sideways dependencies and the shared helper.
- Changing command payloads or names.

## Further Notes

- Speculative in the review; most of its value falls out of tickets 01 and 03. Do the remainder only if the direction check finds more than a handful of sideways imports after those land.

## Notes

- 2026-10-27: landed on `refactor/arch-07-commands` (cut from `refactor/arch-integration`, which carries 01, 03, 04, 05 and 06). **What the check found.** After 01 and 03, eight files outside `src-tauri/src/commands` still imported the commands layer (not counting `lib.rs`): chat actions (course note insert, plugin CIDs, enrolment), the turn and run tests (attach, attach goal, save note), the access policy test (sign-in target), the scoring and opinion eligibility readers in `db` (stored completion evidence, integrity hash), the scoring adversarial tests (derived skill state), and the profile lifecycle tests (Studio runtime, rate limiter). That is more than a handful, so the remainder was done. **Moves** (commands become wrappers; tests moved with the code unchanged except for paths): the gated chat bodies 03 left in `commands/ai_chat.rs` → `ai_chat::ops` with their three tests (`commands/ai_chat.rs` went from 1,093 to 573 lines); course notes → `crate::course_notes` with their tests; enrol and list → `crate::enrollment` with the four enrolment tests (the raw-SQL schema tests stay with the commands); `course_plugin_cids` → `plugins::registry`; `sign_in_target` → `ai_auth`; the completion endorsement store (claim context, import, list, status, stored evidence) → `evidence::completion`, with the import tests and a shared `fixtures::fixture`, plus `endorsement_request` so the command no longer reads the private claim context; `integrity_hash_of` → `domain::vc` (it had two identical copies, in `commands/credentials.rs` and `commands/auto_issuance.rs`); the derived skill state store → `aggregation::store`. The profile lifecycle tests build the Studio runtime and rate limiter with `Default` instead of naming their types. **Shared helper.** The three identical per-file `with_conn` copies (chat, sign-in, course notes) are one `commands::with_conn`. **Guard.** `scripts/check-tauri-commands.mjs` fails when a Rust file outside `src-tauri/src/commands` names the commands layer (`commands::`, `crate::commands`, or a grouped `crate::{.. commands ..}`), with comments blanked first. `lib.rs` is exempt as the composition root. It runs in the existing Tauri command guard CI step, and its tests cover a direct, grouped and nested import and a comment. **Deviations.** (1) The shared helper does not touch activity or apply a rate limit: only `export_mnemonic` touches activity and only three commands are rate limited, so doing it in every command would change behaviour. (2) No crate-wide typed command error. The chat, sign-in and course note commands already write their codes through the typed `ChatErrorCode` with `refuse()`/`refused()` (ticket 06), and the helper passes their strings through unchanged. The other command files return free-form strings, and converting about seventy files is the "split every commands file" work this ticket rules out. (3) The derived skill state store's tests stay in `commands/aggregation.rs`. Their fixture issues credentials through `commands::credentials::issue_credential_impl`, and moving issuance down is a larger follow-up. (4) Studio's `with_db` (a synchronous lock plus Studio's blocked flag and error type, used by the Studio, decisions and assistant broker commands) is not a copy of `with_conn` and is left as is. (5) Imports between commands files (credential issuance, `now_rfc3339`, `load_issuer_key`, holder pull, talent index) are out of scope; about two hundred remain. **Handler registration** is left as is. `Builder::invoke_handler` holds one boxed handler and a later call replaces it. A `generate_handler!` handler takes the `Invoke` by value, so per-module handlers cannot be tried in turn. Splicing per-module lists through `macro_rules!` callbacks would move the list out of the `generate_handler!` block that the command guard parses for the registered set, the profile-scope check and the generated command names. **Not touched:** tutoring and classroom (ticket 02 is rewriting them); none of their non-command files imported the commands layer. No schema change, no command renamed, and the webview and command list are unchanged.
