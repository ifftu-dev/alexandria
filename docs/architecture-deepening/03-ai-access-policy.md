# AI access policy: age band, guardian approval, attempt gate

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/architecture-deepening-spec.md

## Problem Statement

Who may talk to which model is one coherent rule set (learners under 13 use models on their device or own network only; 13 to 17 need a guardian's approval per remote endpoint and never an adults-only vendor; nothing during an assessment attempt; only text lessons visible to a model). Today it lives inside the chat commands file, is copied into the Studio commands file with the error re-parsed from a string, is derived again for provider sign-in, and is bypassed by the tutor's own lesson lookup. The chat module's own doc says every entry point refuses during an attempt, but ten of its thirty-three commands do not. Guardian approvals are four hand-written SQL touchpoints across three files.

## Solution

One **AI access** policy module answers three questions with typed reasons: may this learner use this connection right now; is an attempt in progress for this surface (recording the integrity flag when it refuses); which lesson text may a model see. Chat, the lesson tutor, Studio runs, provider sign-in, chat actions, and course notes all ask it. Guardian approvals become part of its implementation.

## User Stories

1. As a learner under 13, I want only on-device and own-network models offered, so that my data never reaches a provider.
2. As a learner aged 13 to 17, I want to ask my guardian from the chat and see the answer bound to that connection and endpoint, so that approval means what it says.
3. As a learner aged 13 to 17, I want adults-only vendors refused everywhere (chat, tutor, Studio, sign-in), so that no surface lets me break a vendor's terms.
4. As a guardian, I want to see the connection's name, endpoint, and model but never chat content, so that approval does not mean surveillance.
5. As a guardian, I want a changed endpoint to need a new approval, so that my decision cannot be stretched.
6. As a learner, I want every AI surface to refuse during an assessment attempt and record that it did, so that credentials stay honest.
7. As a learner, I want the tutor and the chat to agree on which lesson text a model may see, so that a withheld lesson is withheld everywhere.
8. As a learner, I want the same refusal reason in the chat, the tutor, and sign-in, so that I understand what to do next.
9. As a learner whose birthdate was corrected, I want the correction to apply at send time, not only at prepare time, so that the rule follows the facts.
10. As a maintainer, I want minors' rules in one module, so that a vendor terms change is one edit with one test file.
11. As a maintainer, I want typed refusal reasons, so that no caller re-parses an error string.
12. As an AI agent working on the codebase, I want one policy module, so that "which connections may a minor use" has one place to read.

## Implementation Decisions

- New **AI access** module. Where it lives: inside the app's AI chat backend module is acceptable, but the Studio crate is preferred because the tutor and workflow runs already depend on that crate and the chat already depends on its gate and learning rules. Guardian approval storage stays in the app database either way; the module takes a connection.
- Interface:
  - `may_use(connection, learner, studio connection)` returns allowed, or refused with a typed reason: too young for remote, vendor adults only, guardian approval pending, guardian declined, guardian not linked, connection disabled, wrong capability.
  - `no_attempt_in_progress(connection, surface)` returns ok or refused; on refusal it records the integrity flag for that surface (chat and tutor record the in-app assistant flag; Studio run steps are refused but not flagged, as recorded in the chat spec).
  - `visible_lesson(connection, course, element)` returns the lesson text target or refused, using the learning rules that chat and the broker already share.
  - Guardian approvals: request, latest decision for a connection at its endpoint, pending requests for sync, and record a guardian's decision. The sync path and the guardian's overview page call these instead of their own SQL.
- Age band is computed from the learner's local identity birthdate as today; the adults-only and restricts-apps-for-minors vendor tables stay in the AI crate's terms module and are consulted by this module only.
- Chat applies `may_use` at prepare and again at begin (today only connection revision, endpoint, and location are re-checked at begin).
- Every chat command that reads or writes thread state applies the attempt gate, matching the module doc. Pure reads of status and lists stay ungated.
- Typed reasons serialise to the same string codes the webview already translates, so the webview is unchanged.
- The Studio tutor's lesson lookup goes through `visible_lesson`; the hand-written text-kind check is deleted.
- Provider sign-in's own age check is deleted in favour of `may_use` with a connection built from the sign-in target.
- No schema change.

## Testing Decisions

- Test through the three interface operations on a migrated SQLite connection with seeded learners of different ages and linked guardians. Assert on allowed or refused and the typed reason, on the integrity flag row written, and on the lesson target returned.
- The existing chat tests for under-13 learners, 18-plus vendors, consumer sign-ins never stored, and guardian approval at that endpoint move here. Add cases for: sign-in follows the same rules; the tutor refuses a non-text lesson through the shared rule; a begin after a guardian revocation is refused; every thread-mutating chat command is refused during an attempt.
- Prior art: the chat command age and guardian tests; the Studio crate's gate and learning tests.

## Out of Scope

- Changing any age threshold, vendor list, or approval semantics.
- The guardian link transport itself.
- Vendor terms shown to adults in the connection form (ticket 06 covers the vocabulary).

## Further Notes

- Closing the ten ungated commands is a behaviour change in the strict sense, but it brings the code in line with the documented rule; note it in the chat spec's implementation record when done.
- Chat actions and course notes each check the gate and the item-bank guard twice today in one transaction; with one policy call per operation the duplicate scans disappear.

## Notes

- 2026-10-26: landed on `refactor/arch-03-access` (on top of 01). **Where it lives.** The module is `src-tauri/src/ai_access.rs` (app crate): `may_use(conn, connection)`, `may_connect(conn, connection)`, `no_attempt_in_progress(conn, surface)`, and guardian approvals (`request_approval`, `approvals`, `pending_approvals` for the sync, `record_decision` for a guardian's decision). It lives in the app, not the Studio crate, because everything it reads or writes besides the lesson rule is app-side: the integrity flag goes through Sentinel's committed snapshot chain, the age band uses `domain::identity` birthdate rules, and the guardian link and sync are the app's. In the Studio crate, the flag writer would have to be injected through every call, and the tests could not assert the flag row or reuse `turn::fixtures`. The typed `Refused` and `visible_lesson` live in `alexandria_studio::access` and are re-exported by `ai_access`, because `store::tutor_context` (Studio crate) must use the lesson rule and the Studio error mapping (`From<Refused> for Error`) belongs beside `Error`. `Display` gives the webview's string codes, unchanged. The snapshot chain and `record_backend_flag` moved from `commands/integrity.rs` to `sentinel::snapshots`, so nothing outside the commands layer imports it. **Deviations from the text above.** (1) `may_use` takes no learner argument: the learner is always the profile's holder, read from the same connection at each call, so a corrected birthdate applies at the next call and no caller can pass a stale one. (2) `may_connect` is the part no approval can lift (under 13, adults-only vendor). Provider sign-in and requesting an approval use it: `may_use` on a sign-in target refuses a 13-to-17 learner with `guardian_required` before the connection exists, but today they may sign in to ChatGPT and then ask. Sign-in checks `sign_in_target(api_base)`, the base the saved connection is built from. (3) `visible_lesson` takes a `Reader`. Chat reads published courses by others, as the broker does. The tutor reads any course the learner takes or wrote, whatever its status, as before. Unifying them would stop authors and draft courses getting the tutor (and would fail the Studio tutor test, whose course is a draft). The text-only rule is shared. **Per-command attempt gate** (refused and flagged = `Surface::Chat`: only the open panel sends it; refused, not flagged = `Surface::ChatRead`: the app may send it by itself as an attempt starts, e.g. reloading the open thread when the attempt cancels its run). Refused and flagged: create thread, rename, set model, set mode, delete thread, attach lesson, attach goal, detach, list models (contacts the provider), request guardian approval, prepare, start, practice attempt/hint/reveal, add/approve/update/forget memory, save note, allow action, decline action, delete artifact. Refused, not flagged: get thread, search threads, list saved artifacts (all return model-written text). Open: status, list threads, list guardian approvals, list memory (the learner's own facts), get/save draft (the learner's unsent text; saving is debounced and can fire after the panel unmounts), cancel run (stopping must always work). Course notes (list, save from chat) stay refused and not flagged; Studio run steps are refused and not flagged; the tutor is refused and flagged. **Behaviour changes.** The commands newly refused during an attempt are rename, set model, set mode, delete thread, detach, list models, request approval, the four memory writes, save note, decline action, delete artifact, get thread, search, and list artifacts. Allowing a chat proposal during an attempt is now flagged as well as refused. `begin` applies `may_use` again, so a withdrawn approval or a corrected birthdate refuses a send prepared before it. A Studio run step checks "changed or disabled" before the age rules, so a disabled connection reports that first. Approval storage failures read `storage_error: approvals could not be read` instead of the raw SQLite text. Course notes `insert` no longer re-runs the gate and item-bank guard that both callers already applied; `save_from_chat` now runs the guard itself. The chat command bodies are now named functions; one table-driven test (`commands::ai_chat::tests::every_chat_command_that_touches_a_thread_is_refused_during_an_attempt`) covers every gated command. No schema change, and the webview and command list are unchanged.
