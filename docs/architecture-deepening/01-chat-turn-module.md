# Collapse the chat turn into one module

Triage: done
Repos: alexandria
Part of: alexandria/docs/architecture-deepening-spec.md

## Problem Statement

When a learner sends a message, what gets gathered, what gets reviewed, what is actually sent, and what the model's reply turns into are decided across one very large commands file and several helper modules that only that file calls, in an order only that file enforces. Changing the token budget, the review rule, or which fenced block is parsed first means editing the command and two or three helpers together. The behaviour is well tested, but every test has to go through the command's private functions.

## Solution

A single deep module, the **chat turn**, owns a message from gather to outcome. Its interface is three operations: prepare a turn, begin a prepared turn, and complete a begun turn with the model's reply. Commands become thin wrappers. The helpers that only the turn uses become internal to it.

## User Stories

1. As a learner, I want what I reviewed to be exactly what is sent, so that the review sheet never lies to me.
2. As a learner, I want a stale prepared message refused when anything I attached changed, so that I never send text I did not see.
3. As a learner, I want a repeated send not to be sent twice, so that a double tap costs nothing.
4. As a learner, I want material too large for the model refused before sending, so that I choose what to trim rather than get an error.
5. As a learner, I want an excluded attachment left out and a fresh review required, so that exclusions are honoured.
6. As a learner, I want a retry to answer the same turn without its old answer in history, so that the model does not argue with itself.
7. As a learner, I want an attached goal to send its current learning path, so that the model sees my real progress.
8. As a learner, I want a reply's practice question, memory suggestion, note or goal proposal split off and shown as a card, so that fenced blocks never appear as raw text.
9. As a learner, I want a generated question that resembles an assessment item dropped, so that chat never leaks the item bank.
10. As a learner, I want nothing prepared, attached, or sent during an assessment attempt, so that integrity holds.
11. As a maintainer, I want the budget rule in one place, so that a change to token accounting is one edit.
12. As a maintainer, I want the turn testable without Tauri, so that behaviour tests run on a database alone.
13. As an AI agent working on the codebase, I want one module named for the concept, so that "what is sent to the model" has one place to read.
14. As a maintainer, I want the commands file to shrink to wrappers, so that command code is only argument parsing, lease, and one call.

## Implementation Decisions

- New module **chat turn** inside the AI chat backend module. Interface:
  - `prepare` takes a database connection and the learner's inputs (thread, text, exclusions, requested connection and model, mode) plus the fetched lesson text, and returns the prepared message: review items, manifest, fit, whether review is required, and a prepare id.
  - `begin` takes a connection, the prepare id and the digest the learner reviewed, and returns the started turn: the request to send, the run row, and the destination. It refuses stale, missing, duplicate, and mismatched prepares with the same reasons as today.
  - `complete` takes a connection, the started turn and the model's full reply plus usage, and returns the outcome: final content, any note, practice question, memory suggestion, draft proposal, and what was refused.
- Gather, excerpt rendering, assembly, destination choice, begin, draft resolution, and finish-completion move into the turn module unchanged in behaviour. Review items and token accounting (including the fixed per-item allowances) move out of the command into the context helper, which becomes internal to the turn.
- The turn module does no I/O beyond the database connection it is handed. Fetching lesson blobs stays in the command before `prepare`; model streaming stays in the run (ticket 05).
- The attempt gate and age gate are called by the turn at the same points as today; once ticket 03 lands, the turn calls the policy module instead.
- Error reasons keep their current string codes so the webview is unchanged.
- No schema change. Prepare TTL, policy version, and byte limits keep their values.
- The command tests' helpers (database, seed, connection, thread, prepare, start, complete) move into the turn module's tests.

## Testing Decisions

- Test through `prepare`, `begin`, and `complete` on a migrated SQLite connection with a seeded learner. Assert on the prepared manifest, the request digest, the stored rows, and the outcome, never on which internal helper ran.
- All existing behaviour tests (what is sent is what was reviewed; stale after change; repeated send not sent twice; nothing during an assessment; destination change needs review; too large refused; retry history; excluded attachment; practice, memory, artifact, goal, course note and enrolment proposals) move and stay green.
- Streaming tests stay in the command until ticket 05.
- Prior art: the existing chat command tests and their database harness.

## Out of Scope

- The streaming run and its event sink (ticket 05).
- Moving age, guardian, and gate rules (ticket 03).
- Any change to what the model is told or to policy version.

## Further Notes

- This is a pure move. If a test needs rewriting, stop and reconsider the seam.
- Expected size: the commands file loses roughly a thousand lines; the turn module gains them.

## Notes

- 2026-10-26: landed on `refactor/arch-01-chat-turn`. New module `src-tauri/src/ai_chat/turn.rs` owns the turn behind `gather` → `prepare` → `begin` → `complete`; `commands/ai_chat.rs` went from 3,461 to about 2,200 lines and its chat commands are wrappers. Deviations from the text above, all forced by keeping behaviour identical: (1) `prepare` is two calls, `gather(conn, Inputs) -> Gathered` and `prepare(conn, Gathered, &[Fetched]) -> PreparedMessage`, because the lesson bodies to fetch are only known after gathering and fetching must happen outside the database lock, as before; `Gathered::blobs_to_fetch()` tells the command what to fetch. (2) `begin` also takes the client request id (duplicate detection needs it) and the `ChatRuntime` (the capacity check happens at the same point as before). (3) `complete` writes the answer and its cards in one transaction and returns the `Outcome` (content, note, practice question, memory suggestion, proposal, what was refused and why, whether anything was saved); the streaming run only hops to the database executor to call it. Review items and the fixed +4/+8/+16 allowances moved into `context::Review`/`Fitted`. The age gate and `refuse_during_assessment` stay in the commands file and are imported by the turn under a `TODO(ticket 03)`. The command tests' database helpers are `turn::fixtures`, shared by both test modules; the fifteen non-streaming behaviour tests moved into the turn's tests unchanged apart from calling the new functions, the streaming tests (including the proposal, practice and memory-suggestion ones, which drive a fake model) stayed in the commands file, and one new test pins `complete` directly. No schema, policy version, error code, TTL or limit changed; the webview and the command list are untouched.
- 2026-10-27: closed (done). Merged into `refactor/arch-integration` with the other six tickets. Correction to the note above: the streaming tests did not stay in the commands file. Ticket 05 moved them into `ai_chat/run.rs` (scripted source, vector sink), and the proposal, practice and memory-suggestion tests into `ai_chat/turn.rs`, where they call `complete` with a literal reply. `commands/ai_chat.rs` has no tests of its own now. Ticket 03 replaced the `TODO(ticket 03)` imports: the turn calls `crate::ai_access` for the age rules and the attempt gate. Also landed with this ticket: one stored timestamp format (`ai_chat::timestamp`), and `complete` finishes the run even when a proposal cannot be looked up (the proposal is dropped and recorded as refused).
